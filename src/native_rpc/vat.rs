use super::{AuthenticatedSession, Connector, Handle, Network, Options, VatId};
use crate::rpc::{task::Task, Completion};
use capnp::capability::{Client, FromClientHook, Promise};
use capnp_rpc::{BootstrapFactory, Bootstrapper, Joiner, RpcSystem};
use std::{future::Future, rc::Rc, time::Duration};

/// Each peer's acknowledged byte-stream drain or failure. Successful receipts
/// do not acknowledge application execution or persistence.
pub type ShutdownReport = Vec<(VatId, capnp::Result<crate::native_shutdown::Receipt>)>;

enum Bootstrap {
    Static(Option<Client>),
    Factory(Rc<dyn BootstrapFactory<VatId>>),
}

/// Configure a native RPC vat before starting its owned local task.
///
/// No bootstrap is exposed and no outbound connections are allowed by default.
/// Attach authenticated sessions explicitly, or supply an application connector
/// for on-demand connections, including routes created by three-party handoff.
#[must_use = "call start() to run the configured vat"]
pub struct VatBuilder {
    local: VatId,
    connector: Option<Rc<dyn Connector>>,
    options: Options,
    bootstrap: Bootstrap,
    flow_limit: usize,
    outgoing_call_limit: usize,
}

impl VatBuilder {
    /// Expose a typed capability to every authenticated peer. Use a factory when
    /// peers should receive different authority. The last bootstrap setter wins.
    pub fn bootstrap<T: FromClientHook>(mut self, client: T) -> Self {
        self.bootstrap = Bootstrap::Static(Some(Client::new(client.into_client_hook())));
        self
    }

    pub fn bootstrap_factory(mut self, factory: Rc<dyn BootstrapFactory<VatId>>) -> Self {
        self.bootstrap = Bootstrap::Factory(factory);
        self
    }

    /// Select authority for each authenticated peer without a factory struct.
    /// Errors reject that Bootstrap request; existing capabilities are unchanged.
    pub fn bootstrap_with(
        self,
        factory: impl Fn(&VatId) -> capnp::Result<Client> + 'static,
    ) -> Self {
        self.bootstrap_factory(Rc::new(FactoryFn(factory)))
    }

    /// Use a directory, discovery, provisioning or application connector.
    pub fn connector(mut self, connector: Rc<dyn Connector>) -> Self {
        self.connector = Some(connector);
        self
    }

    /// Dial with an async closure. The network still validates session identity,
    /// bounds setup time and cancels abandoned dials. The closure must choose
    /// routes from application policy, not treat an introduction as authority.
    pub fn connect_with<F, Fut>(self, connect: F) -> Self
    where
        F: Fn(VatId) -> Fut + 'static,
        Fut: Future<Output = capnp::Result<AuthenticatedSession>> + 'static,
    {
        self.connector(Rc::new(ConnectorFn(connect)))
    }

    pub fn options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }

    /// Incoming call backpressure per connection, in words. See
    /// [`RpcSystem::set_flow_limit`] for deadlock and memory-bound caveats.
    pub fn flow_limit(mut self, words: usize) -> Self {
        self.flow_limit = words;
        self
    }

    /// Outgoing Calls per connection, including introduced connections. See
    /// [`RpcSystem::set_outgoing_call_limit`] for credit lifetimes and scope.
    pub fn outgoing_call_limit(mut self, calls: usize) -> Self {
        self.outgoing_call_limit = calls;
        self
    }

    /// Validate configuration and start RPC. Panics outside a Tokio local
    /// executor, like `spawn_local`. No network dial occurs until requested.
    pub fn start(self) -> capnp::Result<Vat> {
        let (network, handle) = Network::with_options(self.local, self.connector, self.options)?;
        let mut system = match self.bootstrap {
            Bootstrap::Static(bootstrap) => RpcSystem::new(Box::new(network), bootstrap),
            Bootstrap::Factory(factory) => {
                RpcSystem::new_with_bootstrap_factory(Box::new(network), self.local, factory)
            }
        };
        system.set_flow_limit(self.flow_limit);
        system.set_outgoing_call_limit(self.outgoing_call_limit);
        Ok(Vat {
            bootstrapper: system.get_bootstrapper(),
            joiner: system.get_joiner(),
            diagnostics: system.diagnostics(),
            handle,
            task: Task::spawn(system),
        })
    }
}

struct FactoryFn<F>(F);
impl<F: Fn(&VatId) -> capnp::Result<Client>> BootstrapFactory<VatId> for FactoryFn<F> {
    fn create_for(&self, peer: &VatId) -> capnp::Result<Client> {
        (self.0)(peer)
    }
}
struct ConnectorFn<F>(F);
impl<F, Fut> Connector for ConnectorFn<F>
where
    F: Fn(VatId) -> Fut,
    Fut: Future<Output = capnp::Result<AuthenticatedSession>> + 'static,
{
    fn connect(&self, peer: VatId) -> Promise<AuthenticatedSession, capnp::Error> {
        Promise::from_future((self.0)(peer))
    }
}

/// Owns a running multiparty RPC system, independent of application scheduling.
///
/// Keep this owner alive while using its capabilities. Drop initiates task
/// cancellation; [`Self::on_disconnect`] observes RPC driver completion. Native
/// session workers are canceled when the driver drops. Cloned
/// bootstrap and join handles do not keep the system alive. Ordinary generated
/// capability parameters/results automatically use native three-party handoff.
#[must_use = "keep the vat owner alive while using its remote capabilities"]
pub struct Vat {
    diagnostics: capnp_rpc::RpcDiagnostics,
    bootstrapper: Bootstrapper<VatId>,
    joiner: Joiner<VatId>,
    handle: Handle,
    task: Task,
}

impl Vat {
    pub fn builder(local: VatId) -> VatBuilder {
        VatBuilder {
            local,
            connector: None,
            options: Options::default(),
            bootstrap: Bootstrap::Static(None),
            flow_limit: usize::MAX,
            outgoing_call_limit: usize::MAX,
        }
    }

    pub fn identity(&self) -> VatId {
        self.handle.identity()
    }

    /// Access route diagnostics, datagrams, mobility and individual shutdown.
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    pub fn diagnostics(&self) -> capnp_rpc::RpcDiagnostics {
        self.diagnostics.clone()
    }

    pub fn attach(&self, session: AuthenticatedSession) -> capnp::Result<()> {
        self.handle.attach(session)
    }

    /// Obtain a typed capability at any time after startup, with pipelining.
    /// Connection errors become failed calls, with no automatic call replay.
    pub fn bootstrap<T: FromClientHook>(&self, peer: VatId) -> T {
        self.bootstrapper.bootstrap(peer)
    }

    pub fn bootstrapper(&self) -> Bootstrapper<VatId> {
        self.bootstrapper.clone()
    }

    /// Capability equality, including independently routed multiparty joins.
    pub fn joiner(&self) -> Joiner<VatId> {
        self.joiner.clone()
    }

    pub fn on_disconnect(&self) -> Completion {
        self.task.completion()
    }

    /// Stop admission, drain every current route concurrently, then stop RPC.
    /// Timeout must be in (0, 60s]. Each peer's result is returned separately.
    /// Stop application work first: transport drain does not finish active calls.
    /// Canceling even an unpolled shutdown drops this owner and aborts its driver.
    pub async fn shutdown(mut self, timeout: Duration) -> capnp::Result<ShutdownReport> {
        let report = self.handle.shutdown_all(timeout).await;
        let completion = self
            .task
            .finish(report.as_ref().map(|_| ()).map_err(Clone::clone))
            .await;
        let report = report?;
        completion?;
        Ok(report)
    }
}

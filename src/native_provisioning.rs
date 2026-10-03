//! Capability-authorized provisioning of fresh shared-listener Native routes.
//! The provisioning capability must use an existing confidential control route
//! independent of the native route being established. Run on a Tokio LocalSet.
use crate::{
    native_listener::{self, Listener, Target, MAX_CONTEXT_BYTES},
    native_provisioning_capnp::{lease, provisioner, ticket},
    native_rpc::{Connector, Handle},
    transport::{AuthenticatedSession, Identity},
};
use capnp::{capability::Promise, Error};
use ring::rand::SecureRandom;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    net::SocketAddr,
    rc::{Rc, Weak},
    time::Duration,
};
use tokio::sync::Notify;
use zeroize::Zeroizing;

fn failed(error: impl std::fmt::Display) -> Error {
    Error::failed(error.to_string())
}
fn canceled() -> Error {
    Error::disconnected("Native provisioning lease retired".into())
}
fn address_ok(address: SocketAddr) -> bool {
    crate::nat::address_ok(address)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Pending,
    Installing,
    Installed,
    Retired,
}
struct LeaseState {
    phase: Cell<Phase>,
    result: RefCell<Option<capnp::Result<()>>>,
    task: RefCell<Option<tokio::task::AbortHandle>>,
    waiters: Cell<usize>,
    changed: Notify,
}
impl LeaseState {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            phase: Cell::new(Phase::Pending),
            result: RefCell::new(None),
            task: RefCell::new(None),
            waiters: Cell::new(0),
            changed: Notify::new(),
        })
    }
    fn active(&self) -> bool {
        matches!(self.phase.get(), Phase::Pending | Phase::Installing)
    }
    // Claim the authenticated session before invoking attach(), which can wake
    // application code. Cancellation after this point cannot undo installation.
    fn begin_install(&self) -> bool {
        if self.phase.get() != Phase::Pending {
            return false;
        }
        self.phase.set(Phase::Installing);
        self.task.borrow_mut().take();
        true
    }
    fn installed(&self, result: capnp::Result<()>) {
        assert_eq!(self.phase.get(), Phase::Installing);
        self.finish(result);
    }
    fn cancel(&self, error: Error) {
        if self.phase.get() == Phase::Pending {
            self.finish(Err(error));
            let task = self.task.borrow_mut().take();
            if let Some(task) = task {
                task.abort();
            }
        }
    }
    fn finish(&self, result: capnp::Result<()>) {
        self.phase.set(if result.is_ok() {
            Phase::Installed
        } else {
            Phase::Retired
        });
        *self.result.borrow_mut() = Some(result);
        self.changed.notify_waiters();
    }
    async fn ready(&self) -> capnp::Result<()> {
        if self.waiters.get() >= 64 {
            return Err(Error::overloaded("Native lease waiter limit".into()));
        }
        self.waiters.set(self.waiters.get() + 1);
        struct Waiter<'a>(&'a Cell<usize>);
        impl Drop for Waiter<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }
        let _waiter = Waiter(&self.waiters);
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(result) = self.result.borrow().clone() {
                return result;
            }
            changed.await;
        }
    }
}
impl Drop for LeaseState {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
    }
}
#[derive(Default)]
struct Registry {
    closed: Cell<bool>,
    leases: RefCell<Vec<Weak<LeaseState>>>,
}
impl Registry {
    fn allocate(&self) -> capnp::Result<Rc<LeaseState>> {
        if self.closed.get() {
            return Err(canceled());
        }
        let mut leases = self.leases.borrow_mut();
        leases.retain(|lease| lease.strong_count() != 0);
        // One in-flight reservation per delegated provider, plus the listener's
        // global active-route and lifetime issuance limits.
        if leases.iter().filter_map(Weak::upgrade).any(|s| s.active()) {
            return Err(Error::overloaded("Native provision already pending".into()));
        }
        let state = LeaseState::new();
        leases.push(Rc::downgrade(&state));
        Ok(state)
    }
    fn close(&self) {
        self.closed.set(true);
        let leases = self.leases.borrow().clone();
        for lease in leases.into_iter().filter_map(|s| s.upgrade()) {
            lease.cancel(canceled());
        }
    }
}
struct Service {
    listener: Listener,
    network: Handle,
    recipient: [u8; 32],
    address: SocketAddr,
    context: Vec<u8>,
    registry: Registry,
    rendezvous: bool,
}
impl Drop for Service {
    fn drop(&mut self) {
        self.registry.close();
    }
}
/// Local owner of a delegated provisioning authority. Clones and issued
/// provisioner capabilities keep it alive. close() or dropping the final owner
/// and provisioner capability cancels pending leases, preserving installed RPC.
#[derive(Clone)]
pub struct Provisioner(Rc<Service>);
impl Provisioner {
    pub fn new(
        listener: Listener,
        network: Handle,
        recipient: [u8; 32],
        advertised: SocketAddr,
        context: &[u8],
    ) -> capnp::Result<Self> {
        if listener.identity() != network.identity()
            || recipient == [0; 32]
            || recipient == listener.identity()
            || !address_ok(advertised)
            || context.len() > MAX_CONTEXT_BYTES
        {
            return Err(failed("invalid Native provisioning authority"));
        }
        Ok(Self(Rc::new(Service {
            listener,
            network,
            recipient,
            address: advertised,
            context: context.to_vec(),
            registry: Registry::default(),
            rendezvous: false,
        })))
    }
    /// Delegate bounded UDP punching as well as reservation authority. Export
    /// only to the named recipient over a confidential control connection.
    pub fn with_rendezvous(
        listener: Listener,
        network: Handle,
        recipient: [u8; 32],
        advertised: SocketAddr,
        context: &[u8],
    ) -> capnp::Result<Self> {
        let mut provider = Self::new(listener, network, recipient, advertised, context)?;
        Rc::get_mut(&mut provider.0).unwrap().rendezvous = true;
        Ok(provider)
    }
    pub fn client(&self) -> provisioner::Client {
        capnp_rpc::new_client(self.clone())
    }
    pub fn close(&self) {
        self.0.registry.close();
    }
}
struct Lease(Rc<LeaseState>);
impl lease::Server for Lease {
    async fn ready(
        self: Rc<Self>,
        _: lease::ReadyParams,
        _: lease::ReadyResults,
    ) -> capnp::Result<()> {
        self.0.ready().await
    }
    async fn cancel(
        self: Rc<Self>,
        _: lease::CancelParams,
        _: lease::CancelResults,
    ) -> capnp::Result<()> {
        self.0.cancel(canceled());
        Ok(())
    }
}
impl provisioner::Server for Provisioner {
    async fn reserve(
        self: Rc<Self>,
        params: provisioner::ReserveParams,
        mut results: provisioner::ReserveResults,
    ) -> capnp::Result<()> {
        let service = &self.0;
        if !service.network.accepts_provision(service.recipient) {
            return Err(failed("Native recipient already has a live route"));
        }
        let address = params.get()?.get_rendezvous_address()?;
        let rendezvous = if address.is_empty() {
            None
        } else {
            if !service.rendezvous || address.len() > 128 {
                return Err(failed("Native rendezvous is not authorized"));
            }
            let address = address.to_str()?.parse::<SocketAddr>().map_err(failed)?;
            if !address_ok(address) {
                return Err(failed("invalid Native rendezvous address"));
            }
            Some(address)
        };
        let state = service.registry.allocate()?;
        let mut psk = Zeroizing::new([0u8; 32]);
        ring::rand::SystemRandom::new()
            .fill(&mut *psk)
            .map_err(failed)?;
        let reservation = service
            .listener
            .reserve(service.recipient, Some(*psk), &service.context)
            .map_err(failed)?;
        let mut out = results.get();
        {
            let mut ticket = out.reborrow().init_ticket();
            ticket.set_host(&service.listener.identity());
            ticket.set_recipient(&service.recipient);
            ticket.set_address(service.address.to_string());
            ticket.set_connection_id(&reservation.target().connection_id);
            ticket.set_psk(&*psk);
            ticket.set_context(&service.context);
        }
        out.set_lease(capnp_rpc::new_client(Lease(state.clone())));
        let weak = Rc::downgrade(&state);
        let network = service.network.clone();
        let listener = service.listener.clone();
        let task = tokio::task::spawn_local(async move {
            let accept = reservation.accept();
            tokio::pin!(accept);
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let accepted = loop {
                tokio::select! {
                    biased;
                    result = &mut accept => break result,
                    _ = interval.tick(), if rendezvous.is_some() => {
                        if let Err(error) = listener.punch(rendezvous.unwrap()).await { break Err(error); }
                    }
                }
            };
            if let Some(state) = weak.upgrade() {
                match accepted {
                    Ok(session) if state.begin_install() => {
                        state.installed(network.attach(session));
                    }
                    Ok(_) => {}
                    Err(error) => state.cancel(failed(error)),
                }
            }
        });
        *state.task.borrow_mut() = Some(task.abort_handle());
        Ok(())
    }
}

/// Keep provisioning calls at an introducer instead of shortening the provider
/// or its returned lease to a host whose route is still being established.
/// Export this local forwarding capability on the recipient's existing control
/// connection. Both control hops must be confidential and authorized.
pub fn relay(provider: provisioner::Client) -> provisioner::Client {
    capnp_rpc::new_client(Relay(provider))
}
struct Relay(provisioner::Client);
struct RelayLease(lease::Client);
impl provisioner::Server for Relay {
    async fn reserve(
        self: Rc<Self>,
        params: provisioner::ReserveParams,
        mut results: provisioner::ReserveResults,
    ) -> capnp::Result<()> {
        let mut request = self.0.reserve_request();
        let address = params.get()?.get_rendezvous_address()?;
        if address.len() > 128 {
            return Err(failed("invalid Native rendezvous address"));
        }
        request.get().set_rendezvous_address(address);
        let response = request.send().promise.await?;
        let value = response.get()?;
        results.get().set_ticket(value.get_ticket()?)?;
        results
            .get()
            .set_lease(capnp_rpc::new_client(RelayLease(value.get_lease()?)));
        Ok(())
    }
}
impl lease::Server for RelayLease {
    async fn ready(
        self: Rc<Self>,
        _: lease::ReadyParams,
        _: lease::ReadyResults,
    ) -> capnp::Result<()> {
        self.0.ready_request().send().promise.await?;
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: lease::CancelParams,
        _: lease::CancelResults,
    ) -> capnp::Result<()> {
        self.0.cancel_request().send().promise.await?;
        Ok(())
    }
}

struct Entry {
    provider: provisioner::Client,
    address: SocketAddr,
    context: Vec<u8>,
}
/// Allowlist of remote provisioning authorities. Each dial obtains a fresh
/// reservation. No ticket can change the configured host, recipient, destination
/// address or context. Existing native routes retain their connection identity.
pub struct ProvisioningConnector {
    identity: Rc<Identity>,
    bind: SocketAddr,
    entries: RefCell<HashMap<[u8; 32], Rc<Entry>>>,
    stun_server: Option<SocketAddr>,
}
impl ProvisioningConnector {
    pub fn new(identity: Rc<Identity>, bind: SocketAddr) -> Self {
        Self {
            identity,
            bind,
            entries: RefCell::new(HashMap::new()),
            stun_server: None,
        }
    }
    /// Discover a mapping before each reservation, then dial from that socket.
    pub fn with_stun(
        identity: Rc<Identity>,
        bind: SocketAddr,
        server: SocketAddr,
    ) -> capnp::Result<Self> {
        if !address_ok(server) {
            return Err(failed("invalid STUN server"));
        }
        Ok(Self {
            stun_server: Some(server),
            ..Self::new(identity, bind)
        })
    }
    pub fn insert(
        &self,
        host: [u8; 32],
        provider: provisioner::Client,
        address: SocketAddr,
        context: &[u8],
    ) -> capnp::Result<()> {
        if host == [0; 32]
            || host == self.identity.public_key()
            || !address_ok(address)
            || context.len() > MAX_CONTEXT_BYTES
        {
            return Err(failed("invalid provisioned Native destination"));
        }
        let old = {
            let mut entries = self.entries.borrow_mut();
            if !entries.contains_key(&host) && entries.len() >= 64 {
                return Err(Error::overloaded(
                    "Native provisioning directory limit".into(),
                ));
            }
            entries.insert(
                host,
                Rc::new(Entry {
                    provider,
                    address,
                    context: context.to_vec(),
                }),
            )
        };
        drop(old); // Capability release/destructors may reenter the directory.
        Ok(())
    }
    pub fn remove(&self, host: [u8; 32]) {
        let removed = self.entries.borrow_mut().remove(&host);
        drop(removed);
    }
}
struct DialTicket {
    target: Target,
    psk: Zeroizing<[u8; 32]>,
}
fn validate(
    ticket: ticket::Reader<'_>,
    host: [u8; 32],
    recipient: [u8; 32],
    entry: &Entry,
) -> capnp::Result<DialTicket> {
    if ticket.get_host()? != host
        || ticket.get_recipient()? != recipient
        || ticket.get_context()? != entry.context
    {
        return Err(failed("Native provisioning binding mismatch"));
    }
    let address = ticket.get_address()?;
    if address.len() > 128
        || address.to_str()?.parse::<SocketAddr>().map_err(failed)? != entry.address
    {
        return Err(failed("Native provisioning destination mismatch"));
    }
    let connection_id = ticket
        .get_connection_id()?
        .try_into()
        .map_err(|_| failed("invalid provisioned connection ID"))?;
    let psk = Zeroizing::new(
        ticket
            .get_psk()?
            .try_into()
            .map_err(|_| failed("invalid provisioned PSK"))?,
    );
    Ok(DialTicket {
        target: Target {
            address: entry.address,
            host,
            connection_id,
        },
        psk,
    })
}
impl Connector for ProvisioningConnector {
    fn connect(&self, peer: [u8; 32]) -> Promise<AuthenticatedSession, Error> {
        let Some(entry) = self.entries.borrow().get(&peer).cloned() else {
            return Promise::err(failed("Native provisioning authority not configured"));
        };
        let identity = self.identity.clone();
        let bind = self.bind;
        let stun_server = self.stun_server;
        Promise::from_future(async move {
            tokio::time::timeout(Duration::from_secs(10), async move {
                let socket = tokio::net::UdpSocket::bind(bind).await.map_err(failed)?;
                let mut request = entry.provider.reserve_request();
                if let Some(server) = stun_server {
                    let observed = crate::nat::discover(&socket, server, Duration::from_secs(3))
                        .await
                        .map_err(crate::native_rpc::dial_error)?;
                    request.get().set_rendezvous_address(observed.to_string());
                }
                let response = request.send().promise.await?;
                let result = response.get()?;
                let lease = result.get_lease()?;
                let ticket = validate(result.get_ticket()?, peer, identity.public_key(), &entry)?;
                drop(response);
                let ready = lease.ready_request().send().promise;
                let connect = async {
                    native_listener::connect(
                        socket,
                        ticket.target,
                        &identity,
                        Some(*ticket.psk),
                        &entry.context,
                    )
                    .await
                    .map_err(crate::native_rpc::dial_error)
                };
                let (session, _) = futures::try_join!(connect, ready)?;
                // Keep the lease through both proofs; a canceled dial releases
                // this capability and its outstanding ready() waiter together.
                drop(lease);
                Ok(session)
            })
            .await
            .map_err(|_| Error::disconnected("Native provisioning timed out".into()))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{future::LocalBoxFuture, FutureExt};

    #[tokio::test(flavor = "current_thread")]
    async fn waiter_limits_installation_claim_and_callback_reentrancy() {
        let state = LeaseState::new();
        let mut waiters: Vec<_> = (0..64).map(|_| Box::pin(state.ready())).collect();
        for waiter in &mut waiters {
            assert!(waiter.as_mut().now_or_never().is_none());
        }
        assert_eq!(state.waiters.get(), 64);
        assert!(state.ready().await.is_err());
        drop(waiters.pop());
        assert_eq!(state.waiters.get(), 63);
        assert!(state.begin_install());
        assert!(!state.begin_install());
        state.cancel(canceled());
        assert_eq!(state.phase.get(), Phase::Installing);
        state.installed(Ok(()));
        for waiter in waiters {
            waiter.await.unwrap();
        }
        assert_eq!(state.waiters.get(), 0);
        state.cancel(canceled());
        state.ready().await.unwrap();

        // A synchronous custom callback inspects/revokes the provider while
        // ready() is notified. No registry/result/task borrow may be held.
        thread_local! {
            static INSPECT: RefCell<Option<(Rc<Registry>, Rc<LeaseState>)>> = const { RefCell::new(None) };
        }
        struct Inspect;
        impl std::task::Wake for Inspect {
            fn wake(self: std::sync::Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                INSPECT.with(|slot| {
                    let slot = slot.borrow();
                    let (registry, state) = slot.as_ref().unwrap();
                    let _ = state.result.borrow_mut();
                    let _ = state.task.borrow_mut();
                    registry.close();
                    assert!(registry.allocate().is_err());
                });
            }
        }
        let registry = Rc::new(Registry::default());
        let state = registry.allocate().unwrap();
        INSPECT.with(|slot| *slot.borrow_mut() = Some((registry.clone(), state.clone())));
        let waker = std::task::Waker::from(std::sync::Arc::new(Inspect));
        let mut context = std::task::Context::from_waker(&waker);
        let mut wait = Box::pin(state.ready());
        assert!(std::future::Future::poll(wait.as_mut(), &mut context).is_pending());
        assert!(state.begin_install());
        state.installed(Ok(()));
        wait.await.unwrap();
        assert!(registry.closed.get());
        assert_eq!(state.phase.get(), Phase::Installed);
        INSPECT.with(|slot| slot.borrow_mut().take());
    }

    #[test]
    fn ticket_validation_rejects_each_binding_and_malformed_field() {
        struct Never;
        impl provisioner::Server for Never {}
        let entry = Entry {
            provider: capnp_rpc::new_client(Never),
            address: "127.0.0.1:1234".parse().unwrap(),
            context: b"expected".to_vec(),
        };
        for case in 0..10 {
            let mut message = capnp::message::Builder::new_default();
            let mut ticket = message.init_root::<ticket::Builder>();
            ticket.set_host(&[1; 32]);
            ticket.set_recipient(&[2; 32]);
            ticket.set_address("127.0.0.1:1234");
            ticket.set_context(b"expected");
            ticket.set_connection_id(&[3; 16]);
            ticket.set_psk(&[4; 32]);
            match case {
                0 => {}
                1 => ticket.set_host(&[5; 32]),
                2 => ticket.set_recipient(&[5; 32]),
                3 => ticket.set_context(b"other"),
                4 => ticket.set_address("127.0.0.1:1235"),
                5 => ticket.set_address("malformed"),
                6 => ticket.set_address("x".repeat(129)),
                7 => ticket.set_connection_id(&[3; 15]),
                8 => ticket.set_psk(&[4; 31]),
                9 => ticket.set_psk(&[]),
                _ => unreachable!(),
            }
            assert_eq!(
                validate(ticket.into_reader(), [1; 32], [2; 32], &entry).is_ok(),
                case == 0
            );
        }
    }

    #[derive(serde::Deserialize)]
    struct Trace {
        steps: Vec<Step>,
    }
    #[derive(serde::Deserialize)]
    struct Step {
        action: String,
        item: usize,
        state: Vec<u64>,
    }
    fn phase(state: &LeaseState) -> u64 {
        match state.phase.get() {
            Phase::Pending => 1,
            Phase::Installing => 2,
            Phase::Installed => 3,
            Phase::Retired => 4,
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_provisioning_traces() {
        let path = capntproto_test_support::verification::input("CAPNTPROTO_PROVISIONING_TRACES")
            .expect("run this test through its verification driver");
        let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(!traces.is_empty());
        for (index, trace) in traces.into_iter().enumerate() {
            let registry = Registry::default();
            let mut held: [Option<Rc<LeaseState>>; 2] = [None, None];
            let mut claimed: [Option<Rc<LeaseState>>; 2] = [None, None];
            let mut weak: [Weak<LeaseState>; 2] = [Weak::new(), Weak::new()];
            let mut waits: [Option<LocalBoxFuture<'static, capnp::Result<()>>>; 2] = [None, None];
            let mut phases = [0; 2];
            let mut proofs = [0; 2];
            let mut ack = [0; 2];
            let mut route = 0;
            for step in trace.steps {
                let i = step.item.saturating_sub(1);
                match step.action.as_str() {
                    "reserve" => {
                        if route == 0 {
                            match registry.allocate() {
                                Ok(state) => {
                                    assert_eq!(phases[i], 0);
                                    weak[i] = Rc::downgrade(&state);
                                    held[i] = Some(state);
                                }
                                Err(_) => assert!(
                                    registry.closed.get()
                                        || weak
                                            .iter()
                                            .filter_map(Weak::upgrade)
                                            .any(|s| s.active())
                                ),
                            }
                        }
                    }
                    "wait" => {
                        let state = held[i].as_ref().unwrap().clone();
                        let pending = state.active();
                        let mut future = async move { state.ready().await }.boxed_local();
                        if pending {
                            assert!(future.as_mut().now_or_never().is_none());
                        }
                        waits[i] = Some(future);
                    }
                    "authenticate" => {
                        let state = weak[i].upgrade().unwrap();
                        assert!(state.begin_install());
                        proofs[i] = 1;
                        claimed[i] = Some(state);
                    }
                    "install" | "fail_install" => {
                        let state = claimed[i].take().unwrap();
                        if step.action == "install" && route == 0 {
                            state.installed(Ok(()));
                            route = i + 1;
                        } else {
                            state.installed(Err(failed("attach rejected")));
                        }
                        phases[i] = phase(&state);
                    }
                    "cancel" | "expire" => weak[i].upgrade().unwrap().cancel(canceled()),
                    "drop" => drop(held[i].take()),
                    "cancel_wait" => drop(waits[i].take()),
                    "ready" => {
                        let result = waits[i]
                            .take()
                            .unwrap()
                            .now_or_never()
                            .expect("ack must be ready");
                        ack[i] = if result.is_ok() { 2 } else { 3 };
                    }
                    "close" => registry.close(),
                    "disconnect" => route = 0,
                    "late_authenticate" => assert!(!weak[i].upgrade().unwrap().begin_install()),
                    action => panic!("unexpected {action}"),
                }
                for j in 0..2 {
                    if let Some(state) = weak[j].upgrade() {
                        phases[j] = phase(&state);
                    } else if phases[j] == 1 {
                        phases[j] = 4;
                    }
                }
                let actual = vec![
                    phases[0],
                    phases[1],
                    held[0].is_some() as u64,
                    held[1].is_some() as u64,
                    waits[0].is_some() as u64,
                    waits[1].is_some() as u64,
                    proofs[0],
                    proofs[1],
                    ack[0],
                    ack[1],
                    registry.closed.get() as u64,
                    route as u64,
                    0,
                    0,
                ];
                assert_eq!(
                    actual, step.state,
                    "trace {index} {} {}",
                    step.action, step.item
                );
            }
        }
    }
}

//! Task-owned hosted RPC using the portable protocol driver.
//!
//! This replacement drives the supplied VatNetwork, accepts and reuses its
//! connections, owns bounded growing storage, and runs protected server calls.
//! Generated clients and servers retain their ordinary API. No executor or clock
//! is selected: poll this system on the caller's local executor.
//!
//! Currently opt-in while default RpcSystem parity is completed. Authenticated
//! independent-path Join, automatic introductions, and incoming answer handoff
//! and outgoing third-party tail transfer are integrated. Bilateral Join and native
//! tail/pipeline-only optimizations still need integration. Live incoming/outgoing limits, exact
//! table diagnostics, opt-in
//! exception traces and transport idle notifications are integrated.
//! Unsupported network controls fail explicitly; this runtime
//! never falls back to the legacy driver. The engine itself remains no_std/no_alloc.
//!
//! ```no_run
//! use capnp_rpc::{portable::{Options, RpcSystem}, rpc_twoparty_capnp::Side};
//! use std::num::NonZeroUsize;
//!
//! # fn example(network: Box<dyn capnp_rpc::VatNetwork<Side>>) -> capnp::Result<()> {
//! let options = Options::bounded(
//!     NonZeroUsize::new(1).unwrap(),     // live/constructing/draining connections
//!     NonZeroUsize::new(256).unwrap(),   // entries per table/queue
//!     NonZeroUsize::new(128 * 1024).unwrap(), // maximum framed message bytes
//!     NonZeroUsize::new(4 * 1024 * 1024).unwrap(), // retained message-buffer bytes
//! )?;
//! let mut system = RpcSystem::new(network, None, options)?;
//! let bootstrap: capnp::capability::Client = system.bootstrap(Side::Server);
//! let disconnector = system.get_disconnector();
//! // Poll `system` on your local executor while making calls and awaiting
//! // `disconnector`. A caller-supplied timer can invoke disconnector.abort(...).
//! # let _ = (system, bootstrap, disconnector);
//! # Ok(())
//! # }
//! ```
use crate::{
    scoped::wire::coordinator::{self, transport::vat, Handle},
    system::registry::{self, Acquired, Connections},
    task_set::{TaskSet, TaskSetHandle},
    Bootstrap, BootstrapFactory, Error, VatNetwork,
};
use capnp::capability::{self, Promise};
use capntproto_rpc_engine::completion::shared;
use futures::future::Shared;
use std::{
    cell::RefCell,
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

mod connection;
mod introduction;
mod join;
mod options;
pub use join::Joiner;
pub use options::Options;

type Network<V> = Rc<RefCell<Box<dyn VatNetwork<V>>>>;
struct State {
    id: usize,
    handle: Handle,
    stop: RefCell<Option<shared::Sender<capnp::Result<()>>>>,
    abort: RefCell<Option<shared::Sender<Error>>>,
    policy: Rc<coordinator::policy::Policy>,
    idle: Rc<dyn Fn(bool)>,
    network_controls: coordinator::multiparty::Handle,
}
impl State {
    fn close_clients(&self, error: Error) {
        self.network_controls.close(error.clone());
        self.handle.close(error);
    }
    fn stop(&self, result: capnp::Result<()>) {
        self.close_clients(result.as_ref().err().cloned().unwrap_or_else(closing));
        let stop = self.stop.borrow_mut().take();
        if let Some(stop) = stop {
            let _ = stop.send(result);
        }
    }
    fn abort(&self, error: Error) {
        self.close_clients(error.clone());
        let abort = self.abort.borrow_mut().take();
        if let Some(abort) = abort {
            let _ = abort.send(error);
        }
    }
    fn bootstrap<T: capability::FromClientHook>(&self) -> T {
        (self.idle)(false);
        self.handle.bootstrap()
    }
}
impl registry::Connection for State {
    fn disconnect(&self, error: Error) {
        self.abort(error);
    }
}

struct ContextOwner<V: 'static> {
    bootstrap: Bootstrap<V>,
    options: Options,
    tasks: TaskSetHandle<Error>,
    policy: Rc<coordinator::policy::Settings>,
    join_network: Option<Rc<dyn crate::multiparty::JoinNetwork<V>>>,
}

/// A local-executor RPC system backed exclusively by the portable engine.
/// All per-connection storage limits are supplied at construction.
#[must_use = "poll the RPC system to drive its network and connection tasks"]
pub struct RpcSystem<V: 'static> {
    network: Network<V>,
    context: Rc<ContextOwner<V>>,
    connections: Rc<Connections<State>>,
    tasks: TaskSet<Error>,
}
impl<V: 'static> RpcSystem<V> {
    pub fn get_joiner(&self) -> Joiner<V> {
        Joiner::new(
            Rc::downgrade(&self.connections),
            Rc::downgrade(&self.context),
        )
    }
    /// Live table occupancy and admission ownership. The observer retains no
    /// network, task or protocol owner; dropping the system empties snapshots.
    pub fn diagnostics(&self) -> crate::RpcDiagnostics {
        let connections = Rc::downgrade(&self.connections);
        crate::RpcDiagnostics(Rc::new(move || {
            let Some(connections) = connections.upgrade() else {
                return Vec::new();
            };
            let mut snapshots: Vec<_> = connections
                .states()
                .iter()
                .map(|state| state.policy.snapshot(state.id))
                .collect();
            snapshots.sort_by_key(|snapshot| snapshot.connection_id);
            snapshots
        }))
    }
    pub fn new(
        network: Box<dyn VatNetwork<V>>,
        bootstrap: Option<capability::Client>,
        options: Options,
    ) -> capnp::Result<Self> {
        let bootstrap = Bootstrap::Static(bootstrap.map_or_else(
            || crate::broken::new_cap(Error::failed("no bootstrap capability".into())),
            |client| client.hook,
        ));
        Self::with_bootstrap(network, bootstrap, options)
    }
    /// The factory receives only the transport's captured peer identity. The
    /// local ID is used for self-bootstrap; V need not implement Clone.
    pub fn new_with_bootstrap_factory(
        network: Box<dyn VatNetwork<V>>,
        local_id: V,
        factory: Rc<dyn BootstrapFactory<V>>,
        options: Options,
    ) -> capnp::Result<Self> {
        Self::with_bootstrap(
            network,
            Bootstrap::Factory {
                factory,
                local_id: Rc::new(local_id),
            },
            options,
        )
    }
    fn with_bootstrap(
        mut network: Box<dyn VatNetwork<V>>,
        bootstrap: Bootstrap<V>,
        options: Options,
    ) -> capnp::Result<Self> {
        options.validate()?;
        let join_network = network.join_network();
        let (mut tasks, driver) = TaskSet::new(Box::new(crate::SystemTaskReaper));
        let connections = Rc::new(Connections::<State>::new());
        let closing_connections = connections.clone();
        crate::system::drive_network_with_completion(
            &mut *network,
            &mut tasks,
            move |result| async move {
                let connections = closing_connections;
                let network_result = result.clone();
                let expected = connections
                    .states()
                    .iter()
                    .all(|state| state.handle.is_closed());
                if result.is_ok() && expected {
                    for state in connections.begin_close() {
                        state.stop(Ok(()));
                    }
                } else {
                    let reason = result.err().unwrap_or_else(|| {
                        Error::disconnected("network ended before protocol shutdown".into())
                    });
                    for state in connections.begin_abort(reason.clone()) {
                        state.abort(reason.clone());
                    }
                }
                connections.closed().await?;
                network_result
            },
        );
        let system = Self {
            network: Rc::new(RefCell::new(network)),
            context: Rc::new(ContextOwner {
                bootstrap,
                options,
                tasks: tasks.clone(),
                policy: Rc::default(),
                join_network,
            }),
            connections,
            tasks: driver,
        };
        let network = system.network.clone();
        let context = system.context.clone();
        let connections = system.connections.clone();
        tasks.add(async move {
            let mut remaining = context.options.poll_budget.get();
            loop {
                let accepting = network.borrow_mut().accept();
                let accepted = accepting.await;
                if connections.is_closing() {
                    // An accept already in flight may win the shutdown race.
                    // It was never admitted; no protocol owner may escape it.
                    drop(accepted);
                    return Ok(());
                }
                let connection = match accepted {
                    Ok(connection) => connection,
                    Err(error) => {
                        return fail_accept(&connections, &context.tasks, error).await;
                    }
                };
                if let Err(error) = connection::connect(&connections, &context, connection) {
                    if connections.is_closing() {
                        return Ok(());
                    }
                    // A full connection budget rejects only this accept.
                    if error.kind != capnp::ErrorKind::Overloaded {
                        return fail_accept(&connections, &context.tasks, error).await;
                    }
                }
                remaining -= 1;
                if remaining == 0 {
                    remaining = context.options.poll_budget.get();
                    let mut yielded = false;
                    std::future::poll_fn(|cx| {
                        if std::mem::replace(&mut yielded, true) {
                            Poll::Ready(())
                        } else {
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                }
            }
        });
        Ok(system)
    }
    pub fn bootstrap<T: capability::FromClientHook>(&mut self, vat: V) -> T {
        self.get_bootstrapper().bootstrap(vat)
    }
    pub fn get_bootstrapper(&self) -> Bootstrapper<V> {
        Bootstrapper {
            network: Rc::downgrade(&self.network),
            context: Rc::downgrade(&self.context),
            connections: Rc::downgrade(&self.connections),
        }
    }
    /// Incoming Call backpressure, in full message words. Like C++, pause all
    /// input above the limit and resume strictly below it. This is not a hard
    /// memory bound; cyclic calls can deadlock while reads are paused. Zero
    /// requires a later increase after the first charged Call crosses it.
    pub fn set_flow_limit(&mut self, words: usize) {
        self.context.policy.flow_limit.set(words);
        for state in self.connections.states() {
            state.policy.notify();
        }
    }
    /// Limit outgoing Calls on current and future connections. Excess requests
    /// fail locally with Overloaded; accepted calls are never canceled by a
    /// reduction. Zero rejects new Calls while protocol progress stays enabled.
    ///
    /// Credit covers remote requests before send, their question lifetime,
    /// response/pipeline contexts, and local write completion. Purely local
    /// unresolved calls acquire credit only if they later reach a remote route.
    pub fn set_outgoing_call_limit(&mut self, calls: usize) {
        self.context.policy.outgoing_limit.set(calls);
        for state in self.connections.states() {
            state.policy.admission.set_limit(calls);
        }
    }
    /// Optional local diagnostic text for subsequently serialized exceptions on
    /// existing and future connections. Remote traces are never echoed by default.
    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        let previous = self
            .context
            .policy
            .trace
            .borrow_mut()
            .replace(Rc::new(encoder));
        drop(previous);
    }
    pub fn clear_trace_encoder(&mut self) {
        let previous = self.context.policy.trace.borrow_mut().take();
        drop(previous);
    }
    /// Start orderly shutdown on first poll. The returned handle can also abort
    /// an already-started drain when the caller's timer/deadline expires.
    pub fn get_disconnector(&self) -> Disconnector {
        Disconnector {
            connections: (*self.connections).clone(),
            closed: self.connections.closed(),
        }
    }
}
impl<V: 'static> Future for RpcSystem<V> {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.tasks).poll(cx)
    }
}
impl<V: 'static> Drop for RpcSystem<V> {
    fn drop(&mut self) {
        self.connections.begin_close();
        for state in self.connections.states() {
            state.abort(Error::disconnected("RPC system was dropped".into()));
        }
    }
}

/// Weak typed bootstrap access. Retaining clients/handles does not retain the
/// network, the connection task, or its branded storage.
pub struct Bootstrapper<V: 'static> {
    network: Weak<RefCell<Box<dyn VatNetwork<V>>>>,
    context: Weak<ContextOwner<V>>,
    connections: Weak<Connections<State>>,
}
impl<V> Clone for Bootstrapper<V> {
    fn clone(&self) -> Self {
        Self {
            network: self.network.clone(),
            context: self.context.clone(),
            connections: self.connections.clone(),
        }
    }
}
impl<V: 'static> Bootstrapper<V> {
    pub fn bootstrap<T: capability::FromClientHook>(&self, vat: V) -> T {
        let prepare = || {
            let network = self.network.upgrade().ok_or_else(closing)?;
            let context = self.context.upgrade().ok_or_else(closing)?;
            let connections = self.connections.upgrade().ok_or_else(closing)?;
            if connections.is_closing() {
                return Err(closing());
            }
            let connection = network.borrow_mut().connect(vat);
            let Some(connection) = connection else {
                return context.bootstrap.local().map(T::new);
            };
            connection::connect(&connections, &context, connection).map(|state| state.bootstrap())
        };
        prepare().unwrap_or_else(|error| T::new(crate::broken::new_cap(error)))
    }
}

/// Completion includes accepted output drain, physical close and storage
/// teardown. Multiple observers are independent. Abort is immediate and its
/// original reason is retained; it does not certify physical close.
pub struct Disconnector {
    connections: Connections<State>,
    closed: Shared<Promise<(), Error>>,
}
impl Disconnector {
    pub fn abort(&self, reason: Error) {
        for state in self.connections.begin_abort(reason.clone()) {
            state.abort(reason.clone());
        }
    }
}
impl Future for Disconnector {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for state in self.connections.begin_close() {
            state.stop(Ok(()));
        }
        Pin::new(&mut self.closed).poll(cx)
    }
}
fn closing() -> Error {
    Error::disconnected("portable RPC system is closing or was dropped".into())
}

// Losing the accept loop is a system failure. The generic task reaper only logs
// task errors; returning one to it would leave a persistent network driver and
// its clients parked forever. Retire registered tasks before terminating them.
async fn fail_accept(
    connections: &Connections<State>,
    tasks: &TaskSetHandle<Error>,
    error: Error,
) -> capnp::Result<()> {
    for state in connections.begin_abort(error.clone()) {
        state.abort(error.clone());
    }
    let outcome = connections.closed().await.and(Err(error));
    tasks.clone().terminate(outcome);
    Ok(())
}

#[cfg(test)]
mod tests;

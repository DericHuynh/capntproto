//! Hosted connection pump over the allocation-free driver and IO interfaces.
//! No socket, timer, runtime, background task, or second protocol ledger is owned
//! here. Bootstrap policy and authenticated network operations stay with Control.
use super::*;
use capntproto_rpc_engine::{ids::ImportId, io};
use capntproto_rpc_wire::owned::{pipelines, tasks, Frame};
use std::num::NonZeroUsize;
mod bootstrap;
pub(crate) mod multiparty;
mod notified;
pub use bootstrap::TwoParty;
mod handle;
pub(crate) use handle::Commands;
pub use handle::Handle;
pub mod network;
pub mod policy;
pub mod transport;
use transport::{Input, NoResources, Output};
mod control;
pub use control::{Activity, Control, ControlContext};
mod scheduler;
pub(in crate::scoped) use scheduler::EnqueueWake;
use scheduler::{Lane, Notifications, Schedule, WorkSet};
mod ingress;
mod lifecycle;
mod pump;
mod work;

pub type Peer<'p, 'id, 's, I, P, const C: usize, const H: usize> =
    Driver<'p, 'id, 's, I, server::Results, P, Rc<dyn ClientHook>, C, H>;

/// Pool capacity and cooperative scheduling are caller policy. Input storage is
/// the largest contiguous wire frame accepted by this connection, including its
/// segment table. Output storage is a minimum; application results size their
/// own buffers. The pool needs room for input, retained results and FIFO output.
#[derive(Clone, Copy)]
pub struct Options {
    pub input_bytes: NonZeroUsize,
    pub output_bytes: NonZeroUsize,
    pub budget: NonZeroUsize,
    pub reader: capnp::message::ReaderOptions,
}

#[derive(Debug)]
pub enum Cause<R, W> {
    Input(tasks::ReceiveError<R>),
    Output(io::SendError<W>),
    Close(W),
    Protocol(Error),
}
/// Preserve the original failure even if terminal cleanup also fails.
#[derive(Debug)]
pub struct RunError<R, W> {
    pub cause: Option<Cause<R, W>>,
    pub cleanup: Option<connection::Error>,
}
impl<R: std::fmt::Debug, W: std::fmt::Debug> std::fmt::Display for RunError<R, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "connection stopped: {:?}; cleanup: {:?}",
            self.cause, self.cleanup
        )
    }
}
impl<R: std::fmt::Debug, W: std::fmt::Debug> std::error::Error for RunError<R, W> {}

#[derive(Default)]
struct Pump<'id> {
    frames: usize,
    // Input admitted while output waits, retained across executor yields.
    output_frames: usize,
    returned: Option<(capntproto_rpc_engine::questions::Key<'id>, Option<usize>)>,
    blocked: bool,
}

/// One hosted connection, owning application futures and the portable driver.
/// Public hooks/foreign/driver fields let Control implement network policy in
/// the same generative scope. Poll `run` with caller-owned IO and a stop future.
/// Completion (including EOF, stop and errors) drains terminal protocol storage.
/// Dropping `run` early instead requires ending the entire message-pool scope.
/// Use run_transport for resource-bearing streams; run adapts byte-only IO to
/// that same pump with explicit omission of ancillary resources.
#[must_use = "run this coordinator to completion or end its entire pool scope"]
pub struct Coordinator<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize> {
    pub driver: Peer<'p, 'id, 's, I, P, C, H>,
    pub hooks: ClientHooks<'id>,
    pub foreign: foreign::ForeignPromises<'id>,
    calls: Calls<'s, 's, 'p, 'id, I, C, H>,
    worker: server::Worker<'p, 'id, I, C, H>,
    options: Options,
    schedule: Schedule,
    output_blocked: bool,
    calls_waiting_for_output: bool,
    delivered: bool,
    cleanup_on_enqueue: bool,
    cleanup_before_call: bool,
    notifications: Notifications,
    control_waker: Waker,
    worker_waker: Waker,
    enqueue_waker: Rc<EnqueueWake>,
    commands: Rc<handle::Commands>,
    output: io::Output,
    registration: Option<network::Registration>,
    policy: Rc<policy::Policy>,
    idle: bool,
    control_idle: bool,
    direct_publication: bool,
    coalescing_output: bool,
}
impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
    pub fn new(
        driver: Peer<'p, 'id, 's, I, P, C, H>,
        hooks: ClientHooks<'id>,
        maximum: NonZeroUsize,
        executor: Option<Rc<dyn capability::CallExecutor>>,
        options: Options,
    ) -> capnp::Result<Self> {
        Self::new_using_commands(
            driver,
            hooks,
            maximum,
            executor,
            options,
            Rc::new(Commands::new(maximum)),
        )
    }
    /// The hosted system reserves this inbox before spawning the branded task.
    /// Bootstrap admission remains ordered, bounded and cancelable before the
    /// connection task is polled for the first time.
    pub(crate) fn new_using_commands(
        mut driver: Peer<'p, 'id, 's, I, P, C, H>,
        hooks: ClientHooks<'id>,
        maximum: NonZeroUsize,
        executor: Option<Rc<dyn capability::CallExecutor>>,
        options: Options,
        commands: Rc<Commands>,
    ) -> capnp::Result<Self> {
        driver.defer_call_dispatch(true);
        let notifications = Notifications::default();
        let enqueue_waker = Rc::new(notifications.enqueue());
        let control_waker = notifications.control_waker();
        let worker_waker = notifications.worker_waker();
        // Commands have one coordinator owner and retain this stable notifier.
        // Parent-task replacement is handled by WorkWake's registration, so
        // ordinary calls need not register the command inbox again.
        commands.register(notifications.waker());
        Ok(Self {
            driver,
            hooks,
            foreign: foreign::ForeignPromises::new(maximum.get()),
            calls: Calls::new(maximum.get(), executor.clone())?,
            worker: server::Worker::new(maximum.get(), executor),
            options,
            schedule: Schedule::default(),
            output_blocked: false,
            calls_waiting_for_output: false,
            delivered: false,
            cleanup_on_enqueue: false,
            cleanup_before_call: false,
            notifications,
            control_waker,
            worker_waker,
            enqueue_waker,
            commands,
            output: io::Output::new(),
            registration: None,
            policy: policy::Policy::new(),
            idle: false,
            control_idle: false,
            direct_publication: false,
            coalescing_output: false,
        })
    }

    pub fn handle(&self) -> Handle {
        Handle::new(&self.commands)
    }
    pub fn with_policy(mut self, policy: Rc<policy::Policy>) -> Self {
        self.policy = policy;
        self
    }

    /// Publish a Bootstrap and expose its root pipeline as an owned hook. Calls
    /// may be sent immediately, before either IO or the peer's factory runs.
    /// The hook cannot keep this coordinator alive after shutdown.
    pub fn bootstrap(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, impl Default>,
    ) -> capnp::Result<capability::Client> {
        let client = self
            .driver
            .bootstrap(
                messages,
                queue,
                self.options.output_bytes.get(),
                self.options.reader,
            )
            .map_err(wire_error)?;
        let hook = self.hooks.export_client(&mut self.driver.runtime, &client);
        self.driver.runtime.release(client);
        self.changed();
        hook
    }
}

#[cfg(test)]
mod tests;

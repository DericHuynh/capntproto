//! Hosted connection pump over the allocation-free driver and IO interfaces.
//! No socket, timer, runtime, background task, or second protocol ledger is owned
//! here. Bootstrap policy and authenticated network operations stay with Control.
use super::*;
use capntproto_rpc_engine::{ids::ImportId, io};
use capntproto_rpc_wire::owned::{pipelines, tasks, Frame};
use std::num::NonZeroUsize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
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

// Application notifications invalidate idle certificates even while other
// lanes are continuing a budgeted scan. Scheduler-only yields use the outer
// waker directly, so they cannot continually restart completed idle scans.
#[derive(Default)]
struct WorkWake {
    changed: AtomicBool,
    parent: futures::task::AtomicWaker,
}
impl std::task::Wake for WorkWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.changed.swap(true, Ordering::AcqRel) {
            self.parent.wake();
        }
    }
}

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

/// Integration with a vat network. Every received event is delivered once.
/// Retain any unfinished bootstrap/network work in this handler and drive it in
/// `poll`, registering the supplied waker. Return true only after making progress.
/// Raw wire IDs never authenticate a handoff; use the driver's authenticated APIs.
/// `shutdown` must return/release every detached protocol owner before teardown.
/// Ordinary Call/Return/Resolve/Finish handling is performed by the coordinator.
pub trait Control<
    'p,
    'id,
    's,
    I,
    P: Copy + Eq,
    const C: usize,
    const H: usize,
    O: Output<I> = NoResources,
>
{
    /// Notify protocol quiescence after output flush. This is advisory transport
    /// policy, not a peer acknowledgement or permission to discard active refs.
    fn set_idle(&mut self, _idle: bool) {}
    fn received(
        &mut self,
        event: connection::Received<'id, server::Results>,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
    ) -> capnp::Result<()>;
    fn poll(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>>;
    fn shutdown(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    );
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
    active: u8,
    output_blocked: bool,
    delivered: bool,
    cursor: usize,
    wake: Arc<WorkWake>,
    commands: Rc<handle::Commands>,
    output: io::Output,
    registration: Option<network::Registration>,
    policy: Rc<policy::Policy>,
    idle: bool,
    control_idle: bool,
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
        driver: Peer<'p, 'id, 's, I, P, C, H>,
        hooks: ClientHooks<'id>,
        maximum: NonZeroUsize,
        executor: Option<Rc<dyn capability::CallExecutor>>,
        options: Options,
        commands: Rc<Commands>,
    ) -> capnp::Result<Self> {
        Ok(Self {
            driver,
            hooks,
            foreign: foreign::ForeignPromises::new(maximum.get()),
            calls: Calls::new(maximum.get(), executor.clone())?,
            worker: server::Worker::new(maximum.get(), executor),
            options,
            active: 0x3f,
            output_blocked: false,
            delivered: false,
            cursor: 0,
            wake: Arc::default(),
            commands,
            output: io::Output::new(),
            registration: None,
            policy: policy::Policy::new(),
            idle: false,
            control_idle: false,
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
    /// New IO/control progress invalidates the partial idle scan. Individual
    /// lanes report Yield separately from true Pending, so mismatched scan sizes
    /// cannot continually wake one another on an otherwise idle connection.
    fn changed(&mut self) {
        self.control_idle = false;
        self.active = 0x15
            | (u8::from(self.driver.runtime.queued() != 0) << 1)
            | (u8::from(!self.worker.is_empty()) << 3)
            | (u8::from(!self.commands.is_empty()) << 5);
        self.calls.invalidate_scan();
        self.worker.invalidate_scan();
        self.foreign.invalidate_scan();
        self.driver.invalidate_maintenance();
    }
    fn resume_at(&mut self, lane: usize) {
        // Once every lane has certified idle, start a new scan at the
        // lane made ready by this event. During an unfinished scan preserve
        // the rotating cursor, so a hot source cannot starve maintenance.
        if self.active == 0 {
            self.cursor = lane;
        }
        self.changed();
    }
    #[cfg(test)]
    fn poll_work<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<()>> {
        let mut budget = self.options.budget.get();
        self.poll_work_budget(messages, queue, scratch, output, cx, &mut budget)
    }
    fn poll_work_budget<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
        remaining: &mut usize,
    ) -> Poll<capnp::Result<()>> {
        self.wake.parent.register(cx.waker());
        if self.wake.changed.swap(false, Ordering::AcqRel) {
            self.resume_at(2);
        }
        // IO-only or spurious polls must preserve a completed idle scan. Real
        // protocol progress and application wakeups invalidate it explicitly.
        if self.active == 0 {
            return Poll::Pending;
        }
        let waker = Waker::from(self.wake.clone());
        self.commands.register(&waker);
        let mut application_cx = Context::from_waker(&waker);
        let mut advanced = false;
        let queued = queue.len();
        let mut completed = false;
        while *remaining > 0 && self.active != 0 {
            // Pick the first active lane at or after the rotating cursor, then
            // wrap once. This is the same bounded round-robin order without a
            // modulo and predicate for each inactive lane.
            let after_cursor = self.active & (u8::MAX << self.cursor);
            let candidates = if after_cursor == 0 {
                self.active
            } else {
                after_cursor
            };
            let index = candidates.trailing_zeros() as usize;
            self.cursor = if index == 5 { 0 } else { index + 1 };
            // Amortize each lane's scan setup while retaining a shared bound
            // and a rotating cursor. Even a productive lane consumes its full
            // allowance, so ready work cannot bypass the cooperative budget.
            let empty = match index {
                0 => self.foreign.is_empty(),
                1 => self.driver.runtime.queued() == 0,
                // Admission and publication share this lane. A fresh request
                // in the hook inbox needs the same quantum as an admitted
                // invocation; otherwise it takes an extra scheduler rotation
                // just to publish its already-ready frame.
                2 => self.hooks.inbox.queue.borrow().is_empty() && self.calls.is_empty(),
                3 => self.worker.is_empty(),
                5 => self.commands.is_empty(),
                _ => false,
            };
            if empty && matches!(index, 1 | 2 | 3 | 5) {
                // Empty lanes still consume their fair quantum and advance the
                // cursor, but need no publication/callback setup. Admission's
                // wake remains registered before certifying its queue idle.
                if index == 2 {
                    self.hooks.register_next(&waker);
                }
                *remaining -= 1;
                self.active &= !(1 << index);
                continue;
            }
            let quantum = if index == 4 && self.active == 1 << 4 {
                // Maintenance has its own rotating, bounded slot scan and
                // stops at the first publication. Once every other lane has
                // reported idle, use the remaining allowance instead of
                // rebuilding the scan every eight slots. Active application
                // lanes retain the ordinary round-robin quantum.
                *remaining
            } else if !empty && matches!(index, 0 | 2 | 3 | 4) {
                (*remaining).min(8)
            } else {
                1
            };
            *remaining -= quantum;
            let quantum = NonZeroUsize::new(quantum).unwrap();
            let options = self.options;
            // Each lane publishes at most one frame per successful step. Reserve
            // FIFO capacity before polling a publishing lane. A full live queue
            // sleeps on IO, while a zero-capacity queue is a configuration error.
            let writable = if index >= 2 && !empty {
                let queued = !queue.is_empty();
                match queue.ensure_capacity() {
                    Ok(()) => true,
                    Err(messages::Error::NoSendSlot) if queued => {
                        self.output_blocked = true;
                        false
                    }
                    Err(error) => return Poll::Ready(Err(wire_error(error))),
                }
            } else {
                true
            };
            let mut published = false;
            let outcome = if !writable {
                Poll::Pending
            } else {
                match index {
                    0 => {
                        self.hooks.collect_wire_optional(
                            &mut self.driver,
                            |owner| output.metadata_if_present(owner),
                            foreign::local_metadata,
                        );
                        match self.foreign.poll(
                            &mut self.hooks,
                            &mut self.driver.runtime,
                            &mut application_cx,
                            quantum,
                        ) {
                            Poll::Ready(Ok(foreign::Progress::Idle)) => Poll::Pending,
                            Poll::Ready(Ok(foreign::Progress::Yield)) => continue,
                            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                            Poll::Pending => Poll::Pending,
                        }
                    }
                    1 => match self.worker.accept(&mut self.driver) {
                        Ok(false) => Poll::Pending,
                        Ok(true) => Poll::Ready(Ok(())),
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    2 => match self.calls.poll_using(
                        &mut self.hooks,
                        &mut self.driver,
                        messages,
                        options.reader,
                        &mut application_cx,
                        quantum,
                        |runtime, hook| self.foreign.import(runtime, hook),
                        |hooks, driver| {
                            hooks.collect_wire_optional(
                                driver,
                                |owner| output.metadata_if_present(owner),
                                foreign::local_metadata,
                            )
                        },
                        |invocations, driver, messages, credit, publication| {
                            let mut bundle = output.bundle();
                            if let Some(credit) = credit {
                                output.retain_call(&mut bundle, credit)?;
                            }
                            let direct = match publication {
                                super::calls::Publication::Immediate {
                                    target,
                                    method,
                                    params,
                                } => {
                                    return driver
                                        .try_send_immediate_from(
                                            target,
                                            method,
                                            params,
                                            messages,
                                            queue,
                                            options.output_bytes.get(),
                                            options.reader,
                                            bundle,
                                        )
                                        .map(|key| key.map(super::calls::Published::Immediate))
                                        .map_err(wire_error);
                                }
                                super::calls::Publication::Queued { direct } => direct,
                            };
                            if direct {
                                return driver
                                    .send_invocation_using(
                                        messages,
                                        queue,
                                        options.output_bytes.get(),
                                        options.reader,
                                        scratch,
                                        bundle,
                                        |owner, imports, bundle| {
                                            output.attach(owner, imports, bundle)
                                        },
                                    )
                                    .map(|sent| sent.map(super::calls::Published::Queued))
                                    .map_err(wire_error);
                            }
                            invocations
                                .send_one_with_resources(
                                    driver,
                                    messages,
                                    queue,
                                    options.output_bytes.get(),
                                    options.reader,
                                    scratch,
                                    bundle,
                                    |owner, imports, bundle| output.attach(owner, imports, bundle),
                                )
                                .map(|sent| sent.map(super::calls::Published::Queued))
                                .map_err(|error| {
                                    wire_error(format!("outgoing invocation: {error:?}"))
                                })
                        },
                    ) {
                        Poll::Ready(Ok(Progress::Idle)) => Poll::Pending,
                        Poll::Ready(Ok(Progress::Yield)) => continue,
                        Poll::Ready(Ok(Progress::Completed)) => {
                            completed = true;
                            self.delivered = true;
                            Poll::Ready(Ok(()))
                        }
                        Poll::Ready(Ok(Progress::Published)) => {
                            published = true;
                            Poll::Ready(Ok(()))
                        }
                        Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                        Poll::Pending => Poll::Pending,
                    },
                    3 => match self.worker.poll_using(
                        &mut self.hooks,
                        &mut self.driver,
                        messages,
                        options.output_bytes.get(),
                        options.reader,
                        &mut application_cx,
                        quantum,
                        |runtime, hook| self.foreign.import(runtime, hook),
                        |hooks, driver| {
                            hooks.collect_wire_optional(
                                driver,
                                |owner| output.metadata_if_present(owner),
                                foreign::local_metadata,
                            )
                        },
                        |driver, messages, key, bytes| {
                            driver
                                .send_answer_using(
                                    key,
                                    messages,
                                    queue,
                                    bytes,
                                    options.reader,
                                    scratch,
                                    output.bundle(),
                                    |owner, imports, bundle| output.attach(owner, imports, bundle),
                                    server::Results::write_exception,
                                )
                                .map_err(wire_error)
                        },
                        |results| results.prepare_trace(&self.policy),
                    ) {
                        Poll::Ready(Ok(server::Progress::Idle)) => Poll::Pending,
                        Poll::Ready(Ok(server::Progress::Yield)) => continue,
                        Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                        Poll::Pending => Poll::Pending,
                    },
                    5 => match self.commands.take() {
                        None => Poll::Pending,
                        Some(sender) => {
                            if !sender.is_closed() {
                                let result = self.bootstrap(messages, queue);
                                sender.send(result);
                            }
                            Poll::Ready(Ok(()))
                        }
                    },
                    _ => match self.driver.poll_maintenance_with(
                        messages,
                        queue,
                        options.output_bytes.get(),
                        options.reader,
                        scratch,
                        quantum,
                        &mut || output.bundle(),
                        &mut |owner, imports, bundle| output.attach(owner, imports, bundle),
                        &mut |results, path, runtime| {
                            self.hooks
                                .server_pipeline(results, path, runtime, |runtime, hook| {
                                    self.foreign.import(runtime, hook)
                                })
                                .map(|client| client.map(Ok))
                                .map_err(|e| pipelines::Error::Input(owned::Error::Wire(e.kind)))
                        },
                    ) {
                        Ok(connection::Progress::Idle) => Poll::Pending,
                        Ok(connection::Progress::Yield) => continue,
                        Ok(_) => Poll::Ready(Ok(())),
                        Err(e) => Poll::Ready(Err(wire_error(e))),
                    },
                }
            };
            // Hook collection can retire the final graph reference even when
            // its application lane reports Pending. Maintenance may already
            // have certified idle earlier in this rotation; reactivate it so
            // Release/Finish and provision cleanup do not wait for unrelated IO.
            if self.driver.runtime.has_retired() {
                self.driver.invalidate_maintenance();
                self.active |= 1 << 4;
            }
            if let Poll::Ready(result) = outcome {
                result?;
                self.changed();
                // Its response cannot have arrived during publication. The
                // inbox wake was registered by calls.poll_using; preserve any
                // cancellation/new-call wake delivered during its callbacks.
                if published
                    && self.calls.idle_after_publication()
                    && self.hooks.inbox.queue.borrow().is_empty()
                    && !self.wake.changed.load(Ordering::Acquire)
                {
                    self.active &= !(1 << 2);
                }
                advanced = true;
                // Publish promptly and give a completed application's waker a
                // chance to run before scanning unrelated maintenance lanes.
                if completed || queue.len() > queued {
                    return Poll::Ready(Ok(()));
                }
                continue;
            }
            // changed() restarts partial idle scans without moving their
            // cursors. A completed scan is therefore already a fresh certificate.
            self.active &= !(1 << index);
            if self.active == 0 {
                return if advanced {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                };
            }
        }
        if self.active != 0 {
            cx.waker().wake_by_ref();
        }
        if advanced {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    /// Full-duplex cooperative pump. The stop future supplies caller-controlled
    /// immediate cancellation/deadlines; no clock is chosen here. IO adapters must register
    /// wakeups on Pending. EOF is terminal (there is no half-close RPC mode).
    /// Capacity exhaustion outside the FIFO is reported, never silently retried
    /// forever. Caller storage must cover the configured concurrency and frames.
    /// Accepted bytes are flushed during normal operation. Stop and EOF discard
    /// unsent output; use run_stream for orderly drain and transport close.
    #[allow(clippy::too_many_arguments)]
    pub async fn run<R: io::Read, W: io::Write>(
        self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        make_import: impl FnMut(ImportId) -> I,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H>,
        stop: impl Future<Output = capnp::Result<()>>,
    ) -> Result<(), RunError<R::Error, W::Error>> {
        self.run_transport(
            messages,
            queue,
            &mut transport::Bytes {
                reader,
                make: make_import,
            },
            &mut transport::Writer(writer),
            scratch,
            &NoResources,
            controls,
            stop,
        )
        .await
    }

    /// Resource-aware pump. Input owns ancillary resources until admission
    /// commits; output bundles follow the exact frame through partial writes.
    /// Both resource cleanup and protocol cleanup complete on every exit.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_transport<O: Output<I>, R: Input<I>, W: io::WriteResources<O::Bundle>>(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        stop: impl Future<Output = capnp::Result<()>>,
    ) -> Result<(), RunError<R::Error, W::Error>> {
        let stop = std::pin::pin!(stop);
        let mut stop = std::pin::pin!(notified::Notified::new(stop));
        let mut pending = None;
        let outcome = self
            .drive(
                messages,
                queue,
                reader,
                writer,
                scratch,
                output,
                controls,
                stop.as_mut(),
                &mut pending,
            )
            .await;
        reader.finish_frame();
        if let Some(frame) = pending {
            frame.release(messages);
        }
        self.terminate::<O, _, _>(messages, queue, controls, outcome)
            .await
    }

    /// Managed byte stream: orderly stop/EOF drains accepted frames, flushes and
    /// closes the write half. No new application work is admitted during drain.
    /// The separate abort future supplies a deadline or immediate cancellation;
    /// it remains polled during writes, flush and close. Error/abort exits skip
    /// further IO and return all protocol storage; the caller then drops the IO.
    /// Completion confirms local closure, never peer receipt or RPC completion.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_stream<R: io::Read, W: io::Write + io::Close>(
        self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        make_import: impl FnMut(ImportId) -> I,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H>,
        shutdown: impl Future<Output = capnp::Result<()>>,
        abort: impl Future<Output = Error>,
    ) -> Result<(), RunError<R::Error, W::Error>> {
        self.run_connection(
            messages,
            queue,
            &mut transport::Bytes {
                reader,
                make: make_import,
            },
            &mut transport::Writer(writer),
            scratch,
            &NoResources,
            controls,
            shutdown,
            abort,
        )
        .await
    }

    /// Resource-bearing version of run_stream. Attachments follow the same
    /// acceptance cursor while buffered output drains. Input resources are
    /// released before waiting for drain/close, even if shutdown later blocks.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_connection<
        O: Output<I>,
        R: Input<I>,
        W: io::WriteResources<O::Bundle> + io::Close,
    >(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        shutdown: impl Future<Output = capnp::Result<()>>,
        abort: impl Future<Output = Error>,
    ) -> Result<(), RunError<R::Error, W::Error>> {
        let abort = std::pin::pin!(abort);
        let mut abort = std::pin::pin!(notified::Notified::new(abort));
        let shutdown = std::pin::pin!(shutdown);
        let mut shutdown = std::pin::pin!(notified::Notified::new(shutdown));
        let mut pending = None;
        let outcome = {
            let stop = std::future::poll_fn(|cx| {
                if let Poll::Ready(error) = abort.as_mut().poll(cx) {
                    return Poll::Ready(Err(error));
                }
                shutdown.as_mut().poll(cx)
            });
            self.drive(
                messages,
                queue,
                reader,
                writer,
                scratch,
                output,
                controls,
                std::pin::pin!(stop),
                &mut pending,
            )
            .await
        };
        reader.finish_frame();
        if let Some(frame) = pending {
            frame.release(messages);
        }
        // Stop owned commands before a potentially blocked drain. In-flight
        // protocol owners remain alive until all transport polling has ended.
        if let Some(registration) = &self.registration {
            registration.detach();
        }
        self.commands.close(match &outcome {
            Err(Cause::Protocol(error)) => error.clone(),
            _ => Error::disconnected("connection shutting down".into()),
        });
        let outcome = match outcome {
            Err(error) => Err(error),
            Ok(()) => {
                let mut drained = false;
                std::future::poll_fn(|cx| {
                    if let Poll::Ready(error) = abort.as_mut().poll(cx) {
                        return Poll::Ready(Err(Cause::Protocol(error)));
                    }
                    if !drained {
                        match self
                            .output
                            .poll(writer, queue, messages, cx, self.options.budget)
                        {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Err(error)) => {
                                return Poll::Ready(Err(Cause::Output(error)))
                            }
                            Poll::Ready(Ok(())) => drained = true,
                        }
                    }
                    io::poll_close(writer, cx, self.options.budget).map_err(Cause::Close)
                })
                .await
            }
        };
        self.terminate::<O, _, _>(messages, queue, controls, outcome)
            .await
    }

    async fn terminate<O: Output<I>, R, W>(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        outcome: Result<(), Cause<R, W>>,
    ) -> Result<(), RunError<R, W>> {
        let reason = match &outcome {
            Err(Cause::Protocol(error)) => error.clone(),
            _ => Error::disconnected("connection stopped".into()),
        };
        // All IO borrows (including the pending receive writer) have ended.
        self.wake.parent.take();
        self.policy.close();
        self.commands.close(reason.clone());
        let mut client_reason = reason.clone();
        client_reason.kind = capnp::ErrorKind::Disconnected;
        self.hooks.disconnect(client_reason.clone());
        self.driver.disconnect(reason.kind);
        controls.shutdown(&mut self, messages);
        self.calls
            .terminate_with_error(&mut self.driver, messages, client_reason);
        self.worker.terminate(&mut self.driver, messages);
        self.foreign.terminate();
        self.hooks.close(&mut self.driver.runtime, reason);
        let mut closing = self.driver.terminate(queue);
        let cleanup =
            std::future::poll_fn(|cx| match closing.poll(messages, self.options.budget) {
                Ok(connection::Termination::Done) => Poll::Ready(Ok(())),
                Err(error) => Poll::Ready(Err(error)),
                Ok(_) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
        let result = match (outcome, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (outcome, cleanup) => Err(RunError {
                cause: outcome.err(),
                cleanup: cleanup.err(),
            }),
        };
        if let Some(registration) = self.registration.take() {
            registration.complete(&result);
        }
        result
    }

    #[inline(always)]
    fn poll_stop(
        &mut self,
        mut stop: Pin<&mut impl Future<Output = capnp::Result<()>>>,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<()>> {
        // Caller abort/deadline keeps priority over an orderly network stop.
        match stop.as_mut().poll(cx) {
            Poll::Ready(result) => Poll::Ready(result),
            Poll::Pending => self
                .registration
                .as_mut()
                .map_or(Poll::Pending, |registration| registration.poll(cx)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn drive<O: Output<I>, R: Input<I>, W: io::WriteResources<O::Bundle>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        mut stop: Pin<&mut impl Future<Output = capnp::Result<()>>>,
        pending: &mut Option<Frame<'p>>,
    ) -> Result<(), Cause<R::Error, W::Error>> {
        let mut frames = 0;
        let mut returned = None;
        let mut blocked = false;
        loop {
            // Buffered transports can admit and prepare their next frame in
            // one polling state. Keep the same yields and budgets as byte IO,
            // without suspending a separate future at each envelope boundary.
            let prepared = std::future::poll_fn(|cx| loop {
                // A Busy envelope is retried intact before reading another one.
                // Its original producer is returned only on non-Busy rejections.
                if pending.is_some() {
                    if let Poll::Ready(result) = self.poll_stop(stop.as_mut(), cx) {
                        return Poll::Ready(result.map(|_| None).map_err(Cause::Protocol));
                    }
                    if blocked {
                        match self
                            .poll_io_work(messages, queue, writer, scratch, output, controls, cx)
                        {
                            Err(error) => return Poll::Ready(Err(error)),
                            Ok(true) => return Poll::Pending,
                            Ok(false) => (),
                        }
                    }
                    let frame = pending.take().expect("pending admission");
                    let event = match self.driver.receive_with_policy(
                        frame,
                        messages,
                        scratch,
                        reader,
                        |words| {
                            Ok(server::Results::with_charge(
                                self.policy.charge(words)?,
                                self.hooks.inbox.message_pool.clone(),
                            ))
                        },
                    ) {
                        Ok(event) => {
                            reader.finish_frame();
                            blocked = false;
                            event
                        }
                        Err(rejected) => {
                            *pending = Some(rejected.frame);
                            if rejected.error == connection::Error::Busy {
                                // The driver rejects Busy before invoking its producer factory.
                                debug_assert!(rejected.producer.is_none());
                                if !blocked {
                                    blocked = true;
                                    cx.waker().wake_by_ref();
                                }
                                return Poll::Pending;
                            } else {
                                return Poll::Ready(Err(Cause::Protocol(wire_error(
                                    rejected.error,
                                ))));
                            }
                        }
                    };
                    if let connection::Received::Aborted(kind) = event {
                        return Poll::Ready(Err(Cause::Protocol(Error::from_kind(kind))));
                    }
                    self.hooks.collect_wire_optional(
                        &mut self.driver,
                        |owner| output.metadata_if_present(owner),
                        foreign::local_metadata,
                    );
                    let first = if matches!(
                        event,
                        connection::Received::Queued(_) | connection::Received::DeferredCall(_)
                    ) {
                        1
                    } else {
                        2
                    };
                    if let connection::Received::Returned(key) = &event {
                        returned = Some(*key);
                    }
                    controls
                        .received(event, self, messages, queue)
                        .map_err(Cause::Protocol)?;
                    self.resume_at(first);
                }
                // Shutdown must not need a fresh input buffer. A full pool can still
                // drain accepted output and release its protocol owners normally.
                if let Poll::Ready(result) = self.poll_stop(stop.as_mut(), cx) {
                    return Poll::Ready(result.map(|()| None).map_err(Cause::Protocol));
                }
                if let Some(key) = returned.take() {
                    if self
                        .calls
                        .returned(
                            key,
                            &mut self.hooks,
                            &mut self.driver,
                            messages,
                            cx,
                            |hooks, driver| {
                                hooks.collect_wire_optional(
                                    driver,
                                    |owner| output.metadata_if_present(owner),
                                    foreign::local_metadata,
                                )
                            },
                        )
                        .map_err(Cause::Protocol)?
                    {
                        // Let the application observe the reply and enqueue its
                        // next call before scanning the remaining lanes. The
                        // scheduled continuation also registers input readiness.
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                }
                match self.poll_io_work(messages, queue, writer, scratch, output, controls, cx) {
                    Err(error) => return Poll::Ready(Err(error)),
                    Ok(true) => return Poll::Pending,
                    Ok(false) => (),
                }
                std::task::ready!(self.policy.poll_ready(cx));
                let size = std::task::ready!(reader.poll_frame_size(cx)).map_err(|error| {
                    Cause::Input(tasks::ReceiveError::Input(
                        capntproto_rpc_wire::io::ReceiveError::Io(error),
                    ))
                })?;
                let size = size.unwrap_or(self.options.input_bytes.get()).max(8);
                if size > self.options.input_bytes.get() {
                    return Poll::Ready(Err(Cause::Protocol(Error::from_kind(
                        capnp::ErrorKind::BufferNotLargeEnough,
                    ))));
                }
                if let Some(frame) = reader
                    .take_buffered_frame(messages, self.options.reader)
                    .map_err(|error| Cause::Protocol(wire_error(error)))?
                {
                    *pending = Some(frame);
                    frames += 1;
                    if frames < self.options.budget.get().min(8) {
                        continue;
                    }
                    frames = 0;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                return Poll::Ready(Ok(Some(size)));
            })
            .await?;
            let Some(size) = prepared else { return Ok(()) };
            let frame = {
                let reservation = messages
                    .reserve_exact(size)
                    .map_err(|e| Cause::Protocol(wire_error(e)))?;
                let writing = match messages.write_owned(reservation) {
                    Ok(writing) => writing,
                    Err(rejected) => {
                        messages.discard(rejected.writer);
                        return Err(Cause::Protocol(wire_error(rejected.error)));
                    }
                };
                let cancel = Cell::new(None);
                let receive = tasks::receive_until(
                    writing,
                    reader,
                    self.options.reader,
                    self.options.budget,
                    |_| cancel.get().map_or(Poll::Pending, Poll::Ready),
                );
                let mut receive = std::pin::pin!(receive);
                let mut first_read = true;
                std::future::poll_fn(|cx| {
                    let end = match self.poll_stop(stop.as_mut(), cx) {
                        Poll::Ready(result) => Some(result.map_err(Cause::Protocol)),
                        Poll::Pending => {
                            // Prepared frame storage commonly already contains the
                            // complete input. Admit it before rescanning work which
                            // cannot observe this frame yet. The outer frame budget
                            // still bounds a continuously readable peer.
                            if first_read {
                                first_read = false;
                                if let Poll::Ready(received) = receive.as_mut().poll(cx) {
                                    return Poll::Ready(
                                        received.finish(messages).map_err(Cause::Input),
                                    );
                                }
                            }
                            match self.poll_io_work(
                                messages, queue, writer, scratch, output, controls, cx,
                            ) {
                                Err(error) => Some(Err(error)),
                                Ok(true) => return Poll::Pending,
                                Ok(false) => None,
                            }
                        }
                    };
                    if let Some(end) = end {
                        cancel.set(Some(capnp::ErrorKind::Disconnected));
                        // Cancellation is polled before IO and must return the writer now.
                        let Poll::Ready(received) = receive.as_mut().poll(cx) else {
                            unreachable!("ready cancellation completes the receive")
                        };
                        let _ = received.finish(messages);
                        return Poll::Ready(end.map(|_| None));
                    }
                    receive
                        .as_mut()
                        .poll(cx)
                        .map(|received| received.finish(messages).map_err(Cause::Input))
                })
                .await?
            };
            let Some(frame) = frame else { return Ok(()) };
            *pending = Some(frame);
            // Receiving bytes does not change protocol work. Invalidate idle
            // scans after admission above, when the frame changes the ledgers.
            // Otherwise every input causes a full scan of the old state first.
            // Bound an always-readable peer without putting an executor round
            // trip in front of each already-received call or reply.
            frames += 1;
            if frames < self.options.budget.get().min(8) {
                continue;
            }
            frames = 0;
            let mut yielded = false;
            std::future::poll_fn(|cx| {
                if yielded {
                    Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn poll_io_work<R, O: Output<I>, W: io::WriteResources<O::Bundle>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        cx: &mut Context<'_>,
    ) -> Result<bool, Cause<R, W::Error>> {
        // Publication can end a lane scan early to flush promptly. Continue
        // its unused budget after flushing, instead of scheduling an otherwise
        // idle executor turn. All passes share the same cooperative allowance.
        let mut remaining = self.options.budget.get();
        loop {
            let queued = queue.len();
            if queued != 0 && self.idle {
                self.idle = false;
                controls.set_idle(false);
            }
            let mut output_ready =
                match self
                    .output
                    .poll(writer, queue, messages, cx, self.options.budget)
                {
                    Poll::Ready(Err(e)) => return Err(Cause::Output(e)),
                    Poll::Ready(Ok(())) => true,
                    Poll::Pending => false,
                };
            // Publication already committed protocol state and invalidated scans.
            // Draining bytes only reactivates lanes which waited for FIFO capacity;
            // resource/application notifications still arrive through WorkWake.
            if queue.len() < queued {
                self.control_idle = false;
                if std::mem::take(&mut self.output_blocked) {
                    self.changed();
                }
            }
            // poll_work_budget registers the parent before consuming changed,
            // including on idle polls. A control callback can wake before that
            // registration: its changed flag is consumed later in this same
            // turn. Registering here as well only repeats the atomic operation.
            if self.wake.changed.load(Ordering::Acquire) {
                self.control_idle = false;
            }
            if !self.control_idle {
                // Controls observe both protocol progress and independently woken
                // application futures. IO-only polls can retain their idle result.
                // Use the same notifying waker as the other application lanes; the
                // poll_work check below also catches a wake during this callback.
                let waker = Waker::from(self.wake.clone());
                let mut control_cx = Context::from_waker(&waker);
                match controls.poll(self, messages, queue, scratch, output, &mut control_cx) {
                    Poll::Ready(Ok(true)) => {
                        self.changed();
                        cx.waker().wake_by_ref();
                    }
                    Poll::Ready(Err(e)) => return Err(Cause::Protocol(e)),
                    _ => self.control_idle = true,
                }
            }
            if let Poll::Ready(result) =
                self.poll_work_budget(messages, queue, scratch, output, cx, &mut remaining)
            {
                result.map_err(Cause::Protocol)?;
            }
            // Publish bytes produced by this turn without an executor round trip.
            // Physical flush remains owned by Output, including a Pending receipt.
            if !queue.is_empty() {
                // This turn may have published while the transport was idle.
                // Wake it before handing over any frame, including network controls.
                if self.idle {
                    self.idle = false;
                    controls.set_idle(false);
                }
                let queued = queue.len();
                // New publication invalidates the earlier empty-output result.
                // Queue acceptance can finish while a physical flush is Pending;
                // advertising idle then could close a transport with live output.
                output_ready =
                    match self
                        .output
                        .poll(writer, queue, messages, cx, self.options.budget)
                    {
                        Poll::Ready(Err(error)) => return Err(Cause::Output(error)),
                        Poll::Ready(Ok(())) => true,
                        Poll::Pending => false,
                    };
                if queue.len() < queued {
                    self.control_idle = false;
                    if std::mem::take(&mut self.output_blocked) {
                        self.changed();
                    }
                }
            }
            let idle = self.active == 0
                && output_ready
                && queue.is_empty()
                && self.commands.is_empty()
                && self.calls.is_empty()
                && self.worker.is_empty()
                && self.foreign.is_empty()
                && self.driver.is_idle();
            if self.idle != idle {
                self.idle = idle;
                controls.set_idle(idle);
            }
            let delivered = std::mem::take(&mut self.delivered);
            if delivered {
                cx.waker().wake_by_ref();
            }
            if !delivered && (self.active != 0 || !self.control_idle) {
                if remaining != 0 {
                    continue;
                }
                cx.waker().wake_by_ref();
            }
            return Ok(delivered);
        }
    }
}

#[cfg(test)]
mod tests;

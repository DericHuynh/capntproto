//! Connection entry points, cancellation and terminal ownership cleanup.
use super::*;

impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
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
            .await
            .map_err(|error| *error);
        if let Err(Cause::Protocol(error)) = &outcome {
            self.disconnect_clients(error);
        }
        reader.stop();
        if let Some(frame) = pending {
            frame.release(messages);
        }
        self.terminate::<O, _, _>(messages, queue, controls, outcome)
            .await
    }

    /// Managed byte stream: orderly stop/EOF drains accepted frames, flushes and
    /// closes the write half. Active connections attempt a terminal Abort, like
    /// legacy. No new application work is admitted during drain.
    /// The separate abort future supplies a deadline or immediate cancellation;
    /// it remains polled during writes, flush and close. Immediate abort skips
    /// further IO; protocol failures only attempt output which is ready now.
    /// All exits return protocol storage before the caller drops the IO.
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
        self,
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
        let (closing, result) = self
            .prepare_connection(
                messages,
                queue,
                reader,
                writer,
                scratch,
                output,
                controls,
                shutdown.as_mut(),
                abort.as_mut(),
                terminal_reason,
            )
            .await;
        closing
            .finish(messages, queue, writer, abort.as_mut(), result)
            .await
    }

    /// Retire protocol ownership while keeping accepted publications with IO.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn prepare_connection<
        O: Output<I>,
        R: Input<I>,
        W: io::WriteResources<O::Bundle>,
    >(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        mut shutdown: Pin<&mut impl Future<Output = capnp::Result<()>>>,
        mut abort: Pin<&mut impl Future<Output = Error>>,
        project_reason: impl FnOnce(&Result<(), Cause<R::Error, W::Error>>) -> Error,
    ) -> (Closing, Result<(), RunError<R::Error, W::Error>>) {
        let mut pending = None;
        let mut hard_abort = false;
        let outcome = {
            let stop = std::future::poll_fn(|cx| {
                if let Poll::Ready(error) = abort.as_mut().poll(cx) {
                    hard_abort = true;
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
            .map_err(|error| *error)
        };
        let reason = project_reason(&outcome);
        self.disconnect_clients(&reason);
        reader.stop();
        if let Some(frame) = pending {
            frame.release(messages);
        }
        let (mut closing, result) = self
            .retire::<O, _, _>(messages, controls, outcome, reason)
            .await;
        closing.hard_abort = hard_abort;
        (closing, result)
    }

    pub(super) async fn terminate<O: Output<I>, R, W>(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        outcome: Result<(), Cause<R, W>>,
    ) -> Result<(), RunError<R, W>> {
        let reason = terminal_reason(&outcome);
        self.disconnect_clients(&reason);
        let (closing, result) = self
            .retire::<O, _, _>(messages, controls, outcome, reason)
            .await;
        let result = with_cleanup(
            result,
            discard_output(messages, queue, closing.options.budget).await,
        );
        closing.complete(&result);
        result
    }

    // All public routes are already broken before releasing callback-bearing
    // owners. The final output allocation has its own queue/transport owner.
    async fn retire<O: Output<I>, R, W>(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        outcome: Result<(), Cause<R, W>>,
        reason: Error,
    ) -> (Closing, Result<(), RunError<R, W>>) {
        self.notifications.detach();
        let mut client_reason = reason.clone();
        client_reason.kind = capnp::ErrorKind::Disconnected;
        for pending in self.native_tails.drain(..) {
            self.driver.release_question(pending.retained);
        }
        controls.shutdown(&mut ControlContext::new(&mut self), messages);
        self.calls
            .terminate_with_error(&mut self.driver, messages, client_reason);
        self.worker.terminate(&mut self.driver, messages);
        self.foreign.terminate(&mut self.driver.capabilities());
        self.hooks
            .close(&mut self.driver.capabilities(), reason.clone());
        let mut protocol = self.driver.terminate();
        let cleanup =
            std::future::poll_fn(|cx| match protocol.poll(messages, self.options.budget) {
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
                close: None,
                cleanup: cleanup.err(),
            }),
        };
        (
            Closing {
                output: self.output,
                options: self.options,
                policy: self.policy,
                registration: self.registration,
                idle: self.idle,
                reason,
                hard_abort: false,
            },
            result,
        )
    }

    fn disconnect_clients(&mut self, error: &Error) {
        if let Some(registration) = &self.registration {
            registration.detach();
        }
        self.policy.close();
        let mut reason = error.clone();
        reason.kind = capnp::ErrorKind::Disconnected;
        self.commands.close(reason.clone());
        self.hooks.disconnect(reason);
        self.driver.disconnect(error.kind);
    }

    #[inline(always)]
    pub(super) fn poll_stop(
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

    // Every wire refresh uses the same concrete metadata projection. Share its
    // ownership/settlement walk across lanes instead of cloning that code into
    // each caller, while retaining static dispatch inside the walk.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn drive<O: Output<I>, R: Input<I>, W: io::WriteResources<O::Bundle>>(
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
    ) -> Result<(), Box<Cause<R::Error, W::Error>>> {
        // All application lanes use one stable wake identity. Borrow it across
        // polls rather than incrementing an atomic Arc count for every lane.
        let waker = self.notifications.producer(WorkSet::CALLS);
        let mut pump = Pump::default();
        loop {
            // Buffered transports can admit and prepare their next frame in
            // one polling state. Keep the same yields and budgets as byte IO,
            // without suspending a separate future at each envelope boundary.
            let prepared = std::future::poll_fn(|cx| {
                let result = self.poll_prepared(
                    messages,
                    queue,
                    reader,
                    writer,
                    scratch,
                    output,
                    controls,
                    stop.as_mut(),
                    pending,
                    &mut pump,
                    cx,
                    &waker,
                );
                // A suspended poll already yielded to the executor. Count only
                // frames processed without suspension toward fairness.
                if result.is_pending() {
                    pump.frames = 0;
                }
                result
            })
            .await?;
            let Some(size) = prepared else { return Ok(()) };
            let frame = {
                let receiving = messages
                    .receive(size)
                    .map_err(|e| Cause::Protocol(wire_error(e)))?;
                let cancel = Cell::new(None);
                let receive = tasks::receive_until(
                    receiving,
                    reader,
                    self.options.reader,
                    self.options.budget,
                    |_| cancel.get().map_or(Poll::Pending, Poll::Ready),
                );
                let mut receive = std::pin::pin!(receive);
                let mut first_read = true;
                std::future::poll_fn(|cx| {
                    let result = (|| {
                        let end = match self.poll_stop(stop.as_mut(), cx) {
                            Poll::Ready(result) => {
                                Some(result.map_err(|error| Box::new(Cause::Protocol(error))))
                            }
                            Poll::Pending => {
                                // Prepared frame storage commonly already contains the
                                // complete input. Admit it before rescanning work which
                                // cannot observe this frame yet. The outer frame budget
                                // still bounds a continuously readable peer.
                                if first_read {
                                    first_read = false;
                                    if let Poll::Ready(received) = receive.as_mut().poll(cx) {
                                        return Poll::Ready(
                                            received
                                                .finish(messages)
                                                .map_err(|error| Box::new(Cause::Input(error))),
                                        );
                                    }
                                }
                                match self.poll_io_work_using(
                                    messages, queue, writer, scratch, output, controls, cx, &waker,
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
                        receive.as_mut().poll(cx).map(|received| {
                            received
                                .finish(messages)
                                .map_err(|error| Box::new(Cause::Input(error)))
                        })
                    })();
                    if result.is_pending() {
                        pump.frames = 0;
                    }
                    result
                })
                .await?
            };
            let Some(frame) = frame else { return Ok(()) };
            *pending = Some(frame);
            // Admission charges the frame budget and invalidates protocol
            // scans. Preparing bytes alone must not reset that budget before
            // the frame is consumed, including across a mixed reply batch.
        }
    }

    // Only private, descriptor-free requests qualify for lookahead.
    // Inspecting an arbitrary ParamsHook while borrowing the inbox could invoke
    // application code. This estimate conservatively includes a maximum target
    // transform and avoids retaining output across configured pool pressure.
}

/// Transport completion after every protocol and application owner has retired.
/// No graph, answer table, import, export or service task remains in this phase.
pub(super) struct Closing {
    output: io::Output,
    options: Options,
    policy: Rc<policy::Policy>,
    registration: Option<network::Registration>,
    idle: bool,
    pub(super) reason: Error,
    pub(super) hard_abort: bool,
}
impl Closing {
    pub(super) async fn finish<'p, B: Default, R, W: io::WriteResources<B> + io::Close>(
        mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, B>,
        writer: &mut W,
        mut abort: Pin<&mut impl Future<Output = Error>>,
        mut result: Result<(), RunError<R, W::Error>>,
    ) -> Result<(), RunError<R, W::Error>> {
        // Cleanup can yield. Recheck the caller deadline before even the
        // best-effort terminal publication performs IO.
        let aborted = if self.hard_abort {
            Poll::Pending
        } else {
            std::future::poll_fn(|cx| Poll::Ready(abort.as_mut().poll(cx))).await
        };
        if let Poll::Ready(error) = aborted {
            self.hard_abort = true;
            result = Err(RunError {
                cause: Some(Cause::Protocol(error)),
                close: None,
                cleanup: result.err().and_then(|error| error.cleanup),
            });
        }
        let output_was_failed = queue.is_failed();
        if result
            .as_ref()
            .err()
            .is_none_or(|error| error.cleanup.is_none())
            && !self.hard_abort
            && !self.idle
        {
            // Like legacy, encode diagnostics only after exports have dropped.
            let trace = self.policy.trace(&self.reason);
            if let Ok(minimum) =
                super::super::exception::publication_bytes(&self.reason, trace.as_deref())
            {
                if owned::controls::send_abort(
                    messages,
                    queue,
                    self.options.reader,
                    minimum,
                    |output| super::super::exception::write(&self.reason, trace.as_deref(), output),
                )
                .is_ok()
                {
                    std::future::poll_fn(|cx| {
                        let _ = self
                            .output
                            .poll(writer, queue, messages, cx, self.options.budget);
                        Poll::Ready(())
                    })
                    .await;
                }
            }
        }
        let mut closed = false;
        if !self.hard_abort {
            // A failed RPC still owns a physical close receipt. Only caller
            // cancellation skips IO; protocol/write failure uses abort-close.
            let mut graceful = result.is_ok() && (output_was_failed || !queue.is_failed());
            let mut draining = graceful;
            let mut canceled = None;
            let close = std::future::poll_fn(|cx| {
                if let Poll::Ready(error) = abort.as_mut().poll(cx) {
                    canceled = Some(error);
                    return Poll::Ready(Ok(()));
                }
                if draining {
                    match self
                        .output
                        .poll(writer, queue, messages, cx, self.options.budget)
                    {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Ok(())) => draining = false,
                        Poll::Ready(Err(error)) => {
                            result = Err(RunError {
                                cause: Some(Cause::Output(error)),
                                close: None,
                                cleanup: None,
                            });
                            graceful = false;
                            draining = false;
                        }
                    }
                }
                if graceful {
                    io::poll_close(writer, cx, self.options.budget)
                } else {
                    io::poll_abort(writer, cx, self.options.budget)
                }
            })
            .await;
            if let Some(error) = canceled {
                result = Err(RunError {
                    cause: Some(Cause::Protocol(error)),
                    close: None,
                    cleanup: result.err().and_then(|error| error.cleanup),
                });
            } else {
                match close {
                    Ok(()) => closed = true,
                    Err(error) => match &mut result {
                        Ok(()) => {
                            result = Err(RunError {
                                cause: None,
                                close: Some(error),
                                cleanup: None,
                            })
                        }
                        Err(result) => result.close = Some(error),
                    },
                }
            }
        }
        let result = with_cleanup(
            result,
            discard_output(messages, queue, self.options.budget).await,
        );
        if let Some(registration) = self.registration.take() {
            registration.complete_physical(&result, closed);
        }
        result
    }
    fn complete<R, W>(mut self, result: &Result<(), RunError<R, W>>) {
        if let Some(registration) = self.registration.take() {
            registration.complete(result);
        }
    }
}

pub(super) fn terminal_reason<R, W>(outcome: &Result<(), Cause<R, W>>) -> Error {
    match outcome {
        Err(Cause::Protocol(error)) => error.clone(),
        Ok(()) => Error::disconnected("client requested disconnect".into()),
        _ => Error::disconnected("connection stopped".into()),
    }
}

async fn discard_output<'p, B>(
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_, B>,
    budget: NonZeroUsize,
) -> Result<(), connection::Error> {
    queue.fail();
    std::future::poll_fn(|cx| {
        for _ in 0..budget.get() {
            match queue.discard_one(messages) {
                Ok(true) => (),
                Ok(false) => return Poll::Ready(Ok(())),
                Err(error) => {
                    return Poll::Ready(Err(connection::Error::Input(owned::Error::Storage(error))))
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    })
    .await
}

fn with_cleanup<R, W>(
    mut result: Result<(), RunError<R, W>>,
    cleanup: Result<(), connection::Error>,
) -> Result<(), RunError<R, W>> {
    if let Err(error) = cleanup {
        match &mut result {
            Ok(()) => {
                return Err(RunError {
                    cause: None,
                    close: None,
                    cleanup: Some(error),
                })
            }
            Err(result) => {
                result.cleanup.get_or_insert(error);
            }
        }
    }
    result
}

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
            .map_err(|error| *error)
        };
        reader.finish_frame();
        if let Some(frame) = pending {
            frame.release(messages);
        }
        if let Err(Cause::Protocol(error)) = &outcome {
            if error.kind != capnp::ErrorKind::Disconnected
                && self.driver.disconnected().is_none()
                && self
                    .driver
                    .send_abort(
                        messages,
                        queue,
                        self.options.reader,
                        error.kind,
                        &error.extra,
                    )
                    .is_ok()
            {
                // Flush what the transport can accept immediately. Terminal
                // notification must not wait forever on a broken/blocked peer.
                std::future::poll_fn(|cx| {
                    let _ = self
                        .output
                        .poll(writer, queue, messages, cx, self.options.budget);
                    Poll::Ready(())
                })
                .await;
            }
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

    pub(super) async fn terminate<O: Output<I>, R, W>(
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
        self.notifications.detach();
        self.policy.close();
        self.commands.close(reason.clone());
        let mut client_reason = reason.clone();
        client_reason.kind = capnp::ErrorKind::Disconnected;
        self.hooks.disconnect(client_reason.clone());
        self.driver.disconnect(reason.kind);
        for pending in self.native_tails.drain(..) {
            self.driver.release_question(pending.retained);
        }
        controls.shutdown(&mut ControlContext::new(&mut self), messages);
        self.calls
            .terminate_with_error(&mut self.driver, messages, client_reason);
        self.worker.terminate(&mut self.driver, messages);
        self.foreign.terminate(&mut self.driver.capabilities());
        self.hooks.close(&mut self.driver.capabilities(), reason);
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

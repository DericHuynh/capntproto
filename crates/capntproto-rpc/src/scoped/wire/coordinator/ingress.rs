//! Frame admission and result delivery; retains Busy input until retry.
use super::*;

impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
    #[inline(always)]
    pub(super) fn delivered_plain<O: Output<I>>(
        &mut self,
        index: usize,
        message: Box<capntproto_rpc_wire::owned::PlainResults>,
        controls: &impl Control<'p, 'id, 's, I, P, C, H, O>,
    ) {
        self.calls.deliver_plain(index, message);
        if controls.preserves_idle(Activity::PlainReturn) {
            // This fused transition removes an original data-only question,
            // requires no Finish, and admits no grants or pipeline work. It
            // cannot enable another call or invalidate a partial idle scan.
            // Completion, endpoint retirement and newly enqueued work retain
            // their independent registered wakeups. Restarting the call scan
            // here would revisit every outstanding owner after every reply.
            if self.driver.runtime.has_retired() {
                self.driver.invalidate_maintenance();
                self.schedule.active |= WorkSet::MAINTENANCE;
            }
        } else {
            self.resume_at(Lane::Calls);
        }
    }

    // Keep the ordinary buffered pump out of the byte-reader async state
    // machine. Both paths retain the same cooperative budget and stop ordering.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_prepared<O: Output<I>, R: Input<I>, W: io::WriteResources<O::Bundle>>(
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
        pump: &mut Pump<'id>,
        cx: &mut Context<'_>,
        waker: &Waker,
    ) -> Poll<Result<Option<usize>, Box<Cause<R::Error, W::Error>>>> {
        loop {
            if queue.is_empty() && self.output.is_clean() {
                pump.output_frames = 0;
            }
            // A Busy envelope is retried intact before reading another one.
            // Its original producer is returned only on non-Busy rejections.
            if pending.is_some() {
                if let Poll::Ready(result) = self.poll_stop(stop.as_mut(), cx) {
                    return Poll::Ready(
                        result
                            .map(|_| None)
                            .map_err(|error| Box::new(Cause::Protocol(error))),
                    );
                }
                if pump.blocked {
                    match self.poll_io_work_using(
                        messages, queue, writer, scratch, output, controls, cx, waker,
                    ) {
                        Err(error) => return Poll::Ready(Err(error)),
                        Ok(true) => return Poll::Pending,
                        Ok(false) => (),
                    }
                }
                let frame = pending.take().expect("pending admission");
                let frame = if controls.accepts_direct_plain_return()
                    && self.calls.has_plain_receivers()
                {
                    match self.driver.try_receive_plain_owned(frame, messages, |key| {
                        self.calls.plain_receiver(key)
                    }) {
                        Ok(connection::PlainReceived::Delivered(index, message)) => {
                            reader.finish_frame();
                            pump.frames += 1;
                            pump.output_frames += usize::from(!queue.is_empty());
                            pump.blocked = false;
                            self.delivered_plain(index, message, controls);
                            if pump.frames < self.options.budget.get().min(8)
                                && (self.calls.has_indexed_replies() || !self.worker.is_empty())
                            {
                                continue;
                            }
                            pump.frames = 0;
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        Ok(connection::PlainReceived::Deferred(frame)) => frame,
                        Err((error, frame)) => {
                            *pending = Some(frame);
                            return Poll::Ready(Err(Box::new(Cause::Protocol(wire_error(error)))));
                        }
                    }
                } else {
                    frame
                };
                let was_idle = self.schedule.active == WorkSet::NONE
                    && self.control_idle
                    && !self.output_blocked;
                let event = match self.driver.receive_with_policy(
                    frame,
                    messages,
                    scratch,
                    reader,
                    |words| {
                        Ok(self.worker.results(
                            self.policy.charge(words)?,
                            self.hooks.inbox.message_pool.clone(),
                        ))
                    },
                ) {
                    Ok(event) => {
                        reader.finish_frame();
                        pump.frames += 1;
                        pump.output_frames += usize::from(!queue.is_empty());
                        pump.blocked = false;
                        event
                    }
                    Err(rejected) => {
                        *pending = Some(rejected.frame);
                        if rejected.error == connection::Error::Busy {
                            // The driver rejects Busy before invoking its producer factory.
                            debug_assert!(rejected.producer.is_none());
                            if !pump.blocked {
                                pump.blocked = true;
                                cx.waker().wake_by_ref();
                            }
                            return Poll::Pending;
                        } else {
                            return Poll::Ready(Err(Box::new(Cause::Protocol(wire_error(
                                rejected.error,
                            )))));
                        }
                    }
                };
                if let connection::Received::Aborted(kind) = event {
                    return Poll::Ready(Err(Box::new(Cause::Protocol(Error::from_kind(kind)))));
                }
                if controls.observes_hooks(&event) {
                    Self::collect_hooks(&mut self.hooks, &mut self.driver, output);
                }
                let first = if matches!(
                    event,
                    connection::Received::Queued(_) | connection::Received::DeferredCall(_)
                ) {
                    Lane::Accept
                } else {
                    Lane::Calls
                };
                if let connection::Received::Returned(key) = &event {
                    pump.returned = Some(*key);
                }
                // Pending outgoing calls were already certified idle before
                // this descriptor-free request arrived. It does not settle
                // their questions. Application work still invalidates the
                // certificate through the registered notifier during dispatch.
                let plain_call = was_idle
                    && matches!(event, connection::Received::DeferredCall(_))
                    && self.driver.has_plain_pending_call()
                    && self.driver.runtime.queued() == 0
                    && self.worker.is_empty()
                    && self.foreign.is_empty()
                    && self.commands.is_empty();
                let retirement = was_idle
                    && matches!(
                        event,
                        connection::Received::Finished(_) | connection::Received::Released
                    )
                    && self.worker.is_empty()
                    // Reclaimed storage can unblock an outgoing admission or
                    // completion even without an application notification.
                    && self.calls.is_empty()
                    && self.foreign.is_empty()
                    && self.commands.is_empty()
                    && self.driver.runtime.queued() == 0
                    && !self.driver.has_pending_call()
                    && self.hooks.inbox.queue.borrow().is_empty();
                let mut context = ControlContext::new(self);
                let received = controls.received(event, &mut context, messages, queue);
                let preserves_application = !context.has_effects();
                drop(context);
                received.map_err(|error| Box::new(Cause::Protocol(error)))?;
                if retirement
                    && preserves_application
                    && controls.preserves_idle(Activity::Retirement)
                    && self.notifications.pending() == WorkSet::NONE
                {
                    // No application execution can progress from these owner
                    // drops. Reclaim their ledger/graph state without restarting
                    // every idle application scan. Destructors and independent
                    // control commands still invalidate through their wakers.
                    self.schedule.active = WorkSet::MAINTENANCE;
                    self.schedule.cursor = Lane::Maintenance;
                    self.driver.invalidate_maintenance();
                } else if plain_call
                    && preserves_application
                    && controls.preserves_idle(Activity::PlainCall)
                    && self.notifications.pending() == WorkSet::NONE
                {
                    self.schedule.active = (WorkSet::ACCEPT) | (WorkSet::WORKER);
                    self.schedule.cursor = Lane::Accept;
                    self.worker.invalidate_scan();
                } else if pump.returned.is_none() {
                    self.resume_at(first);
                }
            }
            // Shutdown must not need a fresh input buffer. A full pool can still
            // drain accepted output and release its protocol owners normally.
            if let Poll::Ready(result) = self.poll_stop(stop.as_mut(), cx) {
                return Poll::Ready(
                    result
                        .map(|()| None)
                        .map_err(|error| Box::new(Cause::Protocol(error))),
                );
            }
            if let Some(key) = pump.returned.take() {
                let completed = self
                    .calls
                    .returned(
                        key,
                        &mut self.hooks,
                        &mut self.driver,
                        messages,
                        cx,
                        Self::hook_refresh(output, true),
                    )
                    .map_err(|error| Box::new(Cause::Protocol(error)))?;
                if completed == Some(true) && controls.preserves_idle(Activity::PlainReturn) {
                    // Receipt-free completion changed only outgoing ownership
                    // and its cleanup. Preserve unrelated idle lane scans.
                    if self.schedule.active == WorkSet::NONE {
                        self.schedule.cursor = Lane::Calls;
                    }
                    self.schedule.active |= (WorkSet::CALLS) | (WorkSet::MAINTENANCE);
                    self.calls.invalidate_scan();
                    self.driver.invalidate_maintenance();
                } else {
                    self.resume_at(Lane::Calls);
                }
                if completed.is_some()
                    && self.worker.is_empty()
                    && (!reader.has_buffered_next_frame()
                        || pump.frames >= self.options.budget.get().min(8))
                {
                    // Let the application enqueue its next call before
                    // scanning cleanup when no bounded buffered successor is
                    // ready. Parent and child replies often share one read;
                    // consume them without an intervening executor turn.
                    pump.frames = 0;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
            }
            if pump.frames >= self.options.budget.get().min(8) {
                pump.frames = 0;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            // Batch replies to already-buffered input across the ordinary
            // fairness yields. Both output and admitted-input counts bound the
            // batch, so control-only traffic cannot indefinitely postpone it.
            // Never wait for new input or hold output across pool pressure.
            let defer_output = (pump.frames != 0 || pump.output_frames != 0 || !queue.is_empty())
                && pump.output_frames < 32
                && queue.len() < 32
                && messages.can_reserve(
                    self.options
                        .input_bytes
                        .get()
                        .max(self.options.output_bytes.get()),
                )
                && reader.has_buffered_next_frame()
                && self.policy.poll_ready(cx).is_ready();
            match self.poll_io_work_batch(
                messages,
                queue,
                writer,
                scratch,
                output,
                controls,
                cx,
                waker,
                defer_output,
            ) {
                Err(error) => return Poll::Ready(Err(error)),
                // A completed parent need not suspend before an already
                // buffered child reply. The frame budget above and readiness
                // gate below still bound and authorize the next admission.
                Ok(true) if defer_output => (),
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
                return Poll::Ready(Err(Box::new(Cause::Protocol(Error::from_kind(
                    capnp::ErrorKind::BufferNotLargeEnough,
                )))));
            }
            // Small frames amortize copying better than another retained owner.
            // Large complete frames can transfer their immutable allocation;
            // the ordinary admission path still validates every protocol field.
            if size > 32 * 1024 {
                if let Some(frame) = reader
                    .take_shared_frame(messages, self.options.reader)
                    .map_err(|error| Cause::Protocol(wire_error(error)))?
                {
                    *pending = Some(frame);
                    continue;
                }
            }
            if controls.accepts_direct_plain_return() && self.calls.has_plain_receivers() {
                if let Poll::Ready(result) = self.poll_stop(stop.as_mut(), cx) {
                    return Poll::Ready(
                        result
                            .map(|_| None)
                            .map_err(|error| Box::new(Cause::Protocol(error))),
                    );
                }
                if let Some(bytes) = reader.buffered_frame() {
                    if bytes.len() > self.options.input_bytes.get() {
                        return Poll::Ready(Err(Box::new(Cause::Protocol(Error::from_kind(
                            capnp::ErrorKind::BufferNotLargeEnough,
                        )))));
                    }
                    if let Some((index, message)) = self
                        .driver
                        .try_receive_plain_buffered(bytes, self.options.reader, messages, |key| {
                            self.calls.plain_receiver(key)
                        })
                        .map_err(|error| Box::new(Cause::Protocol(wire_error(error))))?
                    {
                        reader.consume_buffered_frame();
                        pump.output_frames += usize::from(!queue.is_empty());
                        self.delivered_plain(index, message, controls);
                        pump.frames += 1;
                        if pump.frames < self.options.budget.get().min(8)
                            && (self.calls.has_indexed_replies() || !self.worker.is_empty())
                        {
                            continue;
                        }
                        pump.frames = 0;
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                }
            }
            if let Some(frame) = reader
                .take_buffered_frame(messages, self.options.reader)
                .map_err(|error| Cause::Protocol(wire_error(error)))?
            {
                *pending = Some(frame);
                continue;
            }
            return Poll::Ready(Ok(Some(size)));
        }
    }
}

//! Work/output arbitration and physical flush policy over caller-owned IO.
use super::*;

impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn poll_io_work<R, O: Output<I>, W: io::WriteResources<O::Bundle>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        cx: &mut Context<'_>,
    ) -> Result<bool, Box<Cause<R, W::Error>>> {
        let waker = self.notifications.waker();
        self.poll_io_work_using(
            messages, queue, writer, scratch, output, controls, cx, &waker,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_io_work_using<R, O: Output<I>, W: io::WriteResources<O::Bundle>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        cx: &mut Context<'_>,
        waker: &Waker,
    ) -> Result<bool, Box<Cause<R, W::Error>>> {
        self.poll_io_work_batch(
            messages, queue, writer, scratch, output, controls, cx, waker, false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_io_work_batch<R, O: Output<I>, W: io::WriteResources<O::Bundle>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        cx: &mut Context<'_>,
        waker: &Waker,
        defer_output: bool,
    ) -> Result<bool, Box<Cause<R, W::Error>>> {
        // IO readiness alone changes no protocol state. Preserve a completed
        // scan while waiting for the next frame, including the parent waker
        // registration. Pending flushes and newly queued output still run.
        if self.schedule.active == WorkSet::NONE
            && !self.delivered
            && self.control_idle
            && self.notifications.pending() == WorkSet::NONE
            && self.output.is_clean()
            && queue.is_empty()
            && !queue.is_failed()
            && self.notifications.same_parent(cx.waker())
        {
            return Ok(false);
        }
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
            let deferred_request =
                self.coalescing_output && queue.len() < 32 && self.can_batch_request(messages);
            let ready_work = !queue.is_empty()
                && remaining != 0
                && (self.schedule.active != WorkSet::NONE
                    || self.notifications.pending() != WorkSet::NONE)
                && queue.len() < 32
                && messages.can_reserve(self.options.output_bytes.get());
            let mut output_ready = if (defer_output || deferred_request || ready_work)
                && self.output.is_clean()
                && !self.output_blocked
            {
                false
            } else {
                match self
                    .output
                    .poll(writer, queue, messages, cx, self.options.budget)
                {
                    Poll::Ready(Err(e)) => return Err(Box::new(Cause::Output(e))),
                    Poll::Ready(Ok(())) => true,
                    Poll::Pending => false,
                }
            };
            // Publication already committed protocol state and invalidated scans.
            // Draining bytes only reactivates lanes which waited for FIFO capacity;
            // resource/application notifications still arrive through WorkWake.
            if queue.len() < queued || (output_ready && self.output_blocked) {
                self.control_idle = false;
                if std::mem::take(&mut self.output_blocked) {
                    self.changed();
                }
            }
            if output_ready && std::mem::take(&mut self.calls_waiting_for_output) {
                self.schedule.active |= WorkSet::CALLS;
                self.calls.invalidate_scan();
            }
            // poll_work_budget registers the parent before consuming changed,
            // including on idle polls. A control callback can wake before that
            // registration: its changed flag is consumed later in this same
            // turn. Registering here as well only repeats the atomic operation.
            if self.notifications.pending() & (!(WorkSet::CALLS) | WorkSet::CONTROL)
                != WorkSet::NONE
            {
                self.control_idle = false;
            }
            if !self.control_idle {
                // Controls observe both protocol progress and independently woken
                // application futures. IO-only polls can retain their idle result.
                let control_waker = self.control_waker.clone();
                let mut control_cx = Context::from_waker(&control_waker);
                let mut context = ControlContext::new(self);
                let result = controls.poll(
                    &mut context,
                    messages,
                    queue,
                    scratch,
                    output,
                    &mut control_cx,
                );
                drop(context);
                match result {
                    Poll::Ready(Ok(true)) => {
                        self.control_idle = false;
                        cx.waker().wake_by_ref();
                    }
                    Poll::Ready(Err(e)) => return Err(Box::new(Cause::Protocol(e))),
                    _ => self.control_idle = true,
                }
            }
            let control_was_idle = self.control_idle;
            self.direct_publication = false;
            let before_work = queue.len();
            if let Poll::Ready(result) =
                self.poll_work_budget(messages, queue, scratch, output, cx, &mut remaining, waker)
            {
                result.map_err(|error| Box::new(Cause::Protocol(error)))?;
            }
            let preserves_control = control_was_idle
                && self.direct_publication
                && controls.preserves_idle(Activity::PlainPublication);
            if preserves_control {
                self.control_idle = true;
            }
            if preserves_control
                && self.schedule.active == WorkSet::MAINTENANCE
                && remaining != 0
                && self.driver.maintenance_is_idle()
            {
                // An unflushed plain publication adds no cleanup obligation.
                // Certify that before deciding whether its buffered successor
                // permits a reply batch. Later IO notifications remain live.
                remaining -= 1;
                self.schedule.active = WorkSet::NONE;
            }
            // Carry a productive burst across cooperative yields, bounded by
            // frame count and known pool capacity. A quantum that publishes no
            // frame always flushes, so pending admission cannot hold old output.
            self.coalescing_output = queue.len() > before_work
                && !self.hooks.inbox.queue.borrow().is_empty()
                && queue.len() < 32
                && !self.delivered
                && !self.output_blocked
                && self.output.is_clean()
                && controls.preserves_idle(Activity::PlainPublication)
                && self.policy.poll_ready(cx).is_ready()
                && self.can_batch_request(messages);
            if !queue.is_empty() && self.idle {
                self.idle = false;
                controls.set_idle(false);
            }
            // Drain immediately ready work before writing, within the same
            // cooperative allowance. New notifications count as ready work.
            // Physical flush remains owned by Output, including a Pending receipt.
            if !queue.is_empty()
                && !(self.coalescing_output
                    || (remaining != 0
                        && (self.schedule.active != WorkSet::NONE
                            || self.notifications.pending() != WorkSet::NONE)
                        && queue.len() < 32
                        && self.output.is_clean()
                        && !self.output_blocked
                        && !self.delivered
                        && messages.can_reserve(self.options.output_bytes.get()))
                    || (defer_output
                        && queue.len() < 32
                        && !self.delivered
                        && !self.output_blocked
                        && self.policy.poll_ready(cx).is_ready()))
            {
                let queued = queue.len();
                // New publication invalidates the earlier empty-output result.
                // Queue acceptance can finish while a physical flush is Pending;
                // advertising idle then could close a transport with live output.
                output_ready =
                    match self
                        .output
                        .poll(writer, queue, messages, cx, self.options.budget)
                    {
                        Poll::Ready(Err(error)) => return Err(Box::new(Cause::Output(error))),
                        Poll::Ready(Ok(())) => true,
                        Poll::Pending => false,
                    };
                if queue.len() < queued || (output_ready && self.output_blocked) {
                    if !preserves_control {
                        self.control_idle = false;
                    }
                    if std::mem::take(&mut self.output_blocked) {
                        self.changed();
                    }
                }
            }
            if output_ready && std::mem::take(&mut self.calls_waiting_for_output) {
                self.schedule.active |= WorkSet::CALLS;
                self.calls.invalidate_scan();
            }
            let idle = self.schedule.active == WorkSet::NONE
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
            if !delivered
                && (self.schedule.active != WorkSet::NONE
                    || !self.control_idle
                    || self.notifications.pending() != WorkSet::NONE)
            {
                // A control-only self-wake consumes no application lane's
                // quantum. Yield it explicitly instead of spinning forever.
                if remaining != 0 && !(self.schedule.active == WorkSet::NONE && !self.control_idle)
                {
                    continue;
                }
                cx.waker().wake_by_ref();
            }
            return Ok(delivered);
        }
    }
}

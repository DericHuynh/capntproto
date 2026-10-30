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
        self.poll_io_work_turn(
            messages, queue, writer, scratch, output, controls, cx, waker, true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_io_work_turn<R, O: Output<I>, W: io::WriteResources<O::Bundle>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        cx: &mut Context<'_>,
        waker: &Waker,
        end_input_turn: bool,
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
        // Work and output completion share one cooperative allowance. A freed
        // resource may resume its owner within the same turn.
        let mut remaining = self.options.budget.get();
        loop {
            // poll_work_budget registers the parent before consuming changed,
            // including on idle polls. A control callback can wake before that
            // registration: its changed flag is consumed later in this same
            // turn. Registering here as well only repeats the atomic operation.
            if self.notifications.pending() & (!(WorkSet::CALLS) | WorkSet::CONTROL)
                != WorkSet::NONE
            {
                self.control_idle = false;
            }
            // A hosted queue publishes into the transport during control or
            // application work, before the output poll below. Revoke idle before
            // either producer runs; a flush-time notification would be too late.
            if self.idle
                && (!queue.is_empty()
                    || !self.control_idle
                    || self.schedule.active != WorkSet::NONE
                    || self.notifications.pending() != WorkSet::NONE)
            {
                self.idle = false;
                controls.set_idle(false);
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
            // Publication only enqueues immutable frames. This is the sole
            // coordinator output boundary. Ingress names its bounded turn;
            // the hosted runtime owns deferred physical output, and fixed byte
            // IO retains partial flushing here.
            let queued = queue.len();
            if queued != 0 && self.idle {
                self.idle = false;
                controls.set_idle(false);
            }
            // An input turn is bounded by the ingress frame budget. Capacity
            // pressure and an unfinished partial flush always force progress.
            let output_ready = if !end_input_turn
                && !self.output_blocked
                && self.output.is_clean()
                && queue.available() != 0
            {
                queue.is_empty()
            } else {
                match self
                    .output
                    .poll(writer, queue, messages, cx, self.options.budget)
                {
                    Poll::Ready(Err(error)) => return Err(Box::new(Cause::Output(error))),
                    Poll::Ready(Ok(())) => true,
                    Poll::Pending => false,
                }
            };
            let output_advanced = queue.len() < queued || (output_ready && self.output_blocked);
            if output_advanced {
                // Like the legacy writer, wake output consumers rather than
                // restarting protocol scans after every successful write.
                // Actual retirement and capacity waits retain their owners.
                if self.output_blocked
                    || self.driver.output_may_unblock()
                    || self.driver.runtime().has_retired()
                {
                    self.schedule.active |= WorkSet::MAINTENANCE;
                }
                if !preserves_control
                    && !(self.control_idle && controls.preserves_idle(Activity::OutputProgress))
                {
                    self.control_idle = false;
                }
                if std::mem::take(&mut self.output_blocked) {
                    self.progressed();
                }
            }
            if output_ready && std::mem::take(&mut self.calls_waiting_for_output) {
                self.schedule.active |= WorkSet::CALLS;
            }
            let idle = self.schedule.active == WorkSet::NONE
                && self.control_idle
                && self.notifications.pending() == WorkSet::NONE
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
                if remaining != 0 && (self.schedule.active != WorkSet::NONE || self.control_idle) {
                    continue;
                }
                if output_advanced && remaining > 1 {
                    // Physical release can resume a control producer now.
                    // Charge each extra turn; output-producing controls remain
                    // bounded, while notifications alone still yield below.
                    remaining -= 1;
                    continue;
                }
                cx.waker().wake_by_ref();
            }
            return Ok(delivered);
        }
    }
}

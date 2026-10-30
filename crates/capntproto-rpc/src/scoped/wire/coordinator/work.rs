//! Application dispatch and completion adapters; no physical IO polling.
use super::*;

impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
    /// New IO/control progress invalidates the partial idle scan. Individual
    /// lanes report Yield separately from true Pending, so mismatched scan sizes
    /// cannot continually wake one another on an otherwise idle connection.
    pub(super) fn changed(&mut self) {
        self.progressed();
    }
    /// Local progress and plain Call admission can change routes and cleanup,
    /// but cannot make an unrelated parked service future ready. Its operation
    /// waker owns that; protocol resolution/cancellation still calls changed().
    pub(super) fn progressed(&mut self) {
        self.control_idle = false;
        self.schedule.active = WorkSet::FOREIGN | WorkSet::CALLS | WorkSet::MAINTENANCE;
        if self.driver.runtime().queued() != 0 || self.driver.has_pending_call() {
            self.schedule.active |= WorkSet::ACCEPT;
        }
        if !self.worker.is_empty() {
            self.schedule.active |= WorkSet::WORKER;
        }
        if !self.commands.is_empty() {
            self.schedule.active |= WorkSet::COMMANDS;
        }
    }
    /// A completed application step keeps unrelated lanes' idle certificates.
    /// Exact operation wakes own parked futures; only new admission, descriptor
    /// work and freed protocol resources need another lane visit.
    pub(super) fn lane_progressed(&mut self, lane: Lane) {
        if matches!(lane, Lane::Foreign | Lane::Commands) {
            // These controls can replace arbitrary routes or policy.
            self.progressed();
            return;
        }
        self.control_idle = false;
        self.schedule.active |= lane.work() | WorkSet::MAINTENANCE;
        if lane == Lane::Accept || self.driver.ledgers().answers.has_changed_producers() {
            self.schedule.active |= WorkSet::WORKER;
        }
        if self.driver.runtime().queued() != 0 || self.driver.has_pending_call() {
            self.schedule.active |= WorkSet::ACCEPT;
        }
        if self.calls.has_unpublished() || !self.hooks.inbox.queue.borrow().is_empty() {
            self.schedule.active |= WorkSet::CALLS;
        }
        if !self.foreign.is_empty() && lane != Lane::Accept {
            self.schedule.active |= WorkSet::FOREIGN;
        }
        if !self.commands.is_empty() {
            self.schedule.active |= WorkSet::COMMANDS;
        }
    }
    /// The wire producer reports transition effects even if its final poll
    /// suspends. Keep exact operation queues and the unfinished fair rotation.
    fn maintenance_changed(&mut self, effects: connection::MaintenanceEffects) {
        let mut ready = WorkSet::NONE;
        if effects.routes() {
            ready |= WorkSet::FOREIGN | WorkSet::CALLS | WorkSet::WORKER;
        }
        if effects.calls() {
            ready |= WorkSet::CALLS;
        }
        if effects.answers() {
            ready |= WorkSet::WORKER;
        }
        if effects.admission() {
            ready |= WorkSet::ACCEPT;
        }
        if effects.resources() {
            ready |= WorkSet::CALLS | WorkSet::FOREIGN | WorkSet::WORKER;
        }
        // Deferred dispatch can make a third-party call runnable without IO.
        // Its network control must prepare the transfer before worker admission.
        if effects.output() || effects.admission() {
            self.control_idle = false;
        }
        self.schedule.active |= ready;
    }
    pub(super) fn resume_at(&mut self, lane: Lane) {
        // Once every lane has certified idle, start a new scan at the
        // lane made ready by this event. During an unfinished scan preserve
        // the rotating cursor, so a hot source cannot starve maintenance.
        self.schedule.resume_at(lane);
        self.changed();
    }
    #[cfg(test)]
    pub(super) fn poll_work<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<()>> {
        let mut budget = self.options.budget.get();
        self.poll_work_budget(
            messages,
            queue,
            scratch,
            output,
            cx,
            &mut budget,
            &self.notifications.producer(WorkSet::CALLS),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn poll_work_budget<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
        remaining: &mut usize,
        waker: &Waker,
    ) -> Poll<capnp::Result<()>> {
        let notified = self.notifications.take(cx.waker());
        if notified & WorkSet::CONTROL != WorkSet::NONE {
            self.control_idle = false;
        }
        let ready = notified & !WorkSet::CONTROL;
        if ready != WorkSet::NONE {
            self.schedule.resume_at(if ready == WorkSet::WORKER {
                Lane::Worker
            } else {
                Lane::Calls
            });
            // Operation queues retain exact keys. Early server pipelines require
            // maintenance as well, but a wake does not resolve foreign promises
            // or admit unrelated calls. Actual transitions report those effects.
            self.schedule.active |= ready;
            if ready & WorkSet::WORKER != WorkSet::NONE {
                self.schedule.active |= WorkSet::MAINTENANCE;
            }
        }
        if !self
            .hooks
            .inbox
            .enqueue_wake
            .borrow()
            .as_ref()
            .is_some_and(|old| Rc::ptr_eq(old, &self.enqueue_waker))
        {
            *self.hooks.inbox.enqueue_wake.borrow_mut() = Some(self.enqueue_waker.clone());
            *self.hooks.inbox.owners.wake.borrow_mut() = Some(self.foreign_waker.clone());
            self.hooks
                .inbox
                .owners
                .metadata
                .register(&self.foreign_waker);
        }
        // IO-only or spurious polls must preserve a completed idle scan. Real
        // protocol progress and application wakeups invalidate it explicitly.
        if self.schedule.active == WorkSet::NONE {
            return Poll::Pending;
        }
        // A plain reply often leaves only an outgoing request and cleanup.
        // Certifying empty cleanup needs no lane adapters or scan state.
        if self.schedule.active == ((WorkSet::CALLS) | (WorkSet::MAINTENANCE))
            && *remaining > 0
            && self.driver.maintenance_is_idle(messages, queue)
        {
            self.schedule.active &= !(WorkSet::MAINTENANCE);
            *remaining -= 1;
        }
        // A batch of immediately routable calls can share the other lanes'
        // idle certificates. Sleeping response owners do not require a scan.
        while self.schedule.active == WorkSet::CALLS
            && *remaining > 0
            && self.calls.idle_after_publication()
            && !self.hooks.inbox.queue.borrow().is_empty()
        {
            self.direct_publication = false;
            let mut quantum = (*remaining).min(8);
            *remaining -= quantum;
            self.schedule.cursor = Lane::Worker;
            if let Err(error) = queue.ensure_capacity() {
                if error == messages::Error::NoSendSlot && !queue.is_empty() {
                    self.output_blocked = true;
                    self.schedule.active = WorkSet::NONE;
                    return Poll::Pending;
                }
                return Poll::Ready(Err(wire_error(error)));
            }
            let result = self.poll_calls(
                messages,
                queue,
                scratch,
                output,
                &mut Context::from_waker(waker),
                &mut quantum,
                false,
            );
            *remaining += quantum;
            return match result {
                Poll::Ready(Err(error)) => Poll::Ready(Err(*error)),
                Poll::Ready(Ok(Progress::Published { plain: true }))
                    if self.calls.idle_after_publication()
                        && self.notifications.pending() == WorkSet::NONE =>
                {
                    self.direct_publication = true;
                    self.control_idle = false;
                    // This lane began with every other lane certified idle.
                    // Immediate publication adds one locally retained question,
                    // no capability grants or deferred task. Hook collection
                    // may retire graph owners; those alone need cleanup now.
                    // Application/resource notifications already prevent this
                    // branch through WorkWake's changed check above.
                    self.schedule.active = if self.hooks.inbox.queue.borrow().is_empty() {
                        WorkSet::NONE
                    } else {
                        WorkSet::CALLS
                    };
                    if self.driver.runtime().has_retired() {
                        self.schedule.active |= WorkSet::MAINTENANCE;
                    }
                    if self.schedule.active == WorkSet::CALLS && *remaining != 0 {
                        continue;
                    }
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Ok(Progress::Idle)) | Poll::Pending => {
                    self.calls_waiting_for_output |= !queue.is_empty() || !self.output.is_clean();
                    self.schedule.active = WorkSet::NONE;
                    if self.driver.runtime().has_retired() {
                        self.schedule.active = WorkSet::MAINTENANCE;
                    }
                    Poll::Pending
                }
                Poll::Ready(Ok(Progress::Yield)) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Ready(Ok(progress)) => {
                    self.lane_progressed(Lane::Calls);
                    self.delivered |= progress == Progress::Completed;
                    Poll::Ready(Ok(()))
                }
            };
        }
        // A newly received plain call preserves every other lane's idle
        // certificate. Service admission and execution with the same quanta
        // as the rotating scanner, without rebuilding its six adapters.
        if self.schedule.active == ((WorkSet::ACCEPT) | (WorkSet::WORKER)) && *remaining > 1 {
            *remaining -= 1;
            if self.accept_request(messages)?
                && self.driver.runtime().queued() == 0
                && !self.driver.has_pending_call()
                && self.notifications.pending() == WorkSet::NONE
            {
                self.schedule.active = WorkSet::WORKER;
                self.schedule.cursor = Lane::Worker;
            } else {
                self.lane_progressed(Lane::Accept);
            }
        }
        if self.schedule.active == WorkSet::WORKER && *remaining > 0 {
            let mut quantum = (*remaining).min(8);
            *remaining -= quantum;
            self.schedule.cursor = Lane::Maintenance;
            if let Err(error) = queue.ensure_capacity() {
                if error == messages::Error::NoSendSlot && !queue.is_empty() {
                    self.output_blocked = true;
                    self.schedule.active = WorkSet::NONE;
                    return Poll::Pending;
                }
                return Poll::Ready(Err(wire_error(error)));
            }
            let result = self.poll_worker(messages, queue, scratch, output, &mut quantum);
            *remaining += quantum;
            if self.driver.runtime().has_retired() {
                self.schedule.active |= WorkSet::MAINTENANCE;
            }
            return match result {
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Ready(Ok(server::Progress::Replied))
                    if self.worker.completed_ordinary()
                        && self.worker.is_empty()
                        && self.schedule.active & (WorkSet::CALLS) == WorkSet::NONE
                        && self.foreign.is_empty()
                        && self.commands.is_empty()
                        && self.driver.runtime().queued() == 0
                        && !self.driver.has_pending_call()
                        && self.hooks.inbox.queue.borrow().is_empty()
                        && self.notifications.pending() == WorkSet::NONE =>
                {
                    self.direct_publication = self.worker.completed_plain();
                    self.control_idle = false;
                    self.schedule.active = if self.driver.runtime().has_retired()
                        || !self.driver.maintenance_is_idle(messages, queue)
                    {
                        WorkSet::MAINTENANCE
                    } else {
                        WorkSet::NONE
                    };
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Ok(server::Progress::Yield)) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Ready(Ok(server::Progress::Idle)) | Poll::Pending => {
                    self.schedule.active &= !(WorkSet::WORKER);
                    Poll::Pending
                }
                Poll::Ready(Ok(_)) => {
                    self.lane_progressed(Lane::Worker);
                    Poll::Ready(Ok(()))
                }
            };
        }
        self.poll_active_work(messages, queue, scratch, output, cx, remaining, waker)
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub(super) fn poll_active_work<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
        remaining: &mut usize,
        waker: &Waker,
    ) -> Poll<capnp::Result<()>> {
        let mut application_cx = Context::from_waker(waker);
        let mut advanced = false;
        let mut completed = false;
        while *remaining > 0 && self.schedule.active != WorkSet::NONE {
            let index = self.schedule.next().expect("active work");
            // Amortize each lane's scan setup while retaining a shared bound
            // and a rotating cursor. Calls, workers and maintenance return
            // unused allowance when publication or suspension ends a visit.
            let empty = match index {
                Lane::Foreign => self.foreign.is_empty(),
                Lane::Accept => {
                    self.driver.runtime().queued() == 0 && !self.driver.has_pending_call()
                }
                // Admission and publication share this lane. A fresh request
                // in the hook inbox needs the same quantum as an admitted
                // invocation; otherwise it takes an extra scheduler rotation
                // just to publish its already-ready frame.
                Lane::Calls => self.hooks.inbox.queue.borrow().is_empty() && self.calls.is_empty(),
                Lane::Worker => self.worker.is_empty() && self.native_tails.is_empty(),
                // Maintenance owns its bounded readiness selection and charges
                // one step when idle. Probing here builds the same sixteen-class
                // readiness mask again immediately before polling the lane.
                Lane::Maintenance => false,
                Lane::Commands => self.commands.is_empty(),
            };
            // A queued outgoing call's admission refreshes all hooks before
            // dispatch. An empty foreign lane can share that upcoming refresh;
            // without queued calls it still collects retired/changed endpoints.
            let refresh_in_admission = index == Lane::Foreign
                && self.schedule.active & (WorkSet::CALLS) != WorkSet::NONE
                && !self.hooks.inbox.queue.borrow().is_empty();
            if empty && (index != Lane::Foreign || refresh_in_admission) {
                // Empty lanes still consume their fair quantum and advance the
                // cursor, but need no publication/callback setup. Admission's
                // wake remains registered before certifying its queue idle.
                if index == Lane::Calls {
                    self.hooks.register_next(waker);
                }
                *remaining -= 1;
                self.schedule.active &= !(index.work());
                continue;
            }
            let quantum =
                if index == Lane::Maintenance && self.schedule.active == WorkSet::MAINTENANCE {
                    // Maintenance has its own rotating, bounded slot scan and
                    // stops at the first publication. Once every other lane has
                    // reported idle, use the remaining allowance instead of
                    // rebuilding the scan every eight slots. Active application
                    // lanes retain the ordinary round-robin quantum.
                    *remaining
                } else if !empty
                    && matches!(
                        index,
                        Lane::Foreign | Lane::Calls | Lane::Worker | Lane::Maintenance
                    )
                {
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
            let writable = if index.publishes() && index != Lane::Maintenance && !empty {
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
            let mut publication = None;
            let outcome = if !writable {
                Poll::Pending
            } else {
                match index {
                    Lane::Foreign => {
                        Self::collect_hooks(&mut self.hooks, &mut self.driver, output);
                        match self.foreign.poll(
                            &mut self.hooks,
                            &mut self.driver.capabilities(),
                            &mut Context::from_waker(&self.foreign_waker),
                            quantum,
                        ) {
                            Poll::Ready(Ok(foreign::Progress::Idle)) => Poll::Pending,
                            Poll::Ready(Ok(foreign::Progress::Yield)) => continue,
                            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                            Poll::Pending => Poll::Pending,
                        }
                    }
                    Lane::Accept => match self.accept_request(messages) {
                        Ok(false) => Poll::Pending,
                        Ok(true) => Poll::Ready(Ok(())),
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Lane::Calls => {
                        let mut unused = quantum.get();
                        let result = self.poll_calls(
                            messages,
                            queue,
                            scratch,
                            output,
                            &mut application_cx,
                            &mut unused,
                            true,
                        );
                        *remaining += unused;
                        match result {
                            Poll::Ready(Ok(Progress::Idle)) => Poll::Pending,
                            Poll::Ready(Ok(Progress::Yield)) => continue,
                            Poll::Ready(Ok(Progress::Completed)) => {
                                completed = true;
                                self.delivered = true;
                                Poll::Ready(Ok(()))
                            }
                            Poll::Ready(Ok(Progress::Published { plain })) => {
                                publication = Some(plain);
                                Poll::Ready(Ok(()))
                            }
                            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                            Poll::Ready(Err(e)) => Poll::Ready(Err(*e)),
                            Poll::Pending => Poll::Pending,
                        }
                    }
                    Lane::Worker => {
                        let mut unused = quantum.get();
                        let result =
                            self.poll_worker(messages, queue, scratch, output, &mut unused);
                        *remaining += unused;
                        match result {
                            Poll::Ready(Ok(server::Progress::Idle)) => Poll::Pending,
                            Poll::Ready(Ok(server::Progress::Yield)) => continue,
                            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                            Poll::Pending => Poll::Pending,
                        }
                    }
                    Lane::Commands => match self.commands.take() {
                        None => Poll::Pending,
                        Some(sender) => {
                            if !sender.is_closed() {
                                let result = self.bootstrap(messages, queue);
                                sender.send(result);
                            }
                            Poll::Ready(Ok(()))
                        }
                    },
                    Lane::Maintenance => {
                        let mut unused = quantum.get();
                        let result = self.driver.poll_maintenance_budget_with(
                            messages,
                            queue,
                            options.output_bytes.get(),
                            options.reader,
                            scratch,
                            &mut unused,
                            &mut || output.bundle(),
                            &mut |owner, imports, bundle| output.attach(owner, imports, bundle),
                            &mut |results, path, runtime| {
                                self.hooks
                                    .server_pipeline(results, path, runtime, |runtime, hook| {
                                        self.foreign.import(runtime, hook)
                                    })
                                    .map(|client| client.map(Ok))
                                    .map_err(|e| {
                                        pipelines::Error::Input(owned::Error::Wire(e.kind))
                                    })
                            },
                        );
                        *remaining += unused;
                        self.maintenance_changed(self.driver.maintenance_effects());
                        match result {
                            Ok(connection::Progress::Idle) => Poll::Pending,
                            Ok(connection::Progress::Yield) => continue,
                            Ok(_) => Poll::Ready(Ok(())),
                            Err(e) => Poll::Ready(Err(wire_error(e))),
                        }
                    }
                }
            };
            if index == Lane::Calls && outcome.is_pending() {
                // Admission owns its capacity checks. Only retry after flush
                // when a real attempt could have waited for retained buffers;
                // physical output must not gate otherwise-ready protocol work.
                self.calls_waiting_for_output |= !queue.is_empty() || !self.output.is_clean();
            }
            // Hook collection can retire the final graph reference even when
            // its application lane reports Pending. Maintenance may already
            // have certified idle earlier in this rotation; reactivate it so
            // Release/Finish and provision cleanup do not wait for unrelated IO.
            if self.driver.runtime().has_retired() {
                self.schedule.active |= WorkSet::MAINTENANCE;
            }
            if let Poll::Ready(result) = outcome {
                result?;
                if index == Lane::Accept
                    && self.schedule.active == ((WorkSet::ACCEPT) | (WorkSet::WORKER))
                    && self.driver.runtime().queued() == 0
                    && !self.driver.has_pending_call()
                    && self.notifications.pending() == WorkSet::NONE
                {
                    // The sole new work was a plain deferred request. Moving
                    // it into the worker changes neither controls nor other
                    // application lanes; execution performs hook refresh.
                    self.schedule.active = WorkSet::WORKER;
                    advanced = true;
                    continue;
                }
                if index == Lane::Worker
                    && self.worker.completed_ordinary()
                    && self.worker.is_empty()
                    && self.schedule.active & (WorkSet::CALLS) == WorkSet::NONE
                    && self.foreign.is_empty()
                    && self.commands.is_empty()
                    && self.driver.runtime().queued() == 0
                    && !self.driver.has_pending_call()
                    && self.hooks.inbox.queue.borrow().is_empty()
                    && self.notifications.pending() == WorkSet::NONE
                {
                    // Ordinary completion updates answer/export ownership.
                    // Dispatch refreshed hooks; revisit ledger and pipeline
                    // cleanup while preserving unrelated application scans.
                    // Capability-bearing Returns still invalidate controls;
                    // only plain publication can reuse their idle certificate.
                    // Reentrant callbacks and independent wakes use WorkWake.
                    self.direct_publication = self.worker.completed_plain();
                    self.control_idle = false;
                    self.schedule.active = if self.driver.runtime().has_retired()
                        || !self.driver.maintenance_is_idle(messages, queue)
                    {
                        WorkSet::MAINTENANCE
                    } else {
                        WorkSet::NONE
                    };
                    return Poll::Ready(Ok(()));
                }
                // Its response cannot have arrived during publication. The
                // inbox wake was registered by calls.poll_using; preserve any
                // cancellation/new-call wake delivered during its callbacks.
                if publication == Some(true)
                    && self.calls.idle_after_publication()
                    && self.notifications.pending() == WorkSet::NONE
                {
                    // This publication used the direct, routable, data-only
                    // path. Hook collection already ran in admission; it did
                    // not create another task, foreign promise or worker.
                    // Preserve those lanes' idle scans while restarting the
                    // question/cleanup scan that publication actually changed.
                    self.control_idle = false;
                    self.schedule.active |= WorkSet::MAINTENANCE;
                    if self.hooks.inbox.queue.borrow().is_empty() {
                        self.schedule.active &= !WorkSet::CALLS;
                    }
                    if self.foreign.is_empty() {
                        self.schedule.active &= !WorkSet::FOREIGN;
                    }
                } else if index != Lane::Maintenance {
                    self.lane_progressed(index);
                }
                advanced = true;
                // Deliver completion promptly. Publications share the
                // remaining cooperative allowance before the output boundary.
                if completed {
                    return Poll::Ready(Ok(()));
                }
                continue;
            }
            // changed() restarts partial idle scans without moving their
            // cursors. A completed scan is therefore already a fresh certificate.
            self.schedule.active &= !(index.work());
            if self.schedule.active == WorkSet::NONE {
                return if advanced {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                };
            }
        }
        if self.schedule.active != WorkSet::NONE {
            cx.waker().wake_by_ref();
        }
        if advanced {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    #[inline(always)]
    pub(super) fn collect_hooks<'domain, O: Output<I>>(
        hooks: &mut ClientHooks<'domain>,
        driver: &mut Peer<'_, 'domain, '_, I, P, C, H>,
        output: &O,
    ) {
        hooks.collect_wire_with_local(
            driver,
            |owner| output.metadata_source(owner),
            foreign::local_metadata,
            |hook| hook.metadata_changes(),
            |hook| Some(hook.clone()),
        );
    }

    // A shared closure type lets all hosted completion lanes use the same wire
    // poller. The first-refresh certificate still applies only at its original
    // call site; every subsequent invocation observes current ownership.
    pub(super) fn hook_refresh<'output, 'domain, O: Output<I>>(
        output: &'output O,
        mut refresh: bool,
    ) -> impl FnMut(&mut ClientHooks<'domain>, &mut Peer<'_, 'domain, '_, I, P, C, H>) + 'output
    {
        move |hooks, driver| {
            if std::mem::replace(&mut refresh, true) {
                Self::collect_hooks(hooks, driver, output);
            }
        }
    }

    #[inline(always)]
    fn accept_request(&mut self, messages: &messages::Messages<'p, '_, '_>) -> capnp::Result<bool> {
        if self.driver.has_promised_pending_call() {
            self.accept_promised_request(messages)
        } else {
            // Keep ordinary local admission in its original caller. Routing
            // it through the larger promised-answer adapter spills and copies
            // the whole owning request even though no resolver is needed.
            self.worker.accept(&mut self.driver)
        }
    }
    #[inline(never)]
    fn accept_promised_request(
        &mut self,
        messages: &messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<bool> {
        let hooks = &mut self.hooks;
        let foreign = &mut self.foreign;
        let calls = &self.calls;
        self.worker.accept_using(&mut self.driver, |driver| {
            driver
                .dispatch_immediate_call_with(true, messages, |answer, path, runtime| {
                    // Final successful results already retain their exact wire
                    // capability routes. Importing their original hooks again
                    // would create duplicate graph endpoints. Explicit early
                    // pipelines and structured failures keep their hook owner.
                    if !answer.is_pending() && !answer.producer.has_pipeline_override() {
                        return Ok(None);
                    }
                    hooks
                        .server_pipeline(&answer.producer, path, runtime, |runtime, hook| {
                            // Native projections already own the exact branded
                            // route. Avoid a foreign promise whose resolver would
                            // keep the parent's response alive until this child.
                            match calls.retain_direct(runtime, hook)? {
                                Some(client) => Ok(client),
                                None => foreign.import(runtime, hook),
                            }
                        })
                        .map(|client| client.map(Ok))
                        .map_err(|error| pipelines::Error::Input(owned::Error::Wire(error.kind)))
                })
                .map_err(wire_error)
        })
    }

    // Inline the small lane adapter into the scheduler so its concrete closures
    // share borrows and eliminate intermediate publication moves.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_calls<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
        quantum: &mut usize,
        refresh: bool,
    ) -> Poll<Result<Progress, Box<Error>>> {
        let options = self.options;
        let result = self.calls.poll_using(
            &mut self.hooks,
            &mut self.driver,
            messages,
            options.reader,
            cx,
            quantum,
            |runtime, hook| self.foreign.import(runtime, hook),
            Self::hook_refresh(output, refresh),
            |invocations, driver, messages, credit, publication| {
                let mut bundle = output.bundle();
                if let Some(credit) = credit {
                    output.retain_call(&mut bundle, credit)?;
                }
                let (direct, native) = match publication {
                    super::calls::Publication::PreparedCapabilities {
                        target,
                        method,
                        source,
                        capabilities,
                    } => {
                        return driver
                            .try_send_immediate_prepared_with_capability_table(
                                target,
                                method,
                                source,
                                capabilities,
                                messages,
                                queue,
                                options.output_bytes.get(),
                                options.reader,
                                scratch,
                                bundle,
                                |owner, imports, bundle| output.attach(owner, imports, bundle),
                            )
                            .map(|key| key.map(super::calls::SendOutcome::Immediate))
                            .or_else(super::calls::publication_error);
                    }
                    super::calls::Publication::Immediate {
                        target,
                        method,
                        params,
                        capabilities,
                    } => {
                        if let Some(capabilities) = capabilities {
                            let connection::ImmediateParams::Content(params) = params else {
                                unreachable!("capability-bearing content")
                            };
                            return driver
                                .try_send_immediate_with_capability_table(
                                    target,
                                    method,
                                    *params,
                                    capabilities,
                                    messages,
                                    queue,
                                    options.output_bytes.get(),
                                    options.reader,
                                    scratch,
                                    bundle,
                                    |owner, imports, bundle| output.attach(owner, imports, bundle),
                                )
                                .map(|key| key.map(super::calls::SendOutcome::Immediate))
                                .or_else(super::calls::publication_error);
                        }
                        return driver
                            .try_send_immediate_borrowed(
                                target,
                                method,
                                params,
                                messages,
                                queue,
                                options.output_bytes.get(),
                                options.reader,
                                bundle,
                            )
                            .map(|key| key.map(super::calls::SendOutcome::Immediate))
                            .or_else(super::calls::publication_error);
                    }
                    super::calls::Publication::Queued { direct, native } => (direct, native),
                };
                let native = native
                    .map(|context| {
                        self.native_tails.try_reserve(1).map_err(wire_error)?;
                        let parent = self.worker.native_parent(context).ok_or_else(|| {
                            Error::disconnected("native tail answer retired".into())
                        })?;
                        Ok::<_, Error>((parent, context.clone()))
                    })
                    .transpose()?;
                if direct {
                    let result = driver
                        .send_invocation_using(
                            messages,
                            queue,
                            options.output_bytes.get(),
                            options.reader,
                            scratch,
                            bundle,
                            |owner, imports, bundle| output.attach(owner, imports, bundle),
                        )
                        .map(|sent| sent.map(super::calls::SendOutcome::Queued))
                        .or_else(super::calls::publication_error)?;
                    if let (
                        Some((parent, context)),
                        Some(super::calls::SendOutcome::Queued(sent)),
                    ) = (native, &result)
                    {
                        let retained = driver.retain_question(sent.question).map_err(wire_error)?;
                        context.published(false);
                        self.native_tails.push_back(PendingTail {
                            parent,
                            child: sent.question,
                            retained,
                            context,
                        });
                    }
                    return Ok(result);
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
                    .map(|sent| sent.map(super::calls::SendOutcome::Queued))
                    .or_else(super::calls::publication_error)
            },
        );
        self.calls
            .retry_output(result, !queue.is_empty() || !self.output.is_clean())
    }

    // Inline the small lane adapter into the scheduler so its concrete closures
    // share borrows and eliminate intermediate publication moves.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_worker<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        quantum: &mut usize,
    ) -> Poll<capnp::Result<server::Progress>> {
        let options = self.options;
        if let Some(pending) = self.native_tails.front() {
            match self.driver.tail_answer(
                pending.parent,
                pending.child,
                messages,
                queue,
                options.output_bytes.get(),
                options.reader,
            ) {
                Ok(retired) => drop(retired),
                Err(error) if super::calls::output_capacity_error(&error) && !queue.is_empty() => {
                    self.output_blocked = true;
                    return Poll::Pending;
                }
                Err(error) => return Poll::Ready(Err(wire_error(error))),
            }
            let pending = self
                .native_tails
                .pop_front()
                .expect("published native tail");
            self.driver.release_question(pending.retained);
            pending.context.published(true);
            *quantum = quantum.saturating_sub(1);
            return Poll::Ready(Ok(server::Progress::Advanced));
        }
        let queue = RefCell::new(queue);
        let mut worker_cx = Context::from_waker(&self.worker_waker);
        self.worker.poll_using(
            &mut self.hooks,
            &mut self.driver,
            messages,
            options.output_bytes.get(),
            options.reader,
            &mut worker_cx,
            quantum,
            |runtime, hook| self.foreign.import(runtime, hook),
            Self::hook_refresh(output, true),
            |driver, messages, key, bytes| {
                driver
                    .send_answer_using(
                        key,
                        messages,
                        &mut queue.borrow_mut(),
                        bytes,
                        options.reader,
                        scratch,
                        output.bundle(),
                        |owner, imports, bundle| output.attach(owner, imports, bundle),
                        server::Producer::write_exception,
                    )
                    .map_err(wire_error)
            },
            |results| results.prepare_trace(&self.policy),
            |driver, messages, request, source| {
                match source {
                    server::PlainPayload::Inline(source) => driver.try_send_answer_prepared(
                        request,
                        source,
                        messages,
                        &mut queue.borrow_mut(),
                        options.reader,
                        output.bundle(),
                    ),
                    server::PlainPayload::Shared(source) => driver.try_send_answer_prepared_shared(
                        request,
                        source,
                        messages,
                        &mut queue.borrow_mut(),
                        options.reader,
                        output.bundle(),
                    ),
                }
                .map(|retired| retired.is_some())
                .map_err(wire_error)
            },
        )
    }
}

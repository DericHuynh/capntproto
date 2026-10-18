//! Application dispatch and completion adapters; no physical IO polling.
use super::*;

impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
    /// New IO/control progress invalidates the partial idle scan. Individual
    /// lanes report Yield separately from true Pending, so mismatched scan sizes
    /// cannot continually wake one another on an otherwise idle connection.
    pub(super) fn changed(&mut self) {
        self.control_idle = false;
        self.schedule.active = WorkSet::FOREIGN | WorkSet::CALLS | WorkSet::MAINTENANCE;
        if self.driver.runtime.queued() != 0 || self.driver.has_pending_call() {
            self.schedule.active |= WorkSet::ACCEPT;
        }
        if !self.worker.is_empty() {
            self.schedule.active |= WorkSet::WORKER;
        }
        if !self.commands.is_empty() {
            self.schedule.active |= WorkSet::COMMANDS;
        }
        self.calls.invalidate_scan();
        self.worker.invalidate_scan();
        self.foreign.invalidate_scan();
        self.driver.invalidate_maintenance();
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
            &self.notifications.waker(),
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
        let changed = notified & !WorkSet::CONTROL;
        if changed != WorkSet::NONE {
            if changed == WorkSet::CALLS {
                // A newly enqueued request cannot change foreign promises,
                // server tasks, network controls or cleanup before admission.
                if self.schedule.active == WorkSet::NONE {
                    self.schedule.cursor = Lane::Calls;
                }
                self.schedule.active |= WorkSet::CALLS;
                self.calls.invalidate_scan();
            } else {
                self.resume_at(Lane::Calls);
            }
            if self.cleanup_on_enqueue
                && self.calls.is_empty()
                && !self.hooks.inbox.queue.borrow().is_empty()
            {
                // A completed capability-bearing call can release its final
                // public owners before the application enqueues its successor.
                // Give that cleanup one ordinary maintenance quantum before
                // admission, as C++ does when those owners are destroyed.
                // Consume the preference once: repeated wakes or a blocked
                // cleanup must not keep resetting the rotating cursor.
                self.cleanup_on_enqueue = false;
                Self::collect_hooks(&mut self.hooks, &mut self.driver, output);
                if self.driver.runtime.has_retired()
                    || self.driver.ledgers.questions.has_pending_finish()
                    || self.driver.ledgers.imports.has_pending_release()
                {
                    self.driver.invalidate_maintenance();
                    self.schedule.active |= WorkSet::MAINTENANCE;
                    self.schedule.cursor = Lane::Maintenance;
                    self.cleanup_before_call = true;
                }
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
            && self.driver.maintenance_is_idle()
        {
            self.schedule.active &= !(WorkSet::MAINTENANCE);
            *remaining -= 1;
        }
        if self.schedule.active == WorkSet::CALLS
            && *remaining > 0
            && self.calls.is_empty()
            && !self.hooks.inbox.queue.borrow().is_empty()
        {
            let quantum = (*remaining).min(8);
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
                NonZeroUsize::new(quantum).unwrap(),
                false,
            );
            return match result {
                Poll::Ready(Err(error)) => Poll::Ready(Err(*error)),
                Poll::Ready(Ok(Progress::Published))
                    if self.calls.idle_after_publication()
                        && self.hooks.inbox.queue.borrow().is_empty()
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
                    if self.driver.runtime.has_retired() {
                        self.driver.invalidate_maintenance();
                        self.schedule.active = WorkSet::MAINTENANCE;
                    } else {
                        self.schedule.active = WorkSet::NONE;
                    }
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Ok(Progress::Idle)) | Poll::Pending => {
                    self.calls_waiting_for_output |= !queue.is_empty() || !self.output.is_clean();
                    self.schedule.active = WorkSet::NONE;
                    if self.driver.runtime.has_retired() {
                        self.driver.invalidate_maintenance();
                        self.schedule.active = WorkSet::MAINTENANCE;
                    }
                    Poll::Pending
                }
                Poll::Ready(Ok(Progress::Yield)) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Ready(Ok(progress)) => {
                    self.changed();
                    self.delivered |= progress == Progress::Completed;
                    self.cleanup_on_enqueue |= progress == Progress::Completed;
                    Poll::Ready(Ok(()))
                }
            };
        }
        // A newly received plain call preserves every other lane's idle
        // certificate. Service admission and execution with the same quanta
        // as the rotating scanner, without rebuilding its six adapters.
        if self.schedule.active == ((WorkSet::ACCEPT) | (WorkSet::WORKER)) && *remaining > 1 {
            *remaining -= 1;
            if self.worker.accept(&mut self.driver)?
                && self.driver.runtime.queued() == 0
                && !self.driver.has_pending_call()
                && self.notifications.pending() == WorkSet::NONE
            {
                self.schedule.active = WorkSet::WORKER;
                self.schedule.cursor = Lane::Worker;
            } else {
                self.changed();
            }
        }
        if self.schedule.active == WorkSet::WORKER && *remaining > 0 {
            let quantum = (*remaining).min(8);
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
            let result = self.poll_worker(
                messages,
                queue,
                scratch,
                output,
                &mut Context::from_waker(waker),
                NonZeroUsize::new(quantum).unwrap(),
            );
            if self.driver.runtime.has_retired() {
                self.driver.invalidate_maintenance();
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
                        && self.driver.runtime.queued() == 0
                        && !self.driver.has_pending_call()
                        && self.hooks.inbox.queue.borrow().is_empty()
                        && self.notifications.pending() == WorkSet::NONE =>
                {
                    self.direct_publication = self.worker.completed_plain();
                    self.control_idle = false;
                    self.schedule.active = if self.driver.runtime.has_retired()
                        || !self.driver.maintenance_is_idle()
                    {
                        self.driver.invalidate_maintenance();
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
                    self.changed();
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
        let queued = queue.len();
        let mut completed = false;
        while *remaining > 0 && self.schedule.active != WorkSet::NONE {
            let index = self.schedule.next().expect("active work");
            // Amortize each lane's scan setup while retaining a shared bound
            // and a rotating cursor. Application lanes charge their allowance;
            // maintenance reports the exact number of visited steps.
            let empty = match index {
                Lane::Foreign => self.foreign.is_empty(),
                Lane::Accept => {
                    self.driver.runtime.queued() == 0 && !self.driver.has_pending_call()
                }
                // Admission and publication share this lane. A fresh request
                // in the hook inbox needs the same quantum as an admitted
                // invocation; otherwise it takes an extra scheduler rotation
                // just to publish its already-ready frame.
                Lane::Calls => self.hooks.inbox.queue.borrow().is_empty() && self.calls.is_empty(),
                Lane::Worker => self.worker.is_empty(),
                Lane::Maintenance => self.driver.maintenance_is_idle(),
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
                if index == Lane::Maintenance {
                    self.cleanup_before_call = false;
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
            let writable = if index.publishes() && !empty {
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
            let mut published_retirement = false;
            let outcome = if !writable {
                Poll::Pending
            } else {
                match index {
                    Lane::Foreign => {
                        Self::collect_hooks(&mut self.hooks, &mut self.driver, output);
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
                    Lane::Accept => match self.worker.accept(&mut self.driver) {
                        Ok(false) => Poll::Pending,
                        Ok(true) => Poll::Ready(Ok(())),
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Lane::Calls => match self.poll_calls(
                        messages,
                        queue,
                        scratch,
                        output,
                        &mut application_cx,
                        quantum,
                        true,
                    ) {
                        Poll::Ready(Ok(Progress::Idle)) => Poll::Pending,
                        Poll::Ready(Ok(Progress::Yield)) => continue,
                        Poll::Ready(Ok(Progress::Completed)) => {
                            completed = true;
                            self.delivered = true;
                            self.cleanup_on_enqueue = true;
                            Poll::Ready(Ok(()))
                        }
                        Poll::Ready(Ok(Progress::Published)) => {
                            published = true;
                            Poll::Ready(Ok(()))
                        }
                        Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(*e)),
                        Poll::Pending => Poll::Pending,
                    },
                    Lane::Worker => match self.poll_worker(
                        messages,
                        queue,
                        scratch,
                        output,
                        &mut application_cx,
                        quantum,
                    ) {
                        Poll::Ready(Ok(server::Progress::Idle)) => Poll::Pending,
                        Poll::Ready(Ok(server::Progress::Yield)) => continue,
                        Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                        Poll::Pending => Poll::Pending,
                    },
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
                        let before_call = std::mem::take(&mut self.cleanup_before_call);
                        let before_cleanup = queue.len();
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
                        published_retirement = self.driver.maintenance_published_retirement();
                        if before_call
                            && queue.len() > before_cleanup
                            && self.driver.ledgers.imports.has_pending_release()
                        {
                            // A Finish can leave its already-ready Release
                            // behind. Allow one further ordinary quantum before
                            // the next Call, so the peer can reclaim the old
                            // capability before allocating its replacement.
                            // The preference was consumed above; busy cleanup
                            // cannot repeatedly reset the rotating cursor.
                            self.schedule.cursor = Lane::Maintenance;
                        }
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
            if self.driver.runtime.has_retired() {
                self.driver.invalidate_maintenance();
                self.schedule.active |= WorkSet::MAINTENANCE;
            }
            if let Poll::Ready(result) = outcome {
                result?;
                if index == Lane::Accept
                    && self.schedule.active == ((WorkSet::ACCEPT) | (WorkSet::WORKER))
                    && self.driver.runtime.queued() == 0
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
                    && self.driver.runtime.queued() == 0
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
                    self.schedule.active = if self.driver.runtime.has_retired()
                        || !self.driver.maintenance_is_idle()
                    {
                        self.driver.invalidate_maintenance();
                        WorkSet::MAINTENANCE
                    } else {
                        WorkSet::NONE
                    };
                    return Poll::Ready(Ok(()));
                }
                // Its response cannot have arrived during publication. The
                // inbox wake was registered by calls.poll_using; preserve any
                // cancellation/new-call wake delivered during its callbacks.
                if published
                    && self.calls.idle_after_publication()
                    && self.hooks.inbox.queue.borrow().is_empty()
                    && self.notifications.pending() == WorkSet::NONE
                {
                    // This publication used the direct, routable, data-only
                    // path. Hook collection already ran in admission; it did
                    // not create another task, foreign promise or worker.
                    // Preserve those lanes' idle scans while restarting the
                    // question/cleanup scan that publication actually changed.
                    self.control_idle = false;
                    self.driver.invalidate_maintenance();
                    self.schedule.active |= WorkSet::MAINTENANCE;
                    self.schedule.active &= !(WorkSet::CALLS);
                    if self.foreign.is_empty() {
                        self.schedule.active &= !WorkSet::FOREIGN;
                    }
                } else if published_retirement
                    && self.foreign.is_empty()
                    && self.worker.is_empty()
                    && self.commands.is_empty()
                    && self.driver.runtime.queued() == 0
                    && !self.driver.has_pending_call()
                    && self.notifications.pending() == WorkSet::NONE
                {
                    // Finish/Release only retire existing owners. Revisit
                    // outgoing calls which may have waited for capacity and
                    // continue cleanup, preserving unrelated idle scans.
                    // Owner destructors and independent futures still wake the
                    // ordinary lanes; controls observe every publication.
                    self.control_idle = false;
                    self.schedule.active |= (WorkSet::CALLS) | (WorkSet::MAINTENANCE);
                    self.calls.invalidate_scan();
                } else {
                    self.changed();
                }
                advanced = true;
                // Publish promptly and give a completed application's waker a
                // chance to run before scanning unrelated maintenance lanes.
                let batch_calls = published
                    && !self.hooks.inbox.queue.borrow().is_empty()
                    && if self.driver.uses_owned_output_queues() {
                        queue.len() < 32 && self.can_batch_request(messages)
                    } else {
                        queue.len() < 8 && !self.hooks.inbox.queue.borrow().is_empty()
                    };
                if completed || (queue.len() > queued && !batch_calls) {
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

    #[inline(never)]
    pub(super) fn collect_hooks<'domain, O: Output<I>>(
        hooks: &mut ClientHooks<'domain>,
        driver: &mut Peer<'_, 'domain, '_, I, P, C, H>,
        output: &O,
    ) {
        hooks.collect_wire(
            driver,
            |owner| output.metadata_source(owner),
            foreign::local_metadata,
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
        quantum: NonZeroUsize,
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
                let direct = match publication {
                    super::calls::Publication::Immediate {
                        target,
                        method,
                        params,
                        capability,
                    } => {
                        if let Some(capability) = capability {
                            let connection::ImmediateParams::Content(params) = params else {
                                unreachable!("capability-bearing content")
                            };
                            return driver
                                .try_send_immediate_with_capability(
                                    target,
                                    method,
                                    *params,
                                    capability,
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
                            |owner, imports, bundle| output.attach(owner, imports, bundle),
                        )
                        .map(|sent| sent.map(super::calls::SendOutcome::Queued))
                        .or_else(super::calls::publication_error);
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
        cx: &mut Context<'_>,
        quantum: NonZeroUsize,
    ) -> Poll<capnp::Result<server::Progress>> {
        let options = self.options;
        let queue = RefCell::new(queue);
        self.worker.poll_using(
            &mut self.hooks,
            &mut self.driver,
            messages,
            options.output_bytes.get(),
            options.reader,
            cx,
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
                        server::Results::write_exception,
                    )
                    .map_err(wire_error)
            },
            |results| results.prepare_trace(&self.policy),
            |driver, messages, request, source| {
                driver
                    .try_send_answer_prepared_shared(
                        request,
                        source,
                        messages,
                        &mut queue.borrow_mut(),
                        options.reader,
                        output.bundle(),
                    )
                    .map(|retired| retired.is_some())
                    .map_err(wire_error)
            },
        )
    }

    pub(super) fn can_batch_request(&self, messages: &messages::Messages<'p, '_, '_>) -> bool {
        let inbox = self.hooks.inbox.queue.borrow();
        let Some(call) = inbox.front() else {
            return false;
        };
        if !self.driver.uses_owned_output_queues()
            || self.calls.has_pending_admission()
            || self.driver.runtime.queued() != 0
            || !self.worker.is_empty()
            || !self.foreign.is_empty()
            || !self.commands.is_empty()
        {
            return false;
        }
        if call.third_party.is_some() || call.hints.only_promise_pipeline {
            return false;
        }
        let Some(overhead) = self
            .driver
            .path_capacity()
            .checked_mul(16)
            .and_then(|n| n.checked_add(256))
        else {
            return false;
        };
        let bytes = match &call.params {
            CallParams::Request(params) => params.prepared_call().and_then(|prepared| {
                prepared
                    .message()
                    .get_segments_for_output()
                    .iter()
                    .try_fold(0usize, |total, segment| total.checked_add(segment.len()))
            }),
            CallParams::OwnedRequest(_, bytes) | CallParams::Hook(_, bytes) => *bytes,
        };
        bytes
            .and_then(|bytes| bytes.checked_add(overhead))
            .is_some_and(|minimum| {
                messages.can_reserve(minimum.max(self.options.output_bytes.get()))
            })
    }
}

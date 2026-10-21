use super::*;

/// Result of visiting consecutive idle phases without polling any owner.
enum IdleWalk {
    Active,
    Complete,
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// The last maintenance poll only retired references and published Finish
    /// or Release. It performed no promise propagation or application dispatch.
    /// Returned owners and independently notified work still need observation.
    pub fn maintenance_published_retirement(&self) -> bool {
        self.maintenance_published_retirement
    }
    /// Check live ownership for an absence of immediately pollable maintenance.
    /// This bounded check calls no application hooks and changes no scan cursor.
    /// False requires normal polling; it does not necessarily mean work is ready.
    pub fn maintenance_is_idle(&self) -> bool {
        self.disconnected.is_none()
            && self.call.is_none()
            && self.resolution.is_none()
            && self.pipelines.is_empty()
            && self.outgoing_maintenance_is_idle()
            && self.ledgers.exports.retirement_is_idle(8)
            && !self.runtime.has_retired()
            && self.ledgers.answers.reclamation_is_idle(8)
            && !self.canceled_exchanges
            && self.exported.is_empty()
            && !self.ledgers.questions.has_pending_finish()
            && !self.ledgers.imports.has_pending_release()
            && self.runtime.queued() == 0
            && self.barrier.is_none()
            && self.echo.is_none()
            && self.tails.len() <= 8
            && self.tails.iter().all(|slot| !slot.is_occupied())
            && self.invocation_pipelines.is_empty()
    }
    /// Account for skipped physical slots without charging an owner poll.
    /// Empty phases form one bounded search for the next operation, just as
    /// maintenance_is_idle proves the whole driver idle in one budget step.
    #[inline(never)]
    fn advance_idle_phase(&mut self, phase: usize) -> bool {
        let visited =
            (self.maintenance_sizes[phase] - self.maintenance_index).min(self.scan_remaining);
        self.maintenance_index += visited;
        if self.maintenance_index == self.maintenance_sizes[phase] {
            self.maintenance_index = 0;
            self.maintenance_phase = if phase + 1 == self.maintenance_sizes.len() {
                0
            } else {
                phase + 1
            };
        }
        self.scan_remaining -= visited;
        if self.scan_remaining == 0 {
            self.scan_remaining = self.maintenance_total;
            return true;
        }
        false
    }
    /// Walk empty phases in order, without repeatedly dispatching on the
    /// cursor. At most one complete cycle of the fixed phase table is visited.
    /// Each proof is bounded independently of storage capacity. Stop before an
    /// active phase: its operation consumes a budget step, and any following
    /// proofs must observe the state after that operation's callbacks.
    fn walk_idle_phases(&mut self) -> IdleWalk {
        macro_rules! phase {
            ($phase:literal, $idle:expr) => {
                if self.maintenance_phase == $phase {
                    if !$idle {
                        return IdleWalk::Active;
                    }
                    if self.advance_idle_phase($phase) {
                        return IdleWalk::Complete;
                    }
                }
            };
        }
        loop {
            phase!(0, self.call.is_none());
            phase!(1, self.resolution.is_none());
            phase!(2, self.pipelines.is_empty());
            phase!(3, self.outgoing_maintenance_is_idle());
            phase!(4, self.ledgers.exports.retirement_is_idle(8));
            phase!(5, !self.runtime.has_retired());
            phase!(6, self.ledgers.answers.reclamation_is_idle(8));
            phase!(7, !self.canceled_exchanges);
            phase!(8, self.exported.is_empty());
            phase!(9, !self.ledgers.questions.has_pending_finish());
            phase!(10, !self.ledgers.imports.has_pending_release());
            phase!(11, self.call.is_none() && self.runtime.queued() == 0);
            phase!(12, self.runtime.queued() == 0);
            phase!(13, self.barrier.is_none());
            phase!(14, self.echo.is_none());
            phase!(
                15,
                self.tails.len() <= 8 && self.tails.iter().all(|slot| !slot.is_occupied())
            );
            phase!(16, self.invocation_pipelines.is_empty());
            phase!(17, self.runtime.queued() == 0);
        }
    }
    /// Outgoing observers can only progress against an authorized Return.
    /// Pending questions alone do not require a scan of every observer slot.
    /// This uses the same prerequisite as poll_outgoing, reads live state on
    /// every check, and keeps the ordinary scan for larger exchange tables.
    #[inline]
    pub(super) fn outgoing_maintenance_is_idle(&self) -> bool {
        self.outgoing.is_empty()
            || (self.exchanges.len() <= 8
                && !self.exchanges.iter().any(|slot| {
                    slot.authorized && matches!(&slot.exchange, Some(Exchange::Returned(_)))
                }))
    }
    /// Poll caller-budgeted promise propagation, deferred admission, cleanup and
    /// cancellation. The cursor advances even on resource errors so one blocked
    /// task cannot starve the cleanup which frees its storage. Control frames use
    /// the same FIFO as application output; the caller supplies its storage.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_maintenance(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        budget: core::num::NonZeroUsize,
    ) -> Result<Progress<I, A, S>, Error> {
        self.poll_maintenance_with(
            messages,
            queue,
            minimum,
            options,
            scratch,
            budget,
            &mut Default::default,
            &mut |_, _, _| u8::MAX,
            &mut |_, _, _| Ok(None),
        )
    }
    /// Promise Resolve publication uses the same resource-owning FIFO as Calls.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_maintenance_with_attachments<R, const N: usize>(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, crate::attachments::Sender<R, N>>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        budget: core::num::NonZeroUsize,
        capacity: Capacity,
        mut imported: impl FnMut(&I) -> Option<R>,
        mut local: impl FnMut(&S) -> Option<R>,
    ) -> Result<Progress<I, A, S>, Error> {
        self.poll_maintenance_with(
            messages,
            queue,
            minimum,
            options,
            scratch,
            budget,
            &mut || crate::attachments::Sender::new(capacity),
            &mut |owner, imports, resources| {
                resources.push(crate::attachments::resource(
                    owner,
                    imports,
                    &mut imported,
                    &mut local,
                ))
            },
            &mut |_, _, _| Ok(None),
        )
    }
    /// Combined maintenance for application pipelines and optional transport
    /// resources. Use this entry point when a producer supplies an early
    /// pipeline: every maintenance scan then observes it before the final result.
    /// The same caller task drives application futures, IO and these callbacks.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    #[inline(always)]
    pub fn poll_maintenance_with<R: Default>(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        budget: core::num::NonZeroUsize,
        resources: &mut impl FnMut() -> R,
        attach: &mut impl FnMut(
            &binding::Route<'id, P, S, PATH>,
            &imports::Imports<'id, '_, I>,
            &mut R,
        ) -> u8,
        resolve: &mut impl FnMut(
            &A,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<
            Option<Result<clients::Client<'id>, capnp::ErrorKind>>,
            pipelines::Error,
        >,
    ) -> Result<Progress<I, A, S>, Error> {
        self.poll_maintenance_budget_with(
            messages,
            queue,
            minimum,
            options,
            scratch,
            &mut budget.get(),
            resources,
            attach,
            resolve,
        )
    }
    /// Poll maintenance and subtract only visited work from a shared budget.
    /// A productive transition may stop before exhausting the allowance. The
    /// caller can spend the remainder on more ready work before flushing output.
    /// Each owner poll costs one step, including a bounded walk over preceding
    /// empty phases. A complete idle proof also costs one step. These walks
    /// invoke no callbacks and retain the rotating cursor; retained vacant
    /// capacity does not consume additional budget. Zero performs no work.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    #[inline(never)]
    pub fn poll_maintenance_budget_with<R: Default>(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        budget: &mut usize,
        resources: &mut impl FnMut() -> R,
        attach: &mut impl FnMut(
            &binding::Route<'id, P, S, PATH>,
            &imports::Imports<'id, '_, I>,
            &mut R,
        ) -> u8,
        resolve: &mut impl FnMut(
            &A,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<
            Option<Result<clients::Client<'id>, capnp::ErrorKind>>,
            pipelines::Error,
        >,
    ) -> Result<Progress<I, A, S>, Error> {
        self.maintenance_published_retirement = false;
        self.check_live()?;
        if *budget == 0 {
            return Ok(Progress::Yield);
        }
        if self.maintenance_is_idle() {
            *budget -= 1;
            self.scan_remaining = self.maintenance_total;
            return Ok(Progress::Idle);
        }
        // Storage growth preserves existing indices. Keep the current cursor so
        // repeated admissions cannot starve later phases; invalidate only the
        // idle certificate, which must include the newly admitted slots.
        let changed = [
            (2, self.pipelines.capacity().max(1)),
            (3, self.outgoing.capacity().max(1)),
            (4, self.ledgers.exports.capacity().max(1)),
            (6, self.ledgers.answers.capacity().max(1)),
            (8, self.exported.capacity().max(1)),
            (10, self.ledgers.imports.capacity().max(1)),
            (15, self.tails.len().max(1)),
            (16, self.invocation_pipelines.len().max(1)),
        ];
        if changed
            .iter()
            .any(|&(phase, size)| self.maintenance_sizes[phase] != size)
        {
            let mut sizes = self.maintenance_sizes;
            for (phase, size) in changed {
                sizes[phase] = size;
            }
            let total = sizes
                .iter()
                .try_fold(0usize, |sum, size| sum.checked_add(*size))
                .ok_or(Error::Input(super::super::Error::Capacity))?;
            self.maintenance_sizes = sizes;
            self.maintenance_total = total;
            self.scan_remaining = total;
        }
        let mut advanced = false;
        let mut retired_burst = 0;
        while *budget != 0 {
            match self.walk_idle_phases() {
                IdleWalk::Active => {}
                IdleWalk::Complete => {
                    *budget -= 1;
                    return Ok(if advanced {
                        Progress::Advanced
                    } else {
                        Progress::Idle
                    });
                }
            }
            let phase = self.maintenance_phase;
            let index = self.maintenance_index;
            *budget -= 1;
            self.maintenance_index += 1;
            if self.maintenance_index == self.maintenance_sizes[phase] {
                self.maintenance_index = 0;
                self.maintenance_phase = if phase + 1 == self.maintenance_sizes.len() {
                    0
                } else {
                    phase + 1
                };
            }
            let result = self.maintenance_step(
                phase, index, messages, queue, minimum, options, scratch, resources, attach,
                resolve,
            );
            match result {
                Ok(Some(Progress::Advanced)) if matches!(phase, 4 | 5) => {
                    // Export/endpoint retirement publishes no frame and returns
                    // no application owner. Complete its bounded reference-drop
                    // cascade here, rather than rebuilding every application
                    // lane between adjacent cleanup steps. Other progress,
                    // including returned owners and output, still stops here.
                    advanced = true;
                    self.scan_remaining = self.maintenance_total;
                    if phase == 5 {
                        retired_burst += 1;
                        if retired_burst < 8 && *budget != 0 && self.runtime.has_retired() {
                            self.maintenance_phase = 5;
                            self.maintenance_index = 0;
                        }
                    }
                }
                Ok(None) => {
                    self.scan_remaining -= 1;
                    if self.scan_remaining == 0 {
                        self.scan_remaining = self.maintenance_total;
                        return Ok(if advanced {
                            Progress::Advanced
                        } else {
                            Progress::Idle
                        });
                    }
                }
                outcome => {
                    self.scan_remaining = self.maintenance_total;
                    self.maintenance_published_retirement =
                        matches!(phase, 9 | 10) && outcome.is_ok();
                    return outcome.map(|progress| progress.unwrap());
                }
            }
        }
        Ok(if advanced {
            Progress::Advanced
        } else {
            Progress::Yield
        })
    }
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn maintenance_step<R: Default>(
        &mut self,
        phase: usize,
        index: usize,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        resources: &mut impl FnMut() -> R,
        attach: &mut impl FnMut(
            &binding::Route<'id, P, S, PATH>,
            &imports::Imports<'id, '_, I>,
            &mut R,
        ) -> u8,
        resolve: &mut impl FnMut(
            &A,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<
            Option<Result<clients::Client<'id>, capnp::ErrorKind>>,
            pipelines::Error,
        >,
    ) -> Result<Option<Progress<I, A, S>>, Error> {
        Ok(match phase {
            0 => self.retry_call()?.map(|_| Progress::Advanced),
            1 => self
                .retry_resolution(messages)?
                .then_some(Progress::Advanced),
            2 if index < self.pipelines.capacity() => {
                match self.poll_pipeline_with(index, messages, &mut *resolve)? {
                    Propagation::Unrelated | Propagation::Pending => None,
                    Propagation::Abandoned | Propagation::Resolved => Some(Progress::Advanced),
                }
            }
            3 if index < self.outgoing.capacity() => match self.poll_outgoing(index, messages)? {
                Propagation::Unrelated | Propagation::Pending => None,
                Propagation::Abandoned | Propagation::Resolved => Some(Progress::Advanced),
            },
            4 => self.drain_export(index).then_some(Progress::Advanced),
            5 => self
                .drain_endpoint()
                .map(|server| server.map_or(Progress::Advanced, Progress::Server)),
            6 => self.drain_answer(index, messages).map(Progress::Producer),
            7 => self
                .drain_canceled_response(messages)
                .then_some(Progress::Advanced),
            8 if index < self.exported.capacity() => match self
                .exported
                .poll_one_using(
                    index,
                    &mut self.runtime,
                    &mut self.ledgers.exports,
                    messages,
                    queue,
                    minimum,
                    options,
                    scratch,
                    resources(),
                    |owner, bundle| attach(owner, &self.ledgers.imports, bundle),
                    &self.descriptors,
                )
                .map_err(Error::Exported)?
            {
                binding::exporting::Progress::Vacant | binding::exporting::Progress::Pending => {
                    None
                }
                _ => Some(Progress::Advanced),
            },
            9 => controls::finish_one(
                &mut self.ledgers.questions,
                messages,
                queue,
                minimum,
                options,
            )
            .map_err(Error::Input)?
            .then_some(Progress::Advanced),
            10 => controls::release_one(
                index,
                &mut self.ledgers.imports,
                messages,
                queue,
                minimum,
                options,
            )
            .map_err(Error::Input)?
            .map(Progress::Import),
            11 => self.cancel_queued(messages, queue, minimum, options)?,
            12 => self
                .send_loopback(messages, queue, minimum, options)?
                .then_some(Progress::Advanced),
            13 => self.retry_barrier()?.then_some(Progress::Advanced),
            14 => self.retry_echo()?.then_some(Progress::Advanced),
            15 => self.drain_tail(index).then_some(Progress::Advanced),
            16 if index < self.invocation_pipelines.len() => {
                match self.poll_invocation_pipeline(index)? {
                    Propagation::Unrelated | Propagation::Pending => None,
                    Propagation::Abandoned | Propagation::Resolved => Some(Progress::Advanced),
                }
            }
            17 => self.poll_network()?.then_some(Progress::Advanced),
            _ => None,
        })
    }

    fn cancel_queued(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Progress<I, A, S>>, Error> {
        let pending = self
            .call
            .as_ref()
            .filter(|call| {
                self.ledgers
                    .answers
                    .cancellation_requested(call.answer())
                    .unwrap_or(false)
            })
            .map(|call| (call.answer(), None));
        let queued = pending.or_else(|| {
            (0..self.runtime.task_capacity()).find_map(|index| {
                let (task, owner) = self.runtime.queued_at(index)?;
                let call = owner.as_call()?;
                self.ledgers
                    .answers
                    .cancellation_requested(call.answer())
                    .unwrap_or(false)
                    .then_some((call.answer(), Some(task)))
            })
        });
        let Some((key, task)) = queued else {
            return Ok(None);
        };
        let output = queue
            .reserve()
            .map_err(|error| Error::Input(crate::owned::Error::Storage(error)))?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            calls::cancelled(builder, key.id())
        })
        .map_err(Error::Input)?;
        let retired = answer::send(key, &mut self.ledgers.answers, messages, frame, output)
            .map_err(Error::Input)?;
        // Only remove the owning request after its Canceled Return is accepted.
        let call = match task {
            Some(task) => self
                .runtime
                .cancel(task)
                .expect("selected queued request")
                .into_call()
                .unwrap_or_else(|_| unreachable!("selected Call")),
            None => self.call.take().expect("selected retained request"),
        };
        self.release_call(call, messages);
        Ok(Some(match retired {
            Some(state) => {
                Progress::Producer(state.release(messages).release(messages, &mut self.runtime))
            }
            None => Progress::Advanced,
        }))
    }
}

use super::*;

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Effects are latched until the next maintenance visit. Applying them does
    /// not require inferring which owners changed from a coarse Progress value.
    pub fn maintenance_effects(&self) -> MaintenanceEffects {
        self.maintenance_effects
    }
    /// Read readiness from producer-owned queues. There is no retained idle
    /// certificate: changes are visible on the next poll without invalidation.
    pub fn maintenance_is_idle(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        queue: &messages::Queue<'pool, '_, impl Sized>,
    ) -> bool {
        self.disconnected.is_none() && self.ready_work(messages, queue) == 0
    }
    /// Whether freed output storage can resume protocol maintenance. Control
    /// publications park their exact owner on exhaustion. Exported promises
    /// also publish Resolve frames, so retain their normal readiness check.
    /// Existing ready work and graph retirement must still be driven separately.
    pub fn output_may_unblock(&self) -> bool {
        self.control_waiting != 0 || !self.exported.is_empty()
    }
    fn queued_work(&self) -> u32 {
        let control = match self.runtime.ready_owner() {
            Some(Task::Loopback(_) | Task::Echo(_)) => true,
            Some(Task::ProvisionBarrier(_) | Task::VineBarrier(_)) => self.network.is_none(),
            _ => false,
        };
        // Queue-head tests are cheap and compile together. Compound producers
        // are checked separately, without a per-class dispatch loop.
        u32::from(self.call.is_some() || self.runtime.dispatch_is_pending())
            | (u32::from(self.resolution.is_some()) << 1)
            | (u32::from(!self.ledgers.exports.retirement_is_idle()) << 4)
            | (u32::from(self.runtime.has_retired()) << 5)
            | (u32::from(!self.ledgers.answers.reclamation_is_idle()) << 6)
            | (u32::from(!self.canceled_ready.is_empty()) << 7)
            | (u32::from(self.ledgers.questions.has_pending_finish()) << 9)
            | (u32::from(self.ledgers.imports.has_pending_release()) << 10)
            | (u32::from(self.ledgers.answers.has_queued_cancellations()) << 11)
            | (u32::from(control) << 12)
            | (u32::from(self.barrier.is_some()) << 13)
            | (u32::from(self.echo.is_some()) << 14)
    }
    fn ready_work(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        queue: &messages::Queue<'pool, '_, impl Sized>,
    ) -> u32 {
        let queued = self.filter_control_waits(self.queued_work(), messages, queue);
        // Inspect compound producers only when they precede the first ready
        // queue in the fair rotation. Rotating bits avoids an indirect dispatch
        // for every empty class. An empty queue mask admits all four producers.
        let distance = (queued as u16)
            .rotate_right(self.work_cursor)
            .trailing_zeros();
        let preceding = (((1u32 << distance) - 1) as u16).rotate_left(self.work_cursor);
        let candidates = u32::from(preceding) & ((1 << 2) | (1 << 3) | (1 << 8) | (1 << 15));
        #[cfg(test)]
        self.readiness_visited
            .set(self.readiness_visited.get() | candidates);
        let compound = (u32::from(
            candidates & (1 << 2) != 0
                && self
                    .pipelines
                    .has_ready(&self.ledgers.answers, &self.runtime),
        ) << 2)
            | (u32::from(candidates & (1 << 3) != 0 && !self.outgoing_maintenance_is_idle()) << 3)
            | (u32::from(
                candidates & (1 << 8) != 0
                    && self.exported.has_ready(
                        &self.runtime,
                        &self.ledgers.exports,
                        messages,
                        queue,
                    ),
            ) << 8)
            | (u32::from(candidates & (1 << 15) != 0 && !self.invocation_work_is_idle()) << 15);
        queued | self.filter_control_waits(compound, messages, queue)
    }
    fn select_work(
        &mut self,
        messages: &messages::Messages<'pool, '_, '_>,
        queue: &messages::Queue<'pool, '_, impl Sized>,
    ) -> Option<u32> {
        let ready = self.ready_work(messages, queue);
        if ready == 0 {
            return None;
        }
        let following = ready & (u32::MAX << self.work_cursor);
        let selected = if following != 0 { following } else { ready }.trailing_zeros();
        self.work_cursor = (selected + 1) & 15;
        Some(selected)
    }
    #[inline]
    pub(super) fn outgoing_maintenance_is_idle(&self) -> bool {
        self.outgoing_ready.is_empty()
            && !self.outgoing.has_events(&self.runtime)
            && !self
                .outgoing
                .retry_ready(|r| self.available_resources().ready(r))
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
    /// pipeline: its owner notification is observed before the final result.
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
            &mut Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
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
    /// Each selected owner costs one step. Empty work classes consume no
    /// steps; an entirely idle poll costs one step.
    /// Selection rotates among ready classes, including on resource errors.
    /// Zero performs no work. Callbacks may create new work in the same poll.
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
            &mut Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<
            Option<Result<clients::Client<'id>, capnp::ErrorKind>>,
            pipelines::Error,
        >,
    ) -> Result<Progress<I, A, S>, Error> {
        self.maintenance_effects = MaintenanceEffects::default();
        self.check_live()?;
        if *budget == 0 {
            return Ok(Progress::Yield);
        }
        if self.control_minimum != minimum {
            self.control_minimum = minimum;
            self.control_waiting = 0;
            self.control_waits = [None; 16];
        }
        let mut advanced = false;
        while *budget != 0 {
            let Some(work) = self.select_work(messages, queue) else {
                *budget -= 1;
                return Ok(if advanced {
                    Progress::Advanced
                } else {
                    Progress::Idle
                });
            };
            *budget -= 1;
            self.control_waiting &= !(1 << work);
            self.control_waits[work as usize] = None;
            self.maintenance_effects.selected(work);
            match self.maintenance_step(
                work, messages, queue, minimum, options, scratch, resources, attach, resolve,
            ) {
                Ok(Some(Progress::Advanced)) if matches!(work, 4 | 5) => {
                    // Continue reference-only retirement while charging each
                    // owner. Selection still rotates past any other ready work.
                    advanced = true;
                }
                Ok(None) => {}
                Err(error) if self.park_control(work, error, messages, queue) => {}
                outcome => {
                    return outcome.map(|progress| progress.expect("productive work"));
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
        work: u32,
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
            &mut Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<
            Option<Result<clients::Client<'id>, capnp::ErrorKind>>,
            pipelines::Error,
        >,
    ) -> Result<Option<Progress<I, A, S>>, Error> {
        Ok(match work {
            0 => {
                if self
                    .runtime
                    .advance_dispatch_one()
                    .map_err(Error::Dispatch)?
                {
                    Some(Progress::Advanced)
                } else {
                    self.retry_call()?.map(|_| Progress::Advanced)
                }
            }
            1 => self
                .retry_resolution(messages)?
                .then_some(Progress::Advanced),
            2 => {
                let Some(index) = self
                    .pipelines
                    .next_ready(&mut self.ledgers.answers, &mut self.runtime)
                else {
                    return Ok(None);
                };
                match self.poll_pipeline_with(index, messages, &mut *resolve) {
                    Err(_) if self.pipelines.resource_waiting(index) => None,
                    Err(error) => return Err(error),
                    Ok(Propagation::Unrelated | Propagation::Pending) => None,
                    Ok(Propagation::Abandoned | Propagation::Resolved) => Some(Progress::Advanced),
                }
            }
            3 => match self.poll_next_outgoing(messages)? {
                Propagation::Unrelated | Propagation::Pending => None,
                Propagation::Abandoned | Propagation::Resolved => Some(Progress::Advanced),
            },
            4 => self.drain_next_export().then_some(Progress::Advanced),
            5 => self
                .drain_endpoint()
                .map(|server| server.map_or(Progress::Advanced, Progress::Server)),
            6 => self.drain_next_answer(messages).map(Progress::Producer),
            7 => self
                .drain_canceled_response(messages)
                .then_some(Progress::Advanced),
            8 => {
                let Some(index) = self.exported.next_ready(
                    &mut self.runtime,
                    &self.ledgers.exports,
                    messages,
                    queue,
                ) else {
                    return Ok(None);
                };
                match self.exported.poll_one_using(
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
                    |owner| reflection::target(owner, self.imports.peer(), &self.ledgers.questions),
                ) {
                    Err(_) if self.exported.resource_waiting(index) => None,
                    Err(error) => return Err(Error::Exported(error)),
                    Ok(
                        binding::exporting::Progress::Vacant
                        | binding::exporting::Progress::Pending,
                    ) => None,
                    Ok(_) => Some(Progress::Advanced),
                }
            }
            9 => controls::finish_one(
                &mut self.ledgers.questions,
                messages,
                queue,
                minimum,
                options,
            )
            .map_err(Error::Input)?
            .then_some(Progress::Advanced),
            10 => {
                controls::release_one(&mut self.ledgers.imports, messages, queue, minimum, options)
                    .map_err(Error::Input)?
                    .map(Progress::Import)
            }
            11 => self.cancel_queued(messages, queue, minimum, options)?,
            12 => {
                let network = matches!(
                    self.runtime.ready_owner(),
                    Some(Task::ProvisionBarrier(_) | Task::VineBarrier(_))
                );
                if network {
                    self.poll_network()?
                } else {
                    self.send_loopback(messages, queue, minimum, options)?
                }
                .then_some(Progress::Advanced)
            }
            13 => self.retry_barrier()?.then_some(Progress::Advanced),
            14 => self.retry_echo()?.then_some(Progress::Advanced),

            15 => match self.poll_next_invocation_pipeline()? {
                Propagation::Unrelated | Propagation::Pending => None,
                Propagation::Abandoned | Propagation::Resolved => Some(Progress::Advanced),
            },
            _ => unreachable!("selected ready work"),
        })
    }

    fn cancel_queued(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Progress<I, A, S>>, Error> {
        let Some(key) = self.ledgers.answers.next_queued_cancellation() else {
            return Ok(None);
        };
        let task = self
            .ledgers
            .answers
            .get(key)
            .expect("queued cancellation")
            .owner
            .queued;
        let retained = self.call.as_ref().is_some_and(|call| call.answer() == key);
        let queued = task.is_some_and(|task| {
            self.runtime
                .queued_owner(task)
                .and_then(Task::as_call)
                .is_some_and(|call| call.answer() == key)
        });
        if !retained && !queued {
            // Bootstrap and externally owned producers have their own execution
            // policy. Charge one notification without walking unrelated tasks.
            self.ledgers
                .answers
                .acknowledge_queued_cancellation(key)
                .expect("live queued cancellation");
            return Ok(Some(Progress::Advanced));
        }
        let task = if queued { task } else { None };
        let output = queue
            .reserve()
            .map_err(|error| Error::Input(crate::owned::Error::Storage(error)))?;
        let redirected = self
            .ledgers
            .answers
            .execution_state(key)
            .expect("queued cancellation")
            .redirected;
        let frame = Frame::build(messages, minimum, options, |builder| {
            if redirected {
                calls::results_sent_elsewhere(builder, key.id())
            } else {
                calls::cancelled(builder, key.id())
            }
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
            None => {
                let call = self.call.take().expect("selected retained request");
                self.clear_call_target();
                call
            }
        };
        self.release_call(call, messages);
        Ok(Some(match retired {
            Some(state) => Progress::Producer(state.release(messages).release(
                messages,
                &mut self.runtime,
                |child| self.ledgers.questions.release(child),
            )),
            None => Progress::Advanced,
        }))
    }
}

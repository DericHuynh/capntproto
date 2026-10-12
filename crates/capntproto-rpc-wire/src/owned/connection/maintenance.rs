use super::*;
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
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
        self.check_live()?;
        // An ordinary settled client often has no protocol maintenance at all.
        // These are ownership facts, not cached assumptions about the last
        // poll. No callbacks run and no unbounded storage scan is introduced.
        // Larger/previously used tables retain the budgeted scan below.
        if self.call.is_none() && self.resolution.is_none()
            && self.pipelines.capacity() == 0 && self.outgoing.is_empty()
            && self.ledgers.exports.capacity() == 0 && !self.runtime.has_retired()
            && self.ledgers.answers.capacity() == 0
            // A retained bootstrap may coexist with an ordinary call. Keep
            // this shortcut bounded independently of configured concurrency.
            && self.exchanges.len() <= 8 && !self.exchanges.iter().any(|slot| slot.canceled)
            && self.exported.capacity() == 0
            && !self.ledgers.questions.has_pending_finish()
            && !self.ledgers.imports.has_pending_release()
            && self.runtime.queued() == 0 && self.barrier.is_none() && self.echo.is_none()
            && self.tails.is_empty() && self.invocation_pipelines.is_empty()
        {
            self.scan_remaining = self.maintenance_total;
            return Ok(Progress::Idle);
        }
        // Storage growth preserves existing indices. Keep the current cursor so
        // repeated admissions cannot starve later phases; invalidate only the
        // idle certificate, which must include the newly admitted slots.
        let mut sizes = self.maintenance_sizes;
        sizes[2] = self.pipelines.capacity().max(1);
        sizes[3] = self.outgoing.capacity().max(1);
        sizes[8] = self.exported.capacity().max(1);
        sizes[15] = self.tails.len().max(1);
        sizes[16] = self.invocation_pipelines.len().max(1);
        sizes[4] = self.ledgers.exports.capacity().max(1);
        sizes[6] = self.ledgers.answers.capacity().max(1);
        sizes[10] = self.ledgers.imports.capacity().max(1);
        if sizes != self.maintenance_sizes {
            self.maintenance_total = sizes
                .iter()
                .try_fold(0usize, |sum, size| sum.checked_add(*size))
                .ok_or(Error::Input(super::super::Error::Capacity))?;
            self.scan_remaining = self.maintenance_total;
            self.maintenance_sizes = sizes;
        }
        for _ in 0..budget.get() {
            let phase = self.maintenance_phase;
            let index = self.maintenance_index;
            self.maintenance_index += 1;
            if self.maintenance_index == self.maintenance_sizes[phase] {
                self.maintenance_index = 0;
                self.maintenance_phase = (phase + 1) % self.maintenance_sizes.len();
            }
            // Keep empty lanes out of the ownership-bearing transition call.
            // They still consume one budget unit and advance the idle scan in
            // exactly the same order. Inspect live storage on every step so a
            // newly admitted operation cannot be hidden by an old idle mask.
            let empty = match phase {
                0 => self.call.is_none(),
                1 => self.resolution.is_none(),
                2 => self.pipelines.capacity() == 0,
                3 => self.outgoing.is_empty(),
                4 => self.ledgers.exports.capacity() == 0,
                5 => !self.runtime.has_retired(),
                6 => self.ledgers.answers.capacity() == 0,
                7 => self.exchanges.is_empty(),
                8 => self.exported.capacity() == 0,
                9 => !self.ledgers.questions.has_pending_finish(),
                10 => !self.ledgers.imports.has_pending_release(),
                11 => self.call.is_none() && self.runtime.queued() == 0,
                12 | 17 => self.runtime.queued() == 0,
                13 => self.barrier.is_none(),
                14 => self.echo.is_none(),
                15 => self.tails.is_empty(),
                16 => self.invocation_pipelines.is_empty(),
                _ => false,
            };
            let result = if empty {
                Ok(None)
            } else {
                self.maintenance_step(
                    phase, index, messages, queue, minimum, options, scratch, resources, attach,
                    resolve,
                )
            };
            match result {
                Ok(None) => {
                    self.scan_remaining -= 1;
                    if self.scan_remaining == 0 {
                        self.scan_remaining = self.maintenance_total;
                        return Ok(Progress::Idle);
                    }
                }
                outcome => {
                    self.scan_remaining = self.maintenance_total;
                    return outcome.map(|progress| progress.unwrap());
                }
            }
        }
        Ok(Progress::Yield)
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

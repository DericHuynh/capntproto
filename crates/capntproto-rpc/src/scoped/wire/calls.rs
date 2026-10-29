//! Outgoing owned calls and local execution over one portable invocation ledger.
use super::*;
use capntproto_rpc_engine::clients::{Destination, Location};
use capntproto_rpc_wire::clients::{Endpoint, Route};
use std::num::NonZeroUsize;

type Peer<'p, 'id, 's, I, A, P, const C: usize, const H: usize> =
    Driver<'p, 'id, 's, I, A, P, Rc<dyn ClientHook>, C, H>;
type Local<'p, 'id, I, const C: usize, const H: usize> = connection::LocalCall<'p, 'id, I, C, H>;

// Use one callback type for immediate, notified and locally executed returns.
// Their wire completion logic is identical and should share generated code.
fn retain_local_hook(hook: &Rc<dyn ClientHook>) -> Option<Box<dyn ClientHook>> {
    Some(hook.add_ref())
}
enum Stage {
    Prepare,
    Execute(server::Execution),
    Collect,
}
struct Execution<'p, 'id, I, const C: usize, const H: usize> {
    call: Local<'p, 'id, I, C, H>,
    stage: Stage,
}
struct Entry<'p, 'id, I, const C: usize, const H: usize> {
    wire: WireCall<'p, 'id, I, C>,
    local: Option<Box<Execution<'p, 'id, I, C, H>>>,
    // Occupancy only; the full generation lives in the existing wire route.
    published: bool,
    #[cfg(test)]
    polls: usize,
    unpublished: bool,
    completion: Option<connection::InvocationKey<'id>>,
    binding_turn: bool,
    closing: bool,
    waker: Waker,
}

pub(super) enum Publication<'a, 'id> {
    Queued {
        direct: bool,
        native: Option<&'a Rc<server::NativeTail>>,
    },
    Immediate {
        target: &'a Client<'id>,
        method: DynamicCall,
        params: &'a connection::ImmediateParams<'a>,
        capability: Option<&'a Client<'id>>,
    },
}
pub(super) enum SendOutcome<'id> {
    /// The request remains owned by its caller until output releases capacity.
    Blocked(Error),
    Queued(connection::SentInvocation<'id>),
    Immediate(capntproto_rpc_engine::questions::Key<'id>),
}

pub(super) fn output_capacity_error(error: &connection::Error) -> bool {
    matches!(
        error,
        connection::Error::Input(owned::Error::Storage(
            messages::Error::NoBuffer | messages::Error::NoSendSlot
        ))
    )
}

pub(super) fn publication_error<'id>(
    error: connection::Error,
) -> capnp::Result<Option<SendOutcome<'id>>> {
    if output_capacity_error(&error) {
        Ok(Some(SendOutcome::Blocked(wire_error(error))))
    } else {
        Err(wire_error(error))
    }
}

/// Each successful step either advances work or delivers an owned completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Ready work remains after the shared budget; poll again.
    Yield,
    Idle,
    Advanced,
    /// A Call is in the output FIFO. `plain` means publication added no
    /// capability grants; it does not imply that the caller discarded pipelines.
    Published {
        plain: bool,
    },
    Completed,
}

/// Caller-polled owned call coordination. The portable invocation ledger owns
/// completion and protocol identity; these slots retain application futures and
/// their original result contexts. No second question/import/export table exists.
///
/// Poll alongside the incoming server worker, driver maintenance and IO. Foreign
/// hook imports must preserve membranes and pending-promise settlement. The caller
/// supplies an executor for protected local methods. Errors preserve work for
/// retry or explicit termination. Exact operation notifications drive local
/// execution, projections, cancellation and completion; admission and ordered
/// dispatch share a bounded rotating turn with those ready owners.
#[must_use = "poll calls and terminate them before releasing their driver"]
pub struct Calls<'s, 'wait, 'p, 'id, I, const C: usize, const H: usize> {
    invocations: connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
    entries: Vec<Option<Entry<'p, 'id, I, C, H>>>,
    live: usize,
    first_free: usize,
    published: usize,
    pending: Option<Box<Call>>,
    maximum: usize,
    admission_wait: Option<std::sync::Arc<super::ready::Signal>>,
    admission_parked: bool,
    ready: super::ready::Queue,
    lane: u8,
    notification_turn: bool,
    question_turn: bool,
    unpublished: usize,
    publication_blocked: bool,
    executor: Option<Rc<dyn capability::CallExecutor>>,
}
impl<'s, 'wait, 'p, 'id, I, const C: usize, const H: usize> Calls<'s, 'wait, 'p, 'id, I, C, H> {
    pub fn new(
        maximum: usize,
        executor: Option<Rc<dyn capability::CallExecutor>>,
    ) -> capnp::Result<Self> {
        Ok(Self {
            invocations: connection::Invocations::new_growing(0, maximum).map_err(wire_error)?,
            entries: Vec::new(),
            live: 0,
            first_free: 0,
            published: 0,
            pending: None,
            maximum,
            admission_wait: None,
            admission_parked: false,
            ready: super::ready::Queue::default(),
            lane: 0,
            notification_turn: true,
            question_turn: true,
            unpublished: 0,
            publication_blocked: false,
            executor,
        })
    }
    pub fn is_empty(&self) -> bool {
        self.pending.is_none() && self.live == 0
    }
    /// Capacity exhaustion is waitable only while accepted output can release
    /// storage. Without that fence, preserve the error (including impossible
    /// buffer sizes) instead of leaving a call asleep without a wakeup source.
    pub(super) fn retry_output(
        &self,
        result: Poll<Result<Progress, Box<Error>>>,
        pending: bool,
    ) -> Poll<Result<Progress, Box<Error>>> {
        if self.publication_blocked && pending && matches!(result, Poll::Ready(Err(_))) {
            Poll::Pending
        } else {
            result
        }
    }
    pub(super) fn can_batch_replies(&self) -> bool {
        self.published != 0
    }
    pub(super) fn has_plain_receivers(&self) -> bool {
        self.published != 0
    }
    pub(super) fn idle_after_publication(&self) -> bool {
        self.pending.is_none() && self.ready.is_empty()
    }
    /// Deliver an already-admitted reply directly to its registered owner.
    /// The wire owner fences unsettled pipeline observers; an incomplete fence
    /// retains its exact completion notification rather than bypassing ordering.
    /// The caller yields after completion and resumes ordinary maintenance
    /// before reading another frame, preserving cooperative fairness.
    #[inline(always)]
    pub(super) fn returned<A, P: Copy + Eq>(
        &mut self,
        completion: (capntproto_rpc_engine::questions::Key<'id>, Option<usize>),
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        _cx: &mut Context<'_>,
        mut refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
    ) -> capnp::Result<Option<bool>> {
        let (key, owner) = completion;
        let Some(index) = self.immediate_receiver(key, owner) else {
            return Ok(None);
        };
        if self.entries[index]
            .as_mut()
            .unwrap()
            .wire
            .complete_plain(key, driver, messages)?
        {
            self.retire_entry(index);
            return Ok(Some(true));
        }
        let waker = self.entries[index].as_ref().unwrap().waker.clone();
        match self.entries[index].as_mut().unwrap().wire.poll_using(
            hooks,
            driver,
            messages,
            &mut Context::from_waker(&waker),
            true,
            &mut refresh,
            retain_local_hook,
        ) {
            Poll::Pending => Ok(None),
            Poll::Ready(result) => {
                result?;
                self.retire_entry(index);
                Ok(Some(false))
            }
        }
    }
    #[inline(always)]
    pub(super) fn plain_receiver(
        &self,
        key: capntproto_rpc_engine::questions::Key<'id>,
        owner: Option<usize>,
    ) -> Option<usize> {
        owner.filter(|&index| {
            self.entries
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|entry| entry.local.is_none() && entry.wire.accepts_plain(key))
        })
    }
    #[inline(always)]
    pub(super) fn deliver_plain(
        &mut self,
        index: usize,
        message: Box<capntproto_rpc_wire::owned::PlainResults>,
    ) {
        let entry = self.entries[index].as_mut().expect("selected reply owner");
        entry.wire.deliver_plain(message);
        self.retire_entry(index);
    }
    #[inline(always)]
    fn immediate_receiver(
        &self,
        key: capntproto_rpc_engine::questions::Key<'id>,
        owner: Option<usize>,
    ) -> Option<usize> {
        owner.filter(|&index| {
            self.entries
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|entry| {
                    entry.local.is_none()
                        && entry.wire.task().is_none()
                        && entry.wire.question() == Some(key)
                })
        })
    }
    #[inline(always)]
    fn retire_entry(&mut self, index: usize) {
        self.ready.remove(index);
        let entry = self.entries[index].take().expect("live call owner");
        debug_assert!(
            entry.wire.projections.is_empty(),
            "direct routes retire with their owner"
        );
        if let Some(pipeline) = &entry.wire.pipeline {
            pipeline.operation.set(None);
        }
        self.unpublished -= usize::from(entry.unpublished);
        self.published -= usize::from(entry.published);
        self.live -= 1;
        self.first_free = self.first_free.min(index);
        drop(entry);
    }
    /// Advance allocation only after admission commits. Failed publication
    /// leaves the free position available; retirement restores the lowest
    /// hole without searching from zero for every admission.
    fn insert_entry(&mut self, index: usize, mut entry: Entry<'p, 'id, I, C, H>) {
        debug_assert_eq!(index, self.first_free);
        entry.waker = self.ready.insert(index, false);
        entry.completion = match entry.wire.route {
            CallRoute::Tracked { task, .. } => Some(task),
            _ => None,
        };
        if let Some(pipeline) = &entry.wire.pipeline {
            pipeline.operation.set(Some(index));
            pipeline.bind_owner(entry.waker.clone());
        }
        let canceled = entry.wire.call.as_mut().is_some_and(|call| {
            call.completion
                .poll_observed(&mut Context::from_waker(&entry.waker))
                .is_ready()
        });
        if !entry.published
            || canceled
            || entry
                .wire
                .pipeline
                .as_ref()
                .is_some_and(|pipeline| !pipeline.waiting.borrow().is_empty())
        {
            self.ready.push(index);
        }
        self.unpublished += usize::from(entry.unpublished);
        self.published += usize::from(entry.published);
        if index == self.entries.len() {
            self.entries.push(Some(entry));
        } else {
            debug_assert!(self.entries[index].is_none());
            self.entries[index] = Some(entry);
        }
        self.live += 1;
        self.first_free = self.entries[index + 1..]
            .iter()
            .position(Option::is_none)
            .map_or(self.entries.len(), |offset| index + 1 + offset);
    }
    pub(crate) fn has_unpublished(&self) -> bool {
        self.pending.is_some() || self.unpublished != 0
    }
    pub(crate) fn adopt_pipeline<A, P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        key: capntproto_rpc_engine::questions::Key<'id>,
        pipeline: Box<dyn PipelineHook>,
        mut fence: impl FnMut(Box<dyn ClientHook>, Box<dyn ClientHook>) -> Option<Box<dyn ClientHook>>,
    ) -> capnp::Result<()> {
        let Some(index) = driver.completion_owner(key) else {
            return Ok(());
        };
        let Some(entry) = self.entries.get(index).and_then(Option::as_ref) else {
            return Ok(());
        };
        let question = entry.wire.question().or_else(|| {
            entry
                .wire
                .task()
                .and_then(|task| driver.invocation_question(task))
        });
        if question != Some(key) {
            return Ok(());
        }
        let entry = self.entries[index]
            .as_mut()
            .expect("validated pipeline owner");
        entry
            .wire
            .projections
            .materialize(hooks, &mut driver.capabilities())?;
        if let Some(state) = &entry.wire.pipeline {
            let cached = {
                let cache = state.cached.borrow();
                cache
                    .first
                    .iter()
                    .chain(cache.rest.iter())
                    .filter_map(|(path, hook)| {
                        let hook = hook.as_any()?.downcast_ref::<Hook>()?;
                        Some((path.clone(), hook.0.clone()))
                    })
                    .collect::<Vec<_>>()
            };
            for (path, endpoint) in cached {
                hooks.fence_projection(
                    &mut driver.capabilities(),
                    endpoint,
                    pipeline.get_pipelined_cap(&path),
                    &mut fence,
                )?;
            }
            // Cached used paths keep their individual fence or original route.
            // Future, previously unused paths can take the authenticated route.
            drop(state.variant.replace(PipelineVariant::Adopted(pipeline)));
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn poll<A, P: Copy + Eq, R: Default>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        cx: &mut Context<'_>,
        budget: NonZeroUsize,
        import: impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'p,
                'id,
                '_,
                I,
                P,
                Rc<dyn ClientHook>,
                C,
                H,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Poll<capnp::Result<Progress>> {
        let result = self.poll_using(
            hooks,
            driver,
            messages,
            options,
            cx,
            &mut budget.get(),
            import,
            |hooks, driver| {
                hooks.collect_with_metadata(&mut driver.capabilities(), foreign::metadata)
            },
            |invocations, driver, messages, _credit, publication| {
                let direct = match publication {
                    Publication::Immediate {
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
                                    minimum,
                                    options,
                                    scratch,
                                    R::default(),
                                    |_, _, _| u8::MAX,
                                )
                                .map(|key| key.map(SendOutcome::Immediate))
                                .or_else(publication_error);
                        }
                        return driver
                            .try_send_immediate_borrowed(
                                target,
                                method,
                                params,
                                messages,
                                queue,
                                minimum,
                                options,
                                R::default(),
                            )
                            .map(|key| key.map(SendOutcome::Immediate))
                            .or_else(publication_error);
                    }
                    Publication::Queued { direct, .. } => direct,
                };
                if direct {
                    return driver
                        .send_invocation(messages, queue, minimum, options, scratch)
                        .map(|sent| sent.map(SendOutcome::Queued))
                        .or_else(publication_error);
                }
                invocations
                    .send_one(driver, messages, queue, minimum, options, scratch)
                    .map(|sent| sent.map(SendOutcome::Queued))
                    .or_else(publication_error)
            },
        );
        self.retry_output(result, !queue.is_empty())
            .map(|result| result.map_err(|error| *error))
    }
    /// Attach resources to the exact accepted Call using the same scheduler.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_with_attachments<A, P: Copy + Eq, R, const N: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, capntproto_rpc_wire::attachments::Sender<R, N>>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        cx: &mut Context<'_>,
        budget: NonZeroUsize,
        capacity: capntproto_rpc_wire::Capacity,
        mut imported_resource: impl FnMut(&I) -> Option<R>,
        mut local_resource: impl FnMut(&Rc<dyn ClientHook>) -> Option<R>,
        import: impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'p,
                'id,
                '_,
                I,
                P,
                Rc<dyn ClientHook>,
                C,
                H,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Poll<capnp::Result<Progress>> {
        let result = self.poll_using(
            hooks,
            driver,
            messages,
            options,
            cx,
            &mut budget.get(),
            import,
            |hooks, driver| {
                hooks.collect_with_metadata(&mut driver.capabilities(), foreign::metadata)
            },
            |invocations, driver, messages, _credit, publication| {
                let direct = match publication {
                    Publication::Immediate {
                        target,
                        method,
                        params,
                        capability,
                    } => {
                        // The fixed attachment adapter retains its ordinary
                        // queued descriptor transaction for capability tables.
                        if capability.is_some() {
                            return Ok(None);
                        }
                        return driver
                            .try_send_immediate_borrowed(
                                target,
                                method,
                                params,
                                messages,
                                queue,
                                minimum,
                                options,
                                capntproto_rpc_wire::attachments::Sender::new(capacity),
                            )
                            .map(|key| key.map(SendOutcome::Immediate))
                            .or_else(publication_error);
                    }
                    Publication::Queued { direct, .. } => direct,
                };
                if direct {
                    return driver
                        .send_invocation_with_attachments(
                            messages,
                            queue,
                            minimum,
                            options,
                            scratch,
                            capacity,
                            &mut imported_resource,
                            &mut local_resource,
                        )
                        .map(|sent| sent.map(SendOutcome::Queued))
                        .or_else(publication_error);
                }
                invocations
                    .send_one_with_attachments(
                        driver,
                        messages,
                        queue,
                        minimum,
                        options,
                        scratch,
                        capacity,
                        &mut imported_resource,
                        &mut local_resource,
                    )
                    .map(|sent| sent.map(SendOutcome::Queued))
                    .or_else(publication_error)
            },
        );
        self.retry_output(result, !queue.is_empty())
            .map(|result| result.map_err(|error| *error))
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn poll_using<A, P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
        cx: &mut Context<'_>,
        budget: &mut usize,
        mut import: impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'p,
                'id,
                '_,
                I,
                P,
                Rc<dyn ClientHook>,
                C,
                H,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        mut refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
        mut send: impl FnMut(
            &mut connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
            &mut Peer<'p, 'id, '_, I, A, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            Option<Rc<crate::admission::Permit>>,
            Publication<'_, 'id>,
        ) -> capnp::Result<Option<SendOutcome<'id>>>,
    ) -> Poll<Result<Progress, Box<Error>>> {
        self.publication_blocked = false;
        if let Some(kind) = driver.disconnected() {
            return Poll::Ready(Err(Box::new(Error::from_kind(kind))));
        }
        self.ready.register(cx.waker());
        if let Some(wait) = &self.admission_wait {
            wait.register(cx.waker());
        }
        hooks.register_next(cx.waker());
        let mut advanced = false;
        while *budget != 0 {
            *budget -= 1;
            let dispatch_ready = driver.runtime().dispatch_is_pending()
                || driver
                    .capabilities()
                    .ready()
                    .map_err(wire_error)?
                    .is_some_and(|ready| matches!(ready.owner(), connection::Task::Invoke(_)));
            let admission_ready = (!self.admission_parked
                || self
                    .admission_wait
                    .as_ref()
                    .is_some_and(|wait| wait.is_ready()))
                && self.live < self.maximum
                && self.invocations.has_capacity()
                && (self.pending.is_some() || !hooks.inbox.queue.borrow().is_empty());
            let owner_ready = !self.ready.is_empty()
                || driver.has_ready_completions()
                || driver.has_changed_invocations();
            let active = [admission_ready, dispatch_ready, owner_ready];
            let Some(lane) = (0..3)
                .map(|offset| (self.lane + offset) % 3)
                .find(|lane| active[*lane as usize])
            else {
                return if advanced {
                    Poll::Ready(Ok(Progress::Advanced))
                } else if self.is_empty() {
                    Poll::Ready(Ok(Progress::Idle))
                } else {
                    Poll::Pending
                };
            };
            self.lane = (lane + 1) % 3;
            let result = match lane {
                0 => {
                    refresh(hooks, driver);
                    self.admit(
                        hooks,
                        driver,
                        messages,
                        options,
                        cx,
                        &mut import,
                        &mut send,
                        &mut refresh,
                    )
                    .map_err(|e| *e)
                }
                1 => {
                    if driver
                        .capabilities()
                        .advance_dispatch_one()
                        .map_err(wire_error)?
                    {
                        Ok(Some(Progress::Advanced))
                    } else {
                        // A route transition selects its exact operation even
                        // when it became broken instead of locally callable.
                        let task =
                            driver
                                .capabilities()
                                .ready()
                                .map_err(wire_error)?
                                .and_then(|ready| match ready.owner() {
                                    connection::Task::Invoke(invocation) => Some(invocation.key()),
                                    _ => None,
                                });
                        if let Some(task) = task {
                            if let Some(index) = driver.invocation_completion_owner(task) {
                                if self
                                    .entries
                                    .get(index)
                                    .and_then(Option::as_ref)
                                    .is_some_and(|entry| entry.wire.task() == Some(task))
                                {
                                    self.ready.push(index);
                                }
                            }
                        }
                        match self.accept_local(driver, messages)? {
                            Some(progress) => Ok(Some(progress)),
                            None => self.send_next(driver, messages, &mut send),
                        }
                    }
                }
                _ => {
                    // Notifications and execution alternate, preserving budget
                    // one fairness even under continuous incoming replies.
                    if (self.notification_turn || self.ready.is_empty())
                        && (driver.has_ready_completions() || driver.has_changed_invocations())
                    {
                        self.notification_turn = false;
                        if (self.question_turn || !driver.has_changed_invocations())
                            && driver.has_ready_completions()
                        {
                            self.question_turn = false;
                            let (key, index) = driver.take_ready_completion().unwrap();
                            if self
                                .entries
                                .get(index)
                                .and_then(Option::as_ref)
                                .is_some_and(|entry| {
                                    entry.wire.question() == Some(key)
                                        || entry
                                            .wire
                                            .task()
                                            .and_then(|task| driver.invocation_question(task))
                                            == Some(key)
                                })
                            {
                                self.ready.push(index);
                            }
                        } else if let Some((task, index)) = driver.take_changed_invocation() {
                            self.question_turn = true;
                            if self
                                .entries
                                .get(index)
                                .and_then(Option::as_ref)
                                .is_some_and(|entry| {
                                    entry.wire.task() == Some(task)
                                        || entry.completion == Some(task)
                                })
                            {
                                self.ready.push(index);
                            }
                        }
                        Ok(Some(Progress::Advanced))
                    } else {
                        self.notification_turn = true;
                        self.poll_ready(hooks, driver, messages, options, &mut import, &mut refresh)
                    }
                }
            };
            match result {
                Err(error) => return Poll::Ready(Err(Box::new(error))),
                Ok(Some(progress @ (Progress::Completed | Progress::Published { .. }))) => {
                    return Poll::Ready(Ok(progress))
                }
                Ok(Some(_)) => advanced = true,
                Ok(None) => (),
            }
        }
        Poll::Ready(Ok(if advanced {
            Progress::Advanced
        } else {
            Progress::Yield
        }))
    }
    #[allow(clippy::too_many_arguments)]
    fn poll_ready<A, P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
        import: &mut impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'p,
                'id,
                '_,
                I,
                P,
                Rc<dyn ClientHook>,
                C,
                H,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        refresh: &mut impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
    ) -> capnp::Result<Option<Progress>> {
        let Some(ready) = self.ready.pop() else {
            return Ok(None);
        };
        let index = ready.index();
        let entry = self.entries[index].as_mut().expect("notified live call");
        #[cfg(test)]
        {
            entry.polls += 1;
        }
        let waker = entry.waker.clone();
        let mut cx = Context::from_waker(&waker);
        let result = if entry.binding_turn || entry.closing {
            entry.binding_turn = false;
            let binding = entry
                .completion
                .and_then(|task| driver.invocation_binding(task).map(|index| (task, index)));
            if let Some((task, binding)) = binding {
                let results = entry.wire.local_results.clone();
                self.invocations
                    .poll_one_with(binding, driver, messages, &mut cx, |seen, path, runtime| {
                        debug_assert_eq!(seen, task);
                        resolve(hooks, results.as_ref(), path, runtime, import)
                    })
                    .map_err(wire_error)
                    .map(|_| Some(Progress::Advanced))
            } else {
                entry.completion = None;
                Ok(Some(if entry.closing {
                    Progress::Completed
                } else {
                    Progress::Advanced
                }))
            }
        } else {
            entry.binding_turn = entry.completion.is_some();
            step(
                entry,
                &self.executor,
                hooks,
                driver,
                &mut self.invocations,
                messages,
                options,
                &mut cx,
                import,
                refresh,
            )
        };
        if entry.unpublished
            && (entry.published
                || entry
                    .local
                    .as_ref()
                    .is_some_and(|local| !matches!(local.stage, Stage::Prepare)))
        {
            entry.unpublished = false;
            self.unpublished -= 1;
        }
        let result = match result {
            Ok(Some(Progress::Completed))
                if entry
                    .completion
                    .and_then(|task| driver.invocation_binding(task))
                    .is_some() =>
            {
                entry.closing = true;
                Ok(Some(Progress::Advanced))
            }
            result => result,
        };
        match &result {
            Ok(Some(Progress::Completed)) => self.retire_entry(index),
            Ok(None) => (),
            _ => self.ready.push(index),
        }
        result
    }
    fn send_next<A, P: Copy + Eq>(
        &mut self,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        send: &mut impl FnMut(
            &mut connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
            &mut Peer<'p, 'id, '_, I, A, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            Option<Rc<crate::admission::Permit>>,
            Publication<'_, 'id>,
        ) -> capnp::Result<Option<SendOutcome<'id>>>,
    ) -> capnp::Result<Option<Progress>> {
        let mut dispatch = driver.capabilities();
        let Some(ready) = dispatch.ready().map_err(wire_error)? else {
            return Ok(None);
        };
        if !matches!(
            ready.destination(),
            Destination::Endpoint {
                location: Location::Remote(_),
                ..
            }
        ) {
            return Ok(None);
        }
        let connection::Task::Invoke(invocation) = ready.owner() else {
            return Ok(None);
        };
        let task = invocation.key();
        drop(ready);
        let Some(index) = driver.invocation_completion_owner(task) else {
            return Ok(None);
        };
        let Some(entry) = self
            .entries
            .get_mut(index)
            .and_then(Option::as_mut)
            .filter(|entry| entry.wire.task() == Some(task))
        else {
            return Ok(None);
        };
        let Some(call) = entry.wire.call.as_ref() else {
            return Ok(None);
        };
        if entry.wire.direct() && call.is_canceled() {
            entry.wire.cancel_direct(driver, messages)?;
            entry.wire.close_receiver();
            entry
                .wire
                .fail(Error::disconnected("wire call canceled".into()));
            return Ok(Some(Progress::Advanced));
        }
        let credit = match call
            .credit
            .as_ref()
            .map(|credit| credit.acquire())
            .transpose()
        {
            Ok(credit) => credit,
            Err(error) => {
                // Refusing one Call is a local reply, not a connection failure.
                // Close its binding before invocation maintenance can send it.
                entry.wire.close_receiver();
                entry.wire.cancel_direct(driver, messages)?;
                entry.wire.fail(error);
                return Ok(Some(Progress::Advanced));
            }
        };
        let direct = entry.wire.direct();
        let Some(sent) = send(
            &mut self.invocations,
            driver,
            messages,
            credit.clone(),
            Publication::Queued {
                direct,
                native: call.native_tail.as_ref(),
            },
        )?
        else {
            return Ok(None);
        };
        if let SendOutcome::Blocked(error) = sent {
            self.publication_blocked = true;
            return Err(error);
        }
        let SendOutcome::Queued(sent) = sent else {
            unreachable!("queued publication")
        };
        entry.published = true;
        if entry.unpublished {
            entry.unpublished = false;
            self.unpublished -= 1;
        }
        self.published += 1;
        self.ready.push(index);
        if direct {
            debug_assert_eq!(sent.task, task);
            entry.wire.route = CallRoute::Remote(sent.question);
        }
        if let Some(credit) = credit {
            driver
                .retain_call_credit(sent.question, credit)
                .map_err(wire_error)?;
        }
        entry
            .wire
            .call
            .as_mut()
            .expect("published call")
            .publication_committed();
        Ok(Some(Progress::Advanced))
    }
    fn accept_local<A, P: Copy + Eq>(
        &mut self,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<Option<Progress>> {
        let mut dispatch = driver.capabilities();
        let Some(ready) = dispatch.ready().map_err(wire_error)? else {
            return Ok(None);
        };
        if !matches!(
            ready.destination(),
            Destination::Endpoint {
                location: Location::Local,
                ..
            } | Destination::Endpoint {
                owner: Route::Capability(Endpoint::Failed { .. }),
                ..
            }
        ) {
            return Ok(None);
        }
        let connection::Task::Invoke(invocation) = ready.owner() else {
            return Ok(None);
        };
        let task = invocation.key();
        drop(ready);
        let Some(index) = driver.invocation_completion_owner(task) else {
            return Ok(None);
        };
        let Some(entry) = self
            .entries
            .get_mut(index)
            .and_then(Option::as_mut)
            .filter(|entry| entry.wire.task() == Some(task) && entry.local.is_none())
        else {
            return Ok(None);
        };
        if matches!(entry.wire.route, CallRoute::Queued(_)) {
            let (sender, receiver) = shared::channel();
            self.invocations
                .bind_queued(driver, messages, task, sender.into())
                .map_err(|rejected| wire_error(rejected.error))?;
            entry.wire.route = CallRoute::Tracked { task, receiver };
            entry.completion = Some(task);
            entry.binding_turn = true;
        }
        let Some(call) = self.invocations.accept_local(driver).map_err(wire_error)? else {
            return Ok(None);
        };
        if let Some(tail) = entry
            .wire
            .call
            .as_ref()
            .and_then(|call| call.native_tail.as_ref())
        {
            tail.forwarded();
        }
        self.ready.push(index);
        entry.wire.local_results = Some(server::Results::new());
        entry.local = Some(Box::new(Execution {
            call,
            stage: Stage::Prepare,
        }));
        Ok(Some(Progress::Advanced))
    }
    pub(super) fn retain_direct(
        &self,
        graph: &mut impl Capabilities<'id>,
        hook: &dyn ClientHook,
    ) -> capnp::Result<Option<Client<'id>>> {
        let Some(hook) = direct_response_hook(hook) else {
            return Ok(None);
        };
        let Some(target) = Self::direct_target(&self.entries, &hook.0) else {
            return Ok(None);
        };
        graph.retain_key(target.key()).map(Some).map_err(wire_error)
    }
    fn direct_target<'a>(
        entries: &'a [Option<Entry<'p, 'id, I, C, H>>],
        endpoint: &Rc<crate::scoped::Endpoint>,
    ) -> Option<&'a Client<'id>> {
        let parent = endpoint.direct_parent.borrow().upgrade()?;
        let entry = entries.get(parent.operation.get()?)?.as_ref()?;
        if !entry
            .wire
            .pipeline
            .as_ref()
            .is_some_and(|pipeline| Rc::ptr_eq(pipeline, &parent))
        {
            return None;
        }
        entry.wire.projections.target(endpoint)
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn admit<A, P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
        cx: &mut Context<'_>,
        import: &mut impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'p,
                'id,
                '_,
                I,
                P,
                Rc<dyn ClientHook>,
                C,
                H,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        send: &mut impl FnMut(
            &mut connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
            &mut Peer<'p, 'id, '_, I, A, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            Option<Rc<crate::admission::Permit>>,
            Publication<'_, 'id>,
        ) -> capnp::Result<Option<SendOutcome<'id>>>,
        refresh: &mut impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
    ) -> Result<Option<Progress>, Box<Error>> {
        if self.pending.is_none() {
            self.admission_wait = None;
            self.admission_parked = false;
            self.pending = match hooks.poll_owned_next(cx) {
                Poll::Ready(call) => call,
                Poll::Pending => None,
            };
            // poll_owned_next checks cancellation immediately before returning
            // its owner; no application callback intervenes here.
        } else if self.pending.as_ref().is_some_and(|call| call.is_canceled()) {
            drop(self.pending.take());
            self.admission_wait = None;
            self.admission_parked = false;
            return Ok(Some(Progress::Advanced));
        }
        if self.pending.is_none() {
            return Ok(None);
        }
        if !self.invocations.has_capacity() {
            // A tracked completion still owns its hosted slot until its
            // delivery observation and protocol cleanup have settled.
            // Preserve the new call until that slot is reclaimed.
            return Ok(None);
        }
        let index = self.first_free;
        if index == self.entries.len() {
            if index == self.maximum {
                // Accepted calls are bounded independently of the inbox. Keep
                // this request pending until an active completion frees a slot.
                return Ok(None);
            }
            self.entries.try_reserve(1).map_err(wire_error)?;
        }
        let call = self.pending.as_ref().expect("pending call");
        let failure = call.endpoint().failure.borrow().clone();
        if let Some(error) = failure {
            self.pending.take().unwrap().complete_boxed(Err(error));
            return Ok(Some(Progress::Advanced));
        }
        if (call.endpoint().needs_binding() && call.endpoint().direct.get().is_none())
            || call.params.capability_table().is_some_and(|caps| {
                caps.iter()
                    .flatten()
                    .any(|hook| hooks.has_pending_projection(&**hook))
            })
        {
            let wait = self.admission_wait.get_or_insert_with(Default::default);
            wait.register(cx.waker());
            wait.clear();
            let wake = Waker::from(wait.clone());
            let mut pending_cx = Context::from_waker(&wake);
            let call = self.pending.as_mut().expect("pending projection admission");
            let canceled = call.poll_canceled(&mut pending_cx).is_ready();
            let endpoint = call.endpoint().binding_pending(&mut pending_cx);
            let caps = call.params.capability_table().is_some_and(|caps| {
                caps.iter()
                    .flatten()
                    .any(|hook| hooks.projection_pending(&**hook, &mut pending_cx))
            });
            self.admission_parked = (endpoint || caps) && !canceled;
            return Ok(None);
        }
        self.admission_parked = false;
        self.admission_wait = None;
        let call = self.pending.as_ref().expect("ready admission");
        let direct_target = Self::direct_target(&self.entries, call.endpoint());
        if call.endpoint().index.get().is_none() && direct_target.is_none() {
            let forwarded = call
                .endpoint()
                .introduced
                .borrow()
                .clone()
                .expect("ready projection fallback");
            let cap = match hooks.try_retain(&mut driver.capabilities(), &*forwarded)? {
                Some(cap) => cap,
                None => import(&mut driver.capabilities(), &*forwarded)?,
            };
            hooks.bind_projection(&mut driver.capabilities(), cap, call.endpoint().clone())?;
        }
        let target = match direct_target {
            Some(target) => target,
            None => hooks.target(call)?,
        };
        if !call.hints.only_promise_pipeline
            && call.third_party.is_none()
            && call.native_tail.is_none()
            && call.endpoint().remote.get()
        {
            let call = self.pending.as_ref().expect("pending call");
            // This is only a refreshed routing hint. The driver's exclusive
            // immediate ticket validates routability, ordering, exact import
            // or question authority and peer identity before publishing bytes.
            let credit = match call
                .credit
                .as_ref()
                .map(|credit| credit.acquire())
                .transpose()
            {
                Ok(credit) => credit,
                Err(error) => {
                    self.pending.take().unwrap().complete_boxed(Err(error));
                    return Ok(Some(Progress::Advanced));
                }
            };
            let method = DynamicCall {
                interface: call.interface,
                method: call.method,
                no_promise_pipelining: call.hints.no_promise_pipelining,
                only_promise_pipeline: false,
            };
            let encoded = call
                .params
                .prepared_call()
                .map(|builder| {
                    if driver.uses_native_writer() {
                        builder.encoded_for_sending(method)
                    } else {
                        builder.encoded(method, options)
                    }
                })
                .transpose()
                .map_err(wire_error)?
                .flatten();
            let params = match encoded {
                Some(encoded) => connection::ImmediateParams::EncodedCall(encoded),
                None => connection::ImmediateParams::Content(call.params()?),
            };
            // A private one-slot table preserves its exact capability mapping.
            // Import through the ordinary authority/membrane path, then retain
            // that branded owner until transactional publication has finished.
            let retained = match (&params, call.params.capability_table()) {
                // Like legacy Request::send_internal/write_descriptors, use
                // the builder's capability table. Rewalking the entire payload
                // just to rediscover this live entry duplicates publication work.
                (connection::ImmediateParams::Content(_), Some([Some(hook)])) => Some(match hooks
                    .try_retain(&mut driver.capabilities(), &**hook)?
                {
                    Some(client) => client,
                    None => import(&mut driver.capabilities(), &**hook)?,
                }),
                _ => None,
            };
            let progress = Progress::Published {
                plain: retained.is_none(),
            };
            let publication = Publication::Immediate {
                target,
                method,
                params: &params,
                capability: retained.as_ref(),
            };
            let published = send(
                &mut self.invocations,
                driver,
                messages,
                credit.clone(),
                publication,
            );
            if let Some(capability) = retained {
                driver.capabilities().release(capability);
            }
            if let Some(published) = published? {
                if let SendOutcome::Blocked(error) = published {
                    self.publication_blocked = true;
                    return Err(Box::new(error));
                }
                let SendOutcome::Immediate(question) = published else {
                    unreachable!("immediate publication")
                };
                if let Some(credit) = credit {
                    driver
                        .retain_call_credit(question, credit)
                        .map_err(wire_error)?;
                }
                let mut call = self.pending.take().expect("published call");
                if let CallParams::OwnedRequest(request) = &mut call.params {
                    // Like C++ RpcRequest's target, this interest protects the
                    // request through send(). The child now owns its question
                    // and FIFO frame, so a parent Finish cannot overtake it.
                    request.publication_committed();
                }
                let pipeline = match hooks.attach_pipeline(&mut call) {
                    Ok(pipeline) => pipeline,
                    Err(error) => {
                        driver.cancel(question).map_err(wire_error)?;
                        call.complete_boxed(Err(error));
                        // The accepted Call still needs its FIFO flush, and its
                        // canceled question retains ordinary late-Return cleanup.
                        return Ok(Some(progress));
                    }
                };
                call.publication_committed();
                let entry = Entry {
                    wire: WireCall {
                        call: Some(call),
                        route: CallRoute::Remote(question),
                        pipeline,
                        projections: Default::default(),
                        local_results: None,
                    },
                    local: None,
                    published: true,
                    #[cfg(test)]
                    polls: 0,
                    unpublished: false,
                    completion: None,
                    binding_turn: false,
                    closing: false,
                    waker: Waker::noop().clone(),
                };
                driver
                    .bind_completion_owner(question, index)
                    .map_err(wire_error)?;
                self.insert_entry(index, entry);
                // Publication committed this question and its authority. Resolve
                // already-requested projections now, allowing their forwarded
                // calls to join the parent's pending transport batch. Retain the
                // owner above before callbacks, including on projection failure.
                if self.entries[index]
                    .as_ref()
                    .unwrap()
                    .wire
                    .pipeline
                    .is_some()
                {
                    self.entries[index]
                        .as_mut()
                        .unwrap()
                        .wire
                        .poll_pipelines_using(
                            hooks,
                            driver,
                            &mut self.invocations,
                            messages,
                            |_, _, _, _| Ok(None),
                            refresh,
                        )?;
                }
                return Ok(Some(progress));
            }
        }
        let parent = self
            .pending
            .as_ref()
            .and_then(|call| call.endpoint().direct_parent.borrow().upgrade());
        if let Some(parent) = parent {
            if let Some(operation) = parent.operation.get() {
                if let Some(entry) = self
                    .entries
                    .get_mut(operation)
                    .and_then(Option::as_mut)
                    .filter(|entry| {
                        entry
                            .wire
                            .pipeline
                            .as_ref()
                            .is_some_and(|pipeline| Rc::ptr_eq(pipeline, &parent))
                    })
                {
                    entry
                        .wire
                        .projections
                        .materialize(hooks, &mut driver.capabilities())?;
                    refresh(hooks, driver);
                }
            }
        }
        self.admit_queued(index, hooks, driver, messages, options, import)
            .map_err(Box::new)
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn admit_queued<A, P: Copy + Eq>(
        &mut self,
        index: usize,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
        import: &mut impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'p,
                'id,
                '_,
                I,
                P,
                Rc<dyn ClientHook>,
                C,
                H,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> capnp::Result<Option<Progress>> {
        let call = self.pending.take().expect("pending call");
        let wire = match hooks.enqueue_wire_using(
            call,
            driver,
            &mut self.invocations,
            messages,
            options,
            true,
            import,
        ) {
            Ok(wire) => wire,
            Err(rejected) => {
                self.pending = Some(rejected.call);
                self.publication_blocked = rejected.output_blocked;
                return Err(rejected.error);
            }
        };
        driver
            .bind_invocation_completion(wire.task().expect("queued invocation"), index)
            .map_err(wire_error)?;
        let entry = Entry {
            wire,
            local: None,
            published: false,
            #[cfg(test)]
            polls: 0,
            unpublished: true,
            completion: None,
            binding_turn: false,
            closing: false,
            waker: Waker::noop().clone(),
        };
        self.insert_entry(index, entry);
        Ok(Some(Progress::Advanced))
    }
    /// End tracked bindings, then release every externally running local call.
    /// Protected application tasks remain owned by the caller's executor.
    pub fn terminate<A, P: Copy + Eq>(
        self,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) {
        self.terminate_with_error(
            driver,
            messages,
            Error::disconnected("wire call canceled".into()),
        );
    }
    pub(crate) fn terminate_with_error<A, P: Copy + Eq>(
        self,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        error: Error,
    ) {
        if let Some(call) = self.pending {
            call.complete_boxed(Err(error.clone()));
        }
        self.ready.close();
        self.invocations.terminate(driver, messages);
        for mut entry in self.entries.into_iter().flatten() {
            entry.wire.release_tail(driver, messages);
            entry
                .wire
                .projections
                .release(&mut driver.capabilities(), &error);
            entry.wire.cancel_with_error(error.clone());
            if let Some(local) = entry.local {
                drop(local.stage);
                local.call.release(driver, messages);
            }
        }
    }
}

fn resolve<'p, 'id, I, P: Copy + Eq, const C: usize, const H: usize>(
    hooks: &mut ClientHooks<'id>,
    results: Option<&server::Results>,
    path: pipeline::Path<'_>,
    runtime: &mut capntproto_rpc_wire::owned::connection::Scope<
        '_,
        'p,
        'id,
        '_,
        I,
        P,
        Rc<dyn ClientHook>,
        C,
        H,
    >,
    import: &mut impl FnMut(
        &mut capntproto_rpc_wire::owned::connection::Scope<
            '_,
            'p,
            'id,
            '_,
            I,
            P,
            Rc<dyn ClientHook>,
            C,
            H,
        >,
        &dyn ClientHook,
    ) -> capnp::Result<Client<'id>>,
) -> Result<Option<Client<'id>>, connection::Error> {
    match results {
        None => Ok(None),
        Some(results) => hooks
            .server_pipeline(results, path, runtime, import)
            .map_err(|error| connection::Error::Input(owned::Error::Wire(error.kind))),
    }
}

#[allow(clippy::too_many_arguments)]
fn step<'p, 'id, I, A, P: Copy + Eq, const C: usize, const H: usize>(
    entry: &mut Entry<'p, 'id, I, C, H>,
    executor: &Option<Rc<dyn capability::CallExecutor>>,
    hooks: &mut ClientHooks<'id>,
    driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
    invocations: &mut connection::Invocations<'_, '_, 'p, 'id, I, C>,
    messages: &mut messages::Messages<'p, '_, '_>,
    options: capnp::message::ReaderOptions,
    cx: &mut Context<'_>,
    import: &mut impl FnMut(
        &mut capntproto_rpc_wire::owned::connection::Scope<
            '_,
            'p,
            'id,
            '_,
            I,
            P,
            Rc<dyn ClientHook>,
            C,
            H,
        >,
        &dyn ClientHook,
    ) -> capnp::Result<Client<'id>>,
    refresh: &mut impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
) -> capnp::Result<Option<Progress>> {
    let results = entry.wire.local_results.clone();
    entry.wire.poll_pipelines_using(
        hooks,
        driver,
        invocations,
        messages,
        |hooks, _, path, runtime| resolve(hooks, results.as_ref(), path, runtime, import),
        &mut *refresh,
    )?;
    let completed = match entry.wire.poll_using(
        hooks,
        driver,
        messages,
        cx,
        true,
        &mut *refresh,
        retain_local_hook,
    ) {
        Poll::Pending => false,
        Poll::Ready(result) => {
            result?;
            true
        }
    };
    let Some(local) = entry.local.as_mut() else {
        return Ok(completed.then_some(Progress::Completed));
    };
    let results = results.as_ref().expect("local result context");
    if completed
        || invocations
            .poll_local_canceled(driver, local.call.task(), cx)
            .map_err(wire_error)?
            .is_ready()
    {
        local.stage = Stage::Collect;
        results.cancel();
    }
    match &mut local.stage {
        Stage::Prepare => {
            let hook = match driver
                .runtime()
                .inspect(local.call.target())
                .map_err(wire_error)?
            {
                Destination::Endpoint {
                    owner:
                        Route::Capability(Endpoint::Local(hook) | Endpoint::Failed { owner: hook, .. })
                        | Route::Vine(hook),
                    ..
                } => hook.clone(),
                _ => {
                    return Err(Error::failed(
                        "accepted local invocation changed destination".into(),
                    ))
                }
            };
            let prepared = hooks.prepare_local_server(&local.call, driver, messages)?;
            refresh(hooks, driver);
            local.stage = Stage::Execute(prepared.execute(&*hook, results, executor.clone())?);
        }
        Stage::Execute(execution) => {
            let _ = results.poll_completed(cx);
            let pending = Pin::new(execution).poll(cx).is_pending();
            // Ordinary local hooks dispatch lazily on their first poll. Only
            // then may reflected resolution bypass older queued calls.
            if let Some(call) = entry.wire.call.as_mut() {
                call.dispatch_committed();
            }
            if pending {
                return Ok(None);
            }
            local.stage = Stage::Collect;
        }
        Stage::Collect => {
            if results.poll_completed(cx).is_pending() || results.projections_pending(hooks, cx) {
                return Ok(None);
            }
            let output =
                hooks.collect_server_results(results, driver, messages, options, import)?;
            let local = entry.local.take().expect("running local call");
            if let Err(rejected) = invocations.complete_local(driver, messages, local.call, output)
            {
                entry.local = Some(Box::new(Execution {
                    call: rejected.call,
                    stage: local.stage,
                }));
                if let Ok(output) = rejected.result {
                    output.release(messages, &mut driver.capabilities());
                }
                return Err(wire_error(rejected.error));
            }
        }
    }
    Ok(Some(Progress::Advanced))
}

#[cfg(test)]
mod tests;

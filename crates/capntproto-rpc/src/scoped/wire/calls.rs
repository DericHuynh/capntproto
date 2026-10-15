//! Outgoing owned calls and local execution over one portable invocation ledger.
use super::*;
use capntproto_rpc_engine::clients::{Destination, Location};
use capntproto_rpc_wire::clients::{Endpoint, Route};
use std::num::NonZeroUsize;

type Peer<'p, 'id, 's, I, A, P, const C: usize, const H: usize> =
    Driver<'p, 'id, 's, I, A, P, Rc<dyn ClientHook>, C, H>;
type Local<'p, 'id, I, const C: usize, const H: usize> = connection::LocalCall<'p, 'id, I, C, H>;
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
}

pub(super) enum Publication<'a, 'id> {
    Queued {
        direct: bool,
    },
    Immediate {
        target: &'a Client<'id>,
        method: DynamicCall,
        params: &'a connection::ImmediateParams<'a>,
    },
}
pub(super) enum Published<'id> {
    Queued(connection::SentInvocation<'id>),
    Immediate(capntproto_rpc_engine::questions::Key<'id>),
}

/// Each successful step either advances work or delivers an owned completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The bounded scan has more slots to visit; poll again.
    Yield,
    Idle,
    Advanced,
    Published,
    Completed,
}

/// Caller-polled owned call coordination. The portable invocation ledger owns
/// completion and protocol identity; these slots retain application futures and
/// their original result contexts. No second question/import/export table exists.
///
/// Poll alongside the incoming server worker, driver maintenance and IO. Foreign
/// hook imports must preserve membranes and pending-promise settlement. The caller
/// supplies an executor for protected local methods. Errors preserve work for
/// retry or explicit termination. A rotating bounded scan services admission,
/// output, local execution, projections, cancellation and completion bindings.
#[must_use = "poll calls and terminate them before releasing their driver"]
pub struct Calls<'s, 'wait, 'p, 'id, I, const C: usize, const H: usize> {
    invocations: connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
    entries: Vec<Option<Entry<'p, 'id, I, C, H>>>,
    pending: Option<Box<Call>>,
    maximum: usize,
    cursor: usize,
    remaining: usize,
    scan_size: usize,
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
            pending: None,
            maximum,
            cursor: 0,
            remaining: 0,
            scan_size: 0,
            executor,
        })
    }
    pub fn is_empty(&self) -> bool {
        self.pending.is_none()
            && self.entries.iter().all(Option::is_none)
            && self.invocations.is_empty()
    }
    pub(super) fn invalidate_scan(&mut self) {
        self.remaining = 0;
    }
    /// Immediately after publication, a lone direct call can only progress on
    /// input or an application wake (including cancellation). Do not apply this
    /// certificate to older entries, queued invocations or pipeline work. Call
    /// only after `Progress::Published`: admission just consumed `pending` and
    /// installed a direct Sent entry with no task, receiver or pipeline. If it
    /// is the sole slot, there cannot also be an older call needing a poll.
    pub(super) fn idle_after_publication(&self) -> bool {
        self.entries.len() == 1 && self.invocations.is_empty()
    }
    /// Deliver an already-admitted reply to its immediate owner. Bound the
    /// lookup; mixed/pipelined workloads continue through the rotating scanner.
    /// The caller yields after completion and resumes ordinary maintenance
    /// before reading another frame, preserving cooperative fairness.
    #[inline(always)]
    pub(super) fn returned<A, P: Copy + Eq>(
        &mut self,
        key: capntproto_rpc_engine::questions::Key<'id>,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        cx: &mut Context<'_>,
        refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
    ) -> capnp::Result<Option<bool>> {
        if self.entries.len() > 8 {
            return Ok(None);
        }
        let Some(index) = self.entries.iter().position(|entry| entry.as_ref().is_some_and(|entry|
            entry.local.is_none() && entry.wire.task.is_none() && entry.wire.pipeline.is_none()
                && matches!(entry.wire.direct, Some(DirectStage::Sent(question)) if question == key))) else {
            return Ok(None);
        };
        if self.entries[index]
            .as_mut()
            .unwrap()
            .wire
            .complete_plain(key, driver, messages)?
        {
            self.entries[index] = None;
            return Ok(Some(true));
        }
        match self.entries[index].as_mut().unwrap().wire.poll_using(
            hooks,
            driver,
            messages,
            cx,
            true,
            refresh,
            |hook| Some(hook.add_ref()),
        ) {
            Poll::Pending => Ok(None),
            Poll::Ready(result) => {
                result?;
                self.entries[index] = None;
                Ok(Some(false))
            }
        }
    }
    #[inline(always)]
    pub(super) fn plain_receiver(
        &self,
        key: capntproto_rpc_engine::questions::Key<'id>,
    ) -> Option<usize> {
        if self.entries.len() > 8 {
            return None;
        }
        self.entries.iter().position(|entry| {
            entry
                .as_ref()
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
        self.entries[index] = None;
    }
    pub(crate) fn has_unpublished(&self) -> bool {
        self.pending.is_some()
            || self.invocations.has_queued()
            || self.entries.iter().flatten().any(|entry| {
                matches!(entry.wire.direct, Some(DirectStage::Queued))
                    || entry
                        .local
                        .as_ref()
                        .is_some_and(|local| matches!(local.stage, Stage::Prepare))
            })
    }
    pub(crate) fn adopt_pipeline(
        &mut self,
        key: capntproto_rpc_engine::questions::Key<'id>,
        pipeline: Box<dyn PipelineHook>,
    ) {
        for entry in self.entries.iter().flatten() {
            if entry
                .wire
                .task
                .and_then(|task| self.invocations.question_for(task))
                == Some(key)
            {
                // Once any path has been used, preserve the original ordering
                // route. An independent-path Join fence can shorten it later.
                if let Some(state) = &entry.wire.pipeline {
                    if !state.used.get() {
                        let previous = state.adopted.replace(Some(pipeline));
                        drop(previous);
                    }
                }
                return;
            }
        }
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
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Poll<capnp::Result<Progress>> {
        self.poll_using(
            hooks,
            driver,
            messages,
            options,
            cx,
            budget,
            import,
            |hooks, driver| hooks.collect_with_metadata(&mut driver.runtime, foreign::metadata),
            |invocations, driver, messages, _credit, publication| {
                let direct = match publication {
                    Publication::Immediate {
                        target,
                        method,
                        params,
                    } => {
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
                            .map(|key| key.map(Published::Immediate))
                            .map_err(wire_error)
                    }
                    Publication::Queued { direct } => direct,
                };
                if direct {
                    return driver
                        .send_invocation(messages, queue, minimum, options, scratch)
                        .map(|sent| sent.map(Published::Queued))
                        .map_err(wire_error);
                }
                invocations
                    .send_one(driver, messages, queue, minimum, options, scratch)
                    .map(|sent| sent.map(Published::Queued))
                    .map_err(wire_error)
            },
        )
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
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Poll<capnp::Result<Progress>> {
        self.poll_using(
            hooks,
            driver,
            messages,
            options,
            cx,
            budget,
            import,
            |hooks, driver| hooks.collect_with_metadata(&mut driver.runtime, foreign::metadata),
            |invocations, driver, messages, _credit, publication| {
                let direct = match publication {
                    Publication::Immediate {
                        target,
                        method,
                        params,
                    } => {
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
                            .map(|key| key.map(Published::Immediate))
                            .map_err(wire_error)
                    }
                    Publication::Queued { direct } => direct,
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
                        .map(|sent| sent.map(Published::Queued))
                        .map_err(wire_error);
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
                    .map(|sent| sent.map(Published::Queued))
                    .map_err(wire_error)
            },
        )
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
        budget: NonZeroUsize,
        mut import: impl FnMut(
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        mut refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
        mut send: impl FnMut(
            &mut connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
            &mut Peer<'p, 'id, '_, I, A, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            Option<Rc<crate::admission::Permit>>,
            Publication<'_, 'id>,
        ) -> capnp::Result<Option<Published<'id>>>,
    ) -> Poll<Result<Progress, Box<Error>>> {
        if let Some(kind) = driver.disconnected() {
            return Poll::Ready(Err(Box::new(Error::from_kind(kind))));
        }
        let mut advanced = false;
        let mut remaining_budget = budget.get();
        if self.is_empty() {
            // With no owned invocation there is no local execution, wire
            // publication or completion to scan. Admission is the only source
            // of work; it still registers the inbox waker before returning idle.
            refresh(hooks, driver);
            match self.admit(hooks, driver, messages, options, cx, &mut import, &mut send)? {
                Some(Progress::Advanced) => {
                    // Queued/pipelined calls still need preparation and
                    // publication. Spend the rest of this same quantum on
                    // those stages instead of yielding after admission.
                    advanced = true;
                    remaining_budget -= 1;
                    self.remaining = 0;
                    self.cursor = 1;
                }
                progress => return Poll::Ready(Ok(progress.unwrap_or(Progress::Idle))),
            }
        }
        self.poll_nonempty(
            hooks,
            driver,
            messages,
            options,
            cx,
            budget,
            advanced,
            remaining_budget,
            import,
            refresh,
            send,
        )
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn poll_nonempty<A, P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
        cx: &mut Context<'_>,
        budget: NonZeroUsize,
        mut advanced: bool,
        mut remaining_budget: usize,
        mut import: impl FnMut(
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        mut refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, A, P, C, H>),
        mut send: impl FnMut(
            &mut connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
            &mut Peer<'p, 'id, '_, I, A, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            Option<Rc<crate::admission::Permit>>,
            Publication<'_, 'id>,
        ) -> capnp::Result<Option<Published<'id>>>,
    ) -> Poll<Result<Progress, Box<Error>>> {
        // Immediate settled calls have no queued dispatch, projection, local
        // execution or internal completion channel. Visit only their actual
        // owners and admission, retaining a rotating cursor and the caller's
        // budget. Bound the eligibility check itself; larger/mixed workloads
        // keep the general resumable scan below.
        if !advanced
            && self.entries.len() <= 8
            && budget.get() > self.entries.len()
            && self.invocations.is_empty()
            && driver.runtime.queued() == 0
            && self.entries.iter().flatten().all(|entry| {
                entry.local.is_none()
                    && entry.wire.task.is_none()
                    && entry.wire.pipeline.is_none()
                    && matches!(entry.wire.direct, Some(DirectStage::Sent(_)))
            })
        {
            let size = self.entries.len() + 1;
            for _ in 0..size {
                remaining_budget -= 1;
                let index = self.cursor % size;
                self.cursor = (index + 1) % size;
                if index == 0 {
                    refresh(hooks, driver);
                    if let Some(progress) =
                        self.admit(hooks, driver, messages, options, cx, &mut import, &mut send)?
                    {
                        if progress != Progress::Advanced {
                            return Poll::Ready(Ok(progress));
                        }
                        // A new local/pipelined call needs the remaining
                        // dispatch allowance in this same turn.
                        advanced = true;
                        self.remaining = 0;
                        self.cursor = 1;
                        break;
                    }
                } else if let Some(entry) = self.entries[index - 1].as_mut() {
                    if let Poll::Ready(result) = entry.wire.poll_using(
                        hooks,
                        driver,
                        messages,
                        cx,
                        true,
                        &mut refresh,
                        |hook| Some(hook.add_ref()),
                    ) {
                        result?;
                        self.entries[index - 1] = None;
                        return Poll::Ready(Ok(Progress::Completed));
                    }
                }
            }
            if !advanced {
                return if self.is_empty() {
                    Poll::Ready(Ok(Progress::Idle))
                } else {
                    Poll::Pending
                };
            }
        }
        for _ in 0..remaining_budget {
            let size = 3 + self.entries.len() + self.invocations.capacity();
            if self.scan_size != size || self.remaining == 0 {
                self.scan_size = size;
                self.remaining = size;
                self.cursor %= size;
            }
            let index = self.cursor;
            self.cursor = (index + 1) % size;
            // Advance even on error, allowing cleanup to free exhausted resources.
            let result = match index {
                0 => {
                    refresh(hooks, driver);
                    self.admit(hooks, driver, messages, options, cx, &mut import, &mut send)
                        .map_err(|error| *error)
                }
                1 => self.accept_local(driver),
                2 => self.send_next(driver, messages, &mut send),
                // Resolve wire completions before polling their hosted
                // receivers. Otherwise every reply takes an extra full scan
                // through admission and dispatch before the caller can see it.
                index if index >= 3 + self.invocations.capacity() => {
                    let index = index - 3 - self.invocations.capacity();
                    match self.entries[index].as_mut() {
                        None => Ok(None),
                        Some(entry) => {
                            let result = step(
                                entry,
                                &self.executor,
                                hooks,
                                driver,
                                &mut self.invocations,
                                messages,
                                options,
                                cx,
                                &mut import,
                                &mut refresh,
                            );
                            if matches!(result, Ok(Some(Progress::Completed))) {
                                self.entries[index] = None;
                            }
                            result
                        }
                    }
                }
                index => {
                    let entries = &self.entries;
                    // These completion receivers live in entries below and
                    // are consumed/canceled only by this same bounded scan.
                    // Progress resets the scan; budget exhaustion yields to
                    // the caller. Internal transfer needs no application wake.
                    let mut completion_cx = Context::from_waker(Waker::noop());
                    self.invocations
                        .poll_one_with(
                            index - 3,
                            driver,
                            messages,
                            &mut completion_cx,
                            |task, path, runtime| {
                                let results = entries
                                    .iter()
                                    .flatten()
                                    .find(|e| e.wire.task == Some(task))
                                    .and_then(|e| e.wire.local_results.as_ref());
                                resolve(hooks, results, path, runtime, &mut import)
                            },
                        )
                        .map(|progress| match progress {
                            connection::CallProgress::Idle | connection::CallProgress::Pending => {
                                None
                            }
                            _ => Some(Progress::Advanced),
                        })
                        .map_err(wire_error)
                }
            };
            match result {
                Err(error) => {
                    self.remaining = size;
                    return Poll::Ready(Err(Box::new(error)));
                }
                Ok(Some(progress)) => {
                    self.remaining = size;
                    // Use the remaining allowance for ready preparation and
                    // completion. Stop after publishing: the caller reserved
                    // FIFO capacity for exactly one outgoing frame.
                    if index == 2 || matches!(progress, Progress::Completed | Progress::Published) {
                        return Poll::Ready(Ok(progress));
                    }
                    advanced = true;
                    continue;
                }
                Ok(None) => (),
            }
            self.remaining -= 1;
            if self.remaining == 0 {
                return if advanced {
                    Poll::Ready(Ok(Progress::Advanced))
                } else if self.is_empty() {
                    Poll::Ready(Ok(Progress::Idle))
                } else {
                    Poll::Pending
                };
            }
        }
        Poll::Ready(Ok(if advanced {
            Progress::Advanced
        } else {
            Progress::Yield
        }))
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
        ) -> capnp::Result<Option<Published<'id>>>,
    ) -> capnp::Result<Option<Progress>> {
        let Some(ready) = driver.runtime.next_ready().map_err(wire_error)? else {
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
        let task = ready.key();
        drop(ready);
        let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.wire.task == Some(task))
        else {
            return Ok(None);
        };
        let Some(call) = entry.wire.call.as_ref() else {
            return Ok(None);
        };
        if entry.wire.direct.is_some() && call.is_canceled() {
            entry.wire.cancel_direct(driver, messages)?;
            drop(entry.wire.receiver.take());
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
                drop(entry.wire.receiver.take());
                entry.wire.cancel_direct(driver, messages)?;
                entry.wire.fail(error);
                return Ok(Some(Progress::Advanced));
            }
        };
        let direct = entry.wire.direct.is_some();
        let Some(sent) = send(
            &mut self.invocations,
            driver,
            messages,
            credit.clone(),
            Publication::Queued { direct },
        )?
        else {
            return Ok(None);
        };
        let Published::Queued(sent) = sent else {
            unreachable!("queued publication")
        };
        if direct {
            debug_assert_eq!(sent.task, task);
            entry.wire.direct = Some(DirectStage::Sent(sent.question));
        }
        if let Some(credit) = credit {
            driver
                .ledgers
                .questions
                .retain_credit(sent.question, credit)
                .map_err(wire_error)?;
        }
        Ok(Some(Progress::Advanced))
    }
    fn accept_local<A, P: Copy + Eq>(
        &mut self,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
    ) -> capnp::Result<Option<Progress>> {
        let Some(ready) = driver.runtime.next_ready().map_err(wire_error)? else {
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
        let task = ready.key();
        drop(ready);
        let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|e| e.wire.task == Some(task) && e.local.is_none())
        else {
            return Ok(None);
        };
        let Some(call) = self.invocations.accept_local(driver).map_err(wire_error)? else {
            return Ok(None);
        };
        entry.wire.local_results = Some(server::Results::new());
        entry.local = Some(Box::new(Execution {
            call,
            stage: Stage::Prepare,
        }));
        Ok(Some(Progress::Advanced))
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn admit<A, P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, A, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
        cx: &mut Context<'_>,
        import: &mut impl FnMut(
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        send: &mut impl FnMut(
            &mut connection::Invocations<'s, 'wait, 'p, 'id, I, C>,
            &mut Peer<'p, 'id, '_, I, A, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            Option<Rc<crate::admission::Permit>>,
            Publication<'_, 'id>,
        ) -> capnp::Result<Option<Published<'id>>>,
    ) -> Result<Option<Progress>, Box<Error>> {
        if self.pending.is_none() {
            self.pending = match hooks.poll_owned_next(cx) {
                Poll::Ready(call) => call,
                Poll::Pending => None,
            };
            // poll_owned_next checks cancellation immediately before returning
            // its owner; no application callback intervenes here.
        } else if self.pending.as_ref().is_some_and(|call| call.is_canceled()) {
            drop(self.pending.take());
            return Ok(Some(Progress::Advanced));
        }
        if self.pending.is_none() {
            return Ok(None);
        }
        if !self.invocations.has_capacity() {
            // The hosted receiver may have just completed while its tracked
            // binding still awaits cleanup later in this rotating scan.
            // Preserve the new call until that slot is reclaimed.
            return Ok(None);
        }
        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.entries.len());
        if index == self.entries.len() {
            if index == self.maximum {
                // Accepted calls are bounded independently of the inbox. Keep
                // this request pending until an active completion frees a slot.
                return Ok(None);
            }
            self.entries.try_reserve(1).map_err(wire_error)?;
        }
        let call = self.pending.as_ref().expect("pending call");
        let target = hooks.target(call)?;
        if call.hints.no_promise_pipelining
            && !call.hints.only_promise_pipeline
            && call.third_party.is_none()
            && call.endpoint.remote.get()
        {
            // This is only a refreshed routing hint. The driver's exclusive
            // immediate ticket validates settlement, ordering, exact import
            // authority and peer identity before publishing any bytes.
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
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            };
            let encoded = call
                .params
                .prepared_call()
                .map(|builder| builder.encoded(method, options))
                .transpose()
                .map_err(wire_error)?
                .flatten();
            let params = match encoded {
                Some(encoded) => connection::ImmediateParams::EncodedCall(encoded),
                None => connection::ImmediateParams::Content(call.params()?),
            };
            let publication = Publication::Immediate {
                target,
                method,
                params: &params,
            };
            if let Some(published) = send(
                &mut self.invocations,
                driver,
                messages,
                credit.clone(),
                publication,
            )? {
                let Published::Immediate(question) = published else {
                    unreachable!("immediate publication")
                };
                if let Some(credit) = credit {
                    driver
                        .ledgers
                        .questions
                        .retain_credit(question, credit)
                        .map_err(wire_error)?;
                }
                let entry = Some(Entry {
                    wire: WireCall {
                        call: self.pending.take(),
                        task: None,
                        receiver: None,
                        direct: Some(DirectStage::Sent(question)),
                        pipeline: None,
                        local_results: None,
                        tail_response: None,
                    },
                    local: None,
                });
                if index == self.entries.len() {
                    self.entries.push(entry);
                } else {
                    self.entries[index] = entry;
                }
                return Ok(Some(Progress::Published));
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
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> capnp::Result<Option<Progress>> {
        let call = self.pending.take().expect("pending call");
        let wire = match hooks.enqueue_wire_using(
            *call,
            driver,
            &mut self.invocations,
            messages,
            options,
            true,
            import,
        ) {
            Ok(wire) => wire,
            Err(rejected) => {
                self.pending = Some(Box::new(rejected.call));
                return Err(rejected.error);
            }
        };
        let entry = Some(Entry { wire, local: None });
        if index == self.entries.len() {
            self.entries.push(entry);
        } else {
            self.entries[index] = entry;
        }
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
        self.invocations.terminate(driver, messages);
        for mut entry in self.entries.into_iter().flatten() {
            entry.wire.release_tail(driver, messages);
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
    runtime: &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
    import: &mut impl FnMut(
        &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
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
        &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
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
    let completed =
        match entry
            .wire
            .poll_using(hooks, driver, messages, cx, true, &mut *refresh, |hook| {
                Some(hook.add_ref())
            }) {
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
            .poll_local_canceled(local.call.task(), cx)
            .map_err(wire_error)?
            .is_ready()
    {
        local.stage = Stage::Collect;
        results.cancel();
    }
    match &mut local.stage {
        Stage::Prepare => {
            let hook = match driver
                .runtime
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
            if Pin::new(execution).poll(cx).is_pending() {
                return Ok(None);
            }
            local.stage = Stage::Collect;
        }
        Stage::Collect => {
            if results.poll_completed(cx).is_pending() {
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
                    output.release(messages, &mut driver.runtime);
                }
                return Err(wire_error(rejected.error));
            }
        }
    }
    Ok(Some(Progress::Advanced))
}

#[cfg(test)]
mod tests;

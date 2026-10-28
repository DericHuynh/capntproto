//! Caller-polled execution of incoming Calls. The portable answer ledger is the
//! only protocol authority; these slots own accepted application work, not IDs.
use super::*;
use capntproto_rpc_engine::{answers, clients};
use capntproto_rpc_wire::clients::{Endpoint, Route};
use std::num::NonZeroUsize;

use super::super::ready;

type Running<'p, 'id, I, const C: usize, const P: usize> =
    dispatch::Running<'id, connection::Request<'p, 'id, I, C, P>>;
type Peer<'p, 'id, 's, I, P, const C: usize, const H: usize> =
    Driver<'p, 'id, 's, I, Producer, P, Rc<dyn ClientHook>, C, H>;

enum Stage {
    Prepare,
    Execute(Execution),
    Transfer(Work),
    Collect,
    Publish,
}
struct Entry<'p, 'id, I, const C: usize, const H: usize> {
    running: Running<'p, 'id, I, C, H>,
    results: Results,
    stage: Stage,
    waker: Waker,
    #[cfg(test)]
    polls: usize,
    result_revision: u64,
    redirect_work: capntproto_rpc_engine::work_queue::Link,
}
impl<'p, 'id, I, const C: usize, const H: usize> Entry<'p, 'id, I, C, H> {
    fn request(&self) -> &connection::Request<'p, 'id, I, C, H> {
        self.running.owner()
    }
}

/// One bounded worker outcome. Pending work registers application wakeups;
/// output/capacity errors retain the operation and advance the fairness cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Ready operations remain after the shared budget; poll again.
    Yield,
    Idle,
    Advanced,
    Replied,
}

/// Incoming service execution for a hosted connection using the portable driver.
///
/// Call `accept` for ready incoming Calls and `poll` alongside IO, invocation
/// delivery and `Driver::poll_maintenance_with`. Protocol transitions notify
/// the exact registered producer; spurious polls do not recheck parked
/// operations. The maintenance resolver must
/// use `ClientHooks::server_pipeline` so explicit early pipelines win over final
/// results. Other dispatcher tasks and authenticated third-party answers remain
/// with their caller. Local endpoint owners are the original policy-preserving
/// hooks. Remote destinations are forwarded through the same owned hook inbox.
///
/// Limits apply to running application work, independently of ledger capacity.
/// Before driver teardown, stop IO and consume this worker with `terminate`.
/// Dropping it without termination requires ending the entire driver scope.
#[must_use = "poll accepted work and terminate it before releasing the driver"]
pub struct Worker<'p, 'id, I, const C: usize, const H: usize> {
    entries: Vec<Option<Entry<'p, 'id, I, C, H>>>,
    // Legacy handle_message polls an ordinary Call before allocating a task
    // node. The owning request stays in its slot; only scheduling is deferred.
    starting: Option<usize>,
    live: usize,
    first_free: usize,
    maximum: usize,
    ready: ready::Queue,
    redirects: capntproto_rpc_engine::work_queue::Queue,
    preparing: usize,
    protocol_turn: bool,
    completed_plain: bool,
    completed_ordinary: bool,
    results_cache: results::Cache,
    executor: Option<Rc<dyn capability::CallExecutor>>,
}
impl<'p, 'id, I, const C: usize, const H: usize> Worker<'p, 'id, I, C, H> {
    pub fn new(maximum: usize, executor: Option<Rc<dyn capability::CallExecutor>>) -> Self {
        Self {
            entries: Vec::new(),
            starting: None,
            live: 0,
            first_free: 0,
            maximum,
            ready: ready::Queue::default(),
            redirects: capntproto_rpc_engine::work_queue::Queue::EMPTY,
            preparing: 0,
            protocol_turn: true,
            completed_plain: false,
            completed_ordinary: false,
            results_cache: results::Cache::new(maximum),
            executor,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    pub(in crate::scoped::wire) fn results(
        &self,
        charge: Option<coordinator::policy::Charge>,
        pool: Option<capnp::message::SegmentPool>,
    ) -> Producer {
        self.results_cache.acquire(charge, pool).into_producer()
    }
    pub(crate) fn completed_plain(&self) -> bool {
        self.completed_plain
    }
    pub(crate) fn completed_ordinary(&self) -> bool {
        self.completed_plain || self.completed_ordinary
    }
    pub(crate) fn has_undispatched(&self) -> bool {
        self.preparing != 0
    }
    pub(crate) fn pending_redirect(
        &self,
    ) -> Option<(
        answers::Key<'id>,
        Results,
        Rc<coordinator::multiparty::tail::Redirect>,
    )> {
        let index = self.redirects.head()?;
        let entry = self.entries[index].as_ref().expect("ready redirect owner");
        let redirect = entry.results.redirect().expect("ready redirect");
        debug_assert!(!redirect.sent.get());
        Some((entry.request().answer(), entry.results.clone(), redirect))
    }
    pub(crate) fn redirect_published(&mut self, key: answers::Key<'id>) {
        let index = self.redirects.head().expect("published redirect owner");
        assert_eq!(
            self.entries[index].as_ref().unwrap().request().answer(),
            key
        );
        self.redirects.remove(&mut self.entries, index, |slot| {
            &mut slot.as_mut().unwrap().redirect_work
        });
    }
    /// Accept only the oldest ready incoming ordinary Call. False leaves other
    /// tasks, including authenticated handoffs, untouched for their owner.
    /// Failure reserves neither a worker slot nor a dispatcher destination.
    #[inline(always)]
    pub fn accept<P: Copy + Eq>(
        &mut self,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
    ) -> capnp::Result<bool> {
        self.accept_using(driver, |driver| {
            driver.dispatch_immediate_call(true).map_err(wire_error)
        })
    }
    #[inline(always)]
    #[allow(clippy::type_complexity)]
    pub(crate) fn accept_using<P: Copy + Eq>(
        &mut self,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
        immediate: impl FnOnce(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
        ) -> capnp::Result<Option<(Running<'p, 'id, I, C, H>, bool)>>,
    ) -> capnp::Result<bool> {
        if let Some(kind) = driver.disconnected() {
            return Err(Error::from_kind(kind));
        }
        if !driver.has_pending_call() && driver.runtime().queued() == 0 {
            return Ok(false);
        }
        if self.live == self.maximum {
            return Ok(false);
        }
        // More than one admitted operation uses the ordinary FIFO. A parked
        // first operation must keep its place ahead of newly accepted work.
        if let Some(index) = self.starting.take() {
            self.schedule_entry(index);
        }
        let index = self.first_free;
        if index == self.entries.len() {
            if index == self.maximum {
                return Ok(false);
            }
            self.entries.try_reserve(1).map_err(wire_error)?;
        }
        if let Some((running, execute)) = immediate(driver)? {
            let key = running.owner().answer();
            let results = driver
                .ledgers()
                .answers
                .get(key)
                .expect("dispatched answer remains owned until Return")
                .owner
                .producer
                .results();
            driver.answers().watch_execution(key).map_err(wire_error)?;
            let ordinary = !running.owner().method().only_promise_pipeline
                && driver
                    .prepare_answer_return(key)
                    .is_ok_and(|pending| !pending.is_redirected());
            self.install(index, running, execute, results, ordinary);
            return Ok(true);
        }
        let (running, execute, results) = {
            // A pending route, remote target or older task keeps ordinary FIFO
            // dispatch. Queueing itself is progress: network controls must get
            // another turn to prepare a third-party destination before execution.
            let enqueued = matches!(driver.retry_call(), Ok(Some(_)));
            let (mut dispatch, mut answers) = driver.execution();
            let Some(ready) = dispatch.ready().map_err(wire_error)? else {
                return Ok(enqueued);
            };
            let Some(request) = ready.owner().as_call() else {
                return Ok(enqueued);
            };
            let key = request.answer();
            let results = &answers.get(key).map_err(wire_error)?.owner.producer;
            if answers.is_third_party(key).map_err(wire_error)? && !results.transfer_ready() {
                return Ok(enqueued);
            }
            let results = results.results();
            // Generated dispatch enforces cancellation via its supplied executor.
            let execute = answers.dispatch(key, true).map_err(wire_error)?;
            (
                ready.accept().map_owner(|task| match task {
                    connection::Task::Call(call) => call,
                    _ => unreachable!("preflighted incoming Call"),
                }),
                execute,
                results,
            )
        };
        driver
            .answers()
            .watch_execution(running.owner().answer())
            .map_err(wire_error)?;
        let ordinary = !running.owner().method().only_promise_pipeline
            && driver
                .prepare_answer_return(running.owner().answer())
                .is_ok_and(|pending| !pending.is_redirected());
        self.install(index, running, execute, results, ordinary);
        Ok(true)
    }

    #[inline(always)]
    fn install(
        &mut self,
        index: usize,
        running: Running<'p, 'id, I, C, H>,
        execute: bool,
        results: Results,
        ordinary: bool,
    ) {
        let entry = Entry {
            #[cfg(test)]
            polls: 0,
            result_revision: 0,
            redirect_work: capntproto_rpc_engine::work_queue::Link::EMPTY,
            waker: Waker::noop().clone(),
            running,
            results,
            stage: if execute {
                Stage::Prepare
            } else {
                Stage::Collect
            },
        };
        if !execute {
            entry.results.cancel();
        }
        if index == self.entries.len() {
            self.entries.push(Some(entry));
        } else {
            self.entries[index] = Some(entry);
        }
        self.live += 1;
        self.preparing += usize::from(execute);
        self.first_free = self.entries[index + 1..]
            .iter()
            .position(Option::is_none)
            .map_or(self.entries.len(), |offset| index + 1 + offset);
        // Like legacy, a fresh ordinary Call may finish immediately while
        // unrelated operations are asleep. Already-runnable work keeps FIFO
        // priority; continuous admission cannot starve its registered owners.
        if ordinary && (self.live == 1 || self.ready.is_empty()) {
            self.starting = Some(index);
        } else {
            self.schedule_entry(index);
        }
    }

    #[inline(always)]
    fn schedule_entry(&mut self, index: usize) {
        let waker = self.ready.insert(index, true);
        let entry = self.entries[index].as_mut().expect("accepted request");
        entry.results.bind_protocol_waker(waker.clone());
        entry.waker = waker;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn poll<P: Copy + Eq, R: Default>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
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
        self.poll_using(
            hooks,
            driver,
            messages,
            minimum,
            options,
            cx,
            &mut budget.get(),
            import,
            |hooks, driver| {
                hooks.collect_with_metadata(&mut driver.capabilities(), foreign::metadata)
            },
            |driver, messages, key, bytes| {
                driver
                    .send_answer(
                        key,
                        messages,
                        queue,
                        bytes,
                        options,
                        scratch,
                        Producer::write_exception,
                    )
                    .map_err(wire_error)
            },
            |_| {},
            |_, _, _, _| Ok(false),
        )
    }

    /// Same execution/cleanup path with resources attached to exact retained
    /// result owners. Publication retries cannot emit attachments twice.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_with_attachments<P: Copy + Eq, R, const N: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
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
        self.poll_using(
            hooks,
            driver,
            messages,
            minimum,
            options,
            cx,
            &mut budget.get(),
            import,
            |hooks, driver| {
                hooks.collect_with_metadata(&mut driver.capabilities(), foreign::metadata)
            },
            |driver, messages, key, bytes| {
                driver
                    .send_answer_with_attachments(
                        key,
                        messages,
                        queue,
                        bytes,
                        options,
                        scratch,
                        capacity,
                        &mut imported_resource,
                        &mut local_resource,
                        Producer::write_exception,
                    )
                    .map_err(wire_error)
            },
            |_| {},
            |_, _, _, _| Ok(false),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn poll_using<P: Copy + Eq>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        cx: &mut Context<'_>,
        remaining_budget: &mut usize,
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
        mut refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, P, C, H>),
        mut publish: impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            answers::Key<'id>,
            usize,
        ) -> capnp::Result<Option<Producer>>,
        mut prepare: impl FnMut(&Results),
        mut publish_plain: impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            &connection::Request<'p, 'id, I, C, H>,
            &PlainPayload<'_>,
        ) -> capnp::Result<bool>,
    ) -> Poll<capnp::Result<Progress>> {
        self.completed_plain = false;
        self.completed_ordinary = false;
        if let Some(kind) = driver.disconnected() {
            return Poll::Ready(Err(Error::from_kind(kind)));
        }
        if self.is_empty() {
            *remaining_budget = remaining_budget.saturating_sub(1);
            return Poll::Ready(Ok(Progress::Idle));
        }
        // Parked operations can wake while the first-poll handler runs. Keep
        // their queue attached to the current caller, including task migration.
        let mut registered = self.starting.is_none() || self.live > 1;
        if registered {
            self.ready.register(cx.waker());
        }
        let mut advanced = false;
        while *remaining_budget != 0 {
            let changed = if self.protocol_turn || self.ready.is_empty() {
                driver
                    .answers()
                    .take_changed_producer()
                    .map(|answer| answer.owner.producer.results())
            } else {
                None
            };
            if let Some(results) = changed {
                self.protocol_turn = false;
                results.protocol_changed();
                *remaining_budget -= 1;
                if *remaining_budget == 0 {
                    break;
                }
            }
            self.protocol_turn = true;
            let starting = self.starting.take();
            let ready = if starting.is_none() {
                if !registered {
                    self.ready.register(cx.waker());
                    registered = true;
                }
                self.ready.pop()
            } else {
                None
            };
            let index = match (&starting, &ready) {
                (Some(index), _) => *index,
                (_, Some(ready)) => ready.index(),
                _ => {
                    *remaining_budget -= 1;
                    return if advanced {
                        Poll::Ready(Ok(Progress::Advanced))
                    } else {
                        Poll::Pending
                    };
                }
            };
            let Some(entry) = self.entries[index].as_mut() else {
                *remaining_budget -= 1;
                continue;
            };
            if entry.result_revision != entry.results.revision() {
                entry.result_revision = entry.results.revision();
                let _ = driver.answers().notify_pipeline(entry.request().answer());
                // Early publication changes maintenance readiness even when
                // this method remains parked. Do not certify the lane idle.
                advanced = true;
            }
            // Legacy's first poll borrows the static noop waker. Only an
            // already scheduled operation needs an owned registration here.
            let waker = starting.is_none().then(|| entry.waker.clone());
            // A ready completion continues toward Return in this turn, as KJ
            // chains its answer continuation. Only suspension or the shared
            // budget yields the operation; intermediate stages need no enqueue.
            let outcome = loop {
                *remaining_budget -= 1;
                #[cfg(test)]
                {
                    entry.polls += 1;
                }
                let preparing = matches!(entry.stage, Stage::Prepare);
                let outcome = Self::step(
                    entry,
                    &self.executor,
                    hooks,
                    driver,
                    messages,
                    minimum,
                    options,
                    &mut Context::from_waker(waker.as_ref().unwrap_or(Waker::noop())),
                    &mut import,
                    &mut refresh,
                    &mut publish,
                    &mut prepare,
                    &mut publish_plain,
                    remaining_budget,
                    &mut self.completed_plain,
                    &mut self.completed_ordinary,
                );
                // Direct plain publication already retired this answer. There
                // is no producer or pipeline left to notify in its ledger.
                if !self.completed_plain && entry.result_revision != entry.results.revision() {
                    entry.result_revision = entry.results.revision();
                    let _ = driver.answers().notify_pipeline(entry.request().answer());
                    advanced = true;
                }
                if preparing && !matches!(entry.stage, Stage::Prepare) {
                    self.preparing -= 1;
                }
                // Dispatch can create routes or cleanup even when its first
                // execution poll suspends. Report that progress without
                // enqueueing an unwoken future for another Execute poll.
                advanced |= preparing && outcome.is_pending();
                if matches!(outcome, Poll::Ready(Ok(Progress::Advanced))) {
                    advanced = true;
                    if *remaining_budget != 0 {
                        continue;
                    }
                }
                break outcome;
            };
            let redirect_ready = !self.completed_plain
                && entry
                    .results
                    .redirect()
                    .is_some_and(|redirect| !redirect.sent.get());
            if starting.is_some() {
                if matches!(outcome, Poll::Ready(Ok(Progress::Replied))) {
                    let entry = self.entries[index].take().expect("completed request");
                    self.live -= 1;
                    self.first_free = self.first_free.min(index);
                    self.preparing -= usize::from(matches!(entry.stage, Stage::Prepare));
                    self.results_cache.recycle(release(
                        entry,
                        driver,
                        messages,
                        self.completed_plain,
                    ));
                    return outcome;
                }
                // Like legacy's now_or_never handoff, a suspended first poll
                // must run again with its operation's real waker. Discard only
                // the inert probe subscription; independent observers remain.
                entry.results.remove_noop_waiter();
                if !registered {
                    self.ready.register(cx.waker());
                    registered = true;
                }
                self.schedule_entry(index);
                if redirect_ready {
                    self.redirects.push(&mut self.entries, index, |slot| {
                        &mut slot.as_mut().unwrap().redirect_work
                    });
                }
                if matches!(outcome, Poll::Ready(Err(_))) {
                    return outcome;
                }
                advanced = true;
                continue;
            }
            if redirect_ready {
                self.redirects.push(&mut self.entries, index, |slot| {
                    &mut slot.as_mut().unwrap().redirect_work
                });
            }
            if let Poll::Ready(outcome) = outcome {
                if matches!(outcome, Ok(Progress::Replied)) {
                    // Retire before dropping application owners: their wakes
                    // cannot enqueue this slot or a later occupant of it.
                    self.ready.remove(index);
                    self.redirects.remove(&mut self.entries, index, |slot| {
                        &mut slot.as_mut().unwrap().redirect_work
                    });
                    let entry = self.entries[index].take().expect("replied operation");
                    self.live -= 1;
                    self.preparing -= usize::from(matches!(entry.stage, Stage::Prepare));
                    self.first_free = self.first_free.min(index);
                    self.results_cache.recycle(release(
                        entry,
                        driver,
                        messages,
                        self.completed_plain,
                    ));
                } else {
                    // Budget exhaustion or a retryable error retains work at
                    // the FIFO tail. Pending sleeps until its own notification
                    // or an explicit protocol change.
                    self.ready.push(index);
                }
                if matches!(outcome, Ok(Progress::Advanced)) {
                    continue;
                }
                return Poll::Ready(outcome);
            }
        }
        if advanced {
            Poll::Ready(Ok(Progress::Advanced))
        } else if self.starting.is_none()
            && self.ready.is_empty()
            && !driver.ledgers().answers.has_changed_producers()
        {
            Poll::Pending
        } else {
            Poll::Ready(Ok(Progress::Yield))
        }
    }

    #[allow(clippy::too_many_arguments)]
    // Keep the stage transition inside its bounded ready turn. Otherwise the many
    // adapter arguments are spilled and reloaded for every synchronous stage.
    #[inline(always)]
    fn step<P: Copy + Eq>(
        entry: &mut Entry<'p, 'id, I, C, H>,
        executor: &Option<Rc<dyn capability::CallExecutor>>,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        minimum: usize,
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
        refresh: &mut impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, P, C, H>),
        publish: &mut impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            answers::Key<'id>,
            usize,
        ) -> capnp::Result<Option<Producer>>,
        prepare: &mut impl FnMut(&Results),
        publish_plain: &mut impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            &connection::Request<'p, 'id, I, C, H>,
            &PlainPayload<'_>,
        ) -> capnp::Result<bool>,
        remaining_budget: &mut usize,
        completed_plain: &mut bool,
        completed_ordinary: &mut bool,
    ) -> Poll<capnp::Result<Progress>> {
        let key = entry.request().answer();
        // AwaitFromThirdParty may retire on Finish before the child completes.
        // The generation's absence means the relay no longer owns interest;
        // dropping our execution lets the destination retain its own interest.
        if entry
            .results
            .redirect()
            .is_some_and(|redirect| redirect.sent.get())
            && driver.ledgers().answers.get(key).is_err()
        {
            return Poll::Ready(Ok(Progress::Replied));
        }
        if matches!(
            entry.stage,
            Stage::Prepare | Stage::Execute(_) | Stage::Transfer(_) | Stage::Collect
        ) && driver
            .ledgers()
            .answers
            .cancellation_requested(key)
            .map_err(wire_error)?
        {
            entry.stage = Stage::Collect; // Drops interest, retaining protected execution.
            if !entry.results.transfer_ready() {
                entry.results.cancel();
            }
        }
        match &mut entry.stage {
            Stage::Prepare => {
                let handoff = entry.request().allows_third_party_tail_call();
                entry.results.tail_origin(if handoff {
                    hooks.inbox.joins.clone()
                } else {
                    None
                });
                let local = match driver
                    .runtime()
                    .inspect(entry.running.target())
                    .map_err(wire_error)?
                {
                    clients::Destination::Endpoint {
                        owner:
                            Route::Capability(
                                Endpoint::Local(hook) | Endpoint::Failed { owner: hook, .. },
                            )
                            | Route::Vine(hook),
                        ..
                    } => Some(Ok(hook.clone())),
                    clients::Destination::Endpoint {
                        owner: Route::Capability(Endpoint::Broken(kind)),
                        ..
                    } => Some(Err(*kind)),
                    _ => None,
                };
                // Clone the Rc before invoking any original hook code.
                let target: Rc<dyn ClientHook> = match local {
                    Some(Ok(hook)) => hook,
                    Some(Err(kind)) => Rc::from(crate::broken::new_cap(Error::from_kind(kind))),
                    None => hooks
                        .export_client(&mut driver.capabilities(), entry.running.target())?
                        .hook
                        .into(),
                };
                let prepared = hooks.prepare_received_server(entry.request(), driver, messages)?;
                refresh(hooks, driver);
                let mut execution = if *remaining_budget != 0 {
                    prepared.execute_during_dispatch(&*target, &entry.results, executor.clone())?
                } else {
                    prepared.execute(&*target, &entry.results, executor.clone())?
                };
                entry.stage = if entry.results.transfer_ready() {
                    Stage::Transfer(entry.results.start_transfer(execution)?)
                } else if *remaining_budget != 0 {
                    // A selected ready turn has no intervening dispatch or IO,
                    // regardless of other live operations. Synchronous services
                    // need no stored Execute state. Charge
                    // the same quantum as the separate Execute step.
                    *remaining_budget -= 1;
                    if Pin::new(&mut execution).poll(cx).is_ready() {
                        Stage::Collect
                    } else {
                        entry.stage = Stage::Execute(execution);
                        return Poll::Pending;
                    }
                } else {
                    Stage::Execute(execution)
                };
                if *remaining_budget >= 2 && matches!(entry.stage, Stage::Collect) {
                    if let Some(payload) = entry.results.plain_payload() {
                        if publish_plain(driver, messages, entry.request(), &payload)? {
                            // A synchronous completion needs no suspended
                            // Collect state. Account for both Collect and
                            // Publish before retiring the ordinary owner.
                            *remaining_budget -= 2;
                            entry.results.returned();
                            *completed_plain = true;
                            return Poll::Ready(Ok(Progress::Replied));
                        }
                    }
                }
            }
            Stage::Execute(execution) => {
                // An independently executed protected method may publish an
                // early pipeline before its completion future wakes this task.
                if Pin::new(execution).poll(cx).is_pending() {
                    return Poll::Pending;
                }
                entry.stage = Stage::Collect;
            }
            Stage::Transfer(work) => {
                if Pin::new(work).poll(cx).is_pending() {
                    return Poll::Pending;
                }
                entry.stage = Stage::Collect;
            }
            Stage::Collect => {
                let canceled_transfer = entry.results.transfer_ready()
                    && driver
                        .ledgers()
                        .answers
                        .cancellation_requested(key)
                        .map_err(wire_error)?;
                if !canceled_transfer
                    && (entry.results.poll_owned_completed().is_pending()
                        || entry.results.projections_pending(hooks, cx))
                {
                    return Poll::Pending;
                }
                if *remaining_budget != 0 && !canceled_transfer {
                    if let Some(payload) = entry.results.plain_payload() {
                        if publish_plain(driver, messages, entry.request(), &payload)? {
                            // Collection and publication remain separately
                            // charged, although no retained intermediate result
                            // is needed when the answer can retire immediately.
                            *remaining_budget -= 1;
                            entry.results.returned();
                            *completed_plain = true;
                            return Poll::Ready(Ok(Progress::Replied));
                        }
                    }
                }
                let redirect = entry.results.redirect();
                if redirect
                    .as_ref()
                    .is_some_and(|redirect| !redirect.sent.get())
                {
                    return Poll::Pending;
                }
                let output = if canceled_transfer {
                    Err(capnp::ErrorKind::Disconnected)
                } else {
                    hooks.collect_server_output(
                        &entry.results,
                        driver,
                        messages,
                        options,
                        // A transferred answer may already have published
                        // AwaitFromThirdParty/ResultsSentElsewhere. Its final
                        // local payload feeds handoff observers, not another
                        // Return reservation on the introducer connection.
                        (redirect.is_none() && !entry.results.transfer_ready()).then_some(key),
                        import,
                    )?
                };
                let completed = if redirect.is_some() {
                    driver.complete_network_answer(entry.request(), output, messages)
                } else if entry.results.transfer_ready() {
                    driver.complete_transferred_answer(entry.request(), output, messages)
                } else {
                    driver.complete_answer(entry.request(), output, messages)
                };
                if let Err(rejected) = completed {
                    if let Ok(result) = rejected.result {
                        result.release(messages, &mut driver.capabilities());
                    }
                    return Poll::Ready(Err(wire_error(rejected.error)));
                }
                entry.stage = Stage::Publish;
            }
            Stage::Publish => {
                if entry
                    .results
                    .redirect()
                    .is_some_and(|redirect| redirect.sent.get())
                {
                    return Poll::Ready(Ok(Progress::Replied));
                }
                prepare(&entry.results);
                // Once Finish arrived, Canceled needs only its fixed envelope.
                let canceled = driver
                    .prepare_answer_return(key)
                    .map_err(wire_error)?
                    .is_cancelled();
                let bytes = if canceled || entry.results.transfer_ready() {
                    minimum.max(128)
                } else {
                    minimum.max(entry.results.publication_bytes()?)
                };
                drop(publish(driver, messages, key, bytes)?);
                *completed_ordinary =
                    entry.results.redirect().is_none() && !entry.results.transfer_ready();
                entry.results.returned();
                return Poll::Ready(Ok(Progress::Replied));
            }
        }
        Poll::Ready(Ok(Progress::Advanced))
    }

    /// Drop application interest and release every accepted request before
    /// terminating its driver. The caller still owns the cancellation executor;
    /// protected methods keep their result context until that executor completes.
    pub fn terminate<P: Copy + Eq>(
        self,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) {
        self.ready.close();
        for entry in self.entries.into_iter().flatten() {
            drop(release(entry, driver, messages, false));
        }
    }
}
#[inline(always)]
fn release<'p, 'id, I, P: Copy + Eq, const C: usize, const H: usize>(
    entry: Entry<'p, 'id, I, C, H>,
    driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
    messages: &mut messages::Messages<'p, '_, '_>,
    retired: bool,
) -> Results {
    entry.results.unbind_protocol_waker();
    if !retired {
        let _ = driver.answers().unwatch_execution(entry.request().answer());
    }
    drop(entry.stage);
    let call = driver.capabilities().complete(entry.running);
    driver.release_call(call, messages);
    entry.results
}

impl<'id> ClientHooks<'id> {
    /// Retain an explicit early/final hosted pipeline through its original hook,
    /// preserving policy membranes and full hosted failure details. An outer
    /// error is retryable; None means the application has not supplied a route.
    pub fn server_pipeline<'p, I, P: Copy + Eq, S, const C: usize, const H: usize>(
        &mut self,
        results: &Results,
        path: pipeline::Path<'_>,
        runtime: &mut capntproto_rpc_wire::owned::connection::Scope<'_, 'p, 'id, '_, I, P, S, C, H>,
        mut import: impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<'_, 'p, 'id, '_, I, P, S, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> capnp::Result<Option<Client<'id>>> {
        let Some(hook) = results.pipeline(path) else {
            return Ok(None);
        };
        match self.try_retain(runtime, &*hook)? {
            Some(client) => Ok(Some(client)),
            None => import(runtime, &*hook).map(Some),
        }
    }
}

#[cfg(test)]
mod tests;

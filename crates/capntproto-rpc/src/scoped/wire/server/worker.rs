//! Caller-polled execution of incoming Calls. The portable answer ledger is the
//! only protocol authority; these slots own accepted application work, not IDs.
use super::*;
use capntproto_rpc_engine::{answers, clients};
use capntproto_rpc_wire::clients::{Endpoint, Route};
use std::num::NonZeroUsize;

type Running<'p, 'id, I, const C: usize, const P: usize> =
    dispatch::Running<'id, connection::Request<'p, 'id, I, C, P>>;
type Peer<'p, 'id, 's, I, P, const C: usize, const H: usize> =
    Driver<'p, 'id, 's, I, Results, P, Rc<dyn ClientHook>, C, H>;

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
    /// The bounded scan has more slots to visit; poll again.
    Yield,
    Idle,
    Advanced,
    Replied,
}

/// Incoming service execution for a hosted connection using the portable driver.
///
/// Call `accept` for ready incoming Calls and `poll` alongside IO, invocation
/// delivery and `Driver::poll_maintenance_with`. The maintenance resolver must
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
    maximum: usize,
    cursor: usize,
    remaining: usize,
    executor: Option<Rc<dyn capability::CallExecutor>>,
}
impl<'p, 'id, I, const C: usize, const H: usize> Worker<'p, 'id, I, C, H> {
    pub fn new(maximum: usize, executor: Option<Rc<dyn capability::CallExecutor>>) -> Self {
        Self {
            entries: Vec::new(),
            maximum,
            cursor: 0,
            remaining: 0,
            executor,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.entries.iter().all(Option::is_none)
    }
    pub(crate) fn invalidate_scan(&mut self) {
        self.remaining = 0;
    }
    pub(crate) fn has_undispatched(&self) -> bool {
        self.entries
            .iter()
            .flatten()
            .any(|entry| matches!(entry.stage, Stage::Prepare))
    }
    pub(crate) fn pending_redirect(
        &self,
    ) -> Option<(
        answers::Key<'id>,
        Results,
        Rc<coordinator::multiparty::tail::Redirect>,
    )> {
        self.entries.iter().flatten().find_map(|entry| {
            let redirect = entry.results.redirect()?;
            (!redirect.sent.get())
                .then(|| (entry.request().answer(), entry.results.clone(), redirect))
        })
    }
    /// Accept only the oldest ready incoming ordinary Call. False leaves other
    /// tasks, including authenticated handoffs, untouched for their owner.
    /// Failure reserves neither a worker slot nor a dispatcher destination.
    pub fn accept<P: Copy + Eq>(
        &mut self,
        driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
    ) -> capnp::Result<bool> {
        if let Some(kind) = driver.disconnected() {
            return Err(Error::from_kind(kind));
        }
        if !driver.has_pending_call() && driver.runtime.queued() == 0 {
            return Ok(false);
        }
        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.entries.len());
        if index == self.entries.len() {
            if index == self.maximum {
                return Ok(false);
            }
            self.entries.try_reserve(1).map_err(wire_error)?;
        }
        let (running, execute, results) = if let Some((running, execute)) =
            driver.dispatch_immediate_call(true).map_err(wire_error)?
        {
            let key = running.owner().answer();
            let results = driver
                .ledgers
                .answers
                .get(key)
                .expect("dispatched answer remains owned until Return")
                .owner
                .producer
                .clone();
            (running, execute, results)
        } else {
            // A pending route, remote target or older task keeps ordinary FIFO
            // dispatch. Queueing itself is progress: network controls must get
            // another turn to prepare a third-party destination before execution.
            let enqueued = matches!(driver.retry_call(), Ok(Some(_)));
            let Some(ready) = driver.runtime.next_ready().map_err(wire_error)? else {
                return Ok(enqueued);
            };
            let Some(request) = ready.owner().as_call() else {
                return Ok(enqueued);
            };
            let key = request.answer();
            let results = &driver
                .ledgers
                .answers
                .get(key)
                .map_err(wire_error)?
                .owner
                .producer;
            if driver
                .ledgers
                .answers
                .is_third_party(key)
                .map_err(wire_error)?
                && !results.transfer_ready()
            {
                return Ok(enqueued);
            }
            let results = results.clone();
            // Generated dispatch enforces cancellation via its supplied executor.
            let execute = driver
                .ledgers
                .answers
                .dispatch(key, true)
                .map_err(wire_error)?;
            (
                ready.accept().map_owner(|task| match task {
                    connection::Task::Call(call) => call,
                    _ => unreachable!("preflighted incoming Call"),
                }),
                execute,
                results,
            )
        };
        let entry = Entry {
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
        self.remaining = self.entries.len();
        Ok(true)
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
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
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
            budget,
            import,
            |hooks, driver| hooks.collect_with_metadata(&mut driver.runtime, foreign::metadata),
            |driver, messages, key, bytes| {
                driver
                    .send_answer(
                        key,
                        messages,
                        queue,
                        bytes,
                        options,
                        scratch,
                        Results::write_exception,
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
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
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
            budget,
            import,
            |hooks, driver| hooks.collect_with_metadata(&mut driver.runtime, foreign::metadata),
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
                        Results::write_exception,
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
        budget: NonZeroUsize,
        mut import: impl FnMut(
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        mut refresh: impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, P, C, H>),
        mut publish: impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            answers::Key<'id>,
            usize,
        ) -> capnp::Result<Option<Results>>,
        mut prepare: impl FnMut(&Results),
        mut publish_plain: impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            &connection::Request<'p, 'id, I, C, H>,
            &connection::PreparedReturn<capnp::message::HeapAllocator>,
        ) -> capnp::Result<bool>,
    ) -> Poll<capnp::Result<Progress>> {
        if let Some(kind) = driver.disconnected() {
            return Poll::Ready(Err(Error::from_kind(kind)));
        }
        if self.entries.is_empty() {
            return Poll::Ready(Ok(Progress::Idle));
        }
        if self.remaining == 0 {
            self.remaining = self.entries.len();
        }
        let mut advanced = false;
        let mut remaining_budget = budget.get();
        let exclusive = self.entries.len() == 1;
        while remaining_budget != 0 {
            remaining_budget -= 1;
            let index = self.cursor;
            self.cursor = if index + 1 == self.entries.len() {
                0
            } else {
                index + 1
            };
            let outcome = if let Some(entry) = &mut self.entries[index] {
                Self::step(
                    entry,
                    &self.executor,
                    hooks,
                    driver,
                    messages,
                    minimum,
                    options,
                    cx,
                    &mut import,
                    &mut refresh,
                    &mut publish,
                    &mut prepare,
                    &mut publish_plain,
                    &mut remaining_budget,
                    exclusive,
                )
            } else {
                Poll::Pending
            };
            if let Poll::Ready(outcome) = outcome {
                self.remaining = self.entries.len();
                if matches!(outcome, Ok(Progress::Replied)) {
                    let entry = self.entries[index].take().expect("replied operation");
                    release(entry, driver, messages);
                }
                if matches!(outcome, Ok(Progress::Advanced)) {
                    advanced = true;
                    continue;
                }
                return Poll::Ready(outcome);
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
        // More slots remain unpolled; do not lose their application wakeups.
        Poll::Ready(Ok(if advanced {
            Progress::Advanced
        } else {
            Progress::Yield
        }))
    }

    #[allow(clippy::too_many_arguments)]
    // Keep the stage transition inside its bounded scan. Otherwise the many
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
            &mut Runtime<'p, 'id, '_, I, P, Rc<dyn ClientHook>, C, H>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
        refresh: &mut impl FnMut(&mut ClientHooks<'id>, &mut Peer<'p, 'id, '_, I, P, C, H>),
        publish: &mut impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            answers::Key<'id>,
            usize,
        ) -> capnp::Result<Option<Results>>,
        prepare: &mut impl FnMut(&Results),
        publish_plain: &mut impl FnMut(
            &mut Peer<'p, 'id, '_, I, P, C, H>,
            &mut messages::Messages<'p, '_, '_>,
            &connection::Request<'p, 'id, I, C, H>,
            &connection::PreparedReturn<capnp::message::HeapAllocator>,
        ) -> capnp::Result<bool>,
        remaining_budget: &mut usize,
        exclusive: bool,
    ) -> Poll<capnp::Result<Progress>> {
        let key = entry.request().answer();
        // AwaitFromThirdParty may retire on Finish before the child completes.
        // The generation's absence means the relay no longer owns interest;
        // dropping our execution lets the destination retain its own interest.
        if entry
            .results
            .redirect()
            .is_some_and(|redirect| redirect.sent.get())
            && driver.ledgers.answers.get(key).is_err()
        {
            return Poll::Ready(Ok(Progress::Replied));
        }
        if matches!(
            entry.stage,
            Stage::Prepare | Stage::Execute(_) | Stage::Transfer(_) | Stage::Collect
        ) && driver
            .ledgers
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
                    .runtime
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
                        .export_client(&mut driver.runtime, entry.running.target())?
                        .hook
                        .into(),
                };
                let prepared = hooks.prepare_received_server(entry.request(), driver, messages)?;
                refresh(hooks, driver);
                let mut execution = prepared.execute(&*target, &entry.results, executor.clone())?;
                entry.stage = if entry.results.transfer_ready() {
                    Stage::Transfer(entry.results.start_transfer(execution)?)
                } else if exclusive && *remaining_budget != 0 {
                    // With one worker there is no intervening dispatch or IO.
                    // Poll before moving the execution into its suspended slot;
                    // synchronous services need no stored Execute state. Charge
                    // the same quantum as the separate Execute step.
                    *remaining_budget -= 1;
                    if entry.results.poll_work(&mut execution, cx).is_ready() {
                        Stage::Collect
                    } else {
                        Stage::Execute(execution)
                    }
                } else {
                    Stage::Execute(execution)
                };
                if exclusive && *remaining_budget >= 2 && matches!(entry.stage, Stage::Collect) {
                    if let Some(payload) = entry.results.plain_payload() {
                        let PayloadBody::Return(source) = &payload.body else {
                            unreachable!()
                        };
                        if publish_plain(driver, messages, entry.request(), source)? {
                            // A synchronous completion needs no suspended
                            // Collect state. Account for both Collect and
                            // Publish before retiring the ordinary owner.
                            *remaining_budget -= 2;
                            entry.results.returned();
                            return Poll::Ready(Ok(Progress::Replied));
                        }
                    }
                }
            }
            Stage::Execute(execution) => {
                // An independently executed protected method may publish an
                // early pipeline before its completion future wakes this task.
                if entry.results.poll_work(execution, cx).is_pending() {
                    return Poll::Pending;
                }
                entry.stage = Stage::Collect;
            }
            Stage::Transfer(work) => {
                if entry.results.poll_work(work, cx).is_pending() {
                    return Poll::Pending;
                }
                entry.stage = Stage::Collect;
            }
            Stage::Collect => {
                let canceled_transfer = entry.results.transfer_ready()
                    && driver
                        .ledgers
                        .answers
                        .cancellation_requested(key)
                        .map_err(wire_error)?;
                if !canceled_transfer && entry.results.poll_completed(cx).is_pending() {
                    return Poll::Pending;
                }
                if *remaining_budget != 0 && !canceled_transfer {
                    if let Some(payload) = entry.results.plain_payload() {
                        let PayloadBody::Return(source) = &payload.body else {
                            unreachable!()
                        };
                        if publish_plain(driver, messages, entry.request(), source)? {
                            // Collection and publication remain separately
                            // charged, although no retained intermediate result
                            // is needed when the answer can retire immediately.
                            *remaining_budget -= 1;
                            entry.results.returned();
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
                        result.release(messages, &mut driver.runtime);
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
                    .ledgers
                    .answers
                    .prepare_return(key)
                    .map_err(wire_error)?
                    .is_cancelled();
                let bytes = if canceled || entry.results.transfer_ready() {
                    minimum.max(128)
                } else {
                    minimum.max(entry.results.publication_bytes()?)
                };
                drop(publish(driver, messages, key, bytes)?);
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
        for entry in self.entries.into_iter().flatten() {
            release(entry, driver, messages);
        }
    }
}
#[inline(always)]
fn release<'p, 'id, I, P: Copy + Eq, const C: usize, const H: usize>(
    entry: Entry<'p, 'id, I, C, H>,
    driver: &mut Peer<'p, 'id, '_, I, P, C, H>,
    messages: &mut messages::Messages<'p, '_, '_>,
) {
    drop(entry.stage);
    let call = driver.runtime.complete(entry.running);
    driver.release_call(call, messages);
}

impl<'id> ClientHooks<'id> {
    /// Retain an explicit early/final hosted pipeline through its original hook,
    /// preserving policy membranes and full hosted failure details. An outer
    /// error is retryable; None means the application has not supplied a route.
    pub fn server_pipeline<'p, I, P: Copy + Eq, S, const C: usize, const H: usize>(
        &mut self,
        results: &Results,
        path: pipeline::Path<'_>,
        runtime: &mut Runtime<'p, 'id, '_, I, P, S, C, H>,
        mut import: impl FnMut(
            &mut Runtime<'p, 'id, '_, I, P, S, C, H>,
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

//! Result capabilities created before an invocation receives a wire question.
//! Weak graph observers retain neither abandoned calls nor their capabilities.
use super::*;
use capntproto_rpc_engine::clients::Capabilities;
use capntproto_rpc_engine::waiting::{Slot as WaitSlot, Waiting};
#[derive(Clone, Copy)]
enum Stage<'id> {
    Queued,
    Sent(questions::Key<'id>),
    Canceled,
}
struct Entry<'id, const PATH: usize> {
    task: InvocationKey<'id>,
    client: clients::Key<'id>,
    subscription: clients::Subscription<'id>,
    previous: Option<core::num::NonZeroUsize>,
    next: Option<core::num::NonZeroUsize>,
    fields: Buffer<u16, PATH>,
    length: usize,
    stage: Stage<'id>,
}
pub struct InvocationPipelineSlot<'id, const PATH: usize> {
    retry: WaitSlot,
    entry: Option<Entry<'id, PATH>>,
}
impl<'id, const PATH: usize> InvocationPipelineSlot<'id, PATH> {
    pub const EMPTY: Self = Self {
        entry: None,
        retry: WaitSlot::EMPTY,
    };
    pub(super) fn is_occupied(&self) -> bool {
        self.entry.is_some()
    }
    pub(super) fn pending_local(&self, task: InvocationKey<'id>) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|entry| entry.task == task && matches!(entry.stage, Stage::Queued))
    }
    pub(super) fn clear(&mut self) {
        self.entry = None;
        self.retry = WaitSlot::EMPTY;
    }
}
// Keep occupancy with its storage: admission, removal and terminal clearing are
// the only writers. Callers can inspect slots but cannot bypass this bookkeeping.
pub(super) struct Bindings<'s, 'id, const PATH: usize> {
    slots: SlotStorage<'s, InvocationPipelineSlot<'id, PATH>>,
    live: usize,
    listener: Option<clients::Listener<'id>>,
    retry: Waiting<{ crate::resources::COUNT }>,
    prefer_event: bool,
}
impl<'s, 'id, const PATH: usize> Bindings<'s, 'id, PATH> {
    pub(super) fn new(slots: SlotStorage<'s, InvocationPipelineSlot<'id, PATH>>) -> Self {
        debug_assert!(slots.iter().all(|slot| !slot.is_occupied()));
        Self {
            slots,
            live: 0,
            listener: None,
            retry: Waiting::EMPTY,
            prefer_event: false,
        }
    }
    pub(super) fn pending_for(
        &self,
        question: questions::Key<'id>,
        questions: &questions::Questions<'id, '_>,
    ) -> bool {
        self.live != 0 && questions.has_observers(question, true)
    }
    pub(super) fn sent(
        &mut self,
        task: InvocationKey<'id>,
        question: questions::Key<'id>,
        questions: &mut questions::Questions<'id, '_>,
        operations: &mut operations::Operations<'_, 'id>,
    ) {
        let operation = operations.get_mut(task).expect("published operation");
        let mut cursor = operation.observers;
        debug_assert!(questions.observer_head(question, true).is_none());
        while let Some(index) = cursor {
            let entry = self.slots[index]
                .entry
                .as_mut()
                .expect("operation observer");
            debug_assert_eq!(entry.task, task);
            cursor = entry.next.map(|i| i.get() - 1);
            entry.stage = Stage::Sent(question);
            entry.previous = None;
            let next = questions.link_observer(question, true, index);
            entry.next = next.and_then(|i| core::num::NonZeroUsize::new(i + 1));
            if let Some(next) = next {
                self.slots[next].entry.as_mut().unwrap().previous =
                    core::num::NonZeroUsize::new(index + 1);
            }
            operation.observers = Some(index);
            self.retry.wake(&mut self.slots, index, |s| &mut s.retry);
        }
    }
    pub(super) fn canceled(
        &mut self,
        task: InvocationKey<'id>,
        operations: &operations::Operations<'_, 'id>,
    ) {
        let mut cursor = operations.get(task).expect("canceled operation").observers;
        while let Some(index) = cursor {
            let entry = self.slots[index]
                .entry
                .as_mut()
                .expect("operation observer");
            debug_assert_eq!(entry.task, task);
            entry.stage = Stage::Canceled;
            cursor = entry.next.map(|i| i.get() - 1);
            self.retry.wake(&mut self.slots, index, |s| &mut s.retry);
        }
    }
    pub(super) fn next(&self, index: usize) -> Option<usize> {
        self.slots
            .get(index)?
            .entry
            .as_ref()?
            .next
            .map(|i| i.get() - 1)
    }
    pub(super) fn local_next(
        &self,
        task: InvocationKey<'id>,
        cursor: usize,
        head: Option<usize>,
    ) -> Option<usize> {
        if self
            .slots
            .get(cursor)
            .is_some_and(|slot| slot.pending_local(task))
        {
            Some(cursor)
        } else {
            head
        }
    }
    pub(super) fn clear_terminal<G: clients::Capabilities<'id>>(&mut self, graph: &mut G) {
        if let Some(listener) = self.listener.take() {
            graph.close_listener(listener);
        }
        for slot in &mut *self.slots {
            slot.clear();
        }
        self.live = 0;
        self.retry = Waiting::EMPTY;
    }
}
impl<'id, const PATH: usize> core::ops::Deref for Bindings<'_, 'id, PATH> {
    type Target = [InvocationPipelineSlot<'id, PATH>];
    fn deref(&self) -> &Self::Target {
        &self.slots
    }
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Obtain a result capability immediately after enqueue_call, before wire
    /// output or local execution. Dependent invocations enter the same dispatcher
    /// and wait until their exact result path becomes callable. Generated method
    /// hints which forbid pipelining are checked before allocating an observer.
    pub fn invocation_pipeline(
        &mut self,
        task: InvocationKey<'id>,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<clients::Client<'id>, Error> {
        self.check_live()?;
        if path.fields().len() > self.path_capacity() {
            return Err(Error::Input(super::super::Error::Capacity));
        }
        let invocation = self.queued_invocation(task)?;
        let allowed = invocation.allows_pipeline(messages).map_err(Error::Input)?;
        if !allowed {
            return Err(Error::Input(super::super::Error::Wire(
                capnp::ErrorKind::Unimplemented,
            )));
        }
        self.observe_invocation(task, path)
    }

    pub(super) fn observe_invocation(
        &mut self,
        task: InvocationKey<'id>,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, Error> {
        if path.fields().len() > self.path_capacity() {
            return Err(Error::Input(super::super::Error::Capacity));
        }
        let previous_head = self.operations.get(task)?.observers;
        let index = self
            .invocation_pipelines
            .slots
            .vacant(
                |_, slot| slot.entry.is_none(),
                || InvocationPipelineSlot::EMPTY,
            )
            .ok_or(Error::Busy)?;
        let fields = self
            .message_path
            .copy(path.fields())
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        let client = self.runtime.promise(&mut None).map_err(Error::Graph)?;
        let subscribed = (|| {
            if self.invocation_pipelines.listener.is_none() {
                self.invocation_pipelines.listener = Some(self.runtime.listen()?);
            }
            self.runtime.subscribe(
                self.invocation_pipelines.listener.as_ref().unwrap(),
                client.key(),
                index,
                clients::Watch::Abandonment,
            )
        })();
        let subscription = match subscribed {
            Ok(subscription) => subscription,
            Err(error) => {
                self.runtime.release(client);
                return Err(Error::Graph(error));
            }
        };
        self.invocation_pipelines.live += 1;
        if let Some(head) = previous_head {
            self.invocation_pipelines.slots[head]
                .entry
                .as_mut()
                .unwrap()
                .previous = core::num::NonZeroUsize::new(index + 1);
        }
        let operation = self.operations.get_mut(task)?;
        operation.observers = Some(index);
        operation.observer_count += 1;
        self.invocation_pipelines.slots[index].entry = Some(Entry {
            previous: None,
            next: previous_head.and_then(|i| core::num::NonZeroUsize::new(i + 1)),
            task,
            client: client.key(),
            subscription,
            fields,
            length: path.fields().len(),
            stage: Stage::Queued,
        });
        Ok(client)
    }
    pub(super) fn invocation_work_is_idle(&self) -> bool {
        !self.invocation_pipelines.retry.has_ready(
            &self.invocation_pipelines.slots,
            |s| &s.retry,
            |r| self.available_resources().ready(r),
        ) && !self
            .invocation_pipelines
            .listener
            .as_ref()
            .is_some_and(|listener| self.runtime.has_events(listener))
    }
    pub(super) fn poll_next_invocation_pipeline(&mut self) -> Result<Propagation, Error> {
        for _ in 0..2 {
            self.invocation_pipelines.prefer_event = !self.invocation_pipelines.prefer_event;
            if self.invocation_pipelines.prefer_event {
                let Some(event) = self
                    .invocation_pipelines
                    .listener
                    .as_ref()
                    .and_then(|l| self.runtime.next_event(l))
                else {
                    continue;
                };
                let index = event.owner;
                if self
                    .invocation_pipelines
                    .get(index)
                    .and_then(|s| s.entry.as_ref())
                    .is_some_and(|e| {
                        e.client == event.client && e.subscription.key() == event.subscription
                    })
                {
                    self.finish_invocation_observer(index);
                    return Ok(Propagation::Abandoned);
                }
                return Ok(Propagation::Unrelated);
            }
            let available = if self.invocation_pipelines.retry.has_waiters() {
                self.available_resources()
            } else {
                Default::default()
            };
            let bindings = &mut self.invocation_pipelines;
            let Some(index) = bindings.retry.pop(
                &mut bindings.slots,
                |s| &mut s.retry,
                |r| available.ready(r),
            ) else {
                continue;
            };
            return match self.poll_invocation_pipeline(index) {
                Err(_) if self.invocation_pipelines.slots[index].retry.is_parked() => {
                    Ok(Propagation::Pending)
                }
                other => other,
            };
        }
        Ok(Propagation::Unrelated)
    }
    /// Attach one observer to its accepted outgoing question, or break it after
    /// pre-publication cancellation. Return may arrive first; its owning response
    /// remains retained and ordered before subsequent Resolve propagation.
    pub fn poll_invocation_pipeline(&mut self, index: usize) -> Result<Propagation, Error> {
        let question = self
            .invocation_pipelines
            .get(index)
            .and_then(|slot| slot.entry.as_ref())
            .and_then(|entry| {
                if let Stage::Sent(key) = entry.stage {
                    Some(key)
                } else {
                    None
                }
            });
        if index < self.invocation_pipelines.slots.len() {
            let bindings = &mut self.invocation_pipelines;
            bindings
                .retry
                .remove(&mut bindings.slots, index, |s| &mut s.retry);
        }
        let result = self.poll_invocation_pipeline_inner(index);
        if let Err(error) = result {
            if let Some(requirement) = self.resource_wait(error) {
                let bindings = &mut self.invocation_pipelines;
                bindings
                    .retry
                    .park(&mut bindings.slots, index, requirement, |s| &mut s.retry);
            }
        }
        if let Some(key) = question {
            self.refresh_exchange(key);
        }
        result
    }
    fn poll_invocation_pipeline_inner(&mut self, index: usize) -> Result<Propagation, Error> {
        self.check_live()?;
        let Some(entry) = self
            .invocation_pipelines
            .get(index)
            .ok_or(Error::Busy)?
            .entry
            .as_ref()
        else {
            return Ok(Propagation::Unrelated);
        };
        let observed = match self.runtime.retain_key(entry.client) {
            Ok(client) => client,
            Err(clients::Error::Stale) => {
                self.finish_invocation_observer(index);
                return Ok(Propagation::Abandoned);
            }
            Err(error) => return Err(Error::Graph(error)),
        };
        let candidate = (|| match entry.stage {
            Stage::Queued => Ok(None),
            Stage::Canceled => self
                .runtime
                .endpoint_with(clients::Location::Broken, || {
                    binding::Route::broken(capnp::ErrorKind::Failed)
                })
                .map(Some)
                .map_err(Error::Graph),
            Stage::Sent(question) => {
                let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, question)
                    .ok_or(Error::UnknownQuestion)?;
                let exchange = self.exchanges[index]
                    .exchange
                    .as_ref()
                    .ok_or(Error::UnknownQuestion)?;
                let path = pipeline::Path::new(&entry.fields[..entry.length]);
                match exchange {
                    Exchange::Pending(question) => self.outgoing.bind(
                        question,
                        &mut self.ledgers.questions,
                        &mut self.runtime,
                        path,
                    ),
                    Exchange::Returned(response) => self.outgoing.observe_returned(
                        response,
                        &mut self.ledgers.questions,
                        &mut self.runtime,
                        path,
                    ),
                }
                .map(Some)
                .map_err(Error::Outgoing)
            }
        })();
        self.resolve_invocation_observer(index, observed, candidate)
    }
    /// Settle one local execution's result path. The caller retains its result
    /// storage and polls every observer until settled. The resolver borrows the
    /// graph to return an independently owned client (or application exception).
    /// It may be retried after capacity failure; no callback runs for stale or
    /// unrelated observers. Complete/release the running Invocation separately.
    pub fn poll_local_invocation_pipeline(
        &mut self,
        index: usize,
        task: InvocationKey<'id>,
        resolve: impl FnOnce(
            pipeline::Path<'_>,
            &mut Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<clients::Client<'id>, capnp::ErrorKind>,
    ) -> Result<Propagation, Error> {
        self.poll_local_invocation_pipeline_with(index, task, |path, runtime| {
            Ok(resolve(path, runtime))
        })
    }
    /// A fallible resolver distinguishes resource pressure from a failed
    /// application result. An outer error preserves the observer for retry;
    /// an inner error resolves it to a broken capability.
    #[allow(clippy::type_complexity)]
    pub fn poll_local_invocation_pipeline_with(
        &mut self,
        index: usize,
        task: InvocationKey<'id>,
        resolve: impl FnOnce(
            pipeline::Path<'_>,
            &mut Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<Result<clients::Client<'id>, capnp::ErrorKind>, Error>,
    ) -> Result<Propagation, Error> {
        self.poll_local_invocation_pipeline_when_ready(index, task, |path, runtime| {
            resolve(path, runtime).map(Some)
        })
    }
    /// Poll an application pipeline before its result is complete. None leaves
    /// the observer pending; resource errors preserve it for a later retry.
    #[allow(clippy::type_complexity)]
    pub fn poll_local_invocation_pipeline_when_ready(
        &mut self,
        index: usize,
        task: InvocationKey<'id>,
        resolve: impl FnOnce(
            pipeline::Path<'_>,
            &mut Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        )
            -> Result<Option<Result<clients::Client<'id>, capnp::ErrorKind>>, Error>,
    ) -> Result<Propagation, Error> {
        self.check_live()?;
        let Some(entry) = self
            .invocation_pipelines
            .get(index)
            .ok_or(Error::Busy)?
            .entry
            .as_ref()
        else {
            return Ok(Propagation::Unrelated);
        };
        if entry.task != task || !matches!(entry.stage, Stage::Queued) {
            return Ok(Propagation::Unrelated);
        }
        if self.queued_invocation(task).is_ok() {
            return Err(Error::Busy);
        }
        let observed = match self.runtime.retain_key(entry.client) {
            Ok(client) => client,
            Err(clients::Error::Stale) => {
                self.finish_invocation_observer(index);
                return Ok(Propagation::Abandoned);
            }
            Err(error) => return Err(Error::Graph(error)),
        };
        let candidate = match resolve(
            pipeline::Path::new(&entry.fields[..entry.length]),
            &mut clients::Access::new(&mut self.runtime),
        ) {
            Ok(Some(Ok(client))) => Ok(Some(client)),
            Ok(Some(Err(kind))) => self
                .runtime
                .endpoint_with(clients::Location::Broken, || binding::Route::broken(kind))
                .map(Some)
                .map_err(Error::Graph),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        self.resolve_invocation_observer(index, observed, candidate)
    }
    fn resolve_invocation_observer(
        &mut self,
        index: usize,
        observed: clients::Client<'id>,
        candidate: Result<Option<clients::Client<'id>>, Error>,
    ) -> Result<Propagation, Error> {
        let candidate = match candidate {
            Ok(Some(client)) => client,
            Ok(None) => {
                self.runtime.release(observed);
                return Ok(Propagation::Pending);
            }
            Err(error) => {
                self.runtime.release(observed);
                return Err(error);
            }
        };
        let mut replacement = Some(candidate);
        let result =
            self.runtime
                .resolve(observed.key(), &mut replacement, clients::Source::Direct);
        self.runtime.release(observed);
        match result {
            Ok(barrier) => {
                debug_assert!(
                    barrier.is_none(),
                    "a local observer has no original remote route"
                );
                self.finish_invocation_observer(index);
                Ok(Propagation::Resolved)
            }
            Err(error) => {
                self.runtime.release(replacement.unwrap());
                Err(Error::Graph(error))
            }
        }
    }
    fn finish_invocation_observer(&mut self, index: usize) {
        let bindings = &mut self.invocation_pipelines;
        bindings
            .retry
            .remove(&mut bindings.slots, index, |s| &mut s.retry);
        let entry = self.invocation_pipelines.slots[index].entry.take().unwrap();
        self.invocation_pipelines.live -= 1;
        self.runtime.unsubscribe(entry.subscription);
        let next = entry.next.map(|i| i.get() - 1);
        if let Some(previous) = entry.previous {
            self.invocation_pipelines.slots[previous.get() - 1]
                .entry
                .as_mut()
                .unwrap()
                .next = entry.next;
        } else {
            self.operations
                .get_mut(entry.task)
                .expect("observer operation")
                .observers = next;
            if let Stage::Sent(question) = entry.stage {
                self.ledgers
                    .questions
                    .unlink_observer_head(question, true, index, next);
            }
        }
        if let Some(next) = next {
            self.invocation_pipelines.slots[next]
                .entry
                .as_mut()
                .unwrap()
                .previous = entry.previous;
        }
        self.operations
            .get_mut(entry.task)
            .expect("observer operation")
            .observer_count -= 1;
        self.operations.retire(entry.task);
        if let Stage::Sent(question) = entry.stage {
            self.ledgers.questions.remove_observer(question, true);
            if !self
                .invocation_pipelines
                .pending_for(question, &self.ledgers.questions)
            {
                if let Some(Exchange::Pending(pending)) =
                    ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, question)
                        .and_then(|index| self.exchanges.get_mut(index))
                        .filter(|slot| slot.canceled)
                        .and_then(|slot| slot.exchange.as_mut())
                {
                    pending.cancel(&mut self.ledgers.questions);
                }
            }
            self.refresh_exchange(question);
        }
    }
}

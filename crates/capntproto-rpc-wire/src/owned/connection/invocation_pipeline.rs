//! Result capabilities created before an invocation receives a wire question.
//! Weak graph observers retain neither abandoned calls nor their capabilities.
use super::*;
use capntproto_rpc_engine::clients::Capabilities;
#[derive(Clone, Copy)]
enum Stage<'id> {
    Queued,
    Sent(questions::Key<'id>),
    Canceled,
}
struct Entry<'id, const PATH: usize> {
    task: dispatch::TaskKey<'id>,
    client: clients::Key<'id>,
    fields: Buffer<u16, PATH>,
    length: usize,
    stage: Stage<'id>,
}
pub struct InvocationPipelineSlot<'id, const PATH: usize> {
    entry: Option<Entry<'id, PATH>>,
}
impl<'id, const PATH: usize> InvocationPipelineSlot<'id, PATH> {
    pub const EMPTY: Self = Self { entry: None };
    pub(super) fn is_occupied(&self) -> bool {
        self.entry.is_some()
    }
    pub(super) fn pending_for(&self, question: questions::Key<'id>) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|entry| matches!(entry.stage, Stage::Sent(key) if key == question))
    }
    pub(super) fn pending_local(&self, task: dispatch::TaskKey<'id>) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|entry| entry.task == task && matches!(entry.stage, Stage::Queued))
    }
    pub(super) fn sent(&mut self, task: dispatch::TaskKey<'id>, question: questions::Key<'id>) {
        if let Some(entry) = self.entry.as_mut().filter(|entry| entry.task == task) {
            entry.stage = Stage::Sent(question);
        }
    }
    pub(super) fn canceled(&mut self, task: dispatch::TaskKey<'id>) {
        if let Some(entry) = self.entry.as_mut().filter(|entry| entry.task == task) {
            entry.stage = Stage::Canceled;
        }
    }
    pub(super) fn clear(&mut self) {
        self.entry = None;
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
        task: dispatch::TaskKey<'id>,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<clients::Client<'id>, Error> {
        self.check_live()?;
        if path.fields().len() > self.path_capacity() {
            return Err(Error::Input(super::super::Error::Capacity));
        }
        let (_, owner) = (0..self.runtime.task_capacity())
            .find_map(|index| {
                self.runtime
                    .queued_at(index)
                    .filter(|(key, _)| *key == task)
            })
            .ok_or(Error::UnknownInvocation)?;
        let Task::Invoke(invocation) = owner else {
            return Err(Error::UnknownInvocation);
        };
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
        task: dispatch::TaskKey<'id>,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, Error> {
        if path.fields().len() > self.path_capacity() {
            return Err(Error::Input(super::super::Error::Capacity));
        }
        let slot = self
            .invocation_pipelines
            .vacant_mut(
                |_, slot| slot.entry.is_none(),
                || InvocationPipelineSlot::EMPTY,
            )
            .ok_or(Error::Busy)?;
        let fields = self
            .message_path
            .copy(path.fields())
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        let client = self.runtime.promise(&mut None).map_err(Error::Graph)?;
        slot.entry = Some(Entry {
            task,
            client: client.key(),
            fields,
            length: path.fields().len(),
            stage: Stage::Queued,
        });
        Ok(client)
    }
    /// Attach one observer to its accepted outgoing question, or break it after
    /// pre-publication cancellation. Return may arrive first; its owning response
    /// remains retained and ordered before subsequent Resolve propagation.
    pub fn poll_invocation_pipeline(&mut self, index: usize) -> Result<Propagation, Error> {
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
                let exchange = self
                    .exchanges
                    .iter()
                    .find_map(|slot| {
                        slot.exchange
                            .as_ref()
                            .filter(|exchange| exchange.key() == question)
                    })
                    .ok_or(Error::UnknownQuestion)?;
                let path = pipeline::Path::new(&entry.fields[..entry.length]);
                match exchange {
                    Exchange::Pending(question) => self.outgoing.bind(
                        question,
                        &mut self.ledgers.questions,
                        &mut self.runtime,
                        path,
                    ),
                    Exchange::Returned(response) => {
                        self.outgoing
                            .observe_returned(response, &mut self.runtime, path)
                    }
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
        task: dispatch::TaskKey<'id>,
        resolve: impl FnOnce(
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
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
        task: dispatch::TaskKey<'id>,
        resolve: impl FnOnce(
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
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
        task: dispatch::TaskKey<'id>,
        resolve: impl FnOnce(
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
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
        if (0..self.runtime.task_capacity()).any(|index| {
            self.runtime
                .queued_at(index)
                .is_some_and(|(key, _)| key == task)
        }) {
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
            &mut self.runtime,
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
        let entry = self.invocation_pipelines[index].entry.take().unwrap();
        if let Stage::Sent(question) = entry.stage {
            if !self
                .invocation_pipelines
                .iter()
                .any(|slot| slot.pending_for(question))
            {
                if let Some(Exchange::Pending(pending)) = self
                    .exchanges
                    .iter_mut()
                    .find(|slot| {
                        slot.canceled
                            && slot
                                .exchange
                                .as_ref()
                                .is_some_and(|exchange| exchange.key() == question)
                    })
                    .and_then(|slot| slot.exchange.as_mut())
                {
                    pending.cancel(&mut self.ledgers.questions);
                }
            }
        }
    }
}

//! Owning routes for calls pipelined onto an outgoing question. Their retained
//! question references prevent Finish until the last original route retires.
use super::*;
use crate::buffer::{Buffer, Capacity};
use capntproto_rpc_engine::storage::Storage as SlotStorage;
use capntproto_rpc_engine::waiting::{Requirement, Slot as WaitSlot, Waiting};
mod handoff;
use crate::{
    calls,
    owned::{self, answer, pipelines, Reply, Response},
    pipeline,
};
use capntproto_rpc_engine::{answers, exports, ids::QuestionId, messages, questions};

#[derive(Debug)]
pub struct QuestionRoute<'id, P, const PATH: usize> {
    local: questions::Local<'id>,
    peer: P,
    fields: Buffer<u16, PATH>,
    length: usize,
}
impl<'id, P: Copy, const PATH: usize> QuestionRoute<'id, P, PATH> {
    pub fn peer(&self) -> P {
        self.peer
    }
    pub fn target(
        &self,
        questions: &questions::Questions<'id, '_>,
    ) -> Result<(QuestionId, pipeline::Path<'_>), questions::Error> {
        Ok((
            questions.pipeline_id(&self.local)?,
            pipeline::Path::new(&self.fields[..self.length]),
        ))
    }
}

#[derive(Debug)]
pub enum Route<'id, P, S, const PATH: usize> {
    Capability(Endpoint<'id, P, S>),
    Question(QuestionRoute<'id, P, PATH>),
    /// A caller-owned network proxy retaining the provision or upstream vine.
    /// Its owner must forward calls and acceptance markers along that same path.
    /// Ordinary local services do not authorize acceptance-marker forwarding.
    Vine(S),
}
impl<P, S, const PATH: usize> sealed::Sealed for Route<'_, P, S, PATH> {}
impl<'id, P, S, const PATH: usize> EndpointOwner<'id, P> for Route<'id, P, S, PATH> {
    fn exception(&self) -> Option<capnp::ErrorKind> {
        match self {
            Self::Capability(endpoint) => endpoint.exception(),
            Self::Question(_) | Self::Vine(_) => None,
        }
    }
    fn imported(remote: Remote<'id, P>) -> Self {
        Self::Capability(Endpoint::Imported(remote))
    }
    fn broken(error: capnp::ErrorKind) -> Self {
        Self::Capability(Endpoint::Broken(error))
    }
    fn into_import(self) -> Result<Remote<'id, P>, Self> {
        match self {
            Self::Capability(Endpoint::Imported(remote)) => Ok(remote),
            other => Err(other),
        }
    }
}
impl<'id, P: Copy, S, const PATH: usize> Route<'id, P, S, PATH> {
    pub fn local(server: S) -> Self {
        Self::Capability(Endpoint::Local(server))
    }
    pub fn target(
        &self,
        questions: &questions::Questions<'id, '_>,
    ) -> Result<Option<calls::Target<'_>>, questions::Error> {
        match self {
            Self::Question(route) => {
                let (question, path) = route.target(questions)?;
                Ok(Some(calls::Target::Promised { question, path }))
            }
            Self::Capability(Endpoint::Imported(remote)) => {
                Ok(Some(calls::Target::Imported(remote.id())))
            }
            _ => Ok(None),
        }
    }
    pub fn release<I>(
        self,
        imports: &mut imports::Imports<'id, '_, I>,
        questions: &mut questions::Questions<'id, '_>,
    ) -> Option<S> {
        match self {
            Self::Capability(endpoint) => endpoint.release(imports),
            Self::Question(route) => {
                questions.release(route.local);
                None
            }
            Self::Vine(owner) => Some(owner),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Capacity,
    ObserversFull,
    Question(questions::Error),
    Canceled,
    Graph(clients::Error),
    Input(owned::Error),
    Binding(BindError),
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "outgoing pipeline binding: {self:?}")
    }
}
impl core::error::Error for Error {}

struct Entry<'id, const PATH: usize> {
    question: questions::Key<'id>,
    client: clients::Key<'id>,
    subscription: clients::Subscription<'id>,
    original_route: bool,
    previous: Option<core::num::NonZeroUsize>,
    next: Option<core::num::NonZeroUsize>,
    fields: Buffer<u16, PATH>,
    length: usize,
}
pub struct Slot<'id, const PATH: usize> {
    retry: WaitSlot,
    entry: Option<Entry<'id, PATH>>,
}
impl<const PATH: usize> Slot<'_, PATH> {
    pub const EMPTY: Self = Self {
        entry: None,
        retry: WaitSlot::EMPTY,
    };
}
pub struct Bindings<'s, 'id, P, const PATH: usize> {
    registry: Option<questions::OutgoingRegistry<'id>>,
    path_capacity: Capacity,
    slots: SlotStorage<'s, Slot<'id, PATH>>,
    peer: P,
    live: usize,
    listener: Option<clients::Listener<'id>>,
    retry: Waiting<{ crate::resources::COUNT }>,
}
pub enum Progress<'id> {
    Unrelated,
    Pending,
    Abandoned,
    Resolved(Option<clients::Barrier<'id>>),
}
impl<'s, 'id, P: Copy + Eq, const PATH: usize> Bindings<'s, 'id, P, PATH> {
    fn claim_registry(
        &mut self,
        questions: &mut questions::Questions<'id, '_>,
    ) -> Result<(), Error> {
        if self.registry.is_none() {
            self.registry = Some(
                questions
                    .claim_outgoing_registry()
                    .map_err(Error::Question)?,
            );
        }
        Ok(())
    }
    pub(crate) fn has_waiters(&self) -> bool {
        self.retry.has_waiters()
    }
    pub(crate) fn next_observer(&self, index: usize) -> Option<usize> {
        self.slots
            .get(index)?
            .entry
            .as_ref()?
            .next
            .map(|i| i.get() - 1)
    }
    pub(crate) fn retry_ready(&self, available: impl FnMut(Requirement) -> bool) -> bool {
        self.retry.has_ready(&self.slots, |s| &s.retry, available)
    }
    pub(crate) fn next_retry(
        &mut self,
        available: impl FnMut(Requirement) -> bool,
    ) -> Option<usize> {
        self.retry.pop(&mut self.slots, |s| &mut s.retry, available)
    }
    pub(crate) fn remove_wait(&mut self, index: usize) {
        if index < self.slots.len() {
            self.retry.remove(&mut self.slots, index, |s| &mut s.retry);
        }
    }
    pub(crate) fn park(&mut self, index: usize, requirement: Requirement) {
        self.retry
            .park(&mut self.slots, index, requirement, |s| &mut s.retry);
    }
    pub(crate) fn resource_waiting(&self, index: usize) -> bool {
        self.slots.get(index).is_some_and(|s| s.retry.is_parked())
    }
    pub(crate) fn clear_terminal<G: clients::Capabilities<'id>>(&mut self, graph: &mut G) {
        if let Some(listener) = self.listener.take() {
            graph.close_listener(listener);
        }
        for slot in self.slots.iter_mut() {
            slot.entry = None;
            slot.retry = WaitSlot::EMPTY;
        }
        self.retry = Waiting::EMPTY;
        self.live = 0;
        self.registry = None;
    }
    fn link(
        &mut self,
        index: usize,
        question: questions::Key<'id>,
        questions: &mut questions::Questions<'id, '_>,
    ) -> Option<core::num::NonZeroUsize> {
        let next = questions.link_observer(question, false, index);
        if let Some(next) = next {
            self.slots[next]
                .entry
                .as_mut()
                .expect("linked observer")
                .previous = core::num::NonZeroUsize::new(index + 1);
        }
        next.and_then(|i| core::num::NonZeroUsize::new(i + 1))
    }
    fn finish<G: clients::Capabilities<'id>>(
        &mut self,
        index: usize,
        questions: &mut questions::Questions<'id, '_>,
        graph: &mut G,
    ) {
        self.retry.remove(&mut self.slots, index, |s| &mut s.retry);
        let entry = self.slots[index].entry.take().expect("live observer");
        questions.advance_observer_work(
            entry.question,
            false,
            index,
            entry.next.map(|i| i.get() - 1),
        );
        graph.unsubscribe(entry.subscription);
        if let Some(previous) = entry.previous {
            self.slots[previous.get() - 1]
                .entry
                .as_mut()
                .expect("previous observer")
                .next = entry.next;
        } else {
            questions.unlink_observer_head(
                entry.question,
                false,
                index,
                entry.next.map(|i| i.get() - 1),
            );
        }
        if let Some(next) = entry.next {
            self.slots[next.get() - 1]
                .entry
                .as_mut()
                .expect("next observer")
                .previous = entry.previous;
        }
        questions.remove_observer(entry.question, false);
        self.live -= 1;
        if self.live == 0 {
            if let Some(registry) = self.registry.take() {
                questions.release_outgoing_registry(registry);
            }
        }
    }

    pub(crate) fn abandon<G: clients::Capabilities<'id>>(
        &mut self,
        event: clients::Event<'id>,
        questions: &mut questions::Questions<'id, '_>,
        graph: &mut G,
    ) -> Option<questions::Key<'id>> {
        let index = event.owner;
        let entry = self.slots.get(index)?.entry.as_ref()?;
        if entry.client != event.client || entry.subscription.key() != event.subscription {
            return None;
        }
        let question = entry.question;
        self.finish(index, questions, graph);
        Some(question)
    }
    pub(crate) fn has_events<G: clients::Capabilities<'id>>(&self, graph: &G) -> bool {
        self.listener
            .as_ref()
            .is_some_and(|listener| graph.has_events(listener))
    }
    pub(crate) fn next_event<G: clients::Capabilities<'id>>(
        &self,
        graph: &mut G,
    ) -> Option<clients::Event<'id>> {
        self.listener
            .as_ref()
            .and_then(|listener| graph.next_event(listener))
    }
    fn subscribe<G: clients::Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        key: clients::Key<'id>,
        index: usize,
    ) -> Result<clients::Subscription<'id>, clients::Error> {
        if self.listener.is_none() {
            self.listener = Some(graph.listen()?);
        }
        graph.subscribe(
            self.listener.as_ref().unwrap(),
            key,
            index,
            clients::Watch::Abandonment,
        )
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    pub(crate) fn available(&self) -> usize {
        self.slots.maximum() - self.live
    }
    pub(crate) fn maximum(&self) -> usize {
        self.slots.maximum()
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub(crate) fn question(&self, index: usize) -> Option<questions::Key<'id>> {
        self.slots
            .get(index)?
            .entry
            .as_ref()
            .map(|entry| entry.question)
    }
    pub fn new(slots: &'s mut [Slot<'id, PATH>], peer: P) -> Result<Self, Error> {
        Self::from_storage(SlotStorage::borrowed(slots), peer)
    }
    pub(crate) fn from_storage(
        slots: SlotStorage<'s, Slot<'id, PATH>>,
        peer: P,
    ) -> Result<Self, Error> {
        Self::from_storage_with_capacity(slots, peer, Capacity::Inline)
    }
    pub(crate) fn from_storage_with_capacity(
        slots: SlotStorage<'s, Slot<'id, PATH>>,
        peer: P,
        path_capacity: Capacity,
    ) -> Result<Self, Error> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            registry: None,
            live: 0,
            listener: None,
            retry: Waiting::EMPTY,
            slots,
            peer,
            path_capacity,
        })
    }
    pub fn pending_for(
        &self,
        question: questions::Key<'id>,
        questions: &questions::Questions<'id, '_>,
    ) -> bool {
        self.live != 0 && questions.has_observers(question, false)
    }

    /// Register an observer whose invocation was still queued when Return
    /// arrived. It never used a remote route, so no original endpoint or loopback
    /// obligation is needed. Keep Response until poll_one consumes the payload.
    pub(crate) fn observe_returned<I, const N: usize, G: clients::Capabilities<'id>>(
        &mut self,
        response: &Response<'_, 'id, 'id, 'id, 'id, I, N>,
        questions: &mut questions::Questions<'id, '_>,
        graph: &mut G,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, Error> {
        if path.fields().len() > self.path_capacity.maximum::<PATH>() {
            return Err(Error::Capacity);
        }
        self.claim_registry(questions)?;
        let index = self
            .slots
            .vacant(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
            .ok_or(Error::ObserversFull)?;
        let fields = self
            .path_capacity
            .copy(path.fields())
            .ok_or(Error::Capacity)?;
        let client = graph.promise(&mut None).map_err(Error::Graph)?;
        let subscription = match self.subscribe(graph, client.key(), index) {
            Ok(subscription) => subscription,
            Err(error) => {
                graph.release(client);
                return Err(Error::Graph(error));
            }
        };
        self.live += 1;
        let next = self.link(index, response.key(), questions);
        self.slots[index].entry = Some(Entry {
            previous: None,
            next,
            question: response.key(),
            client: client.key(),
            subscription,
            original_route: false,
            fields,
            length: path.fields().len(),
        });
        self.retry.wake(&mut self.slots, index, |s| &mut s.retry);
        Ok(client)
    }

    /// Settle one matching observer from the exact admitted Return. The caller
    /// retains the Response until `pending_for` is false. A local adopted answer
    /// may still be pending; unrelated responses cannot settle or clear a slot.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_one<
        'pool,
        I,
        A,
        S,
        const N: usize,
        const CAPS: usize,
        const ANSWER_PATH: usize,
        G,
    >(
        &mut self,
        index: usize,
        response: &Response<'pool, 'id, 'id, 'id, 'id, I, N>,
        questions: &mut questions::Questions<'id, '_>,
        messages: &messages::Messages<'pool, '_, '_>,
        graph: &mut G,
        bindings: &mut super::Imports<'_, 'id, 'id, P>,
        imports: &mut imports::Imports<'id, '_, I>,
        exports: &exports::Exports<'id, '_, clients::Client<'id>>,
        answers: &mut answers::Answers<
            'id,
            '_,
            answer::State<'pool, pipelines::Results<'pool, 'id, A, CAPS>>,
        >,
        pipelines: &mut pipelines::Bindings<'_, 'id, 'id, ANSWER_PATH>,
    ) -> Result<Progress<'id>, Error>
    where
        G: clients::Capabilities<'id, Owner = Route<'id, P, S, PATH>, Peer = P>,
    {
        let slot = self.slots.get_mut(index).ok_or(Error::Capacity)?;
        let Some(entry) = slot
            .entry
            .as_ref()
            .filter(|entry| entry.question == response.key())
        else {
            return Ok(Progress::Unrelated);
        };
        let observed = match graph.retain_key(entry.client) {
            Ok(client) => client,
            Err(clients::Error::Stale) => {
                self.finish(index, questions, graph);
                return Ok(Progress::Abandoned);
            }
            Err(error) => return Err(Error::Graph(error)),
        };
        let path = pipeline::Path::new(&entry.fields[..entry.length]);
        let candidate = response
            .with(messages, |reply| -> Result<_, Error> {
                let failure = match reply {
                    Reply::Results(payload) => match payload.pipeline(path) {
                        Ok(capability) => {
                            return bindings
                                .bind_capability(
                                    capability, graph, imports, exports, answers, pipelines,
                                )
                                .map(|bound| Some(bound.parts()))
                                .map_err(Error::Binding)
                        }
                        Err(_) => capnp::ErrorKind::Failed,
                    },
                    Reply::Exception(exception) => match exception.get_type() {
                        Ok(crate::rpc_capnp::exception::Type::Overloaded) => {
                            capnp::ErrorKind::Overloaded
                        }
                        Ok(crate::rpc_capnp::exception::Type::Disconnected) => {
                            capnp::ErrorKind::Disconnected
                        }
                        Ok(crate::rpc_capnp::exception::Type::Unimplemented) => {
                            capnp::ErrorKind::Unimplemented
                        }
                        _ => capnp::ErrorKind::Failed,
                    },
                    Reply::Adopted(adopted) => {
                        match answers.adopted(adopted).owner.target(path, messages, graph) {
                            Ok(client) => {
                                return Ok(client
                                    .map(|client| (client, clients::Source::Reflected, false)))
                            }
                            Err(pipelines::Error::Graph(error)) => return Err(Error::Graph(error)),
                            Err(pipelines::Error::Input(owned::Error::Wire(kind))) => kind,
                            Err(_) => capnp::ErrorKind::Failed,
                        }
                    }
                    // Yourself results stay on the peer's original answer route.
                    Reply::ResultsSentElsewhere => {
                        return graph
                            .snapshot(&observed)
                            .map(|client| Some((client, clients::Source::Direct, false)))
                            .map_err(Error::Graph)
                    }
                    Reply::AwaitFromThirdParty(_) => return Ok(None),
                    Reply::Discarded => capnp::ErrorKind::Disconnected,
                };
                graph
                    .endpoint_with(clients::Location::Broken, || Route::broken(failure))
                    .map(|client| Some((client, clients::Source::Direct, false)))
                    .map_err(Error::Graph)
            })
            .map_err(Error::Input)
            .and_then(core::convert::identity);
        let candidate = match candidate {
            Ok(Some(candidate)) => candidate,
            Ok(None) => {
                graph.release(observed);
                return Ok(Progress::Pending);
            }
            Err(error) => {
                graph.release(observed);
                return Err(error);
            }
        };
        let (client, source, observer) = candidate;
        let key = client.key();
        let mut replacement = Some(client);
        let resolved = graph.resolve(observed.key(), &mut replacement, source);
        graph.release(observed);
        match resolved {
            Ok(barrier) => {
                self.finish(index, questions, graph);
                Ok(Progress::Resolved(barrier))
            }
            Err(error) => {
                if observer {
                    pipelines.discard_unexposed(key, answers, graph);
                }
                graph.release(replacement.expect("unaccepted replacement"));
                Err(Error::Graph(error))
            }
        }
    }
    /// Create a callable remote pipeline only from a still-owned outgoing
    /// question. Aliases of the same path share one resolution observer.
    pub fn bind<S, G>(
        &mut self,
        question: &owned::Question<'_, 'id>,
        questions: &mut questions::Questions<'id, '_>,
        graph: &mut G,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, Error>
    where
        G: clients::Capabilities<'id, Owner = Route<'id, P, S, PATH>, Peer = P>,
    {
        let local = question.local().ok_or(Error::Canceled)?;
        if question.key().id().is_pipeline_only() {
            // No Return will resolve this route. Like C++/legacy, expose a
            // terminal remote capability and retain the question until its
            // last pipeline reference is released.
            return freeze(graph, questions, local, path, self.peer, self.path_capacity);
        }
        questions.pipeline_id(local).map_err(Error::Question)?;
        if path.fields().len() > self.path_capacity.maximum::<PATH>() {
            return Err(Error::Capacity);
        }
        self.claim_registry(questions)?;
        let mut matching = None;
        let mut cursor = questions.observer_head(question.key(), false);
        while let Some(index) = cursor {
            let entry = self.slots[index].entry.as_ref().expect("question observer");
            debug_assert_eq!(entry.question, question.key());
            if &entry.fields[..entry.length] == path.fields() {
                matching = Some(index);
                break;
            }
            cursor = entry.next.map(|i| i.get() - 1);
        }
        if let Some(index) = matching {
            match graph.retain_key(self.slots[index].entry.as_ref().unwrap().client) {
                Ok(client) => return Ok(client),
                Err(clients::Error::Stale) => self.finish(index, questions, graph),
                Err(error) => return Err(Error::Graph(error)),
            }
        }
        self.claim_registry(questions)?;
        let index = matching
            .or_else(|| self.slots.iter().position(|slot| slot.entry.is_none()))
            .or_else(|| {
                self.slots
                    .vacant(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
            })
            .ok_or(Error::ObserversFull)?;
        let fields = self
            .path_capacity
            .copy(path.fields())
            .ok_or(Error::Capacity)?;
        let observer_fields = self
            .path_capacity
            .copy(path.fields())
            .ok_or(Error::Capacity)?;
        let local = questions.retain(local).map_err(Error::Question)?;
        let owner = Route::Question(QuestionRoute {
            local,
            peer: self.peer,
            fields,
            length: path.fields().len(),
        });
        let client = graph
            .imported_promise(owner, self.peer)
            .map_err(|rejected| {
                let Route::Question(route) = rejected.value else {
                    unreachable!("new question route")
                };
                questions.release(route.local);
                Error::Graph(rejected.error)
            })?;
        let subscription = match self.subscribe(graph, client.key(), index) {
            Ok(subscription) => subscription,
            Err(error) => {
                graph.release(client);
                return Err(Error::Graph(error));
            }
        };
        let (previous, next) = if let Some(old) = self.slots[index].entry.as_ref() {
            (old.previous, old.next)
        } else {
            self.live += 1;
            (None, self.link(index, question.key(), questions))
        };
        self.slots[index].entry = Some(Entry {
            previous,
            next,
            question: question.key(),
            client: client.key(),
            subscription,
            original_route: true,
            fields: observer_fields,
            length: path.fields().len(),
        });
        Ok(client)
    }
}

/// Freeze the precise question and transform advertised by receiverAnswer.
/// There is no result observer: later Return must not change this route.
pub(crate) fn freeze<'id, P: Copy + Eq, S, G, const PATH: usize>(
    graph: &mut G,
    questions: &mut questions::Questions<'id, '_>,
    question: &questions::Local<'id>,
    path: pipeline::Path<'_>,
    peer: P,
    capacity: Capacity,
) -> Result<clients::Client<'id>, Error>
where
    G: clients::Capabilities<'id, Owner = Route<'id, P, S, PATH>, Peer = P>,
{
    if path.fields().len() > capacity.maximum::<PATH>() {
        return Err(Error::Capacity);
    }
    questions.pipeline_id(question).map_err(Error::Question)?;
    let fields = capacity.copy(path.fields()).ok_or(Error::Capacity)?;
    let local = questions.retain(question).map_err(Error::Question)?;
    let owner = Route::Question(QuestionRoute {
        local,
        peer,
        fields,
        length: path.fields().len(),
    });
    graph
        .endpoint(owner, clients::Location::Remote(peer))
        .map_err(|rejected| {
            let Route::Question(route) = rejected.value else {
                unreachable!()
            };
            questions.release(route.local);
            Error::Graph(rejected.error)
        })
}

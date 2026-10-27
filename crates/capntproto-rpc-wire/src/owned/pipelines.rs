//! Frozen local answer results and bounded promised-answer observers. Result
//! frames retain the exact capability routes which their descriptors advertised.
use super::*;
use capntproto_rpc_engine::clients::{self, Capabilities, Client, Location, Source};
use capntproto_rpc_engine::storage::Storage as SlotStorage;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Input(super::Error),
    Graph(clients::Error),
    Answer(answers::Error),
    Question(capntproto_rpc_engine::questions::Error),
    Descriptors(exports::DescriptorError),
    Capacity,
    ObserversFull,
    NonemptyStorage,
    UnsupportedDescriptor,
    Missing,
    Mismatch,
    AlreadyCompleted,
    UnfrozenExport,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "owning answer pipeline: {self:?}")
    }
}
impl core::error::Error for Error {}

/// Constructed from an immutable result and this connection's exact staged
/// export owners. A later promise resolution cannot rewrite these snapshots.
#[must_use = "release the frozen result clients and frame explicitly"]
pub struct Published<'pool, 'graph, const N: usize> {
    answer: AnswerId,
    pub(super) frame: Frame<'pool>,
    pub(super) clients: Buffer<Option<Client<'graph>>, N>,
    pub(super) count: usize,
    pub(super) prepared_return: Option<(AnswerId, bool)>,
}
impl<'pool, 'graph, const N: usize> Published<'pool, 'graph, N> {
    /// An adopted Return exposes the completed local answer as an ordinary
    /// result. Retain its exact frozen clients and frame before releasing the
    /// adoption lease; partial reference/capacity failure changes no ownership.
    pub(crate) fn retain_local<G: Capabilities<'graph>>(
        &self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
        capacity: Capacity,
        method: (u64, u16),
    ) -> Result<super::connection::LocalResult<'pool, 'graph, N>, Error> {
        let mut clients = capacity
            .buffer::<_, N>(self.count, || None)
            .ok_or(Error::Capacity)?;
        let prepared = (|| {
            for (slot, client) in clients.iter_mut().zip(self.clients.iter()).take(self.count) {
                if let Some(client) = client {
                    *slot = Some(graph.retain_key(client.key()).map_err(Error::Graph)?);
                }
            }
            messages
                .retain(&self.frame.lease)
                .map_err(|e| Error::Input(super::Error::Storage(e)))
        })();
        match prepared {
            Ok(lease) => Ok(super::connection::LocalResult {
                prepared_return: self.prepared_return,
                capacity,
                frame: Frame::new(lease, self.frame.options),
                capabilities: clients,
                count: self.count,
                interface: method.0,
                method: method.1,
            }),
            Err(error) => {
                for client in clients.into_iter().flatten() {
                    graph.release(client);
                }
                Err(error)
            }
        }
    }
    pub(crate) fn from_local<G: Capabilities<'graph>>(
        result: &super::connection::LocalResult<'pool, 'graph, N>,
        answer: AnswerId,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
    ) -> Result<Self, Error> {
        let mut clients = result
            .capacity
            .buffer::<_, N>(result.count, || None)
            .ok_or(Error::Capacity)?;
        let prepared = (|| {
            for (slot, client) in clients.iter_mut().zip(result.capabilities.iter()) {
                if let Some(client) = client {
                    *slot = Some(graph.snapshot(client).map_err(Error::Graph)?);
                }
            }
            messages
                .retain(&result.frame.lease)
                .map_err(|e| Error::Input(super::Error::Storage(e)))
        })();
        match prepared {
            Ok(lease) => Ok(Self {
                prepared_return: result.prepared_return,
                answer,
                frame: Frame::new(lease, result.frame.options),
                clients,
                count: result.count,
            }),
            Err(error) => {
                for client in clients.into_iter().flatten() {
                    graph.release(client);
                }
                Err(error)
            }
        }
    }
    pub fn from_exports<G: Capabilities<'graph>>(
        frame: &Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        batch: &exports::Batch<'_, '_, '_, Client<'graph>>,
        graph: &mut G,
        scratch: &mut [u32],
    ) -> Result<Self, Error> {
        Self::from_exports_with_capacity(frame, messages, batch, graph, scratch, Capacity::Inline)
    }
    pub fn from_exports_with_capacity<G: Capabilities<'graph>>(
        frame: &Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        batch: &exports::Batch<'_, '_, '_, Client<'graph>>,
        graph: &mut G,
        scratch: &mut [u32],
        capacity: Capacity,
    ) -> Result<Self, Error> {
        let mut clients = Buffer::Inline(core::array::from_fn(|_| None));
        let result = frame
            .with(messages, |root| {
                let rpc_capnp::message::Return(result) =
                    root.which().map_err(|_| Error::Missing)?
                else {
                    return Err(Error::Missing);
                };
                let result = result.map_err(|e| Error::Input(e.into()))?;
                let rpc_capnp::return_::Results(payload) =
                    result.which().map_err(|_| Error::Missing)?
                else {
                    return Err(Error::Missing);
                };
                let payload = payload.map_err(|e| Error::Input(e.into()))?;
                let table = payload
                    .get_cap_table()
                    .map_err(|e| Error::Input(e.into()))?;
                let count = usize::try_from(table.len()).map_err(|_| Error::Capacity)?;
                if !clients.ensure(capacity, count, || None) {
                    return Err(Error::Capacity);
                }
                payload
                    .get_content()
                    .validate_capability_indices(table.len())
                    .map_err(|e| Error::Input(e.into()))?;
                let mut descriptors = batch.descriptors(scratch).map_err(Error::Descriptors)?;
                for descriptor in table {
                    match descriptor
                        .which()
                        .map_err(|_| Error::UnsupportedDescriptor)?
                    {
                        rpc_capnp::cap_descriptor::None(()) => (),
                        rpc_capnp::cap_descriptor::SenderHosted(id) => descriptors
                            .record_kind(ExportId::from_wire(id), false)
                            .map_err(Error::Descriptors)?,
                        rpc_capnp::cap_descriptor::SenderPromise(id) => descriptors
                            .record_kind(ExportId::from_wire(id), true)
                            .map_err(Error::Descriptors)?,
                        _ => return Err(Error::UnsupportedDescriptor),
                    }
                }
                descriptors.finish().map_err(Error::Descriptors)?;
                for (index, descriptor) in table.iter().enumerate() {
                    let id = match descriptor
                        .which()
                        .map_err(|_| Error::UnsupportedDescriptor)?
                    {
                        rpc_capnp::cap_descriptor::None(()) => continue,
                        rpc_capnp::cap_descriptor::SenderHosted(id)
                        | rpc_capnp::cap_descriptor::SenderPromise(id) => ExportId::from_wire(id),
                        _ => return Err(Error::UnsupportedDescriptor),
                    };
                    let client = batch.pending_owner(id).ok_or(Error::Missing)?;
                    let frozen = graph.snapshot(client).map_err(Error::Graph)?;
                    if frozen.key() != client.key() {
                        graph.release(frozen);
                        return Err(Error::UnfrozenExport);
                    }
                    clients[index] = Some(frozen);
                }
                Ok((count, AnswerId::from_wire(result.get_answer_id())))
            })
            .map_err(Error::Input)
            .and_then(core::convert::identity);
        let (count, answer) = match result {
            Ok(count) => count,
            Err(error) => {
                for client in clients.into_iter().flatten() {
                    graph.release(client);
                }
                return Err(error);
            }
        };
        match messages.retain(&frame.lease) {
            Ok(lease) => Ok(Self {
                prepared_return: None,
                answer,
                frame: Frame::new(lease, frame.options),
                clients,
                count,
            }),
            Err(error) => {
                for client in clients.into_iter().flatten() {
                    graph.release(client);
                }
                Err(Error::Input(super::Error::Storage(error)))
            }
        }
    }
    pub fn target<G: Capabilities<'graph>>(
        &self,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
        graph: &mut G,
    ) -> Result<Client<'graph>, Error> {
        // Only the retained capability escapes this lookup. Borrow the frame
        // for decoding rather than allocating a shared reader for its bytes.
        self.frame
            .with_borrowed_root(messages, |root| {
                let rpc_capnp::message::Return(result) =
                    root.which().map_err(|_| Error::Missing)?
                else {
                    return Err(Error::Missing);
                };
                let result = result.map_err(|e| Error::Input(e.into()))?;
                let rpc_capnp::return_::Results(payload) =
                    result.which().map_err(|_| Error::Missing)?
                else {
                    return Err(Error::Missing);
                };
                let index = usize::try_from(
                    path.resolve(payload.map_err(|e| Error::Input(e.into()))?.get_content())
                        .map_err(|e| Error::Input(e.into()))?,
                )
                .map_err(|_| Error::Missing)?;
                let client = self
                    .clients
                    .get(..self.count)
                    .and_then(|slice| slice.get(index))
                    .and_then(Option::as_ref)
                    .ok_or(Error::Missing)?;
                graph.retain_key(client.key()).map_err(Error::Graph)
            })
            .map_err(Error::Input)?
    }
    pub fn release<G: Capabilities<'graph>>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
    ) {
        for client in self.clients.into_iter().flatten() {
            graph.release(client);
        }
        self.frame.release(messages);
    }
}

/// Stored as an answer owner. A producer may finish locally before its wire
/// Return is accepted; already-admitted pipelines still need those local results
/// if the peer has meanwhile sent Finish and will receive a canceled Return.
pub struct Results<'pool, 'graph, A, const N: usize> {
    pub producer: A,
    pub(crate) queued: Option<capntproto_rpc_engine::clients::dispatch::TaskKey<'graph>>,
    pub(super) value: Option<core::result::Result<Published<'pool, 'graph, N>, capnp::ErrorKind>>,
    pub(crate) network_authority: Option<Client<'graph>>,
    pub(crate) redirect: Option<capntproto_rpc_engine::questions::Local<'graph>>,
}
impl<'pool, 'graph, A, const N: usize> Results<'pool, 'graph, A, N> {
    /// Whether the producer's final local result has not yet been frozen.
    /// An explicit early pipeline can still be available while this is true.
    pub fn is_pending(&self) -> bool {
        self.value.is_none()
    }
    pub fn new(producer: A) -> Self {
        Self {
            producer,
            queued: None,
            value: None,
            network_authority: None,
            redirect: None,
        }
    }
    pub fn target<G: Capabilities<'graph>>(
        &self,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
        graph: &mut G,
    ) -> Result<Option<Client<'graph>>, Error> {
        match &self.value {
            None => Ok(None),
            Some(Ok(published)) => published.target(path, messages, graph).map(Some),
            Some(Err(kind)) => Err(Error::Input(super::Error::Wire(*kind))),
        }
    }
    pub fn release<G: Capabilities<'graph>>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
        release_question: impl FnOnce(capntproto_rpc_engine::questions::Local<'graph>),
    ) -> A {
        if let Some(child) = self.redirect {
            release_question(child);
        }
        if let Some(Ok(published)) = self.value {
            published.release(messages, graph);
        }
        if let Some(provision) = self.network_authority {
            graph.release(provision);
        }
        self.producer
    }
}

#[must_use = "release or retry the unconsumed producer result"]
pub struct CompletionRejected<'pool, 'graph, const N: usize> {
    pub error: Error,
    pub value: Result<Published<'pool, 'graph, N>, capnp::ErrorKind>,
}

/// Complete exactly the live answer generation and validate the result envelope
/// against its wire ID. Rejection preserves every result owner for cleanup.
#[allow(clippy::result_large_err)] // Inline bounded owners; boxing would require an allocator.
pub fn complete<'pool, 'graph, 'a, A, const N: usize>(
    answers: &mut answers::Answers<
        'a,
        '_,
        super::answer::State<'pool, Results<'pool, 'graph, A, N>>,
    >,
    key: answers::Key<'a>,
    value: Result<Published<'pool, 'graph, N>, capnp::ErrorKind>,
) -> Result<(), CompletionRejected<'pool, 'graph, N>> {
    let result = (|| {
        let state = answers.get_mut(key).map_err(Error::Answer)?;
        if state.owner.value.is_some() {
            return Err(Error::AlreadyCompleted);
        }
        if value
            .as_ref()
            .is_ok_and(|published| published.answer != key.id())
        {
            return Err(Error::Mismatch);
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            answers.get_mut(key).expect("validated answer").owner.value = Some(value);
            Ok(())
        }
        Err(error) => Err(CompletionRejected { error, value }),
    }
}
struct Waiter<'graph, 'a, const P: usize> {
    answer: answers::Pipeline<'a>,
    client: clients::Key<'graph>,
    subscription: clients::Subscription<'graph>,
    previous: Option<usize>,
    next: Option<usize>,
    path: Buffer<u16, P>,
    length: usize,
}
pub struct Slot<'graph, 'a, const P: usize> {
    waiter: Option<Waiter<'graph, 'a, P>>,
    ready: capntproto_rpc_engine::waiting::Slot,
}
impl<const P: usize> Slot<'_, '_, P> {
    pub const EMPTY: Self = Self {
        waiter: None,
        ready: capntproto_rpc_engine::waiting::Slot::EMPTY,
    };
}

pub struct Bindings<'slots, 'graph, 'a, const P: usize> {
    pub(crate) path_capacity: Capacity,
    slots: SlotStorage<'slots, Slot<'graph, 'a, P>>,
    live: usize,
    listener: Option<clients::Listener<'graph>>,
    registry: Option<answers::PipelineRegistry<'a>>,
    ready: capntproto_rpc_engine::waiting::Waiting<3>,
    ready_class: u8,
}
pub enum Progress<'graph> {
    Vacant,
    Pending,
    Abandoned,
    Resolved(Option<clients::Barrier<'graph>>),
}
impl<'slots, 'graph, 'a, const P: usize> Bindings<'slots, 'graph, 'a, P> {
    /// Forward a local answer observer to its tail child's exact question/path.
    /// This is not a Return observer: the child deliberately keeps its results
    /// on the peer, so ResultsSentElsewhere must not erase this original route.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn poll_redirect<'pool, A, Peer: Copy + Eq, S, G, const N: usize>(
        &mut self,
        index: usize,
        questions: &mut capntproto_rpc_engine::questions::Questions<'graph, '_>,
        graph: &mut G,
        answers: &mut answers::Answers<
            'a,
            '_,
            super::answer::State<'pool, Results<'pool, 'graph, A, N>>,
        >,
        peer: Peer,
    ) -> Result<Option<Progress<'graph>>, Error>
    where
        G: Capabilities<'graph, Owner = crate::clients::Route<'graph, Peer, S, P>, Peer = Peer>,
    {
        let result = self.poll_redirect_inner(index, questions, graph, answers, peer);
        if result.is_err()
            && self
                .slots
                .get(index)
                .is_some_and(|slot| slot.waiter.is_some())
        {
            if let Err(Error::Graph(error)) = &result {
                if let Some(requirement) = crate::resources::graph_wait(*error, graph) {
                    self.ready
                        .park(&mut self.slots, index, requirement, |slot| &mut slot.ready);
                } else {
                    self.ready
                        .wake(&mut self.slots, index, |slot| &mut slot.ready);
                }
            } else {
                self.ready
                    .wake(&mut self.slots, index, |slot| &mut slot.ready);
            }
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn poll_redirect_inner<'pool, A, Peer: Copy + Eq, S, G, const N: usize>(
        &mut self,
        index: usize,
        questions: &mut capntproto_rpc_engine::questions::Questions<'graph, '_>,
        graph: &mut G,
        answers: &mut answers::Answers<
            'a,
            '_,
            super::answer::State<'pool, Results<'pool, 'graph, A, N>>,
        >,
        peer: Peer,
    ) -> Result<Option<Progress<'graph>>, Error>
    where
        G: Capabilities<'graph, Owner = crate::clients::Route<'graph, Peer, S, P>, Peer = Peer>,
    {
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(Error::Capacity);
        };
        let Some(waiter) = &slot.waiter else {
            return Ok(None);
        };
        let Some(source) = answers.pipelined(&waiter.answer).owner.redirect.as_ref() else {
            return Ok(None);
        };
        let observed = match graph.retain_key(waiter.client) {
            Ok(client) => client,
            Err(clients::Error::Stale) => {
                self.finish(index, answers, graph);
                return Ok(Some(Progress::Abandoned));
            }
            Err(error) => return Err(Error::Graph(error)),
        };
        let replacement = crate::clients::outgoing::freeze(
            graph,
            questions,
            source,
            pipeline::Path::new(&waiter.path[..waiter.length]),
            peer,
            self.path_capacity,
        );
        let replacement = match replacement {
            Ok(replacement) => replacement,
            Err(error) => {
                graph.release(observed);
                return Err(match error {
                    crate::clients::outgoing::Error::Graph(error) => Error::Graph(error),
                    crate::clients::outgoing::Error::Question(error) => Error::Question(error),
                    crate::clients::outgoing::Error::Capacity => Error::Capacity,
                    _ => unreachable!("freeze only reserves a question route"),
                });
            }
        };
        let mut replacement = Some(replacement);
        let outcome = graph.resolve(waiter.client, &mut replacement, Source::Direct);
        graph.release(observed);
        match outcome {
            Ok(barrier) => {
                self.finish(index, answers, graph);
                Ok(Some(Progress::Resolved(barrier)))
            }
            Err(error) => {
                graph.release(replacement.expect("unaccepted redirect"));
                Err(Error::Graph(error))
            }
        }
    }
    pub(crate) fn resource_waiting(&self, index: usize) -> bool {
        self.slots.get(index).is_some_and(|s| s.ready.is_parked())
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
    /// Occupancy follows observer ownership, independently of retained capacity.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.live == 0
    }
    fn finish<T, G: Capabilities<'graph>>(
        &mut self,
        index: usize,
        answers: &mut answers::Answers<'a, '_, T>,
        graph: &mut G,
    ) {
        self.ready
            .remove(&mut self.slots, index, |slot| &mut slot.ready);
        let waiter = self.slots[index]
            .waiter
            .take()
            .expect("live answer observer");
        graph.unsubscribe(waiter.subscription);
        if let Some(previous) = waiter.previous {
            self.slots[previous].waiter.as_mut().unwrap().next = waiter.next;
        }
        if let Some(next) = waiter.next {
            self.slots[next].waiter.as_mut().unwrap().previous = waiter.previous;
        }
        answers.unlink_pipeline_observer(
            self.registry.as_ref().unwrap(),
            waiter.answer.key(),
            index,
            waiter.next,
        );
        self.live -= 1;
        answers.release_pipeline_deferred(waiter.answer);
    }
    pub(crate) fn has_ready<T, G: Capabilities<'graph>>(
        &self,
        answers: &answers::Answers<'a, '_, T>,
        graph: &G,
    ) -> bool {
        self.ready.has_ready(
            &self.slots,
            |slot| &slot.ready,
            |r| crate::resources::graph_ready(r, graph),
        ) || (self.registry.is_some() && answers.has_pipeline_work())
            || self
                .listener
                .as_ref()
                .is_some_and(|listener| graph.has_events(listener))
    }
    pub(crate) fn next_ready<T, G: Capabilities<'graph>>(
        &mut self,
        answers: &mut answers::Answers<'a, '_, T>,
        graph: &mut G,
    ) -> Option<usize> {
        // Rotate sources as well as owners. A stream of canceled clients or a
        // capacity-blocked retry cannot starve a ready producer at budget 1.
        for _ in 0..3 {
            let class = self.ready_class;
            self.ready_class = (class + 1) % 3;
            match class {
                0 => {
                    if let Some(event) = self
                        .listener
                        .as_ref()
                        .and_then(|listener| graph.next_event(listener))
                    {
                        if self
                            .slots
                            .get(event.owner)
                            .and_then(|s| s.waiter.as_ref())
                            .is_some_and(|waiter| waiter.subscription.key() == event.subscription)
                        {
                            self.ready
                                .remove(&mut self.slots, event.owner, |slot| &mut slot.ready);
                            return Some(event.owner);
                        }
                    }
                }
                1 => {
                    if let Some(index) = self.ready.pop(
                        &mut self.slots,
                        |slot| &mut slot.ready,
                        |r| crate::resources::graph_ready(r, graph),
                    ) {
                        return Some(index);
                    }
                }
                _ => {
                    if let Some((key, index)) = self
                        .registry
                        .as_ref()
                        .and_then(|registry| answers.next_pipeline_work(registry))
                    {
                        let next = self.slots[index]
                            .waiter
                            .as_ref()
                            .expect("ready answer observer")
                            .next;
                        answers.advance_pipeline_work(
                            self.registry.as_ref().unwrap(),
                            key,
                            index,
                            next,
                        );
                        self.ready
                            .remove(&mut self.slots, index, |slot| &mut slot.ready);
                        return Some(index);
                    }
                }
            }
        }
        None
    }
    /// Roll back an unexposed binding before releasing its private graph client.
    pub(crate) fn discard_unexposed<T, G: Capabilities<'graph>>(
        &mut self,
        key: clients::Key<'graph>,
        answers: &mut answers::Answers<'a, '_, T>,
        graph: &mut G,
    ) {
        let index = graph
            .exclusive_subscription_owner(self.listener.as_ref().expect("pipeline listener"), key)
            .expect("unexposed live pipeline observer");
        debug_assert_eq!(self.slots[index].waiter.as_ref().unwrap().client, key);
        self.finish(index, answers, graph);
    }
    pub fn new(slots: &'slots mut [Slot<'graph, 'a, P>]) -> Result<Self, Error> {
        Self::from_storage(SlotStorage::borrowed(slots))
    }
    pub(crate) fn from_storage(
        slots: SlotStorage<'slots, Slot<'graph, 'a, P>>,
    ) -> Result<Self, Error> {
        Self::from_storage_with_capacity(slots, Capacity::Inline)
    }
    pub(crate) fn from_storage_with_capacity(
        slots: SlotStorage<'slots, Slot<'graph, 'a, P>>,
        path_capacity: Capacity,
    ) -> Result<Self, Error> {
        if slots.iter().any(|s| s.waiter.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            slots,
            live: 0,
            listener: None,
            registry: None,
            ready: capntproto_rpc_engine::waiting::Waiting::<3>::EMPTY,
            ready_class: 0,
            path_capacity,
        })
    }
    pub fn bind<T, G: Capabilities<'graph>>(
        &mut self,
        graph: &mut G,
        answers: &mut answers::Answers<'a, '_, T>,
        answer: &answers::Pipeline<'a>,
        path: pipeline::Path<'_>,
    ) -> Result<Client<'graph>, Error> {
        if self.registry.is_none() {
            self.registry = Some(answers.claim_pipeline_registry().map_err(Error::Answer)?);
        }
        let fields = self
            .path_capacity
            .copy(path.fields())
            .ok_or(Error::Capacity)?;
        let index = self
            .slots
            .vacant(|_, slot| slot.waiter.is_none(), || Slot::EMPTY)
            .ok_or(Error::ObserversFull)?;
        let answer = answers.retain_pipeline(answer).map_err(Error::Answer)?;
        let client = match graph.promise(&mut None) {
            Ok(client) => client,
            Err(error) => {
                answers.release_pipeline_deferred(answer);
                return Err(Error::Graph(error));
            }
        };

        let subscribed = (|| {
            if self.listener.is_none() {
                self.listener = Some(graph.listen()?);
            }
            graph.subscribe(
                self.listener.as_ref().unwrap(),
                client.key(),
                index,
                clients::Watch::Abandonment,
            )
        })();
        let subscription = match subscribed {
            Ok(subscription) => subscription,
            Err(error) => {
                graph.release(client);
                answers.release_pipeline_deferred(answer);
                return Err(Error::Graph(error));
            }
        };
        let next = answers
            .link_pipeline_observer(self.registry.as_ref().unwrap(), answer.key(), index)
            .expect("retained answer");
        if let Some(next) = next {
            self.slots[next].waiter.as_mut().unwrap().previous = Some(index);
        }
        self.slots[index].waiter = Some(Waiter {
            subscription,
            previous: None,
            next,
            answer,
            client: client.key(),
            path: fields,
            length: path.fields().len(),
        });
        self.live += 1;
        Ok(client)
    }
    /// Observe one producer without retaining a canceled graph observer. The
    /// weak graph key prevents a dependency cycle after all child calls cancel.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_one<'pool, A, const N: usize, G: Capabilities<'graph>>(
        &mut self,
        index: usize,
        graph: &mut G,
        answers: &mut answers::Answers<
            'a,
            '_,
            super::answer::State<'pool, Results<'pool, 'graph, A, N>>,
        >,
        messages: &messages::Messages<'pool, '_, '_>,
        broken: impl FnOnce(capnp::ErrorKind) -> G::Owner,
    ) -> Result<Progress<'graph>, Error> {
        self.poll_one_with(index, graph, answers, messages, broken, |_, _, _| Ok(None))
    }
    /// An application may publish a pipeline before its final result. The
    /// resolver receives the exact retained producer, never a numeric answer ID.
    /// Some wins over the final payload; None falls back to the stored result.
    /// Outer errors preserve the observer for retry; inner errors break it.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub fn poll_one_with<'pool, A, const N: usize, G: Capabilities<'graph>>(
        &mut self,
        index: usize,
        graph: &mut G,
        answers: &mut answers::Answers<
            'a,
            '_,
            super::answer::State<'pool, Results<'pool, 'graph, A, N>>,
        >,
        messages: &messages::Messages<'pool, '_, '_>,
        broken: impl FnOnce(capnp::ErrorKind) -> G::Owner,
        resolve: impl FnOnce(
            &A,
            pipeline::Path<'_>,
            &mut G,
        ) -> Result<Option<Result<Client<'graph>, capnp::ErrorKind>>, Error>,
    ) -> Result<Progress<'graph>, Error> {
        if index >= self.slots.len() {
            return Err(Error::Capacity);
        }
        self.ready
            .remove(&mut self.slots, index, |slot| &mut slot.ready);
        let result = self.poll_one_inner(index, graph, answers, messages, broken, resolve);
        if result.is_err() && self.slots[index].waiter.is_some() {
            if let Err(Error::Graph(error)) = &result {
                if let Some(requirement) = crate::resources::graph_wait(*error, graph) {
                    self.ready
                        .park(&mut self.slots, index, requirement, |slot| &mut slot.ready);
                } else {
                    self.ready
                        .wake(&mut self.slots, index, |slot| &mut slot.ready);
                }
            } else {
                self.ready
                    .wake(&mut self.slots, index, |slot| &mut slot.ready);
            }
        }
        result
    }
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn poll_one_inner<'pool, A, const N: usize, G: Capabilities<'graph>>(
        &mut self,
        index: usize,
        graph: &mut G,
        answers: &mut answers::Answers<
            'a,
            '_,
            super::answer::State<'pool, Results<'pool, 'graph, A, N>>,
        >,
        messages: &messages::Messages<'pool, '_, '_>,
        broken: impl FnOnce(capnp::ErrorKind) -> G::Owner,
        resolve: impl FnOnce(
            &A,
            pipeline::Path<'_>,
            &mut G,
        ) -> Result<Option<Result<Client<'graph>, capnp::ErrorKind>>, Error>,
    ) -> Result<Progress<'graph>, Error> {
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(Error::Capacity);
        };
        let Some(waiter) = &slot.waiter else {
            return Ok(Progress::Vacant);
        };
        let observed = match graph.retain_key(waiter.client) {
            Ok(client) => client,
            Err(clients::Error::Stale) => {
                self.finish(index, answers, graph);
                return Ok(Progress::Abandoned);
            }
            Err(error) => return Err(Error::Graph(error)),
        };
        let owner = &answers.pipelined(&waiter.answer).owner;
        let path = pipeline::Path::new(&waiter.path[..waiter.length]);
        let replacement = match resolve(&owner.producer, path, graph) {
            Err(error) => {
                graph.release(observed);
                return Err(error);
            }
            Ok(Some(value)) => value.map_err(|kind| Error::Input(super::Error::Wire(kind))),
            Ok(None) => match &owner.value {
                None => {
                    graph.release(observed);
                    return Ok(Progress::Pending);
                }
                Some(Ok(published)) => published.target(path, messages, graph),
                Some(Err(kind)) => Err(Error::Input(super::Error::Wire(*kind))),
            },
        };
        let replacement = match replacement {
            Ok(client) => Ok(client),
            Err(Error::Graph(error)) => Err(Error::Graph(error)),
            Err(error) => {
                let kind = match error {
                    Error::Input(super::Error::Wire(kind)) => kind,
                    _ => capnp::ErrorKind::Failed,
                };
                graph
                    .endpoint_with(Location::Broken, || broken(kind))
                    .map_err(Error::Graph)
            }
        };
        let replacement = match replacement {
            Ok(client) => client,
            Err(error) => {
                graph.release(observed);
                return Err(error);
            }
        };
        let mut replacement = Some(replacement);
        let outcome = graph.resolve(waiter.client, &mut replacement, Source::Direct);
        graph.release(observed);
        match outcome {
            Ok(barrier) => {
                self.finish(index, answers, graph);
                Ok(Progress::Resolved(barrier))
            }
            Err(error) => {
                if let Some(client) = replacement {
                    graph.release(client);
                }
                Err(Error::Graph(error))
            }
        }
    }
    /// Detach pending answer dependencies during terminal driver cleanup.
    pub fn drain_one<T, G: Capabilities<'graph>>(
        &mut self,
        answers: &mut answers::Answers<'a, '_, T>,
        graph: &mut G,
    ) -> bool {
        let Some(index) = self.slots.iter().position(|s| s.waiter.is_some()) else {
            if let Some(registry) = self.registry.take() {
                answers.release_pipeline_registry(registry);
            }
            if let Some(listener) = self.listener.take() {
                graph.close_listener(listener);
            }
            return false;
        };
        self.finish(index, answers, graph);
        true
    }
}

impl<'pool, 'id, const N: usize> Published<'pool, 'id, N> {
    /// Snapshot a mixed result table, including exact receiverHosted and
    /// receiverAnswer routes. Their original remote endpoint is retained even
    /// if an observed promise has since resolved locally.
    #[allow(clippy::too_many_arguments)]
    pub fn from_reflected<I, P: Copy + Eq, S, G, const PATH: usize>(
        frame: &Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        batch: &exports::Batch<'_, 'id, '_, Client<'id>>,
        graph: &mut G,
        bindings: &crate::clients::Imports<'_, 'id, 'id, P>,
        references: &mut crate::publication::reflected::References<'_, 'id, '_, 'id, '_, '_, I>,
        scratch: &mut [u32],
    ) -> Result<Self, Error>
    where
        G: Capabilities<'id, Owner = crate::clients::Route<'id, P, S, PATH>, Peer = P>,
    {
        Self::from_reflected_with_capacity(
            frame,
            messages,
            batch,
            graph,
            bindings,
            references,
            scratch,
            Capacity::Inline,
            Capacity::Inline,
        )
    }
    /// Snapshot mixed results with explicit capability and transform budgets.
    #[allow(clippy::too_many_arguments)]
    pub fn from_reflected_with_capacity<I, P: Copy + Eq, S, G, const PATH: usize>(
        frame: &Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        batch: &exports::Batch<'_, 'id, '_, Client<'id>>,
        graph: &mut G,
        bindings: &crate::clients::Imports<'_, 'id, 'id, P>,
        references: &mut crate::publication::reflected::References<'_, 'id, '_, 'id, '_, '_, I>,
        scratch: &mut [u32],
        capacity: Capacity,
        path_capacity: Capacity,
    ) -> Result<Self, Error>
    where
        G: Capabilities<'id, Owner = crate::clients::Route<'id, P, S, PATH>, Peer = P>,
    {
        use crate::publication::{
            self,
            reflected::{self, Reference},
        };
        let mut count = 0;
        publication::validate(
            batch,
            &messages.read(&frame.lease),
            scratch,
            frame.options,
            |descriptor, index| {
                count += 1;
                let reference = references
                    .slots
                    .get(index)
                    .ok_or(publication::Error::ReflectionMismatch)?;
                reflected::check_reference(
                    descriptor,
                    reference,
                    references.imports,
                    references.questions,
                )
            },
        )
        .map_err(|error| Error::Input(super::Error::Output(error.into())))?;
        if count != references.slots.len() {
            return Err(Error::Mismatch);
        }
        let mut clients = capacity
            .buffer::<_, N>(count, || None)
            .ok_or(Error::Capacity)?;
        let outcome = frame
            .with(messages, |root| -> Result<AnswerId, Error> {
                let rpc_capnp::message::Return(result) =
                    root.which().map_err(|_| Error::Missing)?
                else {
                    return Err(Error::Missing);
                };
                let result = result.map_err(|error| Error::Input(error.into()))?;
                let rpc_capnp::return_::Results(payload) =
                    result.which().map_err(|_| Error::Missing)?
                else {
                    return Err(Error::Missing);
                };
                let payload = payload.map_err(|error| Error::Input(error.into()))?;
                let table = payload
                    .get_cap_table()
                    .map_err(|error| Error::Input(error.into()))?;
                for (index, descriptor) in table.iter().enumerate() {
                    let frozen = match &references.slots[index] {
                        Reference::Hosted(local) => bindings
                            .freeze_import(graph, references.imports, local)
                            .map_err(|error| match error {
                                crate::clients::Error::Graph(error) => Error::Graph(error),
                                _ => Error::Missing,
                            })?,
                        Reference::Answer { question, path } => crate::clients::outgoing::freeze(
                            graph,
                            references.questions,
                            question,
                            *path,
                            bindings.peer(),
                            path_capacity,
                        )
                        .map_err(|error| match error {
                            crate::clients::outgoing::Error::Graph(error) => Error::Graph(error),
                            crate::clients::outgoing::Error::Capacity => Error::Capacity,
                            _ => Error::Missing,
                        })?,
                        Reference::None => {
                            let id = match descriptor
                                .which()
                                .map_err(|_| Error::UnsupportedDescriptor)?
                            {
                                rpc_capnp::cap_descriptor::None(()) => continue,
                                rpc_capnp::cap_descriptor::SenderHosted(id)
                                | rpc_capnp::cap_descriptor::SenderPromise(id) => {
                                    ExportId::from_wire(id)
                                }
                                _ => return Err(Error::UnsupportedDescriptor),
                            };
                            let client = batch.pending_owner(id).ok_or(Error::Missing)?;
                            let snapshot = graph.snapshot(client).map_err(Error::Graph)?;
                            if snapshot.key() != client.key() {
                                graph.release(snapshot);
                                return Err(Error::UnfrozenExport);
                            }
                            snapshot
                        }
                    };
                    clients[index] = Some(frozen);
                }
                Ok(AnswerId::from_wire(result.get_answer_id()))
            })
            .map_err(Error::Input)
            .and_then(core::convert::identity);
        let outcome = outcome.and_then(|answer| {
            messages
                .retain(&frame.lease)
                .map(|lease| (answer, lease))
                .map_err(|error| Error::Input(super::Error::Storage(error)))
        });
        match outcome {
            Ok((answer, lease)) => Ok(Self {
                prepared_return: None,
                answer,
                frame: Frame::new(lease, frame.options),
                clients,
                count,
            }),
            Err(error) => {
                for client in clients.into_iter().flatten() {
                    graph.release(client);
                }
                Err(error)
            }
        }
    }
}

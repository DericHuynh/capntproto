//! Owning routes for calls pipelined onto an outgoing question. Their retained
//! question references prevent Finish until the last original route retires.
use super::*;
use crate::buffer::{Buffer, Capacity};
use capntproto_rpc_engine::storage::Storage as SlotStorage;
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
    original_route: bool,
    fields: Buffer<u16, PATH>,
    length: usize,
}
pub struct Slot<'id, const PATH: usize> {
    entry: Option<Entry<'id, PATH>>,
}
impl<const PATH: usize> Slot<'_, PATH> {
    pub const EMPTY: Self = Self { entry: None };
}
pub struct Bindings<'s, 'id, P, const PATH: usize> {
    path_capacity: Capacity,
    slots: SlotStorage<'s, Slot<'id, PATH>>,
    peer: P,
    live: usize,
}
pub enum Progress<'id> {
    Unrelated,
    Pending,
    Abandoned,
    Resolved(Option<clients::Barrier<'id>>),
}
impl<'s, 'id, P: Copy + Eq, const PATH: usize> Bindings<'s, 'id, P, PATH> {
    pub(crate) fn clear_terminal(&mut self) {
        for slot in self.slots.iter_mut() {
            slot.entry = None;
        }
        self.live = 0;
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
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
            live: 0,
            slots,
            peer,
            path_capacity,
        })
    }
    pub fn pending_for(&self, question: questions::Key<'id>) -> bool {
        self.slots.iter().any(|slot| {
            slot.entry
                .as_ref()
                .is_some_and(|entry| entry.question == question)
        })
    }

    /// Register an observer whose invocation was still queued when Return
    /// arrived. It never used a remote route, so no original endpoint or loopback
    /// obligation is needed. Keep Response until poll_one consumes the payload.
    pub(crate) fn observe_returned<I, const N: usize, G: clients::Capabilities<'id>>(
        &mut self,
        response: &Response<'_, 'id, 'id, 'id, 'id, I, N>,
        graph: &mut G,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, Error> {
        if path.fields().len() > self.path_capacity.maximum::<PATH>() {
            return Err(Error::Capacity);
        }
        let slot = self
            .slots
            .vacant_mut(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
            .ok_or(Error::Capacity)?;
        let fields = self
            .path_capacity
            .copy(path.fields())
            .ok_or(Error::Capacity)?;
        let client = graph.promise(&mut None).map_err(Error::Graph)?;
        self.live += 1;
        slot.entry = Some(Entry {
            question: response.key(),
            client: client.key(),
            original_route: false,
            fields,
            length: path.fields().len(),
        });
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
                slot.entry = None;
                self.live -= 1;
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
                slot.entry = None;
                self.live -= 1;
                Ok(Progress::Resolved(barrier))
            }
            Err(error) => {
                graph.release(replacement.expect("unaccepted replacement"));
                if observer {
                    pipelines.discard_unexposed(key, answers);
                }
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
        questions.pipeline_id(local).map_err(Error::Question)?;
        if path.fields().len() > self.path_capacity.maximum::<PATH>() {
            return Err(Error::Capacity);
        }
        let matching = self.slots.iter().position(|slot| {
            slot.entry.as_ref().is_some_and(|entry| {
                entry.question == question.key() && &entry.fields[..entry.length] == path.fields()
            })
        });
        if let Some(index) = matching {
            match graph.retain_key(self.slots[index].entry.as_ref().unwrap().client) {
                Ok(client) => return Ok(client),
                Err(clients::Error::Stale) => (),
                Err(error) => return Err(Error::Graph(error)),
            }
        }
        let index = matching
            .or_else(|| self.slots.iter().position(|slot| slot.entry.is_none()))
            .or_else(|| {
                self.slots.iter_mut().enumerate().find_map(|(index, slot)| {
                    match graph.retain_key(slot.entry.as_ref()?.client) {
                        Err(clients::Error::Stale) => {
                            slot.entry = None;
                            self.live -= 1;
                            Some(index)
                        }
                        Ok(client) => {
                            graph.release(client);
                            None
                        }
                        Err(_) => None,
                    }
                })
            })
            .or_else(|| {
                self.slots
                    .vacant(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
            })
            .ok_or(Error::Capacity)?;
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
        if self.slots[index].entry.is_none() {
            self.live += 1;
        }
        self.slots[index].entry = Some(Entry {
            question: question.key(),
            client: client.key(),
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

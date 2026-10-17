//! Bounded observers for exported local promises. Each export gets an independent
//! promise, resolved once to a frozen route before its wire Resolve is published.
use super::*;
use crate::{
    owned::{self, Frame},
    rpc_capnp::api::{exception::Type, CapDescriptor, Exception, Message, Resolve},
};
use capntproto_rpc_engine::storage::Storage as SlotStorage;
use capntproto_rpc_engine::{
    exports,
    ids::{ExportId, WireId},
    messages,
};
#[cfg(test)]
mod tests;

enum State<'g> {
    Pending(clients::Client<'g>),
    Ready(clients::Client<'g>),
}
struct Entry<'g, 'e> {
    state: State<'g>,
    target: clients::Key<'g>,
    export: exports::Key<'e>,
    id: ExportId,
}
pub struct Slot<'g, 'e> {
    entry: Option<Entry<'g, 'e>>,
}
impl Slot<'_, '_> {
    pub const EMPTY: Self = Self { entry: None };
}
pub struct Bindings<'s, 'g, 'e> {
    slots: SlotStorage<'s, Slot<'g, 'e>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Capacity,
    Graph(clients::Error),
    Export(exports::Error),
    Output(owned::Error),
    Storage(messages::Error),
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "exported promise observer: {self:?}")
    }
}
impl core::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Vacant,
    Pending,
    Abandoned,
    SettledLocally,
    Published,
}

impl<'s, 'g, 'e> Bindings<'s, 'g, 'e> {
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub(crate) fn vacant_within(&self, maximum: usize) -> bool {
        self.slots.len() <= maximum && self.slots.iter().all(|slot| slot.entry.is_none())
    }
    pub fn new(slots: &'s mut [Slot<'g, 'e>]) -> Result<Self, Error> {
        Self::from_storage(SlotStorage::borrowed(slots))
    }
    pub(crate) fn from_storage(slots: SlotStorage<'s, Slot<'g, 'e>>) -> Result<Self, Error> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self { slots })
    }
    pub fn stage<G: clients::Capabilities<'g>>(
        &mut self,
        graph: &mut G,
        batch: &mut exports::Batch<'_, 'e, '_, clients::Client<'g>>,
        client: &clients::Client<'g>,
    ) -> Result<exports::Grant<'e>, Error> {
        let (snapshot, pending) = graph.snapshot_for_export(client).map_err(Error::Graph)?;
        if !pending {
            return batch.export(snapshot).map_err(|rejected| {
                graph.release(rejected.value);
                Error::Export(rejected.error)
            });
        }
        let Some(slot) = self
            .slots
            .vacant_mut(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
        else {
            graph.release(snapshot);
            return Err(Error::Capacity);
        };
        let promise = match graph.promise(&mut None) {
            Ok(client) => client,
            Err(error) => {
                graph.release(snapshot);
                return Err(Error::Graph(error));
            }
        };
        let target = promise.key();
        let grant = match batch.export_promise(promise) {
            Ok(grant) => grant,
            Err(rejected) => {
                graph.release(snapshot);
                graph.release(rejected.value);
                return Err(Error::Export(rejected.error));
            }
        };
        slot.entry = Some(Entry {
            state: State::Pending(snapshot),
            target,
            export: grant.key(),
            id: grant.id(),
        });
        Ok(grant)
    }

    /// Poll one observer. Local settlement is once-only even if wire output is
    /// backpressured; retries publish metadata for the already-frozen route.
    /// Drain aborted/retired export owners through the graph before polling.
    #[allow(clippy::too_many_arguments)]
    pub fn poll_one<'i, 'pool, P: Copy + Eq, G>(
        &mut self,
        index: usize,
        graph: &mut G,
        exports: &mut exports::Exports<'e, '_, clients::Client<'g>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
    ) -> Result<Progress, Error>
    where
        G: clients::Capabilities<'g, Peer = P>,
        G::Owner: EndpointOwner<'i, P>,
    {
        self.poll_one_using(
            index,
            graph,
            exports,
            messages,
            queue,
            minimum,
            options,
            scratch,
            Default::default(),
            |_, _| u8::MAX,
            &Default::default(),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn poll_one_using<'i, 'pool, P: Copy + Eq, G, R>(
        &mut self,
        index: usize,
        graph: &mut G,
        exports: &mut exports::Exports<'e, '_, clients::Client<'g>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        mut resources: R,
        mut attach: impl FnMut(&G::Owner, &mut R) -> u8,
        descriptors: &crate::publication::descriptors::Descriptors<G::Owner>,
    ) -> Result<Progress, Error>
    where
        G: clients::Capabilities<'g, Peer = P>,
        G::Owner: EndpointOwner<'i, P>,
    {
        let slot = self.slots.get_mut(index).ok_or(Error::Capacity)?;
        let Some(entry) = &mut slot.entry else {
            return Ok(Progress::Vacant);
        };
        if let State::Pending(source) = &entry.state {
            let observed = match graph.retain_key(entry.target) {
                Ok(client) => client,
                Err(clients::Error::Stale) => {
                    release_entry(slot.entry.take().unwrap(), graph);
                    return Ok(Progress::Abandoned);
                }
                Err(error) => return Err(Error::Graph(error)),
            };
            let (frozen, pending) = match graph.snapshot_for_export(source) {
                Ok(value) => value,
                Err(error) => {
                    graph.release(observed);
                    return Err(Error::Graph(error));
                }
            };
            if frozen.key() == observed.key() {
                graph.release(observed);
                graph.release(frozen);
                return Err(Error::Graph(clients::Error::Cycle));
            }
            if pending {
                graph.release(observed);
                graph.release(frozen);
                return Ok(Progress::Pending);
            }
            let candidate = match graph.retain_key(frozen.key()) {
                Ok(client) => client,
                Err(error) => {
                    graph.release(observed);
                    graph.release(frozen);
                    return Err(Error::Graph(error));
                }
            };
            let mut candidate = Some(candidate);
            let result = graph.resolve(entry.target, &mut candidate, clients::Source::Direct);
            graph.release(observed);
            match result {
                Ok(None) => (),
                Ok(Some(_)) => unreachable!("export observer has no initial route"),
                Err(error) => {
                    graph.release(candidate.unwrap());
                    graph.release(frozen);
                    return Err(Error::Graph(error));
                }
            }
            let State::Pending(source) = core::mem::replace(&mut entry.state, State::Ready(frozen))
            else {
                unreachable!()
            };
            graph.release(source);
        }
        if exports.lookup(entry.id) != Some(entry.export) {
            release_entry(slot.entry.take().unwrap(), graph);
            return Ok(Progress::SettledLocally);
        }
        let State::Ready(frozen) = &entry.state else {
            unreachable!()
        };
        let exception = match graph.inspect(frozen).map_err(Error::Graph)? {
            clients::Destination::Endpoint { owner, .. } => owner.exception(),
            clients::Destination::Waiting(_) => unreachable!("frozen settled route"),
        };
        let attached = match graph.inspect(frozen).map_err(Error::Graph)? {
            clients::Destination::Endpoint { owner, .. } if exception.is_none() => {
                attach(owner, &mut resources)
            }
            _ => u8::MAX,
        };
        let output = queue.reserve_with(resources).map_err(Error::Storage)?;
        let mut batch = exports.batch();
        batch.resolve_promise(entry.export).map_err(Error::Export)?;
        let grant = if exception.is_none() {
            let candidate = graph.retain_key(frozen.key()).map_err(Error::Graph)?;
            Some(batch.export(candidate).map_err(|rejected| {
                graph.release(rejected.value);
                Error::Export(rejected.error)
            })?)
        } else {
            None
        };
        let owner = match graph.inspect(frozen).map_err(Error::Graph)? {
            clients::Destination::Endpoint { owner, .. } => Some(owner),
            _ => None,
        };
        let extra = descriptors
            .bytes(owner, false)
            .map_err(|e| Error::Output(e.into()))?;
        let minimum = minimum.checked_add(extra).ok_or(Error::Capacity)?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            let mut root = builder.try_init_root::<Message>()?;
            let mut resolve = root.field(Message::RESOLVE).init()?;
            resolve.scalar(Resolve::PROMISE_ID).set(entry.id.to_wire());
            if let Some(grant) = &grant {
                let mut descriptor = resolve.field(Resolve::CAP).init()?;
                descriptor.scalar(CapDescriptor::ATTACHED_FD).set(attached);
                descriptors.describe(owner, grant.id(), false, descriptor)?;
            } else {
                let mut failed = resolve.field(Resolve::EXCEPTION).init()?;
                let kind = match exception.unwrap() {
                    capnp::ErrorKind::Overloaded => Type::Overloaded,
                    capnp::ErrorKind::Disconnected => Type::Disconnected,
                    capnp::ErrorKind::Unimplemented => Type::Unimplemented,
                    _ => Type::Failed,
                };
                failed.scalar(Exception::TYPE).set(kind);
                failed
                    .field(Exception::REASON)
                    .copy_from("exported capability failed")?;
            }
            Ok(())
        });
        let outcome = match frame {
            Ok(frame) => owned::publish_granting(frame, batch, messages, scratch, output),
            Err(error) => {
                drop(batch);
                Err(error)
            }
        };
        if let Err(error) = outcome {
            if let Some(grant) = grant {
                graph.release(
                    exports
                        .take_retired(grant.id().to_wire() as usize)
                        .expect("aborted replacement export"),
                );
            }
            return Err(Error::Output(error));
        }
        release_entry(slot.entry.take().unwrap(), graph);
        Ok(Progress::Published)
    }
    pub fn drain_one<G: clients::Capabilities<'g>>(&mut self, graph: &mut G) -> bool {
        let Some(slot) = self.slots.iter_mut().find(|slot| slot.entry.is_some()) else {
            return false;
        };
        release_entry(slot.entry.take().unwrap(), graph);
        true
    }
}
fn release_entry<'g, G: clients::Capabilities<'g>>(entry: Entry<'g, '_>, graph: &mut G) {
    let (State::Pending(client) | State::Ready(client)) = entry.state;
    graph.release(client);
}

//! Bounded observers for exported local promises. Each export gets an independent
//! promise, resolved once to a frozen route before its wire Resolve is published.
use super::*;
use crate::{
    owned::{self, Frame},
    rpc_capnp::api::{exception::Type, CapDescriptor, Exception, Message, Resolve},
};
use capntproto_rpc_engine::{
    exports,
    ids::{ExportId, WireId},
    messages,
};
use capntproto_rpc_engine::{
    storage::Storage as SlotStorage,
    waiting::{Requirement, Slot as WaitSlot, Waiting},
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
    source_subscription: Option<clients::Subscription<'g>>,
    target_subscription: clients::Subscription<'g>,
    export: exports::Key<'e>,
    id: ExportId,
}
pub struct Slot<'g, 'e> {
    entry: Option<Entry<'g, 'e>>,
    ready: WaitSlot,
}
impl Slot<'_, '_> {
    pub const EMPTY: Self = Self {
        entry: None,
        ready: WaitSlot::EMPTY,
    };
}
pub struct Bindings<'s, 'g, 'e> {
    slots: SlotStorage<'s, Slot<'g, 'e>>,
    live: usize,
    listener: Option<clients::Listener<'g>>,
    ready: Waiting<{ crate::resources::COUNT }>,
    prefer_event: bool,
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
    fn resource_ready<G: clients::Capabilities<'g>>(
        r: Requirement,
        graph: &G,
        exports: &exports::Exports<'e, '_, clients::Client<'g>>,
        messages: &messages::Messages<'_, '_, '_>,
        queue: &messages::Queue<'_, '_, impl Sized>,
    ) -> bool {
        crate::resources::graph_ready(r, graph)
            || (r.resource == crate::resources::EXPORTS && exports.available() >= r.minimum)
            || crate::resources::message_ready(r, messages, queue)
    }
    pub(crate) fn resource_waiting(&self, index: usize) -> bool {
        self.slots.get(index).is_some_and(|s| s.ready.is_parked())
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    /// Occupancy follows observer ownership, independently of retained capacity.
    pub(crate) fn is_empty(&self) -> bool {
        self.live == 0
    }
    pub fn new(slots: &'s mut [Slot<'g, 'e>]) -> Result<Self, Error> {
        Self::from_storage(SlotStorage::borrowed(slots))
    }
    pub(crate) fn from_storage(slots: SlotStorage<'s, Slot<'g, 'e>>) -> Result<Self, Error> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            slots,
            live: 0,
            listener: None,
            ready: Waiting::EMPTY,
            prefer_event: true,
        })
    }
    pub(crate) fn has_ready<G: clients::Capabilities<'g>>(
        &self,
        graph: &G,
        exports: &exports::Exports<'e, '_, clients::Client<'g>>,
        messages: &messages::Messages<'_, '_, '_>,
        queue: &messages::Queue<'_, '_, impl Sized>,
    ) -> bool {
        self.ready.has_ready(
            &self.slots,
            |slot| &slot.ready,
            |r| Self::resource_ready(r, graph, exports, messages, queue),
        ) || self
            .listener
            .as_ref()
            .is_some_and(|listener| graph.has_events(listener))
    }
    pub(crate) fn next_ready<G: clients::Capabilities<'g>>(
        &mut self,
        graph: &mut G,
        exports: &exports::Exports<'e, '_, clients::Client<'g>>,
        messages: &messages::Messages<'_, '_, '_>,
        queue: &messages::Queue<'_, '_, impl Sized>,
    ) -> Option<usize> {
        self.prefer_event = !self.prefer_event;
        if !self.prefer_event {
            if let Some(index) = self.ready.pop(
                &mut self.slots,
                |slot| &mut slot.ready,
                |r| Self::resource_ready(r, graph, exports, messages, queue),
            ) {
                return Some(index);
            }
        }
        if let Some(event) = self
            .listener
            .as_ref()
            .and_then(|listener| graph.next_event(listener))
        {
            let index = event.owner;
            let entry = self.slots.get(index)?.entry.as_ref()?;
            if entry.target_subscription.key() == event.subscription
                || entry
                    .source_subscription
                    .as_ref()
                    .is_some_and(|sub| sub.key() == event.subscription)
            {
                self.ready
                    .remove(&mut self.slots, index, |slot| &mut slot.ready);
                return Some(index);
            }
        }
        self.ready.pop(
            &mut self.slots,
            |slot| &mut slot.ready,
            |r| Self::resource_ready(r, graph, exports, messages, queue),
        )
    }
    pub fn stage<G: clients::Capabilities<'g>>(
        &mut self,
        graph: &mut G,
        batch: &mut exports::Batch<'_, 'e, '_, clients::Client<'g>>,
        client: &clients::Client<'g>,
    ) -> Result<exports::Grant<'e>, Error> {
        self.stage_using(graph, batch, client, |_| None)
    }

    /// Stage with an optional privately authenticated hosted-owner identity.
    pub fn stage_using<G: clients::Capabilities<'g>>(
        &mut self,
        graph: &mut G,
        batch: &mut exports::Batch<'_, 'e, '_, clients::Client<'g>>,
        client: &clients::Client<'g>,
        identify: fn(&G::Owner) -> Option<exports::Identity>,
    ) -> Result<exports::Grant<'e>, Error> {
        let view = graph.inspect_for_export(client).map_err(Error::Graph)?;
        let identity = match &view.destination {
            clients::Destination::Endpoint { owner, .. } => identify(owner),
            clients::Destination::Waiting(_) => None,
        }
        .unwrap_or_else(|| view.key.into());
        let key = view.key;
        let pending = view.pending;
        let route = match view.destination {
            clients::Destination::Endpoint { key, .. } => Some(key),
            clients::Destination::Waiting(_) => None,
        };
        if let Some(grant) = batch.grant_canonical(identity).map_err(Error::Export)? {
            return Ok(grant);
        }
        let snapshot = graph.retain_key(key).map_err(Error::Graph)?;
        if !pending {
            return batch
                .export_canonical(identity, snapshot)
                .map_err(|rejected| {
                    graph.release(rejected.value);
                    Error::Export(rejected.error)
                });
        }
        let Some(index) = self
            .slots
            .vacant(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
        else {
            graph.release(snapshot);
            return Err(Error::Capacity);
        };
        // Like legacy's exported promise, retain its currently callable route.
        // Only resolution of this independent observer freezes the final export;
        // forwarding through its initial route does not authorize that resolution.
        let mut route = match route {
            Some(key) => match graph.retain_key(key) {
                Ok(route) => Some(route),
                Err(error) => {
                    graph.release(snapshot);
                    return Err(Error::Graph(error));
                }
            },
            None => None,
        };
        let promise = match graph.promise(&mut route) {
            Ok(client) => client,
            Err(error) => {
                if let Some(route) = route {
                    graph.release(route);
                }
                graph.release(snapshot);
                return Err(Error::Graph(error));
            }
        };
        let target = promise.key();
        let subscribed = (|| {
            if self.listener.is_none() {
                self.listener = Some(graph.listen()?);
            }
            let listener = self.listener.as_ref().unwrap();
            let target_subscription =
                graph.subscribe(listener, target, index, clients::Watch::Abandonment)?;
            match graph.subscribe(listener, snapshot.key(), index, clients::Watch::Changes) {
                Ok(source_subscription) => Ok((source_subscription, target_subscription)),
                Err(error) => {
                    graph.unsubscribe(target_subscription);
                    Err(error)
                }
            }
        })();
        let (source_subscription, target_subscription) = match subscribed {
            Ok(value) => value,
            Err(error) => {
                graph.release(snapshot);
                graph.release(promise);
                return Err(Error::Graph(error));
            }
        };
        let grant = match batch.export_canonical_promise(identity, snapshot.key(), promise) {
            Ok(grant) => grant,
            Err(rejected) => {
                graph.unsubscribe(source_subscription);
                graph.unsubscribe(target_subscription);
                graph.release(snapshot);
                graph.release(rejected.value);
                return Err(Error::Export(rejected.error));
            }
        };
        self.slots[index].entry = Some(Entry {
            source_subscription: Some(source_subscription),
            target_subscription,
            state: State::Pending(snapshot),
            target,
            export: grant.key(),
            id: grant.id(),
        });
        self.live += 1;
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
            |_| Ok(None),
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
        resources: R,
        attach: impl FnMut(&G::Owner, &mut R) -> u8,
        descriptors: &crate::publication::descriptors::Descriptors<G::Owner>,
        reflect: impl for<'a> Fn(
            &'a G::Owner,
        ) -> Result<
            Option<crate::calls::Target<'a>>,
            capntproto_rpc_engine::questions::Error,
        >,
    ) -> Result<Progress, Error>
    where
        G: clients::Capabilities<'g, Peer = P>,
        G::Owner: EndpointOwner<'i, P>,
    {
        if index >= self.slots.len() {
            return Err(Error::Capacity);
        }
        self.ready
            .remove(&mut self.slots, index, |slot| &mut slot.ready);
        let result = self.poll_one_inner(
            index,
            graph,
            exports,
            messages,
            queue,
            minimum,
            options,
            scratch,
            resources,
            attach,
            descriptors,
            reflect,
        );
        if let Err(error) = &result {
            if self
                .slots
                .get(index)
                .is_some_and(|slot| slot.entry.is_some())
            {
                let requirement = match error {
                    Error::Graph(error) => crate::resources::graph_wait(*error, graph),
                    Error::Export(exports::Error::Full)
                        if exports.available() < exports.maximum() =>
                    {
                        Some(Requirement {
                            resource: crate::resources::EXPORTS,
                            minimum: exports.available().saturating_add(1),
                        })
                    }
                    Error::Storage(error) | Error::Output(owned::Error::Storage(error)) => {
                        crate::resources::message_wait(*error, messages, queue)
                    }
                    _ => None,
                };
                if let Some(requirement) = requirement {
                    self.ready
                        .park(&mut self.slots, index, requirement, |slot| &mut slot.ready);
                } else {
                    self.ready
                        .wake(&mut self.slots, index, |slot| &mut slot.ready);
                }
            }
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn poll_one_inner<'i, 'pool, P: Copy + Eq, G, R>(
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
        reflect: impl for<'a> Fn(
            &'a G::Owner,
        ) -> Result<
            Option<crate::calls::Target<'a>>,
            capntproto_rpc_engine::questions::Error,
        >,
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
                    self.live -= 1;
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
                let rebound = graph
                    .rebind_subscription(entry.source_subscription.as_ref().unwrap(), frozen.key());
                graph.release(observed);
                if let Err(error) = rebound {
                    graph.release(frozen);
                    return Err(Error::Graph(error));
                }
                let State::Pending(previous) =
                    core::mem::replace(&mut entry.state, State::Pending(frozen))
                else {
                    unreachable!()
                };
                graph.release(previous);
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
            graph.unsubscribe(entry.source_subscription.take().unwrap());
        }
        if exports.lookup(entry.id) != Some(entry.export) {
            self.live -= 1;
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
        let reflectable = match graph.inspect(frozen).map_err(Error::Graph)? {
            clients::Destination::Endpoint { owner, .. } if exception.is_none() => reflect(owner)
                .map_err(|e| {
                    Error::Output(owned::Error::Output(
                        crate::publication::Error::Question(e).into(),
                    ))
                })?
                .is_some(),
            _ => false,
        };
        let attached = match graph.inspect(frozen).map_err(Error::Graph)? {
            clients::Destination::Endpoint { owner, .. } if exception.is_none() && !reflectable => {
                attach(owner, &mut resources)
            }
            _ => u8::MAX,
        };
        let output = queue.reserve_with(resources).map_err(Error::Storage)?;
        let mut batch = exports.batch();
        batch.resolve_promise(entry.export).map_err(Error::Export)?;
        let grant = if exception.is_none() && !reflectable {
            let identity = match graph.inspect(frozen).map_err(Error::Graph)? {
                clients::Destination::Endpoint { owner, .. } => (descriptors.identity)(owner),
                _ => None,
            }
            .unwrap_or_else(|| frozen.key().into());
            Some(
                match batch.grant_canonical(identity).map_err(Error::Export)? {
                    Some(grant) => grant,
                    None => {
                        let candidate = graph.retain_key(frozen.key()).map_err(Error::Graph)?;
                        batch
                            .export_canonical(identity, candidate)
                            .map_err(|rejected| {
                                graph.release(rejected.value);
                                Error::Export(rejected.error)
                            })?
                    }
                },
            )
        } else {
            None
        };
        let owner = match graph.inspect(frozen).map_err(Error::Graph)? {
            clients::Destination::Endpoint { owner, .. } => Some(owner),
            _ => None,
        };
        let reflected = if reflectable {
            reflect(owner.expect("reflected endpoint")).map_err(|e| {
                Error::Output(owned::Error::Output(
                    crate::publication::Error::Question(e).into(),
                ))
            })?
        } else {
            None
        };
        let extra = descriptors
            .bytes(owner, false)
            .map_err(|e| Error::Output(e.into()))?;
        let reflection_bytes = match reflected {
            Some(crate::calls::Target::Promised { path, .. }) => path
                .fields()
                .len()
                .checked_mul(8)
                .and_then(|n| n.checked_add(32))
                .ok_or(Error::Capacity)?,
            _ => 0,
        };
        let minimum = minimum
            .checked_add(extra)
            .and_then(|n| n.checked_add(reflection_bytes))
            .ok_or(Error::Capacity)?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            let mut root = builder.try_init_root::<Message>()?;
            let mut resolve = root.field(Message::RESOLVE).init()?;
            resolve.scalar(Resolve::PROMISE_ID).set(entry.id.to_wire());
            if let Some(target) = reflected {
                crate::owned::connection::reflection::write(
                    target,
                    resolve.field(Resolve::CAP).init()?,
                )?;
            } else if let Some(grant) = &grant {
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
            Ok(frame) => {
                owned::publish_granting(frame, batch, messages, scratch, output, |descriptor, _| {
                    crate::owned::connection::reflection::check(reflected, descriptor)
                })
            }
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
        self.live -= 1;
        release_entry(slot.entry.take().unwrap(), graph);
        Ok(Progress::Published)
    }
    pub fn drain_one<G: clients::Capabilities<'g>>(&mut self, graph: &mut G) -> bool {
        let Some(index) = self.slots.iter().position(|slot| slot.entry.is_some()) else {
            if let Some(listener) = self.listener.take() {
                graph.close_listener(listener);
            }
            return false;
        };
        self.ready
            .remove(&mut self.slots, index, |slot| &mut slot.ready);
        self.live -= 1;
        release_entry(self.slots[index].entry.take().unwrap(), graph);
        true
    }
}
fn release_entry<'g, G: clients::Capabilities<'g>>(entry: Entry<'g, '_>, graph: &mut G) {
    if let Some(subscription) = entry.source_subscription {
        graph.unsubscribe(subscription);
    }
    graph.unsubscribe(entry.target_subscription);
    let (State::Pending(client) | State::Ready(client)) = entry.state;
    graph.release(client);
}

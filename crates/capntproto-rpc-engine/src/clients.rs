//! Caller-backed capability ownership and promise routing. Internal edges own
//! references; public handles cannot cross a generative runtime scope. Frozen
//! descriptors retain the advertised endpoint instead of following later remote
//! promise resolutions (the Cap'n Proto Tribble four-way ordering rule).
use core::marker::PhantomData;
mod events;
#[cfg(feature = "alloc")]
pub use events::NotificationGrowth;
pub use events::{
    Event, EventKind, Listener, ListenerSlot, NotificationStorage, Subscription, SubscriptionKey,
    SubscriptionSlot, Watch,
};
mod access;
pub mod dispatch;
mod store;
pub use access::{Access, ExportSources};
pub use store::Capabilities;

type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Raw {
    pub(crate) index: usize,
    pub(crate) generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key<'id> {
    raw: Raw,
    brand: Brand<'id>,
}
impl Key<'_> {
    pub(crate) fn export_identity(self) -> crate::exports::Identity {
        crate::exports::Identity::new(self.raw.index, self.raw.generation)
    }
    pub(crate) fn raw(self) -> Raw {
        self.raw
    }
    pub(crate) fn from_raw(raw: Raw) -> Self {
        Self {
            raw,
            brand: PhantomData,
        }
    }
}
#[derive(Debug)]
#[must_use = "release through the graph or terminate its scope"]
pub struct Client<'id> {
    key: Key<'id>,
}
impl<'id> Client<'id> {
    pub fn key(&self) -> Key<'id> {
        self.key
    }
}

/// Currently reusable slots, including permitted growth but excluding exhausted generations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resources {
    pub nodes: usize,
    pub listeners: usize,
    pub subscriptions: usize,
    pub node_limit: usize,
    pub listener_limit: usize,
    pub subscription_limit: usize,
}

/// Connection identities belong to the caller's transport registry. A local or
/// broken endpoint cannot silently become a remote endpoint after publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location<P> {
    Local,
    Remote(P),
    Broken,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Full,
    AllocationFailed,
    ListenersFull,
    SubscriptionsFull,
    GenerationsExhausted,
    ReferencesExhausted,
    Stale,
    NotPromise,
    AlreadyResolved,
    AlreadyObserved,
    Cycle,
    NotEndpoint,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "capability graph: {self:?}")
    }
}
impl core::error::Error for Error {}
#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub value: T,
}

struct Entry<T, P> {
    refs: u32,
    kind: Kind<T, P>,
}
enum Kind<T, P> {
    Endpoint {
        owner: T,
        location: Location<P>,
    },
    Promise {
        target: Option<Raw>,
        resolved: bool,
        called: bool,
        prior: Option<Raw>,
    },
}
pub struct Slot<T, P> {
    generation: u64,
    free: crate::free_slots::Link,
    entry: Option<Entry<T, P>>,
    retired_next: Option<usize>,
    subscribers: Option<usize>,
    waiting: dispatch::List,
    dispatch_changed: bool,
    dispatch_previous: Option<usize>,
    dispatch_next: Option<usize>,
}
impl<T, P> Slot<T, P> {
    pub const EMPTY: Self = Self {
        generation: 0,
        free: crate::free_slots::Link::EMPTY,
        entry: None,
        retired_next: None,
        subscribers: None,
        waiting: dispatch::List::EMPTY,
        dispatch_changed: false,
        dispatch_previous: None,
        dispatch_next: None,
    };
}
pub struct Graph<'id, 'storage, T, P> {
    slots: crate::storage::Storage<'storage, Slot<T, P>>,
    unavailable: usize,
    free: crate::free_slots::FreeSlots,
    dispatch_sequence: u64,
    retired_head: Option<usize>,
    retired_tail: Option<usize>,
    dispatch_changes: dispatch::List,
    notifications: events::Notifications<'storage>,
    brand: Brand<'id>,
}

pub fn with_graph<'s, T, P: Copy + Eq, R>(
    slots: &'s mut [Slot<T, P>],
    notifications: NotificationStorage<'s>,
    f: impl for<'id> FnOnce(&mut Graph<'id, '_, T, P>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots, notifications)?))
}
pub async fn with_graph_async<'s, T, P: Copy + Eq, R>(
    slots: &'s mut [Slot<T, P>],
    notifications: NotificationStorage<'s>,
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Graph<'id, '_, T, P>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots, notifications)?).await)
}
pub(crate) fn new_scope<'id, 's, T, P>(
    slots: &'s mut [Slot<T, P>],
    notifications: NotificationStorage<'s>,
) -> Result<Graph<'id, 's, T, P>, Error> {
    if slots.iter().any(|s| s.entry.is_some()) {
        return Err(Error::NonemptyStorage);
    }
    Ok(Graph {
        unavailable: slots.iter().filter(|s| s.generation == u64::MAX).count(),
        slots: crate::storage::Storage::borrowed(slots),
        free: crate::free_slots::FreeSlots::new(),
        dispatch_sequence: 0,
        retired_head: None,
        retired_tail: None,
        dispatch_changes: dispatch::List::EMPTY,
        notifications: events::Notifications::new(notifications)?,
        brand: PhantomData,
    })
}

/// The same scoped capability graph with fallible allocator-backed growth.
/// `maximum` is an explicit admission limit; existing keys survive relocation.
/// Fixed graphs never fall back to this provider when they run out of slots.
#[cfg(feature = "alloc")]
pub fn with_growing_graph<T, P: Copy + Eq, R>(
    initial: usize,
    maximum: usize,
    notifications: NotificationGrowth,
    f: impl for<'id> FnOnce(&mut Graph<'id, '_, T, P>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(initial, maximum, notifications)?))
}
#[cfg(feature = "alloc")]
pub(crate) fn new_growing_scope<'id, 's, T, P>(
    initial: usize,
    maximum: usize,
    notifications: NotificationGrowth,
) -> Result<Graph<'id, 's, T, P>, Error> {
    let slots =
        crate::storage::Storage::growing(initial, maximum, || Slot::EMPTY).ok_or(Error::Full)?;
    Ok(Graph {
        unavailable: 0,
        slots,
        free: crate::free_slots::FreeSlots::new(),
        dispatch_sequence: 0,
        retired_head: None,
        retired_tail: None,
        dispatch_changes: dispatch::List::EMPTY,
        notifications: events::Notifications::growing(notifications)?,
        brand: PhantomData,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Destination<'id, 'a, T, P> {
    Endpoint {
        key: Key<'id>,
        owner: &'a T,
        location: Location<P>,
    },
    Waiting(Key<'id>),
}
/// An accepted call, including one queued behind a promise, marks every promise
/// on its path used. Build/queue the caller-owned call before accepting this
/// ticket; dropping it does not impose a spurious loopback requirement.
#[must_use]
pub struct Call<'a, 'id, 's, T, P> {
    graph: &'a mut Graph<'id, 's, T, P>,
    start: Raw,
    end: Raw,
    waiting: bool,
}
impl<'id, T, P: Copy + Eq> Call<'_, 'id, '_, T, P> {
    pub fn destination(&self) -> Destination<'id, '_, T, P> {
        self.graph.destination(self.end, self.waiting)
    }
    pub fn accept(self) -> Client<'id> {
        self.accept_releasing(None)
    }
    fn accept_releasing(self, previous: Option<Raw>) -> Client<'id> {
        self.graph.slots[self.end.index]
            .entry
            .as_mut()
            .unwrap()
            .refs += 1;
        let mut current = self.start;
        loop {
            let entry = self.graph.slots[current.index].entry.as_mut().unwrap();
            if let Kind::Promise { called, .. } = &mut entry.kind {
                *called = true;
            }
            if current == self.end {
                break;
            }
            let Kind::Promise {
                target: Some(next), ..
            } = entry.kind
            else {
                unreachable!()
            };
            current = next;
        }
        if let Some(previous) = previous {
            self.graph.release_raw(previous);
        }
        Client {
            key: self.graph.key(self.end),
        }
    }
}

/// Owns the blocked promise until its original-route echo is accepted. Pair this
/// token with the embargo ledger, which authenticates the expected wire ID.
/// Dropping it retains the reference until terminal teardown.
#[derive(Debug)]
#[must_use = "open only after the matching loopback echo, or terminate the scope"]
pub struct Barrier<'id> {
    client: Client<'id>,
}
impl<'id> Barrier<'id> {
    pub fn key(&self) -> Key<'id> {
        self.client.key
    }
}

/// `Reflected` must be preserved from receiverHosted/receiverAnswer, even when
/// the reflected capability already routes back to the original peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Direct,
    Reflected,
}
#[must_use]
pub struct Resolution<'a, 'id, 's, T, P> {
    graph: &'a mut Graph<'id, 's, T, P>,
    source: Raw,
    replacement: &'a mut Option<Client<'id>>,
    barrier: bool,
}
impl<'id, T, P: Copy + Eq> Resolution<'_, 'id, '_, T, P> {
    pub fn needs_barrier(&self) -> bool {
        self.barrier
    }
    pub fn accept(self) -> Option<Barrier<'id>> {
        let replacement = self.replacement.take().unwrap().key.raw;
        let entry = self.graph.slots[self.source.index].entry.as_mut().unwrap();
        let Kind::Promise {
            target,
            resolved,
            prior,
            ..
        } = &mut entry.kind
        else {
            unreachable!()
        };
        let old = target.replace(replacement);
        *resolved = true;
        if self.barrier {
            *prior = old;
            entry.refs += 1; // preflighted, and this borrow excludes other transitions
            self.graph.notify_changed(self.source.index);
            Some(Barrier {
                client: Client {
                    key: self.graph.key(self.source),
                },
            })
        } else {
            if let Some(old) = old {
                self.graph.release_raw(old);
            }
            self.graph.dispatch_changed(self.source.index);
            self.graph.notify_changed(self.source.index);
            None
        }
    }
}

impl<'id, 's, T, P: Copy + Eq> Graph<'id, 's, T, P> {
    pub fn resources(&self) -> Resources {
        Resources {
            nodes: self.slots.maximum() - self.unavailable,
            node_limit: self.slots.maximum(),
            listener_limit: self.notifications.listener_limit(),
            subscription_limit: self.notifications.subscription_limit(),
            listeners: self.notifications.listeners_available(),
            subscriptions: self.notifications.subscriptions_available(),
        }
    }

    fn key(&self, raw: Raw) -> Key<'id> {
        Key {
            raw,
            brand: PhantomData,
        }
    }
    fn live(&self, raw: Raw) -> Result<&Entry<T, P>, Error> {
        let slot = self.slots.get(raw.index).ok_or(Error::Stale)?;
        if slot.generation != raw.generation {
            return Err(Error::Stale);
        }
        slot.entry
            .as_ref()
            .filter(|e| e.refs != 0)
            .ok_or(Error::Stale)
    }
    fn vacant(&mut self) -> Result<usize, Error> {
        loop {
            if let Some(index) = self.free.peek(&self.slots, |slot| &slot.free) {
                if self.slots[index].generation != u64::MAX {
                    return Ok(index);
                }
                self.free.take(&mut self.slots, |slot| &mut slot.free);
            } else if !self.slots.grow(1, || Slot::EMPTY) {
                break;
            }
        }
        Err({
            if self.slots.iter().any(|s| s.entry.is_none()) {
                Error::GenerationsExhausted
            } else if self.slots.len() < self.slots.maximum() {
                Error::AllocationFailed
            } else {
                Error::Full
            }
        })
    }
    fn insert(&mut self, index: usize, kind: Kind<T, P>) -> Client<'id> {
        let reserved = self.free.take(&mut self.slots, |slot| &mut slot.free);
        debug_assert_eq!(reserved, index);
        let slot = &mut self.slots[index];
        slot.generation += 1;
        slot.entry = Some(Entry { refs: 1, kind });
        self.unavailable += 1;
        Client {
            key: Key {
                raw: Raw {
                    index,
                    generation: slot.generation,
                },
                brand: PhantomData,
            },
        }
    }
    // Only promises with parked dispatch owners enter this journal. Their
    // queued references pin the node generation until the runtime unlinks them.
    fn dispatch_changed(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        if slot.waiting.head.is_none() || slot.dispatch_changed {
            return;
        }
        slot.dispatch_changed = true;
        slot.dispatch_previous = self.dispatch_changes.tail;
        slot.dispatch_next = None;
        if let Some(tail) = self.dispatch_changes.tail {
            self.slots[tail].dispatch_next = Some(index);
        } else {
            self.dispatch_changes.head = Some(index);
        }
        self.dispatch_changes.tail = Some(index);
    }
    fn unlink_dispatch_change(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        if !slot.dispatch_changed {
            return;
        }
        slot.dispatch_changed = false;
        let previous = slot.dispatch_previous.take();
        let next = slot.dispatch_next.take();
        if let Some(previous) = previous {
            self.slots[previous].dispatch_next = next;
        } else {
            self.dispatch_changes.head = next;
        }
        if let Some(next) = next {
            self.slots[next].dispatch_previous = previous;
        } else {
            self.dispatch_changes.tail = previous;
        }
    }
    pub fn endpoint(
        &mut self,
        owner: T,
        location: Location<P>,
    ) -> Result<Client<'id>, Rejected<T>> {
        let index = match self.vacant() {
            Ok(i) => i,
            Err(error) => {
                return Err(Rejected {
                    error,
                    value: owner,
                })
            }
        };
        Ok(self.insert(index, Kind::Endpoint { owner, location }))
    }
    /// Create an owner only after storage has been reserved. A failed admission
    /// never invokes the factory, so resource-bearing owners need no rollback.
    pub fn endpoint_with(
        &mut self,
        location: Location<P>,
        owner: impl FnOnce() -> T,
    ) -> Result<Client<'id>, Error> {
        let index = self.vacant()?;
        Ok(self.insert(
            index,
            Kind::Endpoint {
                owner: owner(),
                location,
            },
        ))
    }
    /// Reserve both an imported promise and its immutable original route before
    /// consuming the owner. Capacity failure cannot leave a partial graph node.
    pub fn imported_promise(&mut self, owner: T, peer: P) -> Result<Client<'id>, Rejected<T>> {
        let vacant = self
            .slots
            .iter()
            .filter(|s| s.entry.is_none() && s.generation != u64::MAX)
            .take(2)
            .count();
        if vacant < 2 && !self.slots.grow(2 - vacant, || Slot::EMPTY) {
            return Err(Rejected {
                error: Error::Full,
                value: owner,
            });
        }
        let mut available = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.entry.is_none() && s.generation != u64::MAX)
            .map(|(i, _)| i);
        let (Some(route), Some(promise)) = (available.next(), available.next()) else {
            return Err(Rejected {
                error: Error::Full,
                value: owner,
            });
        };
        let route = self.insert(
            route,
            Kind::Endpoint {
                owner,
                location: Location::Remote(peer),
            },
        );
        Ok(self.insert(
            promise,
            Kind::Promise {
                target: Some(route.key.raw),
                resolved: false,
                called: false,
                prior: None,
            },
        ))
    }
    /// A local promise starts with no route. An imported/pipelined promise owns
    /// an immutable endpoint; the caller's retained endpoint handle can be used
    /// for the exact descriptor independently of this promise's later resolution.
    pub fn promise(&mut self, route: &mut Option<Client<'id>>) -> Result<Client<'id>, Error> {
        if let Some(route) = route {
            if !matches!(self.live(route.key.raw)?.kind, Kind::Endpoint { .. }) {
                return Err(Error::NotEndpoint);
            }
        }
        let index = self.vacant()?;
        Ok(self.insert(
            index,
            Kind::Promise {
                target: route.take().map(|c| c.key.raw),
                resolved: false,
                called: false,
                prior: None,
            },
        ))
    }
    pub fn retain(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.retain_key(client.key)
    }
    pub fn retain_key(&mut self, key: Key<'id>) -> Result<Client<'id>, Error> {
        let refs = self
            .live(key.raw)?
            .refs
            .checked_add(1)
            .ok_or(Error::ReferencesExhausted)?;
        self.slots[key.raw.index].entry.as_mut().unwrap().refs = refs;
        Ok(Client { key })
    }
    /// Freeze the exact route to advertise. A remote promise resolves no further
    /// after this snapshot: its endpoint has independent ownership. A local
    /// unresolved promise remains a promise, because it has no advertised route.
    pub fn snapshot(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.snapshot_for_export(client).map(|(client, _)| client)
    }
    /// Freeze the actual advertised route and derive whether it still promises
    /// a future resolution. A known remote route is an immutable hosted export;
    /// an unresolved local route remains a promise until its producer settles.
    pub fn snapshot_for_export(
        &mut self,
        client: &Client<'id>,
    ) -> Result<(Client<'id>, bool), Error> {
        let mut raw = client.key.raw;
        let pending = loop {
            match &self.live(raw)?.kind {
                Kind::Endpoint { .. } => break false,
                Kind::Promise { prior: Some(_), .. } | Kind::Promise { target: None, .. } => {
                    break true
                }
                Kind::Promise {
                    target: Some(next),
                    resolved,
                    ..
                } => {
                    // A caller-owned local proxy can accept calls before its
                    // promise settles. Export its resolution obligation, not
                    // merely the currently callable proxy. Remote endpoints
                    // still freeze the original route for reflected ordering.
                    if !resolved
                        && matches!(
                            self.live(*next)?.kind,
                            Kind::Endpoint {
                                location: Location::Local,
                                ..
                            }
                        )
                    {
                        break true;
                    }
                    raw = *next;
                }
            }
        };
        self.retain_key(self.key(raw))
            .map(|client| (client, pending))
    }
    fn walk(&self, mut raw: Raw) -> Result<(Raw, bool, &Entry<T, P>), Error> {
        loop {
            let entry = self.live(raw)?;
            match &entry.kind {
                Kind::Endpoint { .. } => return Ok((raw, false, entry)),
                Kind::Promise { prior: Some(_), .. } | Kind::Promise { target: None, .. } => {
                    return Ok((raw, true, entry))
                }
                Kind::Promise {
                    target: Some(next), ..
                } => raw = *next,
            }
        }
    }
    fn destination(&self, raw: Raw, waiting: bool) -> Destination<'id, '_, T, P> {
        if waiting {
            return Destination::Waiting(self.key(raw));
        }
        // A Call ticket exclusively borrows the graph from preflight through
        // acceptance. Its terminal entry cannot be released or recycled here.
        let slot = &self.slots[raw.index];
        debug_assert_eq!(slot.generation, raw.generation);
        let entry = slot.entry.as_ref().expect("validated call destination");
        debug_assert_ne!(entry.refs, 0);
        let Kind::Endpoint { owner, location } = &entry.kind else {
            unreachable!()
        };
        Destination::Endpoint {
            key: self.key(raw),
            owner,
            location: *location,
        }
    }
    /// Inspect routing without admitting a call or changing ordering state.
    pub fn inspect(&self, client: &Client<'id>) -> Result<Destination<'id, '_, T, P>, Error> {
        // Keep the validated entry from traversal. Looking it up a second time
        // repeats both storage selection and generation/refcount checks.
        let (raw, waiting, entry) = self.walk(client.key.raw)?;
        if waiting {
            return Ok(Destination::Waiting(self.key(raw)));
        }
        let Kind::Endpoint { owner, location } = &entry.kind else {
            unreachable!("walk reached endpoint")
        };
        Ok(Destination::Endpoint {
            key: self.key(raw),
            owner,
            location: *location,
        })
    }
    /// Readiness differs from routability: an imported promise can already
    /// accept pipelined calls while still owing resolution. A reflected promise
    /// remains unsettled until its original-route barrier opens.
    pub fn is_settled(&self, client: &Client<'id>) -> Result<bool, Error> {
        self.settled_endpoint(client).map(|end| end.is_some())
    }
    fn settled_endpoint(&self, client: &Client<'id>) -> Result<Option<Raw>, Error> {
        let mut raw = client.key.raw;
        loop {
            match &self.live(raw)?.kind {
                Kind::Endpoint { .. } => return Ok(Some(raw)),
                Kind::Promise {
                    resolved: true,
                    prior: None,
                    target: Some(next),
                    ..
                } => raw = *next,
                Kind::Promise { .. } => return Ok(None),
            }
        }
    }
    fn prepare_settled_call(
        &mut self,
        client: &Client<'id>,
    ) -> Result<Option<Call<'_, 'id, 's, T, P>>, Error> {
        let Some(end) = self.settled_endpoint(client)? else {
            return Ok(None);
        };
        if self.live(end)?.refs == u32::MAX {
            return Err(Error::ReferencesExhausted);
        }
        Ok(Some(Call {
            graph: self,
            start: client.key.raw,
            end,
            waiting: false,
        }))
    }
    pub fn prepare_call(&mut self, client: &Client<'id>) -> Result<Call<'_, 'id, 's, T, P>, Error> {
        let (end, waiting, entry) = self.walk(client.key.raw)?;
        if entry.refs == u32::MAX {
            return Err(Error::ReferencesExhausted);
        }
        Ok(Call {
            graph: self,
            start: client.key.raw,
            end,
            waiting,
        })
    }
    /// Resolve once, preserving the caller's replacement on all preflight errors
    /// and dropped offers. Reject cycles without touching references or routes.
    pub fn prepare_resolution<'a>(
        &'a mut self,
        key: Key<'id>,
        replacement: &'a mut Option<Client<'id>>,
        source: Source,
    ) -> Result<Resolution<'a, 'id, 's, T, P>, Error> {
        let entry = self.live(key.raw)?;
        let Kind::Promise {
            target,
            resolved,
            called,
            ..
        } = &entry.kind
        else {
            return Err(Error::NotPromise);
        };
        if *resolved {
            return Err(Error::AlreadyResolved);
        }
        let candidate = replacement.as_ref().ok_or(Error::Stale)?.key.raw;
        let mut next = candidate;
        loop {
            if next == key.raw {
                return Err(Error::Cycle);
            }
            match &self.live(next)?.kind {
                Kind::Endpoint { .. } | Kind::Promise { target: None, .. } => break,
                Kind::Promise {
                    target: Some(target),
                    ..
                } => next = *target,
            }
        }
        let old_location = target.and_then(|raw| match &self.live(raw).unwrap().kind {
            Kind::Endpoint { location, .. } => Some(*location),
            _ => None,
        });
        let (_, waiting, destination) = self.walk(candidate)?;
        let new_location = if waiting {
            None
        } else {
            match &destination.kind {
                Kind::Endpoint { location, .. } => Some(*location),
                _ => unreachable!(),
            }
        };
        let barrier = *called
            && matches!(old_location, Some(Location::Remote(_)))
            && new_location != Some(Location::Broken)
            && (source == Source::Reflected || old_location != new_location);
        if barrier && entry.refs == u32::MAX {
            return Err(Error::ReferencesExhausted);
        }
        Ok(Resolution {
            graph: self,
            source: key.raw,
            replacement,
            barrier,
        })
    }
    pub fn barrier_route(&self, barrier: &Barrier<'id>) -> &T {
        let Kind::Promise {
            prior: Some(prior), ..
        } = &self.live(barrier.key().raw).unwrap().kind
        else {
            unreachable!()
        };
        let Kind::Endpoint { owner, .. } = &self.live(*prior).unwrap().kind else {
            unreachable!()
        };
        owner
    }
    /// Call only after the associated embargo ledger accepts its matching echo.
    /// Existing queued calls must be dispatched in order by the caller's queue.
    pub fn open(&mut self, barrier: Barrier<'id>) {
        let entry = self.slots[barrier.key().raw.index].entry.as_mut().unwrap();
        let Kind::Promise { prior, .. } = &mut entry.kind else {
            unreachable!()
        };
        let prior = prior.take().expect("one affine barrier per resolution");
        self.release_raw(prior);
        self.dispatch_changed(barrier.key().raw.index);
        self.notify_changed(barrier.key().raw.index);
        self.release(barrier.client);
    }
    pub fn release(&mut self, client: Client<'id>) {
        self.release_raw(client.key.raw);
    }
    fn release_raw(&mut self, mut raw: Raw) {
        loop {
            debug_assert_eq!(self.slots[raw.index].generation, raw.generation);
            let entry = self.slots[raw.index]
                .entry
                .as_mut()
                .expect("owned graph reference");
            entry.refs -= 1;
            if entry.refs != 0 {
                return;
            }
            match &mut entry.kind {
                Kind::Endpoint { .. } => {
                    // Retired owners remain in their slot until explicitly
                    // detached, outside graph borrows. Link those slots in FIFO
                    // order so idle checks and retirement never scan live peers.
                    if let Some(tail) = self.retired_tail {
                        self.slots[tail].retired_next = Some(raw.index);
                    } else {
                        self.retired_head = Some(raw.index);
                    }
                    self.retired_tail = Some(raw.index);
                    self.notify_abandoned(raw.index);
                    return;
                }
                Kind::Promise { target, prior, .. } => {
                    debug_assert!(prior.is_none(), "barrier retains a local reference");
                    let next = target.take();
                    self.slots[raw.index].entry = None;
                    if self.slots[raw.index].generation != u64::MAX {
                        self.free
                            .release(raw.index, &mut self.slots, |slot| &mut slot.free);
                    }
                    self.unavailable -= usize::from(self.slots[raw.index].generation != u64::MAX);
                    self.notify_abandoned(raw.index);
                    if let Some(next) = next {
                        raw = next;
                    } else {
                        return;
                    }
                }
            }
        }
    }
    /// Detach exactly one retired endpoint. No application destructor runs in a
    /// graph transition; storage is not reusable until its owner is drained.
    pub fn take_retired(&mut self) -> Option<T> {
        let index = self.retired_head?;
        self.retired_head = self.slots[index].retired_next.take();
        if self.retired_head.is_none() {
            self.retired_tail = None;
        }
        let Kind::Endpoint { owner, .. } = self.slots[index].entry.take().unwrap().kind else {
            unreachable!()
        };
        self.unavailable -= usize::from(self.slots[index].generation != u64::MAX);
        if self.slots[index].generation != u64::MAX {
            self.free
                .release(index, &mut self.slots, |slot| &mut slot.free);
        }
        Some(owner)
    }
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.entry.is_none())
    }
    pub fn has_retired(&self) -> bool {
        self.retired_head.is_some()
    }
}
impl<T, P> Drop for Graph<'_, '_, T, P> {
    fn drop(&mut self) {
        for slot in &mut *self.slots {
            slot.entry.take();
            slot.retired_next = None;
            slot.subscribers = None;
            slot.waiting = dispatch::List::EMPTY;
            slot.dispatch_changed = false;
            slot.dispatch_previous = None;
            slot.dispatch_next = None;
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs;

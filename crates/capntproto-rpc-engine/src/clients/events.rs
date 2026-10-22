//! Caller-backed subscriptions to exact graph transitions. Each registry owns
//! its event queue; notification never invokes application code or allocates.
use super::*;
use crate::{
    storage::Storage,
    work_queue::{Link, Queue},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Changed,
    Abandoned,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    Changes,
    Abandonment,
}
#[must_use = "close the listener or terminate its graph scope"]
pub struct Listener<'id> {
    raw: Raw,
    brand: Brand<'id>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscriptionKey<'id> {
    raw: Raw,
    brand: Brand<'id>,
}
#[must_use = "unsubscribe or terminate its listener"]
pub struct Subscription<'id> {
    key: SubscriptionKey<'id>,
}
impl<'id> Subscription<'id> {
    pub fn key(&self) -> SubscriptionKey<'id> {
        self.key
    }
}
pub struct Event<'id> {
    pub subscription: SubscriptionKey<'id>,
    pub client: Key<'id>,
    pub owner: usize,
    pub kind: EventKind,
}
pub struct ListenerSlot {
    generation: u64,
    entry: Option<Registry>,
}
impl ListenerSlot {
    pub const EMPTY: Self = Self {
        generation: 0,
        entry: None,
    };
}
struct Registry {
    first: Option<usize>,
    ready: Queue,
}
pub struct SubscriptionSlot {
    generation: u64,
    entry: Option<Registration>,
    ready: Link,
}
impl SubscriptionSlot {
    pub const EMPTY: Self = Self {
        generation: 0,
        entry: None,
        ready: Link::EMPTY,
    };
}
struct Registration {
    client: Raw,
    listener: Raw,
    owner: usize,
    watch: Watch,
    kind: EventKind,
    attached: bool,
    previous: Option<usize>,
    next: Option<usize>,
    registry_previous: Option<usize>,
    registry_next: Option<usize>,
}
pub struct NotificationStorage<'s> {
    pub listeners: &'s mut [ListenerSlot],
    pub subscriptions: &'s mut [SubscriptionSlot],
}
impl NotificationStorage<'_> {
    /// A graph with no notification consumers needs no observer storage.
    pub fn empty() -> Self {
        Self {
            listeners: &mut [],
            subscriptions: &mut [],
        }
    }
}
#[cfg(feature = "alloc")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationGrowth {
    pub listeners: crate::connection::Growth,
    pub subscriptions: crate::connection::Growth,
}
#[cfg(feature = "alloc")]
impl NotificationGrowth {
    pub fn for_clients(clients: crate::connection::Growth) -> Self {
        Self::bounded(8, clients.maximum.saturating_mul(4))
    }
    pub fn bounded(listeners: usize, subscriptions: usize) -> Self {
        Self {
            listeners: crate::connection::Growth {
                initial: 0,
                maximum: listeners,
            },
            subscriptions: crate::connection::Growth {
                initial: 0,
                maximum: subscriptions,
            },
        }
    }
}
pub(super) struct Notifications<'s> {
    listeners_unavailable: usize,
    subscriptions_unavailable: usize,
    listeners: Storage<'s, ListenerSlot>,
    subscriptions: Storage<'s, SubscriptionSlot>,
}
impl<'s> Notifications<'s> {
    pub(super) fn listener_limit(&self) -> usize {
        self.listeners.maximum()
    }
    pub(super) fn subscription_limit(&self) -> usize {
        self.subscriptions.maximum()
    }
    pub(super) fn listeners_available(&self) -> usize {
        self.listeners.maximum() - self.listeners_unavailable
    }
    pub(super) fn subscriptions_available(&self) -> usize {
        self.subscriptions.maximum() - self.subscriptions_unavailable
    }
    pub(super) fn new(storage: NotificationStorage<'s>) -> Result<Self, Error> {
        if storage.listeners.iter().any(|s| s.entry.is_some())
            || storage.subscriptions.iter().any(|s| s.entry.is_some())
        {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            listeners_unavailable: storage
                .listeners
                .iter()
                .filter(|s| s.generation == u64::MAX)
                .count(),
            subscriptions_unavailable: storage
                .subscriptions
                .iter()
                .filter(|s| s.generation == u64::MAX)
                .count(),
            listeners: Storage::borrowed(storage.listeners),
            subscriptions: Storage::borrowed(storage.subscriptions),
        })
    }
    #[cfg(feature = "alloc")]
    pub(super) fn growing(growth: NotificationGrowth) -> Result<Self, Error> {
        Ok(Self {
            listeners_unavailable: 0,
            subscriptions_unavailable: 0,
            listeners: Storage::growing(growth.listeners.initial, growth.listeners.maximum, || {
                ListenerSlot::EMPTY
            })
            .ok_or(Error::Full)?,
            subscriptions: Storage::growing(
                growth.subscriptions.initial,
                growth.subscriptions.maximum,
                || SubscriptionSlot::EMPTY,
            )
            .ok_or(Error::Full)?,
        })
    }
    fn live_listener(&self, raw: Raw) -> Result<&Registry, Error> {
        self.listeners
            .get(raw.index)
            .filter(|s| s.generation == raw.generation)
            .and_then(|s| s.entry.as_ref())
            .ok_or(Error::Stale)
    }
    fn live_subscription(&self, raw: Raw) -> Result<&Registration, Error> {
        self.subscriptions
            .get(raw.index)
            .filter(|s| s.generation == raw.generation)
            .and_then(|s| s.entry.as_ref())
            .ok_or(Error::Stale)
    }
    pub(super) fn notify(&mut self, mut index: Option<usize>, kind: EventKind) {
        while let Some(current) = index {
            let entry = self.subscriptions[current].entry.as_mut().unwrap();
            index = entry.next;
            if kind == EventKind::Abandoned {
                entry.attached = false;
                entry.previous = None;
                entry.next = None;
            }
            if kind == EventKind::Abandoned || entry.watch == Watch::Changes {
                entry.kind = kind;
                let listener = entry.listener.index;
                self.listeners[listener].entry.as_mut().unwrap().ready.push(
                    &mut self.subscriptions,
                    current,
                    |slot| &mut slot.ready,
                );
            }
        }
    }
}
impl Drop for Notifications<'_> {
    fn drop(&mut self) {
        for slot in self.listeners.iter_mut() {
            slot.entry = None;
        }
        for slot in self.subscriptions.iter_mut() {
            slot.entry = None;
            slot.ready = Link::EMPTY;
        }
    }
}
impl<'id, T, P: Copy + Eq> Graph<'id, '_, T, P> {
    pub fn listen(&mut self) -> Result<Listener<'id>, Error> {
        let index = self
            .notifications
            .listeners
            .vacant(
                |_, slot| slot.entry.is_none() && slot.generation != u64::MAX,
                || ListenerSlot::EMPTY,
            )
            .ok_or(
                if self.notifications.listeners.len() < self.notifications.listeners.maximum() {
                    Error::AllocationFailed
                } else {
                    Error::ListenersFull
                },
            )?;
        self.notifications.listeners_unavailable += 1;
        let slot = &mut self.notifications.listeners[index];
        slot.generation += 1;
        slot.entry = Some(Registry {
            first: None,
            ready: Queue::EMPTY,
        });
        Ok(Listener {
            raw: Raw {
                index,
                generation: slot.generation,
            },
            brand: PhantomData,
        })
    }
    pub fn subscribe(
        &mut self,
        listener: &Listener<'id>,
        client: Key<'id>,
        owner: usize,
        watch: Watch,
    ) -> Result<Subscription<'id>, Error> {
        self.live(client.raw)?;
        self.notifications.live_listener(listener.raw)?;
        let index = self
            .notifications
            .subscriptions
            .vacant(
                |_, slot| slot.entry.is_none() && slot.generation != u64::MAX,
                || SubscriptionSlot::EMPTY,
            )
            .ok_or(
                if self.notifications.subscriptions.len()
                    < self.notifications.subscriptions.maximum()
                {
                    Error::AllocationFailed
                } else {
                    Error::SubscriptionsFull
                },
            )?;
        let next = self.slots[client.raw.index].subscribers;
        let registry_next = self.notifications.listeners[listener.raw.index]
            .entry
            .as_ref()
            .unwrap()
            .first;
        self.notifications.subscriptions_unavailable += 1;
        let slot = &mut self.notifications.subscriptions[index];
        slot.generation += 1;
        let key = SubscriptionKey {
            raw: Raw {
                index,
                generation: slot.generation,
            },
            brand: PhantomData,
        };
        slot.entry = Some(Registration {
            client: client.raw,
            listener: listener.raw,
            owner,
            watch,
            kind: EventKind::Changed,
            attached: true,
            previous: None,
            next,
            registry_previous: None,
            registry_next,
        });
        if let Some(next) = next {
            self.notifications.subscriptions[next]
                .entry
                .as_mut()
                .unwrap()
                .previous = Some(index);
        }
        self.slots[client.raw.index].subscribers = Some(index);
        if let Some(next) = registry_next {
            self.notifications.subscriptions[next]
                .entry
                .as_mut()
                .unwrap()
                .registry_previous = Some(index);
        }
        self.notifications.listeners[listener.raw.index]
            .entry
            .as_mut()
            .unwrap()
            .first = Some(index);
        Ok(Subscription { key })
    }
    /// Find an exclusive binding on this exact live node. Inspect only this
    /// node's subscribers; unrelated registries and retained capacity are idle.
    /// Multiple bindings from the listener are intentionally ambiguous.
    pub fn exclusive_subscription_owner(
        &self,
        listener: &Listener<'id>,
        client: Key<'id>,
    ) -> Option<usize> {
        self.live(client.raw).ok()?;
        self.notifications.live_listener(listener.raw).ok()?;
        let mut cursor = self.slots[client.raw.index].subscribers;
        let mut owner = None;
        while let Some(index) = cursor {
            let entry = self.notifications.subscriptions[index]
                .entry
                .as_ref()
                .unwrap();
            cursor = entry.next;
            if entry.listener == listener.raw {
                if owner.is_some() {
                    return None;
                }
                owner = Some(entry.owner);
            }
        }
        owner
    }
    pub fn has_events(&self, listener: &Listener<'id>) -> bool {
        self.notifications
            .live_listener(listener.raw)
            .is_ok_and(|registry| !registry.ready.is_empty())
    }
    pub fn next_event(&mut self, listener: &Listener<'id>) -> Option<Event<'id>> {
        self.notifications.live_listener(listener.raw).ok()?;
        let index = self.notifications.listeners[listener.raw.index]
            .entry
            .as_mut()
            .unwrap()
            .ready
            .pop(&mut self.notifications.subscriptions, |slot| {
                &mut slot.ready
            })?;
        let slot = &self.notifications.subscriptions[index];
        let entry = slot.entry.as_ref().unwrap();
        Some(Event {
            subscription: SubscriptionKey {
                raw: Raw {
                    index,
                    generation: slot.generation,
                },
                brand: PhantomData,
            },
            client: Key {
                raw: entry.client,
                brand: PhantomData,
            },
            owner: entry.owner,
            kind: entry.kind,
        })
    }
    fn detach_subscription(&mut self, index: usize) {
        let entry = self.notifications.subscriptions[index]
            .entry
            .as_mut()
            .unwrap();
        if !entry.attached {
            return;
        }
        entry.attached = false;
        let client = entry.client;
        let previous = entry.previous.take();
        let next = entry.next.take();
        if let Some(previous) = previous {
            self.notifications.subscriptions[previous]
                .entry
                .as_mut()
                .unwrap()
                .next = next;
        } else {
            debug_assert_eq!(self.slots[client.index].generation, client.generation);
            self.slots[client.index].subscribers = next;
        }
        if let Some(next) = next {
            self.notifications.subscriptions[next]
                .entry
                .as_mut()
                .unwrap()
                .previous = previous;
        }
    }
    pub fn unsubscribe(&mut self, subscription: Subscription<'id>) {
        let raw = subscription.key.raw;
        if self.notifications.live_subscription(raw).is_err() {
            return;
        }
        self.detach_subscription(raw.index);
        let entry = self.notifications.subscriptions[raw.index]
            .entry
            .take()
            .unwrap();
        self.notifications.subscriptions_unavailable -=
            usize::from(self.notifications.subscriptions[raw.index].generation != u64::MAX);
        let registry = self.notifications.listeners[entry.listener.index]
            .entry
            .as_mut()
            .unwrap();
        registry
            .ready
            .remove(&mut self.notifications.subscriptions, raw.index, |slot| {
                &mut slot.ready
            });
        if let Some(previous) = entry.registry_previous {
            self.notifications.subscriptions[previous]
                .entry
                .as_mut()
                .unwrap()
                .registry_next = entry.registry_next;
        } else {
            registry.first = entry.registry_next;
        }
        if let Some(next) = entry.registry_next {
            self.notifications.subscriptions[next]
                .entry
                .as_mut()
                .unwrap()
                .registry_previous = entry.registry_previous;
        }
    }
    pub fn rebind_subscription(
        &mut self,
        subscription: &Subscription<'id>,
        client: Key<'id>,
    ) -> Result<(), Error> {
        self.live(client.raw)?;
        let raw = subscription.key.raw;
        let entry = self.notifications.live_subscription(raw)?;
        if entry.client == client.raw && entry.attached {
            return Ok(());
        }
        self.detach_subscription(raw.index);
        let next = self.slots[client.raw.index].subscribers;
        let entry = self.notifications.subscriptions[raw.index]
            .entry
            .as_mut()
            .unwrap();
        entry.client = client.raw;
        entry.kind = EventKind::Changed;
        entry.attached = true;
        entry.next = next;
        let listener = entry.listener.index;
        self.notifications.listeners[listener]
            .entry
            .as_mut()
            .unwrap()
            .ready
            .remove(&mut self.notifications.subscriptions, raw.index, |slot| {
                &mut slot.ready
            });
        if let Some(next) = next {
            self.notifications.subscriptions[next]
                .entry
                .as_mut()
                .unwrap()
                .previous = Some(raw.index);
        }
        self.slots[client.raw.index].subscribers = Some(raw.index);
        Ok(())
    }
    pub fn close_listener(&mut self, listener: Listener<'id>) {
        if self.notifications.live_listener(listener.raw).is_err() {
            return;
        }
        while let Some(index) = self.notifications.listeners[listener.raw.index]
            .entry
            .as_ref()
            .unwrap()
            .first
        {
            let generation = self.notifications.subscriptions[index].generation;
            self.unsubscribe(Subscription {
                key: SubscriptionKey {
                    raw: Raw { index, generation },
                    brand: PhantomData,
                },
            });
        }
        self.notifications.listeners[listener.raw.index].entry = None;
        self.notifications.listeners_unavailable -=
            usize::from(self.notifications.listeners[listener.raw.index].generation != u64::MAX);
    }
    /// Subscribe to the first unresolved promise, then rebind after its event.
    /// This follows chains without notifying or traversing unrelated ancestors.
    pub fn pending_resolution(&self, client: &Client<'id>) -> Result<Option<Key<'id>>, Error> {
        let mut raw = client.key.raw;
        loop {
            match &self.live(raw)?.kind {
                Kind::Endpoint { .. } => return Ok(None),
                Kind::Promise {
                    resolved: true,
                    prior: None,
                    target: Some(next),
                    ..
                } => raw = *next,
                Kind::Promise { .. } => return Ok(Some(self.key(raw))),
            }
        }
    }
    pub(super) fn notify_changed(&mut self, index: usize) {
        self.notifications
            .notify(self.slots[index].subscribers, EventKind::Changed);
    }
    pub(super) fn notify_abandoned(&mut self, index: usize) {
        let first = self.slots[index].subscribers.take();
        self.notifications.notify(first, EventKind::Abandoned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registries_are_isolated_and_old_events_survive_node_reuse_without_authority() {
        let measured = allocation_counter::measure(|| {
            let mut nodes = [const { Slot::EMPTY }; 1];
            let mut listeners = [const { ListenerSlot::EMPTY }; 2];
            let mut subscriptions = [const { SubscriptionSlot::EMPTY }; 4];
            with_graph(
                &mut nodes,
                NotificationStorage {
                    listeners: &mut listeners,
                    subscriptions: &mut subscriptions,
                },
                |graph: &mut Graph<'_, '_, u32, u8>| {
                    let a = graph.listen().unwrap();
                    let b = graph.listen().unwrap();
                    let client = graph.endpoint(1, Location::Local).unwrap();
                    let old = client.key();
                    let first = graph.subscribe(&a, old, 1, Watch::Abandonment).unwrap();
                    let second = graph.subscribe(&b, old, 1, Watch::Abandonment).unwrap();
                    graph.release(client);
                    assert_eq!(graph.take_retired(), Some(1));
                    let reused = graph.endpoint(2, Location::Local).unwrap();
                    assert_ne!(old, reused.key());
                    let third = graph
                        .subscribe(&a, reused.key(), 2, Watch::Abandonment)
                        .unwrap();
                    let event = graph.next_event(&a).unwrap();
                    assert_eq!(event.subscription, first.key());
                    assert_eq!(event.client, old);
                    assert_eq!(event.kind, EventKind::Abandoned);
                    assert!(graph.next_event(&a).is_none());
                    assert!(graph.has_events(&b));
                    graph.unsubscribe(first);
                    graph.close_listener(b);
                    graph.unsubscribe(second); // Closing a registry invalidates its registrations.
                    graph.release(reused);
                    assert_eq!(graph.take_retired(), Some(2));
                    let event = graph.next_event(&a).unwrap();
                    assert_eq!(event.subscription, third.key());
                    assert_eq!(event.owner, 2);
                    graph.unsubscribe(third);
                    graph.close_listener(a);
                    assert!(graph.is_empty());
                },
            )
            .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
    #[test]
    fn subscriptions_follow_only_the_first_unresolved_promise() {
        let mut nodes = [const { Slot::EMPTY }; 3];
        let mut listeners = [const { ListenerSlot::EMPTY }; 1];
        let mut subscriptions = [const { SubscriptionSlot::EMPTY }; 1];
        with_graph(
            &mut nodes,
            NotificationStorage {
                listeners: &mut listeners,
                subscriptions: &mut subscriptions,
            },
            |graph: &mut Graph<'_, '_, u32, u8>| {
                let listener = graph.listen().unwrap();
                let first = graph.promise(&mut None).unwrap();
                let second = graph.promise(&mut None).unwrap();
                let observed = graph
                    .subscribe(&listener, first.key(), 17, Watch::Changes)
                    .unwrap();
                let mut next = Some(graph.retain(&second).unwrap());
                graph
                    .resolve(first.key(), &mut next, Source::Direct)
                    .unwrap();
                assert_eq!(graph.next_event(&listener).unwrap().client, first.key());
                let pending = graph.pending_resolution(&first).unwrap().unwrap();
                assert_eq!(pending, second.key());
                graph.rebind_subscription(&observed, pending).unwrap();
                let mut target = Some(graph.endpoint(41, Location::Local).unwrap());
                graph
                    .resolve(second.key(), &mut target, Source::Direct)
                    .unwrap();
                let event = graph.next_event(&listener).unwrap();
                assert_eq!(event.subscription, observed.key());
                assert_eq!(event.client, second.key());
                assert_eq!(event.kind, EventKind::Changed);
                assert_eq!(graph.pending_resolution(&first).unwrap(), None);
                graph.unsubscribe(observed);
                graph.close_listener(listener);
                graph.release(first);
                graph.release(second);
                assert_eq!(graph.take_retired(), Some(41));
                assert!(graph.is_empty());
            },
        )
        .unwrap();
    }
}

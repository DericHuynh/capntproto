//! Bounded local import ownership with caller-supplied storage.
//!
//! A scope is one connection's local handle domain. Handles cannot escape it or
//! cross into another scope. Copyable keys represent delayed work, not ownership;
//! they stop resolving when the last local reference is released. Re-admission
//! advances a generation without wrapping. Wire IDs remain unchanged.
//!
//! Release explicitly through the table: dropping/forgetting a local handle
//! retains its reference until scope teardown. Cleanup stays in the occupied
//! slot until an output adapter accepts its Release message. Ending a nonempty
//! scope is terminal teardown; the caller must also terminate that connection.
//! Ordinary transitions perform no IO, scheduling or application callbacks.
//! Borrowed storage never allocates; the optional growing provider reserves
//! capacity fallibly. Terminal scope teardown drops remaining application owners.
//!
//! ```
//! use capntproto_rpc_engine::{
//!     ids::{ImportId, WireId}, imports::{with_imports, Slot},
//! };
//! let mut storage = [const { Slot::EMPTY }; 2];
//! with_imports(&mut storage, |imports| {
//!     let local = imports.admit(ImportId::from_wire(7), 42).unwrap();
//!     let callback = local.key();
//!     assert_eq!(imports.get(&local), &42);
//!     imports.release(local);
//!     assert!(imports.resolve(callback).is_none());
//!     // Select the oldest actual obligation without inspecting unrelated slots.
//!     let release = imports.next_release().unwrap();
//!     assert_eq!(release.count(), 1);
//!     // Commit only after the adapter accepts the Release(id, count) message.
//!     let owner = release.commit();
//!     assert_eq!(owner, 42);
//!     assert!(imports.is_empty());
//! }).unwrap();
//! ```
use crate::{
    ids::{ImportId, WireId},
    promise,
    references::ImportRefs,
};
use core::{fmt, marker::PhantomData};
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("imports/verified.rs");
mod batch;
pub use batch::{Receive, ReceiveError, ReceiveSlot, Resolution};

// Invariance prevents shortening one domain's lifetime to match another domain.
type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Duplicate,
    Missing,
    Full,
    GenerationExhausted,
    LocalReferencesExhausted,
    RemoteReferencesExhausted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonemptyStorage => "import storage is not empty",
            Self::Duplicate => "import wire ID is already present",
            Self::Missing => "import wire ID is absent",
            Self::Full => "import storage is full",
            Self::GenerationExhausted => "import slot generation exhausted",
            Self::LocalReferencesExhausted => "local import reference count exhausted",
            Self::RemoteReferencesExhausted => "remote import reference count exhausted",
        })
    }
}
impl core::error::Error for Error {}

/// Failed admission preserves the rejected owner, for retry/drop outside table borrows.
#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub value: T,
}

#[cfg_attr(capntproto_verus, verus_verify)]
struct Entry<T> {
    #[cfg(feature = "alloc")]
    presence: crate::observation::Presence,
    wire: ImportId,
    value: T,
    local: u32,
    remote: ImportRefs,
    promise: promise::ImportState,
    // Weak graph slot/generation; reviving an import clears the prior link.
    client_binding: Option<(usize, u64)>,
}

/// Storage and cleanup capacity for one distinct import. Generations survive
/// removal and reuse. An exhausted vacant slot is permanently retired.
#[cfg_attr(capntproto_verus, verus_verify)]
pub struct Slot<T> {
    generation: u64,
    free: crate::free_slots::Link,
    release_previous: Option<core::num::NonZeroUsize>,
    release_next: Option<core::num::NonZeroUsize>,
    entry: Option<Entry<T>>,
}
impl<T> Slot<T> {
    pub const EMPTY: Self = Self {
        generation: 0,
        free: crate::free_slots::Link::EMPTY,
        release_previous: None,
        release_next: None,
        entry: None,
    };
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == match self.entry {
        Some(entry) => if self.generation == generation && entry.local != 0 { Some(&entry.value) } else { None },
        None => None,
    }))]
    fn resolve(&self, generation: u64) -> Option<&T> {
        let entry = self.entry.as_ref()?;
        if self.generation == generation && entry.local != 0 {
            Some(&entry.value)
        } else {
            None
        }
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires old(self).entry.is_some(),
        ensures received(*old(self), *final(self), r)
    ))]
    fn receive(&mut self) -> Result<bool, Error> {
        let entry = self.entry.as_mut().expect("located import");
        let local = entry
            .local
            .checked_add(1)
            .ok_or(Error::LocalReferencesExhausted)?;
        let revived = entry.local == 0;
        let generation = if revived {
            self.generation
                .checked_add(1)
                .ok_or(Error::GenerationExhausted)?
        } else {
            self.generation
        };
        if entry.remote.receive(1).is_err() {
            return Err(Error::RemoteReferencesExhausted);
        }
        if revived {
            entry.client_binding = None;
        }
        entry.local = local;
        self.generation = generation;
        Ok(revived)
    }
}

/// Non-owning token for delayed work. It is neither a wire ID nor a capability
/// reference. Resolve it again when the work runs; a saved pointer is not enough.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key<'id> {
    index: usize,
    generation: u64,
    brand: Brand<'id>,
}

/// One local reference. It can only be duplicated by a checked `retain` and is
/// consumed by `release`. Dropping it does not secretly enqueue cleanup.
#[derive(Debug)]
#[must_use = "release this reference through its import table or terminate the scope"]
pub struct Local<'id> {
    key: Key<'id>,
}
impl<'id> Local<'id> {
    pub const fn key(&self) -> Key<'id> {
        self.key
    }
}

pub struct Imports<'id, 'storage, T> {
    binding_revision: usize,
    #[cfg(feature = "alloc")]
    observation: crate::observation::Counter,
    slots: crate::storage::Storage<'storage, Slot<T>>,
    free: crate::free_slots::FreeSlots,
    displaced: usize,
    release_head: Option<core::num::NonZeroUsize>,
    release_tail: Option<core::num::NonZeroUsize>,
    brand: Brand<'id>,
}
impl<T> Imports<'_, '_, T> {
    fn enqueue_release(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        debug_assert!(slot.release_previous.is_none() && slot.release_next.is_none());
        slot.release_previous = self.release_tail;
        if let Some(tail) = self.release_tail {
            self.slots[tail.get() - 1].release_next = core::num::NonZeroUsize::new(index + 1);
        } else {
            self.release_head = core::num::NonZeroUsize::new(index + 1);
        }
        self.release_tail = core::num::NonZeroUsize::new(index + 1);
    }
    fn unlink_release(&mut self, index: usize) {
        unlink_release(
            &mut self.slots,
            index,
            &mut self.release_head,
            &mut self.release_tail,
        );
    }

    pub(crate) fn take_terminal_owner(&mut self, index: usize) -> Option<T> {
        let entry = self.slots.get_mut(index)?.entry.take()?;
        self.displaced -= usize::from(usize::try_from(entry.wire.to_wire()).ok() != Some(index));
        if entry.local == 0 {
            self.unlink_release(index);
        }
        if self.slots[index].generation != u64::MAX {
            self.free
                .release(index, &mut self.slots, |slot| &mut slot.free);
        }
        Some(entry.value)
    }
}

/// Introduce a fresh, invariant handle domain without a global counter, atomics
/// or caller-chosen epoch. Neither a table, handle nor key can escape `f`.
/// Owners returned by cleanup can escape. On normal return or unwinding, all
/// remaining entries are detached and dropped; this requires terminal IO when
/// references/cleanup remain. Owner destructors have their own requirements.
pub fn with_imports<'storage, T, R>(
    slots: &'storage mut [Slot<T>],
    f: impl for<'id> FnOnce(&mut Imports<'id, 'storage, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots)?))
}

/// Keep the same invariant domain across suspension. The caller pins/polls this
/// future; no executor or allocation is introduced. Dropping the future ends
/// the scope and requires terminal connection IO, just like synchronous teardown.
pub async fn with_imports_async<'storage, T, R>(
    slots: &'storage mut [Slot<T>],
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Imports<'id, 'storage, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots)?).await)
}

pub(crate) fn new_scope<'id, T>(slots: &mut [Slot<T>]) -> Result<Imports<'id, '_, T>, Error> {
    if slots.iter().any(|slot| slot.entry.is_some()) {
        return Err(Error::NonemptyStorage);
    }
    Ok(Imports {
        free: crate::free_slots::FreeSlots::new(),
        displaced: 0,
        release_head: None,
        release_tail: None,
        binding_revision: 0,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(slots),
        brand: PhantomData,
    })
}

#[cfg(feature = "alloc")]
pub(crate) fn new_growing_scope<'id, 's, T>(
    initial: usize,
    maximum: usize,
) -> Result<Imports<'id, 's, T>, Error> {
    let slots =
        crate::storage::Storage::growing(initial, maximum, || Slot::EMPTY).ok_or(Error::Full)?;
    Ok(Imports {
        free: crate::free_slots::FreeSlots::new(),
        displaced: 0,
        release_head: None,
        release_tail: None,
        binding_revision: 0,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots,
        brand: PhantomData,
    })
}
#[cfg(feature = "alloc")]
pub fn with_growing_imports<T, R>(
    initial: usize,
    maximum: usize,
    f: impl for<'id> FnOnce(&mut Imports<'id, '_, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(initial, maximum)?))
}

impl<'id, T> Imports<'id, '_, T> {
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    /// Wire identity of a retained local reference in this connection domain.
    pub fn wire_id(&self, local: &Local<'id>) -> ImportId {
        self.slots[local.key.index]
            .entry
            .as_ref()
            .expect("live local reference")
            .wire
    }
    /// The ledger, rather than the most recent descriptor kind, owns the
    /// outstanding Resolve obligation. Hosted aliases do not erase it.
    pub fn promise_state(&self, local: &Local<'id>) -> promise::ImportState {
        self.slots[local.key.index]
            .entry
            .as_ref()
            .expect("live local reference")
            .promise
    }
    /// Includes slots retaining pending Release messages, excluding retired slots.
    #[cfg(feature = "alloc")]
    pub fn observe_len(&mut self) -> crate::observation::Observation {
        self.observation.observe(
            self.slots
                .iter_mut()
                .filter_map(|s| s.entry.as_mut())
                .map(|r| &mut r.presence),
        )
    }
    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.entry.is_some())
            .count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Includes imports with no local observers which still owe a wire Release
    /// or Resolve. A descriptor for such an ID is an alias, not a new owner.
    pub fn contains(&self, id: ImportId) -> bool {
        self.index(id).is_some()
    }

    fn index(&self, id: ImportId) -> Option<usize> {
        // Port legacy's dense-ID lookup without reserving extra storage or
        // trusting a peer ID as a local slot/generation identity.
        if let Ok(index) = usize::try_from(id.to_wire()) {
            if self
                .slots
                .get(index)
                .is_some_and(|slot| slot.entry.as_ref().is_some_and(|entry| entry.wire == id))
            {
                return Some(index);
            }
        }
        if self.displaced == 0 {
            return None;
        }
        self.slots
            .iter()
            .position(|slot| slot.entry.as_ref().is_some_and(|entry| entry.wire == id))
    }
    fn local(&self, index: usize) -> Local<'id> {
        Local {
            key: Key {
                index,
                generation: self.slots[index].generation,
                brand: PhantomData,
            },
        }
    }

    /// Admit the first descriptor for a wire ID. Duplicates (including pending
    /// cleanup) are rejected without replacing owners; use `receive` for aliases.
    /// Lookup/admission are bounded linear scans, independent of the wire ID.
    pub fn admit(&mut self, id: ImportId, value: T) -> Result<Local<'id>, Rejected<T>> {
        if self.index(id).is_some() {
            return Err(Rejected {
                error: Error::Duplicate,
                value,
            });
        }
        let index = loop {
            if let Some(index) = self.free.peek(&self.slots, |slot| &slot.free) {
                if self.slots[index].generation != u64::MAX {
                    break Some(index);
                }
                self.free.take(&mut self.slots, |slot| &mut slot.free);
            } else if !self.slots.grow(1, || Slot::EMPTY) {
                break None;
            }
        };
        let Some(index) = index else {
            let error = if self.slots.iter().any(|slot| slot.entry.is_none()) {
                Error::GenerationExhausted
            } else {
                Error::Full
            };
            return Err(Rejected { error, value });
        };
        let reserved = self.free.take(&mut self.slots, |slot| &mut slot.free);
        debug_assert_eq!(reserved, index);
        self.displaced += usize::from(usize::try_from(id.to_wire()).ok() != Some(index));
        let slot = &mut self.slots[index];
        let mut remote = ImportRefs::new();
        remote.receive(1).expect("first remote reference");
        slot.generation += 1;
        slot.entry = Some(Entry {
            client_binding: None,
            #[cfg(feature = "alloc")]
            presence: self.observation.enter(),
            wire: id,
            value,
            local: 1,
            remote,
            promise: promise::ImportState::Hosted,
        });
        Ok(self.local(index))
    }

    /// Receive another descriptor for an existing wire ID, creating one local
    /// reference. A grant can revive pending cleanup, but never its stale keys.
    /// Any exhaustion leaves local/remote counts and generation unchanged.
    pub fn receive(&mut self, id: ImportId) -> Result<Local<'id>, Error> {
        let index = self.index(id).ok_or(Error::Missing)?;
        let slot = &mut self.slots[index];
        if slot.receive()? {
            self.binding_revision = self.binding_revision.saturating_add(1);
            self.unlink_release(index);
        }
        Ok(self.local(index))
    }

    /// Retain local ownership only; this does not manufacture a remote grant.
    pub fn retain(&mut self, reference: &Local<'id>) -> Result<Local<'id>, Error> {
        let entry = self.slots[reference.key.index]
            .entry
            .as_mut()
            .expect("live local reference");
        entry.local = entry
            .local
            .checked_add(1)
            .ok_or(Error::LocalReferencesExhausted)?;
        Ok(Local { key: reference.key })
    }

    pub fn get(&self, reference: &Local<'id>) -> &T {
        self.resolve(reference.key).expect("live local reference")
    }

    /// Mutate application metadata through a live scoped receipt. Raw wire IDs
    /// cannot authorize this access and ownership counters remain private.
    pub fn get_mut(&mut self, reference: &Local<'id>) -> &mut T {
        let slot = &mut self.slots[reference.key.index];
        assert_eq!(slot.generation, reference.key.generation);
        let entry = slot.entry.as_mut().expect("live local reference");
        assert_ne!(entry.local, 0);
        &mut entry.value
    }

    /// Release consumes the unique token. Last release reserves cleanup in the
    /// existing slot and invalidates non-owning keys, without running destructors.
    pub fn release(&mut self, reference: Local<'id>) {
        let entry = self.slots[reference.key.index]
            .entry
            .as_mut()
            .expect("live local reference");
        entry.local -= 1;
        if entry.local == 0 {
            self.binding_revision = self.binding_revision.saturating_add(1);
            self.enqueue_release(reference.key.index);
        }
    }

    pub fn resolve(&self, key: Key<'id>) -> Option<&T> {
        let slot = self.slots.get(key.index)?;
        slot.resolve(key.generation)
    }

    /// Inspect exactly one slot, so the caller controls the cleanup scan budget.
    /// Rejected output leaves the ticket/slot unchanged. Commit only after the
    /// adapter accepts ownership of the exact Release message (ID and count).
    pub fn pending_release(&mut self, index: usize) -> Option<Release<'_, T>> {
        let slot = self.slots.get_mut(index)?;
        if slot.entry.as_ref()?.local != 0 {
            return None;
        }
        Some(Release {
            binding_revision: &mut self.binding_revision,
            free: &mut self.free,
            displaced: &mut self.displaced,
            slots: &mut self.slots,
            index,
            head: &mut self.release_head,
            tail: &mut self.release_tail,
        })
    }
    /// Select only an actual obligation. Its slot remains owned until the
    /// transport accepts Release; revival unlinks it in constant time.
    pub fn next_release(&mut self) -> Option<Release<'_, T>> {
        self.pending_release(self.release_head?.get() - 1)
    }
    /// Changes only when committed ownership invalidates a weak binding key or
    /// retires its record. Speculative admission and live aliases leave it alone.
    pub fn binding_revision(&self) -> usize {
        self.binding_revision
    }
    pub fn pending_release_key(&self) -> Option<Key<'id>> {
        let index = self.release_head?.get() - 1;
        Some(Key {
            index,
            generation: self.slots[index].generation,
            brand: PhantomData,
        })
    }
    pub fn has_pending_release(&self) -> bool {
        self.release_head.is_some()
    }
}

impl<T> Drop for Imports<'_, '_, T> {
    fn drop(&mut self) {
        for slot in &mut *self.slots {
            // Clear the slot before dropping its owner. No implicit wire IO.
            let entry = slot.entry.take();
            slot.release_previous = None;
            slot.release_next = None;
            drop(entry);
        }
    }
}

fn unlink_release<T>(
    slots: &mut [Slot<T>],
    index: usize,
    head: &mut Option<core::num::NonZeroUsize>,
    tail: &mut Option<core::num::NonZeroUsize>,
) {
    let previous = slots[index].release_previous.take();
    let next = slots[index].release_next.take();
    if let Some(previous) = previous {
        slots[previous.get() - 1].release_next = next;
    } else {
        debug_assert_eq!(head.map(|link| link.get() - 1), Some(index));
        *head = next;
    }
    if let Some(next) = next {
        slots[next.get() - 1].release_previous = previous;
    } else {
        debug_assert_eq!(tail.map(|link| link.get() - 1), Some(index));
        *tail = previous;
    }
}

/// Exclusive cleanup effect. Dropping it retains the owner and every remote
/// reference; commit detaches the owner for disposal outside the engine borrow.
#[must_use = "commit only after output acceptance; dropping retains pending cleanup"]
pub struct Release<'a, T> {
    binding_revision: &'a mut usize,
    free: &'a mut crate::free_slots::FreeSlots,
    displaced: &'a mut usize,
    slots: &'a mut [Slot<T>],
    index: usize,
    head: &'a mut Option<core::num::NonZeroUsize>,
    tail: &'a mut Option<core::num::NonZeroUsize>,
}
impl<T> Release<'_, T> {
    pub fn id(&self) -> ImportId {
        self.slots[self.index]
            .entry
            .as_ref()
            .expect("pending cleanup")
            .wire
    }
    pub fn count(&self) -> u32 {
        self.slots[self.index]
            .entry
            .as_ref()
            .expect("pending cleanup")
            .remote
            .count()
    }
    pub fn commit(self) -> T {
        *self.binding_revision = self.binding_revision.saturating_add(1);
        unlink_release(self.slots, self.index, self.head, self.tail);
        let mut entry = self.slots[self.index]
            .entry
            .take()
            .expect("pending cleanup");
        *self.displaced -=
            usize::from(usize::try_from(entry.wire.to_wire()).ok() != Some(self.index));
        entry
            .remote
            .pending_release()
            .expect("nonzero remote references")
            .commit();
        if self.slots[self.index].generation != u64::MAX {
            self.free
                .release(self.index, self.slots, |slot| &mut slot.free);
        }
        entry.value
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod resolution_tests;
#[cfg(test)]
mod tests;

impl<'id, T> Imports<'id, '_, T> {
    /// The connection's non-owning graph link for this exact import generation.
    /// The returned key must still be checked by the graph; a retired node can
    /// be reused independently of its import. This creates no capability grant.
    pub fn client_binding(&self, key: Key<'id>) -> Option<crate::clients::Key<'id>> {
        let slot = self.slots.get(key.index)?;
        if slot.generation != key.generation {
            return None;
        }
        let (index, client_generation) = slot.entry.as_ref()?.client_binding?;
        Some(crate::clients::Key::from_raw(crate::clients::Raw {
            index,
            generation: client_generation,
        }))
    }
    /// Record a weak client link in the import itself, like legacy ImportClient.
    /// Both handles belong to this connection's affine ownership domain. The
    /// link does not keep a graph client, import, or resolved promise alive.
    /// ```compile_fail,E0521
    /// use capntproto_rpc_engine::{clients, imports, ids::{ImportId, WireId}};
    /// imports::with_imports(&mut [imports::Slot::EMPTY], |imports| {
    ///     let receipt = imports.admit(ImportId::from_wire(7), ()).unwrap();
    ///     clients::with_graph(&mut [clients::Slot::EMPTY], clients::NotificationStorage {
    ///         listeners: &mut [], subscriptions: &mut [],
    ///     }, |graph: &mut clients::Graph<'_, '_, u8, u8>| {
    ///         let client = graph.endpoint(1, clients::Location::Local).unwrap();
    ///         imports.bind_client(&receipt, &client); // distinct affine domains
    ///     }).unwrap();
    /// }).unwrap();
    /// ```
    pub fn bind_client(&mut self, local: &Local<'id>, client: &crate::clients::Client<'id>) {
        let slot = &mut self.slots[local.key.index];
        debug_assert_eq!(slot.generation, local.key.generation);
        debug_assert!(slot.entry.as_ref().is_some_and(|entry| entry.local != 0));
        let raw = client.key().raw();
        slot.entry.as_mut().expect("live import").client_binding =
            Some((raw.index, raw.generation));
    }
}

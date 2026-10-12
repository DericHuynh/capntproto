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
//!     // The caller can inspect one slot per poll to bound cleanup work.
//!     let release = imports.pending_release(0).unwrap();
//!     assert_eq!(release.count(), 1);
//!     // Commit only after the adapter accepts the Release(id, count) message.
//!     let owner = release.commit();
//!     assert_eq!(owner, 42);
//!     assert!(imports.is_empty());
//! }).unwrap();
//! ```
use crate::{ids::ImportId, promise, references::ImportRefs};
use core::{fmt, marker::PhantomData};
mod batch;
pub use batch::{Receive, ReceiveError, ReceiveSlot, Resolution};

// Invariance prevents shortening one domain's lifetime to match another domain.
type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;

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

struct Entry<T> {
    #[cfg(feature = "alloc")]
    presence: crate::observation::Presence,
    wire: ImportId,
    value: T,
    local: u32,
    remote: ImportRefs,
    promise: promise::ImportState,
}

/// Storage and cleanup capacity for one distinct import. Generations survive
/// removal and reuse. An exhausted vacant slot is permanently retired.
pub struct Slot<T> {
    generation: u64,
    entry: Option<Entry<T>>,
}
impl<T> Slot<T> {
    pub const EMPTY: Self = Self {
        generation: 0,
        entry: None,
    };
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
    #[cfg(feature = "alloc")]
    observation: crate::observation::Counter,
    slots: crate::storage::Storage<'storage, Slot<T>>,
    pending_releases: usize,
    brand: Brand<'id>,
}
impl<T> Imports<'_, '_, T> {
    pub(crate) fn take_terminal_owner(&mut self, index: usize) -> Option<T> {
        let entry = self.slots.get_mut(index)?.entry.take()?;
        if entry.local == 0 {
            self.pending_releases -= 1;
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
        pending_releases: 0,
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
        pending_releases: 0,
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
        let Some(index) = self.slots.vacant(
            |_, slot| slot.entry.is_none() && slot.generation != u64::MAX,
            || Slot::EMPTY,
        ) else {
            let error = if self.slots.iter().any(|slot| slot.entry.is_none()) {
                Error::GenerationExhausted
            } else {
                Error::Full
            };
            return Err(Rejected { error, value });
        };
        let slot = &mut self.slots[index];
        let mut remote = ImportRefs::new();
        remote.receive(1).expect("first remote reference");
        slot.generation += 1;
        slot.entry = Some(Entry {
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
        let entry = slot.entry.as_mut().expect("located import");
        let local = entry
            .local
            .checked_add(1)
            .ok_or(Error::LocalReferencesExhausted)?;
        let generation = if entry.local == 0 {
            slot.generation
                .checked_add(1)
                .ok_or(Error::GenerationExhausted)?
        } else {
            slot.generation
        };
        entry
            .remote
            .receive(1)
            .map_err(|_| Error::RemoteReferencesExhausted)?;
        if entry.local == 0 {
            self.pending_releases -= 1;
        }
        entry.local = local;
        slot.generation = generation;
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
            self.pending_releases += 1;
        }
    }

    pub fn resolve(&self, key: Key<'id>) -> Option<&T> {
        let slot = self.slots.get(key.index)?;
        let entry = slot.entry.as_ref()?;
        (slot.generation == key.generation && entry.local != 0).then_some(&entry.value)
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
            slot,
            pending: &mut self.pending_releases,
        })
    }
    pub fn has_pending_release(&self) -> bool {
        self.pending_releases != 0
    }
}

impl<T> Drop for Imports<'_, '_, T> {
    fn drop(&mut self) {
        for slot in &mut *self.slots {
            // Clear the slot before dropping its owner. No implicit wire IO.
            let entry = slot.entry.take();
            drop(entry);
        }
    }
}

/// Exclusive cleanup effect. Dropping it retains the owner and every remote
/// reference; commit detaches the owner for disposal outside the engine borrow.
#[must_use = "commit only after output acceptance; dropping retains pending cleanup"]
pub struct Release<'a, T> {
    slot: &'a mut Slot<T>,
    pending: &'a mut usize,
}
impl<T> Release<'_, T> {
    pub fn id(&self) -> ImportId {
        self.slot.entry.as_ref().expect("pending cleanup").wire
    }
    pub fn count(&self) -> u32 {
        self.slot
            .entry
            .as_ref()
            .expect("pending cleanup")
            .remote
            .count()
    }
    pub fn commit(self) -> T {
        let mut entry = self.slot.entry.take().expect("pending cleanup");
        *self.pending -= 1;
        entry
            .remote
            .pending_release()
            .expect("nonzero remote references")
            .commit();
        entry.value
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod resolution_tests;
#[cfg(test)]
mod tests;

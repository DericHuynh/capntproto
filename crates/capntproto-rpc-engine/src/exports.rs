//! Bounded export ownership, local call leases and transactional wire grants.
//!
//! One scoped table owns one connection's export domain. Keys are non-owning,
//! generation-checked local tokens; wire IDs contain no generation. The caller
//! must still enforce protocol ordering before wire-ID reuse.
//!
//! A batch reserves descriptor slots. Commit only when the output
//! adapter accepts the message containing every reserved descriptor. Dropping a
//! batch restores live counts and retires unpublished owners for explicit draining.
//! No ordinary transition runs owner destructors or performs IO. Borrowed
//! storage never allocates; optional growing storage reserves capacity fallibly.
//! Scope teardown drops remaining owners and requires terminal connection IO.
//!
//! ```
//! use capntproto_rpc_engine::exports::{with_exports, Slot};
//! let mut slots = [const { Slot::EMPTY }; 2];
//! with_exports(&mut slots, |exports| {
//!     let mut batch = exports.batch();
//!     let descriptor = batch.export(42).unwrap();
//!     let id = descriptor.id();
//!     // Serialize this descriptor; commit after the adapter accepts the message.
//!     batch.commit();
//!     let call = exports.lease(id).unwrap();
//!     assert_eq!(exports.release(id, 1), Ok(None));
//!     assert!(exports.lookup(id).is_none());
//!     assert_eq!(exports.get(&call), &42);
//!     assert_eq!(exports.release_local(call), Some(42));
//! }).unwrap();
//! ```
use crate::{
    ids::{ExportId, WireId},
    promise,
    references::ExportRefs,
};
use core::{fmt, marker::PhantomData};

mod descriptors;
pub use descriptors::{DescriptorCheck, DescriptorError};
mod leases;
mod release;
pub use leases::{LeaseError, LeaseSlot, Leases};
pub use release::{LeasedReleases, ReleaseBatch, ReleaseError, Releases};

type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;
const ID_LIMIT: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    CapacityTooLarge,
    Full,
    GenerationExhausted,
    StaleKey,
    Missing,
    RemoteReferencesExhausted,
    LocalReferencesExhausted,
    OverRelease,
    Promise(promise::Error),
    ResolutionConflict,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonemptyStorage => "export storage is not empty",
            Self::CapacityTooLarge => "export capacity exceeds the local wire-ID range",
            Self::Full => "export storage is full",
            Self::GenerationExhausted => "export slot generation exhausted",
            Self::StaleKey => "export key is no longer live",
            Self::Missing => "export wire ID is not live",
            Self::RemoteReferencesExhausted => "remote export reference count exhausted",
            Self::LocalReferencesExhausted => "local export lease count exhausted",
            Self::OverRelease => "release exceeds published export references",
            Self::Promise(_) => "export promise is not pending resolution",
            Self::ResolutionConflict => {
                "one Resolve cannot grant its own promise or resolve multiple promises"
            }
        })
    }
}
impl core::error::Error for Error {}

#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub value: T,
}

enum State {
    Staged,
    Live(ExportRefs),
    Retired,
}
struct Entry<T> {
    #[cfg(feature = "alloc")]
    presence: crate::observation::Presence,
    value: T,
    state: State,
    local: u32,
    pending: u32,
    promise: promise::ImportState,
}

/// A value, generation and in-place publication/cleanup bookkeeping. Exhausted
/// vacant generations are retired permanently; storage must be empty on entry.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key<'id> {
    index: usize,
    generation: u64,
    brand: Brand<'id>,
}

/// A reserved descriptor, not proof of transmission. Use its ID exactly once
/// in the batch's message. Generated message publication remains caller-owned.
#[derive(Debug)]
#[must_use = "serialize this grant in the batch's message or abort the batch"]
pub struct Grant<'id> {
    key: Key<'id>,
    promise: bool,
}
impl<'id> Grant<'id> {
    pub const fn key(&self) -> Key<'id> {
        self.key
    }
    pub fn id(&self) -> ExportId {
        ExportId::from_wire(self.key.index as u32)
    }
    pub fn is_promise(&self) -> bool {
        self.promise
    }
}

/// One local call's ownership. Peer Release can invalidate the wire ID while
/// this lease keeps the owner alive. Explicitly release or end the scope;
/// dropping/forgetting a lease retains its count until terminal teardown.
#[derive(Debug)]
#[must_use = "release this lease through its export table or terminate the scope"]
pub struct Lease<'id> {
    key: Key<'id>,
}

pub struct Exports<'id, 'storage, T> {
    #[cfg(feature = "alloc")]
    observation: crate::observation::Counter,
    slots: crate::storage::Storage<'storage, Slot<T>>,
    open_batch: bool,
    pending_resolution: Option<Key<'id>>,
    brand: Brand<'id>,
}

/// Introduce an invariant connection domain. Keys and leases cannot escape or
/// cross scopes. Detached owners can escape. Teardown also runs during unwind;
/// owner destructors and IO adapters retain their own allocation requirements.
pub fn with_exports<'storage, T, R>(
    slots: &'storage mut [Slot<T>],
    f: impl for<'id> FnOnce(&mut Exports<'id, 'storage, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots)?))
}

/// Asynchronous scope with caller-selected pinning, polling and storage. Tokens
/// remain connection-local across await; dropping the future is terminal teardown.
pub async fn with_exports_async<'storage, T, R>(
    slots: &'storage mut [Slot<T>],
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Exports<'id, 'storage, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots)?).await)
}

pub(crate) fn new_scope<'id, T>(slots: &mut [Slot<T>]) -> Result<Exports<'id, '_, T>, Error> {
    if slots.len() as u128 > u128::from(ID_LIMIT) {
        return Err(Error::CapacityTooLarge);
    }
    if slots.iter().any(|slot| slot.entry.is_some()) {
        return Err(Error::NonemptyStorage);
    }
    Ok(Exports {
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(slots),
        open_batch: false,
        pending_resolution: None,
        brand: PhantomData,
    })
}

#[cfg(feature = "alloc")]
pub(crate) fn new_growing_scope<'id, 's, T>(
    initial: usize,
    maximum: usize,
) -> Result<Exports<'id, 's, T>, Error> {
    if maximum as u64 > ID_LIMIT {
        return Err(Error::CapacityTooLarge);
    }
    let slots =
        crate::storage::Storage::growing(initial, maximum, || Slot::EMPTY).ok_or(Error::Full)?;
    Ok(Exports {
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots,
        open_batch: false,
        pending_resolution: None,
        brand: PhantomData,
    })
}
#[cfg(feature = "alloc")]
pub fn with_growing_exports<T, R>(
    initial: usize,
    maximum: usize,
    f: impl for<'id> FnOnce(&mut Exports<'id, '_, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(initial, maximum)?))
}

impl<'id, 'storage, T> Exports<'id, 'storage, T> {
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    /// Includes unpublished/retired owners awaiting explicit draining.
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
        self.slots.iter().filter(|s| s.entry.is_some()).count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn key(&self, index: usize) -> Key<'id> {
        Key {
            index,
            generation: self.slots[index].generation,
            brand: PhantomData,
        }
    }
    fn live_index(&self, id: ExportId) -> Option<usize> {
        let index = usize::try_from(id.to_wire()).ok()?;
        matches!(self.slots.get(index)?.entry.as_ref()?.state, State::Live(_)).then_some(index)
    }
    /// Authorize the wire target and create a non-owning delayed-work key.
    /// Pin with `lease` before retaining the owner for a pending call.
    pub fn lookup(&self, id: ExportId) -> Option<Key<'id>> {
        Some(self.key(self.live_index(id)?))
    }
    pub fn resolve(&self, key: Key<'id>) -> Option<&T> {
        let slot = self.slots.get(key.index)?;
        let entry = slot.entry.as_ref()?;
        (slot.generation == key.generation && matches!(entry.state, State::Live(_)))
            .then_some(&entry.value)
    }

    /// The table is exclusively borrowed until this batch commits or aborts.
    /// If a previous batch was forgotten, recover its staged state first.
    pub fn batch(&mut self) -> Batch<'_, 'id, 'storage, T> {
        self.abort_pending();
        self.open_batch = true;
        Batch { exports: self }
    }

    fn abort_pending(&mut self) {
        if !self.open_batch {
            return;
        }
        for slot in &mut *self.slots {
            if let Some(entry) = &mut slot.entry {
                if matches!(entry.state, State::Staged) {
                    entry.state = State::Retired;
                }
                entry.pending = 0;
            }
        }
        self.open_batch = false;
        self.pending_resolution = None;
    }

    /// Acquire a call lease only from a published export. No remote grant is added.
    pub fn lease(&mut self, id: ExportId) -> Result<Lease<'id>, Error> {
        Ok(self.prepare_lease(id)?.accept())
    }
    /// Check capacity without acquiring the reference. The exclusive ticket can
    /// be combined with other ledgers' preflights before committing an input.
    pub fn prepare_lease(
        &mut self,
        id: ExportId,
    ) -> Result<LeaseReservation<'_, 'id, 'storage, T>, Error> {
        self.abort_pending();
        let index = self.live_index(id).ok_or(Error::Missing)?;
        let entry = self.slots[index].entry.as_ref().expect("live export");
        entry
            .local
            .checked_add(1)
            .ok_or(Error::LocalReferencesExhausted)?;
        let key = self.key(index);
        Ok(LeaseReservation { exports: self, key })
    }
    pub fn retain(&mut self, lease: &Lease<'id>) -> Result<Lease<'id>, Error> {
        self.abort_pending();
        let entry = self.slots[lease.key.index]
            .entry
            .as_mut()
            .expect("live lease");
        entry.local = entry
            .local
            .checked_add(1)
            .ok_or(Error::LocalReferencesExhausted)?;
        Ok(Lease { key: lease.key })
    }
    pub fn get(&self, lease: &Lease<'id>) -> &T {
        &self.slots[lease.key.index]
            .entry
            .as_ref()
            .expect("live lease")
            .value
    }
    /// Consume a lease and return the owner if the peer already released it and
    /// no local call remains. Run its destructor outside other runtime borrows.
    pub fn release_local(&mut self, lease: Lease<'id>) -> Option<T> {
        let index = lease.key.index;
        self.release_local_deferred(lease);
        self.take_retired(index)
    }
    /// Drop local ownership while retaining retired owners for explicit draining.
    /// This permits a mixed descriptor batch to release without any destructor.
    pub fn release_local_deferred(&mut self, lease: Lease<'id>) {
        self.abort_pending();
        self.slots[lease.key.index]
            .entry
            .as_mut()
            .expect("live lease")
            .local -= 1;
    }
    /// Apply a peer Release only to published grants. Underflow is atomic.
    /// Final peer release invalidates keys/wire lookup immediately; call leases
    /// can retain the owner until their own completion/cancellation.
    pub fn release(&mut self, id: ExportId, count: u32) -> Result<Option<T>, Error> {
        self.abort_pending();
        let index = self.live_index(id).ok_or(Error::Missing)?;
        let entry = self.slots[index].entry.as_mut().expect("live export");
        let State::Live(refs) = &mut entry.state else {
            unreachable!()
        };
        if refs.release(count).map_err(|_| Error::OverRelease)? {
            entry.state = State::Retired;
        }
        Ok(self.take_retired(index))
    }
    /// Drain at most one unpublished/retired owner, including a forgotten batch.
    /// Reclamation is impossible while a local call lease still owns the value.
    pub fn take_retired(&mut self, index: usize) -> Option<T> {
        self.abort_pending();
        let slot = self.slots.get_mut(index)?;
        let entry = slot.entry.as_ref()?;
        if entry.local != 0 || !matches!(entry.state, State::Retired) {
            return None;
        }
        Some(slot.entry.take().expect("retired export").value)
    }
}

impl<T> Drop for Exports<'_, '_, T> {
    fn drop(&mut self) {
        for slot in &mut *self.slots {
            let entry = slot.entry.take();
            drop(entry);
        }
    }
}
impl<T> Exports<'_, '_, T> {
    pub(crate) fn take_terminal_owner(&mut self, index: usize) -> Option<T> {
        self.slots
            .get_mut(index)?
            .entry
            .take()
            .map(|entry| entry.value)
    }
}

/// All grants are private to one unsent message. Per-slot pending counters make
/// repeated IDs atomic without a separate journal or callbacks during rollback.
/// This is not a message lease; the adapter must bind acceptance to that message.
#[must_use = "commit after output accepts the complete batch; dropping aborts all grants"]
pub struct Batch<'a, 'id, 'storage, T> {
    exports: &'a mut Exports<'id, 'storage, T>,
}
impl<'id, T> Batch<'_, 'id, '_, T> {
    /// Inspect an owner reserved by this batch without publishing its grant.
    /// Exact descriptor multiplicity/kind must still pass `descriptors()`.
    pub fn pending_owner(&self, id: ExportId) -> Option<&T> {
        let index = usize::try_from(id.to_wire()).ok()?;
        self.exports
            .slots
            .get(index)?
            .entry
            .as_ref()
            .filter(|e| e.pending != 0)
            .map(|e| &e.value)
    }
    /// Reserve the first descriptor and take ownership. Allocation uses the
    /// lowest available wire ID, excluding retired owners/generations.
    pub fn export(&mut self, value: T) -> Result<Grant<'id>, Rejected<T>> {
        let Some(index) = self.exports.slots.vacant(
            |_, s| s.entry.is_none() && s.generation != u64::MAX,
            || Slot::EMPTY,
        ) else {
            let error = if self.exports.slots.iter().any(|s| s.entry.is_none()) {
                Error::GenerationExhausted
            } else {
                Error::Full
            };
            return Err(Rejected { error, value });
        };
        let slot = &mut self.exports.slots[index];
        slot.generation += 1;
        slot.entry = Some(Entry {
            #[cfg(feature = "alloc")]
            presence: self.exports.observation.enter(),
            value,
            state: State::Staged,
            local: 0,
            pending: 1,
            promise: promise::ImportState::Hosted,
        });
        Ok(Grant {
            key: self.exports.key(index),
            promise: false,
        })
    }
    /// Publish a promise whose one Resolve remains an explicit engine obligation.
    pub fn export_promise(&mut self, value: T) -> Result<Grant<'id>, Rejected<T>> {
        let mut grant = self.export(value)?;
        self.exports.slots[grant.key.index]
            .entry
            .as_mut()
            .unwrap()
            .promise = promise::ImportState::Pending;
        grant.promise = true;
        Ok(grant)
    }
    /// Stage one Resolve without adding a reference to the original promise.
    /// The adapter must bind this batch to that exact Resolve frame. Applications
    /// keep forwarding the old export through its exact one-hop replacement;
    /// resolving an inner promise must not bypass that ordering path.
    pub fn resolve_promise(&mut self, key: Key<'id>) -> Result<(), Error> {
        if self.exports.pending_resolution.is_some() {
            return Err(Error::ResolutionConflict);
        }
        if self.exports.resolve(key).is_none() {
            return Err(Error::StaleKey);
        }
        let entry = self.exports.slots[key.index].entry.as_ref().unwrap();
        entry.promise.resolution().map_err(Error::Promise)?;
        if entry.pending != 0 {
            return Err(Error::ResolutionConflict);
        }
        self.exports.pending_resolution = Some(key);
        Ok(())
    }
    pub fn resolution_id(&self) -> Option<ExportId> {
        self.exports
            .pending_resolution
            .map(|key| ExportId::from_wire(key.index as u32))
    }
    /// Reserve one additional descriptor, including repeats within this batch.
    pub fn grant(&mut self, key: Key<'id>) -> Result<Grant<'id>, Error> {
        let slot = self
            .exports
            .slots
            .get_mut(key.index)
            .ok_or(Error::StaleKey)?;
        if slot.generation != key.generation {
            return Err(Error::StaleKey);
        }
        let entry = slot.entry.as_mut().ok_or(Error::StaleKey)?;
        if self.exports.pending_resolution == Some(key) {
            return Err(Error::ResolutionConflict);
        }
        if entry.promise == promise::ImportState::Resolved {
            return Err(Error::Promise(promise::Error::AlreadyResolved));
        }
        let published = match &entry.state {
            State::Staged => 0,
            State::Live(refs) => refs.count(),
            State::Retired => return Err(Error::StaleKey),
        };
        published
            .checked_add(entry.pending)
            .and_then(|n| n.checked_add(1))
            .ok_or(Error::RemoteReferencesExhausted)?;
        entry.pending += 1;
        Ok(Grant {
            key,
            promise: entry.promise == promise::ImportState::Pending,
        })
    }
    /// Publish all reserved counts after the output adapter accepts the message.
    /// Cannot fail: reservations already checked all additions under this borrow.
    pub fn commit(self) {
        if let Some(key) = self.exports.pending_resolution.take() {
            let entry = self.exports.slots[key.index].entry.as_mut().unwrap();
            entry.promise = entry
                .promise
                .resolution()
                .expect("reserved pending promise");
        }
        for slot in &mut *self.exports.slots {
            if let Some(entry) = &mut slot.entry {
                match &mut entry.state {
                    State::Staged => {
                        let mut refs = ExportRefs::new();
                        refs.retain(entry.pending - 1)
                            .expect("reserved initial grants");
                        entry.state = State::Live(refs);
                    }
                    State::Live(refs) => refs.retain(entry.pending).expect("reserved grants"),
                    State::Retired => (),
                }
                entry.pending = 0;
            }
        }
        self.exports.open_batch = false;
    }
}
impl<T> Drop for Batch<'_, '_, '_, T> {
    fn drop(&mut self) {
        self.exports.abort_pending();
    }
}

#[cfg(test)]
mod descriptors_tests;
#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;

/// A checked local call reference, acquired only when the entire input commits.
#[must_use]
pub struct LeaseReservation<'a, 'id, 'storage, T> {
    exports: &'a mut Exports<'id, 'storage, T>,
    key: Key<'id>,
}
impl<'id, T> LeaseReservation<'_, 'id, '_, T> {
    pub fn accept(self) -> Lease<'id> {
        self.exports.slots[self.key.index]
            .entry
            .as_mut()
            .expect("preflight export")
            .local += 1;
        Lease { key: self.key }
    }
}

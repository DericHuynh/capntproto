//! Allocation-free loopback embargo ownership over caller-provided storage.
//!
//! IDs are never reused in one connection. A duplicate/delayed echo cannot
//! release a later barrier, even after an earlier barrier was cancelled.
//! Store the queued-call owner in each record. Release it only on a matching
//! accepted echo, explicit cancellation, or terminal connection draining.
use crate::{
    ids::{EmbargoId, WireId},
    peer_table::SparseStorage,
};
use core::marker::PhantomData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Full,
    IdsExhausted,
    Unknown,
    NotAccepted,
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::peer_table::{BorrowedSparse, Slot};

    /// Full-width connection cursor and arbitrary cancellation/output rejection.
    /// Reusing the storage slot must never let an old echo open a new barrier.
    #[kani::proof]
    #[kani::unwind(4)]
    fn stale_echo_cannot_open_reused_storage() {
        let start: u32 = kani::any();
        let mut slots = [Slot::EMPTY];
        let mut table = Embargoes::new(BorrowedSparse::new(&mut slots))
            .ok()
            .unwrap();
        table.next = u64::from(start);
        let offer = table.reserve(17u8).unwrap();
        let old = offer.id();
        if kani::any() {
            offer.accept();
            if kani::any() {
                assert_eq!(table.cancel(old), Ok(Some(17)));
                assert_eq!(table.receive(old), Ok(None));
            } else {
                assert_eq!(table.receive(old), Ok(Some(17)));
            }
        } else {
            drop(offer);
            assert_eq!(table.receive(old), Err(Error::NotAccepted));
            assert_eq!(table.take_rejected(), Some(17));
        }
        if start == u32::MAX {
            let rejected = table.reserve(23).err().unwrap();
            kani::assert(
                rejected.error == Error::IdsExhausted && rejected.value == 23,
                "embargo exhaustion preserves the rejected owner",
            );
        } else {
            let next = table.reserve(23).unwrap().accept();
            kani::assert(
                table.receive(old) == Err(Error::Unknown),
                "stale echo cannot open a new embargo",
            );
            assert_eq!(table.receive(next), Ok(Some(23)));
        }
        assert!(table.is_empty());
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC embargo: {self:?}")
    }
}
impl core::error::Error for Error {}
#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub value: T,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Offered,
    Accepted,
    Retired,
}
pub struct Record<T> {
    #[cfg(feature = "alloc")]
    presence: crate::observation::Presence,
    id: EmbargoId,
    phase: Phase,
    owner: Option<T>,
}
pub struct Embargoes<T, S: SparseStorage<EmbargoId, Record<T>>> {
    #[cfg(feature = "alloc")]
    observation: crate::observation::Counter,
    storage: S,
    next: u64,
    _owner: PhantomData<T>,
}
impl<T, S: SparseStorage<EmbargoId, Record<T>>> Embargoes<T, S> {
    pub fn new(storage: S) -> Result<Self, S> {
        if !storage.is_empty() {
            return Err(storage);
        }
        Ok(Self {
            #[cfg(feature = "alloc")]
            observation: Default::default(),
            storage,
            next: 0,
            _owner: PhantomData,
        })
    }
    #[cfg(feature = "alloc")]
    pub fn observe_len(&mut self) -> crate::observation::Observation {
        self.observation
            .observe(self.storage.values_mut().map(|r| &mut r.presence))
    }
    pub fn len(&self) -> usize {
        self.storage.len()
    }
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
    /// Reserve output ownership. Storage failure returns the owner unchanged and
    /// consumes no ID. Exhausting the u32 connection ID space fails explicitly.
    pub fn reserve(&mut self, value: T) -> Result<Offer<'_, T, S>, Rejected<T>> {
        let Ok(id) = u32::try_from(self.next) else {
            return Err(Rejected {
                error: Error::IdsExhausted,
                value,
            });
        };
        let id = EmbargoId::from_wire(id);
        let record = Record {
            #[cfg(feature = "alloc")]
            presence: self.observation.enter(),
            id,
            phase: Phase::Offered,
            owner: Some(value),
        };
        match self.storage.try_insert(id, record) {
            Ok(None) => (),
            Ok(Some(_)) => unreachable!("embargo IDs are never reused"),
            Err(record) => {
                return Err(Rejected {
                    error: Error::Full,
                    value: record.owner.unwrap(),
                })
            }
        }
        self.next += 1;
        Ok(Offer {
            table: self,
            id,
            accepted: false,
        })
    }
    /// Process an incoming receiverLoopback. An abandoned waiter leaves a
    /// tombstone until this echo arrives, preserving peer protocol obligations.
    pub fn receive(&mut self, id: EmbargoId) -> Result<Option<T>, Error> {
        let record = self.storage.get(&id).ok_or(Error::Unknown)?;
        if record.phase != Phase::Accepted {
            return Err(Error::NotAccepted);
        }
        Ok(self.storage.remove(&id).unwrap().owner)
    }
    /// Cancel queued work without allowing this ID to identify a new barrier.
    /// No user destructor runs under the table borrow.
    pub fn cancel(&mut self, id: EmbargoId) -> Result<Option<T>, Error> {
        let record = self.storage.get_mut(&id).ok_or(Error::Unknown)?;
        if record.phase != Phase::Accepted {
            return Err(Error::NotAccepted);
        }
        Ok(record.owner.take())
    }
    /// Detach a rejected/forgotten output offer's owner for cleanup outside the
    /// table borrow. Forgotten offers cannot become externally accepted sends.
    pub fn take_rejected(&mut self) -> Option<T> {
        let id = self
            .storage
            .values()
            .find(|r| r.phase != Phase::Accepted)
            .map(|r| r.id)?;
        self.storage.remove(&id).unwrap().owner
    }
    /// Terminal connection cleanup, including queued waiters and tombstones.
    pub fn drain_one(&mut self) -> Option<Option<T>> {
        let id = self.storage.values().next()?.id;
        Some(self.storage.remove(&id).unwrap().owner)
    }
}
/// Ordered output acceptance is the point at which a peer echo may open the
/// barrier. Dropping this offer retains its owner for take_rejected().
#[must_use]
pub struct Offer<'a, T, S: SparseStorage<EmbargoId, Record<T>>> {
    table: &'a mut Embargoes<T, S>,
    id: EmbargoId,
    accepted: bool,
}
impl<T, S: SparseStorage<EmbargoId, Record<T>>> Offer<'_, T, S> {
    pub fn id(&self) -> EmbargoId {
        self.id
    }
    pub fn accept(mut self) -> EmbargoId {
        self.table.storage.get_mut(&self.id).unwrap().phase = Phase::Accepted;
        self.accepted = true;
        self.id
    }
}
impl<T, S: SparseStorage<EmbargoId, Record<T>>> Drop for Offer<'_, T, S> {
    fn drop(&mut self) {
        if !self.accepted {
            self.table.storage.get_mut(&self.id).unwrap().phase = Phase::Retired;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_table::{BorrowedSparse, Slot};
    #[test]
    fn echo_cannot_release_a_new_barrier_after_storage_reuse() {
        let measured = allocation_counter::measure(|| {
            let mut slots = [Slot::EMPTY];
            let mut table = Embargoes::new(BorrowedSparse::new(&mut slots))
                .ok()
                .unwrap();
            let old = table.reserve(17).unwrap().accept();
            assert_eq!(table.cancel(old), Ok(Some(17)));
            assert_eq!(table.reserve(23).err().unwrap().value, 23);
            assert_eq!(table.receive(old), Ok(None));
            let new = table.reserve(23).unwrap().accept();
            assert_ne!(old, new);
            assert_eq!(table.receive(old), Err(Error::Unknown));
            assert_eq!(table.receive(new), Ok(Some(23)));
            assert!(table.is_empty());
        });
        assert_eq!(measured.count_total, 0);
    }
    #[test]
    fn output_rejection_forgetting_and_disconnect_preserve_owners() {
        let mut slots = [const { Slot::EMPTY }; 2];
        let mut table = Embargoes::new(BorrowedSparse::new(&mut slots))
            .ok()
            .unwrap();
        let offer = table.reserve(17).unwrap();
        let old = offer.id();
        drop(offer);
        assert_eq!(table.receive(old), Err(Error::NotAccepted));
        assert_eq!(table.take_rejected(), Some(17));
        core::mem::forget(table.reserve(23).unwrap());
        assert_eq!(table.take_rejected(), Some(23));
        let live = table.reserve(29).unwrap().accept();
        let tombstone = table.reserve(31).unwrap().accept();
        assert_eq!(table.cancel(tombstone), Ok(Some(31)));
        let a = table.drain_one().unwrap();
        let b = table.drain_one().unwrap();
        assert!(matches!((a, b), (Some(29), None) | (None, Some(29))));
        assert!(table.drain_one().is_none());
        assert_eq!(table.receive(live), Err(Error::Unknown));
    }
    #[test]
    fn maximum_id_is_used_once_and_never_wraps() {
        let mut slots = [Slot::EMPTY];
        let mut table = Embargoes::new(BorrowedSparse::new(&mut slots))
            .ok()
            .unwrap();
        table.next = u64::from(u32::MAX);
        let id = table.reserve(17).unwrap().accept();
        assert_eq!(id.to_wire(), u32::MAX);
        assert_eq!(table.receive(id), Ok(Some(17)));
        let rejected = table.reserve(23).err().unwrap();
        assert_eq!(rejected.error, Error::IdsExhausted);
        assert_eq!(rejected.value, 23);
        assert!(table.is_empty());
    }
}

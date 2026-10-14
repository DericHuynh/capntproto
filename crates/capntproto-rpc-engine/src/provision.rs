//! Third-party Accept embargo ordering. The network authenticates recipients
//! and owns provisions; this module only enforces rendezvous lifecycle rules.
//! Callers retain capability authority before claiming an embargo and keep it
//! until successful completion or cancellation. No IO or allocator is involved.
use core::marker::PhantomData;
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("provision/verified.rs");

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Full,
    IdentifierTooLong,
    DuplicateAccept,
    DuplicateRelease,
    Closed,
    Pending,
    Consumed,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "third-party embargo: {self:?}")
    }
}
impl core::error::Error for Error {}

/// One identifier within a provision. Accept and disembargo can arrive in either
/// order, exactly once each. Ending the provision cancels a pending embargo, but
/// does not revoke an already released acceptance's independently held authority.
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmbargoState {
    claimed: bool,
    released: bool,
    closed: bool,
}
#[cfg_attr(capntproto_verus, verus_spec(r => ensures embargo_initial(r)))]
const fn initial_embargo() -> EmbargoState {
    EmbargoState {
        claimed: false,
        released: false,
        closed: false,
    }
}
impl EmbargoState {
    pub const fn new() -> Self {
        initial_embargo()
    }
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures embargo_claim(*old(self), *final(self), r)))]
    pub fn claim(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.claimed {
            return Err(Error::DuplicateAccept);
        }
        self.claimed = true;
        Ok(())
    }
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures embargo_release(*old(self), *final(self), r)))]
    pub fn release(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.released {
            return Err(Error::DuplicateRelease);
        }
        self.released = true;
        Ok(())
    }
    #[cfg_attr(capntproto_verus, verus_spec(ensures embargo_close(*old(self), *final(self))))]
    pub fn close(&mut self) {
        self.closed = true;
    }
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == embargo_readiness(*self)))]
    pub fn readiness(&self) -> Result<(), Error> {
        if !self.claimed {
            return Err(Error::Pending);
        }
        if self.released {
            Ok(())
        } else if self.closed {
            Err(Error::Closed)
        } else {
            Err(Error::Pending)
        }
    }
}

struct Entry<const BYTES: usize> {
    id: [u8; BYTES],
    length: usize,
    state: EmbargoState,
    consumed: bool,
}
pub struct Slot<const BYTES: usize> {
    entry: Option<Entry<BYTES>>,
}
impl<const BYTES: usize> Slot<BYTES> {
    pub const EMPTY: Self = Self { entry: None };
}
/// Identity is scoped to one provision; identifiers and tombstones are retained
/// until that scope ends. Finishing a claim cannot permit a duplicate Accept.
#[derive(Debug)]
#[must_use = "complete or cancel the claim and release its independently held capability authority"]
pub struct Claim<'id> {
    index: usize,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}
#[derive(Debug)]
#[must_use = "retry or cancel this claim; it has not completed"]
pub struct Rejected<'id> {
    pub error: Error,
    pub claim: Claim<'id>,
}

pub struct Embargoes<'id, 'storage, const BYTES: usize> {
    slots: &'storage mut [Slot<BYTES>],
    closed: bool,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}
pub fn with_embargoes<const BYTES: usize, R>(
    slots: &mut [Slot<BYTES>],
    f: impl for<'id> FnOnce(&mut Embargoes<'id, '_, BYTES>) -> R,
) -> Result<R, Error> {
    if slots.iter().any(|slot| slot.entry.is_some()) {
        return Err(Error::NonemptyStorage);
    }
    Ok(f(&mut Embargoes {
        slots,
        closed: false,
        brand: PhantomData,
    }))
}
impl<'id, const BYTES: usize> Embargoes<'id, '_, BYTES> {
    fn find_or_insert(&mut self, id: &[u8]) -> Result<usize, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if id.len() > BYTES {
            return Err(Error::IdentifierTooLong);
        }
        if let Some(index) = self.slots.iter().position(|slot| {
            slot.entry
                .as_ref()
                .is_some_and(|entry| &entry.id[..entry.length] == id)
        }) {
            return Ok(index);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| slot.entry.is_none())
            .ok_or(Error::Full)?;
        let mut bytes = [0; BYTES];
        bytes[..id.len()].copy_from_slice(id);
        self.slots[index].entry = Some(Entry {
            id: bytes,
            length: id.len(),
            state: EmbargoState::new(),
            consumed: false,
        });
        Ok(index)
    }
    pub fn claim(&mut self, id: &[u8]) -> Result<Claim<'id>, Error> {
        let index = self.find_or_insert(id)?;
        self.slots[index].entry.as_mut().unwrap().state.claim()?;
        Ok(Claim {
            index,
            brand: PhantomData,
        })
    }
    pub fn release(&mut self, id: &[u8]) -> Result<(), Error> {
        let index = self.find_or_insert(id)?;
        self.slots[index].entry.as_mut().unwrap().state.release()
    }
    /// Readiness is polled by the caller's network/connection event loop. No
    /// application callback or wake hook runs while table state is being changed.
    pub fn readiness(&self, claim: &Claim<'id>) -> Result<(), Error> {
        let entry = self.slots[claim.index].entry.as_ref().unwrap();
        if entry.consumed {
            return Err(Error::Consumed);
        }
        entry.state.readiness()
    }
    pub fn complete(&mut self, claim: Claim<'id>) -> Result<(), Rejected<'id>> {
        if let Err(error) = self.readiness(&claim) {
            return Err(Rejected { error, claim });
        }
        self.slots[claim.index].entry.as_mut().unwrap().consumed = true;
        Ok(())
    }
    pub fn cancel(&mut self, claim: Claim<'id>) {
        self.slots[claim.index].entry.as_mut().unwrap().consumed = true;
    }
    pub fn close(&mut self) {
        self.closed = true;
        for entry in self.slots.iter_mut().filter_map(|slot| slot.entry.as_mut()) {
            entry.state.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn either_arrival_order_and_pending_revoke_preserve_exact_claim() {
        for first_release in [false, true] {
            let mut slots = [const { Slot::<4>::EMPTY }; 2];
            with_embargoes(&mut slots, |table| {
                if first_release {
                    table.release(b"a").unwrap();
                }
                let claim = table.claim(b"a").unwrap();
                assert_eq!(table.claim(b"a").unwrap_err(), Error::DuplicateAccept);
                let pending = table.claim(b"b").unwrap();
                assert_eq!(table.claim(b"c").unwrap_err(), Error::Full);
                assert_eq!(
                    table.release(b"large").unwrap_err(),
                    Error::IdentifierTooLong
                );
                let claim = if first_release {
                    claim
                } else {
                    let rejected = table.complete(claim).unwrap_err();
                    assert_eq!(rejected.error, Error::Pending);
                    table.release(b"a").unwrap();
                    rejected.claim
                };
                assert_eq!(table.release(b"a"), Err(Error::DuplicateRelease));
                table.close();
                table.complete(claim).unwrap();
                let rejected = table.complete(pending).unwrap_err();
                assert_eq!(rejected.error, Error::Closed);
                table.cancel(rejected.claim);
                assert_eq!(table.claim(b"a").unwrap_err(), Error::Closed);
                assert_eq!(table.release(b"b"), Err(Error::Closed));
            })
            .unwrap();
        }
    }
    #[test]
    fn tombstone_prevents_replay_and_empty_identifier_is_distinct_from_absence() {
        let mut slots = [const { Slot::<0>::EMPTY }; 1];
        with_embargoes(&mut slots, |table| {
            assert_eq!(table.release(b"a"), Err(Error::IdentifierTooLong));
            let claim = table.claim(b"").unwrap();
            table.cancel(claim);
            assert_eq!(table.claim(b"").unwrap_err(), Error::DuplicateAccept);
            table.release(b"").unwrap();
            assert_eq!(table.release(b""), Err(Error::DuplicateRelease));
        })
        .unwrap();
        assert_eq!(
            with_embargoes(&mut slots, |_| ()),
            Err(Error::NonemptyStorage)
        );
    }
}
#[cfg(kani)]
mod proofs;

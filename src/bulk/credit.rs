//! Byte-credit ownership, independent of remote application success.
#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
    sync::Arc,
};

/// A reservation issued by exactly one [`CreditWindow`]. Its sequence is only
/// a wire value, not authority to release another window's credit. This handle
/// cannot be constructed, cloned or deserialized by callers.
///
/// Dropping it does **not** release credit: losing a local handle does not mean
/// that a remote call has stopped. Keep it until observing the matching reply.
#[derive(Debug)]
#[must_use = "retain the reservation until its matching call settles; dropping it keeps credit charged"]
pub struct Reservation {
    owner: Arc<()>,
    sequence: NonZeroU64,
    bytes: NonZeroU32,
}

impl Reservation {
    /// Explicit projection for the outgoing request and matching reply check.
    #[must_use]
    pub fn sequence(&self) -> u64 {
        self.sequence.get()
    }

    #[must_use]
    pub fn bytes(&self) -> u32 {
        self.bytes.get()
    }
}

/// Local accounting only. Neither outcome establishes successful execution or
/// publication by the receiver; the caller must separately inspect the reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "settlement reports credit release, not successful remote execution"]
pub enum Settlement {
    Released,
    AlreadySettled,
}

/// Strict byte credit with sequential reservations. A reservation carries its
/// issuing window's identity even if either value moves. Settlement is
/// idempotent; foreign reservations fail without changing either window.
/// Only settle after observing the matching call's terminal reply, including an
/// error reply. A canceled local wait does not justify settlement.
pub struct CreditWindow {
    owner: Arc<()>,
    limit: u32,
    available: u32,
    max_chunks: u32,
    next: u64,
    pending: BTreeMap<NonZeroU64, NonZeroU32>,
}

impl CreditWindow {
    pub fn new(limit: u32, max_chunks: u32) -> capnp::Result<Self> {
        if limit == 0 || limit > 16 * 1024 * 1024 || max_chunks == 0 || max_chunks > 65536 {
            return Err(super::failed("invalid bulk credit limits"));
        }
        Ok(Self {
            owner: Arc::new(()),
            limit,
            available: limit,
            max_chunks,
            next: 1,
            pending: BTreeMap::new(),
        })
    }

    /// None means backpressure; no sequence or credit is consumed. Invalid sizes
    /// and exhausted sequence space fail without changing the window.
    pub fn reserve(&mut self, bytes: u32) -> capnp::Result<Option<Reservation>> {
        if bytes == 0 || bytes > self.limit || self.next > u64::from(self.max_chunks) {
            return Err(super::failed("bulk reservation outside limits"));
        }
        if bytes > self.available {
            return Ok(None);
        }
        let sequence = NonZeroU64::new(self.next).expect("sequences start at one");
        let bytes = NonZeroU32::new(bytes).expect("nonzero size checked");
        self.next += 1;
        self.available -= bytes.get();
        self.pending.insert(sequence, bytes);
        Ok(Some(Reservation {
            owner: self.owner.clone(),
            sequence,
            bytes,
        }))
    }

    pub fn settle(&mut self, reservation: &Reservation) -> capnp::Result<Settlement> {
        if !Arc::ptr_eq(&self.owner, &reservation.owner) {
            return Err(super::failed(
                "bulk reservation belongs to another credit window",
            ));
        }
        if let Some(bytes) = self.pending.remove(&reservation.sequence) {
            self.available += bytes.get();
            Ok(Settlement::Released)
        } else {
            Ok(Settlement::AlreadySettled)
        }
    }

    #[must_use]
    pub fn available(&self) -> u32 {
        self.available
    }
    #[must_use]
    pub fn in_flight(&self) -> u32 {
        self.limit - self.available
    }
    #[must_use]
    pub fn issued(&self) -> u64 {
        self.next - 1
    }
    #[must_use]
    pub fn pending_chunks(&self) -> usize {
        self.pending.len()
    }
}

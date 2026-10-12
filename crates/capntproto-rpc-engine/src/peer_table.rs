// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Copyright (c) 2026 Capntproto contributors
// Licensed under the MIT license; see LICENSE.

//! Peer-chosen answer/import IDs with a small direct-indexed fast path.
//!
//! Every wire ID, including zero and `u32::MAX`, is legal at this storage
//! boundary. Protocol handlers enforce any message-specific reserved ranges.
//! High IDs consume one sparse entry, never an array sized by an untrusted ID.
//! Returned owners can be dropped after releasing the caller's table borrow.
//!
//! ```
//! use capntproto_rpc_engine::{
//!     ids::{AnswerId, WireId},
//!     peer_table::{BorrowedSparse, InsertErrorKind, PeerTable, Slot},
//! };
//! let mut slots = [const { Slot::EMPTY }; 2];
//! // LOW = 0 gives a single, two-entry budget for all IDs.
//! let mut answers = PeerTable::<AnswerId, u32, _, 0>::try_new(
//!     BorrowedSparse::new(&mut slots),
//! ).expect("fresh storage");
//! let id = AnswerId::from_wire(u32::MAX);
//! answers.try_insert_unique(id, 42).unwrap();
//! let rejected = answers.try_insert_unique(id, 7).unwrap_err();
//! assert_eq!(rejected.kind, InsertErrorKind::Duplicate);
//! assert_eq!(rejected.value, 7);
//! assert_eq!(answers.remove(&id), Some(42));
//! ```

use crate::ids::{PeerId, WireId};
use core::marker::PhantomData;

/// Sparse storage supplied by the caller or hosted adapter.
///
/// `try_insert` replaces an existing entry even when full. For an absent key,
/// failure must return the value without changing any live entry. All other
/// methods have ordinary map semantics. Optional growing providers may allocate;
/// borrowed providers do not. Value destructors retain their own requirements.
pub trait SparseStorage<I: WireId, T> {
    fn get(&self, id: &I) -> Option<&T>;
    fn get_mut(&mut self, id: &I) -> Option<&mut T>;
    fn try_insert(&mut self, id: I, value: T) -> Result<Option<T>, T>;
    fn remove(&mut self, id: &I) -> Option<T>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn values<'s>(&'s self) -> impl Iterator<Item = &'s T>
    where
        T: 's;
    fn values_mut<'s>(&'s mut self) -> impl Iterator<Item = &'s mut T>
    where
        T: 's;
}

/// Explicitly disable a sparse ID range without borrowing a dummy buffer.
pub struct NoSparse;
impl<I: WireId, T> SparseStorage<I, T> for NoSparse {
    fn get(&self, _: &I) -> Option<&T> {
        None
    }
    fn get_mut(&mut self, _: &I) -> Option<&mut T> {
        None
    }
    fn try_insert(&mut self, _: I, value: T) -> Result<Option<T>, T> {
        Err(value)
    }
    fn remove(&mut self, _: &I) -> Option<T> {
        None
    }
    fn len(&self) -> usize {
        0
    }
    fn values<'s>(&'s self) -> impl Iterator<Item = &'s T>
    where
        T: 's,
    {
        core::iter::empty()
    }
    fn values_mut<'s>(&'s mut self) -> impl Iterator<Item = &'s mut T>
    where
        T: 's,
    {
        core::iter::empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertErrorKind {
    Duplicate,
    Full,
}

/// Rejected ownership is returned so the driver can retry or release it after
/// dropping its table borrow. No diagnostic string or error reply is allocated.
#[derive(Debug, PartialEq, Eq)]
pub struct InsertError<T> {
    pub kind: InsertErrorKind,
    pub value: T,
}

pub struct PeerTable<I: PeerId, T, S: SparseStorage<I, T>, const LOW: usize = 16> {
    low: [Option<T>; LOW],
    low_len: usize,
    high: S,
    id: PhantomData<I>,
}

impl<I: PeerId, T, S: SparseStorage<I, T>, const LOW: usize> PeerTable<I, T, S, LOW> {
    /// Require empty backing storage, returning it unchanged on rejection.
    /// Small IDs use `LOW` inline slots; other IDs use the supplied provider.
    /// These capacities are separate: a full sparse provider cannot use an
    /// unused inline slot. Choose `LOW = 0` for a single sparse capacity budget.
    pub fn try_new(high: S) -> Result<Self, S> {
        if !high.is_empty() {
            return Err(high);
        }
        Ok(Self {
            low: [const { None }; LOW],
            low_len: 0,
            high,
            id: PhantomData,
        })
    }

    fn low_index(id: I) -> Option<usize> {
        usize::try_from(id.to_wire()).ok().filter(|&i| i < LOW)
    }

    pub fn get(&self, id: &I) -> Option<&T> {
        match Self::low_index(*id) {
            Some(i) => self.low[i].as_ref(),
            None => self.high.get(id),
        }
    }

    pub fn get_mut(&mut self, id: &I) -> Option<&mut T> {
        match Self::low_index(*id) {
            Some(i) => self.low[i].as_mut(),
            None => self.high.get_mut(id),
        }
    }

    pub fn contains_key(&self, id: &I) -> bool {
        self.get(id).is_some()
    }

    /// Insert a new owner without replacing an existing owner. A duplicate is
    /// reported as such even when storage is full; neither case mutates state.
    pub fn try_insert_unique(&mut self, id: I, value: T) -> Result<(), InsertError<T>> {
        if self.contains_key(&id) {
            return Err(InsertError {
                kind: InsertErrorKind::Duplicate,
                value,
            });
        }
        self.try_insert(id, value)
            .map(|_| ())
            .map_err(|value| InsertError {
                kind: InsertErrorKind::Full,
                value,
            })
    }

    /// Explicitly allow replacement; return the previous owner instead of
    /// destroying it while the table is borrowed. `Err` returns the new owner.
    pub fn try_insert(&mut self, id: I, value: T) -> Result<Option<T>, T> {
        match Self::low_index(id) {
            Some(i) => {
                let previous = self.low[i].replace(value);
                self.low_len += usize::from(previous.is_none());
                Ok(previous)
            }
            None => self.high.try_insert(id, value),
        }
    }

    pub fn remove(&mut self, id: &I) -> Option<T> {
        match Self::low_index(*id) {
            Some(i) => {
                let previous = self.low[i].take();
                self.low_len -= usize::from(previous.is_some());
                previous
            }
            None => self.high.remove(id),
        }
    }

    pub fn len(&self) -> usize {
        self.low_len + self.high.len()
    }

    pub fn is_empty(&self) -> bool {
        self.low_len == 0 && self.high.is_empty()
    }

    pub fn values<'s>(&'s self) -> impl Iterator<Item = &'s T>
    where
        T: 's,
    {
        self.low
            .iter()
            .filter_map(Option::as_ref)
            .chain(self.high.values())
    }

    pub fn values_mut<'s>(&'s mut self) -> impl Iterator<Item = &'s mut T>
    where
        T: 's,
    {
        self.low
            .iter_mut()
            .filter_map(Option::as_mut)
            .chain(self.high.values_mut())
    }
}

/// One caller-owned sparse slot. Its contents cannot be mutated outside its
/// exclusive provider borrow, so ID uniqueness and occupancy stay coherent.
pub struct Slot<I: WireId, T> {
    entry: Option<(I, T)>,
}

impl<I: WireId, T> Slot<I, T> {
    pub const EMPTY: Self = Self { entry: None };
    pub fn is_empty(&self) -> bool {
        self.entry.is_none()
    }
}

/// Borrowed provider name retained for existing fixed-capacity callers.
pub type BorrowedSparse<'a, I, T> = SparseSlots<'a, I, T>;

/// Slot provider over exclusive caller-owned or optionally growing storage. Lookup takes
/// O(capacity) time. Borrowed capacity never grows; growing capacity is bounded
/// independently of the numeric wire ID.
/// Removal, replacement and rejection transfer ownership to the caller.
/// Dropping this provider drops remaining entries and empties the slice for
/// reuse. Detach the table from any runtime borrow before teardown/reentry.
pub struct SparseSlots<'a, I: WireId, T> {
    slots: crate::storage::Storage<'a, Slot<I, T>>,
    len: usize,
}

// Report resource metadata without requiring (or formatting) capability owners.
impl<I: WireId, T> core::fmt::Debug for SparseSlots<'_, I, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BorrowedSparse")
            .field("len", &self.len)
            .field("capacity", &self.slots.len())
            .finish()
    }
}

impl<'a, I: WireId, T> SparseSlots<'a, I, T> {
    pub fn new(slots: &'a mut [Slot<I, T>]) -> Self {
        Self::from_storage(crate::storage::Storage::borrowed(slots))
    }
    /// Optional bounded growth. Numeric wire IDs never determine allocation size.
    #[cfg(feature = "alloc")]
    pub fn growing(initial: usize, maximum: usize) -> Option<Self> {
        Some(Self::from_storage(crate::storage::Storage::growing(
            initial,
            maximum,
            || Slot::EMPTY,
        )?))
    }
    #[doc(hidden)]
    pub fn from_storage(slots: crate::storage::Storage<'a, Slot<I, T>>) -> Self {
        // Normally empty. Reattaching storage after mem::forget must neither
        // silently destroy its owners nor misreport its occupancy.
        let len = slots.iter().filter(|slot| slot.entry.is_some()).count();
        Self { slots, len }
    }
}

impl<I: WireId, T> SparseStorage<I, T> for SparseSlots<'_, I, T> {
    fn get(&self, id: &I) -> Option<&T> {
        self.slots
            .iter()
            .filter_map(|slot| slot.entry.as_ref())
            .find_map(|(key, value)| (key == id).then_some(value))
    }

    fn get_mut(&mut self, id: &I) -> Option<&mut T> {
        self.slots
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut())
            .find_map(|(key, value)| (key == id).then_some(value))
    }

    fn try_insert(&mut self, id: I, value: T) -> Result<Option<T>, T> {
        if let Some(previous) = self.get_mut(&id) {
            return Ok(Some(core::mem::replace(previous, value)));
        }
        let Some(slot) = self
            .slots
            .vacant_mut(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
        else {
            return Err(value);
        };
        slot.entry = Some((id, value));
        self.len += 1;
        Ok(None)
    }

    fn remove(&mut self, id: &I) -> Option<T> {
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.entry.as_ref().is_some_and(|(key, _)| key == id))?;
        let (_, value) = slot.entry.take()?;
        self.len -= 1;
        Some(value)
    }

    fn len(&self) -> usize {
        self.len
    }

    fn values<'s>(&'s self) -> impl Iterator<Item = &'s T>
    where
        T: 's,
    {
        self.slots
            .iter()
            .filter_map(|slot| slot.entry.as_ref().map(|(_, value)| value))
    }

    fn values_mut<'s>(&'s mut self) -> impl Iterator<Item = &'s mut T>
    where
        T: 's,
    {
        self.slots
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut().map(|(_, value)| value))
    }
}

impl<I: WireId, T> Drop for SparseSlots<'_, I, T> {
    fn drop(&mut self) {
        for slot in self.slots.iter_mut() {
            drop(slot.entry.take());
        }
    }
}

#[cfg(test)]
mod tests;

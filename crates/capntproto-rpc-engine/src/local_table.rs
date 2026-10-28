// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Copyright (c) 2026 Capntproto contributors
// Licensed under the MIT license; see LICENSE.

//! Locally allocated IDs with caller-owned storage and lowest-free-ID reuse.
//!
//! Ordinary IDs occupy the low 30 bits. Questions also have separate adopted
//! and pipeline-only ranges; only question tables expose those insertion APIs.
//! Removal returns ownership so destructors can run outside a runtime borrow.
//! Wire IDs do not contain generations: the protocol must authorize reuse.
//!
//! ```
//! use capntproto_rpc_engine::{
//!     ids::{ExportId, WireId},
//!     local_table::{BorrowedSlots, LocalTable, Slot},
//!     peer_table::BorrowedSparse,
//! };
//! let mut storage = [const { Slot::EMPTY }; 2];
//! let mut high = [];
//! let mut adopted = [];
//! let mut table = LocalTable::<ExportId, _, _, _, _>::try_new(
//!     BorrowedSlots::new(&mut storage),
//!     BorrowedSparse::new(&mut high),
//!     BorrowedSparse::new(&mut adopted),
//! ).expect("empty storage");
//! let id = table.try_push(42).unwrap();
//! assert_eq!(id.to_wire(), 0);
//! assert_eq!(table.remove(id), Some(42));
//! assert_eq!(table.try_push(7).unwrap(), id);
//! ```

use crate::{
    ids::{LocalId, QuestionId, WireId},
    peer_table::SparseStorage,
};
use core::marker::PhantomData;

const LOW_LIMIT: u32 = 1 << 30;
const HIGH_BIT: u32 = 1 << 31;

/// Dense entry plus one cell of the table's free-ID min-heap. Heap cells are
/// independent of occupied values, so removal never needs extra storage.
pub struct Slot<T> {
    value: Option<T>,
    free_id: u32,
}

impl<T> Slot<T> {
    pub const EMPTY: Self = Self {
        value: None,
        free_id: 0,
    };
    pub fn is_empty(&self) -> bool {
        self.value.is_none()
    }
}

/// Contiguous local-ID storage. A successful growth must extend the slice with
/// empty slots, preserving every existing slot. Failure leaves it unchanged.
/// Optional allocator-backed providers grow; fixed providers return false.
pub trait Storage<T> {
    fn slots(&self) -> &[Slot<T>];
    fn slots_mut(&mut self) -> &mut [Slot<T>];
    fn try_grow(&mut self) -> bool;
}

/// Exclusive fixed capacity. Dropping it clears retained owners for reuse.
/// User value destructors retain their own allocation and reentry requirements.
pub struct BorrowedSlots<'a, T>(&'a mut [Slot<T>]);

impl<'a, T> BorrowedSlots<'a, T> {
    pub fn new(slots: &'a mut [Slot<T>]) -> Self {
        Self(slots)
    }
}

impl<T> core::fmt::Debug for BorrowedSlots<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BorrowedSlots")
            .field("capacity", &self.0.len())
            .finish()
    }
}

impl<T> Storage<T> for BorrowedSlots<'_, T> {
    fn slots(&self) -> &[Slot<T>] {
        self.0
    }
    fn slots_mut(&mut self) -> &mut [Slot<T>] {
        self.0
    }
    fn try_grow(&mut self) -> bool {
        false
    }
}

impl<T> Drop for BorrowedSlots<'_, T> {
    fn drop(&mut self) {
        for slot in self.0.iter_mut() {
            drop(slot.value.take());
        }
    }
}

/// Optional allocator-backed storage for the same local-ID transitions. Growth
/// is fallible and cannot cross either the explicit bound or the low-ID range.
#[cfg(feature = "alloc")]
pub struct GrowingSlots<T> {
    slots: alloc::vec::Vec<Slot<T>>,
    maximum: usize,
}
#[cfg(feature = "alloc")]
impl<T> GrowingSlots<T> {
    pub const fn new() -> Self {
        Self {
            slots: alloc::vec::Vec::new(),
            maximum: LOW_LIMIT as usize,
        }
    }
    pub fn try_with_capacity(initial: usize, maximum: usize) -> Option<Self> {
        if initial > maximum || maximum as u64 > u64::from(LOW_LIMIT) {
            return None;
        }
        let mut slots = alloc::vec::Vec::new();
        slots.try_reserve_exact(initial).ok()?;
        slots.resize_with(initial, || Slot::EMPTY);
        Some(Self { slots, maximum })
    }
}
#[cfg(feature = "alloc")]
impl<T> Default for GrowingSlots<T> {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(feature = "alloc")]
impl<T> Storage<T> for GrowingSlots<T> {
    fn slots(&self) -> &[Slot<T>] {
        &self.slots
    }
    fn slots_mut(&mut self) -> &mut [Slot<T>] {
        &mut self.slots
    }
    fn try_grow(&mut self) -> bool {
        let Some(target) = crate::storage::growth_target(self.slots.len(), 1, self.maximum) else {
            return false;
        };
        if self
            .slots
            .try_reserve_exact(target - self.slots.len())
            .is_err()
        {
            return false;
        }
        self.slots.resize_with(target, || Slot::EMPTY);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertErrorKind {
    Full,
    IdSpaceExhausted,
    InvalidAdoptedId,
    InvalidPipelineOnlyId,
    Duplicate,
}

/// Rejection preserves the value, live entries, free IDs and high-ID cursor.
#[derive(Debug, PartialEq, Eq)]
pub struct InsertError<T> {
    pub kind: InsertErrorKind,
    pub value: T,
}

pub struct LocalTable<I: LocalId, T, D: Storage<T>, H: SparseStorage<I, T>, A: SparseStorage<I, T>>
{
    dense: D,
    high: H,
    adopted: A,
    issued: usize,
    free_len: usize,
    high_counter: u32,
    marker: PhantomData<(I, T)>,
}

impl<I: LocalId, T, D: Storage<T>, H: SparseStorage<I, T>, A: SparseStorage<I, T>>
    LocalTable<I, T, D, H, A>
{
    /// All three providers must be empty. Returns them intact on rejection.
    /// The capacities are independent; unused adopted/high slots cannot serve
    /// ordinary allocations. Exports and embargoes can supply empty sparse slices.
    pub fn try_new(dense: D, high: H, adopted: A) -> Result<Self, (D, H, A)> {
        if dense.slots().iter().any(|slot| slot.value.is_some())
            || !high.is_empty()
            || !adopted.is_empty()
        {
            return Err((dense, high, adopted));
        }
        Ok(Self {
            dense,
            high,
            adopted,
            issued: 0,
            free_len: 0,
            high_counter: 0,
            marker: PhantomData,
        })
    }

    pub fn try_push(&mut self, value: T) -> Result<I, InsertError<T>> {
        self.try_push_with(value, |_, value| value)
    }

    /// Construct a value with its final ID, before consuming that ID. A failed
    /// reservation returns the input untouched; constructor unwinding leaves
    /// every ID and existing entry intact (growth may retain empty capacity).
    #[inline(always)]
    pub fn try_push_with<V>(
        &mut self,
        value: V,
        build: impl FnOnce(I, V) -> T,
    ) -> Result<I, InsertError<V>> {
        let index = if self.free_len != 0 {
            self.dense.slots()[0].free_id as usize
        } else {
            if self.issued as u64 == u64::from(LOW_LIMIT) {
                return Err(InsertError {
                    kind: InsertErrorKind::IdSpaceExhausted,
                    value,
                });
            }
            if self.issued == self.dense.slots().len() && !self.dense.try_grow() {
                return Err(InsertError {
                    kind: InsertErrorKind::Full,
                    value,
                });
            }
            self.issued
        };
        let id = I::from_wire(index as u32);
        let value = build(id, value);
        if self.free_len != 0 {
            let selected = self.pop_free();
            debug_assert_eq!(selected as usize, index);
        } else {
            self.issued += 1;
        }
        self.dense.slots_mut()[index].value = Some(value);
        Ok(id)
    }

    pub fn get(&self, id: I) -> Option<&T> {
        let raw = id.to_wire();
        if raw & HIGH_BIT != 0 {
            return self.high.get(&id);
        }
        if raw & (1 << 30) != 0 {
            return self.adopted.get(&id);
        }
        self.dense
            .slots()
            .get(usize::try_from(raw).ok()?)?
            .value
            .as_ref()
    }

    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        let raw = id.to_wire();
        if raw & HIGH_BIT != 0 {
            return self.high.get_mut(&id);
        }
        if raw & (1 << 30) != 0 {
            return self.adopted.get_mut(&id);
        }
        self.dense
            .slots_mut()
            .get_mut(usize::try_from(raw).ok()?)?
            .value
            .as_mut()
    }

    #[inline(always)]
    pub fn remove(&mut self, id: I) -> Option<T> {
        let raw = id.to_wire();
        if raw & HIGH_BIT != 0 {
            return self.high.remove(&id);
        }
        if raw & (1 << 30) != 0 {
            return self.adopted.remove(&id);
        }
        let value = self
            .dense
            .slots_mut()
            .get_mut(usize::try_from(raw).ok()?)?
            .value
            .take()?;
        self.push_free(raw);
        Some(value)
    }

    pub fn len(&self) -> usize {
        self.issued - self.free_len + self.high.len() + self.adopted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.issued == self.free_len && self.high.is_empty() && self.adopted.is_empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.dense
            .slots()
            .iter()
            .take(self.issued)
            .filter_map(|slot| slot.value.as_ref())
            .chain(self.high.values())
            .chain(self.adopted.values())
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.dense
            .slots_mut()
            .iter_mut()
            .take(self.issued)
            .filter_map(|slot| slot.value.as_mut())
            .chain(self.high.values_mut())
            .chain(self.adopted.values_mut())
    }

    #[inline(always)]
    fn push_free(&mut self, id: u32) {
        let slots = self.dense.slots_mut();
        let mut position = self.free_len;
        self.free_len += 1;
        while position != 0 {
            let parent = (position - 1) / 2;
            if slots[parent].free_id <= id {
                break;
            }
            slots[position].free_id = slots[parent].free_id;
            position = parent;
        }
        slots[position].free_id = id;
    }

    #[inline(always)]
    fn pop_free(&mut self) -> u32 {
        let slots = self.dense.slots_mut();
        let id = slots[0].free_id;
        self.free_len -= 1;
        if self.free_len == 0 {
            return id;
        }
        let last = slots[self.free_len].free_id;
        let mut position = 0;
        while position * 2 + 1 < self.free_len {
            let mut child = position * 2 + 1;
            if child + 1 < self.free_len && slots[child + 1].free_id < slots[child].free_id {
                child += 1;
            }
            if last <= slots[child].free_id {
                break;
            }
            slots[position].free_id = slots[child].free_id;
            position = child;
        }
        slots[position].free_id = last;
        id
    }
}

impl<T, D: Storage<T>, H: SparseStorage<QuestionId, T>, A: SparseStorage<QuestionId, T>>
    LocalTable<QuestionId, T, D, H, A>
{
    pub fn try_push_high(&mut self, value: T) -> Result<QuestionId, InsertError<T>> {
        if self.high.len() as u64 >= u64::from(HIGH_BIT) {
            return Err(InsertError {
                kind: InsertErrorKind::IdSpaceExhausted,
                value,
            });
        }
        let mut candidate = self.high_counter | HIGH_BIT;
        // At most len + 1 candidates: every occupied candidate is distinct and
        // the sparse range has a free ID. No iteration over all 2^31 IDs.
        while self.high.get(&QuestionId::from_wire(candidate)).is_some() {
            candidate = candidate.wrapping_add(1) | HIGH_BIT;
        }
        let id = QuestionId::from_wire(candidate);
        self.high
            .try_insert(id, value)
            .map_err(|value| InsertError {
                kind: InsertErrorKind::Full,
                value,
            })?;
        self.high_counter = candidate.wrapping_add(1);
        Ok(id)
    }

    pub fn adopted_len(&self) -> usize {
        self.adopted.len()
    }

    /// Insert a caller-selected pipeline-only ID. The protocol ledger owns
    /// monotonic allocation and exhaustion; this provider never sizes storage
    /// from the numeric ID or changes the wrapping allocator's cursor.
    pub fn try_insert_high(&mut self, id: QuestionId, value: T) -> Result<(), InsertError<T>> {
        let kind = if !id.is_pipeline_only() {
            InsertErrorKind::InvalidPipelineOnlyId
        } else if self.high.get(&id).is_some() {
            InsertErrorKind::Duplicate
        } else {
            self.high
                .try_insert(id, value)
                .map_err(|value| InsertError {
                    kind: InsertErrorKind::Full,
                    value,
                })?;
            return Ok(());
        };
        Err(InsertError { kind, value })
    }

    pub fn try_insert_adopted(&mut self, id: QuestionId, value: T) -> Result<(), InsertError<T>> {
        let kind = if !id.is_adopted() {
            InsertErrorKind::InvalidAdoptedId
        } else if self.adopted.get(&id).is_some() {
            InsertErrorKind::Duplicate
        } else {
            self.adopted
                .try_insert(id, value)
                .map_err(|value| InsertError {
                    kind: InsertErrorKind::Full,
                    value,
                })?;
            return Ok(());
        };
        Err(InsertError { kind, value })
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs;

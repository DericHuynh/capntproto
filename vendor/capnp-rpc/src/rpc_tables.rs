// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! Typed local tables. Storage and free-list mutation stay behind this API so
//! removing an absent entry cannot recycle a live slot twice. Returned owners
//! may be dropped after releasing a runtime RefCell borrow, allowing reentry.
#![forbid(unsafe_code)]

use super::rpc_ids::{LocalId, PeerId, QuestionId, WireId};
use std::{
    cmp::Reverse,
    collections::{hash_map::Entry, BinaryHeap, HashMap},
};

/// The peer chooses these keys. Zero and the entire u32 range remain legal at
/// this storage boundary; individual message handlers enforce reserved ranges.
pub(super) struct PeerTable<I: PeerId, T> {
    pub(super) slots: HashMap<I, T>,
}

impl<I: PeerId, T> PeerTable<I, T> {
    pub(super) fn new() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }
}

pub(super) struct LocalTable<I: LocalId, T> {
    slots: Vec<Option<T>>,
    high: HashMap<I, T>,
    adopted: HashMap<I, T>,
    high_counter: u32,
    free_ids: BinaryHeap<Reverse<I>>,
}

impl<I: LocalId, T> LocalTable<I, T> {
    pub(super) fn new() -> Self {
        Self {
            slots: Vec::new(),
            high: HashMap::new(),
            adopted: HashMap::new(),
            high_counter: 0,
            free_ids: BinaryHeap::new(),
        }
    }

    pub(super) fn remove(&mut self, id: I) -> Option<T> {
        let raw = id.to_wire();
        if raw & (1 << 31) != 0 {
            return self.high.remove(&id);
        }
        if raw & (1 << 30) != 0 {
            return self.adopted.remove(&id);
        }
        let value = self.slots.get_mut(raw as usize)?.take()?;
        self.free_ids.push(Reverse(id));
        Some(value)
    }

    pub(super) fn erase(&mut self, id: I) {
        drop(self.remove(id));
    }

    pub(super) fn push(&mut self, value: T) -> I {
        if let Some(Reverse(id)) = self.free_ids.pop() {
            self.slots[id.to_wire() as usize] = Some(value);
            id
        } else {
            assert!(self.slots.len() < (1 << 30), "local RPC ID space exhausted");
            let id = I::from_wire(self.slots.len() as u32);
            self.slots.push(Some(value));
            id
        }
    }

    pub(super) fn get(&self, id: I) -> Option<&T> {
        let raw = id.to_wire();
        if raw & (1 << 31) != 0 {
            return self.high.get(&id);
        }
        if raw & (1 << 30) != 0 {
            return self.adopted.get(&id);
        }
        self.slots.get(raw as usize)?.as_ref()
    }

    pub(super) fn find(&mut self, id: I) -> Option<&mut T> {
        let raw = id.to_wire();
        if raw & (1 << 31) != 0 {
            return self.high.get_mut(&id);
        }
        if raw & (1 << 30) != 0 {
            return self.adopted.get_mut(&id);
        }
        self.slots.get_mut(raw as usize)?.as_mut()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.slots.len() == self.free_ids.len() && self.high.is_empty() && self.adopted.is_empty()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots
            .iter()
            .filter_map(Option::as_ref)
            .chain(self.high.values())
            .chain(self.adopted.values())
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots
            .iter_mut()
            .filter_map(Option::as_mut)
            .chain(self.high.values_mut())
            .chain(self.adopted.values_mut())
    }
}

// The sparse ID spaces belong only to questions, not exports or embargoes.
impl<T> LocalTable<QuestionId, T> {
    pub(super) fn push_high(&mut self, value: T) -> QuestionId {
        assert!(
            self.high.len() < (1 << 31) - 1,
            "high question ID space exhausted"
        );
        loop {
            let id = QuestionId::from_wire(self.high_counter | (1 << 31));
            self.high_counter = self.high_counter.wrapping_add(1);
            if let Entry::Vacant(slot) = self.high.entry(id) {
                slot.insert(value);
                return id;
            }
        }
    }

    pub(super) fn adopted_len(&self) -> usize {
        self.adopted.len()
    }

    pub(super) fn insert_adopted(&mut self, id: QuestionId, value: T) -> Result<(), T> {
        if id.is_adopted() {
            if let Entry::Vacant(slot) = self.adopted.entry(id) {
                slot.insert(value);
                return Ok(());
            }
        }
        Err(value)
    }
}

#[cfg(test)]
#[path = "rpc_tables/tests.rs"]
mod tests;

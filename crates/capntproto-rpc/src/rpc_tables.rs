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
use capntproto_rpc_engine::local_table::{self, GrowingSlots};
use std::collections::HashMap;

/// The peer chooses these keys. Zero and the entire u32 range remain legal at
/// this storage boundary; individual message handlers enforce reserved ranges.
pub(super) struct PeerTable<I: PeerId, T> {
    pub(super) slots: PeerSlots<I, T>,
}

impl<I: PeerId, T> PeerTable<I, T> {
    pub(super) fn new() -> Self {
        Self {
            slots: PeerSlots::default(),
        }
    }
}

// Keep the shared engine's direct-indexed fast path; only its sparse storage
// provider depends on std. Heap exhaustion retains the hosted allocator's
// existing policy, while bounded providers return the uninserted value.
pub(super) struct HostedSparse<I: WireId, T>(HashMap<I, T>);
impl<I: WireId, T> Default for HostedSparse<I, T> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<I: WireId, T> capntproto_rpc_engine::peer_table::SparseStorage<I, T> for HostedSparse<I, T> {
    fn get(&self, id: &I) -> Option<&T> {
        self.0.get(id)
    }
    fn get_mut(&mut self, id: &I) -> Option<&mut T> {
        self.0.get_mut(id)
    }
    fn try_insert(&mut self, id: I, value: T) -> Result<Option<T>, T> {
        Ok(self.0.insert(id, value))
    }
    fn remove(&mut self, id: &I) -> Option<T> {
        self.0.remove(id)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn values<'s>(&'s self) -> impl Iterator<Item = &'s T>
    where
        T: 's,
    {
        self.0.values()
    }
    fn values_mut<'s>(&'s mut self) -> impl Iterator<Item = &'s mut T>
    where
        T: 's,
    {
        self.0.values_mut()
    }
}

pub(super) struct PeerSlots<I: PeerId, T> {
    table: capntproto_rpc_engine::peer_table::PeerTable<I, T, HostedSparse<I, T>>,
}
impl<I: PeerId, T> Default for PeerSlots<I, T> {
    fn default() -> Self {
        Self {
            table: capntproto_rpc_engine::peer_table::PeerTable::try_new(HostedSparse(
                HashMap::new(),
            ))
            .ok()
            .expect("new map is empty"),
        }
    }
}
impl<I: PeerId, T> std::ops::Deref for PeerSlots<I, T> {
    type Target = capntproto_rpc_engine::peer_table::PeerTable<I, T, HostedSparse<I, T>>;
    fn deref(&self) -> &Self::Target {
        &self.table
    }
}
impl<I: PeerId, T> std::ops::DerefMut for PeerSlots<I, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.table
    }
}
impl<I: PeerId, T> PeerSlots<I, T> {
    pub(super) fn insert(&mut self, id: I, value: T) -> Option<T> {
        // The HashMap provider cannot return Err. Discarding that impossible
        // error type avoids imposing Debug on capability owners.
        self.table
            .try_insert(id, value)
            .ok()
            .expect("hosted map insertion is infallible")
    }
}

pub(super) struct LocalTable<I: LocalId, T> {
    table: HostedLocalTable<I, T>,
}

type HostedLocalTable<I, T> =
    local_table::LocalTable<I, T, GrowingSlots<T>, HostedSparse<I, T>, HostedSparse<I, T>>;

impl<I: LocalId, T> LocalTable<I, T> {
    pub(super) fn new() -> Self {
        Self {
            table: local_table::LocalTable::try_new(
                GrowingSlots::new(),
                HostedSparse(HashMap::new()),
                HostedSparse(HashMap::new()),
            )
            .ok()
            .expect("new local storage is empty"),
        }
    }

    pub(super) fn remove(&mut self, id: I) -> Option<T> {
        self.table.remove(id)
    }

    pub(super) fn erase(&mut self, id: I) {
        drop(self.remove(id));
    }

    pub(super) fn push(&mut self, value: T) -> I {
        self.table
            .try_push(value)
            .ok()
            .expect("local RPC ID space exhausted")
    }

    pub(super) fn get(&self, id: I) -> Option<&T> {
        self.table.get(id)
    }

    pub(super) fn find(&mut self, id: I) -> Option<&mut T> {
        self.table.get_mut(id)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.table.len()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        self.table.values()
    }
}

// The sparse ID spaces belong only to questions, not exports or embargoes.
impl<T> LocalTable<QuestionId, T> {
    pub(super) fn push_high(&mut self, value: T) -> QuestionId {
        self.table
            .try_push_high(value)
            .ok()
            .expect("high question ID space exhausted")
    }

    pub(super) fn adopted_len(&self) -> usize {
        self.table.adopted_len()
    }

    pub(super) fn insert_adopted(&mut self, id: QuestionId, value: T) -> Result<(), T> {
        self.table
            .try_insert_adopted(id, value)
            .map_err(|error| error.value)
    }
}

#[cfg(test)]
#[path = "rpc_tables/tests.rs"]
mod tests;

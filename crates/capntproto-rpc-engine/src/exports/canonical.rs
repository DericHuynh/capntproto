//! The legacy exports-by-capability index, backed by the export slots themselves.
//! Graph generations and privately recognized hosted owners supply identities.
//! Buckets share the slot array but are independent of each slot's occupant. Growing the
//! array rehashes once; ordinary lookup, removal and rollback do not scan it.
use super::*;
use core::num::{NonZeroU32, NonZeroU64};

pub(super) type Link = Option<NonZeroU32>;

/// Connection-local outgoing deduplication identity. It never authorizes an
/// incoming capability. Graph identities include their generation; hosted
/// owners may provide an identity only after checking their own private type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Identity(Kind);
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Graph {
        index: usize,
        generation: NonZeroU64,
    },
    Owner {
        namespace: usize,
        value: usize,
    },
}
impl Identity {
    pub(crate) fn new(index: usize, generation: u64) -> Self {
        Self(Kind::Graph {
            index,
            generation: NonZeroU64::new(generation).expect("live graph generation"),
        })
    }
    /// Distinct owner classes must use distinct namespaces. The policy must
    /// retain the original owner while this identity is exported and preserve
    /// membranes instead of deriving authority from public hook diagnostics.
    pub fn for_owner(namespace: usize, value: usize) -> Self {
        Self(Kind::Owner { namespace, value })
    }
    fn bucket(self, capacity: usize) -> usize {
        let hash = match self.0 {
            Kind::Graph { index, generation } => index as u64 ^ generation.get().rotate_left(32),
            Kind::Owner { namespace, value } => {
                (namespace as u64).rotate_left(32) ^ value as u64 ^ 0x9e37_79b9_7f4a_7c15
            }
        }
        .wrapping_mul(0x517c_c1b7_2722_0a95);
        // Fold high bits down: aligned hosted pointers otherwise all collide
        // in a small subset of power-of-two slot arrays.
        let hash = hash ^ (hash >> 32);
        (hash % capacity as u64) as usize
    }
}
impl From<crate::clients::Key<'_>> for Identity {
    fn from(key: crate::clients::Key<'_>) -> Self {
        key.export_identity()
    }
}
pub(super) struct Entry {
    identity: Identity,
    previous: Link,
    next: Link,
}
fn link(index: usize) -> NonZeroU32 {
    NonZeroU32::new(index as u32 + 1).expect("bounded export index")
}
fn index(link: NonZeroU32) -> usize {
    (link.get() - 1) as usize
}

impl<T> Exports<'_, '_, T> {
    fn insert_canonical(&mut self, index: usize, identity: Identity) {
        debug_assert!(self.slots[index].canonical.is_none());
        self.slots[index].canonical = Some(Entry {
            identity,
            previous: None,
            next: None,
        });
        self.link_canonical(index);
        self.canonical_count += 1;
    }
    fn link_canonical(&mut self, item: usize) {
        let bucket = self.slots[item]
            .canonical
            .as_ref()
            .unwrap()
            .identity
            .bucket(self.slots.len());
        let next = self.slots[bucket].bucket;
        let entry = self.slots[item].canonical.as_mut().unwrap();
        entry.previous = None;
        entry.next = next;
        if let Some(next) = next {
            self.slots[index(next)].canonical.as_mut().unwrap().previous = Some(link(item));
        }
        self.slots[bucket].bucket = Some(link(item));
    }
    pub(super) fn remove_canonical(&mut self, item: usize) {
        let Some(entry) = self.slots.get_mut(item).and_then(|s| s.canonical.take()) else {
            return;
        };
        if let Some(previous) = entry.previous {
            self.slots[index(previous)].canonical.as_mut().unwrap().next = entry.next;
        } else {
            let bucket = entry.identity.bucket(self.slots.len());
            self.slots[bucket].bucket = entry.next;
        }
        if let Some(next) = entry.next {
            self.slots[index(next)].canonical.as_mut().unwrap().previous = entry.previous;
        }
        self.canonical_count -= 1;
    }
    pub(super) fn rehash_canonical(&mut self) {
        if self.canonical_count == 0 {
            return;
        }
        for slot in &mut *self.slots {
            slot.bucket = None;
        }
        for index in 0..self.slots.len() {
            if self.slots[index].canonical.is_some() {
                self.link_canonical(index);
            }
        }
    }
    fn find_canonical(&self, identity: Identity) -> Option<usize> {
        if self.canonical_count == 0 {
            return None;
        }
        let mut next = self.slots[identity.bucket(self.slots.len())].bucket;
        while let Some(item) = next {
            let item = index(item);
            let entry = self.slots[item].canonical.as_ref().unwrap();
            if entry.identity == identity {
                return Some(item);
            }
            next = entry.next;
        }
        None
    }
}

impl<'e, 'g> Batch<'_, 'e, '_, crate::clients::Client<'g>> {
    /// Reuse a staged or published export of this exact graph identity. The
    /// grant participates in this batch's descriptor counts and rollback, just
    /// like a newly exported capability. Final Release invalidates the index
    /// immediately, even while local call leases retain its former owner.
    pub fn grant_canonical(
        &mut self,
        client: impl Into<Identity>,
    ) -> Result<Option<Grant<'e>>, Error> {
        let Some(index) = self.exports.find_canonical(client.into()) else {
            return Ok(None);
        };
        let mut grant = self.grant(self.exports.key(index))?;
        // Like legacy write_descriptor(), only the first descriptor introduces
        // senderPromise. Aliases use senderHosted and share its one Resolve.
        grant.promise = false;
        Ok(Some(grant))
    }

    /// Associate a frozen graph route with its first export. Check
    /// `grant_canonical` first; the source identity may differ from the owner
    /// for an independent exported-promise observer. No graph reference is added
    /// or destroyed by the index itself.
    pub fn export_canonical(
        &mut self,
        source: impl Into<Identity>,
        value: crate::clients::Client<'g>,
    ) -> Result<Grant<'e>, Rejected<crate::clients::Client<'g>>> {
        let grant = self.export(value)?;
        self.exports
            .insert_canonical(grant.key.index, source.into());
        Ok(grant)
    }

    /// As `export_canonical`, with one explicit Resolve obligation shared by
    /// every subsequent descriptor for this promise.
    pub fn export_canonical_promise(
        &mut self,
        source: impl Into<Identity>,
        value: crate::clients::Client<'g>,
    ) -> Result<Grant<'e>, Rejected<crate::clients::Client<'g>>> {
        let grant = self.export_promise(value)?;
        self.exports
            .insert_canonical(grant.key.index, source.into());
        Ok(grant)
    }
}

#[cfg(test)]
mod tests;

//! Immutable component manifests. References are object-local, backward-only,
//! and resolved at recovery/commit, never by walking a delta chain on reads.
use super::*;
use std::ops::Bound::{Excluded, Included};

const MAX_COMPONENTS: usize = 256;
const PREFIX: usize = 24;
const DESCRIPTOR: usize = 24;

/// Stable application-selected component name within one storage object.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ComponentId(u64);
impl ComponentId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Replace a component with `Some(bytes)`, or remove it with `None`.
/// Duplicate IDs and removing a missing component are rejected before IO.
pub struct ComponentUpdate<'a> {
    pub id: ComponentId,
    pub value: Option<&'a [u8]>,
}

#[derive(Clone, Copy)]
pub(super) struct Chunk {
    origin: Revision,
    offset: usize,
    len: usize,
}
pub(super) type Manifest = BTreeMap<ComponentId, Chunk>;
pub(super) type Manifests = BTreeMap<(ObjectKey, Revision), Arc<Manifest>>;
type Rebased = BTreeMap<(ObjectKey, ComponentId, Revision), Revision>;

/// Shared manifests and borrowed reads: snapshots never copy component payloads.
/// One mapping and the immutable manifest retain all bytes and writer locks.
#[derive(Clone)]
pub struct ComponentSnapshot {
    backing: Snapshot,
    manifest: Arc<Manifest>,
}
impl ComponentSnapshot {
    pub fn object(&self) -> ObjectKey {
        self.backing.object()
    }
    pub fn revision(&self) -> Revision {
        self.backing.revision()
    }
    pub fn get(&self, id: ComponentId) -> Option<&[u8]> {
        self.manifest
            .get(&id)
            .map(|c| &self.backing.map[c.offset..c.offset + c.len])
    }
    pub fn components(&self) -> impl ExactSizeIterator<Item = (ComponentId, &[u8])> {
        self.manifest
            .iter()
            .map(|(&id, c)| (id, &self.backing.map[c.offset..c.offset + c.len]))
    }
}

#[derive(Clone)]
pub struct ComponentPublication {
    snapshot: ComponentSnapshot,
    cursor: PublicationCursor,
}
impl ComponentPublication {
    pub fn snapshot(&self) -> &ComponentSnapshot {
        &self.snapshot
    }
    pub fn cursor(&self) -> &PublicationCursor {
        &self.cursor
    }
    pub fn into_parts(self) -> (ComponentSnapshot, PublicationCursor) {
        (self.snapshot, self.cursor)
    }
}

fn padded(len: usize) -> Result<usize> {
    Ok(len.checked_add(7).ok_or(Error::Limit)? & !7)
}
pub(super) fn full_size(manifest: &Manifest) -> Result<usize> {
    manifest.values().try_fold(PREFIX, |size, c| {
        size.checked_add(DESCRIPTOR)
            .and_then(|n| n.checked_add(padded(c.len).ok()?))
            .ok_or(Error::Limit)
    })
}

/// Validate the entire manifest before the caller updates any live indexes.
pub(super) fn decode(
    bytes: &[u8],
    object: ObjectKey,
    revision: Revision,
    base: usize,
    prior: &Manifests,
    limits: Limits,
) -> Result<(Manifest, bool)> {
    if bytes.len() < PREFIX
        || u64_at(bytes, 0) != 1
        || u64_at(bytes, 8) > MAX_COMPONENTS as u64
        || u64_at(bytes, 16) > 1
    {
        return Err(Error::Corrupt("component manifest header"));
    }
    let count = u64_at(bytes, 8) as usize;
    let mut position = PREFIX;
    let mut manifest = Manifest::new();
    let mut previous = None;
    for _ in 0..count {
        let descriptor = bytes
            .get(position..position.checked_add(DESCRIPTOR).ok_or(Error::Limit)?)
            .ok_or(Error::Corrupt("component descriptor"))?;
        position += DESCRIPTOR;
        let id = ComponentId::new(u64_at(descriptor, 0));
        let origin = Revision::new(u64_at(descriptor, 8));
        let len = usize::try_from(u64_at(descriptor, 16)).map_err(|_| Error::Limit)?;
        if previous.is_some_and(|p| p >= id) {
            return Err(Error::Corrupt("component ID order"));
        }
        previous = Some(id);
        let chunk = if origin == Revision::INITIAL {
            let end = position.checked_add(padded(len)?).ok_or(Error::Limit)?;
            let payload = bytes
                .get(position..end)
                .ok_or(Error::Corrupt("component payload"))?;
            if payload[len..].iter().any(|b| *b != 0) {
                return Err(Error::Corrupt("component padding"));
            }
            let chunk = Chunk {
                origin: revision,
                offset: base.checked_add(position).ok_or(Error::Limit)?,
                len,
            };
            position = end;
            chunk
        } else {
            if origin >= revision {
                return Err(Error::Corrupt("component forward reference"));
            }
            let chunk = prior
                .get(&(object, origin))
                .and_then(|m| m.get(&id))
                .ok_or(Error::Corrupt("missing component origin"))?;
            if chunk.origin != origin || chunk.len != len {
                return Err(Error::Corrupt("component origin mismatch"));
            }
            *chunk
        };
        manifest.insert(id, chunk);
    }
    if position != bytes.len() {
        return Err(Error::Corrupt("component trailing bytes"));
    }
    if full_size(&manifest)? > limits.max_entry_bytes {
        return Err(Error::Limit);
    }
    Ok((manifest, u64_at(bytes, 16) == 1))
}

fn encode<'a>(
    parts: impl ExactSizeIterator<Item = (ComponentId, Revision, &'a [u8])>,
    publish: bool,
) -> Result<Vec<u8>> {
    if parts.len() > MAX_COMPONENTS {
        return Err(Error::Limit);
    }
    let mut bytes = vec![0; PREFIX];
    set(&mut bytes, 0, 1);
    set(&mut bytes, 8, parts.len() as u64);
    set(&mut bytes, 16, u64::from(publish));
    for (id, origin, value) in parts {
        for word in [id.get(), origin.get(), value.len() as u64] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        if origin == Revision::INITIAL {
            bytes.extend_from_slice(value);
            bytes.resize(
                bytes
                    .len()
                    .checked_add(padded(value.len())? - value.len())
                    .ok_or(Error::Limit)?,
                0,
            );
        }
    }
    Ok(bytes)
}

pub(super) fn checkpoint_size(
    manifest: &Manifest,
    object: ObjectKey,
    revision: Revision,
    seen: &mut Rebased,
) -> Result<usize> {
    manifest.iter().try_fold(PREFIX, |size, (&id, c)| {
        let first = *seen.entry((object, id, c.origin)).or_insert(revision) == revision;
        size.checked_add(DESCRIPTOR)
            .and_then(|n| n.checked_add(if first { padded(c.len).ok()? } else { 0 }))
            .ok_or(Error::Limit)
    })
}
pub(super) fn checkpoint(
    manifest: &Manifest,
    object: ObjectKey,
    revision: Revision,
    map: &[u8],
    seen: &mut Rebased,
) -> Result<Vec<u8>> {
    encode(
        manifest.iter().map(|(&id, c)| {
            let origin = *seen.entry((object, id, c.origin)).or_insert(revision);
            (
                id,
                if origin == revision {
                    Revision::INITIAL
                } else {
                    origin
                },
                &map[c.offset..c.offset + c.len],
            )
        }),
        false,
    )
}

impl Store {
    /// Stage or atomically stage-and-publish a component revision. Each update
    /// inherits unmentioned components from the current head. At most 256
    /// components/updates are accepted. The fully materialized manifest,
    /// including its descriptors, must fit max_entry_bytes even for small edits.
    /// Some(expected_published) checks publication CAS and publishes in the same
    /// checksummed record and sync. Errors after IO begins require reopening.
    pub fn edit_components(
        &mut self,
        object: ObjectKey,
        expected_head: Revision,
        expected_published: Option<Revision>,
        updates: &[ComponentUpdate<'_>],
    ) -> Result<Revision> {
        self.healthy()?;
        if self.version != 5 {
            return Err(Error::Layout);
        }
        let head = self.head(object);
        if head != expected_head || expected_published.is_some_and(|p| self.published(object) != p)
        {
            return Err(Error::Conflict);
        }
        let revision = head.checked_next().ok_or(Error::Conflict)?;
        let previous = self.components.get(&(object, head));
        if head != Revision::INITIAL && previous.is_none() {
            return Err(Error::Layout);
        }
        if updates.is_empty() || updates.len() > MAX_COMPONENTS {
            return Err(Error::Component("update count must be in 1..=256"));
        }
        let mut parts: BTreeMap<_, _> = previous
            .into_iter()
            .flat_map(|m| m.iter())
            .map(|(&id, c)| (id, (c.origin, &self.map[c.offset..c.offset + c.len])))
            .collect();
        let mut changed = BTreeSet::new();
        for update in updates {
            if !changed.insert(update.id) {
                return Err(Error::Component("duplicate ID"));
            }
            match update.value {
                Some(value) => {
                    if value.len() > self.limits.max_entry_bytes {
                        return Err(Error::Limit);
                    }
                    if !parts.get(&update.id).is_some_and(|(_, old)| *old == value) {
                        parts.insert(update.id, (Revision::INITIAL, value));
                    }
                }
                None => {
                    if parts.remove(&update.id).is_none() {
                        return Err(Error::NotFound);
                    }
                }
            }
        }
        if parts.len() > MAX_COMPONENTS {
            return Err(Error::Limit);
        }
        let logical = parts.values().try_fold(PREFIX, |n, (_, v)| {
            n.checked_add(DESCRIPTOR)
                .and_then(|n| n.checked_add(padded(v.len()).ok()?))
                .ok_or(Error::Limit)
        })?;
        if logical > self.limits.max_entry_bytes {
            return Err(Error::Limit);
        }
        let bytes = encode(
            parts
                .into_iter()
                .map(|(id, (origin, bytes))| (id, origin, bytes)),
            expected_published.is_some(),
        )?;
        let base = self.end.checked_add(RECORD).ok_or(Error::Limit)?;
        let (manifest, _) = decode(
            &bytes,
            object,
            revision,
            base,
            &self.components,
            self.limits,
        )?;
        self.append(8, object, revision, &bytes).inspect_err(|_| {
            self.changes.send_replace(());
        })?;
        self.entries.insert((object, revision), (base, bytes.len()));
        self.components
            .insert((object, revision), Arc::new(manifest));
        self.heads.insert(object, revision);
        if expected_published.is_some() {
            self.published.insert(object, revision);
            self.publications.insert((object, revision));
            self.changes.send_replace(());
        }
        Ok(revision)
    }

    pub fn get_components(&self, object: ObjectKey) -> Result<ComponentSnapshot> {
        self.component_revision(object, self.published(object))
    }
    pub fn component_revision(
        &self,
        object: ObjectKey,
        revision: Revision,
    ) -> Result<ComponentSnapshot> {
        let backing = self.mapped_revision(object, revision)?;
        let manifest = self
            .components
            .get(&(object, revision))
            .ok_or(Error::Layout)?
            .clone();
        Ok(ComponentSnapshot { backing, manifest })
    }
    /// The component equivalent of publication_after, with identical cursor and
    /// retention rules. A cursor does not retain data or advance on read.
    pub fn component_publication_after(
        &self,
        cursor: &PublicationCursor,
    ) -> Result<Option<ComponentPublication>> {
        if !Arc::ptr_eq(&self.cursor_owner, &cursor.owner) {
            return Err(Error::ForeignCursor);
        }
        self.check_publication_position(cursor.object, cursor.after)?;
        self.publications
            .range((
                Excluded((cursor.object, cursor.after)),
                Included((cursor.object, Revision::MAX)),
            ))
            .next()
            .map(|&(_, revision)| {
                Ok(ComponentPublication {
                    snapshot: self.component_revision(cursor.object, revision)?,
                    cursor: PublicationCursor {
                        owner: cursor.owner.clone(),
                        object: cursor.object,
                        after: revision,
                    },
                })
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod crash_tests;

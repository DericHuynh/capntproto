//! Whole-entry and component storage with append-only generations and atomic compaction.
//! Mapped snapshots retain their generation and the stable path lock.
use fs2::FileExt;
use memmap2::{Mmap, MmapOptions};
use ring::digest::{digest, Context, SHA256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
#[cfg(test)]
mod batch_tests;
mod components;
mod history;
pub use components::{ComponentId, ComponentPublication, ComponentSnapshot, ComponentUpdate};
mod io;
mod key;
#[cfg(test)]
mod revision_tests;
pub mod worker;
pub use crate::semantics::Revision;
pub use history::{Publication, PublicationCursor};
use io::{Point, StorageWriter};
pub use key::ObjectKey;
const HEADER: usize = 64;
const RECORD: usize = 80;
const FOOTER: usize = 16;
const MAX_ENTRY: usize = 16 * 1024 * 1024;
const MAX_FILE: usize = 256 * 1024 * 1024;

/// Host-selected bounds, checked before allocation or file mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_entry_bytes: usize,
    pub max_file_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entry_bytes: MAX_ENTRY,
            max_file_bytes: MAX_FILE,
        }
    }
}
impl Limits {
    fn check(self) -> Result<()> {
        if self.max_file_bytes < HEADER
            || self.max_file_bytes > isize::MAX as usize
            || self.max_entry_bytes > isize::MAX as usize - RECORD - FOOTER - 7
        {
            return Err(Error::Limit);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    /// Retain the current publication and every draft that can still be published.
    Publishable,
    /// Retain only the head and current publication; explicitly discard older drafts.
    /// Suitable for ledgers which restore their latest head, without publication.
    Latest,
    /// Retain every available publication and all still-publishable drafts.
    /// Previously discarded history remains unavailable and keeps its cursor floor.
    History,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compaction {
    pub before_bytes: usize,
    pub after_bytes: usize,
    pub removed_revisions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompactStage {
    Written,
    Synced,
    Renamed,
    DirectorySynced,
}
#[derive(Debug, Error)]
pub enum Error {
    #[error("operation does not match the store format or object layout")]
    Layout,
    #[error("invalid component update: {0}")]
    Component(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("corrupt storage: {0}")]
    Corrupt(&'static str),
    #[error("revision conflict")]
    Conflict,
    #[error("object/revision not found")]
    NotFound,
    #[error("storage quota exceeded")]
    Limit,
    #[error("compaction requires a file without hard-link aliases")]
    LinkedFile,
    #[error("publication cursor is not a published revision")]
    InvalidCursor,
    #[error("publication cursor belongs to another open Store")]
    ForeignCursor,
    #[error("publication history expired; minimum valid cursor is {floor}")]
    HistoryExpired { floor: Revision },
}
pub type Result<T> = std::result::Result<T, Error>;
/// One entry in an atomic, single-store commit. Some(expected) also publishes
/// the new revision after comparing the object's current publication.
pub struct Update<'a> {
    pub object: ObjectKey,
    pub expected_head: Revision,
    pub expected_published: Option<Revision>,
    pub value: &'a [u8],
}

// A batch is one checksummed commit record, never a sequence of acknowledged
// records. Offsets point directly into its payload for zero-copy snapshots.
struct BatchEntry {
    object: ObjectKey,
    revision: Revision,
    publish: bool,
    offset: usize,
    len: usize,
}
fn batch_entries(bytes: &[u8]) -> Result<Vec<BatchEntry>> {
    let (count, mut remaining) = bytes
        .split_first_chunk::<8>()
        .ok_or(Error::Corrupt("batch count"))?;
    let count = u64::from_le_bytes(*count);
    if !(1..=16).contains(&count) {
        return Err(Error::Corrupt("batch count"));
    }
    let mut out = Vec::new();
    let mut objects = BTreeSet::new();
    for _ in 0..count {
        let (header, payload) = remaining
            .split_first_chunk::<32>()
            .ok_or(Error::Corrupt("batch header"))?;
        let (words, _) = header.as_chunks::<8>();
        let object = ObjectKey::new(u64::from_le_bytes(words[0]));
        let revision = Revision::new(u64::from_le_bytes(words[1]));
        let publish = u64::from_le_bytes(words[2]);
        let len = usize::try_from(u64::from_le_bytes(words[3])).map_err(|_| Error::Limit)?;
        let offset = bytes.len() - payload.len();
        let padded = len.checked_add(7).ok_or(Error::Limit)? & !7;
        if revision == Revision::INITIAL
            || publish > 1
            || !objects.insert(object)
            || padded > payload.len()
        {
            return Err(Error::Corrupt("batch entry"));
        }
        if payload[len..padded].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("batch padding"));
        }
        out.push(BatchEntry {
            object,
            revision,
            publish: publish == 1,
            offset,
            len,
        });
        remaining = &payload[padded..];
    }
    if !remaining.is_empty() {
        return Err(Error::Corrupt("batch trailing bytes"));
    }
    Ok(out)
}
fn u64_at(b: &[u8], p: usize) -> u64 {
    u64::from_le_bytes(b[p..p + 8].try_into().unwrap())
}
fn set(b: &mut [u8], p: usize, v: u64) {
    b[p..p + 8].copy_from_slice(&v.to_le_bytes());
}
fn record_size(len: usize) -> Result<(usize, usize)> {
    let padded = len.checked_add(7).ok_or(Error::Limit)? & !7;
    Ok((
        padded,
        padded.checked_add(RECORD + FOOTER).ok_or(Error::Limit)?,
    ))
}
fn record(kind: u64, object: ObjectKey, revision: Revision, value: &[u8]) -> Result<Vec<u8>> {
    let (padded, total) = record_size(value.len())?;
    let mut record = vec![0; total];
    record[..8].copy_from_slice(b"RPENTRY2");
    set(&mut record, 8, kind);
    set(&mut record, 16, object.get());
    set(&mut record, 24, revision.get());
    set(&mut record, 32, value.len() as u64);
    let mut hashed = Context::new(&SHA256);
    hashed.update(&record[..40]);
    hashed.update(value);
    record[40..72].copy_from_slice(hashed.finish().as_ref());
    // Independently protect framing before recovery trusts the payload length.
    // This truncated digest detects accidental header corruption; it is not a MAC.
    let header_digest = digest(&SHA256, &record[..72]);
    record[72..80].copy_from_slice(&header_digest.as_ref()[..8]);
    record[RECORD..RECORD + value.len()].copy_from_slice(value);
    record[RECORD + padded..RECORD + padded + 8].copy_from_slice(b"RPCOMMIT");
    set(&mut record, RECORD + padded + 8, total as u64);
    Ok(record)
}
/// Immutable object/revision coordinates and bytes from one mapped generation.
/// Clones retain the mapping and locks; the trusted host must prevent external
/// mutation of mapped files. A snapshot is not an authorization token.
#[derive(Clone)]
pub struct Snapshot {
    map: Arc<Mmap>,
    _lock: Arc<File>,
    _path_lock: Arc<File>,
    offset: usize,
    len: usize,
    object: ObjectKey,
    revision: Revision,
}
impl Snapshot {
    #[must_use]
    pub fn object(&self) -> ObjectKey {
        self.object
    }
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.revision
    }
    pub fn bytes(&self) -> &[u8] {
        &self.map[self.offset..self.offset + self.len]
    }
}
pub struct Store {
    version: u64,
    path: PathBuf,
    path_lock: Arc<File>,
    file: Arc<File>,
    map: Arc<Mmap>,
    entries: BTreeMap<(ObjectKey, Revision), (usize, usize)>,
    components: components::Manifests,
    heads: BTreeMap<ObjectKey, Revision>,
    published: BTreeMap<ObjectKey, Revision>,
    publications: BTreeSet<(ObjectKey, Revision)>,
    history_floors: BTreeMap<ObjectKey, Revision>,
    cursor_owner: Arc<()>,
    changes: tokio::sync::watch::Sender<()>,
    end: usize,
    poisoned: bool,
    limits: Limits,
}
impl Store {
    /// All writers must honor this exclusive lock. External truncation or writes
    /// through another process bypassing the lock violate the mmap safety contract.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(path, Limits::default())
    }
    pub fn open_with_limits(path: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        Self::open_version(path.as_ref(), limits, 4)
    }
    /// Create or open an opt-in version-5 store supporting component objects.
    /// Existing version-4 files are rejected unchanged; migration is explicit.
    pub fn open_components(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_components_with_limits(path, Limits::default())
    }
    pub fn open_components_with_limits(path: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        Self::open_version(path.as_ref(), limits, 5)
    }
    pub fn format_version(&self) -> u64 {
        self.version
    }
    fn open_version(input: &Path, limits: Limits, version: u64) -> Result<Self> {
        limits.check()?;
        // Resolve symlink aliases before taking the stable lock. Open the data
        // inode only AFTER the lock: a waiting opener must not read a generation
        // replaced by compaction while it waited.
        let path = match fs::canonicalize(input) {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if fs::symlink_metadata(input).is_ok() {
                    return Err(e.into());
                }
                let parent = input
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                fs::canonicalize(parent)?.join(input.file_name().ok_or(e)?)
            }
            Err(e) => return Err(e.into()),
        };
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let path_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        path_lock.try_lock_exclusive()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        file.try_lock_exclusive()?;
        if file.metadata()?.len() == 0 {
            let mut h = [0; HEADER];
            h[..8].copy_from_slice(if version == 4 {
                b"RPROTO04"
            } else {
                b"RPROTO05"
            });
            set(&mut h, 8, version);
            set(&mut h, 16, HEADER as u64);
            let checksum = digest(&SHA256, &h[..32]);
            h[32..].copy_from_slice(checksum.as_ref());
            (&file).write_all(&h)?;
            file.sync_all()?;
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                io::directory(parent)?.sync_all()?;
            }
        }
        let len = usize::try_from(file.metadata()?.len()).map_err(|_| Error::Limit)?;
        if len > limits.max_file_bytes {
            return Err(Error::Limit);
        }
        if len < HEADER {
            return Err(Error::Corrupt("file size"));
        }
        // SAFETY: exclusive lifetime lock; this format only appends. No writer
        // mutates mapped committed bytes. Snapshots retain both mapping and lock.
        let map = unsafe { MmapOptions::new().map(&file)? };
        if &map[..8]
            != (if version == 4 {
                b"RPROTO04"
            } else {
                b"RPROTO05"
            })
            || u64_at(&map, 8) != version
        {
            return Err(Error::Corrupt("header/version"));
        }
        if u64_at(&map, 24) != 0 || digest(&SHA256, &map[..32]).as_ref() != &map[32..64] {
            return Err(Error::Corrupt("checkpoint header"));
        }
        let checkpoint_end =
            usize::try_from(u64_at(&map, 16)).map_err(|_| Error::Corrupt("checkpoint size"))?;
        if checkpoint_end < HEADER || checkpoint_end > len || checkpoint_end % 8 != 0 {
            return Err(Error::Corrupt("incomplete checkpoint"));
        }
        let mut entries = BTreeMap::new();
        let mut components = components::Manifests::new();
        let mut heads = BTreeMap::new();
        let mut published = BTreeMap::new();
        let mut publications = BTreeSet::new();
        let mut history_floors = BTreeMap::new();
        let mut end = HEADER;
        let mut checkpoint_key = None;
        let mut checkpoint_publication = None;
        let mut checkpoint_floor = None;
        while end < len {
            let checkpoint = end < checkpoint_end;
            if len - end < RECORD {
                if checkpoint {
                    return Err(Error::Corrupt("incomplete checkpoint"));
                }
                break;
            }
            let h = &map[end..end + RECORD];
            if &h[..8] != b"RPENTRY2" {
                return Err(Error::Corrupt("record magic"));
            }
            if digest(&SHA256, &h[..72]).as_ref()[..8] != h[72..80] {
                return Err(Error::Corrupt("record header checksum"));
            }
            let size = usize::try_from(u64_at(h, 32)).map_err(|_| Error::Corrupt("length"))?;
            if size > limits.max_entry_bytes {
                return Err(Error::Limit);
            }
            let (padded, total) = record_size(size)?;
            if checkpoint && total > checkpoint_end - end {
                return Err(Error::Corrupt("checkpoint boundary"));
            }
            if total > len - end {
                break;
            }
            let tail = &map[end + RECORD + padded..end + total];
            if &tail[..8] != b"RPCOMMIT" || u64_at(tail, 8) != total as u64 {
                return Err(Error::Corrupt("commit footer"));
            }
            let mut hashed = Context::new(&SHA256);
            hashed.update(&h[..40]);
            hashed.update(&map[end + RECORD..end + RECORD + size]);
            if hashed.finish().as_ref() != &h[40..72]
                || map[end + RECORD + size..end + RECORD + padded]
                    .iter()
                    .any(|b| *b != 0)
            {
                return Err(Error::Corrupt("checksum/padding"));
            }
            let kind = u64_at(h, 8);
            let object = ObjectKey::new(u64_at(h, 16));
            let rev = Revision::new(u64_at(h, 24));
            match kind {
                7 if !checkpoint && object.get() == 0 && rev == Revision::INITIAL => {
                    let batch = batch_entries(&map[end + RECORD..end + RECORD + size])?;
                    // Validate every member before changing any index.
                    for &BatchEntry {
                        object, revision, ..
                    } in &batch
                    {
                        if components.contains_key(&(
                            object,
                            heads.get(&object).copied().unwrap_or(Revision::INITIAL),
                        )) || heads
                            .get(&object)
                            .copied()
                            .unwrap_or(Revision::INITIAL)
                            .checked_next()
                            != Some(revision)
                        {
                            return Err(Error::Corrupt("batch revision order"));
                        }
                    }
                    for BatchEntry {
                        object,
                        revision,
                        publish,
                        offset,
                        len,
                    } in batch
                    {
                        entries.insert((object, revision), (end + RECORD + offset, len));
                        heads.insert(object, revision);
                        if publish {
                            published.insert(object, revision);
                            publications.insert((object, revision));
                        }
                    }
                }
                3 | 9 if checkpoint && (kind == 3 || version == 5) => {
                    let key = (object, rev);
                    if rev == Revision::INITIAL
                        || checkpoint_publication.is_some()
                        || checkpoint_floor.is_some()
                        || checkpoint_key.is_some_and(|old| old >= key)
                    {
                        return Err(Error::Corrupt("checkpoint revision order"));
                    }
                    checkpoint_key = Some(key);
                    let prior = heads.get(&object).copied().unwrap_or(Revision::INITIAL);
                    if prior != Revision::INITIAL
                        && components.contains_key(&(object, prior)) != (kind == 9)
                    {
                        return Err(Error::Corrupt("object layout changed"));
                    }
                    if kind == 9 {
                        let (manifest, publish) = components::decode(
                            &map[end + RECORD..end + RECORD + size],
                            object,
                            rev,
                            end + RECORD,
                            &components,
                            limits,
                        )?;
                        if publish {
                            return Err(Error::Corrupt("checkpoint component publication"));
                        }
                        components.insert(key, Arc::new(manifest));
                    }
                    entries.insert(key, (end + RECORD, size));
                    heads.insert(object, rev);
                }
                1 if !checkpoint => {
                    let head = heads.get(&object).copied().unwrap_or(Revision::INITIAL);
                    if components.contains_key(&(object, head)) {
                        return Err(Error::Corrupt("object layout changed"));
                    }
                    if rev
                        != head
                            .checked_next()
                            .ok_or(Error::Corrupt("revision overflow"))?
                    {
                        return Err(Error::Corrupt("revision order"));
                    }
                    entries.insert((object, rev), (end + RECORD, size));
                    heads.insert(object, rev);
                }
                8 if !checkpoint && version == 5 => {
                    let head = heads.get(&object).copied().unwrap_or(Revision::INITIAL);
                    if head.checked_next() != Some(rev)
                        || (head != Revision::INITIAL && !components.contains_key(&(object, head)))
                    {
                        return Err(Error::Corrupt("component revision order/layout"));
                    }
                    let (manifest, publish) = components::decode(
                        &map[end + RECORD..end + RECORD + size],
                        object,
                        rev,
                        end + RECORD,
                        &components,
                        limits,
                    )?;
                    components.insert((object, rev), Arc::new(manifest));
                    entries.insert((object, rev), (end + RECORD, size));
                    heads.insert(object, rev);
                    if publish {
                        published.insert(object, rev);
                        publications.insert((object, rev));
                    }
                }
                2 | 4 | 5
                    if (kind == 2 && !checkpoint) || ((kind == 4 || kind == 5) && checkpoint) =>
                {
                    if checkpoint {
                        if checkpoint_floor.is_some()
                            || checkpoint_publication.is_some_and(
                                |old: (ObjectKey, Revision, u64)| {
                                    (old.0, old.1) >= (object, rev)
                                        || (old.0 == object && (kind == 4 || old.2 == 4))
                                },
                            )
                        {
                            return Err(Error::Corrupt("checkpoint publication order"));
                        }
                        checkpoint_publication = Some((object, rev, kind));
                    }
                    if size != 0
                        || !entries.contains_key(&(object, rev))
                        || rev <= published.get(&object).copied().unwrap_or(Revision::INITIAL)
                    {
                        return Err(Error::Corrupt("publication"));
                    }
                    published.insert(object, rev);
                    publications.insert((object, rev));
                    if kind == 4 {
                        // Compact publication records also establish the history floor.
                        history_floors.insert(object, rev);
                    }
                }
                6 if checkpoint => {
                    if size != 0
                        || rev == Revision::INITIAL
                        || checkpoint_floor.is_some_and(|old| old >= object)
                        || publications
                            .range((object, Revision::INITIAL)..=(object, Revision::MAX))
                            .next()
                            != Some(&(object, rev))
                    {
                        return Err(Error::Corrupt("checkpoint history floor"));
                    }
                    checkpoint_floor = Some(object);
                    history_floors.insert(object, rev);
                }
                _ => return Err(Error::Corrupt("record kind")),
            }
            end += total;
        }
        drop(map);
        if end < len {
            file.set_len(end as u64)?;
        }
        // Complete records and a renamed checkpoint may survive process death
        // before the previous writer's barriers. Stabilize both before serving.
        io::sync(&file, Point::RecoverySync)?;
        io::sync(
            &io::directory(path.parent().unwrap())?,
            Point::RecoveryDirectorySync,
        )?;
        io::point(Point::Recovered)?;
        let file = Arc::new(file);
        // SAFETY: see the exclusive-lock and append-only contract above.
        let map = Arc::new(unsafe { MmapOptions::new().map(&*file)? });
        Ok(Self {
            version,
            path,
            path_lock: Arc::new(path_lock),
            file,
            map,
            entries,
            components,
            heads,
            published,
            publications,
            history_floors,
            cursor_owner: Arc::new(()),
            changes: tokio::sync::watch::channel(()).0,
            end,
            poisoned: false,
            limits,
        })
    }
    pub fn head(&self, object: ObjectKey) -> Revision {
        self.heads
            .get(&object)
            .copied()
            .unwrap_or(Revision::INITIAL)
    }
    pub fn file_bytes(&self) -> usize {
        self.end
    }
    pub fn limits(&self) -> Limits {
        self.limits
    }
    /// Update host policy without changing the file or its persisted format.
    /// Reopen with the same (or larger) limits when using files beyond defaults.
    pub fn set_limits(&mut self, limits: Limits) -> Result<()> {
        self.healthy()?;
        limits.check()?;
        if self.end > limits.max_file_bytes
            || self
                .components
                .values()
                .any(|m| components::full_size(m).map_or(true, |n| n > limits.max_entry_bytes))
            || self
                .entries
                .values()
                .any(|(_, len)| *len > limits.max_entry_bytes)
        {
            return Err(Error::Limit);
        }
        self.limits = limits;
        Ok(())
    }
    pub(crate) fn healthy(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Corrupt("writer must be reopened after IO failure"))
        } else {
            Ok(())
        }
    }
    /// Atomically replace the file with a durable checkpoint. Superseded
    /// revisions become NotFound; already-held snapshots remain valid. All
    /// processes must honor the adjacent .lock file, which must never be removed.
    pub fn compact(&mut self, retention: Retention) -> Result<Compaction> {
        let result = self.compact_inner(retention, |_| Ok(()));
        if result.is_err() {
            self.changes.send_replace(());
        }
        result
    }
    fn compact_inner(
        &mut self,
        retention: Retention,
        mut stage: impl FnMut(CompactStage) -> std::io::Result<()>,
    ) -> Result<Compaction> {
        self.healthy()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if self.file.metadata()?.nlink() != 1 {
                return Err(Error::LinkedFile);
            }
        }
        let retained: Vec<_> = self
            .entries
            .iter()
            .filter(|((object, rev), _)| {
                *rev == self.published(*object)
                    || *rev == self.head(*object)
                    || (retention != Retention::Latest && *rev > self.published(*object))
                    || (retention == Retention::History
                        && self.publications.contains(&(*object, *rev)))
            })
            .map(|(key, value)| (*key, *value))
            .collect();
        let mut after_bytes = HEADER;
        let mut seen = BTreeMap::new();
        for ((object, rev), (_, len)) in &retained {
            let len = if let Some(manifest) = self.components.get(&(*object, *rev)) {
                components::checkpoint_size(manifest, *object, *rev, &mut seen)?
            } else {
                *len
            };
            after_bytes = after_bytes
                .checked_add(record_size(len)?.1)
                .ok_or(Error::Limit)?;
        }
        after_bytes = after_bytes
            .checked_add(
                (if retention == Retention::History {
                    self.publications
                        .len()
                        .checked_add(self.history_floors.len())
                        .ok_or(Error::Limit)?
                } else {
                    self.published.len()
                })
                .checked_mul(RECORD + FOOTER)
                .ok_or(Error::Limit)?,
            )
            .ok_or(Error::Limit)?;
        if after_bytes > self.limits.max_file_bytes {
            return Err(Error::Limit);
        }
        let result = Compaction {
            before_bytes: self.end,
            after_bytes,
            removed_revisions: self.entries.len() - retained.len(),
        };
        let parent = self.path.parent().unwrap(); // Canonical absolute path.
        let temporary = tempfile::Builder::new()
            .prefix(".reproto-compact-")
            .tempfile_in(parent)?;
        temporary.as_file().try_lock_exclusive()?;
        temporary
            .as_file()
            .set_permissions(self.file.metadata()?.permissions())?;
        let mut header = [0; HEADER];
        let history = retention == Retention::History;
        header[..8].copy_from_slice(if self.version == 4 {
            b"RPROTO04"
        } else {
            b"RPROTO05"
        });
        set(&mut header, 8, self.version);
        set(&mut header, 16, after_bytes as u64);
        let checksum = digest(&SHA256, &header[..32]);
        header[32..].copy_from_slice(checksum.as_ref());
        StorageWriter::new(temporary.as_file(), Point::CompactWrite).write_all(&header)?;
        let mut entries = BTreeMap::new();
        let mut components = components::Manifests::new();
        let mut seen = BTreeMap::new();
        let mut offset = HEADER;
        for ((object, rev), (old_offset, len)) in retained {
            let payload;
            let (kind, value) = if let Some(manifest) = self.components.get(&(object, rev)) {
                payload = components::checkpoint(manifest, object, rev, &self.map, &mut seen)?;
                let (manifest, _) = components::decode(
                    &payload,
                    object,
                    rev,
                    offset + RECORD,
                    &components,
                    self.limits,
                )?;
                components.insert((object, rev), Arc::new(manifest));
                (9, payload.as_slice())
            } else {
                (3, &self.map[old_offset..old_offset + len])
            };
            let bytes = record(kind, object, rev, value)?;
            StorageWriter::new(temporary.as_file(), Point::CompactWrite).write_all(&bytes)?;
            entries.insert((object, rev), (offset + RECORD, value.len()));
            offset += bytes.len();
        }
        if history {
            for &(object, rev) in &self.publications {
                StorageWriter::new(temporary.as_file(), Point::CompactWrite).write_all(&record(
                    5,
                    object,
                    rev,
                    &[],
                )?)?;
            }
            for (&object, &floor) in &self.history_floors {
                StorageWriter::new(temporary.as_file(), Point::CompactWrite).write_all(&record(
                    6,
                    object,
                    floor,
                    &[],
                )?)?;
            }
        } else {
            for (&object, &rev) in &self.published {
                StorageWriter::new(temporary.as_file(), Point::CompactWrite).write_all(&record(
                    4,
                    object,
                    rev,
                    &[],
                )?)?;
            }
        }
        stage(CompactStage::Written)?;
        io::sync(temporary.as_file(), Point::CompactSync)?;
        stage(CompactStage::Synced)?;
        io::point(Point::CompactSynced)?;
        // SAFETY: the checkpoint is immutable, locked, and fully written. No
        // committed bytes in this or the old generation will be overwritten.
        let map = Arc::new(unsafe { MmapOptions::new().map(temporary.as_file())? });
        // From replacement until directory sync, failures have an ambiguous
        // durability outcome. Quarantine appends and require reopen/recovery.
        self.poisoned = true;
        io::point(Point::CompactRename)?;
        let file = io::persist(temporary, &self.path)?;
        self.file = Arc::new(file);
        self.map = map;
        self.entries = entries;
        self.components = components;
        if !history {
            self.publications = self.published.iter().map(|(&o, &r)| (o, r)).collect();
            self.history_floors = self.published.clone();
        }
        self.end = after_bytes;
        stage(CompactStage::Renamed)?;
        io::point(Point::CompactRenamed)?;
        io::sync(&io::directory(parent)?, Point::CompactDirectorySync)?;
        stage(CompactStage::DirectorySynced)?;
        io::point(Point::CompactComplete)?;
        self.poisoned = false;
        self.changes.send_replace(());
        Ok(result)
    }
    pub fn objects(&self) -> impl Iterator<Item = ObjectKey> + '_ {
        self.heads.keys().copied()
    }
    pub fn published(&self, object: ObjectKey) -> Revision {
        self.published
            .get(&object)
            .copied()
            .unwrap_or(Revision::INITIAL)
    }
    /// Minimum resumable cursor and latest publication. A floor of zero means
    /// that all publication events are retained. Cursors are object-local.
    pub fn history_bounds(&self, object: ObjectKey) -> Result<(Revision, Revision)> {
        self.healthy()?;
        Ok((
            self.history_floors
                .get(&object)
                .copied()
                .unwrap_or(Revision::INITIAL),
            self.published(object),
        ))
    }
    pub(crate) fn watch_publications(&self) -> tokio::sync::watch::Receiver<()> {
        self.changes.subscribe()
    }
    pub fn get(&self, object: ObjectKey) -> Result<Snapshot> {
        self.revision(object, self.published(object))
    }
    pub fn revision(&self, object: ObjectKey, revision: Revision) -> Result<Snapshot> {
        if self.components.contains_key(&(object, revision)) {
            return Err(Error::Layout);
        }
        self.mapped_revision(object, revision)
    }
    fn mapped_revision(&self, object: ObjectKey, revision: Revision) -> Result<Snapshot> {
        self.healthy()?;
        let &(offset, len) = self
            .entries
            .get(&(object, revision))
            .ok_or(Error::NotFound)?;
        Ok(Snapshot {
            map: self.map.clone(),
            _lock: self.file.clone(),
            _path_lock: self.path_lock.clone(),
            offset,
            len,
            object,
            revision,
        })
    }
    pub fn put(
        &mut self,
        object: ObjectKey,
        expected_head: Revision,
        value: &[u8],
    ) -> Result<Revision> {
        if self.components.contains_key(&(object, self.head(object))) {
            return Err(Error::Layout);
        }
        let current = crate::semantics::Revisions::new(self.head(object), self.published(object))
            .ok_or(Error::Corrupt("revision state"))?;
        let revision = current.stage(expected_head).ok_or(Error::Conflict)?.head();
        let offset = self.append(1, object, revision, value).inspect_err(|_| {
            self.changes.send_replace(());
        })?;
        self.entries
            .insert((object, revision), (offset, value.len()));
        self.heads.insert(object, revision);
        Ok(revision)
    }
    pub fn publish(
        &mut self,
        object: ObjectKey,
        revision: Revision,
        expected_published: Revision,
    ) -> Result<Revision> {
        let current = crate::semantics::Revisions::new(self.head(object), self.published(object))
            .ok_or(Error::Corrupt("revision state"))?;
        let _ = current
            .publish(revision, expected_published)
            .ok_or(Error::Conflict)?;
        if !self.entries.contains_key(&(object, revision)) {
            return Err(Error::NotFound);
        }
        self.append(2, object, revision, &[]).inspect_err(|_| {
            self.changes.send_replace(());
        })?;
        self.published.insert(object, revision);
        self.publications.insert((object, revision));
        self.changes.send_replace(());
        Ok(revision)
    }
    /// Commit up to sixteen distinct objects atomically, syncing before return.
    /// The entire encoded batch is bounded by max_entry_bytes. IO failure has
    /// an uncertain outcome and quarantines the writer until reopen.
    pub fn commit(&mut self, updates: &[Update<'_>]) -> Result<Vec<Revision>> {
        self.healthy()?;
        if updates.is_empty() || updates.len() > 16 {
            return Err(Error::Limit);
        }
        let mut objects = BTreeSet::new();
        let mut size = 8usize;
        for update in updates {
            if self
                .components
                .contains_key(&(update.object, self.head(update.object)))
            {
                return Err(Error::Layout);
            }
            if !objects.insert(update.object)
                || self.head(update.object) != update.expected_head
                || update
                    .expected_published
                    .is_some_and(|p| self.published(update.object) != p)
            {
                return Err(Error::Conflict);
            }
            update.expected_head.checked_next().ok_or(Error::Conflict)?;
            size = size
                .checked_add(32)
                .and_then(|n| n.checked_add(update.value.len().checked_add(7)? & !7))
                .ok_or(Error::Limit)?;
        }
        if size > self.limits.max_entry_bytes {
            return Err(Error::Limit);
        }
        let mut bytes = vec![0; size];
        set(&mut bytes, 0, updates.len() as u64);
        let mut offset = 8;
        for update in updates {
            set(&mut bytes, offset, update.object.get());
            set(
                &mut bytes,
                offset + 8,
                update
                    .expected_head
                    .checked_next()
                    .expect("validated batch head")
                    .get(),
            );
            set(
                &mut bytes,
                offset + 16,
                u64::from(update.expected_published.is_some()),
            );
            set(&mut bytes, offset + 24, update.value.len() as u64);
            bytes[offset + 32..offset + 32 + update.value.len()].copy_from_slice(update.value);
            offset += 32 + ((update.value.len() + 7) & !7);
        }
        let batch = batch_entries(&bytes)?;
        let base = self
            .append(7, ObjectKey::new(0), Revision::INITIAL, &bytes)
            .inspect_err(|_| {
                self.changes.send_replace(());
            })?;
        let mut revisions = Vec::new();
        for BatchEntry {
            object,
            revision,
            publish,
            offset,
            len,
        } in batch
        {
            self.entries
                .insert((object, revision), (base + offset, len));
            self.heads.insert(object, revision);
            if publish {
                self.published.insert(object, revision);
                self.publications.insert((object, revision));
            }
            revisions.push(revision);
        }
        self.changes.send_replace(());
        Ok(revisions)
    }
    fn append(
        &mut self,
        kind: u64,
        object: ObjectKey,
        revision: Revision,
        value: &[u8],
    ) -> Result<usize> {
        self.healthy()?;
        if value.len() > self.limits.max_entry_bytes {
            return Err(Error::Limit);
        }
        let total = record_size(value.len())?.1;
        if self.end.checked_add(total).ok_or(Error::Limit)? > self.limits.max_file_bytes {
            return Err(Error::Limit);
        }
        let record = record(kind, object, revision, value)?;
        self.poisoned = true;
        let mut f = &*self.file;
        f.seek(SeekFrom::Start(self.end as u64))?;
        StorageWriter::new(&self.file, Point::AppendWrite).write_all(&record)?;
        io::point(Point::Appended)?;
        io::sync(&self.file, Point::AppendSync)?;
        io::point(Point::AppendSynced)?;
        // SAFETY: only appended bytes changed; older mappings remain valid.
        self.map = Arc::new(unsafe { MmapOptions::new().map(&*self.file)? });
        let offset = self.end + RECORD;
        self.end += total;
        self.poisoned = false;
        Ok(offset)
    }
}

#[cfg(test)]
mod fault_tests;
#[cfg(test)]
mod recovery_traces;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarantined_store_cannot_be_promoted_to_a_serving_realm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("realm");
        let realm =
            crate::persistence::Realm::open(&path, crate::persistence::Limits::default()).unwrap();
        realm.register_owner([1; 16], [1; 32]).unwrap();
        realm.close();
        let mut store = Store::open(&path).unwrap();
        store.poisoned = true; // An ambiguous previous write has no trusted index.
        assert!(crate::persistence::Realm::from_store(
            store,
            crate::persistence::Limits::default(),
            None
        )
        .is_err());
        assert!(
            crate::persistence::Realm::open(&path, crate::persistence::Limits::default()).is_ok()
        );
    }

    #[test]
    fn interrupted_compaction_selects_a_complete_generation_and_quarantines_uncertain_writes() {
        for retention in [Retention::Latest, Retention::History] {
            for cut in [
                CompactStage::Written,
                CompactStage::Synced,
                CompactStage::Renamed,
                CompactStage::DirectorySynced,
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("objects");
                let mut store = Store::open(&path).unwrap();
                store
                    .put(ObjectKey::new(1), Revision::INITIAL, b"old")
                    .unwrap();
                store
                    .publish(ObjectKey::new(1), Revision::new(1), Revision::INITIAL)
                    .unwrap();
                let held = store.get(ObjectKey::new(1)).unwrap();
                store
                    .put(ObjectKey::new(1), Revision::new(1), b"new")
                    .unwrap();
                store
                    .publish(ObjectKey::new(1), Revision::new(2), Revision::new(1))
                    .unwrap();
                let before = fs::read(&path).unwrap();
                let outcome = store.compact_inner(retention, |stage| {
                    // Readers of the path see either complete generation at each
                    // filesystem boundary; another cooperating writer stays excluded.
                    assert!(Store::open(&path).is_err());
                    if stage == cut {
                        Err(std::io::Error::other("injected IO failure"))
                    } else {
                        Ok(())
                    }
                });
                assert!(outcome.is_err());
                let replaced = matches!(cut, CompactStage::Renamed | CompactStage::DirectorySynced);
                assert_eq!(store.poisoned, replaced);
                assert_eq!(fs::read(&path).unwrap() != before, replaced);
                if replaced {
                    assert!(store
                        .put(ObjectKey::new(1), Revision::new(2), b"blocked")
                        .is_err());
                    assert!(store.compact(Retention::Latest).is_err());
                }
                assert_eq!(held.bytes(), b"old");
                drop(store);
                assert!(Store::open(&path).is_err());
                drop(held);
                let mut recovered = Store::open(&path).unwrap();
                assert_eq!(recovered.head(ObjectKey::new(1)), Revision::new(2));
                assert_eq!(recovered.published(ObjectKey::new(1)), Revision::new(2));
                assert_eq!(recovered.get(ObjectKey::new(1)).unwrap().bytes(), b"new");
                if !replaced || retention == Retention::History {
                    assert_eq!(
                        recovered
                            .publication_after(
                                &recovered.publication_cursor(ObjectKey::new(1), 0).unwrap()
                            )
                            .unwrap()
                            .unwrap()
                            .snapshot()
                            .bytes(),
                        b"old"
                    );
                } else {
                    assert!(matches!(
                        recovered.publication_cursor(ObjectKey::new(1), 0),
                        Err(Error::HistoryExpired { floor }) if floor == Revision::new(2)
                    ));
                }
                assert_eq!(
                    recovered
                        .put(ObjectKey::new(1), Revision::new(2), b"after recovery")
                        .unwrap(),
                    Revision::new(3)
                );
                assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2); // Data plus persistent lock.
            }
        }
    }
}

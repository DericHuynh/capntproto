//! Resumable, capability-authorized transfers into typed ORM entries.
//! Chunk acknowledgments and the final ORM/receipt transaction are durable.
use crate::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    bulk::{Config, Status, Summary},
    bulk_capnp::durable_transfer,
    orm::{ObjectServer, ObjectState},
    storage::{ObjectKey, Revision, Store, Update},
};
use capnp::{
    message::ReaderOptions,
    traits::{HasTypeId, Owned, SetterInput},
};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::{marker::PhantomData, rc::Rc};
mod journal;
pub use journal::JournalId;

const MAGIC: &[u8; 8] = b"RPBULK01";
const MAX_METADATA: usize = 4096;
fn failure(error: impl std::fmt::Display) -> capnp::Error {
    capnp::Error::failed(error.to_string())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Phase {
    Receiving,
    Complete,
    Canceled,
    Failed,
}
impl Phase {
    fn status(self) -> Status {
        match self {
            Self::Receiving => Status::Receiving,
            Self::Complete => Status::Complete,
            Self::Canceled => Status::Canceled,
            Self::Failed => Status::Failed,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Metadata {
    config: Config,
    object: ObjectId,
    generation: ObjectGeneration,
    schema: u64,
    expected_head: Revision,
    expected_published: Revision,
    sha256: [u8; 32],
    progress: Summary,
    phase: Phase,
    revision: Revision,
}
fn encode(meta: &Metadata, chunk: &[u8]) -> capnp::Result<Vec<u8>> {
    let header = serde_json::to_vec(meta).map_err(failure)?;
    if header.len() > MAX_METADATA {
        return Err(failure("bulk metadata limit"));
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(chunk);
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> capnp::Result<(Metadata, &[u8])> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return Err(failure("invalid bulk journal"));
    }
    let size =
        usize::try_from(u64::from_le_bytes(bytes[8..16].try_into().unwrap())).map_err(failure)?;
    if size > MAX_METADATA || size > bytes.len() - 16 {
        return Err(failure("invalid bulk metadata length"));
    }
    let meta: Metadata = serde_json::from_slice(&bytes[16..16 + size]).map_err(failure)?;
    if meta.expected_published > meta.expected_head
        || meta.expected_head == Revision::MAX
        || meta.progress.bytes > meta.config.length()
        || meta.progress.chunks > u64::from(meta.config.max_chunks())
        || meta.progress.bytes < meta.progress.chunks
        || meta.progress.bytes > meta.progress.chunks * u64::from(meta.config.max_chunk_bytes())
        || (meta.phase == Phase::Complete
            && (meta.progress.bytes != meta.config.length()
                || Some(meta.revision) != meta.expected_head.checked_next()))
        || (meta.phase != Phase::Complete && meta.revision != Revision::INITIAL)
    {
        return Err(failure("inconsistent bulk metadata"));
    }
    Ok((meta, &bytes[16 + size..]))
}

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub config: Config,
    pub progress: Summary,
    pub status: Status,
    /// The committed ORM revision, or zero before completion.
    pub revision: Revision,
    pub sha256: [u8; 32],
}
impl From<Metadata> for Checkpoint {
    fn from(m: Metadata) -> Self {
        Self {
            config: m.config,
            progress: m.progress,
            status: m.phase.status(),
            revision: m.revision,
            sha256: m.sha256,
        }
    }
}

struct Service<T> {
    state: Rc<ObjectState>,
    journal: JournalId,
    grant: Grant,
    marker: PhantomData<T>,
}
/// Host-owned transfer handle. Dropping it preserves its durable checkpoint.
/// Journal IDs are selected by the trusted host in the same Store as the target;
/// they are not bearer credentials or remotely selected object IDs.
pub struct Receiver<T>(Rc<Service<T>>);
impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Owned + Unpin + 'static> Receiver<T>
where
    for<'a> T::Reader<'a>: HasTypeId + SetterInput<T>,
{
    pub fn create(
        state: Rc<ObjectState>,
        journal: JournalId,
        grant: Grant,
        config: Config,
        sha256: [u8; 32],
    ) -> capnp::Result<Self> {
        let receiver = Self::handle(state, journal, grant)?;
        // Bind/validate the target's schema through the ordinary ORM boundary.
        let _ = ObjectServer::<T>::client(receiver.0.state.clone(), receiver.0.grant.clone())?;
        let mut store = receiver.0.state.store().borrow_mut();
        store.healthy().map_err(failure)?;
        if store.head(journal.key()) != Revision::INITIAL {
            return Err(failure("bulk journal already exists"));
        }
        if config
            .length()
            .checked_add((MAX_METADATA + 128) as u64)
            .is_none_or(|n| n > store.limits().max_entry_bytes as u64)
        {
            return Err(failure("bulk value exceeds atomic store entry limit"));
        }
        let object = receiver.0.state.object();
        let meta = Metadata {
            config,
            object,
            generation: receiver.0.grant.generation(),
            schema: <T::Reader<'static> as HasTypeId>::TYPE_ID,
            expected_head: store.head(ObjectKey::from(object)),
            expected_published: store.published(ObjectKey::from(object)),
            sha256,
            progress: Summary {
                bytes: 0,
                chunks: 0,
            },
            phase: Phase::Receiving,
            revision: Revision::INITIAL,
        };
        if meta.expected_head == Revision::MAX {
            return Err(failure("object revision exhausted"));
        }
        Service::<T>::save(&mut store, journal, Revision::INITIAL, &meta, &[])?;
        drop(store);
        Ok(receiver)
    }
    /// Reissue only after the host has authenticated the holder and selected
    /// an authorized target and journal. Generation/schema mismatches fail.
    pub fn resume(state: Rc<ObjectState>, journal: JournalId, grant: Grant) -> capnp::Result<Self> {
        let receiver = Self::handle(state, journal, grant)?;
        {
            let store = receiver.0.state.store().borrow();
            let (_, meta) = receiver.0.load(&store)?;
            if meta.phase == Phase::Receiving {
                receiver.0.prefix(&store, &meta, false)?;
                drop(store);
                let _ =
                    ObjectServer::<T>::client(receiver.0.state.clone(), receiver.0.grant.clone())?;
            }
        }
        Ok(receiver)
    }
    fn handle(state: Rc<ObjectState>, journal: JournalId, grant: Grant) -> capnp::Result<Self> {
        if journal.key() == ObjectKey::from(state.object()) || grant.object() != state.object() {
            return Err(failure("bulk object/journal authority mismatch"));
        }
        let receiver = Self(Rc::new(Service {
            state,
            journal,
            grant,
            marker: PhantomData,
        }));
        receiver.0.authorize()?;
        Ok(receiver)
    }
    pub fn capability(&self) -> durable_transfer::Client {
        capnp_rpc::new_client_from_rc(self.0.clone())
    }
    pub fn checkpoint(&self) -> capnp::Result<Checkpoint> {
        self.0.authorize()?;
        Ok(self.0.load(&self.0.state.store().borrow())?.1.into())
    }
    pub fn write(&self, sequence: u64, data: &[u8]) -> capnp::Result<()> {
        self.0.write_chunk(sequence, data)
    }
    pub fn done(&self) -> capnp::Result<Checkpoint> {
        self.0.complete()
    }
    pub fn cancel(&self) -> capnp::Result<Status> {
        self.0.cancel_transfer()
    }
}
impl<T: Owned> Service<T>
where
    for<'a> T::Reader<'a>: HasTypeId + SetterInput<T>,
{
    fn authorize(&self) -> capnp::Result<()> {
        if !self.grant.allows(Rights::PUT) || !self.grant.allows(Rights::PUBLISH) {
            return Err(failure("bulk transfer authority revoked or insufficient"));
        }
        Ok(())
    }
    fn load(&self, store: &Store) -> capnp::Result<(Revision, Metadata)> {
        store.healthy().map_err(failure)?;
        let head = store.head(self.journal.key());
        if head == Revision::INITIAL || head != store.published(self.journal.key()) {
            return Err(failure("uncommitted bulk journal"));
        }
        let snapshot = store.get(self.journal.key()).map_err(failure)?;
        let (meta, _) = decode(snapshot.bytes())?;
        if meta.object != self.state.object()
            || meta.generation != self.grant.generation()
            || meta.schema != <T::Reader<'static> as HasTypeId>::TYPE_ID
            || head.get()
                != meta.progress.chunks + if meta.phase == Phase::Receiving { 1 } else { 2 }
        {
            return Err(failure("bulk journal identity or revision mismatch"));
        }
        Ok((head, meta))
    }
    fn save(
        store: &mut Store,
        journal: JournalId,
        head: Revision,
        meta: &Metadata,
        data: &[u8],
    ) -> capnp::Result<()> {
        let value = encode(meta, data)?;
        store
            .commit(&[Update {
                object: journal.key(),
                expected_head: head,
                expected_published: Some(head),
                value: &value,
            }])
            .map_err(failure)?;
        Ok(())
    }
    fn prefix(&self, store: &Store, meta: &Metadata, collect: bool) -> capnp::Result<Vec<u8>> {
        let mut value = Vec::new();
        let mut length = 0u64;
        if collect {
            value
                .try_reserve_exact(meta.progress.bytes as usize)
                .map_err(failure)?;
        }
        for sequence in 1..=meta.progress.chunks {
            let snapshot = store
                .revision(self.journal.key(), Revision::new(sequence + 1))
                .map_err(|_| failure("bulk journal prefix was retired"))?;
            let (old, data) = decode(snapshot.bytes())?;
            if old.object != meta.object
                || old.generation != meta.generation
                || old.schema != meta.schema
                || old.config != meta.config
                || old.sha256 != meta.sha256
                || old.expected_head != meta.expected_head
                || old.expected_published != meta.expected_published
                || old.phase != Phase::Receiving
                || old.progress.chunks != sequence
                || data.is_empty()
                || data.len() > meta.config.max_chunk_bytes() as usize
                || old.progress.bytes != length + data.len() as u64
            {
                return Err(failure("inconsistent bulk chunk journal"));
            }
            length += data.len() as u64;
            if collect {
                value.extend_from_slice(data);
            }
        }
        if length != meta.progress.bytes {
            return Err(failure("bulk prefix length mismatch"));
        }
        Ok(value)
    }
    fn write_chunk(&self, sequence: u64, data: &[u8]) -> capnp::Result<()> {
        self.authorize()?;
        let mut store = self.state.store().borrow_mut();
        let (head, mut meta) = self.load(&store)?;
        if !matches!(meta.phase, Phase::Receiving | Phase::Complete) {
            return Err(failure("bulk transfer closed"));
        }
        if sequence > 0 && sequence <= meta.progress.chunks {
            let old = store
                .revision(self.journal.key(), Revision::new(sequence + 1))
                .map_err(failure)?;
            if decode(old.bytes())?.1 == data {
                return Ok(());
            }
            return Err(failure("conflicting bulk chunk retry"));
        }
        if meta.phase != Phase::Receiving
            || sequence != meta.progress.chunks + 1
            || sequence > u64::from(meta.config.max_chunks())
            || data.is_empty()
            || data.len() > meta.config.max_chunk_bytes() as usize
            || meta.progress.bytes + data.len() as u64 > meta.config.length()
        {
            return Err(failure("bulk chunk violates sequence or size limits"));
        }
        // Chunks occupy every published journal revision starting at 2. A
        // retained floor above 2 means a required chunk was explicitly retired.
        // This constant-time check avoids rescanning the payload on every write;
        // resume and completion validate the entire immutable mapped prefix.
        if meta.progress.chunks > 0
            && store.history_bounds(self.journal.key()).map_err(failure)?.0 > Revision::new(2)
        {
            return Err(failure("bulk journal prefix was retired"));
        }
        meta.progress.bytes += data.len() as u64;
        meta.progress.chunks = sequence;
        Self::save(&mut store, self.journal, head, &meta, data)
    }
    fn complete(&self) -> capnp::Result<Checkpoint> {
        self.authorize()?;
        let mut store = self.state.store().borrow_mut();
        let (head, mut meta) = self.load(&store)?;
        if meta.phase == Phase::Complete {
            return Ok(meta.into());
        }
        if meta.phase != Phase::Receiving {
            return Err(failure("bulk transfer closed"));
        }
        if meta.progress.bytes != meta.config.length() {
            return Err(failure("bulk transfer incomplete"));
        }
        let bytes = self.prefix(&store, &meta, true)?;
        let validate = (|| -> capnp::Result<Vec<u8>> {
            if digest(&SHA256, &bytes).as_ref() != meta.sha256 {
                return Err(failure("bulk payload digest mismatch"));
            }
            let mut input = bytes.as_slice();
            let message =
                capnp::serialize::read_message_from_flat_slice(&mut input, ReaderOptions::new())?;
            if !input.is_empty() {
                return Err(failure("bulk payload has trailing messages"));
            }
            let _ = message.get_root::<T::Reader<'_>>()?;
            if message
                .get_root::<capnp::any_pointer::Reader<'_>>()?
                .target_size()?
                .cap_count
                != 0
            {
                return Err(failure("bulk value contains live connection capabilities"));
            }
            let mut value = meta.schema.to_le_bytes().to_vec();
            value.extend_from_slice(&bytes);
            Ok(value)
        })();
        let value = match validate {
            Ok(value) => value,
            Err(error) => {
                meta.phase = Phase::Failed;
                Self::save(&mut store, self.journal, head, &meta, &[])?;
                return Err(error);
            }
        };
        meta.phase = Phase::Complete;
        meta.revision = meta
            .expected_head
            .checked_next()
            .ok_or_else(|| failure("object revision exhausted"))?;
        let receipt = encode(&meta, &[])?;
        store
            .commit(&[
                Update {
                    object: ObjectKey::from(meta.object),
                    expected_head: meta.expected_head,
                    expected_published: Some(meta.expected_published),
                    value: &value,
                },
                Update {
                    object: self.journal.key(),
                    expected_head: head,
                    expected_published: Some(head),
                    value: &receipt,
                },
            ])
            .map_err(failure)?;
        drop(store);
        self.state.notify_publication(meta.revision);
        Ok(meta.into())
    }
    fn cancel_transfer(&self) -> capnp::Result<Status> {
        self.authorize()?;
        let mut store = self.state.store().borrow_mut();
        let (head, mut meta) = self.load(&store)?;
        if meta.phase == Phase::Receiving {
            meta.phase = Phase::Canceled;
            Self::save(&mut store, self.journal, head, &meta, &[])?;
        }
        Ok(meta.phase.status())
    }
}
impl<T: Owned + Unpin + 'static> durable_transfer::Server for Service<T>
where
    for<'a> T::Reader<'a>: HasTypeId + SetterInput<T>,
{
    async fn describe(
        self: Rc<Self>,
        _: durable_transfer::DescribeParams,
        mut out: durable_transfer::DescribeResults,
    ) -> capnp::Result<()> {
        self.authorize()?;
        let (_, meta) = self.load(&self.state.store().borrow())?;
        let mut out = out.get();
        meta.config.write(out.reborrow().init_config());
        out.reborrow()
            .init_progress()
            .set_bytes(meta.progress.bytes);
        out.reborrow()
            .get_progress()?
            .set_chunks(meta.progress.chunks);
        out.set_status(meta.phase.status());
        out.set_revision(meta.revision.get());
        out.set_sha256(&meta.sha256);
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        p: durable_transfer::WriteParams,
        mut out: durable_transfer::WriteResults,
    ) -> capnp::Result<()> {
        let p = p.get()?;
        self.write_chunk(p.get_sequence(), p.get_data()?)?;
        out.get().set_sequence(p.get_sequence());
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: durable_transfer::DoneParams,
        mut out: durable_transfer::DoneResults,
    ) -> capnp::Result<()> {
        let checkpoint = self.complete()?;
        let mut out = out.get();
        out.reborrow()
            .init_summary()
            .set_bytes(checkpoint.progress.bytes);
        out.reborrow()
            .get_summary()?
            .set_chunks(checkpoint.progress.chunks);
        out.set_revision(checkpoint.revision.get());
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: durable_transfer::CancelParams,
        mut out: durable_transfer::CancelResults,
    ) -> capnp::Result<()> {
        out.get().set_status(self.cancel_transfer()?);
        Ok(())
    }
}

/// Upload or resume an immutable, seekable file containing one unpacked Cap'n
/// Proto message of the receiver's declared type. The host reobtains the same
/// transfer capability after connection failure and calls this again. One
/// acknowledged chunk is in flight, bounding memory and satisfying byte credit.
pub async fn upload_file(
    client: durable_transfer::Client,
    file: &mut std::fs::File,
) -> capnp::Result<Checkpoint> {
    use std::io::{Read, Seek, SeekFrom};
    let response = client.describe_request().send().promise.await?;
    let remote = response.get()?;
    let config = Config::read(remote.get_config()?)?;
    let sha256: [u8; 32] = remote.get_sha256()?.try_into().map_err(failure)?;
    let progress = remote.get_progress()?;
    let mut progress = Summary {
        bytes: progress.get_bytes(),
        chunks: progress.get_chunks(),
    };
    let status = remote.get_status()?;
    if progress.bytes > config.length()
        || progress.chunks > u64::from(config.max_chunks())
        || progress.bytes > progress.chunks * u64::from(config.max_chunk_bytes())
        || (progress.chunks > 0 && progress.bytes < progress.chunks)
        || (status == Status::Complete
            && (progress.bytes != config.length() || remote.get_revision() == 0))
        || (status != Status::Complete && remote.get_revision() != 0)
    {
        return Err(failure("invalid durable bulk checkpoint"));
    }
    if !matches!(status, Status::Receiving | Status::Complete) {
        return Err(failure("bulk transfer closed"));
    }
    if file.metadata().map_err(failure)?.len() != config.length() {
        return Err(failure("bulk file length mismatch"));
    }
    file.seek(SeekFrom::Start(0)).map_err(failure)?;
    let mut buffer = vec![0; config.max_chunk_bytes() as usize];
    let mut hash = ring::digest::Context::new(&SHA256);
    let mut remaining = config.length();
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count]).map_err(failure)?;
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    if hash.finish().as_ref() != sha256 {
        return Err(failure("bulk file digest mismatch"));
    }
    file.seek(SeekFrom::Start(progress.bytes))
        .map_err(failure)?;
    while progress.bytes < config.length() {
        if progress.chunks == u64::from(config.max_chunks()) {
            return Err(failure("bulk chunk budget exhausted"));
        }
        let count = (config.length() - progress.bytes).min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count]).map_err(failure)?;
        let sequence = progress.chunks + 1;
        let mut write = client.write_request();
        write.get().set_sequence(sequence);
        write.get().set_data(&buffer[..count]);
        let response = write.send().promise.await?;
        if response.get()?.get_sequence() != sequence {
            return Err(failure("bulk acknowledgment mismatch"));
        }
        progress.bytes += count as u64;
        progress.chunks = sequence;
    }
    let response = client.done_request().send().promise.await?;
    let result = response.get()?;
    let summary = result.get_summary()?;
    if summary.get_bytes() != progress.bytes
        || summary.get_chunks() != progress.chunks
        || result.get_revision() == 0
        || (status == Status::Complete && result.get_revision() != remote.get_revision())
    {
        return Err(failure("bulk completion receipt mismatch"));
    }
    Ok(Checkpoint {
        config,
        progress,
        status: Status::Complete,
        revision: Revision::new(result.get_revision()),
        sha256,
    })
}

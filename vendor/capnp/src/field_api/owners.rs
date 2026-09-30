//! Typed owners keep segment storage and capability-table authority together.
use super::*;
use crate::message::{Allocator, HeapAllocator, Reader, ReaderOptions, ReaderSegments};
use alloc::vec::Vec;
use core::borrow::{Borrow, BorrowMut};

/// Live capability hooks indexed by encoded capability pointers. Serialized
/// bytes carry indices only; applications must supply their matching authority.
pub type CapabilityTable = layout::CapTable;

/// Owns an established typed root. `C` owns or exclusively borrows the capability
/// table through `Borrow` / `BorrowMut`; `A` controls arena allocation. Context
/// implementations must expose the same table and preserve index meanings.
/// Freezing moves both owners without copying or deeply validating the payload.
pub struct Message<
    S: Schema,
    State: MessageState = Mutable,
    A: Allocator = HeapAllocator,
    C: Borrow<CapabilityTable> = CapabilityTable,
> {
    pub(super) storage: crate::message::Builder<A>,
    pub(super) caps: C,
    marker: PhantomData<(S, State)>,
}
pub type FrozenMessage<S, A = HeapAllocator, C = CapabilityTable> = Message<S, Frozen, A, C>;
impl<S: Schema> Message<S> {
    pub fn new() -> Result<Self> {
        Self::with_allocator(HeapAllocator::new())
    }
    /// Start a message with a supplied capability-table owner or exclusive borrow.
    pub fn with_capabilities<C: BorrowMut<CapabilityTable>>(
        caps: C,
    ) -> Result<Message<S, Mutable, HeapAllocator, C>> {
        Message::with_allocator_and_capabilities(HeapAllocator::new(), caps)
    }
}
impl<S: Schema, A: Allocator> Message<S, Mutable, A> {
    pub fn with_allocator(allocator: A) -> Result<Self> {
        Self::with_allocator_and_capabilities(allocator, Vec::new())
    }
}
impl<S: Schema, A: Allocator, C: BorrowMut<CapabilityTable>> Message<S, Mutable, A, C> {
    pub fn with_allocator_and_capabilities(allocator: A, caps: C) -> Result<Self> {
        let mut storage = crate::message::Builder::new(allocator);
        storage
            .init_root::<crate::any_pointer::Builder<'_>>()
            .builder
            .init_struct(S::SIZE);
        Ok(Self {
            storage,
            caps,
            marker: PhantomData,
        })
    }
    pub fn edit(&mut self) -> S::Mut<'_> {
        let root = self
            .storage
            .get_root::<crate::any_pointer::Builder<'_>>()
            .expect("established root");
        let mut raw = root
            .builder
            .get_struct(S::SIZE, None)
            .expect("established root");
        raw.imbue(layout::CapTableBuilder::Plain(self.caps.borrow_mut()));
        raw.into()
    }
    /// Split one message borrow into its root and an arena/context identity token.
    pub fn edit_with_orphans(&mut self) -> (S::Mut<'_>, Orphanage<'_>) {
        let root = self
            .storage
            .get_root::<crate::any_pointer::Builder<'_>>()
            .expect("established root");
        let mut raw = root
            .builder
            .get_struct(S::SIZE, None)
            .expect("established root");
        raw.imbue(layout::CapTableBuilder::Plain(self.caps.borrow_mut()));
        let orphanage = Orphanage {
            identity: raw.identity(),
            lifetime: PhantomData,
        };
        (raw.into(), orphanage)
    }
    /// Mint a fresh invariant brand for an exclusive editing session.
    pub fn scoped_edit<R>(
        &mut self,
        edit: impl for<'brand> FnOnce(EditSession<'_, 'brand, S>) -> R,
    ) -> R {
        let root = self
            .storage
            .get_root::<crate::any_pointer::Builder<'_>>()
            .expect("established root");
        let mut raw = root
            .builder
            .get_struct(S::SIZE, None)
            .expect("established root");
        raw.imbue(layout::CapTableBuilder::Plain(self.caps.borrow_mut()));
        edit(EditSession {
            raw,
            schema: PhantomData,
            brand: PhantomData,
        })
    }
}
impl<S: Schema, A: Allocator, C: Borrow<CapabilityTable>> Message<S, Mutable, A, C> {
    pub fn freeze(self) -> FrozenMessage<S, A, C> {
        Message {
            storage: self.storage,
            caps: self.caps,
            marker: PhantomData,
        }
    }
}
impl<S: Schema, State: MessageState, A: Allocator, C: Borrow<CapabilityTable>>
    Message<S, State, A, C>
{
    pub fn read(&self) -> S::Ref<'_> {
        let root = self
            .storage
            .get_root_as_reader::<crate::any_pointer::Reader<'_>>()
            .expect("established root");
        let mut raw = root.reader.get_struct(None).expect("established root");
        raw.imbue(layout::CapTableReader::Plain(self.caps.borrow()));
        raw.into()
    }
    pub fn size_in_words(&self) -> usize {
        self.storage.size_in_words()
    }
    /// Serialize wire indices, not live hooks. Keep the matching capability
    /// context when using these bytes as a local capability-bearing message.
    pub fn to_vec(&self) -> Vec<u8> {
        crate::serialize::write_message_to_words(&self.storage)
    }
    pub fn write_unpacked<W: crate::io::Write>(&self, writer: W) -> Result<()> {
        crate::serialize::write_message(writer, &self.storage)
    }
    /// Independently copy reachable data and capability hooks into a private context.
    pub fn compact_copy(&self) -> Result<Message<S>> {
        copy_root::<S>(self.read())
    }
    /// Transfer storage and its capability context together. Existing views must
    /// be released first; a frozen owner makes no lasting claim after extraction.
    pub fn into_parts(self) -> (crate::message::Builder<A>, C) {
        (self.storage, self.caps)
    }
    /// Consume this owner as an immutable reader without copying payloads or hooks.
    /// Its new traversal budget starts at this explicit reader boundary.
    pub fn into_reader(
        self,
        limits: ReaderOptions,
    ) -> Result<MessageReader<S, crate::message::Builder<A>, C>> {
        MessageReader::from_segments_with_capabilities(self.storage, limits, self.caps)
    }
}
fn copy_root<S: Schema>(source: S::Ref<'_>) -> Result<Message<S>> {
    use crate::traits::IntoInternalStructReader;
    let mut result = Message::<S>::new()?;
    let mut root = result
        .storage
        .get_root::<crate::any_pointer::Builder<'_>>()?;
    root.builder
        .imbue(layout::CapTableBuilder::Plain(&mut result.caps));
    root.builder
        .set_struct(&source.into_internal_struct_reader(), false)?;
    Ok(result)
}

/// A checked root over application-owned or borrowed segments and capabilities.
/// Segment providers obey `ReaderSegments`' stable-storage contract. Capability
/// contexts expose a consistent, matching table via `Borrow`; they do not resolve
/// authority from bytes. Descendants remain lazily validated with one shared budget.
/// Returned views borrow this owner; extracted clients own their hooks separately.
pub struct MessageReader<S: Schema, R: ReaderSegments, C: Borrow<CapabilityTable> = CapabilityTable>
{
    reader: crate::message::CheckedStructReader<R>,
    caps: C,
    marker: PhantomData<S>,
}
/// A borrowed unpacked frame; payload bytes are not copied.
pub type MessageView<'a, S, C = CapabilityTable> =
    MessageReader<S, crate::serialize::BufferSegments<&'a [u8]>, C>;

impl<S: Schema, R: ReaderSegments> MessageReader<S, R> {
    pub fn from_segments(segments: R, limits: ReaderOptions) -> Result<Self> {
        Self::from_reader(Reader::new(segments, limits))
    }
    /// Retain an existing reader's traversal budget, including prior consumption.
    pub fn from_reader(reader: Reader<R>) -> Result<Self> {
        Self::from_reader_with_capabilities(reader, Vec::new())
    }
}
impl<S: Schema, R: ReaderSegments, C: Borrow<CapabilityTable>> MessageReader<S, R, C> {
    pub fn from_segments_with_capabilities(
        segments: R,
        limits: ReaderOptions,
        caps: C,
    ) -> Result<Self> {
        Self::from_reader_with_capabilities(Reader::new(segments, limits), caps)
    }
    /// Adopt a reader without resetting its limits or copying its backing storage.
    /// Only root shape is checked here. The supplied table is authority from the
    /// application, not a table inferred from the encoded capability indices.
    pub fn from_reader_with_capabilities(reader: Reader<R>, caps: C) -> Result<Self> {
        let reader = crate::message::CheckedStructReader::new(reader)?;
        Ok(Self {
            reader,
            caps,
            marker: PhantomData,
        })
    }
    pub fn read(&self) -> S::Ref<'_> {
        let mut root = self.reader.root();
        root.imbue(layout::CapTableReader::Plain(self.caps.borrow()));
        root.into()
    }
    pub fn compact_copy(&self) -> Result<Message<S>> {
        copy_root::<S>(self.read())
    }
    /// Recover both owners, retaining the reader's consumed traversal budget.
    pub fn into_parts(self) -> (Reader<R>, C) {
        (self.reader.into_reader(), self.caps)
    }
}
impl<'a, S: Schema> MessageView<'a, S> {
    pub fn from_unpacked(bytes: &'a [u8], limits: ReaderOptions) -> Result<Self> {
        Self::from_unpacked_with_capabilities(bytes, limits, Vec::new())
    }
    pub fn from_unpacked_prefix(
        bytes: &'a [u8],
        limits: ReaderOptions,
    ) -> Result<(Self, &'a [u8])> {
        Self::from_unpacked_prefix_with_capabilities(bytes, limits, Vec::new())
    }
}
impl<'a, S: Schema, C: Borrow<CapabilityTable>> MessageView<'a, S, C> {
    pub fn from_unpacked_with_capabilities(
        bytes: &'a [u8],
        limits: ReaderOptions,
        caps: C,
    ) -> Result<Self> {
        let (view, rest) = Self::from_unpacked_prefix_with_capabilities(bytes, limits, caps)?;
        if !rest.is_empty() {
            return Err(Error::from_kind(ErrorKind::TrailingData));
        }
        Ok(view)
    }
    pub fn from_unpacked_prefix_with_capabilities(
        bytes: &'a [u8],
        limits: ReaderOptions,
        caps: C,
    ) -> Result<(Self, &'a [u8])> {
        let mut rest = bytes;
        let reader = crate::serialize::read_message_from_flat_slice(&mut rest, limits)?;
        Ok((Self::from_reader_with_capabilities(reader, caps)?, rest))
    }
}

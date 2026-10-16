//! Bounded construction over one caller-owned segment.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::{
    field_api::{FixedElement, FixedList, FixedStruct, Schema},
    private::{
        arena::{fixed::FixedArena, BuilderArena, ReaderArena},
        layout,
    },
    traits::FromPointerReader,
    Error, ErrorKind, OutputSegments, Result, Word,
};

/// A fallible, allocation-free builder with no heap fallback or legacy editor escape.
///
/// Struct, list, text and data construction use the ordinary wire encoder. Successful
/// replacement consumes fresh words; cleared/replaced objects are reclaimed by
/// [`Self::reset`] or drop. Capacity/length rejection preserves both the published
/// message and the allocation cursor. Recursive copies stage before publication
/// and reclaim failed reservations. Automatic schema upgrades are not exposed.
///
/// Readers and editors borrow this owner, so reset cannot invalidate live views.
/// No method formats allocating diagnostics, even with `alloc` enabled.
/// ```
/// use capnp::{message::FixedBuilder, Word};
/// let mut words = [capnp::word(0,0,0,0,0,0,0,0); 3];
/// let mut message = FixedBuilder::new(&mut words)?;
/// message.try_set_root_text("hello")?;
/// assert!(message.try_set_root_data(&[1; 16]).is_err());
/// assert_eq!(message.get_root_as_reader::<capnp::text::Reader<'_>>()?, "hello");
/// # Ok::<(), capnp::Error>(())
/// ```
/// ```compile_fail,E0502
/// use capnp::message::FixedBuilder;
/// let mut words = [capnp::word(0,0,0,0,0,0,0,0); 3];
/// let mut message = FixedBuilder::new(&mut words).unwrap();
/// message.try_set_root_text("live").unwrap();
/// let text = message.get_root_as_reader::<capnp::text::Reader<'_>>().unwrap();
/// message.reset();
/// assert_eq!(text, "live");
/// ```
pub struct FixedBuilder<'buffer> {
    arena: FixedArena<'buffer>,
}

impl<'buffer> FixedBuilder<'buffer> {
    pub fn new(words: &'buffer mut [Word]) -> Result<Self> {
        Self::from_bytes(Word::words_to_bytes_mut(words))
    }

    /// Check alignment and reserve a zero root word. A trailing partial word is
    /// unused. On construction failure the caller's buffer is untouched. Only
    /// reserved words are zeroed during construction, reset and drop.
    pub fn from_bytes(bytes: &'buffer mut [u8]) -> Result<Self> {
        Ok(Self {
            arena: FixedArena::new(bytes)?,
        })
    }

    pub fn capacity_words(&self) -> u32 {
        self.arena.capacity()
    }
    pub fn remaining_words(&self) -> u32 {
        self.arena.remaining()
    }
    pub fn size_in_words(&self) -> usize {
        self.arena.used() as usize
    }
    pub fn reset(&mut self) {
        self.arena.reset();
    }

    fn root(&mut self) -> layout::PointerBuilder<'_> {
        let (base, _) = self.arena.get_segment_mut(0);
        layout::PointerBuilder::get_root(&mut self.arena, 0, base)
    }

    /// Replace the root with a schema-sized, zero-initialized struct.
    pub fn try_init_root<S: Schema>(&mut self) -> Result<FixedStruct<'_, S>> {
        Ok(FixedStruct::new(self.root().try_init_struct(S::SIZE)?))
    }

    /// Borrow an existing struct without allocating, default-copying or upgrading.
    pub fn try_get_root<S: Schema>(&mut self) -> Result<FixedStruct<'_, S>> {
        Ok(FixedStruct::new(
            self.root().get_struct_without_upgrade(S::SIZE)?,
        ))
    }

    /// Explicitly enlarge a present root without truncation or default copying.
    pub fn try_upgrade_root<S: Schema>(&mut self) -> Result<FixedStruct<'_, S>> {
        Ok(FixedStruct::new(self.root().try_upgrade_struct(S::SIZE)?))
    }

    pub fn try_init_root_list<E: FixedElement>(&mut self, len: usize) -> Result<FixedList<'_, E>> {
        FixedList::init(self.root(), len)
    }

    /// Borrow a present list without copying defaults or upgrading its elements.
    pub fn try_get_root_list<E: FixedElement>(&mut self) -> Result<FixedList<'_, E>> {
        FixedList::edit(self.root())
    }

    pub fn try_upgrade_root_list<E: FixedElement>(&mut self) -> Result<FixedList<'_, E>> {
        FixedList::upgrade(self.root())
    }

    /// Losslessly copy a reader's tree, including fields unknown to its schema.
    /// Traversal/nesting limits belong to the source reader. Capability pointers
    /// are rejected without invoking hooks. Replacing a non-null root consumes
    /// a staging word; an empty root needs only the copied tree's storage.
    /// Failure restores both the previous root and the allocation cursor.
    pub fn try_copy_root(&mut self, source: crate::any_pointer::Reader<'_>) -> Result<()> {
        self.root().stage(|mut p| p.copy_from(source.reader, false))
    }
    /// Copy a wire tree while translating its message-local capability indices.
    /// Descriptor ownership must be admitted separately by the RPC connection.
    /// Missing mappings, invalid input and capacity failure preserve the root
    /// and rewind unpublished reservations; no hosted hooks are invoked.
    pub fn try_copy_root_with_capabilities(
        &mut self,
        source: crate::any_pointer::Reader<'_>,
        mapping: crate::wire_capability::IndexMap<'_>,
    ) -> Result<()> {
        self.root()
            .stage(|mut p| p.copy_wire_from(source.reader, mapping))
    }

    pub fn try_set_root_text(&mut self, value: &str) -> Result<()> {
        let len =
            u32::try_from(value.len()).map_err(|_| Error::from_kind(ErrorKind::LengthOverflow))?;
        self.root()
            .try_init_text(len)?
            .as_bytes_mut()
            .copy_from_slice(value.as_bytes());
        Ok(())
    }

    pub fn try_set_root_data(&mut self, value: &[u8]) -> Result<()> {
        let len =
            u32::try_from(value.len()).map_err(|_| Error::from_kind(ErrorKind::LengthOverflow))?;
        self.root().try_init_data(len)?.copy_from_slice(value);
        Ok(())
    }

    /// Encode a message-local capability index. The RPC publisher must supply
    /// and validate the corresponding descriptor; this does not grant authority.
    pub fn set_root_capability_index(&mut self, index: u32) -> Result<()> {
        self.root().set_wire_capability(index)
    }

    pub fn get_root_as_reader<'a, T: FromPointerReader<'a>>(&'a self) -> Result<T> {
        let (base, _) = self.arena.get_segment(0)?;
        // SAFETY: root word zero is reserved and initialized on construction;
        // its arena stays borrowed for every returned reader and descendant.
        let root = unsafe { layout::PointerReader::get_root(&self.arena, 0, base, i32::MAX) }?;
        T::get_from_pointer(&root, None)
    }

    pub fn get_segments_for_output(&self) -> OutputSegments<'_> {
        OutputSegments::SingleSegment([self.arena.bytes()])
    }

    /// Consume this builder and publish its initialized segment without copying
    /// or clearing it. The returned slice keeps the caller's buffer borrowed;
    /// live readers/editors prevent this operation. Unused capacity is untouched.
    /// This is one raw segment, without a stream framing header. RPC descriptor
    /// ownership and output admission must still be validated by the caller.
    pub fn into_segment(self) -> &'buffer [u8] {
        self.arena.into_bytes()
    }
}

impl super::ReaderSegments for FixedBuilder<'_> {
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        (id == 0).then(|| self.arena.bytes())
    }
    fn len(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests;

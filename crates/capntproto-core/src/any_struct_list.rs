//! Schema-free struct lists, including compatible primitive/pointer lists.
//! Elements borrow their actual data/pointer sections; no schema is synthesized.
use crate::{
    any_list, any_struct,
    private::layout::{ListBuilder, ListReader, PointerBuilder, PointerReader, StructSize},
    traits::{FromPointerBuilder, FromPointerReader, IndexMove, IntoInternalListReader, ListIter},
    Result,
};

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    reader: ListReader<'a>,
}
impl<'a> Reader<'a> {
    pub(crate) fn new(reader: ListReader<'a>) -> Self {
        Self { reader }
    }
    pub fn from_reader(value: impl IntoInternalListReader<'a>) -> Result<Self> {
        any_list::Reader::from_reader(value).get_as_struct_list()
    }
    pub fn len(self) -> u32 {
        self.reader.len()
    }
    pub fn is_empty(self) -> bool {
        self.reader.is_empty()
    }
    /// Panics on an out-of-range index; nesting-limit failures remain errors.
    pub fn get(self, index: u32) -> Result<any_struct::Reader<'a>> {
        assert!(index < self.len());
        Ok(any_struct::Reader::new(
            self.reader.get_struct_element_checked(index)?,
        ))
    }
    pub fn try_get(self, index: u32) -> Option<Result<any_struct::Reader<'a>>> {
        (index < self.len()).then(|| self.get(index))
    }
    pub fn iter(self) -> ListIter<Self, Result<any_struct::Reader<'a>>> {
        ListIter::new(self, self.len())
    }
    pub fn total_size(self) -> Result<crate::MessageSize> {
        self.reader.total_size()
    }
    pub fn as_any_list(self) -> any_list::Reader<'a> {
        any_list::Reader::new(self.reader)
    }
}
impl Default for Reader<'_> {
    fn default() -> Self {
        Self::new(ListReader::new_default_struct_list())
    }
}
impl<'a> IntoInternalListReader<'a> for Reader<'a> {
    fn into_internal_list_reader(self) -> ListReader<'a> {
        self.reader
    }
}
impl<'a> FromPointerReader<'a> for Reader<'a> {
    fn get_from_pointer(
        pointer: &PointerReader<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Self> {
        any_list::Reader::get_from_pointer(pointer, default)?.get_as_struct_list()
    }
}
impl<'a> IndexMove<u32, Result<any_struct::Reader<'a>>> for Reader<'a> {
    fn index_move(&self, index: u32) -> Result<any_struct::Reader<'a>> {
        self.get(index)
    }
}
impl<'a> IntoIterator for Reader<'a> {
    type Item = Result<any_struct::Reader<'a>>;
    type IntoIter = ListIter<Self, Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl crate::traits::SetterInput<crate::any_pointer::Owned> for Reader<'_> {
    fn set_pointer_builder(
        mut pointer: PointerBuilder<'_>,
        input: Self,
        canonicalize: bool,
    ) -> Result<()> {
        pointer.set_list(&input.reader, canonicalize)
    }
}
pub struct Builder<'a> {
    builder: ListBuilder<'a>,
}
impl<'a> Builder<'a> {
    pub(crate) fn new(builder: ListBuilder<'a>) -> Self {
        Self { builder }
    }
    pub fn from_builder(value: impl Into<crate::dynamic_value::Builder<'a>>) -> Result<Self> {
        any_list::Builder::from_builder(value)?.get_as_struct_list()
    }
    pub fn reborrow(&mut self) -> Builder<'_> {
        Builder::new(self.builder.reborrow())
    }
    pub fn len(&self) -> u32 {
        self.builder.len()
    }
    pub fn is_empty(&self) -> bool {
        self.builder.is_empty()
    }
    pub fn as_reader(&self) -> Reader<'_> {
        Reader::new(self.builder.as_reader())
    }
    pub fn into_reader(self) -> Reader<'a> {
        Reader::new(self.builder.into_reader())
    }
    pub fn into_any_list(self) -> any_list::Builder<'a> {
        any_list::Builder::new(self.builder)
    }
    /// Panics on an out-of-range index. Reborrow the list for sequential edits.
    ///
    /// ```compile_fail
    /// let mut message = capnp::message::Builder::new_default();
    /// let mut list = message.init_root::<capnp::any_pointer::Builder>()
    ///     .init_as_list_of_any_struct(1, 0, 2).unwrap();
    /// let mut first = list.reborrow().get(0);
    /// let mut second = list.reborrow().get(1); // overlapping mutable views
    /// first.get_data_section()[0] = 1;
    /// second.get_data_section()[0] = 2;
    /// ```
    pub fn get(self, index: u32) -> any_struct::Builder<'a> {
        assert!(index < self.len());
        any_struct::Builder::new(self.builder.get_struct_element(index))
    }
    pub fn try_get(self, index: u32) -> Option<any_struct::Builder<'a>> {
        if index < self.len() {
            Some(self.get(index))
        } else {
            None
        }
    }
}
impl<'a> FromPointerBuilder<'a> for Builder<'a> {
    fn init_pointer(pointer: PointerBuilder<'a>, count: u32) -> Self {
        assert!(count < 1 << 30, "struct-list count exceeds wire limit");
        Self::new(pointer.init_struct_list(
            count,
            StructSize {
                data: 0,
                pointers: 0,
            },
        ))
    }
    fn get_from_pointer(
        pointer: PointerBuilder<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Self> {
        // Preserve primitive storage for virtual elements. A typed getter on
        // the owning pointer may upgrade it when a larger schema is needed.
        any_list::Builder::get_from_pointer(pointer, default)?.get_as_struct_list()
    }
}
#[cfg(feature = "alloc")]
impl<'a> crate::traits::Imbue<'a> for Reader<'a> {
    fn imbue(&mut self, caps: &'a crate::private::layout::CapTable) {
        self.reader
            .imbue(crate::private::layout::CapTableReader::Plain(caps));
    }
}
#[cfg(feature = "alloc")]
impl<'a> crate::traits::ImbueMut<'a> for Builder<'a> {
    fn imbue_mut(&mut self, caps: &'a mut crate::private::layout::CapTable) {
        self.builder
            .imbue(crate::private::layout::CapTableBuilder::Plain(caps));
    }
}

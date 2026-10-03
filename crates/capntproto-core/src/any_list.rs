//! Schema-free list views. Erasing a schema preserves physical element layout,
//! unknown fields and capabilities. Copies use `any_pointer::Owned` as receiver.
//!
//! ```
//! use capnp::{any_list, any_pointer, message, primitive_list};
//! let mut message = message::Builder::new_default();
//! let mut list = message.init_root::<any_pointer::Builder>()
//!     .init_as_any_list(any_list::ElementSize::TwoBytes, 3)?;
//! list.reborrow().get_as::<primitive_list::Owned<u16>>()?.set(1, 42);
//! assert_eq!(list.as_reader().get_as::<primitive_list::Owned<u16>>()?.get(1), 42);
//! # Ok::<(), capnp::Error>(())
//! ```

pub use crate::private::layout::ElementSize;
use crate::{
    dynamic_list, dynamic_value,
    introspect::{Type, TypeVariant},
    private::layout::{ListBuilder, ListReader, PointerBuilder, PointerReader},
    traits::{FromPointerBuilder, FromPointerReader, IntoInternalListReader, Owned},
    Error, ErrorKind, Result,
};

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub(crate) reader: ListReader<'a>,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(reader: ListReader<'a>) -> Self {
        Self { reader }
    }

    /// Erases a native, compiled-dynamic or loaded-schema list reader.
    pub fn from_reader(value: impl IntoInternalListReader<'a>) -> Self {
        Self::new(value.into_internal_list_reader())
    }
    pub fn len(self) -> u32 {
        self.reader.len()
    }
    pub fn is_empty(self) -> bool {
        self.reader.is_empty()
    }
    pub fn get_element_size(self) -> ElementSize {
        self.reader.get_element_size()
    }

    /// Borrows physical data bytes, excluding any inline-composite tag. Like
    /// C++ AnyList, rejects lists whose elements contain pointer slots, even
    /// when those slots are null. Bit-list padding is included in the last byte.
    pub fn get_raw_bytes(self) -> Result<&'a [u8]> {
        self.reader.data_only_bytes()
    }
    pub fn total_size(self) -> Result<crate::MessageSize> {
        self.reader.total_size()
    }
    pub fn equals(self, other: Reader<'_>) -> Result<crate::Equality> {
        crate::raw::list_equals(self, other)
    }

    /// Views compatible storage through a native list type. This checks the
    /// layout, not the validity of pointed-to children. Packed bits cannot be
    /// reinterpreted as other element encodings. Struct fields may read defaults.
    pub fn get_as<T: Owned>(self) -> Result<T::Reader<'a>>
    where
        T::Reader<'a>: dynamic_value::DowncastReader<'a>,
    {
        let TypeVariant::List(element) = T::introspect().which() else {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        };
        Ok(dynamic_value::Reader::List(self.get_as_dynamic(element)?).downcast())
    }
    pub fn get_as_dynamic(self, element_type: Type) -> Result<dynamic_list::Reader<'a>> {
        self.reader
            .check_element_size(element_type.expected_element_size())?;
        if matches!(element_type.which(), TypeVariant::Struct(_)) {
            // Native/dynamic struct-list indexing is infallible internally.
            // Check its extra nesting step before publishing such a view.
            self.reader.check_struct_nesting()?;
        }
        Ok(dynamic_list::Reader::new(self.reader, element_type))
    }
    /// Interpret a list using a resolved loaded element type. Layout and the
    /// extra nesting step for struct elements are checked before publishing the
    /// view. Children are not traversed; missing struct fields read defaults.
    /// Native registration is unnecessary and the supplied brands are retained.
    #[cfg(feature = "alloc")]
    pub fn get_as_loaded<'s: 'a>(
        self,
        element_type: crate::schema_loader::Type<'s>,
    ) -> Result<crate::schema_loader::dynamic::ListReader<'a, 's>> {
        crate::schema_loader::dynamic::ListReader::from_list(self.reader, element_type)
    }
    pub fn get_as_struct_list(self) -> Result<crate::any_struct_list::Reader<'a>> {
        self.reader
            .check_element_size(ElementSize::InlineComposite)?;
        Ok(crate::any_struct_list::Reader::new(self.reader))
    }
}
impl Default for Reader<'_> {
    fn default() -> Self {
        Self::new(ListReader::new_default())
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
        Ok(Self::new(pointer.get_any_list(default)?))
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
    pub(crate) builder: ListBuilder<'a>,
}
impl<'a> Builder<'a> {
    pub(crate) fn new(builder: ListBuilder<'a>) -> Self {
        Self { builder }
    }
    /// Erases a native or compiled-dynamic list builder without copying.
    pub fn from_builder(value: impl Into<dynamic_value::Builder<'a>>) -> Result<Self> {
        match value.into() {
            dynamic_value::Builder::List(value) => Ok(Self::new(value.builder)),
            _ => Err(Error::from_kind(ErrorKind::TypeMismatch)),
        }
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
    pub fn get_element_size(&self) -> ElementSize {
        self.builder.get_element_size()
    }
    pub fn as_reader(&self) -> Reader<'_> {
        Reader::new(self.builder.as_reader())
    }
    pub fn into_reader(self) -> Reader<'a> {
        Reader::new(self.builder.into_reader())
    }
    pub fn equals(&self, other: Reader<'_>) -> Result<crate::Equality> {
        self.as_reader().equals(other)
    }

    /// Checked, in-place native list view. Struct layouts must fit both
    /// sections; this never upgrades storage. Use the owning pointer's typed
    /// getter when an upgrade is required.
    pub fn get_as<T: Owned>(self) -> Result<T::Builder<'a>>
    where
        T::Builder<'a>: dynamic_value::DowncastBuilder<'a>,
    {
        let TypeVariant::List(element) = T::introspect().which() else {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        };
        Ok(dynamic_value::Builder::List(self.get_as_dynamic(element)?).downcast())
    }
    pub fn get_as_dynamic(self, element_type: Type) -> Result<dynamic_list::Builder<'a>> {
        self.builder
            .as_reader()
            .check_element_size(element_type.expected_element_size())?;
        if matches!(element_type.which(), TypeVariant::Struct(_)) {
            self.builder
                .check_struct_size(crate::dynamic_struct::struct_size_from_schema(
                    element_type.as_struct_schema()?,
                )?)?;
        }
        Ok(dynamic_list::Builder::new(self.builder, element_type))
    }
    /// Borrow a loaded mutable list view without reallocating. The element
    /// encoding and, for structs, both physical sections must fit. Unknown or
    /// unresolved element types are rejected. Children and capabilities are not
    /// read or resolved by the cast.
    ///
    /// ```compile_fail
    /// use capnp::{any_list, schema_loader::{dynamic, Type}};
    /// fn escape<'a, 's: 'a>(value: any_list::Builder<'a>, ty: Type<'s>)
    ///     -> dynamic::ListBuilder<'static, 'static> {
    ///     value.get_as_loaded(ty).unwrap()
    /// }
    /// ```
    #[cfg(feature = "alloc")]
    pub fn get_as_loaded<'s: 'a>(
        self,
        element_type: crate::schema_loader::Type<'s>,
    ) -> Result<crate::schema_loader::dynamic::ListBuilder<'a, 's>> {
        crate::schema_loader::dynamic::ListBuilder::from_list(self.builder, element_type)
    }
    pub fn get_as_struct_list(self) -> Result<crate::any_struct_list::Builder<'a>> {
        self.builder
            .as_reader()
            .check_element_size(ElementSize::InlineComposite)?;
        Ok(crate::any_struct_list::Builder::new(self.builder))
    }
}
impl<'a> FromPointerBuilder<'a> for Builder<'a> {
    fn init_pointer(pointer: PointerBuilder<'a>, count: u32) -> Self {
        assert!(count < 1 << 29, "list count exceeds wire limit");
        Self::new(pointer.init_list(ElementSize::Void, count))
    }
    fn get_from_pointer(
        pointer: PointerBuilder<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Self> {
        Ok(Self::new(pointer.get_any_list(default)?))
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

//! Schema-free views of a struct's data and capability-bearing pointer sections.
//!
//! These views describe physical storage, not a schema. In particular, copying
//! a reader preserves unknown fields. Use `any_pointer::Owned` as the receiver
//! type when passing a reader to a generic message or pointer setter.
//!
//! ```
//! use capnp::{any_pointer, any_struct, message};
//! let mut message = message::Builder::new_default();
//! let mut value = message.init_root::<any_pointer::Builder>().init_as_any_struct(1, 1);
//! value.get_data_section()[0] = 42;
//! value.get_pointer_section().get(0).set_as::<capnp::text::Owned>("hello")?;
//! assert_eq!(value.as_reader().get_data_section()[0], 42);
//! let mut copy = message::Builder::new_default();
//! copy.set_root::<any_pointer::Owned>(value.as_reader())?;
//! assert_eq!(copy.get_root_as_reader::<any_struct::Reader>()?.get_pointer_section().len(), 1);
//! # Ok::<(), capnp::Error>(())
//! ```

use crate::private::layout::{
    PointerBuilder, PointerReader, StructBuilder, StructReader, StructSize,
};
use crate::traits::{
    FromPointerBuilder, FromPointerReader, HasStructSize, IntoInternalStructReader, OwnedStruct,
};
use crate::{Error, ErrorKind, Result};

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    reader: StructReader<'a>,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(reader: StructReader<'a>) -> Self {
        Self { reader }
    }
    /// Erases a generated, compiled-dynamic, or loaded-schema struct reader.
    pub fn from_reader(value: impl IntoInternalStructReader<'a>) -> Self {
        Self {
            reader: value.into_internal_struct_reader(),
        }
    }

    pub fn get_data_section(self) -> &'a [u8] {
        self.reader.get_data_section_as_blob()
    }

    pub fn get_pointer_section(self) -> crate::any_pointer_list::Reader<'a> {
        crate::any_pointer_list::Reader::new(self.reader.get_pointer_section_as_list())
    }

    pub fn total_size(self) -> Result<crate::MessageSize> {
        self.reader.total_size()
    }

    pub fn equals(self, right: Reader<'_>) -> Result<crate::Equality> {
        crate::raw::struct_equals(self, right)
    }

    /// Interprets fields using a generated schema. Missing storage reads as
    /// schema defaults, just as it does for an older version of a message.
    pub fn get_as<T: OwnedStruct>(self) -> T::Reader<'a> {
        self.reader.into()
    }

    pub fn get_as_dynamic(
        self,
        schema: crate::schema::StructSchema,
    ) -> crate::dynamic_struct::Reader<'a> {
        crate::dynamic_struct::Reader::new(self.reader, schema)
    }

    /// Interpret this storage using a loaded struct schema, without copying or
    /// traversing children. Missing fields read as schema defaults. The view
    /// borrows both the message and schema loader; native registration is not
    /// required.
    ///
    /// ```compile_fail
    /// use capnp::{any_struct, schema_loader::{dynamic, Schema}};
    /// fn escape<'a, 's: 'a>(value: any_struct::Reader<'a>, schema: Schema<'s>)
    ///     -> dynamic::Reader<'a, 'static> {
    ///     value.get_as_loaded(schema).unwrap()
    /// }
    /// ```
    #[cfg(feature = "alloc")]
    pub fn get_as_loaded<'s: 'a>(
        self,
        schema: crate::schema_loader::Schema<'s>,
    ) -> Result<crate::schema_loader::dynamic::Reader<'a, 's>> {
        crate::schema_loader::dynamic::Reader::from_struct(self.reader, schema)
    }

    /// Returns a canonical, single-segment message, including its root pointer.
    /// Capability pointers cannot be canonicalized. Reader limits still apply.
    #[cfg(feature = "alloc")]
    pub fn canonicalize(self) -> Result<alloc::vec::Vec<crate::Word>> {
        let size = self
            .total_size()?
            .word_count
            .checked_add(1)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
        let mut message = crate::message::Builder::new(
            crate::message::HeapAllocator::new().first_segment_words(size),
        );
        message.set_root_canonical::<crate::any_pointer::Owned>(self)?;
        let segments = message.get_segments_for_output();
        let mut words = crate::Word::allocate_zeroed_vec(segments[0].len() / 8);
        crate::Word::words_to_bytes_mut(&mut words).copy_from_slice(segments[0]);
        Ok(words)
    }
}

impl Default for Reader<'_> {
    fn default() -> Self {
        Self {
            reader: StructReader::new_default(),
        }
    }
}

impl<'a> IntoInternalStructReader<'a> for Reader<'a> {
    fn into_internal_struct_reader(self) -> StructReader<'a> {
        self.reader
    }
}

impl<'a> FromPointerReader<'a> for Reader<'a> {
    fn get_from_pointer(
        pointer: &PointerReader<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Self> {
        Ok(Self {
            reader: pointer.get_struct(default)?,
        })
    }
}

impl crate::traits::SetterInput<crate::any_pointer::Owned> for Reader<'_> {
    fn set_pointer_builder(
        mut pointer: PointerBuilder<'_>,
        input: Self,
        canonicalize: bool,
    ) -> Result<()> {
        pointer.set_struct(&input.reader, canonicalize)
    }
}

pub struct Builder<'a> {
    builder: StructBuilder<'a>,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(builder: StructBuilder<'a>) -> Self {
        Self { builder }
    }

    /// Erases a generated or compiled-dynamic struct builder. Other dynamic
    /// value kinds are rejected without modifying their storage.
    pub fn from_builder(value: impl Into<crate::dynamic_value::Builder<'a>>) -> Result<Self> {
        match value.into() {
            crate::dynamic_value::Builder::Struct(value) => Ok(Self::new(value.builder)),
            _ => Err(Error::from_kind(ErrorKind::TypeMismatch)),
        }
    }

    pub fn reborrow(&mut self) -> Builder<'_> {
        Builder::new(self.builder.reborrow())
    }

    pub fn as_reader(&self) -> Reader<'_> {
        Reader {
            reader: self.builder.as_reader(),
        }
    }

    pub fn into_reader(self) -> Reader<'a> {
        Reader {
            reader: self.builder.into_reader(),
        }
    }

    /// Mutable data bytes, excluding all pointer slots. Borrowing these bytes
    /// prevents simultaneous access through this builder or its other views.
    ///
    /// ```compile_fail
    /// let mut message = capnp::message::Builder::new_default();
    /// let mut value = message.init_root::<capnp::any_pointer::Builder>()
    ///     .init_as_any_struct(1, 1);
    /// let data = value.get_data_section();
    /// let pointers = value.get_pointer_section(); // overlapping mutable borrows
    /// data[0] = 1;
    /// pointers.get(0).clear();
    /// ```
    pub fn get_data_section(&mut self) -> &mut [u8] {
        self.builder.get_data_section_as_blob()
    }

    pub fn get_pointer_section(&mut self) -> crate::any_pointer_list::Builder<'_> {
        crate::any_pointer_list::Builder::new(self.builder.get_pointer_section_as_list())
    }

    pub fn equals(&self, right: Reader<'_>) -> Result<crate::Equality> {
        self.as_reader().equals(right)
    }

    fn check_size(&self, size: StructSize) -> Result<()> {
        let reader = self.builder.as_reader();
        if reader.get_data_section_size() < u32::from(size.data) * 64
            || reader.get_pointer_section_size() < size.pointers
        {
            Err(Error::from_kind(ErrorKind::TypeMismatch))
        } else {
            Ok(())
        }
    }

    /// Borrows an existing generated layout without reallocating it. Returns
    /// `TypeMismatch` if either section is too small. To grow storage, obtain a
    /// typed builder from the owning pointer instead.
    pub fn get_as<T: OwnedStruct>(self) -> Result<T::Builder<'a>> {
        self.check_size(T::Builder::STRUCT_SIZE)?;
        Ok(self.builder.into())
    }

    /// Borrow a mutable view using a loaded schema. Both physical sections must
    /// fit the schema; this never upgrades storage. Unknown fields, capability
    /// tables and the supplied generic bindings are retained.
    ///
    /// ```compile_fail
    /// use capnp::{any_struct, schema_loader::Schema};
    /// fn alias<'a, 's: 'a>(mut value: any_struct::Builder<'a>, schema: Schema<'s>) {
    ///     let mut loaded = value.reborrow().get_as_loaded(schema).unwrap();
    ///     value.get_data_section()[0] = 1;
    ///     loaded.clear_named("number").unwrap();
    /// }
    /// ```
    #[cfg(feature = "alloc")]
    pub fn get_as_loaded<'s: 'a>(
        self,
        schema: crate::schema_loader::Schema<'s>,
    ) -> Result<crate::schema_loader::dynamic::Builder<'a, 's>> {
        self.check_size(schema.struct_size()?)?;
        Ok(crate::schema_loader::dynamic::Builder::from_checked_struct(
            self.builder,
            schema,
        ))
    }

    pub fn get_as_dynamic(
        self,
        schema: crate::schema::StructSchema,
    ) -> Result<crate::dynamic_struct::Builder<'a>> {
        self.check_size(crate::dynamic_struct::struct_size_from_schema(schema)?)?;
        Ok(crate::dynamic_struct::Builder::new(self.builder, schema))
    }
}

impl<'a> FromPointerBuilder<'a> for Builder<'a> {
    fn init_pointer(pointer: PointerBuilder<'a>, _length: u32) -> Self {
        Self::new(pointer.init_struct(StructSize {
            data: 0,
            pointers: 0,
        }))
    }

    fn get_from_pointer(
        pointer: PointerBuilder<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Self> {
        Ok(Self::new(pointer.get_struct(
            StructSize {
                data: 0,
                pointers: 0,
            },
            default,
        )?))
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

/// Schema-free pipelining uses the same pointer-field paths as `AnyPointer`.
pub type Pipeline = crate::any_pointer::Pipeline;

//! Dynamically-typed lists.

use crate::dynamic_value;
use crate::introspect::{Type, TypeVariant};
use crate::private::layout::{self, PrimitiveElement};
use crate::traits::{IndexMove, ListIter};
use crate::{Error, ErrorKind, Result};

/// A read-only dynamically-typed list.
#[derive(Copy, Clone)]
pub struct Reader<'a> {
    pub(crate) reader: layout::ListReader<'a>,
    pub(crate) element_type: Type,
}

impl<'a> crate::traits::IntoInternalListReader<'a> for Reader<'a> {
    fn into_internal_list_reader(self) -> layout::ListReader<'a> {
        self.reader
    }
}

impl<'a> From<Reader<'a>> for dynamic_value::Reader<'a> {
    fn from(x: Reader<'a>) -> dynamic_value::Reader<'a> {
        dynamic_value::Reader::List(x)
    }
}

impl<'a> Reader<'a> {
    pub(crate) fn new(reader: layout::ListReader<'a>, element_type: Type) -> Self {
        Self {
            reader,
            element_type,
        }
    }

    pub fn len(&self) -> u32 {
        self.reader.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn element_type(&self) -> Type {
        self.element_type
    }

    pub fn get(self, index: u32) -> Result<crate::dynamic_value::Reader<'a>> {
        assert!(index < self.reader.len());
        match self.element_type.which() {
            TypeVariant::Void => Ok(dynamic_value::Reader::Void),
            TypeVariant::Bool => Ok(dynamic_value::Reader::Bool(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Int8 => Ok(dynamic_value::Reader::Int8(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Int16 => Ok(dynamic_value::Reader::Int16(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Int32 => Ok(dynamic_value::Reader::Int32(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Int64 => Ok(dynamic_value::Reader::Int64(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::UInt8 => Ok(dynamic_value::Reader::UInt8(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::UInt16 => Ok(dynamic_value::Reader::UInt16(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::UInt32 => Ok(dynamic_value::Reader::UInt32(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::UInt64 => Ok(dynamic_value::Reader::UInt64(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Float32 => Ok(dynamic_value::Reader::Float32(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Float64 => Ok(dynamic_value::Reader::Float64(PrimitiveElement::get(
                &self.reader,
                index,
            ))),
            TypeVariant::Enum(e) => Ok(dynamic_value::Enum::new(
                PrimitiveElement::get(&self.reader, index),
                e.into(),
            )
            .into()),
            TypeVariant::Text => Ok(dynamic_value::Reader::Text(
                self.reader.get_pointer_element(index).get_text(None)?,
            )),
            TypeVariant::Data => Ok(dynamic_value::Reader::Data(
                self.reader.get_pointer_element(index).get_data(None)?,
            )),
            TypeVariant::List(element_type) => Ok(Reader {
                reader: self
                    .reader
                    .get_pointer_element(index)
                    .get_list(element_type.expected_element_size(), None)?,
                element_type,
            }
            .into()),
            TypeVariant::Struct(_) => {
                let r = self.reader.get_struct_element(index);
                Ok(dynamic_value::Reader::Struct(
                    crate::dynamic_struct::Reader::new(r, self.element_type.as_struct_schema()?),
                ))
            }
            TypeVariant::AnyPointer => {
                Ok(crate::any_pointer::Reader::new(self.reader.get_pointer_element(index)).into())
            }
            TypeVariant::Capability | TypeVariant::Interface(_) => Ok(
                dynamic_value::Reader::Capability(dynamic_value::Capability::from_pointer(
                    self.reader.get_pointer_element(index),
                    self.element_type,
                )?),
            ),
        }
    }

    pub fn iter(self) -> ListIter<Reader<'a>, Result<dynamic_value::Reader<'a>>> {
        ListIter::new(self, self.len())
    }
}

impl<'a> IndexMove<u32, Result<dynamic_value::Reader<'a>>> for Reader<'a> {
    fn index_move(&self, index: u32) -> Result<dynamic_value::Reader<'a>> {
        self.get(index)
    }
}

impl<'a> ::core::iter::IntoIterator for Reader<'a> {
    type Item = Result<dynamic_value::Reader<'a>>;
    type IntoIter = ListIter<Reader<'a>, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A mutable dynamically-typed list.
pub struct Builder<'a> {
    pub(crate) builder: layout::ListBuilder<'a>,
    pub(crate) element_type: Type,
}

impl<'a> From<Builder<'a>> for dynamic_value::Builder<'a> {
    fn from(x: Builder<'a>) -> dynamic_value::Builder<'a> {
        dynamic_value::Builder::List(x)
    }
}

impl<'a> Builder<'a> {
    /// Split this list borrow into an editor and a message identity token.
    #[cfg(feature = "alloc")]
    pub fn with_orphanage(self) -> (Self, crate::dynamic_orphan::Orphanage<'a>) {
        let token = crate::dynamic_orphan::Orphanage::new(self.builder.identity());
        (self, token)
    }

    /// Detach an element. Inline structs allocate a new header and move their
    /// complete contents, including unknown pointer fields, without copying
    /// descendant payloads. Scalars reset to zero.
    #[cfg(feature = "alloc")]
    pub fn disown<'message>(
        &mut self,
        index: u32,
        token: &crate::dynamic_orphan::Orphanage<'message>,
    ) -> Result<crate::dynamic_orphan::Orphan<'message>> {
        use crate::dynamic_orphan::{Orphan, Value};
        token.check(self.builder.identity())?;
        self.check_orphan_index(index)?;
        let value = if matches!(self.element_type.which(), TypeVariant::Struct(_)) {
            Value::Pointer(
                self.builder
                    .reborrow()
                    .get_struct_element(index)
                    .disown_content()?,
            )
        } else if self.element_type.is_pointer_type() {
            if !self
                .builder
                .reborrow()
                .get_pointer_element(index)
                .is_external()?
            {
                self.reborrow().get(index)?;
            }
            Value::Pointer(
                self.builder
                    .reborrow()
                    .get_pointer_element(index)
                    .disown()?,
            )
        } else {
            let value = crate::dynamic_orphan::scalar(self.reborrow().into_reader().get(index)?)?;
            use dynamic_value::Reader as R;
            let zero = match &value {
                R::Void => R::Void,
                R::Bool(_) => R::Bool(false),
                R::Int8(_) => R::Int8(0),
                R::Int16(_) => R::Int16(0),
                R::Int32(_) => R::Int32(0),
                R::Int64(_) => R::Int64(0),
                R::UInt8(_) => R::UInt8(0),
                R::UInt16(_) => R::UInt16(0),
                R::UInt32(_) => R::UInt32(0),
                R::UInt64(_) => R::UInt64(0),
                R::Float32(_) => R::Float32(0.0),
                R::Float64(_) => R::Float64(0.0),
                R::Enum(e) => dynamic_value::Enum::new(0, e.get_schema()).into(),
                _ => unreachable!(),
            };
            self.set(index, zero)?;
            Value::Scalar(value)
        };
        Ok(Orphan::new(self.element_type, token, value))
    }

    /// Adopt a same-message value after checking its type and capacity. On
    /// failure, return the still-owned orphan without changing the element.
    #[cfg(feature = "alloc")]
    pub fn adopt<'message>(
        &mut self,
        index: u32,
        mut orphan: crate::dynamic_orphan::Orphan<'message>,
    ) -> core::result::Result<
        (),
        crate::dynamic_orphan::AdoptError<crate::dynamic_orphan::Orphan<'message>>,
    > {
        let result = (|| {
            use crate::dynamic_orphan::Value;
            self.check_orphan_index(index)?;
            orphan.check(self.element_type, self.builder.identity())?;
            match &mut orphan.inner.value {
                Value::Pointer(object)
                    if matches!(self.element_type.which(), TypeVariant::Struct(_)) =>
                {
                    self.builder
                        .reborrow()
                        .get_struct_element(index)
                        .adopt_content(object)
                }
                Value::Pointer(object) => self
                    .builder
                    .reborrow()
                    .get_pointer_element(index)
                    .adopt(object),
                Value::Scalar(value) => self.set(index, value.clone()),
                Value::Group(_) => Err(Error::from_kind(ErrorKind::TypeMismatch)),
            }
        })();
        result.map_err(|error| crate::dynamic_orphan::AdoptError { error, orphan })
    }

    #[cfg(feature = "alloc")]
    fn check_orphan_index(&self, index: u32) -> Result<()> {
        if index >= self.len() {
            return Err(Error::failed("orphan list index out of bounds".into()));
        }
        if matches!(self.element_type.which(), TypeVariant::AnyPointer) {
            return Err(Error::from_kind(ErrorKind::ListAnyPointerNotSupported));
        }
        Ok(())
    }

    pub(crate) fn new(builder: layout::ListBuilder<'a>, element_type: Type) -> Self {
        Self {
            builder,
            element_type,
        }
    }

    pub fn reborrow(&mut self) -> Builder<'_> {
        Builder {
            builder: self.builder.reborrow(),
            element_type: self.element_type,
        }
    }

    pub fn into_reader(self) -> Reader<'a> {
        Reader {
            reader: self.builder.into_reader(),
            element_type: self.element_type,
        }
    }

    pub fn len(&self) -> u32 {
        self.builder.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn element_type(&self) -> Type {
        self.element_type
    }

    pub fn get(self, index: u32) -> Result<dynamic_value::Builder<'a>> {
        assert!(index < self.builder.len());
        match self.element_type.which() {
            TypeVariant::Void => Ok(dynamic_value::Builder::Void),
            TypeVariant::Bool => Ok(dynamic_value::Builder::Bool(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Int8 => Ok(dynamic_value::Builder::Int8(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Int16 => Ok(dynamic_value::Builder::Int16(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Int32 => Ok(dynamic_value::Builder::Int32(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Int64 => Ok(dynamic_value::Builder::Int64(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::UInt8 => Ok(dynamic_value::Builder::UInt8(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::UInt16 => Ok(dynamic_value::Builder::UInt16(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::UInt32 => Ok(dynamic_value::Builder::UInt32(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::UInt64 => Ok(dynamic_value::Builder::UInt64(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Float32 => Ok(dynamic_value::Builder::Float32(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Float64 => Ok(dynamic_value::Builder::Float64(
                PrimitiveElement::get_from_builder(&self.builder, index),
            )),
            TypeVariant::Enum(e) => Ok(dynamic_value::Enum::new(
                PrimitiveElement::get_from_builder(&self.builder, index),
                e.into(),
            )
            .into()),
            TypeVariant::Text => Ok(dynamic_value::Builder::Text(
                self.builder.get_pointer_element(index).get_text(None)?,
            )),
            TypeVariant::Data => Ok(dynamic_value::Builder::Data(
                self.builder.get_pointer_element(index).get_data(None)?,
            )),
            TypeVariant::List(element_type) => Ok(Builder {
                builder: {
                    let pointer = self.builder.get_pointer_element(index);
                    match element_type.which() {
                        TypeVariant::Struct(schema) => pointer.get_struct_list(
                            crate::dynamic_struct::struct_size_from_schema(schema.into())?,
                            None,
                        )?,
                        _ => pointer.get_list(element_type.expected_element_size(), None)?,
                    }
                },
                element_type,
            }
            .into()),
            TypeVariant::Struct(_) => {
                let r = self.builder.get_struct_element(index);
                Ok(dynamic_value::Builder::Struct(
                    crate::dynamic_struct::Builder::new(r, self.element_type.as_struct_schema()?),
                ))
            }
            TypeVariant::AnyPointer => Ok(crate::any_pointer::Builder::new(
                self.builder.get_pointer_element(index),
            )
            .into()),
            TypeVariant::Capability | TypeVariant::Interface(_) => Ok(
                dynamic_value::Builder::Capability(dynamic_value::Capability::from_pointer(
                    self.builder.get_pointer_element(index).into_reader(),
                    self.element_type,
                )?),
            ),
        }
    }

    pub fn set(&mut self, index: u32, value: dynamic_value::Reader<'_>) -> Result<()> {
        assert!(index < self.builder.len());
        value.validate_type(self.element_type)?;
        match (self.element_type.which(), value) {
            (TypeVariant::Void, _) => Ok(()),
            (TypeVariant::Bool, dynamic_value::Reader::Bool(b)) => {
                PrimitiveElement::set(&self.builder, index, b);
                Ok(())
            }
            (TypeVariant::Int8, dynamic_value::Reader::Int8(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::Int16, dynamic_value::Reader::Int16(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::Int32, dynamic_value::Reader::Int32(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::Int64, dynamic_value::Reader::Int64(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::UInt8, dynamic_value::Reader::UInt8(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::UInt16, dynamic_value::Reader::UInt16(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::UInt32, dynamic_value::Reader::UInt32(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::UInt64, dynamic_value::Reader::UInt64(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::Float32, dynamic_value::Reader::Float32(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::Float64, dynamic_value::Reader::Float64(x)) => {
                PrimitiveElement::set(&self.builder, index, x);
                Ok(())
            }
            (TypeVariant::Enum(_es), dynamic_value::Reader::Enum(e)) => {
                PrimitiveElement::set(&self.builder, index, e.get_value());
                Ok(())
            }
            (TypeVariant::Text, dynamic_value::Reader::Text(t)) => {
                self.builder
                    .reborrow()
                    .get_pointer_element(index)
                    .set_text(t);
                Ok(())
            }
            (TypeVariant::Data, dynamic_value::Reader::Data(d)) => {
                self.builder
                    .reborrow()
                    .get_pointer_element(index)
                    .set_data(d);
                Ok(())
            }
            (TypeVariant::Struct(_), dynamic_value::Reader::Struct(s)) => self
                .builder
                .reborrow()
                .get_struct_element(index)
                .copy_content_from(&s.reader),
            (TypeVariant::List(_element_type), dynamic_value::Reader::List(list)) => self
                .builder
                .reborrow()
                .get_pointer_element(index)
                .set_list(&list.reader, false),
            (TypeVariant::AnyPointer, _) => {
                Err(Error::from_kind(ErrorKind::ListAnyPointerNotSupported))
            }
            (
                TypeVariant::Capability | TypeVariant::Interface(_),
                dynamic_value::Reader::Capability(cap),
            ) => cap.set_pointer(
                self.builder.reborrow().get_pointer_element(index),
                self.element_type,
            ),
            (_, _) => Err(Error::from_kind(ErrorKind::TypeMismatch)),
        }
    }

    pub fn init(self, index: u32, size: u32) -> Result<dynamic_value::Builder<'a>> {
        assert!(index < self.builder.len());
        match self.element_type.which() {
            TypeVariant::Void
            | TypeVariant::Bool
            | TypeVariant::Int8
            | TypeVariant::Int16
            | TypeVariant::Int32
            | TypeVariant::Int64
            | TypeVariant::UInt8
            | TypeVariant::UInt16
            | TypeVariant::UInt32
            | TypeVariant::UInt64
            | TypeVariant::Float32
            | TypeVariant::Float64
            | TypeVariant::Enum(_)
            | TypeVariant::Struct(_)
            | TypeVariant::Capability
            | TypeVariant::Interface(_) => Err(Error::from_kind(ErrorKind::ExpectedAListOrBlob)),
            TypeVariant::Text => Ok(self
                .builder
                .get_pointer_element(index)
                .init_text(size)
                .into()),
            TypeVariant::Data => Ok(self
                .builder
                .get_pointer_element(index)
                .init_data(size)
                .into()),
            TypeVariant::List(inner_element_type) => match inner_element_type.which() {
                TypeVariant::Struct(rbs) => Ok(Builder::new(
                    self.builder.get_pointer_element(index).init_struct_list(
                        size,
                        crate::dynamic_struct::struct_size_from_schema(rbs.into())?,
                    ),
                    inner_element_type,
                )
                .into()),
                _ => Ok(Builder::new(
                    self.builder
                        .get_pointer_element(index)
                        .init_list(inner_element_type.expected_element_size(), size),
                    inner_element_type,
                )
                .into()),
            },
            TypeVariant::AnyPointer => Err(Error::from_kind(ErrorKind::ListAnyPointerNotSupported)),
        }
    }
}

impl<'a> crate::traits::SetterInput<crate::any_pointer::Owned> for Reader<'a> {
    fn set_pointer_builder<'b>(
        mut pointer: crate::private::layout::PointerBuilder<'b>,
        value: Reader<'a>,
        canonicalize: bool,
    ) -> Result<()> {
        pointer.set_list(&value.reader, canonicalize)
    }
}

impl core::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&crate::dynamic_value::Reader::from(*self), f)
    }
}

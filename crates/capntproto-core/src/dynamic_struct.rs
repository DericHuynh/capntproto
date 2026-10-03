//! Dynamically-typed structs.

use crate::introspect::TypeVariant;
use crate::private::layout;
use crate::schema::{Field, StructSchema};
use crate::schema_capnp::{field, node, value};
use crate::{dynamic_list, dynamic_value};
use crate::{Error, ErrorKind, Result};

/// Selects how dynamic field presence is tested. Inactive union arms are always
/// absent; active groups are always present. Pointer fields use wire nullness in
/// both modes, without traversing the pointer or materializing schema defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HasMode {
    /// Active primitive fields are present, including Void and default values.
    #[default]
    NonNull,
    /// Primitive fields are present only when their encoded bits are nonzero.
    /// Encoding XORs the value with its schema default; floating-point defaults
    /// are compared by bits, preserving signed zero and NaN payload distinctions.
    NonDefault,
}

pub(crate) fn has_wire_field(
    reader: layout::StructReader<'_>,
    offset: usize,
    size: layout::ElementSize,
    mode: HasMode,
) -> bool {
    use layout::ElementSize as E;
    match size {
        E::Pointer | E::InlineComposite => !reader.get_pointer_field(offset).is_null(),
        _ if mode == HasMode::NonNull => true,
        E::Void => false,
        E::Bit => reader.get_bool_field(offset),
        E::Byte => reader.get_data_field::<u8>(offset) != 0,
        E::TwoBytes => reader.get_data_field::<u16>(offset) != 0,
        E::FourBytes => reader.get_data_field::<u32>(offset) != 0,
        E::EightBytes => reader.get_data_field::<u64>(offset) != 0,
    }
}

#[cfg(feature = "alloc")]
pub(crate) fn scalar_is_default(
    actual: dynamic_value::Reader<'_>,
    default: value::Reader<'_>,
) -> Result<bool> {
    use dynamic_value::Reader as V;
    Ok(match (actual, default.which()?) {
        (V::Void, value::Void(())) => true,
        (V::Bool(v), value::Bool(d)) => v == d,
        (V::Int8(v), value::Int8(d)) => v == d,
        (V::Int16(v), value::Int16(d)) => v == d,
        (V::Int32(v), value::Int32(d)) => v == d,
        (V::Int64(v), value::Int64(d)) => v == d,
        (V::UInt8(v), value::Uint8(d)) => v == d,
        (V::UInt16(v), value::Uint16(d)) => v == d,
        (V::UInt32(v), value::Uint32(d)) => v == d,
        (V::UInt64(v), value::Uint64(d)) => v == d,
        (V::Float32(v), value::Float32(d)) => v.to_bits() == d.to_bits(),
        (V::Float64(v), value::Float64(d)) => v.to_bits() == d.to_bits(),
        (V::Enum(v), value::Enum(d)) => v.get_value() == d,
        _ => return Err(Error::from_kind(ErrorKind::FieldAndDefaultMismatch)),
    })
}

fn has_discriminant_value(reader: field::Reader) -> bool {
    reader.get_discriminant_value() != field::NO_DISCRIMINANT
}

pub(crate) fn struct_size_from_schema(schema: StructSchema) -> Result<layout::StructSize> {
    if let node::Struct(s) = schema.proto.which()? {
        Ok(layout::StructSize {
            data: s.get_data_word_count(),
            pointers: s.get_pointer_count(),
        })
    } else {
        Err(Error::from_kind(ErrorKind::NotAStruct))
    }
}

/// A read-only dynamically-typed struct.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub(crate) reader: layout::StructReader<'a>,
    pub(crate) schema: StructSchema,
}

impl<'a> crate::traits::IntoInternalStructReader<'a> for Reader<'a> {
    fn into_internal_struct_reader(self) -> layout::StructReader<'a> {
        self.reader
    }
}

impl<'a> From<Reader<'a>> for dynamic_value::Reader<'a> {
    fn from(x: Reader<'a>) -> dynamic_value::Reader<'a> {
        dynamic_value::Reader::Struct(x)
    }
}

impl<'a> Reader<'a> {
    pub fn new(reader: layout::StructReader<'a>, schema: StructSchema) -> Self {
        Self { reader, schema }
    }

    pub fn total_size(&self) -> crate::Result<crate::MessageSize> {
        self.reader.total_size()
    }

    pub fn get_schema(&self) -> StructSchema {
        self.schema
    }

    pub fn get(self, field: Field) -> Result<dynamic_value::Reader<'a>> {
        self.check_active(field)?;
        self.read_field(field)
    }

    fn check_active(&self, field: Field) -> Result<()> {
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        if has_discriminant_value(field.get_proto())
            && !self
                .which()?
                .is_some_and(|active| active.get_index() == field.get_index())
        {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        Ok(())
    }

    // Orphan construction reads defaults without a message union selection.
    pub(crate) fn read_field(self, field: Field) -> Result<dynamic_value::Reader<'a>> {
        let ty = field.get_type();
        match field.get_proto().which()? {
            field::Slot(slot) => {
                let offset = slot.get_offset();
                let default_value = slot.get_default_value()?;

                match (ty.which(), default_value.which()?) {
                    (TypeVariant::Void, _) => Ok(dynamic_value::Reader::Void),
                    (TypeVariant::Bool, value::Bool(b)) => Ok(dynamic_value::Reader::Bool(
                        self.reader.get_bool_field_mask(offset as usize, b),
                    )),
                    (TypeVariant::Int8, value::Int8(x)) => Ok(dynamic_value::Reader::Int8(
                        self.reader.get_data_field_mask::<i8>(offset as usize, x),
                    )),
                    (TypeVariant::Int16, value::Int16(x)) => Ok(dynamic_value::Reader::Int16(
                        self.reader.get_data_field_mask::<i16>(offset as usize, x),
                    )),
                    (TypeVariant::Int32, value::Int32(x)) => Ok(dynamic_value::Reader::Int32(
                        self.reader.get_data_field_mask::<i32>(offset as usize, x),
                    )),
                    (TypeVariant::Int64, value::Int64(x)) => Ok(dynamic_value::Reader::Int64(
                        self.reader.get_data_field_mask::<i64>(offset as usize, x),
                    )),
                    (TypeVariant::UInt8, value::Uint8(x)) => Ok(dynamic_value::Reader::UInt8(
                        self.reader.get_data_field_mask::<u8>(offset as usize, x),
                    )),
                    (TypeVariant::UInt16, value::Uint16(x)) => Ok(dynamic_value::Reader::UInt16(
                        self.reader.get_data_field_mask::<u16>(offset as usize, x),
                    )),
                    (TypeVariant::UInt32, value::Uint32(x)) => Ok(dynamic_value::Reader::UInt32(
                        self.reader.get_data_field_mask::<u32>(offset as usize, x),
                    )),
                    (TypeVariant::UInt64, value::Uint64(x)) => Ok(dynamic_value::Reader::UInt64(
                        self.reader.get_data_field_mask::<u64>(offset as usize, x),
                    )),
                    (TypeVariant::Float32, value::Float32(x)) => {
                        Ok(dynamic_value::Reader::Float32(
                            self.reader
                                .get_data_field_mask::<f32>(offset as usize, x.to_bits()),
                        ))
                    }
                    (TypeVariant::Float64, value::Float64(x)) => {
                        Ok(dynamic_value::Reader::Float64(
                            self.reader
                                .get_data_field_mask::<f64>(offset as usize, x.to_bits()),
                        ))
                    }
                    (TypeVariant::Enum(schema), value::Enum(d)) => Ok(dynamic_value::Enum::new(
                        self.reader.get_data_field_mask::<u16>(offset as usize, d),
                        schema.into(),
                    )
                    .into()),
                    (TypeVariant::Text, dval) => {
                        let p = self.reader.get_pointer_field(offset as usize);
                        // If the type is a generic, then the default value
                        // is always an empty AnyPointer. Ignore that case.
                        let t1 = if let (true, value::Text(t)) = (p.is_null(), dval) {
                            t?
                        } else {
                            p.get_text(None)?
                        };
                        Ok(dynamic_value::Reader::Text(t1))
                    }
                    (TypeVariant::Data, dval) => {
                        let p = self.reader.get_pointer_field(offset as usize);
                        // If the type is a generic, then the default value
                        // is always an empty AnyPointer. Ignore that case.
                        let d1 = if let (true, value::Data(d)) = (p.is_null(), dval) {
                            d?
                        } else {
                            p.get_data(None)?
                        };
                        Ok(dynamic_value::Reader::Data(d1))
                    }
                    (TypeVariant::Struct(_), dval) => {
                        let p = self.reader.get_pointer_field(offset as usize);
                        // If the type is a generic, then the default value
                        // is always an empty AnyPointer. Ignore that case.
                        let p1 = if let (true, value::Struct(s)) = (p.is_null(), dval) {
                            s.reader
                        } else {
                            p
                        };
                        let r = p1.get_struct(None)?;
                        Ok(Reader::new(r, ty.as_struct_schema()?).into())
                    }
                    (TypeVariant::List(element_type), dval) => {
                        let p = self.reader.get_pointer_field(offset as usize);
                        // If the type is a generic, then the default value
                        // is always an empty AnyPointer. Ignore that case.
                        let p1 = if let (true, value::List(l)) = (p.is_null(), dval) {
                            l.reader
                        } else {
                            p
                        };
                        let l = p1.get_list(element_type.expected_element_size(), None)?;
                        Ok(dynamic_list::Reader::new(l, element_type).into())
                    }
                    (TypeVariant::AnyPointer, value::AnyPointer(a)) => {
                        let p = self.reader.get_pointer_field(offset as usize);
                        let a1 = if p.is_null() {
                            a
                        } else {
                            crate::any_pointer::Reader::new(p)
                        };
                        Ok(dynamic_value::Reader::AnyPointer(a1))
                    }
                    (
                        TypeVariant::Capability | TypeVariant::Interface(_),
                        value::Interface(()) | value::AnyPointer(_),
                    ) => Ok(dynamic_value::Reader::Capability(
                        dynamic_value::Capability::from_pointer(
                            self.reader.get_pointer_field(offset as usize),
                            ty,
                        )?,
                    )),
                    _ => Err(Error::from_kind(ErrorKind::FieldAndDefaultMismatch)),
                }
            }
            field::Group(_) => {
                if let TypeVariant::Struct(_) = ty.which() {
                    Ok(Reader::new(self.reader, ty.as_struct_schema()?).into())
                } else {
                    Err(Error::from_kind(ErrorKind::GroupFieldButTypeIsNotStruct))
                }
            }
        }
    }

    /// Gets the field with the given name.
    pub fn get_named(self, field_name: &str) -> Result<dynamic_value::Reader<'a>> {
        self.get(self.schema.get_field_by_name(field_name)?)
    }

    /// If this struct has union fields, returns the one that is currently active.
    /// Otherwise, returns None.
    pub fn which(&self) -> Result<Option<Field>> {
        let node::Struct(st) = self.schema.get_proto().which()? else {
            return Err(Error::from_kind(ErrorKind::NotAStruct));
        };
        if st.get_discriminant_count() == 0 {
            Ok(None)
        } else {
            let discrim = self
                .reader
                .get_data_field::<u16>(st.get_discriminant_offset() as usize);
            self.schema.get_field_by_discriminant(discrim)
        }
    }

    /// On a field that is part of a union, returns `true` if the field
    /// is active in the union and is not a null pointer. On non-union fields,
    /// returns `true` if the field is not a null pointer.
    pub fn has(&self, field: Field) -> Result<bool> {
        self.has_with_mode(field, HasMode::NonNull)
    }

    /// Test field presence using the selected wire-level interpretation.
    pub fn has_with_mode(&self, field: Field, mode: HasMode) -> Result<bool> {
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        let proto = field.get_proto();
        if has_discriminant_value(proto) {
            let node::Struct(st) = self.schema.get_proto().which()? else {
                return Err(Error::from_kind(ErrorKind::NotAStruct));
            };

            let discrim = self
                .reader
                .get_data_field::<u16>(st.get_discriminant_offset() as usize);
            if discrim != proto.get_discriminant_value() {
                // Field is not active in the union.
                return Ok(false);
            }
        }
        let slot = match proto.which()? {
            field::Group(_) => return Ok(true),
            field::Slot(s) => s,
        };
        Ok(has_wire_field(
            self.reader,
            slot.get_offset() as usize,
            field.get_type().expected_element_size(),
            mode,
        ))
    }

    pub fn has_named(&self, field_name: &str) -> Result<bool> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.has(field)
    }

    pub fn has_named_with_mode(&self, field_name: &str, mode: HasMode) -> Result<bool> {
        self.has_with_mode(self.schema.get_field_by_name(field_name)?, mode)
    }

    /// Downcasts the `Reader` into a specific struct type. Panics if the
    /// expected type does not match the value.
    pub fn downcast<T: crate::traits::OwnedStruct>(self) -> T::Reader<'a> {
        assert!(self.schema.as_type().loose_equals(T::introspect()));
        self.reader.into()
    }
}

/// A mutable dynamically-typed struct.
pub struct Builder<'a> {
    pub(crate) builder: layout::StructBuilder<'a>,
    pub(crate) schema: StructSchema,
}

impl<'a> From<Builder<'a>> for dynamic_value::Builder<'a> {
    fn from(x: Builder<'a>) -> dynamic_value::Builder<'a> {
        dynamic_value::Builder::Struct(x)
    }
}

impl<'a> Builder<'a> {
    /// Split this message borrow into an editor and an orphan lifetime token.
    #[cfg(feature = "alloc")]
    pub fn with_orphanage(self) -> (Self, crate::dynamic_orphan::Orphanage<'a>) {
        let token = crate::dynamic_orphan::Orphanage::new(self.builder.identity());
        (self, token)
    }

    /// Detach a field, retaining its capabilities and runtime type. Scalar
    /// fields reset to their defaults; pointer fields become null. A union arm
    /// must be active. Groups move their known fields, preserving siblings.
    #[cfg(feature = "alloc")]
    pub fn disown<'message>(
        &mut self,
        field: Field,
        token: &crate::dynamic_orphan::Orphanage<'message>,
    ) -> Result<crate::dynamic_orphan::Orphan<'message>> {
        token.check(self.builder.identity())?;
        self.prepare_disown(field, 0)?;
        self.disown_prepared(field, token)
    }

    #[cfg(feature = "alloc")]
    pub fn disown_named<'message>(
        &mut self,
        name: &str,
        token: &crate::dynamic_orphan::Orphanage<'message>,
    ) -> Result<crate::dynamic_orphan::Orphan<'message>> {
        self.disown(self.schema.get_field_by_name(name)?, token)
    }

    #[cfg(feature = "alloc")]
    fn group_fields(&self) -> Result<alloc::vec::Vec<Field>> {
        let mut fields = alloc::vec::Vec::new();
        let node::Struct(proto) = self.schema.get_proto().which()? else {
            return Err(Error::from_kind(ErrorKind::NotAStruct));
        };
        if proto.get_discriminant_count() != 0 {
            fields.push(
                self.which()?
                    .ok_or_else(|| Error::failed("unknown union arm".into()))?,
            );
        }
        for field in self.schema.get_non_union_fields()? {
            if self.has(field)? {
                fields.push(field);
            }
        }
        Ok(fields)
    }

    #[cfg(feature = "alloc")]
    fn prepare_disown(&mut self, field: Field, depth: usize) -> Result<()> {
        if depth >= 64 {
            return Err(Error::failed("orphan group nesting limit".into()));
        }
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        if has_discriminant_value(field.get_proto())
            && !self
                .which()?
                .is_some_and(|active| active.get_index() == field.get_index())
        {
            return Err(Error::failed("cannot disown an inactive union arm".into()));
        }
        match field.get_proto().which()? {
            field::Slot(slot) if field.get_type().is_pointer_type() => {
                // Match C++: disown materializes a pointer's default through
                // the builder accessor before detaching that value.
                // External data already has a concrete immutable payload;
                // detaching ownership must not request a mutable blob view.
                if !self
                    .builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize)
                    .is_external()?
                {
                    self.reborrow().get(field)?;
                }
                self.builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize)
                    .check_disown()
            }
            field::Slot(_) => {
                crate::dynamic_orphan::scalar(self.reborrow_as_reader().get(field)?)?;
                Ok(())
            }
            field::Group(_) => {
                let mut group = Builder::new(
                    self.builder.reborrow(),
                    field.get_type().as_struct_schema()?,
                );
                for child in group.group_fields()? {
                    group.prepare_disown(child, depth + 1)?;
                }
                Ok(())
            }
        }
    }

    #[cfg(feature = "alloc")]
    fn disown_prepared<'message>(
        &mut self,
        field: Field,
        token: &crate::dynamic_orphan::Orphanage<'message>,
    ) -> Result<crate::dynamic_orphan::Orphan<'message>> {
        use crate::dynamic_orphan::{Orphan, Value};
        let ty = field.get_type();
        let value = match field.get_proto().which()? {
            field::Slot(slot) if ty.is_pointer_type() => Value::Pointer(
                self.builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize)
                    .disown()?,
            ),
            field::Slot(_) => {
                let value = crate::dynamic_orphan::scalar(self.reborrow_as_reader().get(field)?)?;
                self.clear(field)?;
                Value::Scalar(value)
            }
            field::Group(_) => {
                let mut group = Builder::new(self.builder.reborrow(), ty.as_struct_schema()?);
                let mut fields = alloc::vec::Vec::new();
                for child in group.group_fields()? {
                    fields.push((child, group.disown_prepared(child, token)?));
                }
                if let Some(default) = group.schema.get_field_by_discriminant(0)? {
                    group.clear(default)?;
                }
                Value::Group(fields)
            }
        };
        Ok(Orphan::new(ty, token, value))
    }

    #[cfg(feature = "alloc")]
    pub(crate) fn disown_group_fields<'message>(
        &mut self,
        token: &crate::dynamic_orphan::Orphanage<'message>,
    ) -> Result<alloc::vec::Vec<(Field, crate::dynamic_orphan::Orphan<'message>)>> {
        token.check(self.builder.identity())?;
        let fields = self.group_fields()?;
        for field in &fields {
            self.prepare_disown(*field, 0)?;
        }
        let mut result = alloc::vec::Vec::with_capacity(fields.len());
        for field in fields {
            result.push((field, self.disown_prepared(field, token)?));
        }
        if let Some(default) = self.schema.get_field_by_discriminant(0)? {
            self.clear(default)?;
        }
        Ok(result)
    }

    #[cfg(feature = "alloc")]
    pub(crate) fn adopt_group_fields(
        &mut self,
        fields: &mut [(Field, crate::dynamic_orphan::Orphan<'_>)],
    ) -> Result<()> {
        self.check_group_adoption(fields)?;
        self.clear_for_adoption()?;
        for (field, value) in fields {
            self.adopt_inner(*field, value)?;
        }
        Ok(())
    }

    /// Adopt a compatible value from the same message and capability context.
    /// Failure returns ownership and leaves the destination and union unchanged.
    #[cfg(feature = "alloc")]
    pub fn adopt<'message>(
        &mut self,
        field: Field,
        mut orphan: crate::dynamic_orphan::Orphan<'message>,
    ) -> core::result::Result<
        (),
        crate::dynamic_orphan::AdoptError<crate::dynamic_orphan::Orphan<'message>>,
    > {
        let result = self.adopt_inner(field, &mut orphan);
        result.map_err(|error| crate::dynamic_orphan::AdoptError { error, orphan })
    }

    #[cfg(feature = "alloc")]
    pub fn adopt_named<'message>(
        &mut self,
        name: &str,
        orphan: crate::dynamic_orphan::Orphan<'message>,
    ) -> core::result::Result<
        (),
        crate::dynamic_orphan::AdoptError<crate::dynamic_orphan::Orphan<'message>>,
    > {
        match self.schema.get_field_by_name(name) {
            Ok(field) => self.adopt(field, orphan),
            Err(error) => Err(crate::dynamic_orphan::AdoptError { error, orphan }),
        }
    }

    #[cfg(feature = "alloc")]
    fn adopt_inner(
        &mut self,
        field: Field,
        orphan: &mut crate::dynamic_orphan::Orphan<'_>,
    ) -> Result<()> {
        use crate::dynamic_orphan::Value;
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        orphan.check(field.get_type(), self.builder.identity())?;
        if matches!(field.get_proto().which()?, field::Group(_)) {
            orphan.dematerialize_group(&mut self.builder.orphan_anchor())?;
        }
        match (field.get_proto().which()?, &mut orphan.inner.value) {
            (field::Slot(slot), Value::Pointer(object)) => {
                // Validate union metadata before changing the owning pointer.
                if has_discriminant_value(field.get_proto()) {
                    let node::Struct(_) = self.schema.get_proto().which()? else {
                        return Err(Error::from_kind(ErrorKind::NotAStruct));
                    };
                }
                self.builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize)
                    .adopt(object)?;
                self.set_in_union(field)
            }
            (field::Slot(_), Value::Scalar(value)) => self.set(field, value.clone()),
            (field::Group(_), Value::Group(fields)) => {
                let schema = field.get_type().as_struct_schema()?;
                let mut group = Builder::new(self.builder.reborrow(), schema);
                // Verify the whole group before clearing any destination field.
                group.adopt_group_fields(fields)?;
                self.set_in_union(field)
            }
            _ => Err(Error::from_kind(ErrorKind::TypeMismatch)),
        }
    }

    #[cfg(feature = "alloc")]
    fn check_group_adoption(
        &self,
        fields: &[(Field, crate::dynamic_orphan::Orphan<'_>)],
    ) -> Result<()> {
        for (field, value) in fields {
            if !self.schema.equals(field.parent)? {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            value.check(field.get_type(), self.builder.identity())?;
        }
        // Clear visits every declared field, including inactive union arms.
        fn check_schema(schema: StructSchema, depth: usize) -> Result<()> {
            if depth >= 64 {
                return Err(Error::failed("orphan group nesting limit".into()));
            }
            for field in schema.get_fields()? {
                if let field::Group(_) = field.get_proto().which()? {
                    check_schema(field.get_type().as_struct_schema()?, depth + 1)?;
                }
            }
            Ok(())
        }
        check_schema(self.schema, 0)
    }

    #[cfg(feature = "alloc")]
    fn clear_for_adoption(&mut self) -> Result<()> {
        for field in self.schema.get_fields()? {
            if let field::Group(_) = field.get_proto().which()? {
                Builder::new(
                    self.builder.reborrow(),
                    field.get_type().as_struct_schema()?,
                )
                .clear_for_adoption()?;
            } else {
                self.clear(field)?;
            }
        }
        if let Some(default) = self.schema.get_field_by_discriminant(0)? {
            self.set_in_union(default)?;
        }
        Ok(())
    }

    pub fn new(builder: layout::StructBuilder<'a>, schema: StructSchema) -> Self {
        Self { builder, schema }
    }

    pub fn reborrow(&mut self) -> Builder<'_> {
        Builder {
            builder: self.builder.reborrow(),
            schema: self.schema,
        }
    }

    pub fn reborrow_as_reader(&self) -> Reader<'_> {
        Reader {
            reader: self.builder.as_reader(),
            schema: self.schema,
        }
    }

    pub fn into_reader(self) -> Reader<'a> {
        Reader {
            schema: self.schema,
            reader: self.builder.into_reader(),
        }
    }

    pub fn get_schema(&self) -> StructSchema {
        self.schema
    }

    pub fn get(self, field: Field) -> Result<dynamic_value::Builder<'a>> {
        self.reborrow_as_reader().check_active(field)?;
        let ty = field.get_type();
        match field.get_proto().which()? {
            field::Slot(slot) => {
                let offset = slot.get_offset();
                let default_value = slot.get_default_value()?;

                match (ty.which(), default_value.which()?) {
                    (TypeVariant::Void, _) => Ok(dynamic_value::Builder::Void),
                    (TypeVariant::Bool, value::Bool(b)) => Ok(dynamic_value::Builder::Bool(
                        self.builder.get_bool_field_mask(offset as usize, b),
                    )),
                    (TypeVariant::Int8, value::Int8(x)) => Ok(dynamic_value::Builder::Int8(
                        self.builder.get_data_field_mask::<i8>(offset as usize, x),
                    )),
                    (TypeVariant::Int16, value::Int16(x)) => Ok(dynamic_value::Builder::Int16(
                        self.builder.get_data_field_mask::<i16>(offset as usize, x),
                    )),
                    (TypeVariant::Int32, value::Int32(x)) => Ok(dynamic_value::Builder::Int32(
                        self.builder.get_data_field_mask::<i32>(offset as usize, x),
                    )),
                    (TypeVariant::Int64, value::Int64(x)) => Ok(dynamic_value::Builder::Int64(
                        self.builder.get_data_field_mask::<i64>(offset as usize, x),
                    )),
                    (TypeVariant::UInt8, value::Uint8(x)) => Ok(dynamic_value::Builder::UInt8(
                        self.builder.get_data_field_mask::<u8>(offset as usize, x),
                    )),
                    (TypeVariant::UInt16, value::Uint16(x)) => Ok(dynamic_value::Builder::UInt16(
                        self.builder.get_data_field_mask::<u16>(offset as usize, x),
                    )),
                    (TypeVariant::UInt32, value::Uint32(x)) => Ok(dynamic_value::Builder::UInt32(
                        self.builder.get_data_field_mask::<u32>(offset as usize, x),
                    )),
                    (TypeVariant::UInt64, value::Uint64(x)) => Ok(dynamic_value::Builder::UInt64(
                        self.builder.get_data_field_mask::<u64>(offset as usize, x),
                    )),
                    (TypeVariant::Float32, value::Float32(x)) => {
                        Ok(dynamic_value::Builder::Float32(
                            self.builder
                                .get_data_field_mask::<f32>(offset as usize, x.to_bits()),
                        ))
                    }
                    (TypeVariant::Float64, value::Float64(x)) => {
                        Ok(dynamic_value::Builder::Float64(
                            self.builder
                                .get_data_field_mask::<f64>(offset as usize, x.to_bits()),
                        ))
                    }
                    (TypeVariant::Enum(schema), value::Enum(d)) => Ok(dynamic_value::Enum::new(
                        self.builder.get_data_field_mask::<u16>(offset as usize, d),
                        schema.into(),
                    )
                    .into()),
                    (TypeVariant::Text, dval) => {
                        let mut p = self.builder.get_pointer_field(offset as usize);
                        if p.is_null() {
                            // If the type is a generic, then the default value
                            // is always an empty AnyPointer. Ignore that case.
                            if let value::Text(t) = dval {
                                let t = t?;
                                if !t.is_empty() {
                                    p.set_text(t);
                                }
                            }
                        }
                        Ok(dynamic_value::Builder::Text(p.get_text(None)?))
                    }
                    (TypeVariant::Data, dval) => {
                        let mut p = self.builder.get_pointer_field(offset as usize);
                        if p.is_null() {
                            // If the type is a generic, then the default value
                            // is always an empty AnyPointer. Ignore that case.
                            if let value::Data(d) = dval {
                                let d = d?;
                                if !d.is_empty() {
                                    p.set_data(d);
                                }
                            }
                        }
                        Ok(dynamic_value::Builder::Data(p.get_data(None)?))
                    }
                    (TypeVariant::Struct(_), dval) => {
                        let mut p = self.builder.get_pointer_field(offset as usize);
                        if p.is_null() {
                            // If the type is a generic, then the default value
                            // is always an empty AnyPointer. Ignore that case.
                            if let value::Struct(s) = dval {
                                p.copy_from(s.reader, false)?;
                            }
                        }
                        Ok(Builder::new(
                            p.get_struct(struct_size_from_schema(ty.as_struct_schema()?)?, None)?,
                            ty.as_struct_schema()?,
                        )
                        .into())
                    }
                    (TypeVariant::List(element_type), dval) => {
                        let mut p = self.builder.get_pointer_field(offset as usize);
                        if p.is_null() {
                            if let value::List(l) = dval {
                                p.copy_from(l.reader, false)?;
                            }
                        }
                        let l = if let TypeVariant::Struct(ss) = element_type.which() {
                            p.get_struct_list(struct_size_from_schema(ss.into())?, None)?
                        } else {
                            p.get_list(element_type.expected_element_size(), None)?
                        };

                        Ok(dynamic_list::Builder::new(l, element_type).into())
                    }
                    (TypeVariant::AnyPointer, value::AnyPointer(_a)) => {
                        // AnyPointer fields can't have a nontrivial default.
                        Ok(crate::any_pointer::Builder::new(
                            self.builder.get_pointer_field(offset as usize),
                        )
                        .into())
                    }
                    (
                        TypeVariant::Capability | TypeVariant::Interface(_),
                        value::Interface(()) | value::AnyPointer(_),
                    ) => Ok(dynamic_value::Builder::Capability(
                        dynamic_value::Capability::from_pointer(
                            self.builder
                                .get_pointer_field(offset as usize)
                                .into_reader(),
                            ty,
                        )?,
                    )),
                    _ => Err(Error::from_kind(ErrorKind::FieldAndDefaultMismatch)),
                }
            }
            field::Group(_) => {
                if let TypeVariant::Struct(_) = ty.which() {
                    Ok(Builder::new(self.builder, ty.as_struct_schema()?).into())
                } else {
                    Err(Error::from_kind(ErrorKind::GroupFieldButTypeIsNotStruct))
                }
            }
        }
    }

    pub fn get_named(self, field_name: &str) -> Result<dynamic_value::Builder<'a>> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.get(field)
    }

    pub fn which(&self) -> Result<Option<Field>> {
        let node::Struct(st) = self.schema.get_proto().which()? else {
            return Err(Error::from_kind(ErrorKind::NotAStruct));
        };
        if st.get_discriminant_count() == 0 {
            Ok(None)
        } else {
            let discrim = self
                .builder
                .get_data_field::<u16>(st.get_discriminant_offset() as usize);
            self.schema.get_field_by_discriminant(discrim)
        }
    }

    pub fn has(&self, field: Field) -> Result<bool> {
        self.reborrow_as_reader().has(field)
    }

    pub fn has_with_mode(&self, field: Field, mode: HasMode) -> Result<bool> {
        self.reborrow_as_reader().has_with_mode(field, mode)
    }

    pub fn has_named(&self, field_name: &str) -> Result<bool> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.has(field)
    }

    pub fn has_named_with_mode(&self, field_name: &str, mode: HasMode) -> Result<bool> {
        self.reborrow_as_reader()
            .has_named_with_mode(field_name, mode)
    }

    pub fn set(&mut self, field: Field, value: dynamic_value::Reader<'_>) -> Result<()> {
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        let ty = field.get_type();
        value.validate_type(ty)?;
        self.set_in_union(field)?;
        match field.get_proto().which()? {
            field::Slot(slot) => {
                let dval = slot.get_default_value()?;
                let offset = slot.get_offset() as usize;
                match (ty.which(), value, dval.which()?) {
                    (TypeVariant::Void, _, _) => Ok(()),
                    (TypeVariant::Bool, dynamic_value::Reader::Bool(v), value::Bool(b)) => {
                        self.builder.set_bool_field_mask(offset, v, b);
                        Ok(())
                    }
                    (TypeVariant::Int8, dynamic_value::Reader::Int8(v), value::Int8(d)) => {
                        self.builder.set_data_field_mask::<i8>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::Int16, dynamic_value::Reader::Int16(v), value::Int16(d)) => {
                        self.builder.set_data_field_mask::<i16>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::Int32, dynamic_value::Reader::Int32(v), value::Int32(d)) => {
                        self.builder.set_data_field_mask::<i32>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::Int64, dynamic_value::Reader::Int64(v), value::Int64(d)) => {
                        self.builder.set_data_field_mask::<i64>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::UInt8, dynamic_value::Reader::UInt8(v), value::Uint8(d)) => {
                        self.builder.set_data_field_mask::<u8>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::UInt16, dynamic_value::Reader::UInt16(v), value::Uint16(d)) => {
                        self.builder.set_data_field_mask::<u16>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::UInt32, dynamic_value::Reader::UInt32(v), value::Uint32(d)) => {
                        self.builder.set_data_field_mask::<u32>(offset, v, d);
                        Ok(())
                    }
                    (TypeVariant::UInt64, dynamic_value::Reader::UInt64(v), value::Uint64(d)) => {
                        self.builder.set_data_field_mask::<u64>(offset, v, d);
                        Ok(())
                    }
                    (
                        TypeVariant::Float32,
                        dynamic_value::Reader::Float32(v),
                        value::Float32(d),
                    ) => {
                        self.builder
                            .set_data_field_mask::<f32>(offset, v, d.to_bits());
                        Ok(())
                    }
                    (
                        TypeVariant::Float64,
                        dynamic_value::Reader::Float64(v),
                        value::Float64(d),
                    ) => {
                        self.builder
                            .set_data_field_mask::<f64>(offset, v, d.to_bits());
                        Ok(())
                    }
                    (TypeVariant::Enum(_), dynamic_value::Reader::Enum(ev), value::Enum(d)) => {
                        self.builder
                            .set_data_field_mask::<u16>(offset, ev.get_value(), d);
                        Ok(())
                    }
                    (TypeVariant::Text, dynamic_value::Reader::Text(tv), _) => {
                        let mut p = self.builder.reborrow().get_pointer_field(offset);
                        p.set_text(tv);
                        Ok(())
                    }
                    (TypeVariant::Data, dynamic_value::Reader::Data(v), _) => {
                        let mut p = self.builder.reborrow().get_pointer_field(offset);
                        p.set_data(v);
                        Ok(())
                    }
                    (TypeVariant::List(_), dynamic_value::Reader::List(l), _) => {
                        let mut p = self.builder.reborrow().get_pointer_field(offset);
                        p.set_list(&l.reader, false)
                    }
                    (TypeVariant::Struct(_), dynamic_value::Reader::Struct(v), _) => {
                        let mut p = self.builder.reborrow().get_pointer_field(offset);
                        p.set_struct(&v.reader, false)
                    }
                    (TypeVariant::AnyPointer, value, _) => {
                        let mut target = crate::any_pointer::Builder::new(
                            self.builder.reborrow().get_pointer_field(offset),
                        );
                        match value {
                            dynamic_value::Reader::Text(t) => target.set_as(t),
                            dynamic_value::Reader::Data(t) => {
                                target.set_as::<crate::data::Owned>(t)
                            }
                            dynamic_value::Reader::Struct(s) => target.set_as(s),
                            dynamic_value::Reader::List(l) => target.set_as(l),
                            dynamic_value::Reader::AnyPointer(p) => target.set_as(p),
                            dynamic_value::Reader::Capability(cap) => {
                                cap.set_pointer(target.builder, TypeVariant::AnyPointer.into())
                            }
                            _ => Err(Error::from_kind(
                                ErrorKind::CannotSetAnyPointerFieldToAPrimitiveValue,
                            )),
                        }
                    }
                    (
                        TypeVariant::Capability | TypeVariant::Interface(_),
                        dynamic_value::Reader::Capability(cap),
                        _,
                    ) => cap.set_pointer(self.builder.reborrow().get_pointer_field(offset), ty),
                    _ => Err(Error::from_kind(ErrorKind::TypeMismatch)),
                }
            }
            field::Group(_group) => {
                let dynamic_value::Reader::Struct(src) = value else {
                    return Err(Error::from_kind(ErrorKind::NotAStruct));
                };
                let dynamic_value::Builder::Struct(mut dst) = self.reborrow().init(field)? else {
                    return Err(Error::from_kind(ErrorKind::NotAStruct));
                };
                if let Some(union_field) = src.which()? {
                    dst.set(union_field, src.get(union_field)?)?;
                }

                let non_union_fields = src.schema.get_non_union_fields()?;
                for idx in 0..non_union_fields.len() {
                    let field = non_union_fields.get(idx);
                    if src.has(field)? {
                        dst.set(field, src.get(field)?)?;
                    }
                }
                Ok(())
            }
        }
    }

    pub fn set_named(&mut self, field_name: &str, value: dynamic_value::Reader<'_>) -> Result<()> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.set(field, value)
    }

    pub fn init(mut self, field: Field) -> Result<dynamic_value::Builder<'a>> {
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        self.set_in_union(field)?;
        let ty = field.get_type();
        match field.get_proto().which()? {
            field::Slot(slot) => {
                let offset = slot.get_offset() as usize;
                match ty.which() {
                    TypeVariant::Struct(ss) => Ok(Builder {
                        schema: ty.as_struct_schema()?,
                        builder: self
                            .builder
                            .get_pointer_field(offset)
                            .init_struct(struct_size_from_schema(ss.into())?),
                    }
                    .into()),
                    TypeVariant::AnyPointer => {
                        let mut p = self.builder.get_pointer_field(offset);
                        p.clear();
                        Ok(crate::any_pointer::Builder::new(p).into())
                    }
                    _ => Err(Error::from_kind(
                        ErrorKind::InitIsOnlyValidForStructAndAnyPointerFields,
                    )),
                }
            }
            field::Group(_) => {
                self.clear(field)?;
                let TypeVariant::Struct(_) = ty.which() else {
                    return Err(Error::from_kind(ErrorKind::NotAStruct));
                };
                Ok((Builder::new(self.builder, ty.as_struct_schema()?)).into())
            }
        }
    }

    pub fn init_named(self, field_name: &str) -> Result<dynamic_value::Builder<'a>> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.init(field)
    }

    pub fn initn(mut self, field: Field, size: u32) -> Result<dynamic_value::Builder<'a>> {
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        self.set_in_union(field)?;
        let ty = field.get_type();
        match field.get_proto().which()? {
            field::Slot(slot) => {
                let offset = slot.get_offset() as usize;
                match ty.which() {
                    TypeVariant::List(element_type) => match element_type.which() {
                        TypeVariant::Struct(ss) => Ok(dynamic_list::Builder::new(
                            self.builder
                                .get_pointer_field(offset)
                                .init_struct_list(size, struct_size_from_schema(ss.into())?),
                            element_type,
                        )
                        .into()),
                        _ => Ok(dynamic_list::Builder::new(
                            self.builder
                                .get_pointer_field(offset)
                                .init_list(element_type.expected_element_size(), size),
                            element_type,
                        )
                        .into()),
                    },
                    TypeVariant::Text => Ok(self
                        .builder
                        .get_pointer_field(offset)
                        .init_text(size)
                        .into()),
                    TypeVariant::Data => Ok(self
                        .builder
                        .get_pointer_field(offset)
                        .init_data(size)
                        .into()),

                    _ => Err(Error::from_kind(
                        ErrorKind::InitnIsOnlyValidForListTextOrDataFields,
                    )),
                }
            }
            field::Group(_) => Err(Error::from_kind(
                ErrorKind::InitnIsOnlyValidForListTextOrDataFields,
            )),
        }
    }

    pub fn initn_named(self, field_name: &str, size: u32) -> Result<dynamic_value::Builder<'a>> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.initn(field, size)
    }

    /// Clears a field, setting it to its default value. For pointer fields,
    /// this makes the field null.
    pub fn clear(&mut self, field: Field) -> Result<()> {
        if !self.schema.equals(field.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        self.set_in_union(field)?;
        let ty = field.get_type();
        match field.get_proto().which()? {
            field::Slot(slot) => {
                let offset = slot.get_offset() as usize;
                match ty.which() {
                    TypeVariant::Void => Ok(()),
                    TypeVariant::Bool => {
                        self.builder.set_bool_field(offset, false);
                        Ok(())
                    }
                    TypeVariant::Int8 => {
                        self.builder.set_data_field::<i8>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::Int16 => {
                        self.builder.set_data_field::<i16>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::Int32 => {
                        self.builder.set_data_field::<i32>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::Int64 => {
                        self.builder.set_data_field::<i64>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::UInt8 => {
                        self.builder.set_data_field::<u8>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::UInt16 => {
                        self.builder.set_data_field::<u16>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::UInt32 => {
                        self.builder.set_data_field::<u32>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::UInt64 => {
                        self.builder.set_data_field::<u64>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::Float32 => {
                        self.builder.set_data_field::<f32>(offset, 0f32);
                        Ok(())
                    }
                    TypeVariant::Float64 => {
                        self.builder.set_data_field::<f64>(offset, 0f64);
                        Ok(())
                    }
                    TypeVariant::Enum(_) => {
                        self.builder.set_data_field::<u16>(offset, 0);
                        Ok(())
                    }
                    TypeVariant::Text
                    | TypeVariant::Data
                    | TypeVariant::Struct(_)
                    | TypeVariant::List(_)
                    | TypeVariant::AnyPointer
                    | TypeVariant::Capability
                    | TypeVariant::Interface(_) => {
                        self.builder.reborrow().get_pointer_field(offset).clear();
                        Ok(())
                    }
                }
            }
            field::Group(_) => {
                let TypeVariant::Struct(_) = ty.which() else {
                    return Err(Error::from_kind(ErrorKind::NotAStruct));
                };
                let mut group = Builder::new(self.builder.reborrow(), ty.as_struct_schema()?);

                // We clear the union field with discriminant 0 rather than the one that
                // is set because we want the union to end up with its default field active.
                if let Some(union_field) = group.schema.get_field_by_discriminant(0)? {
                    group.clear(union_field)?;
                }

                let non_union_fields = group.schema.get_non_union_fields()?;
                for idx in 0..non_union_fields.len() {
                    group.clear(non_union_fields.get(idx))?;
                }
                Ok(())
            }
        }
    }

    pub fn clear_named(&mut self, field_name: &str) -> Result<()> {
        let field = self.schema.get_field_by_name(field_name)?;
        self.clear(field)
    }

    fn set_in_union(&mut self, field: Field) -> Result<()> {
        if has_discriminant_value(field.get_proto()) {
            let node::Struct(st) = self.schema.get_proto().which()? else {
                return Err(Error::from_kind(ErrorKind::NotAStruct));
            };
            self.builder.set_data_field::<u16>(
                st.get_discriminant_offset() as usize,
                field.get_proto().get_discriminant_value(),
            );
        }
        Ok(())
    }

    /// Downcasts the `Builder` into a specific struct type. Panics if the
    /// expected type does not match the value.
    pub fn downcast<T: crate::traits::OwnedStruct>(self) -> T::Builder<'a> {
        assert!(self.schema.as_type().loose_equals(T::introspect()));
        self.builder.into()
    }
}

impl<'a> crate::traits::SetterInput<crate::any_pointer::Owned> for Reader<'a> {
    fn set_pointer_builder<'b>(
        mut pointer: crate::private::layout::PointerBuilder<'b>,
        value: Reader<'a>,
        canonicalize: bool,
    ) -> Result<()> {
        pointer.set_struct(&value.reader, canonicalize)
    }
}

impl core::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&crate::dynamic_value::Reader::from(*self), f)
    }
}

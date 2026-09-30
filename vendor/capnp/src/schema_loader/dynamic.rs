//! Dynamic message access using loader-borrowed schema handles.
mod cast;
mod conversion;
mod native;
pub mod orphan;
mod pointer;
use super::*;
pub use crate::dynamic_struct::HasMode;
use crate::{
    any_pointer, capability,
    private::layout::{self, PrimitiveElement},
    schema_capnp::value,
};
#[derive(Clone)]
pub enum Value<'a, 's: 'a> {
    Unknown(u16),
    Void,
    Bool(bool),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    Float32(f32),
    Float64(f64),
    Text(crate::text::Reader<'a>),
    Data(&'a [u8]),
    Enum(u16, Schema<'s>),
    Struct(Reader<'a, 's>),
    List(ListReader<'a, 's>),
    AnyPointer(any_pointer::Reader<'a>),
    Capability(Client<'s>),
}
impl Value<'_, '_> {
    fn check(&self, ty: &Type<'_>) -> Result<()> {
        let ok = match (self, ty) {
            (Self::Void, Type::Void) => true,
            (Self::Unknown(a), Type::Unknown(b)) => a == b,
            (Self::Bool(_), Type::Bool) => true,
            (Self::Int8(_), Type::Int8) => true,
            (Self::Int16(_), Type::Int16) => true,
            (Self::Int32(_), Type::Int32) => true,
            (Self::Int64(_), Type::Int64) => true,
            (Self::UInt8(_), Type::UInt8) => true,
            (Self::UInt16(_), Type::UInt16) => true,
            (Self::UInt32(_), Type::UInt32) => true,
            (Self::UInt64(_), Type::UInt64) => true,
            (Self::Float32(_), Type::Float32) => true,
            (Self::Float64(_), Type::Float64) => true,
            (Self::Text(_), Type::Text) | (Self::Data(_), Type::Data) => true,
            (Self::Enum(_, a), Type::Enum(b)) => a.equals(b),
            (Self::Struct(a), Type::Struct(b)) => a.schema.equals(b),
            (Self::List(a), Type::List(b)) => a.element == **b,
            (Self::Capability(a), Type::Interface(b)) => a
                .schema
                .as_ref()
                .is_none_or(|s| s.extends(b).unwrap_or(false)),
            (Self::Capability(_), Type::AnyPointer(PointerKind::Capability)) => true,
            (Self::AnyPointer(p), Type::AnyPointer(kind)) => match kind {
                PointerKind::Any => true,
                PointerKind::Struct => {
                    p.is_null()
                        || matches!(p.reader.get_pointer_type()?, layout::PointerType::Struct)
                }
                PointerKind::List => {
                    p.is_null() || matches!(p.reader.get_pointer_type()?, layout::PointerType::List)
                }
                PointerKind::Capability => {
                    p.is_null()
                        || matches!(
                            p.reader.get_pointer_type()?,
                            layout::PointerType::Capability
                        )
                }
            },
            _ => false,
        };
        require(ok, "dynamic value type/brand mismatch")
    }
}
#[derive(Clone)]
pub struct Reader<'a, 's: 'a> {
    reader: layout::StructReader<'a>,
    schema: Schema<'s>,
}
impl<'a, 's: 'a> crate::traits::IntoInternalStructReader<'a> for Reader<'a, 's> {
    fn into_internal_struct_reader(self) -> layout::StructReader<'a> {
        self.reader
    }
}
impl<'a, 's: 'a> Reader<'a, 's> {
    pub fn new(pointer: any_pointer::Reader<'a>, schema: Schema<'s>) -> Result<Self> {
        schema.struct_size()?;
        Ok(Self {
            reader: pointer.reader.get_struct(None)?,
            schema,
        })
    }
    pub fn schema(&self) -> Schema<'s> {
        self.schema.clone()
    }
    /// C++ native casts require registration and erase generic arguments.
    pub fn downcast_native<T: crate::traits::OwnedStruct>(self) -> Result<T::Reader<'a>> {
        self.schema.check_native::<T>()?;
        Ok(self.reader.into())
    }
    pub fn total_size(&self) -> Result<crate::MessageSize> {
        self.reader.total_size()
    }
    pub fn get_named(&self, name: &str) -> Result<Value<'a, 's>> {
        self.get(self.schema.field(name)?)
    }
    pub fn which(&self) -> Result<Option<Field<'s>>> {
        let node::Struct(s) = self.schema.get_proto().which()? else {
            unreachable!()
        };
        if s.get_discriminant_count() == 0 {
            return Ok(None);
        }
        let tag = self
            .reader
            .get_data_field::<u16>(s.get_discriminant_offset() as usize);
        self.schema.field_by_discriminant(tag)
    }
    pub fn has(&self, f: Field<'s>) -> Result<bool> {
        self.has_with_mode(f, HasMode::NonNull)
    }
    /// Uses encoded bits for primitive defaults and wire nullness for pointers.
    /// Inactive union arms and unknown future field types are absent.
    pub fn has_with_mode(&self, f: Field<'s>, mode: HasMode) -> Result<bool> {
        require(
            self.schema.equals(&f.parent),
            "field belongs to another schema",
        )?;
        if f.get_proto().get_discriminant_value() != field::NO_DISCRIMINANT
            && !self.which()?.is_some_and(|a| a.index == f.index)
        {
            return Ok(false);
        }
        match f.get_proto().which()? {
            field::Group(_) => Ok(true),
            field::Slot(s) => {
                let ty = f.get_type()?;
                Ok(!matches!(ty, Type::Unknown(_))
                    && crate::dynamic_struct::has_wire_field(
                        self.reader,
                        s.get_offset() as usize,
                        ty.element_size(),
                        mode,
                    ))
            }
        }
    }
    pub fn has_named(&self, name: &str) -> Result<bool> {
        self.has(self.schema.field(name)?)
    }
    pub fn has_named_with_mode(&self, name: &str, mode: HasMode) -> Result<bool> {
        self.has_with_mode(self.schema.field(name)?, mode)
    }
    pub fn get(&self, f: Field<'s>) -> Result<Value<'a, 's>> {
        self.check_active(&f)?;
        self.read_field(f)
    }
    // Check before pointer access, default materialization or layout upgrades.
    // Mutable getters share the reader's rule without reading the field value.
    fn check_active(&self, f: &Field<'s>) -> Result<()> {
        require(
            self.schema.equals(&f.parent),
            "field belongs to another schema",
        )?;
        if f.get_proto().get_discriminant_value() != field::NO_DISCRIMINANT {
            require(
                self.which()?.is_some_and(|active| active.index == f.index),
                "cannot read inactive union arm",
            )?;
        }
        Ok(())
    }
    // Field defaults are read from a synthetic zeroed struct by orphan group
    // construction, where the message's active union tag is not relevant.
    fn read_field(&self, f: Field<'s>) -> Result<Value<'a, 's>> {
        let ty = f.get_type()?;
        if let Type::Unknown(tag) = ty {
            return Ok(Value::Unknown(tag));
        }
        match f.get_proto().which()? {
            field::Group(_) => {
                let Type::Struct(schema) = ty else {
                    unreachable!()
                };
                Ok(Value::Struct(Self {
                    reader: self.reader,
                    schema,
                }))
            }
            field::Slot(s) => {
                let offset = s.get_offset() as usize;
                let default = s.get_default_value()?;
                Ok(match (&ty, default.which()?) {
                    (Type::Void, _) => Value::Void,
                    (Type::Bool, value::Bool(d)) => {
                        Value::Bool(self.reader.get_bool_field_mask(offset, d))
                    }
                    (Type::Int8, value::Int8(d)) => {
                        Value::Int8(self.reader.get_data_field_mask::<i8>(offset, d))
                    }
                    (Type::Int16, value::Int16(d)) => {
                        Value::Int16(self.reader.get_data_field_mask::<i16>(offset, d))
                    }
                    (Type::Int32, value::Int32(d)) => {
                        Value::Int32(self.reader.get_data_field_mask::<i32>(offset, d))
                    }
                    (Type::Int64, value::Int64(d)) => {
                        Value::Int64(self.reader.get_data_field_mask::<i64>(offset, d))
                    }
                    (Type::UInt8, value::Uint8(d)) => {
                        Value::UInt8(self.reader.get_data_field_mask::<u8>(offset, d))
                    }
                    (Type::UInt16, value::Uint16(d)) => {
                        Value::UInt16(self.reader.get_data_field_mask::<u16>(offset, d))
                    }
                    (Type::UInt32, value::Uint32(d)) => {
                        Value::UInt32(self.reader.get_data_field_mask::<u32>(offset, d))
                    }
                    (Type::UInt64, value::Uint64(d)) => {
                        Value::UInt64(self.reader.get_data_field_mask::<u64>(offset, d))
                    }
                    (Type::Float32, value::Float32(d)) => {
                        Value::Float32(self.reader.get_data_field_mask::<f32>(offset, d.to_bits()))
                    }
                    (Type::Float64, value::Float64(d)) => {
                        Value::Float64(self.reader.get_data_field_mask::<f64>(offset, d.to_bits()))
                    }
                    (Type::Enum(schema), value::Enum(d)) => Value::Enum(
                        self.reader.get_data_field_mask::<u16>(offset, d),
                        schema.clone(),
                    ),
                    _ if ty.is_pointer() => {
                        let p = self.reader.get_pointer_field(offset);
                        if p.is_null() {
                            match default.which()? {
                                value::Text(t) if matches!(ty, Type::Text) => {
                                    return Ok(Value::Text(t?))
                                }
                                value::Data(d) if matches!(ty, Type::Data) => {
                                    return Ok(Value::Data(d?))
                                }
                                value::Struct(d) | value::List(d) | value::AnyPointer(d) => {
                                    return read_pointer(d.reader, ty)
                                }
                                _ => (),
                            }
                        }
                        return read_pointer(p, ty);
                    }
                    _ => return Err(invalid("field default mismatch")),
                })
            }
        }
    }
}
/// Decode schema metadata using the resolved type, including annotation brands.
/// A schema Value's union tag must match; metadata cannot reinterpret scalars
/// or turn a null interface value into a live capability.
pub(super) fn read_schema_value<'a, 's: 'a>(
    value: value::Reader<'a>,
    ty: Type<'s>,
) -> Result<Value<'a, 's>> {
    if let Type::Unknown(tag) = ty {
        return Ok(Value::Unknown(tag));
    }
    Ok(match (ty, value.which()?) {
        (Type::Void, value::Void(())) => Value::Void,
        (Type::Bool, value::Bool(v)) => Value::Bool(v),
        (Type::Int8, value::Int8(v)) => Value::Int8(v),
        (Type::Int16, value::Int16(v)) => Value::Int16(v),
        (Type::Int32, value::Int32(v)) => Value::Int32(v),
        (Type::Int64, value::Int64(v)) => Value::Int64(v),
        (Type::UInt8, value::Uint8(v)) => Value::UInt8(v),
        (Type::UInt16, value::Uint16(v)) => Value::UInt16(v),
        (Type::UInt32, value::Uint32(v)) => Value::UInt32(v),
        (Type::UInt64, value::Uint64(v)) => Value::UInt64(v),
        (Type::Float32, value::Float32(v)) => Value::Float32(v),
        (Type::Float64, value::Float64(v)) => Value::Float64(v),
        (Type::Text, value::Text(v)) => Value::Text(v?),
        (Type::Data, value::Data(v)) => Value::Data(v?),
        (Type::Enum(schema), value::Enum(v)) => Value::Enum(v, schema),
        (t @ Type::Struct(_), value::Struct(v))
        | (t @ Type::List(_), value::List(v))
        | (t @ Type::AnyPointer(_), value::AnyPointer(v)) => read_pointer(v.reader, t)?,
        (Type::Interface(schema), value::Interface(())) => {
            Value::Capability(Client::null(Some(schema)))
        }
        _ => return Err(invalid("schema value type/brand mismatch")),
    })
}

fn read_pointer<'a, 's: 'a>(p: layout::PointerReader<'a>, ty: Type<'s>) -> Result<Value<'a, 's>> {
    Ok(match ty {
        Type::Text => Value::Text(p.get_text(None)?),
        Type::Data => Value::Data(p.get_data(None)?),
        Type::Struct(schema) => Value::Struct(Reader {
            reader: p.get_struct(None)?,
            schema,
        }),
        Type::List(element) => Value::List(ListReader {
            reader: p.get_list(element.element_size(), None)?,
            element: *element,
        }),
        Type::Interface(schema) => Value::Capability(Client {
            client: if p.is_null() {
                None
            } else {
                Some(capability::Client::new(p.get_capability()?))
            },
            schema: Some(schema),
        }),
        Type::AnyPointer(PointerKind::Capability) => Value::Capability(Client {
            client: if p.is_null() {
                None
            } else {
                Some(capability::Client::new(p.get_capability()?))
            },
            schema: None,
        }),
        Type::AnyPointer(kind) => {
            let value = Value::AnyPointer(any_pointer::Reader::new(p));
            value.check(&Type::AnyPointer(kind))?;
            value
        }
        _ => return Err(invalid("not a concrete pointer type")),
    })
}
fn write_pointer(mut p: layout::PointerBuilder<'_>, value: Value<'_, '_>) -> Result<()> {
    match value {
        Value::Text(t) => p.set_text(t),
        Value::Data(d) => p.set_data(d),
        Value::Struct(s) => p.set_struct(&s.reader, false)?,
        Value::List(l) => p.set_list(&l.reader, false)?,
        Value::AnyPointer(a) => p.copy_from(a.reader, false)?,
        Value::Capability(c) => {
            if let Some(c) = c.client {
                p.set_capability(c.hook);
            } else {
                p.clear();
            }
        }
        _ => return Err(invalid("not a pointer value")),
    }
    Ok(())
}
pub struct Builder<'a, 's: 'a> {
    builder: layout::StructBuilder<'a>,
    schema: Schema<'s>,
}
impl<'a, 's: 'a> crate::traits::ImbueMut<'a> for Builder<'a, 's> {
    fn imbue_mut(&mut self, table: &'a mut layout::CapTable) {
        self.builder.imbue(layout::CapTableBuilder::Plain(table));
    }
}
impl<'a, 's: 'a> Builder<'a, 's> {
    pub fn init(pointer: any_pointer::Builder<'a>, schema: Schema<'s>) -> Result<Self> {
        let size = schema.struct_size()?;
        if let node::Struct(s) = schema.get_proto().which()? {
            require(!s.get_is_group(), "cannot allocate a group independently")?;
        }
        Ok(Self {
            builder: pointer.builder.init_struct(size),
            schema,
        })
    }
    pub fn new(pointer: any_pointer::Builder<'a>, schema: Schema<'s>) -> Result<Self> {
        Ok(Self {
            builder: pointer.builder.get_struct(schema.struct_size()?, None)?,
            schema,
        })
    }
    pub fn schema(&self) -> Schema<'s> {
        self.schema.clone()
    }
    pub fn downcast_native<T: crate::traits::OwnedStruct>(self) -> Result<T::Builder<'a>> {
        self.schema.check_native::<T>()?;
        Ok(self.builder.into())
    }
    pub fn reborrow(&mut self) -> Builder<'_, 's> {
        Builder {
            builder: self.builder.reborrow(),
            schema: self.schema.clone(),
        }
    }
    pub fn as_reader(&self) -> Reader<'_, 's> {
        Reader {
            reader: self.builder.as_reader(),
            schema: self.schema.clone(),
        }
    }
    pub fn has(&self, f: Field<'s>) -> Result<bool> {
        self.as_reader().has(f)
    }
    pub fn has_with_mode(&self, f: Field<'s>, mode: HasMode) -> Result<bool> {
        self.as_reader().has_with_mode(f, mode)
    }
    pub fn has_named(&self, name: &str) -> Result<bool> {
        self.as_reader().has_named(name)
    }
    pub fn has_named_with_mode(&self, name: &str, mode: HasMode) -> Result<bool> {
        self.as_reader().has_named_with_mode(name, mode)
    }
    pub fn into_reader(self) -> Reader<'a, 's> {
        Reader {
            reader: self.builder.into_reader(),
            schema: self.schema,
        }
    }
    fn activate(&mut self, f: &Field<'_>) -> Result<()> {
        let tag = f.get_proto().get_discriminant_value();
        if tag != field::NO_DISCRIMINANT {
            let node::Struct(s) = self.schema.get_proto().which()? else {
                unreachable!()
            };
            self.builder
                .set_data_field::<u16>(s.get_discriminant_offset() as usize, tag);
        }
        Ok(())
    }
    pub fn set_named(&mut self, name: &str, value: Value<'_, '_>) -> Result<()> {
        self.set(self.schema.field(name)?, value)
    }
    pub fn set(&mut self, f: Field<'_>, value: Value<'_, '_>) -> Result<()> {
        require(
            self.schema.equals(&f.parent),
            "field belongs to another schema",
        )?;
        value.check(&f.get_type()?)?;
        let field::Slot(s) = f.get_proto().which()? else {
            return Err(invalid("set group fields through group()"));
        };
        let offset = s.get_offset() as usize;
        match (value, s.get_default_value()?.which()?) {
            (Value::Void, _) => (),
            (Value::Bool(v), value::Bool(d)) => self.builder.set_bool_field_mask(offset, v, d),
            (Value::Int8(v), value::Int8(d)) => {
                self.builder.set_data_field_mask::<i8>(offset, v, d)
            }
            (Value::Int16(v), value::Int16(d)) => {
                self.builder.set_data_field_mask::<i16>(offset, v, d)
            }
            (Value::Int32(v), value::Int32(d)) => {
                self.builder.set_data_field_mask::<i32>(offset, v, d)
            }
            (Value::Int64(v), value::Int64(d)) => {
                self.builder.set_data_field_mask::<i64>(offset, v, d)
            }
            (Value::UInt8(v), value::Uint8(d)) => {
                self.builder.set_data_field_mask::<u8>(offset, v, d)
            }
            (Value::UInt16(v), value::Uint16(d)) => {
                self.builder.set_data_field_mask::<u16>(offset, v, d)
            }
            (Value::UInt32(v), value::Uint32(d)) => {
                self.builder.set_data_field_mask::<u32>(offset, v, d)
            }
            (Value::UInt64(v), value::Uint64(d)) => {
                self.builder.set_data_field_mask::<u64>(offset, v, d)
            }
            (Value::Float32(v), value::Float32(d)) => {
                self.builder
                    .set_data_field_mask::<f32>(offset, v, d.to_bits())
            }
            (Value::Float64(v), value::Float64(d)) => {
                self.builder
                    .set_data_field_mask::<f64>(offset, v, d.to_bits())
            }
            (Value::Enum(v, _), value::Enum(d)) => {
                self.builder.set_data_field_mask::<u16>(offset, v, d)
            }
            (v, _) if f.get_type()?.is_pointer() => {
                write_pointer(self.builder.reborrow().get_pointer_field(offset), v)?
            }
            _ => return Err(invalid("field default mismatch")),
        }
        self.activate(&f)
    }
    pub fn clear_named(&mut self, name: &str) -> Result<()> {
        self.clear(self.schema.field(name)?)
    }
    /// Restore a field's schema default and select it if it is a union member.
    /// Groups reset their default union alternative and non-union fields, as in
    /// C++; storage exclusive to inactive alternatives is left intact.
    pub fn clear(&mut self, f: Field<'_>) -> Result<()> {
        require(
            self.schema.equals(&f.parent),
            "field belongs to another schema",
        )?;
        match f.get_proto().which()? {
            field::Slot(slot) => {
                let offset = slot.get_offset() as usize;
                match f.get_type()? {
                    Type::Void => (),
                    Type::Bool => self.builder.set_bool_field(offset, false),
                    Type::Int8 | Type::UInt8 => self.builder.set_data_field::<u8>(offset, 0),
                    Type::Int16 | Type::UInt16 | Type::Enum(_) => {
                        self.builder.set_data_field::<u16>(offset, 0)
                    }
                    Type::Int32 | Type::UInt32 | Type::Float32 => {
                        self.builder.set_data_field::<u32>(offset, 0)
                    }
                    Type::Int64 | Type::UInt64 | Type::Float64 => {
                        self.builder.set_data_field::<u64>(offset, 0)
                    }
                    ty if ty.is_pointer() => {
                        self.builder.reborrow().get_pointer_field(offset).clear()
                    }
                    _ => return Err(invalid("cannot clear an unknown field layout")),
                }
            }
            field::Group(_) => {
                let Type::Struct(schema) = f.get_type()? else {
                    unreachable!()
                };
                let mut group = Builder {
                    builder: self.builder.reborrow(),
                    schema,
                };
                // Select and clear the default arm, not the active arm or all
                // alternatives. This also handles a group as the default arm.
                if let Some(default) = group.schema.field_by_discriminant(0)? {
                    group.clear(default)?;
                }
                for child in group.schema.non_union_fields()? {
                    group.clear(child)?;
                }
            }
        }
        self.activate(&f)
    }
    /// Borrow an active text field for in-place editing. A nonempty default is
    /// copied into the message on first access; an empty default stays null.
    /// Inactive union arms and non-text fields fail before pointer access.
    /// The returned bytes borrow the message, independently of the schema:
    ///
    /// ```compile_fail
    /// use capnp::{text, schema_loader::dynamic::Builder};
    /// fn escape<'a, 's: 'a>(root: Builder<'a, 's>) -> text::Builder<'static> {
    ///     root.get_text("text").unwrap()
    /// }
    /// ```
    pub fn get_text(self, name: &str) -> Result<crate::text::Builder<'a>> {
        let f = self.schema.field(name)?;
        self.as_reader().check_active(&f)?;
        require(matches!(f.get_type()?, Type::Text), "not a text field")?;
        let field::Slot(slot) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        let mut pointer = self.builder.get_pointer_field(slot.get_offset() as usize);
        if pointer.is_null() {
            if let value::Text(default) = slot.get_default_value()?.which()? {
                let default = default?;
                if !default.is_empty() {
                    pointer.set_text(default);
                }
            }
        }
        pointer.get_text(None)
    }
    /// Borrow an active data field for in-place editing. Nonempty defaults are
    /// materialized; empty defaults stay null. Selection and type are checked
    /// before touching the pointer or any retained capability owner.
    pub fn get_data(self, name: &str) -> Result<crate::data::Builder<'a>> {
        let f = self.schema.field(name)?;
        self.as_reader().check_active(&f)?;
        require(matches!(f.get_type()?, Type::Data), "not a data field")?;
        let field::Slot(slot) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        let mut pointer = self.builder.get_pointer_field(slot.get_offset() as usize);
        if pointer.is_null() {
            if let value::Data(default) = slot.get_default_value()?.which()? {
                let default = default?;
                if !default.is_empty() {
                    pointer.set_data(default);
                }
            }
        }
        pointer.get_data(None)
    }
    /// Select and replace a text field with `size` zeroed content bytes plus
    /// its NUL terminator. Type and wire-size errors leave the message unchanged.
    pub fn init_text(mut self, name: &str, size: u32) -> Result<crate::text::Builder<'a>> {
        let f = self.schema.field(name)?;
        require(matches!(f.get_type()?, Type::Text), "not a text field")?;
        let field::Slot(slot) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        require(size < (1 << 29) - 1, "text exceeds wire limit")?;
        self.activate(&f)?;
        Ok(self
            .builder
            .get_pointer_field(slot.get_offset() as usize)
            .init_text(size))
    }
    /// Select and replace a data field with `size` zeroed bytes. Type and
    /// wire-size errors leave the message and retained capability owners unchanged.
    pub fn init_data(mut self, name: &str, size: u32) -> Result<crate::data::Builder<'a>> {
        let f = self.schema.field(name)?;
        require(matches!(f.get_type()?, Type::Data), "not a data field")?;
        let field::Slot(slot) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        require(size < 1 << 29, "data exceeds wire limit")?;
        self.activate(&f)?;
        Ok(self
            .builder
            .get_pointer_field(slot.get_offset() as usize)
            .init_data(size))
    }
    /// Access an active list field, materializing its default if needed.
    /// Inactive or unknown union selections fail before touching its pointer.
    pub fn get_list(self, name: &str) -> Result<ListBuilder<'a, 's>> {
        let f = self.schema.field(name)?;
        self.as_reader().check_active(&f)?;
        let Type::List(element) = f.get_type()? else {
            return Err(invalid("not a list field"));
        };
        let field::Slot(slot) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        let mut pointer = self.builder.get_pointer_field(slot.get_offset() as usize);
        if pointer.is_null() {
            if let value::List(default) = slot.get_default_value()?.which()? {
                pointer.copy_from(default.reader, false)?;
            }
        }
        list_builder(pointer, *element)
    }
    /// Select a group and return a view without resetting its fields.
    pub fn group(self, name: &str) -> Result<Self> {
        let f = self.schema.field(name)?;
        require(
            matches!(f.get_proto().which()?, field::Group(_)),
            "not a group field",
        )?;
        let Type::Struct(schema) = f.get_type()? else {
            unreachable!()
        };
        let mut result = self;
        result.activate(&f)?;
        result.schema = schema;
        Ok(result)
    }
    /// Initialize a struct pointer or reset a group to its schema defaults.
    /// Returns a writable view, preserving the group's generic bindings.
    pub fn init_struct(mut self, name: &str) -> Result<Self> {
        let f = self.schema.field(name)?;
        let Type::Struct(schema) = f.get_type()? else {
            return Err(invalid("not a struct field"));
        };
        let field::Slot(s) = f.get_proto().which()? else {
            self.clear(f)?;
            return self.group(name);
        };
        self.activate(&f)?;
        Ok(Self {
            builder: self
                .builder
                .get_pointer_field(s.get_offset() as usize)
                .init_struct(schema.struct_size()?),
            schema,
        })
    }
    /// Access an active struct/group field without selecting a union arm.
    /// Struct pointer defaults are materialized only after checking selection.
    /// Use `group()` to explicitly select a group without resetting its fields.
    pub fn get_struct(self, name: &str) -> Result<Self> {
        let f = self.schema.field(name)?;
        self.as_reader().check_active(&f)?;
        let Type::Struct(schema) = f.get_type()? else {
            return Err(invalid("not a struct field"));
        };
        let field::Slot(s) = f.get_proto().which()? else {
            return Ok(Self {
                builder: self.builder,
                schema,
            });
        };
        let mut p = self.builder.get_pointer_field(s.get_offset() as usize);
        if p.is_null() {
            if let value::Struct(default) = s.get_default_value()?.which()? {
                p.copy_from(default.reader, false)?;
            }
        }
        Ok(Self {
            builder: p.get_struct(schema.struct_size()?, None)?,
            schema,
        })
    }
    pub fn init_list(mut self, name: &str, count: u32) -> Result<ListBuilder<'a, 's>> {
        let f = self.schema.field(name)?;
        let Type::List(element) = f.get_type()? else {
            return Err(invalid("not a list field"));
        };
        let field::Slot(s) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        check_list_count(&element, count)?;
        self.activate(&f)?;
        init_list(
            self.builder.get_pointer_field(s.get_offset() as usize),
            *element,
            count,
        )
    }
}
fn check_list_count(element: &Type<'_>, count: u32) -> Result<()> {
    require(count < 1 << 29, "list element count exceeds wire limit")?;
    if let Type::Struct(s) = element {
        let size = s.struct_size()?;
        require(
            u64::from(count) * (u64::from(size.data) + u64::from(size.pointers)) < 1 << 29,
            "struct list exceeds wire limit",
        )?;
    }
    Ok(())
}
fn list_builder<'a, 's: 'a>(
    p: layout::PointerBuilder<'a>,
    element: Type<'s>,
) -> Result<ListBuilder<'a, 's>> {
    let builder = match &element {
        Type::Struct(s) => p.get_struct_list(s.struct_size()?, None)?,
        _ => p.get_list(element.element_size(), None)?,
    };
    Ok(ListBuilder { builder, element })
}
fn init_list<'a, 's: 'a>(
    p: layout::PointerBuilder<'a>,
    element: Type<'s>,
    count: u32,
) -> Result<ListBuilder<'a, 's>> {
    check_list_count(&element, count)?;
    let builder = match &element {
        Type::Struct(s) => p.init_struct_list(count, s.struct_size()?),
        _ => p.init_list(element.element_size(), count),
    };
    Ok(ListBuilder { builder, element })
}
#[derive(Clone)]
pub struct ListReader<'a, 's: 'a> {
    reader: layout::ListReader<'a>,
    element: Type<'s>,
}
impl<'a, 's: 'a> crate::traits::IntoInternalListReader<'a> for ListReader<'a, 's> {
    fn into_internal_list_reader(self) -> layout::ListReader<'a> {
        self.reader
    }
}
impl<'a, 's: 'a> ListReader<'a, 's> {
    /// Cast without copying, after checking element kind, list nesting and
    /// native registration. Generic arguments are erased as in C++ native
    /// casts. Capability pointers are not read or resolved by this operation.
    /// The native reader continues to borrow the message:
    ///
    /// ```compile_fail
    /// use capnp::{primitive_list, schema_loader::dynamic::ListReader};
    /// fn escape<'a, 's: 'a>(list: ListReader<'a, 's>) -> primitive_list::Reader<'static, u32> {
    ///     list.downcast_native::<primitive_list::Owned<u32>>().unwrap()
    /// }
    /// ```
    pub fn downcast_native<T: crate::traits::Owned>(self) -> Result<T::Reader<'a>>
    where
        T::Reader<'a>: crate::dynamic_value::DowncastReader<'a>,
    {
        let crate::introspect::TypeVariant::List(element) = T::introspect().which() else {
            return Err(invalid("native target is not a list"));
        };
        self.element.require_usable_as_type(element)?;
        Ok(
            crate::dynamic_value::Reader::List(crate::dynamic_list::Reader::new(
                self.reader,
                element,
            ))
            .downcast(),
        )
    }
    pub fn len(&self) -> u32 {
        self.reader.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn element_type(&self) -> Type<'s> {
        self.element.clone()
    }
    pub fn get(&self, index: u32) -> Result<Value<'a, 's>> {
        require(index < self.len(), "list index out of bounds")?;
        Ok(match &self.element {
            Type::Unknown(tag) => Value::Unknown(*tag),
            Type::Void => Value::Void,
            Type::Bool => Value::Bool(PrimitiveElement::get(&self.reader, index)),
            Type::Int8 => Value::Int8(PrimitiveElement::get(&self.reader, index)),
            Type::Int16 => Value::Int16(PrimitiveElement::get(&self.reader, index)),
            Type::Int32 => Value::Int32(PrimitiveElement::get(&self.reader, index)),
            Type::Int64 => Value::Int64(PrimitiveElement::get(&self.reader, index)),
            Type::UInt8 => Value::UInt8(PrimitiveElement::get(&self.reader, index)),
            Type::UInt16 => Value::UInt16(PrimitiveElement::get(&self.reader, index)),
            Type::UInt32 => Value::UInt32(PrimitiveElement::get(&self.reader, index)),
            Type::UInt64 => Value::UInt64(PrimitiveElement::get(&self.reader, index)),
            Type::Float32 => Value::Float32(PrimitiveElement::get(&self.reader, index)),
            Type::Float64 => Value::Float64(PrimitiveElement::get(&self.reader, index)),
            Type::Enum(s) => Value::Enum(PrimitiveElement::get(&self.reader, index), s.clone()),
            Type::Struct(schema) => Value::Struct(Reader {
                reader: self.reader.get_struct_element(index),
                schema: schema.clone(),
            }),
            _ => return read_pointer(self.reader.get_pointer_element(index), self.element.clone()),
        })
    }
}
pub struct ListBuilder<'a, 's: 'a> {
    builder: layout::ListBuilder<'a>,
    element: Type<'s>,
}
impl<'a, 's: 'a> crate::traits::ImbueMut<'a> for ListBuilder<'a, 's> {
    fn imbue_mut(&mut self, table: &'a mut layout::CapTable) {
        self.builder.imbue(layout::CapTableBuilder::Plain(table));
    }
}
impl<'a, 's: 'a> ListBuilder<'a, 's> {
    /// Cast the same list storage to a registered native type. A successful
    /// cast preserves its capability table and permits native writes in place.
    /// Use `reborrow()` when retaining the dynamic builder after a failed cast.
    /// Native and loaded views cannot mutate the same storage concurrently:
    ///
    /// ```compile_fail
    /// use capnp::{primitive_list, schema_loader::dynamic::{ListBuilder, Value}};
    /// fn alias(mut list: ListBuilder<'_, '_>) {
    ///     let mut native = list.reborrow().downcast_native::<primitive_list::Owned<u32>>().unwrap();
    ///     list.set(0, Value::UInt32(1)).unwrap();
    ///     native.set(0, 2);
    /// }
    /// ```
    pub fn downcast_native<T: crate::traits::Owned>(self) -> Result<T::Builder<'a>>
    where
        T::Builder<'a>: crate::dynamic_value::DowncastBuilder<'a>,
    {
        let crate::introspect::TypeVariant::List(element) = T::introspect().which() else {
            return Err(invalid("native target is not a list"));
        };
        self.element.require_usable_as_type(element)?;
        Ok(
            crate::dynamic_value::Builder::List(crate::dynamic_list::Builder::new(
                self.builder,
                element,
            ))
            .downcast(),
        )
    }
    pub fn len(&self) -> u32 {
        self.builder.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn reborrow(&mut self) -> ListBuilder<'_, 's> {
        ListBuilder {
            builder: self.builder.reborrow(),
            element: self.element.clone(),
        }
    }
    pub fn into_reader(self) -> ListReader<'a, 's> {
        ListReader {
            reader: self.builder.into_reader(),
            element: self.element,
        }
    }
    pub fn set(&mut self, index: u32, value: Value<'_, '_>) -> Result<()> {
        require(index < self.len(), "list index out of bounds")?;
        value.check(&self.element)?;
        match value {
            Value::Void => (),
            Value::Bool(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Int8(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Int16(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Int32(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Int64(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::UInt8(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::UInt16(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::UInt32(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::UInt64(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Float32(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Float64(v) => PrimitiveElement::set(&self.builder, index, v),
            Value::Enum(v, _) => PrimitiveElement::set(&self.builder, index, v),
            Value::Struct(s) => self
                .builder
                .reborrow()
                .get_struct_element(index)
                .copy_content_from_strict(&s.reader)?,
            v => write_pointer(self.builder.reborrow().get_pointer_element(index), v)?,
        }
        Ok(())
    }
    /// Borrow a text element without replacing its storage. Null elements
    /// return empty views. Bounds and element type are checked first.
    pub fn get_text(self, index: u32) -> Result<crate::text::Builder<'a>> {
        require(index < self.len(), "list index out of bounds")?;
        require(matches!(self.element, Type::Text), "not a text list")?;
        self.builder.get_pointer_element(index).get_text(None)
    }
    /// Borrow a data element without replacing its storage. Null elements
    /// return empty views and do not materialize an empty pointer.
    /// The element and its parent cannot be mutated concurrently:
    ///
    /// ```compile_fail
    /// use capnp::schema_loader::dynamic::ListBuilder;
    /// fn alias(mut list: ListBuilder<'_, '_>) {
    ///     let bytes = list.reborrow().get_data(0).unwrap();
    ///     list.init_data(0, 2).unwrap();
    ///     bytes[0] = 1;
    /// }
    /// ```
    pub fn get_data(self, index: u32) -> Result<crate::data::Builder<'a>> {
        require(index < self.len(), "list index out of bounds")?;
        require(matches!(self.element, Type::Data), "not a data list")?;
        self.builder.get_pointer_element(index).get_data(None)
    }
    /// Replace a text element with `size` zeroed content bytes and a NUL
    /// terminator, checking bounds, type and wire size before replacement.
    pub fn init_text(self, index: u32, size: u32) -> Result<crate::text::Builder<'a>> {
        require(index < self.len(), "list index out of bounds")?;
        require(matches!(self.element, Type::Text), "not a text list")?;
        require(size < (1 << 29) - 1, "text exceeds wire limit")?;
        Ok(self.builder.get_pointer_element(index).init_text(size))
    }
    /// Replace a data element with `size` zeroed bytes, checking bounds, type
    /// and wire size before replacement.
    pub fn init_data(self, index: u32, size: u32) -> Result<crate::data::Builder<'a>> {
        require(index < self.len(), "list index out of bounds")?;
        require(matches!(self.element, Type::Data), "not a data list")?;
        require(size < 1 << 29, "data exceeds wire limit")?;
        Ok(self.builder.get_pointer_element(index).init_data(size))
    }
    pub fn get_struct(self, index: u32) -> Result<Builder<'a, 's>> {
        require(index < self.len(), "list index out of bounds")?;
        let Type::Struct(schema) = self.element else {
            return Err(invalid("not a struct list"));
        };
        Ok(Builder {
            builder: self.builder.get_struct_element(index),
            schema,
        })
    }
    /// Access an existing nested list without replacing its contents.
    /// Null elements yield empty typed views; older struct-list layouts are
    /// upgraded when necessary. Generic bindings and capability tables survive
    /// access. Bounds and element type are checked before touching the pointer.
    /// Use `init_list()` to replace an element with a newly allocated list.
    /// A nested builder retains the exclusive borrow of its parent:
    ///
    /// ```compile_fail
    /// use capnp::schema_loader::dynamic::ListBuilder;
    /// fn alias(mut list: ListBuilder<'_, '_>) {
    ///     let child = list.reborrow().get_list(0).unwrap();
    ///     list.reborrow().init_list(0, 2).unwrap();
    ///     assert_eq!(child.len(), 1);
    /// }
    /// ```
    pub fn get_list(self, index: u32) -> Result<Self> {
        require(index < self.len(), "list index out of bounds")?;
        let Type::List(element) = self.element else {
            return Err(invalid("not a nested list"));
        };
        list_builder(self.builder.get_pointer_element(index), *element)
    }
    pub fn init_list(self, index: u32, count: u32) -> Result<Self> {
        require(index < self.len(), "list index out of bounds")?;
        let Type::List(element) = self.element else {
            return Err(invalid("not a nested list"));
        };
        init_list(self.builder.get_pointer_element(index), *element, count)
    }
}
#[derive(Clone)]
pub struct Client<'a> {
    client: Option<capability::Client>,
    schema: Option<Schema<'a>>,
}
impl<'a> Client<'a> {
    /// Metadata describes the capability already held; it grants no authority.
    pub fn new<C: capability::FromClientHook>(client: C, schema: Schema<'a>) -> Result<Self> {
        require(schema.kind() == Kind::Interface, "not an interface schema")?;
        Ok(Self {
            client: Some(capability::Client::new(client.into_client_hook())),
            schema: Some(schema),
        })
    }
    pub fn null(schema: Option<Schema<'a>>) -> Self {
        Self {
            client: None,
            schema,
        }
    }
    pub fn as_client(&self) -> Result<&capability::Client> {
        self.client
            .as_ref()
            .ok_or_else(|| invalid("null capability"))
    }
    pub fn schema(&self) -> Option<Schema<'a>> {
        self.schema.clone()
    }
    pub fn new_request(&self, name: &str) -> Result<Request<'a>> {
        let schema = self
            .schema
            .as_ref()
            .ok_or_else(|| invalid("capability has no interface metadata"))?;
        self.new_request_for(schema.method(name)?)
    }
    pub fn new_request_for(&self, method: Method<'a>) -> Result<Request<'a>> {
        if let Some(s) = &self.schema {
            require(
                s.extends(&method.parent)?,
                "method belongs to another interface",
            )?;
        }
        let call = self.as_client()?.hook.new_call_with_hints(
            method.parent.id,
            method.index,
            None,
            capability::CallHints {
                no_promise_pipelining: !method.results()?.may_contain_capabilities()?,
                only_promise_pipeline: false,
            },
        );
        Ok(Request {
            hook: call.hook,
            method,
        })
    }
}
pub struct Request<'a> {
    hook: Box<dyn crate::private::capability::RequestHook>,
    method: Method<'a>,
}
impl<'a> Request<'a> {
    pub fn get(&mut self) -> Result<Builder<'_, 'a>> {
        Builder::new(self.hook.get(), self.method.params()?)
    }
    pub fn send(self) -> Result<RemoteCall<'a>> {
        let schema = self.method.results()?;
        let result = self.hook.send();
        Ok(RemoteCall {
            promise: result.promise,
            pipeline: Pipeline {
                pipeline: result.pipeline,
                schema: schema.clone(),
            },
            schema,
        })
    }
    pub fn send_for_pipeline(self) -> Result<Pipeline<'a>> {
        let schema = self.method.results()?;
        Ok(Pipeline {
            pipeline: self.hook.send_for_pipeline(),
            schema,
        })
    }
    pub fn send_ignoring_result(self) -> capability::Promise<(), Error> {
        capability::Request::<any_pointer::Owned, any_pointer::Owned>::new(self.hook)
            .send_ignoring_result()
    }
    pub fn send_streaming(self) -> Result<capability::Promise<(), Error>> {
        // StreamResult is the protocol's distinguished empty streaming result.
        require(
            self.method.get_proto().get_result_struct_type() == 0x995f9a3377c0b16e,
            "method is not streaming",
        )?;
        Ok(self.hook.send_streaming())
    }
}
pub struct RemoteCall<'a> {
    promise: capability::Promise<capability::Response<any_pointer::Owned>, Error>,
    pub pipeline: Pipeline<'a>,
    schema: Schema<'a>,
}
impl<'a> RemoteCall<'a> {
    pub async fn resolve(self) -> Result<Response<'a>> {
        let Self {
            promise,
            pipeline,
            schema,
        } = self;
        drop(pipeline);
        Ok(Response {
            response: promise.await?,
            schema,
        })
    }
}
pub struct Response<'a> {
    response: capability::Response<any_pointer::Owned>,
    schema: Schema<'a>,
}
impl<'a> Response<'a> {
    pub fn get(&self) -> Result<Reader<'_, 'a>> {
        Reader::new(self.response.get()?, self.schema.clone())
    }
}
pub struct Pipeline<'a> {
    pipeline: any_pointer::Pipeline,
    schema: Schema<'a>,
}
impl Clone for Pipeline<'_> {
    fn clone(&self) -> Self {
        Self {
            pipeline: self.pipeline.noop(),
            schema: self.schema.clone(),
        }
    }
}
pub enum PipelineValue<'a> {
    Struct(Pipeline<'a>),
    Capability(Client<'a>),
}
impl<'a> Pipeline<'a> {
    /// Attach loaded result metadata to an independently built pipeline.
    pub fn new(pipeline: any_pointer::Pipeline, schema: Schema<'a>) -> Result<Self> {
        schema.struct_size()?;
        Ok(Self { pipeline, schema })
    }
    pub fn get(&self, name: &str) -> Result<PipelineValue<'a>> {
        let f = self.schema.field(name)?;
        require(
            f.get_proto().get_discriminant_value() == field::NO_DISCRIMINANT,
            "cannot pipeline a union field",
        )?;
        let pipeline = match f.get_proto().which()? {
            field::Group(_) => self.pipeline.noop(),
            field::Slot(s) => self.pipeline.get_pointer_field(
                s.get_offset()
                    .try_into()
                    .map_err(|_| invalid("pipeline offset overflow"))?,
            ),
        };
        Ok(match f.get_type()? {
            Type::Struct(schema) => PipelineValue::Struct(Self { pipeline, schema }),
            Type::Interface(schema) => PipelineValue::Capability(Client::new(
                capability::Client::new(pipeline.as_cap()),
                schema,
            )?),
            Type::AnyPointer(PointerKind::Capability) => PipelineValue::Capability(Client {
                client: Some(capability::Client::new(pipeline.as_cap())),
                schema: None,
            }),
            _ => return Err(invalid("not a pipeline field")),
        })
    }
}

/// An owned interface binding for a server or a long-lived reflected client.
/// The loader is immutable while shared; no reflected reader needs a static lifetime.
#[derive(Clone)]
pub struct ServiceSchema {
    loader: Rc<SchemaLoader>,
    id: u64,
    brand: Option<Rc<message::Builder<message::HeapAllocator>>>,
}
impl ServiceSchema {
    pub fn new(loader: Rc<SchemaLoader>, id: u64) -> Result<Self> {
        require(
            loader.get(id)?.kind() == Kind::Interface,
            "not an interface schema",
        )?;
        Ok(Self {
            loader,
            id,
            brand: None,
        })
    }
    pub fn with_brand(mut self, brand: brand::Reader<'_>) -> Result<Self> {
        self.loader.get(self.id)?.bind(brand, None)?;
        let mut m = message::Builder::new_default();
        m.set_root(brand)?;
        self.brand = Some(Rc::new(m));
        Ok(self)
    }
    pub fn get(&self) -> Result<Schema<'_>> {
        let schema = self.loader.get(self.id)?;
        match &self.brand {
            Some(b) => schema.bind(b.get_root_as_reader()?, None),
            None => Ok(schema),
        }
    }
    pub fn reflect<C: capability::FromClientHook>(&self, client: C) -> Result<Client<'_>> {
        Client::new(client, self.get()?)
    }
    fn find_path(&self, interface_id: u64) -> Result<Vec<usize>> {
        fn search(schema: Schema<'_>, id: u64, path: &mut Vec<usize>) -> Result<bool> {
            require(path.len() < 64, "interface inheritance depth limit")?;
            if schema.id == id {
                return Ok(true);
            }
            for (index, parent) in schema.superclasses()?.into_iter().enumerate() {
                path.push(index);
                if search(parent, id, path)? {
                    return Ok(true);
                }
                path.pop();
            }
            Ok(false)
        }
        let mut path = Vec::new();
        if search(self.get()?, interface_id, &mut path)? {
            Ok(path)
        } else {
            Err(Error::unimplemented("interface not implemented".into()))
        }
    }
    fn method(&self, path: &[usize], index: u16) -> Result<Method<'_>> {
        let mut schema = self.get()?;
        for &i in path {
            schema = schema
                .superclasses()?
                .into_iter()
                .nth(i)
                .ok_or_else(|| invalid("invalid interface path"))?;
        }
        schema
            .methods()?
            .into_iter()
            .nth(index as usize)
            .ok_or_else(|| Error::unimplemented("method not implemented".into()))
    }
}
pub trait Server: 'static {
    fn call(self: Rc<Self>, context: CallContext) -> capability::Promise<(), Error>;
    fn allow_cancellation(&self) -> bool {
        false
    }
    fn get_hooks(&self) -> Option<&dyn capability::ServerHooks> {
        None
    }
}
pub struct CallContext {
    schema: ServiceSchema,
    path: Vec<usize>,
    index: u16,
    params: Option<capability::Params<any_pointer::Owned>>,
    results: capability::Results<any_pointer::Owned>,
}
impl CallContext {
    fn check_results(&self) -> Result<()> {
        require(
            self.method()?.get_proto().get_result_struct_type() != 0x995f9a3377c0b16e,
            "streaming method has no results",
        )
    }
    pub fn method(&self) -> Result<Method<'_>> {
        self.schema.method(&self.path, self.index)
    }
    pub fn get_params(&self) -> Result<Reader<'_, '_>> {
        let params = self
            .params
            .as_ref()
            .ok_or_else(|| invalid("parameters already released"))?;
        Reader::new(params.get()?, self.method()?.params()?)
    }
    /// Borrow parameters and results together, allowing capability and struct
    /// values to be copied without extending the lifetime of this call context.
    pub fn get(&mut self) -> Result<(Reader<'_, '_>, Builder<'_, '_>)> {
        self.check_results()?;
        let method = self.schema.method(&self.path, self.index)?;
        let params = self
            .params
            .as_ref()
            .ok_or_else(|| invalid("parameters already released"))?;
        Ok((
            Reader::new(params.get()?, method.params()?)?,
            Builder::new(self.results.hook.get()?, method.results()?)?,
        ))
    }
    pub fn release_params(&mut self) {
        self.params.take();
    }
    pub fn get_results(&mut self) -> Result<Builder<'_, '_>> {
        self.get_results_with_size_hint(None)
    }
    pub fn get_results_with_size_hint(
        &mut self,
        size_hint: Option<crate::MessageSize>,
    ) -> Result<Builder<'_, '_>> {
        self.check_results()?;
        let schema = self.schema.method(&self.path, self.index)?.results()?;
        Builder::new(self.results.hook.get_with_size_hint(size_hint)?, schema)
    }
    pub fn init_results(
        &mut self,
        size_hint: Option<crate::MessageSize>,
    ) -> Result<Builder<'_, '_>> {
        self.check_results()?;
        let schema = self.schema.method(&self.path, self.index)?.results()?;
        Builder::init(self.results.hook.get_with_size_hint(size_hint)?, schema)
    }
    pub fn get_results_orphanage(
        &mut self,
        size_hint: Option<crate::MessageSize>,
    ) -> Result<(orphan::Root<'_, '_>, orphan::Orphanage<'_>)> {
        self.check_results()?;
        let schema = self.schema.method(&self.path, self.index)?.results()?;
        Ok(
            orphan::Root::new(self.results.hook.get_with_size_hint(size_hint)?, schema)?
                .with_orphanage(),
        )
    }
    pub fn set_pipeline(&mut self) -> Result<()> {
        self.results.set_pipeline()
    }
    /// Publish an independent pipeline using the exact loaded result schema.
    /// Eventual result capabilities must resolve to the same hook identities.
    pub fn set_pipeline_from(&mut self, pipeline: Pipeline<'_>) -> Result<()> {
        self.check_results()?;
        require(
            self.method()?.results()?.equals(&pipeline.schema),
            "pipeline result schema mismatch",
        )?;
        self.results
            .hook
            .set_pipeline_from(pipeline.pipeline.into_hook())
    }
    pub fn tail_call(self, request: Request<'_>) -> capability::Promise<(), Error> {
        self.results.hook.tail_call(request.hook)
    }
}
pub struct ServerDispatch<S: Server> {
    pub server: Rc<S>,
    pub schema: ServiceSchema,
}
impl<S: Server> Clone for ServerDispatch<S> {
    fn clone(&self) -> Self {
        Self {
            server: self.server.clone(),
            schema: self.schema.clone(),
        }
    }
}
impl<S: Server> capability::Server for ServerDispatch<S> {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.server) as usize
    }
    fn get_hooks(&self) -> Option<&dyn capability::ServerHooks> {
        self.server.get_hooks()
    }
    fn dispatch_call(
        self,
        interface_id: u64,
        method_id: u16,
        params: capability::Params<any_pointer::Owned>,
        results: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        let found = self.schema.find_path(interface_id).and_then(|path| {
            let method = self.schema.method(&path, method_id)?;
            let streaming = method.get_proto().get_result_struct_type() == 0x995f9a3377c0b16e;
            Ok((path, streaming))
        });
        match found {
            Ok((path, streaming)) => {
                let allow = self.server.allow_cancellation();
                capability::DispatchCallResult::with_cancellation_policy(
                    self.server.call(CallContext {
                        schema: self.schema,
                        path,
                        index: method_id,
                        params: Some(params),
                        results,
                    }),
                    streaming,
                    allow,
                )
            }
            Err(error) => {
                capability::DispatchCallResult::new(capability::Promise::err(error), false)
            }
        }
    }
}

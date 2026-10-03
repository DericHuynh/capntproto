//! Detached ownership for runtime-loaded schemas. Message and schema lifetimes
//! remain separate; no schema is leaked or promoted to a static lifetime.
use super::*;
use crate::{private::layout::DetachedObject, ErrorKind};
use core::marker::PhantomData;
mod group;
mod root;
pub use group::{GroupEditor, GroupReader};
pub use root::Root;

pub use crate::dynamic_orphan::ExternalData;
pub use crate::field_api::AdoptError;

/// A message-borrow token, including the capability-table identity.
pub struct Orphanage<'message>(crate::dynamic_orphan::Orphanage<'message>);
impl Orphanage<'_> {
    fn new(identity: layout::ArenaIdentity) -> Self {
        Self(crate::dynamic_orphan::Orphanage::new(identity))
    }
}
/// Move-only ownership. Failed adoption returns this owner to the caller.
pub struct Orphan<'message, 'schema> {
    data: Box<OwnedState<'message, 'schema>>,
}
struct OwnedState<'message, 'schema> {
    ty: Type<'schema>,
    identity: layout::ArenaIdentity,
    value: OwnedValue<'message, 'schema>,
    lifetime: PhantomData<&'message mut ()>,
}
enum OwnedValue<'message, 'schema> {
    Scalar(Value<'schema, 'schema>),
    Pointer(DetachedObject),
    Group(Vec<(Field<'schema>, Orphan<'message, 'schema>)>),
}
fn mismatch() -> Error {
    Error::from_kind(ErrorKind::TypeMismatch)
}
fn is_group(ty: &Type<'_>) -> Result<bool> {
    Ok(match ty {
        Type::Struct(s) => match s.get_proto().which()? {
            node::Struct(s) => s.get_is_group(),
            _ => false,
        },
        _ => false,
    })
}
fn scalar<'s>(value: Value<'_, 's>) -> Result<Value<'s, 's>> {
    Ok(match value {
        Value::Void => Value::Void,
        Value::Bool(v) => Value::Bool(v),
        Value::Int8(v) => Value::Int8(v),
        Value::Int16(v) => Value::Int16(v),
        Value::Int32(v) => Value::Int32(v),
        Value::Int64(v) => Value::Int64(v),
        Value::UInt8(v) => Value::UInt8(v),
        Value::UInt16(v) => Value::UInt16(v),
        Value::UInt32(v) => Value::UInt32(v),
        Value::UInt64(v) => Value::UInt64(v),
        Value::Float32(v) => Value::Float32(v),
        Value::Float64(v) => Value::Float64(v),
        Value::Enum(v, schema) => Value::Enum(v, schema),
        _ => return Err(mismatch()),
    })
}
impl<'m, 's> Orphan<'m, 's> {
    fn new(ty: Type<'s>, token: &Orphanage<'m>, value: OwnedValue<'m, 's>) -> Self {
        Self {
            data: Box::new(OwnedState {
                ty,
                identity: token.0.identity,
                value,
                lifetime: PhantomData,
            }),
        }
    }
    pub fn get_type(&self) -> Type<'s> {
        self.data.ty.clone()
    }
    fn check(&self, target: &Type<'_>, identity: layout::ArenaIdentity) -> Result<()> {
        if self.data.identity != identity {
            return Err(Error::from_kind(ErrorKind::WrongArena));
        }
        let compatible = match (target, &self.data.ty) {
            (Type::AnyPointer(PointerKind::Any), source) => {
                source.is_pointer() && !is_group(source)?
            }
            (Type::AnyPointer(PointerKind::Struct), Type::Struct(_)) => !is_group(&self.data.ty)?,
            (Type::AnyPointer(PointerKind::List), Type::List(_) | Type::Text | Type::Data) => true,
            (Type::AnyPointer(PointerKind::Capability), Type::Interface(_)) => true,
            (Type::Interface(target), Type::Interface(source)) => source.extends(target)?,
            _ => *target == self.data.ty,
        };
        if !compatible {
            return Err(mismatch());
        }
        match &self.data.value {
            OwnedValue::Pointer(object) => object.check_adopt(identity),
            OwnedValue::Group(fields) => {
                for (f, owner) in fields {
                    owner.check(&f.get_type()?, identity)?;
                }
                Ok(())
            }
            OwnedValue::Scalar(_) => Ok(()),
        }
    }
}
fn selected_fields<'s>(reader: &Reader<'_, 's>) -> Result<Vec<Field<'s>>> {
    let node::Struct(s) = reader.schema.get_proto().which()? else {
        return Err(mismatch());
    };
    let mut fields = Vec::new();
    if s.get_discriminant_count() != 0 {
        fields.push(
            reader
                .which()?
                .ok_or_else(|| invalid("unknown union arm"))?,
        );
    }
    for f in reader.schema.fields()? {
        if f.get_proto().get_discriminant_value() == field::NO_DISCRIMINANT
            && reader.has(f.clone())?
        {
            fields.push(f);
        }
    }
    Ok(fields)
}
fn validate_group(schema: &Schema<'_>, depth: usize) -> Result<()> {
    require(depth < 64, "orphan group nesting limit")?;
    schema.struct_size()?;
    for f in schema.fields()? {
        let ty = f.get_type()?;
        require(
            !matches!(ty, Type::Unknown(_) | Type::Parameter(..)),
            "unknown orphan field layout",
        )?;
        if matches!(f.get_proto().which()?, field::Group(_)) {
            let Type::Struct(s) = ty else {
                return Err(mismatch());
            };
            validate_group(&s, depth + 1)?;
        }
    }
    Ok(())
}
impl<'a, 's: 'a> Builder<'a, 's> {
    /// Move an entire same-schema orphan into this struct, retaining ownership on
    /// failure. Group adoption changes only the group's fields, not its siblings.
    pub fn adopt_content<'m>(
        &mut self,
        mut orphan: Orphan<'m, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m, 's>>> {
        let result = (|| {
            orphan.check(&Type::Struct(self.schema.clone()), self.builder.identity())?;
            if is_group(&orphan.data.ty)? {
                orphan.dematerialize(&mut self.builder.orphan_anchor())?;
            }
            match &mut orphan.data.value {
                OwnedValue::Pointer(object) => self.builder.adopt_content(object),
                OwnedValue::Group(fields) => self.adopt_group(fields),
                _ => Err(mismatch()),
            }
        })();
        result.map_err(|error| AdoptError { error, orphan })
    }

    pub fn with_orphanage(self) -> (Self, Orphanage<'a>) {
        let token = Orphanage::new(self.builder.identity());
        (self, token)
    }
    pub fn disown_named<'m>(
        &mut self,
        name: &str,
        token: &Orphanage<'m>,
    ) -> Result<Orphan<'m, 's>> {
        self.disown(self.schema.field(name)?, token)
    }
    pub fn disown<'m>(&mut self, f: Field<'s>, token: &Orphanage<'m>) -> Result<Orphan<'m, 's>> {
        token.0.check(self.builder.identity())?;
        self.prepare_disown(&f, 0)?;
        self.disown_prepared(f, token)
    }
    fn prepare_disown(&mut self, f: &Field<'s>, depth: usize) -> Result<()> {
        require(depth < 64, "orphan group nesting limit")?;
        if !self.schema.equals(&f.parent) {
            return Err(mismatch());
        }
        if f.get_proto().get_discriminant_value() != field::NO_DISCRIMINANT {
            require(
                self.as_reader()
                    .which()?
                    .is_some_and(|a| a.index == f.index),
                "cannot disown inactive union arm",
            )?;
        }
        let ty = f.get_type()?;
        match f.get_proto().which()? {
            field::Slot(slot) if ty.is_pointer() => {
                let mut p = self
                    .builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize);
                if p.is_null() {
                    match slot.get_default_value()?.which()? {
                        value::Text(t) => p.set_text(t?),
                        value::Data(d) => p.set_data(d?),
                        value::Struct(d) | value::List(d) | value::AnyPointer(d) => {
                            p.copy_from(d.reader, false)?
                        }
                        _ => (),
                    }
                }
                // Upgrade through the builder, like compiled/C++ reflection.
                // Immutable external Data can be detached without a write view.
                if !p.is_external()? {
                    match &ty {
                        Type::Struct(s) => {
                            p.reborrow().get_struct(s.struct_size()?, None)?;
                        }
                        Type::List(e) => {
                            list_builder(p.reborrow(), *e.clone())?;
                        }
                        _ => (),
                    }
                }
                read_pointer(p.as_reader(), ty)?;
                p.check_disown()
            }
            field::Slot(_) => {
                scalar(self.as_reader().get(f.clone())?)?;
                Ok(())
            }
            field::Group(_) => {
                let Type::Struct(schema) = ty else {
                    return Err(mismatch());
                };
                validate_group(&schema, depth)?;
                let mut group = Builder {
                    builder: self.builder.reborrow(),
                    schema,
                };
                for child in selected_fields(&group.as_reader())? {
                    group.prepare_disown(&child, depth + 1)?;
                }
                Ok(())
            }
        }
    }
    fn disown_prepared<'m>(
        &mut self,
        f: Field<'s>,
        token: &Orphanage<'m>,
    ) -> Result<Orphan<'m, 's>> {
        let ty = f.get_type()?;
        let value = match f.get_proto().which()? {
            field::Slot(slot) if ty.is_pointer() => OwnedValue::Pointer(
                self.builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize)
                    .disown()?,
            ),
            field::Slot(_) => {
                let value = scalar(self.as_reader().get(f.clone())?)?;
                self.clear(f)?;
                OwnedValue::Scalar(value)
            }
            field::Group(_) => {
                let Type::Struct(schema) = &ty else {
                    return Err(mismatch());
                };
                let mut group = Builder {
                    builder: self.builder.reborrow(),
                    schema: schema.clone(),
                };
                OwnedValue::Group(group.disown_group(token)?)
            }
        };
        Ok(Orphan::new(ty, token, value))
    }
    fn disown_group<'m>(
        &mut self,
        token: &Orphanage<'m>,
    ) -> Result<Vec<(Field<'s>, Orphan<'m, 's>)>> {
        let fields = selected_fields(&self.as_reader())?;
        for f in &fields {
            self.prepare_disown(f, 0)?;
        }
        let mut result = Vec::new();
        for f in fields {
            result.push((f.clone(), self.disown_prepared(f, token)?));
        }
        if let Some(default) = self
            .schema
            .fields()?
            .into_iter()
            .find(|f| f.get_proto().get_discriminant_value() == 0)
        {
            self.clear(default)?;
        }
        Ok(result)
    }
    pub fn adopt_named<'m>(
        &mut self,
        name: &str,
        orphan: Orphan<'m, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m, 's>>> {
        match self.schema.field(name) {
            Ok(f) => self.adopt(f, orphan),
            Err(error) => Err(AdoptError { error, orphan }),
        }
    }
    pub fn adopt<'m>(
        &mut self,
        f: Field<'s>,
        mut orphan: Orphan<'m, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m, 's>>> {
        self.adopt_inner(f, &mut orphan)
            .map_err(|error| AdoptError { error, orphan })
    }
    fn adopt_inner(&mut self, f: Field<'s>, orphan: &mut Orphan<'_, 's>) -> Result<()> {
        if !self.schema.equals(&f.parent) {
            return Err(mismatch());
        }
        orphan.check(&f.get_type()?, self.builder.identity())?;
        if matches!(f.get_proto().which()?, field::Group(_)) {
            orphan.dematerialize(&mut self.builder.orphan_anchor())?;
        }
        match (f.get_proto().which()?, &mut orphan.data.value) {
            (field::Slot(slot), OwnedValue::Pointer(object)) => {
                self.builder
                    .reborrow()
                    .get_pointer_field(slot.get_offset() as usize)
                    .adopt(object)?;
            }
            (field::Slot(_), OwnedValue::Scalar(v)) => return self.set(f, v.clone()),
            (field::Group(_), OwnedValue::Group(fields)) => {
                let Type::Struct(schema) = f.get_type()? else {
                    return Err(mismatch());
                };
                Builder {
                    builder: self.builder.reborrow(),
                    schema,
                }
                .adopt_group(fields)?;
            }
            _ => return Err(mismatch()),
        }
        self.activate(&f)
    }
    fn adopt_group(&mut self, fields: &mut [(Field<'s>, Orphan<'_, 's>)]) -> Result<()> {
        validate_group(&self.schema, 0)?;
        for (f, owner) in fields.iter() {
            if !self.schema.equals(&f.parent) {
                return Err(mismatch());
            }
            owner.check(&f.get_type()?, self.builder.identity())?;
        }
        self.clear_for_adoption()?;
        for (f, owner) in fields {
            self.adopt_inner(f.clone(), owner)?;
        }
        Ok(())
    }

    fn clear_for_adoption(&mut self) -> Result<()> {
        // Adoption replaces owned contents, including hidden pointer owners.
        // Ordinary group clear/init deliberately preserve inactive storage to
        // match C++; do not use that selective reset for this cleanup contract.
        // adopt_group validates the complete group graph before reaching here.
        for f in self.schema.fields()? {
            if matches!(f.get_proto().which()?, field::Group(_)) {
                let Type::Struct(schema) = f.get_type()? else {
                    return Err(mismatch());
                };
                Builder {
                    builder: self.builder.reborrow(),
                    schema,
                }
                .clear_for_adoption()?;
            } else {
                self.clear(f)?;
            }
        }
        if let Some(default) = self.schema.field_by_discriminant(0)? {
            self.activate(&default)?;
        }
        Ok(())
    }
}
impl<'a, 's: 'a> ListBuilder<'a, 's> {
    pub fn with_orphanage(self) -> (Self, Orphanage<'a>) {
        let token = Orphanage::new(self.builder.identity());
        (self, token)
    }
    pub fn as_reader(&self) -> ListReader<'_, 's> {
        ListReader {
            reader: self.builder.as_reader(),
            element: self.element.clone(),
        }
    }
    pub fn disown<'m>(&mut self, index: u32, token: &Orphanage<'m>) -> Result<Orphan<'m, 's>> {
        token.0.check(self.builder.identity())?;
        require(index < self.len(), "list index out of bounds")?;
        let value = match &self.element {
            Type::Struct(_) => OwnedValue::Pointer(
                self.builder
                    .reborrow()
                    .get_struct_element(index)
                    .disown_content()?,
            ),
            t if t.is_pointer() => {
                let mut p = self.builder.reborrow().get_pointer_element(index);
                read_pointer(p.as_reader(), self.element.clone())?;
                OwnedValue::Pointer(p.disown()?)
            }
            t => {
                let value = scalar(self.as_reader().get(index)?)?;
                match t {
                    Type::Void => (),
                    Type::Bool => PrimitiveElement::set(&self.builder, index, false),
                    Type::Int8 | Type::UInt8 => PrimitiveElement::set(&self.builder, index, 0u8),
                    Type::Int16 | Type::UInt16 | Type::Enum(_) => {
                        PrimitiveElement::set(&self.builder, index, 0u16)
                    }
                    Type::Int32 | Type::UInt32 | Type::Float32 => {
                        PrimitiveElement::set(&self.builder, index, 0u32)
                    }
                    Type::Int64 | Type::UInt64 | Type::Float64 => {
                        PrimitiveElement::set(&self.builder, index, 0u64)
                    }
                    _ => return Err(mismatch()),
                }
                OwnedValue::Scalar(value)
            }
        };
        Ok(Orphan::new(self.element.clone(), token, value))
    }
    pub fn adopt<'m>(
        &mut self,
        index: u32,
        mut orphan: Orphan<'m, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m, 's>>> {
        let result = (|| {
            require(index < self.len(), "list index out of bounds")?;
            orphan.check(&self.element, self.builder.identity())?;
            match &mut orphan.data.value {
                OwnedValue::Pointer(object) if matches!(self.element, Type::Struct(_)) => self
                    .builder
                    .reborrow()
                    .get_struct_element(index)
                    .adopt_content(object),
                OwnedValue::Pointer(object) => self
                    .builder
                    .reborrow()
                    .get_pointer_element(index)
                    .adopt(object),
                OwnedValue::Scalar(v) => self.set(index, v.clone()),
                _ => Err(mismatch()),
            }
        })();
        result.map_err(|error| AdoptError { error, orphan })
    }
}

/// A live exclusive editor is required to access detached arena bytes.
pub struct Access<'a, 'message> {
    anchor: layout::PointerBuilder<'a>,
    token: Orphanage<'message>,
}
impl<'m> Orphanage<'m> {
    pub fn in_root<'a>(&self, root: &'a mut Root<'_, '_>) -> Result<Access<'a, 'm>> {
        self.0.check(root.pointer.builder.identity())?;
        Ok(Access {
            anchor: root.pointer.builder.reborrow(),
            token: Orphanage::new(self.0.identity),
        })
    }
    pub fn in_struct<'a>(&self, root: &'a mut Builder<'_, '_>) -> Result<Access<'a, 'm>> {
        self.0.check(root.builder.identity())?;
        Ok(Access {
            anchor: root.builder.orphan_anchor(),
            token: Orphanage::new(self.0.identity),
        })
    }
    pub fn in_list<'a>(&self, root: &'a mut ListBuilder<'_, '_>) -> Result<Access<'a, 'm>> {
        self.0.check(root.builder.identity())?;
        Ok(Access {
            anchor: root.builder.orphan_anchor(),
            token: Orphanage::new(self.0.identity),
        })
    }
}
/// Scoped mutable views. Scalar and capability values are passed by value;
/// aggregate/blob editors change the orphan and retain edits on callback error.
pub enum Editor<'a, 's: 'a> {
    Scalar(Value<'s, 's>),
    Text(crate::text::Builder<'a>),
    Data(&'a mut [u8]),
    Struct(Builder<'a, 's>),
    List(ListBuilder<'a, 's>),
    AnyPointer(any_pointer::Builder<'a>),
    Capability(Client<'s>),
}
fn editor<'a, 's: 'a>(p: layout::PointerBuilder<'a>, ty: Type<'s>) -> Result<Editor<'a, 's>> {
    Ok(match ty {
        Type::Text => Editor::Text(p.get_text(None)?),
        Type::Data => Editor::Data(p.get_data(None)?),
        Type::Struct(schema) => Editor::Struct(Builder {
            builder: p.get_struct(schema.struct_size()?, None)?,
            schema,
        }),
        Type::List(element) => Editor::List(list_builder(p, *element)?),
        ty @ (Type::Interface(_) | Type::AnyPointer(PointerKind::Capability)) => {
            let Value::Capability(c) = read_pointer(p.into_reader(), ty)? else {
                unreachable!()
            };
            Editor::Capability(c)
        }
        ty @ Type::AnyPointer(_) => {
            read_pointer(p.as_reader(), ty)?;
            Editor::AnyPointer(any_pointer::Builder::new(p))
        }
        _ => return Err(mismatch()),
    })
}
fn value_type<'s>(value: &Value<'_, 's>) -> Type<'s> {
    match value {
        Value::Unknown(t) => Type::Unknown(*t),
        Value::Void => Type::Void,
        Value::Bool(_) => Type::Bool,
        Value::Int8(_) => Type::Int8,
        Value::Int16(_) => Type::Int16,
        Value::Int32(_) => Type::Int32,
        Value::Int64(_) => Type::Int64,
        Value::UInt8(_) => Type::UInt8,
        Value::UInt16(_) => Type::UInt16,
        Value::UInt32(_) => Type::UInt32,
        Value::UInt64(_) => Type::UInt64,
        Value::Float32(_) => Type::Float32,
        Value::Float64(_) => Type::Float64,
        Value::Enum(_, s) => Type::Enum(s.clone()),
        Value::Text(_) => Type::Text,
        Value::Data(_) => Type::Data,
        Value::Struct(s) => Type::Struct(s.schema.clone()),
        Value::List(l) => Type::List(Box::new(l.element.clone())),
        Value::AnyPointer(_) => Type::AnyPointer(PointerKind::Any),
        Value::Capability(c) => c
            .schema
            .clone()
            .map_or(Type::AnyPointer(PointerKind::Capability), Type::Interface),
    }
}
fn concrete(ty: &Type<'_>, depth: usize) -> Result<()> {
    require(depth < 64, "orphan type nesting limit")?;
    match ty {
        Type::Unknown(_) | Type::Parameter(..) => Err(mismatch()),
        Type::List(element) => {
            require(!is_group(element)?, "groups cannot be list elements")?;
            concrete(element, depth + 1)
        }
        _ => Ok(()),
    }
}
impl<'m> Access<'_, 'm> {
    fn build<'s>(
        &mut self,
        ty: Type<'s>,
        build: impl for<'b> FnOnce(layout::PointerBuilder<'b>) -> Result<()>,
    ) -> Result<Orphan<'m, 's>> {
        concrete(&ty, 0)?;
        let mut object = DetachedObject::empty(self.anchor.allocate_detached());
        object.access(&mut self.anchor, build)?;
        Ok(Orphan::new(ty, &self.token, OwnedValue::Pointer(object)))
    }
    pub fn new_struct<'s>(&mut self, schema: Schema<'s>) -> Result<Orphan<'m, 's>> {
        let size = schema.struct_size()?;
        if is_group(&Type::Struct(schema.clone()))? {
            validate_group(&schema, 0)?;
        }
        self.build(Type::Struct(schema), |p| {
            p.init_struct(size);
            Ok(())
        })
    }
    pub fn new_group<'s>(&mut self, schema: Schema<'s>) -> Result<Orphan<'m, 's>> {
        require(
            is_group(&Type::Struct(schema.clone()))?,
            "not a group schema",
        )?;
        validate_group(&schema, 0)?;
        Ok(Orphan::new(
            Type::Struct(schema),
            &self.token,
            OwnedValue::Group(Vec::new()),
        ))
    }
    pub fn null<'s>(&mut self, ty: Type<'s>) -> Result<Orphan<'m, 's>> {
        require(ty.is_pointer(), "not a pointer type")?;
        if is_group(&ty)? {
            let Type::Struct(s) = &ty else { unreachable!() };
            validate_group(s, 0)?;
        }
        self.build(ty, |_| Ok(()))
    }
    pub fn new_list<'s>(&mut self, element: Type<'s>, count: u32) -> Result<Orphan<'m, 's>> {
        check_list_count(&element, count)?;
        self.build(Type::List(Box::new(element.clone())), |mut p| {
            init_list(p.reborrow(), element, count)?;
            Ok(())
        })
    }
    pub fn new_text<'s>(&mut self, count: u32) -> Result<Orphan<'m, 's>> {
        require(count < (1 << 29) - 1, "text exceeds wire limit")?;
        self.build(Type::Text, |p| {
            p.init_text(count);
            Ok(())
        })
    }
    pub fn new_data<'s>(&mut self, count: u32) -> Result<Orphan<'m, 's>> {
        require(count < 1 << 29, "data exceeds wire limit")?;
        self.build(Type::Data, |p| {
            p.init_data(count);
            Ok(())
        })
    }
    pub fn reference_external_data<'s>(&mut self, data: ExternalData) -> Result<Orphan<'m, 's>> {
        self.build(Type::Data, |p| p.reference_external_data(data))
    }
    pub fn copy<'s>(&mut self, value: Value<'_, 's>) -> Result<Orphan<'m, 's>> {
        let ty = value_type(&value);
        if let Value::Struct(group) = &value {
            if is_group(&ty)? {
                validate_group(&group.schema, 0)?;
                return self.copy_group(group.clone());
            }
        }
        if !ty.is_pointer() {
            return Ok(Orphan::new(
                ty,
                &self.token,
                OwnedValue::Scalar(scalar(value)?),
            ));
        }
        self.build(ty, |p| write_pointer(p, value))
    }
    pub fn copy_group<'s>(&mut self, group: Reader<'_, 's>) -> Result<Orphan<'m, 's>> {
        require(
            is_group(&Type::Struct(group.schema.clone()))?,
            "not a group schema",
        )?;
        validate_group(&group.schema, 0)?;
        let mut fields = Vec::new();
        for f in selected_fields(&group)? {
            fields.push((f.clone(), self.copy(group.get(f)?)?));
        }
        Ok(Orphan::new(
            Type::Struct(group.schema),
            &self.token,
            OwnedValue::Group(fields),
        ))
    }
    pub fn concat<'s>(
        &mut self,
        element: Type<'s>,
        lists: &[ListReader<'_, 's>],
    ) -> Result<Orphan<'m, 's>> {
        let size = match &element {
            Type::Struct(s) => s.struct_size()?,
            _ => layout::StructSize {
                data: 0,
                pointers: 0,
            },
        };
        let mut readers = Vec::new();
        for l in lists {
            if l.element != element {
                return Err(mismatch());
            }
            readers.push(l.reader);
        }
        let size_kind = element.element_size();
        self.build(Type::List(Box::new(element)), |p| {
            p.init_concat_list(size_kind, size, &readers)
        })
    }
    pub fn is_null(&mut self, orphan: &mut Orphan<'_, '_>) -> Result<bool> {
        orphan.check(&orphan.data.ty, self.token.0.identity)?;
        match &mut orphan.data.value {
            OwnedValue::Pointer(object) => object.access(&mut self.anchor, |p| Ok(p.is_null())),
            _ => Ok(false),
        }
    }
    pub fn materialize_group(&mut self, orphan: &mut Orphan<'_, '_>) -> Result<()> {
        orphan.check(&orphan.data.ty, self.token.0.identity)?;
        let Type::Struct(schema) = &orphan.data.ty else {
            return Err(mismatch());
        };
        require(is_group(&orphan.data.ty)?, "not a group schema")?;
        validate_group(schema, 0)?;
        if let OwnedValue::Group(fields) = &mut orphan.data.value {
            let slot = self.anchor.allocate_detached();
            let builder = self
                .anchor
                .detached(&slot)
                .init_struct(schema.struct_size()?);
            Builder {
                builder,
                schema: schema.clone(),
            }
            .adopt_group(fields)?;
            orphan.data.value = OwnedValue::Pointer(self.anchor.detached(&slot).disown()?);
        }
        Ok(())
    }
    /// The schema is reborrowed together with the payload for this callback.
    /// Return owned data or an owned capability hook, not a borrowed view.
    pub fn read<R>(
        &mut self,
        orphan: &mut Orphan<'_, '_>,
        read: impl for<'b> FnOnce(Value<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        orphan.check(&orphan.data.ty, self.token.0.identity)?;
        if matches!(orphan.data.value, OwnedValue::Group(_)) {
            self.materialize_group(orphan)?;
        }
        match &mut orphan.data.value {
            OwnedValue::Scalar(v) => read(v.clone()),
            OwnedValue::Pointer(object) => object.access(&mut self.anchor, |p| {
                read(read_pointer(p.into_reader(), orphan.data.ty.clone())?)
            }),
            OwnedValue::Group(_) => unreachable!(),
        }
    }
    pub fn edit<R>(
        &mut self,
        orphan: &mut Orphan<'_, '_>,
        edit: impl for<'b> FnOnce(Editor<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        orphan.check(&orphan.data.ty, self.token.0.identity)?;
        if matches!(orphan.data.value, OwnedValue::Group(_)) {
            self.materialize_group(orphan)?;
        }
        match &mut orphan.data.value {
            OwnedValue::Scalar(v) => edit(Editor::Scalar(v.clone())),
            OwnedValue::Pointer(object) => object.access(&mut self.anchor, |mut p| {
                edit(editor(p.reborrow(), orphan.data.ty.clone())?)
            }),
            OwnedValue::Group(_) => unreachable!(),
        }
    }
    pub fn resize(&mut self, orphan: &mut Orphan<'_, '_>, size: u32) -> Result<()> {
        orphan.check(&orphan.data.ty, self.token.0.identity)?;
        let text = match &orphan.data.ty {
            Type::Text => true,
            Type::Data | Type::List(_) => false,
            _ => return Err(mismatch()),
        };
        let count = size
            .checked_add(u32::from(text))
            .ok_or_else(|| invalid("length overflow"))?;
        require(count < 1 << 29, "list exceeds wire limit")?;
        let OwnedValue::Pointer(object) = &mut orphan.data.value else {
            return Err(mismatch());
        };
        object.access(&mut self.anchor, |mut p| {
            if p.is_null() {
                match &orphan.data.ty {
                    Type::Text => {
                        p.reborrow().init_text(0);
                    }
                    Type::Data => {
                        p.reborrow().init_data(0);
                    }
                    Type::List(element) => {
                        init_list(p.reborrow(), *element.clone(), 0)?;
                    }
                    _ => unreachable!(),
                }
            }
            read_pointer(p.as_reader(), orphan.data.ty.clone())?;
            p.resize_owned_list(count, text)
        })
    }
}
impl<'m, 's> Orphan<'m, 's> {
    fn dematerialize(&mut self, anchor: &mut layout::PointerBuilder<'_>) -> Result<()> {
        self.check(&self.data.ty, anchor.identity())?;
        let Type::Struct(schema) = &self.data.ty else {
            return Err(mismatch());
        };
        require(is_group(&self.data.ty)?, "not a group schema")?;
        validate_group(schema, 0)?;
        if let OwnedValue::Pointer(object) = &mut self.data.value {
            let token = Orphanage::new(anchor.identity());
            let fields = object.move_group_fields(anchor, |mut p| {
                let builder = p.reborrow().get_struct(schema.struct_size()?, None)?;
                let fields = Builder {
                    builder,
                    schema: schema.clone(),
                }
                .disown_group(&token)?;
                p.clear();
                Ok(fields)
            })?;
            self.data.value = OwnedValue::Group(fields);
        }
        Ok(())
    }

    /// Require registered native schemas and exact generic arguments. Unlike
    /// ordinary native casts, this ownership conversion does not erase brands.
    /// Materialize a fieldwise group before converting it.
    pub fn release_as<T: crate::traits::Owned>(
        self,
    ) -> core::result::Result<TypedOrphan<'m, 's, T>, AdoptError<Self>> {
        let valid = native_matches(&self.data.ty, T::introspect(), 0);
        match valid {
            Ok(true) if matches!(self.data.value, OwnedValue::Pointer(_)) => Ok(TypedOrphan {
                inner: self,
                marker: PhantomData,
            }),
            result => Err(AdoptError {
                error: result.err().unwrap_or_else(mismatch),
                orphan: self,
            }),
        }
    }
}

/// A checked native view of a loaded orphan. The schema borrow is retained.
pub struct TypedOrphan<'m, 's, T: crate::traits::Owned> {
    inner: Orphan<'m, 's>,
    marker: PhantomData<T>,
}
impl<'m, 's, T: crate::traits::Owned> TypedOrphan<'m, 's, T> {
    pub fn into_dynamic(self) -> Orphan<'m, 's> {
        self.inner
    }
}
impl Access<'_, '_> {
    pub fn read_typed<T: crate::traits::Owned, R>(
        &mut self,
        orphan: &mut TypedOrphan<'_, '_, T>,
        read: impl for<'b> FnOnce(T::Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        use crate::traits::FromPointerReader;
        orphan
            .inner
            .check(&orphan.inner.data.ty, self.token.0.identity)?;
        let OwnedValue::Pointer(object) = &mut orphan.inner.data.value else {
            unreachable!()
        };
        object.access(&mut self.anchor, |p| {
            read(T::Reader::get_from_pointer(&p.into_reader(), None)?)
        })
    }
    pub fn edit_typed<T: crate::traits::Owned, R>(
        &mut self,
        orphan: &mut TypedOrphan<'_, '_, T>,
        edit: impl for<'b> FnOnce(T::Builder<'b>) -> Result<R>,
    ) -> Result<R> {
        use crate::traits::FromPointerBuilder;
        orphan
            .inner
            .check(&orphan.inner.data.ty, self.token.0.identity)?;
        let OwnedValue::Pointer(object) = &mut orphan.inner.data.value else {
            unreachable!()
        };
        object.access(&mut self.anchor, |p| {
            edit(T::Builder::get_from_pointer(p, None)?)
        })
    }
    pub fn resize_typed<T: crate::traits::Owned>(
        &mut self,
        orphan: &mut TypedOrphan<'_, '_, T>,
        count: u32,
    ) -> Result<()> {
        self.resize(&mut orphan.inner, count)
    }
}

fn native_matches(
    loaded: &Type<'_>,
    native: crate::introspect::Type,
    depth: usize,
) -> Result<bool> {
    use crate::introspect::TypeVariant as N;
    require(depth < 64, "native orphan type nesting limit")?;
    let (schema, id, brand) = match (loaded, native.which()) {
        (Type::Void, N::Void)
        | (Type::Bool, N::Bool)
        | (Type::Int8, N::Int8)
        | (Type::Int16, N::Int16)
        | (Type::Int32, N::Int32)
        | (Type::Int64, N::Int64)
        | (Type::UInt8, N::UInt8)
        | (Type::UInt16, N::UInt16)
        | (Type::UInt32, N::UInt32)
        | (Type::UInt64, N::UInt64)
        | (Type::Float32, N::Float32)
        | (Type::Float64, N::Float64)
        | (Type::Text, N::Text)
        | (Type::Data, N::Data)
        | (Type::AnyPointer(PointerKind::Any), N::AnyPointer)
        | (Type::AnyPointer(PointerKind::Capability), N::Capability) => return Ok(true),
        (Type::List(l), N::List(n)) => return native_matches(l, n, depth + 1),
        (Type::Struct(s), N::Struct(_)) => {
            let n = native.as_struct_schema()?;
            (
                s,
                n.get_proto().get_id(),
                n.brand.or_else(|| {
                    (!n.get_proto().get_is_generic()).then_some(crate::introspect::Brand::EMPTY)
                }),
            )
        }
        (Type::Interface(s), N::Interface(n)) => {
            let n = crate::schema::InterfaceSchema::new(n);
            (s, n.get_proto().get_id(), Some(n.get_brand()))
        }
        (Type::Enum(s), N::Enum(n)) => {
            return Ok(s.loader.native_ids.contains(&s.id)
                && s.id == crate::schema::EnumSchema::new(n).get_proto().get_id());
        }
        _ => return Ok(false),
    };
    if schema.id != id || !schema.loader.native_ids.contains(&id) {
        return Ok(false);
    }
    let Some(brand) = brand else {
        return Ok(false);
    };
    if !schema.get_proto().get_is_generic() {
        return Ok(brand.is_empty());
    }
    let mut scopes = Vec::new();
    let mut scope = schema.clone();
    for _ in 0..64 {
        let proto = scope.get_proto();
        scopes.push((scope.id, proto.get_parameters()?.len()));
        let parent = proto.get_scope_id();
        if parent == 0 {
            break;
        }
        scope = schema.loader.get(parent)?;
    }
    require(scopes.len() < 64, "native orphan scope nesting limit")?;
    if scopes.iter().map(|(_, n)| *n).sum::<u32>() != u32::from(brand.len()) {
        return Ok(false);
    }
    let mut index = 0;
    for (id, count) in scopes.into_iter().rev() {
        for parameter in 0..count {
            let parameter =
                u16::try_from(parameter).expect("validated schema parameter count fits u16");
            let ty = schema.arguments_at_scope(id).get(parameter);
            if !native_matches(&ty, brand.get(index), depth + 1)? {
                return Ok(false);
            }
            index += 1;
        }
    }
    Ok(true)
}

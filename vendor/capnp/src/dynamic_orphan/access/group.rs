//! Group views operate on owned fields, without manufacturing an aliasing
//! parent struct. Each pointer field uses the existing scoped orphan access.
use super::*;
use crate::schema_capnp::{field, node};

type Fields<'message> = Vec<(Field, Orphan<'message>)>;

/// A scoped read view of a detached group. Read fields through callbacks;
/// owned clients and values may escape, borrowed payload views cannot.
pub struct GroupReader<'view, 'arena, 'message> {
    inner: GroupEditor<'view, 'arena, 'message>,
}

/// A scoped editor for a detached group's known fields and active union arm.
/// Edits survive returned errors and unwinding. Checked adoption failures
/// return the supplied owner and preserve the group's selection and fields.
pub struct GroupEditor<'view, 'arena, 'message> {
    access: &'view mut Access<'arena, 'message>,
    schema: StructSchema,
    fields: &'view mut Fields<'message>,
}

pub(super) fn is_group(schema: StructSchema) -> Result<bool> {
    let node::Struct(s) = schema.get_proto().which()? else {
        return Err(Error::from_kind(ErrorKind::NotAStruct));
    };
    Ok(s.get_is_group())
}
pub(super) fn validate_schema(schema: StructSchema, depth: usize) -> Result<()> {
    if depth >= 64 {
        return Err(Error::failed("orphan group nesting limit".into()));
    }
    if !is_group(schema)? {
        return Err(Error::from_kind(ErrorKind::TypeMismatch));
    }
    for f in schema.get_fields()? {
        if matches!(f.get_proto().which()?, field::Group(_)) {
            validate_schema(f.get_type().as_struct_schema()?, depth + 1)?;
        }
    }
    Ok(())
}
fn union_field(f: Field) -> bool {
    f.get_proto().get_discriminant_value() != u16::MAX
}
fn default_value(schema: StructSchema, f: Field) -> Result<Reader<'static>> {
    dynamic_struct::Reader::new(crate::private::layout::StructReader::new_default(), schema)
        .read_field(f)
}

impl<'arena, 'message> Access<'arena, 'message> {
    /// Materialize an independent parent-sized allocation for a detached group.
    /// Known fields move without copying pointed-to payloads; parent siblings
    /// start empty. The owner can then use ordinary or typed struct callbacks.
    /// Fieldwise group APIs remain available and may detach these fields again.
    pub fn materialize_group(&mut self, orphan: &mut Orphan<'_>) -> Result<()> {
        orphan.check(orphan.get_type(), self.identity)?;
        let schema = orphan.get_type().as_struct_schema()?;
        validate_schema(schema, 0)?;
        let Value::Group(fields) = &mut orphan.inner.value else {
            return Ok(());
        };
        let slot = self.anchor.allocate_detached();
        let structure = self
            .anchor
            .detached(&slot)
            .init_struct(dynamic_struct::struct_size_from_schema(schema)?);
        dynamic_struct::Builder::new(structure, schema).adopt_group_fields(fields)?;
        let object = self.anchor.detached(&slot).disown()?;
        orphan.inner.value = Value::Pointer(object);
        Ok(())
    }
    /// Create a default group as owned fields. Groups have no independent wire
    /// allocation and may only be adopted into a compatible group field.
    pub fn new_group(&mut self, schema: StructSchema) -> Result<Orphan<'message>> {
        validate_schema(schema, 0)?;
        Ok(self.orphan(schema.as_type(), Value::Group(Vec::new())))
    }
    /// Copy only the group's known fields and active union arm. Parent siblings
    /// are excluded. Regular pointed-to values retain unknown fields as usual.
    pub fn copy_group(&mut self, value: dynamic_struct::Reader<'_>) -> Result<Orphan<'message>> {
        validate_schema(value.get_schema(), 0)?;
        self.copy_group_inner(value)
    }
    fn copy_group_inner(&mut self, value: dynamic_struct::Reader<'_>) -> Result<Orphan<'message>> {
        let schema = value.get_schema();
        let mut selected = Vec::new();
        if let node::Struct(s) = schema.get_proto().which()? {
            if s.get_discriminant_count() != 0 {
                selected.push(
                    value
                        .which()?
                        .ok_or_else(|| Error::failed("unknown union arm".into()))?,
                );
            }
        }
        for f in schema.get_non_union_fields()? {
            if value.has(f)? {
                selected.push(f);
            }
        }
        let mut fields = Vec::new();
        for f in selected {
            let v = value.get(f)?;
            let orphan = if matches!(f.get_proto().which()?, field::Group(_)) {
                self.copy_group_inner(v.downcast())?
            } else {
                self.copy(v)?
            };
            fields.push((f, orphan));
        }
        Ok(self.orphan(schema.as_type(), Value::Group(fields)))
    }
    pub fn read_group<R>(
        &mut self,
        orphan: &mut Orphan<'message>,
        read: impl for<'g> FnOnce(GroupReader<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        self.edit_group(orphan, |inner| read(GroupReader { inner }))
    }
    pub fn edit_group<R>(
        &mut self,
        orphan: &mut Orphan<'message>,
        edit: impl for<'g> FnOnce(GroupEditor<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        orphan.check(orphan.get_type(), self.identity)?;
        let schema = orphan.get_type().as_struct_schema()?;
        orphan.dematerialize_group(&mut self.anchor)?;
        let Value::Group(fields) = &mut orphan.inner.value else {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        };
        edit(GroupEditor {
            access: self,
            schema,
            fields,
        })
    }
}

impl<'message> Orphan<'message> {
    pub(crate) fn dematerialize_group(&mut self, anchor: &mut PointerBuilder<'_>) -> Result<()> {
        self.check(self.get_type(), anchor.identity())?;
        let schema = self.get_type().as_struct_schema()?;
        validate_schema(schema, 0)?;
        let Value::Pointer(object) = &mut self.inner.value else {
            return Ok(());
        };
        // The live anchor exclusively borrows the original arena and capability
        // context. Restore ownership before moving its fields into new owners.
        let token = Orphanage::new(anchor.identity());
        let fields = object.move_group_fields(anchor, |mut pointer| {
            let structure = pointer
                .reborrow()
                .get_struct(dynamic_struct::struct_size_from_schema(schema)?, None)?;
            let fields =
                dynamic_struct::Builder::new(structure, schema).disown_group_fields(&token)?;
            // Release pointer slots from inactive union arms too; these may
            // remain populated after ordinary generated discriminant setters.
            pointer.clear();
            Ok(fields)
        })?;
        self.inner.value = Value::Group(fields);
        Ok(())
    }
}

impl<'arena, 'message> GroupReader<'_, 'arena, 'message> {
    pub fn get_schema(&self) -> StructSchema {
        self.inner.get_schema()
    }
    pub fn which(&self) -> Result<Option<Field>> {
        self.inner.which()
    }
    pub fn has(&mut self, f: Field) -> Result<bool> {
        self.inner.has(f)
    }
    pub fn has_with_mode(&mut self, f: Field, mode: dynamic_struct::HasMode) -> Result<bool> {
        self.inner.has_with_mode(f, mode)
    }
    pub fn has_named(&mut self, name: &str) -> Result<bool> {
        self.has(self.get_schema().get_field_by_name(name)?)
    }
    pub fn has_named_with_mode(
        &mut self,
        name: &str,
        mode: dynamic_struct::HasMode,
    ) -> Result<bool> {
        self.has_with_mode(self.get_schema().get_field_by_name(name)?, mode)
    }
    pub fn read<R>(
        &mut self,
        f: Field,
        read: impl for<'b> FnOnce(Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        self.inner.read(f, read)
    }
    pub fn read_named<R>(
        &mut self,
        name: &str,
        read: impl for<'b> FnOnce(Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        self.read(self.get_schema().get_field_by_name(name)?, read)
    }
    pub fn read_group<R>(
        &mut self,
        f: Field,
        read: impl for<'g> FnOnce(GroupReader<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        self.inner.read_group(f, read)
    }
    pub fn read_group_named<R>(
        &mut self,
        name: &str,
        read: impl for<'g> FnOnce(GroupReader<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        self.read_group(self.get_schema().get_field_by_name(name)?, read)
    }
}
impl<'arena, 'message> GroupEditor<'_, 'arena, 'message> {
    pub fn get_schema(&self) -> StructSchema {
        self.schema
    }
    pub fn which(&self) -> Result<Option<Field>> {
        for (f, _) in self.fields.iter() {
            if union_field(*f) {
                return Ok(Some(*f));
            }
        }
        self.schema.get_field_by_discriminant(0)
    }
    fn check(&self, f: Field, active: bool) -> Result<()> {
        if !self.schema.equals(f.parent)? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        if active
            && union_field(f)
            && !self
                .which()?
                .is_some_and(|a| a.get_index() == f.get_index())
        {
            return Err(Error::failed("cannot access an inactive union arm".into()));
        }
        Ok(())
    }
    fn position(&self, f: Field) -> Option<usize> {
        self.fields
            .iter()
            .position(|(a, _)| a.get_index() == f.get_index())
    }
    /// Match dynamic struct presence: inactive union arms and null pointer
    /// slots are absent; active scalar and group fields are present.
    pub fn has(&mut self, f: Field) -> Result<bool> {
        self.has_with_mode(f, dynamic_struct::HasMode::NonNull)
    }
    pub fn has_with_mode(&mut self, f: Field, mode: dynamic_struct::HasMode) -> Result<bool> {
        self.check(f, false)?;
        if union_field(f)
            && !self
                .which()?
                .is_some_and(|a| a.get_index() == f.get_index())
        {
            return Ok(false);
        }
        if matches!(f.get_proto().which()?, field::Group(_)) {
            return Ok(true);
        }
        if !f.get_type().is_pointer_type() {
            if mode == dynamic_struct::HasMode::NonNull {
                return Ok(true);
            }
            let field::Slot(slot) = f.get_proto().which()? else {
                unreachable!()
            };
            let default = slot.get_default_value()?;
            return self.read(f, |v| Ok(!dynamic_struct::scalar_is_default(v, default)?));
        }
        match self.position(f) {
            Some(i) => Ok(!self.access.is_null(&mut self.fields[i].1)?),
            None => Ok(false),
        }
    }
    pub fn has_named(&mut self, name: &str) -> Result<bool> {
        self.has(self.schema.get_field_by_name(name)?)
    }
    pub fn has_named_with_mode(
        &mut self,
        name: &str,
        mode: dynamic_struct::HasMode,
    ) -> Result<bool> {
        self.has_with_mode(self.schema.get_field_by_name(name)?, mode)
    }
    fn group_schema(&self, f: Field, active: bool) -> Result<StructSchema> {
        self.check(f, active)?;
        if !matches!(f.get_proto().which()?, field::Group(_)) {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        f.get_type().as_struct_schema()
    }
    fn default_owner(&mut self, f: Field) -> Result<Orphan<'message>> {
        if matches!(f.get_proto().which()?, field::Group(_)) {
            self.access.new_group(f.get_type().as_struct_schema()?)
        } else if f.get_type().is_pointer_type() {
            self.access.null(f.get_type())
        } else {
            self.access.copy(default_value(self.schema, f)?)
        }
    }
    pub fn read<R>(
        &mut self,
        f: Field,
        read: impl for<'b> FnOnce(Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        self.check(f, true)?;
        if matches!(f.get_proto().which()?, field::Group(_)) {
            return Err(Error::failed("use read_group() for a nested group".into()));
        }
        if let Some(index) = self.position(f) {
            let value = &mut self.fields[index].1;
            if !self.access.is_null(value)? {
                return self.access.read(value, read);
            }
        }
        read(default_value(self.schema, f)?)
    }
    pub fn read_named<R>(
        &mut self,
        name: &str,
        read: impl for<'b> FnOnce(Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        self.read(self.schema.get_field_by_name(name)?, read)
    }
    pub fn read_group<R>(
        &mut self,
        f: Field,
        read: impl for<'g> FnOnce(GroupReader<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        let schema = self.group_schema(f, true)?;
        if let Some(i) = self.position(f) {
            return self.access.read_group(&mut self.fields[i].1, read);
        }
        // Missing fields denote defaults. The temporary view creates no arena
        // allocation or parent ownership, and cannot escape the callback.
        let mut fields = Vec::new();
        read(GroupReader {
            inner: GroupEditor {
                access: self.access,
                schema,
                fields: &mut fields,
            },
        })
    }
    pub fn read_group_named<R>(
        &mut self,
        name: &str,
        read: impl for<'g> FnOnce(GroupReader<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        self.read_group(self.schema.get_field_by_name(name)?, read)
    }
    pub fn adopt(
        &mut self,
        f: Field,
        orphan: Orphan<'message>,
    ) -> core::result::Result<(), AdoptError<Orphan<'message>>> {
        let result = self
            .check(f, false)
            .and_then(|()| orphan.check(f.get_type(), self.access.identity));
        if let Err(error) = result {
            return Err(AdoptError { error, orphan });
        }
        // Reserve before releasing any existing owners. Only checked same-arena
        // ownership moves follow; no borrowed arena bytes are read or copied.
        self.fields.reserve(1);
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.fields.len() {
            let old = self.fields[i].0;
            if old.get_index() == f.get_index() || (union_field(f) && union_field(old)) {
                removed.push(self.fields.remove(i));
            } else {
                i += 1;
            }
        }
        self.fields.push((f, orphan));
        // Publish the new selection before releasing replaced capabilities.
        drop(removed);
        Ok(())
    }
    pub fn adopt_named(
        &mut self,
        name: &str,
        orphan: Orphan<'message>,
    ) -> core::result::Result<(), AdoptError<Orphan<'message>>> {
        match self.schema.get_field_by_name(name) {
            Ok(f) => self.adopt(f, orphan),
            Err(error) => Err(AdoptError { error, orphan }),
        }
    }
    pub fn set(&mut self, f: Field, value: Reader<'_>) -> Result<()> {
        self.check(f, false)?;
        let orphan = self.access.copy(value)?;
        self.adopt(f, orphan).map_err(|e| e.error)
    }
    pub fn set_named(&mut self, name: &str, value: Reader<'_>) -> Result<()> {
        self.set(self.schema.get_field_by_name(name)?, value)
    }
    pub fn clear(&mut self, f: Field) -> Result<()> {
        self.check(f, false)?;
        if union_field(f) {
            let value = self.default_owner(f)?;
            self.adopt(f, value).map_err(|e| e.error)
        } else {
            if let Some(i) = self.position(f) {
                let old = self.fields.remove(i);
                drop(old);
            }
            Ok(())
        }
    }
    pub fn clear_named(&mut self, name: &str) -> Result<()> {
        self.clear(self.schema.get_field_by_name(name)?)
    }
    pub fn disown(&mut self, f: Field) -> Result<Orphan<'message>> {
        self.check(f, true)?;
        // Allocate/materialize defaults before taking ownership, so a returned
        // error preserves the existing field and union selection.
        let replacement = if union_field(f) {
            Some(self.default_owner(f)?)
        } else {
            None
        };
        let position = self.position(f);
        let use_default = match position {
            Some(i) => self.access.is_null(&mut self.fields[i].1)?,
            None => true,
        };
        let default = if use_default {
            Some(self.access.copy(default_value(self.schema, f)?)?)
        } else {
            None
        };
        let old = position.map(|i| self.fields.remove(i).1);
        if let Some(value) = replacement {
            self.fields.push((f, value));
        }
        Ok(default.or(old).expect("stored or default field"))
    }
    pub fn disown_named(&mut self, name: &str) -> Result<Orphan<'message>> {
        self.disown(self.schema.get_field_by_name(name)?)
    }
    pub fn edit<R>(
        &mut self,
        f: Field,
        edit: impl for<'b> FnOnce(dynamic_value::Builder<'b>) -> Result<R>,
    ) -> Result<R> {
        self.check(f, true)?;
        if matches!(f.get_proto().which()?, field::Group(_)) || !f.get_type().is_pointer_type() {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        let index = self.position(f);
        let materialize = match index {
            Some(i) => self.access.is_null(&mut self.fields[i].1)?,
            None => true,
        };
        if materialize {
            let value = self.access.copy(default_value(self.schema, f)?)?;
            self.adopt(f, value).map_err(|e| e.error)?;
        }
        let i = self.position(f).unwrap();
        self.access.edit(&mut self.fields[i].1, edit)
    }
    pub fn edit_named<R>(
        &mut self,
        name: &str,
        edit: impl for<'b> FnOnce(dynamic_value::Builder<'b>) -> Result<R>,
    ) -> Result<R> {
        self.edit(self.schema.get_field_by_name(name)?, edit)
    }
    pub fn edit_group<R>(
        &mut self,
        f: Field,
        edit: impl for<'g> FnOnce(GroupEditor<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        let schema = self.group_schema(f, true)?;
        if self.position(f).is_none() {
            let value = self.access.new_group(schema)?;
            self.adopt(f, value).map_err(|e| e.error)?;
        }
        let i = self.position(f).unwrap();
        self.access.edit_group(&mut self.fields[i].1, edit)
    }
    pub fn edit_group_named<R>(
        &mut self,
        name: &str,
        edit: impl for<'g> FnOnce(GroupEditor<'g, 'arena, 'message>) -> Result<R>,
    ) -> Result<R> {
        self.edit_group(self.schema.get_field_by_name(name)?, edit)
    }
}

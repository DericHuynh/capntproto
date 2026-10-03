//! Fieldwise group views do not allocate a synthetic parent or copy descendants.
use super::*;

pub struct GroupEditor<'g, 'a, 'm, 's> {
    access: &'g mut Access<'a, 'm>,
    schema: Schema<'s>,
    fields: &'g mut Vec<(Field<'s>, Orphan<'m, 's>)>,
}
pub struct GroupReader<'g, 'a, 'm, 's> {
    inner: GroupEditor<'g, 'a, 'm, 's>,
}
fn union(f: &Field<'_>) -> bool {
    f.get_proto().get_discriminant_value() != field::NO_DISCRIMINANT
}
fn default<'s>(schema: Schema<'s>, field: Field<'s>) -> Result<Value<'s, 's>> {
    Reader {
        reader: layout::StructReader::new_default(),
        schema,
    }
    .read_field(field)
}
impl<'a, 'm> Access<'a, 'm> {
    pub fn read_group<'s, R>(
        &mut self,
        orphan: &mut Orphan<'m, 's>,
        read: impl for<'g> FnOnce(GroupReader<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        self.edit_group(orphan, |inner| read(GroupReader { inner }))
    }
    pub fn edit_group<'s, R>(
        &mut self,
        orphan: &mut Orphan<'m, 's>,
        edit: impl for<'g> FnOnce(GroupEditor<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        orphan.dematerialize(&mut self.anchor)?;
        let Type::Struct(schema) = &orphan.data.ty else {
            return Err(mismatch());
        };
        let OwnedValue::Group(fields) = &mut orphan.data.value else {
            unreachable!()
        };
        edit(GroupEditor {
            access: self,
            schema: schema.clone(),
            fields,
        })
    }
}
impl<'a, 'm, 's> GroupReader<'_, 'a, 'm, 's> {
    pub fn get_schema(&self) -> Schema<'s> {
        self.inner.get_schema()
    }
    pub fn which(&self) -> Result<Option<Field<'s>>> {
        self.inner.which()
    }
    pub fn has(&mut self, f: Field<'s>) -> Result<bool> {
        self.inner.has(f)
    }
    pub fn has_with_mode(&mut self, f: Field<'s>, mode: HasMode) -> Result<bool> {
        self.inner.has_with_mode(f, mode)
    }
    pub fn has_named(&mut self, name: &str) -> Result<bool> {
        self.inner.has_named(name)
    }
    pub fn has_named_with_mode(&mut self, name: &str, mode: HasMode) -> Result<bool> {
        self.inner.has_named_with_mode(name, mode)
    }
    pub fn read<R>(
        &mut self,
        f: Field<'s>,
        read: impl for<'b> FnOnce(Value<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        self.inner.read(f, read)
    }
    pub fn read_named<R>(
        &mut self,
        name: &str,
        read: impl for<'b> FnOnce(Value<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        self.inner.read_named(name, read)
    }
    pub fn read_group<R>(
        &mut self,
        f: Field<'s>,
        read: impl for<'g> FnOnce(GroupReader<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        self.inner.read_group(f, read)
    }
    pub fn read_group_named<R>(
        &mut self,
        name: &str,
        read: impl for<'g> FnOnce(GroupReader<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        self.inner.read_group_named(name, read)
    }
}
impl<'a, 'm, 's> GroupEditor<'_, 'a, 'm, 's> {
    pub fn get_schema(&self) -> Schema<'s> {
        self.schema.clone()
    }
    pub fn which(&self) -> Result<Option<Field<'s>>> {
        if let Some((f, _)) = self.fields.iter().find(|(f, _)| union(f)) {
            return Ok(Some(f.clone()));
        }
        Ok(self
            .schema
            .fields()?
            .into_iter()
            .find(|f| f.get_proto().get_discriminant_value() == 0))
    }
    fn check(&self, f: &Field<'s>, active: bool) -> Result<()> {
        if !self.schema.equals(&f.parent) {
            return Err(mismatch());
        }
        if active && union(f) && !self.which()?.is_some_and(|a| a.index == f.index) {
            return Err(invalid("cannot access inactive union arm"));
        }
        Ok(())
    }
    fn position(&self, f: &Field<'s>) -> Option<usize> {
        self.fields.iter().position(|(a, _)| a.index == f.index)
    }
    pub fn has(&mut self, f: Field<'s>) -> Result<bool> {
        self.has_with_mode(f, HasMode::NonNull)
    }
    pub fn has_with_mode(&mut self, f: Field<'s>, mode: HasMode) -> Result<bool> {
        self.check(&f, false)?;
        if union(&f) && !self.which()?.is_some_and(|a| a.index == f.index) {
            return Ok(false);
        }
        if matches!(f.get_proto().which()?, field::Group(_)) {
            return Ok(true);
        }
        let ty = f.get_type()?;
        if matches!(ty, Type::Unknown(_)) {
            return Ok(false);
        }
        if !ty.is_pointer() {
            if mode == HasMode::NonNull {
                return Ok(true);
            }
            let field::Slot(slot) = f.get_proto().which()? else {
                unreachable!()
            };
            let default = slot.get_default_value()?;
            return self.read(f, |v| {
                use crate::dynamic_value::Reader as V;
                // Detached scalars store decoded values. Reconstruct the bitwise
                // default comparison without materializing a parent message.
                let v = match v {
                    Value::Void => V::Void,
                    Value::Bool(v) => V::Bool(v),
                    Value::Int8(v) => V::Int8(v),
                    Value::Int16(v) => V::Int16(v),
                    Value::Int32(v) => V::Int32(v),
                    Value::Int64(v) => V::Int64(v),
                    Value::UInt8(v) => V::UInt8(v),
                    Value::UInt16(v) => V::UInt16(v),
                    Value::UInt32(v) => V::UInt32(v),
                    Value::UInt64(v) => V::UInt64(v),
                    Value::Float32(v) => V::Float32(v),
                    Value::Float64(v) => V::Float64(v),
                    Value::Enum(v, _) => {
                        return match default.which()? {
                            value::Enum(d) => Ok(v != d),
                            _ => Err(mismatch()),
                        }
                    }
                    _ => return Err(mismatch()),
                };
                Ok(!crate::dynamic_struct::scalar_is_default(v, default)?)
            });
        }
        match self.position(&f) {
            Some(i) => Ok(!self.access.is_null(&mut self.fields[i].1)?),
            None => Ok(false),
        }
    }
    pub fn has_named(&mut self, name: &str) -> Result<bool> {
        self.has(self.schema.field(name)?)
    }
    pub fn has_named_with_mode(&mut self, name: &str, mode: HasMode) -> Result<bool> {
        self.has_with_mode(self.schema.field(name)?, mode)
    }
    fn group_schema(&self, f: &Field<'s>) -> Result<Schema<'s>> {
        self.check(f, true)?;
        require(
            matches!(f.get_proto().which()?, field::Group(_)),
            "not a group field",
        )?;
        let Type::Struct(s) = f.get_type()? else {
            return Err(mismatch());
        };
        Ok(s)
    }
    fn default_owner(&mut self, f: &Field<'s>) -> Result<Orphan<'m, 's>> {
        if matches!(f.get_proto().which()?, field::Group(_)) {
            let Type::Struct(s) = f.get_type()? else {
                return Err(mismatch());
            };
            self.access.new_group(s)
        } else if f.get_type()?.is_pointer() {
            self.access.null(f.get_type()?)
        } else {
            self.access.copy(default(self.schema.clone(), f.clone())?)
        }
    }
    pub fn read<R>(
        &mut self,
        f: Field<'s>,
        read: impl for<'b> FnOnce(Value<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        self.check(&f, true)?;
        require(
            !matches!(f.get_proto().which()?, field::Group(_)),
            "use read_group for nested groups",
        )?;
        if let Some(i) = self.position(&f) {
            if !self.access.is_null(&mut self.fields[i].1)? {
                return self.access.read(&mut self.fields[i].1, read);
            }
        }
        read(default(self.schema.clone(), f)?)
    }
    pub fn read_named<R>(
        &mut self,
        name: &str,
        read: impl for<'b> FnOnce(Value<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        self.read(self.schema.field(name)?, read)
    }
    pub fn read_group<R>(
        &mut self,
        f: Field<'s>,
        read: impl for<'g> FnOnce(GroupReader<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        let schema = self.group_schema(&f)?;
        if let Some(i) = self.position(&f) {
            return self.access.read_group(&mut self.fields[i].1, read);
        }
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
        read: impl for<'g> FnOnce(GroupReader<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        self.read_group(self.schema.field(name)?, read)
    }
    pub fn adopt(
        &mut self,
        f: Field<'s>,
        orphan: Orphan<'m, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m, 's>>> {
        let check = (|| {
            self.check(&f, false)?;
            orphan.check(&f.get_type()?, self.access.token.0.identity)
        })();
        if let Err(error) = check {
            return Err(AdoptError { error, orphan });
        }
        self.fields.reserve(1);
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.fields.len() {
            let old = &self.fields[i].0;
            if old.index == f.index || (union(&f) && union(old)) {
                removed.push(self.fields.remove(i));
            } else {
                i += 1;
            }
        }
        self.fields.push((f, orphan));
        drop(removed);
        Ok(())
    }
    pub fn adopt_named(
        &mut self,
        name: &str,
        orphan: Orphan<'m, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m, 's>>> {
        match self.schema.field(name) {
            Ok(f) => self.adopt(f, orphan),
            Err(error) => Err(AdoptError { error, orphan }),
        }
    }
    pub fn set(&mut self, f: Field<'s>, value: Value<'_, 's>) -> Result<()> {
        self.check(&f, false)?;
        let target = f.get_type()?;
        value.check(&target)?;
        let mut orphan = self.access.copy(value)?;
        // Copy assignment has validated the value against the declared field
        // type, including untyped capabilities and constrained AnyPointers.
        // Give the new owner that type, as reading the assigned field would.
        // Moving an existing opaque orphan still cannot narrow its type.
        orphan.data.ty = target;
        self.adopt(f, orphan).map_err(|e| e.error)
    }
    pub fn set_named(&mut self, name: &str, value: Value<'_, 's>) -> Result<()> {
        self.set(self.schema.field(name)?, value)
    }
    pub fn clear(&mut self, f: Field<'s>) -> Result<()> {
        self.check(&f, false)?;
        if union(&f) {
            let value = self.default_owner(&f)?;
            self.adopt(f, value).map_err(|e| e.error)
        } else {
            if let Some(i) = self.position(&f) {
                drop(self.fields.remove(i));
            }
            Ok(())
        }
    }
    pub fn clear_named(&mut self, name: &str) -> Result<()> {
        self.clear(self.schema.field(name)?)
    }
    pub fn disown(&mut self, f: Field<'s>) -> Result<Orphan<'m, 's>> {
        self.check(&f, true)?;
        let replacement = if union(&f) {
            Some(self.default_owner(&f)?)
        } else {
            None
        };
        let position = self.position(&f);
        let use_default = match position {
            Some(i) => self.access.is_null(&mut self.fields[i].1)?,
            None => true,
        };
        let fallback = if use_default {
            Some(self.access.copy(default(self.schema.clone(), f.clone())?)?)
        } else {
            None
        };
        let old = position.map(|i| self.fields.remove(i).1);
        if let Some(o) = replacement {
            self.fields.push((f, o));
        }
        Ok(fallback.or(old).expect("stored or default field"))
    }
    pub fn disown_named(&mut self, name: &str) -> Result<Orphan<'m, 's>> {
        self.disown(self.schema.field(name)?)
    }
    pub fn edit<R>(
        &mut self,
        f: Field<'s>,
        edit: impl for<'b> FnOnce(Editor<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        self.check(&f, true)?;
        require(
            !matches!(f.get_proto().which()?, field::Group(_)) && f.get_type()?.is_pointer(),
            "not a pointer slot",
        )?;
        let materialize = match self.position(&f) {
            Some(i) => self.access.is_null(&mut self.fields[i].1)?,
            None => true,
        };
        if materialize {
            let value = self.access.copy(default(self.schema.clone(), f.clone())?)?;
            self.adopt(f.clone(), value).map_err(|e| e.error)?;
        }
        let i = self.position(&f).unwrap();
        self.access.edit(&mut self.fields[i].1, edit)
    }
    pub fn edit_named<R>(
        &mut self,
        name: &str,
        edit: impl for<'b> FnOnce(Editor<'b, 'b>) -> Result<R>,
    ) -> Result<R> {
        self.edit(self.schema.field(name)?, edit)
    }
    pub fn edit_group<R>(
        &mut self,
        f: Field<'s>,
        edit: impl for<'g> FnOnce(GroupEditor<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        let schema = self.group_schema(&f)?;
        if self.position(&f).is_none() {
            let value = self.access.new_group(schema)?;
            self.adopt(f.clone(), value).map_err(|e| e.error)?;
        }
        let i = self.position(&f).unwrap();
        self.access.edit_group(&mut self.fields[i].1, edit)
    }
    pub fn edit_group_named<R>(
        &mut self,
        name: &str,
        edit: impl for<'g> FnOnce(GroupEditor<'g, 'a, 'm, 's>) -> Result<R>,
    ) -> Result<R> {
        self.edit_group(self.schema.field(name)?, edit)
    }
}

//! Detached objects are accessed only through a live message borrow. Views use
//! a temporary private capability context when the message carries a capability
//! table, isolating ownership from the surrounding message's orphans. Owners
//! created inside a pointer view and borrowed views cannot outlive that callback.
//! Aggregate group views instead operate directly on message-scoped field owners.
use super::*;
use crate::{
    dynamic_list, dynamic_struct, dynamic_value,
    private::layout::{PointerBuilder, PointerReader},
    schema::StructSchema,
    traits::{FromPointerBuilder, FromPointerReader, Owned},
};

mod group;
pub use group::{GroupEditor, GroupReader};

/// Exclusive access to a message's orphan allocations. Reader/editor callbacks
/// cannot return references into the arena; owned values and clients may escape.
pub struct Access<'arena, 'message> {
    anchor: PointerBuilder<'arena>,
    identity: ArenaIdentity,
    lifetime: PhantomData<&'message mut ()>,
}
impl<'message> Orphanage<'message> {
    pub fn in_root<'a>(&self, root: &'a mut super::Root<'_>) -> Result<Access<'a, 'message>> {
        self.check(root.pointer.builder.identity())?;
        Ok(Access {
            anchor: root.pointer.builder.reborrow(),
            identity: self.identity,
            lifetime: PhantomData,
        })
    }
    pub fn in_struct<'a>(
        &self,
        anchor: &'a mut dynamic_struct::Builder<'_>,
    ) -> Result<Access<'a, 'message>> {
        self.check(anchor.builder.identity())?;
        Ok(Access {
            anchor: anchor.builder.orphan_anchor(),
            identity: self.identity,
            lifetime: PhantomData,
        })
    }
    pub fn in_list<'a>(
        &self,
        anchor: &'a mut dynamic_list::Builder<'_>,
    ) -> Result<Access<'a, 'message>> {
        self.check(anchor.builder.identity())?;
        Ok(Access {
            anchor: anchor.builder.orphan_anchor(),
            identity: self.identity,
            lifetime: PhantomData,
        })
    }
}
impl<'message> Access<'_, 'message> {
    fn orphan(&self, ty: Type, value: Value<'message>) -> Orphan<'message> {
        Orphan::new(ty, &Orphanage::new(self.identity), value)
    }
    fn build(
        &mut self,
        ty: Type,
        make: impl for<'b> FnOnce(PointerBuilder<'b>) -> Result<()>,
    ) -> Result<Orphan<'message>> {
        let mut object = DetachedObject::empty(self.anchor.allocate_detached());
        object.access(&mut self.anchor, make)?;
        Ok(self.orphan(ty, Value::Pointer(object)))
    }
    /// Allocate an independent struct, including parent-sized storage for a
    /// group schema. Only that group's fields may be adopted back into a parent.
    pub fn new_struct(&mut self, schema: StructSchema) -> Result<Orphan<'message>> {
        if group::is_group(schema)? {
            group::validate_schema(schema, 0)?;
        }
        let size = dynamic_struct::struct_size_from_schema(schema)?;
        self.build(schema.as_type(), |p| {
            p.init_struct(size);
            Ok(())
        })
    }
    pub fn null(&mut self, ty: Type) -> Result<Orphan<'message>> {
        if !ty.is_pointer_type() {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        if let TypeVariant::Struct(_) = ty.which() {
            let schema = ty.as_struct_schema()?;
            if group::is_group(schema)? {
                group::validate_schema(schema, 0)?;
            }
        }
        self.build(ty, |_| Ok(()))
    }
    pub fn new_list(&mut self, element: Type, size: u32) -> Result<Orphan<'message>> {
        // Check before allocation, including zero-sized (Void) lists.
        if size >= (1 << 29) {
            return Err(Error::failed(
                "orphan list length exceeds wire limit".into(),
            ));
        }
        let structure = if let TypeVariant::Struct(_) = element.which() {
            let schema = element.as_struct_schema()?;
            reject_group(schema)?;
            Some(dynamic_struct::struct_size_from_schema(schema)?)
        } else {
            None
        };
        if let Some(s) = structure {
            if u64::from(size) * (u64::from(s.data) + u64::from(s.pointers)) >= (1 << 29) {
                return Err(Error::failed(
                    "orphan struct list exceeds wire limit".into(),
                ));
            }
        }
        self.build(Type::list_of(element), |p| {
            if let Some(s) = structure {
                p.init_struct_list(size, s);
            } else {
                p.init_list(element.expected_element_size(), size);
            }
            Ok(())
        })
    }
    pub fn new_text(&mut self, size: u32) -> Result<Orphan<'message>> {
        if size >= (1 << 29) - 1 {
            return Err(Error::failed(
                "orphan text length exceeds wire limit".into(),
            ));
        }
        self.build(TypeVariant::Text.into(), |p| {
            p.init_text(size);
            Ok(())
        })
    }
    pub fn new_data(&mut self, size: u32) -> Result<Orphan<'message>> {
        if size >= (1 << 29) {
            return Err(Error::failed(
                "orphan data length exceeds wire limit".into(),
            ));
        }
        self.build(TypeVariant::Data.into(), |p| {
            p.init_data(size);
            Ok(())
        })
    }
    /// Reference immutable external bytes as a Data orphan, without copying.
    /// The arena retains the buffer even after the orphan is adopted or dropped.
    /// Readers and serialization work normally; mutable views and resize fail.
    pub fn reference_external_data(&mut self, data: ExternalData) -> Result<Orphan<'message>> {
        self.build(TypeVariant::Data.into(), |p| {
            p.reference_external_data(data)
        })
    }
    /// Deep-copy a value from any message. Unknown struct/list fields and
    /// capabilities are copied; subsequent mutation is independent of its source.
    /// Inline groups copy only known fields and the active union arm, excluding
    /// parent siblings; their pointed-to values still retain unknown fields.
    pub fn copy(&mut self, value: Reader<'_>) -> Result<Orphan<'message>> {
        if let Reader::Struct(value) = &value {
            if group::is_group(value.get_schema())? {
                return self.copy_group(*value);
            }
        }
        let ty = value_type(&value);
        if !ty.is_pointer_type() {
            return Ok(self.orphan(ty, Value::Scalar(scalar(value)?)));
        }
        if let TypeVariant::Struct(_) = ty.which() {
            reject_group(ty.as_struct_schema()?)?;
        }
        self.build(ty, |mut p| match value {
            Reader::Text(v) => {
                p.set_text(v);
                Ok(())
            }
            Reader::Data(v) => {
                p.set_data(v);
                Ok(())
            }
            Reader::Struct(v) => p.set_struct(&v.reader, false),
            Reader::List(v) => p.set_list(&v.reader, false),
            Reader::AnyPointer(v) => p.copy_from(v.reader, false),
            Reader::Capability(v) => v.set_pointer(p, ty),
            _ => unreachable!(),
        })
    }

    /// Deep-copy a value, then transform every capability in the unpublished
    /// copy. This includes unknown fields and nested lists/structs, using the
    /// same field selection as [`Self::copy`] for inline groups.
    ///
    /// The source and existing destination values are unchanged. Structural
    /// copying completes before the first callback. Each copied capability
    /// reference is transformed; repeated references need not be deduplicated.
    /// Failure or unwinding drops the unpublished copy and its hooks. Callback
    /// side effects are not rolled back. The destination must have a capability
    /// table when the source contains capabilities, just as for `copy()`.
    pub fn copy_with_capability_transform(
        &mut self,
        value: Reader<'_>,
        mut transform: impl FnMut(crate::capability::Client) -> Result<crate::capability::Client>,
    ) -> Result<Orphan<'message>> {
        fn transform_orphan(
            orphan: &mut Orphan<'_>,
            transform: &mut impl FnMut(crate::capability::Client) -> Result<crate::capability::Client>,
        ) -> Result<()> {
            match &mut orphan.inner.value {
                Value::Pointer(object) => object.transform_capabilities(transform),
                Value::Group(fields) => {
                    for (_, field) in fields {
                        transform_orphan(field, transform)?;
                    }
                    Ok(())
                }
                Value::Scalar(_) => Ok(()),
            }
        }
        let mut orphan = self.copy(value)?;
        transform_orphan(&mut orphan, &mut transform)?;
        Ok(orphan)
    }
    /// Deep-copy and concatenate lists with the same declared element type.
    /// Retains the largest actual data/pointer sections of the inputs, including
    /// unknown fields. Inputs stay unchanged; failed copies release acquired
    /// capabilities. Empty inputs produce an empty list of the explicit type.
    pub fn concat(
        &mut self,
        element: Type,
        lists: &[dynamic_list::Reader<'_>],
    ) -> Result<Orphan<'message>> {
        let size = if let TypeVariant::Struct(_) = element.which() {
            let schema = element.as_struct_schema()?;
            reject_group(schema)?;
            dynamic_struct::struct_size_from_schema(schema)?
        } else {
            crate::private::layout::StructSize {
                data: 0,
                pointers: 0,
            }
        };
        let mut readers = Vec::with_capacity(lists.len());
        for list in lists {
            if !element.equals(list.element_type())? {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            readers.push(list.reader);
        }
        self.build(Type::list_of(element), |p| {
            p.init_concat_list(element.expected_element_size(), size, &readers)
        })
    }
    pub fn is_null(&mut self, orphan: &mut Orphan<'_>) -> Result<bool> {
        orphan.check(orphan.get_type(), self.identity)?;
        match &mut orphan.inner.value {
            Value::Pointer(object) => object.access(&mut self.anchor, |p| Ok(p.is_null())),
            _ => Ok(false),
        }
    }
    pub fn read<R>(
        &mut self,
        orphan: &mut Orphan<'_>,
        read: impl for<'b> FnOnce(Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        let ty = orphan.get_type();
        orphan.check(ty, self.identity)?;
        if matches!(orphan.inner.value, Value::Group(_)) {
            self.materialize_group(orphan)?;
        }
        match &mut orphan.inner.value {
            Value::Scalar(value) => read(value.clone()),
            Value::Pointer(object) => object.access(&mut self.anchor, |p| {
                read(pointer_reader(p.into_reader(), ty)?)
            }),
            Value::Group(_) => Err(Error::failed(
                "use read_group() for a detached group".into(),
            )),
        }
    }
    /// Edit detached contents. Group fields are materialized on demand. Scalars
    /// are passed by value, as in C++ DynamicValue::Builder. Returned errors do
    /// not roll back aggregate edits; ownership survives error and unwinding.
    pub fn edit<R>(
        &mut self,
        orphan: &mut Orphan<'_>,
        edit: impl for<'b> FnOnce(dynamic_value::Builder<'b>) -> Result<R>,
    ) -> Result<R> {
        let ty = orphan.get_type();
        orphan.check(ty, self.identity)?;
        if matches!(orphan.inner.value, Value::Group(_)) {
            self.materialize_group(orphan)?;
        }
        match &mut orphan.inner.value {
            Value::Pointer(object) => {
                object.access(&mut self.anchor, |p| edit(pointer_builder(p, ty)?))
            }
            Value::Scalar(value) => edit(scalar_builder(value.clone())?),
            Value::Group(_) => unreachable!(),
        }
    }
    /// Resize a list/blob, zero-initializing growth and releasing removed
    /// capabilities. Shrinking keeps the payload address and clears the removed
    /// tail and padding. Tail allocations reclaim unused words and grow in
    /// place when capacity permits. Other growth may relocate the payload.
    /// Descendant pointers and unknown inline fields retain their allocations.
    pub fn resize(&mut self, orphan: &mut Orphan<'_>, size: u32) -> Result<()> {
        let ty = orphan.get_type();
        orphan.check(ty, self.identity)?;
        let text = match ty.which() {
            TypeVariant::Text => true,
            TypeVariant::Data | TypeVariant::List(_) => false,
            _ => return Err(Error::from_kind(ErrorKind::TypeMismatch)),
        };
        let count = size
            .checked_add(u32::from(text))
            .ok_or_else(|| Error::failed("orphan length overflow".into()))?;
        if count >= (1 << 29) {
            return Err(Error::failed(
                "orphan list length exceeds wire limit".into(),
            ));
        }
        let Value::Pointer(object) = &mut orphan.inner.value else {
            unreachable!()
        };
        object.access(&mut self.anchor, |mut p| {
            // Validate the declared view, and materialize null lists/blobs with
            // the declared element size before choosing a physical layout.
            if p.is_null() {
                match ty.which() {
                    TypeVariant::Text => {
                        p.reborrow().init_text(0);
                    }
                    TypeVariant::Data => {
                        p.reborrow().init_data(0);
                    }
                    TypeVariant::List(element) => {
                        if let TypeVariant::Struct(_) = element.which() {
                            p.reborrow().init_struct_list(
                                0,
                                dynamic_struct::struct_size_from_schema(
                                    element.as_struct_schema()?,
                                )?,
                            );
                        } else {
                            p.reborrow().init_list(element.expected_element_size(), 0);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            pointer_reader(p.as_reader(), ty)?;
            p.resize_owned_list(count, text)
        })
    }
    pub fn resize_typed<T: Owned>(
        &mut self,
        orphan: &mut TypedOrphan<'_, T>,
        size: u32,
    ) -> Result<()> {
        self.resize(&mut orphan.inner, size)
    }
    pub fn read_typed<T: Owned, R>(
        &mut self,
        orphan: &mut TypedOrphan<'_, T>,
        read: impl for<'b> FnOnce(T::Reader<'b>) -> Result<R>,
    ) -> Result<R> {
        orphan.inner.check(T::introspect(), self.identity)?;
        let Value::Pointer(object) = &mut orphan.inner.inner.value else {
            unreachable!()
        };
        object.access(&mut self.anchor, |p| {
            read(T::Reader::get_from_pointer(&p.into_reader(), None)?)
        })
    }
    pub fn edit_typed<T: Owned, R>(
        &mut self,
        orphan: &mut TypedOrphan<'_, T>,
        edit: impl for<'b> FnOnce(T::Builder<'b>) -> Result<R>,
    ) -> Result<R> {
        orphan.inner.check(T::introspect(), self.identity)?;
        let Value::Pointer(object) = &mut orphan.inner.inner.value else {
            unreachable!()
        };
        object.access(&mut self.anchor, |p| {
            edit(T::Builder::get_from_pointer(p, None)?)
        })
    }
}

/// A checked typed owner. Conversion consumes ownership; failed conversion
/// returns the original dynamic orphan. Convert back before dynamic adoption.
pub struct TypedOrphan<'message, T: Owned> {
    inner: Orphan<'message>,
    marker: PhantomData<T>,
}
impl<'message, T: Owned> TypedOrphan<'message, T> {
    pub fn into_dynamic(self) -> Orphan<'message> {
        self.inner
    }
}
impl<'message> Orphan<'message> {
    pub fn release_as<T: Owned>(
        self,
    ) -> core::result::Result<TypedOrphan<'message, T>, AdoptError<Self>> {
        let result = (|| {
            if !matches!(self.inner.value, Value::Pointer(_))
                || !self.get_type().equals(T::introspect())?
            {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(TypedOrphan {
                inner: self,
                marker: PhantomData,
            }),
            Err(error) => Err(AdoptError {
                error,
                orphan: self,
            }),
        }
    }
}
fn reject_group(schema: StructSchema) -> Result<()> {
    if let crate::schema_capnp::node::Struct(s) = schema.get_proto().which()? {
        if s.get_is_group() {
            return Err(Error::failed("group is not an independent struct".into()));
        }
    }
    Ok(())
}
pub(super) fn pointer_reader(p: PointerReader<'_>, ty: Type) -> Result<Reader<'_>> {
    Ok(match ty.which() {
        TypeVariant::Text => Reader::Text(p.get_text(None)?),
        TypeVariant::Data => Reader::Data(p.get_data(None)?),
        TypeVariant::Struct(_) => {
            dynamic_struct::Reader::new(p.get_struct(None)?, ty.as_struct_schema()?).into()
        }
        TypeVariant::List(element) => {
            dynamic_list::Reader::new(p.get_list(element.expected_element_size(), None)?, element)
                .into()
        }
        TypeVariant::AnyPointer => crate::any_pointer::Reader::new(p).into(),
        TypeVariant::Capability | TypeVariant::Interface(_) => {
            dynamic_value::Capability::from_pointer(p, ty)?.into()
        }
        _ => return Err(Error::from_kind(ErrorKind::TypeMismatch)),
    })
}
pub(super) fn pointer_builder(
    p: PointerBuilder<'_>,
    ty: Type,
) -> Result<dynamic_value::Builder<'_>> {
    Ok(match ty.which() {
        TypeVariant::Text => p.get_text(None)?.into(),
        TypeVariant::Data => p.get_data(None)?.into(),
        TypeVariant::Struct(_) => dynamic_struct::Builder::new(
            p.get_struct(
                dynamic_struct::struct_size_from_schema(ty.as_struct_schema()?)?,
                None,
            )?,
            ty.as_struct_schema()?,
        )
        .into(),
        TypeVariant::List(element) => {
            let list = if let TypeVariant::Struct(_) = element.which() {
                p.get_struct_list(
                    dynamic_struct::struct_size_from_schema(element.as_struct_schema()?)?,
                    None,
                )?
            } else {
                p.get_list(element.expected_element_size(), None)?
            };
            dynamic_list::Builder::new(list, element).into()
        }
        TypeVariant::AnyPointer => crate::any_pointer::Builder::new(p).into(),
        TypeVariant::Capability | TypeVariant::Interface(_) => {
            dynamic_value::Capability::from_pointer(p.into_reader(), ty)?.into()
        }
        _ => return Err(Error::from_kind(ErrorKind::TypeMismatch)),
    })
}
fn value_type(value: &Reader<'_>) -> Type {
    use Reader as R;
    use TypeVariant as T;
    let v = match value {
        R::Void => T::Void,
        R::Bool(_) => T::Bool,
        R::Int8(_) => T::Int8,
        R::Int16(_) => T::Int16,
        R::Int32(_) => T::Int32,
        R::Int64(_) => T::Int64,
        R::UInt8(_) => T::UInt8,
        R::UInt16(_) => T::UInt16,
        R::UInt32(_) => T::UInt32,
        R::UInt64(_) => T::UInt64,
        R::Float32(_) => T::Float32,
        R::Float64(_) => T::Float64,
        R::Text(_) => T::Text,
        R::Data(_) => T::Data,
        R::Enum(e) => T::Enum(e.get_schema().raw),
        R::Struct(s) => return s.get_schema().as_type(),
        R::List(l) => return Type::list_of(l.element_type()),
        R::AnyPointer(_) => T::AnyPointer,
        R::Capability(c) => c
            .get_schema()
            .map_or(T::Capability, |s| T::Interface(s.raw)),
    };
    v.into()
}

// C++ DynamicValue::Builder also exposes scalars by value. Assigning to this
// callback's copy does not mutate the detached owner.
fn scalar_builder(value: Reader<'_>) -> Result<dynamic_value::Builder<'_>> {
    use dynamic_value::Builder as B;
    Ok(match value {
        Reader::Void => B::Void,
        Reader::Bool(v) => B::Bool(v),
        Reader::Int8(v) => B::Int8(v),
        Reader::Int16(v) => B::Int16(v),
        Reader::Int32(v) => B::Int32(v),
        Reader::Int64(v) => B::Int64(v),
        Reader::UInt8(v) => B::UInt8(v),
        Reader::UInt16(v) => B::UInt16(v),
        Reader::UInt32(v) => B::UInt32(v),
        Reader::UInt64(v) => B::UInt64(v),
        Reader::Float32(v) => B::Float32(v),
        Reader::Float64(v) => B::Float64(v),
        Reader::Enum(v) => B::Enum(v),
        _ => return Err(Error::from_kind(ErrorKind::TypeMismatch)),
    })
}

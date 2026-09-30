//! Message-scoped ownership for reflected values. Split a dynamic builder with
//! `with_orphanage()` before disowning fields or list elements. Orphans cannot
//! outlive that message borrow, be cloned, or be adopted into another arena or
//! capability context. `Orphanage::in_struct()` / `in_list()` borrow a live
//! editor for independent allocation, copying, scoped views and resizing.

use crate::{
    dynamic_value::Reader,
    introspect::{Type, TypeVariant},
    private::layout::{ArenaIdentity, DetachedObject},
    schema::Field,
    Error, ErrorKind, Result,
};
use alloc::{boxed::Box, vec::Vec};
use core::marker::PhantomData;

mod access;
mod external;
mod root;
pub use access::{Access, GroupEditor, GroupReader, TypedOrphan};
pub use external::ExternalData;
pub use root::Root;

pub use crate::field_api::AdoptError;

/// A lifetime and identity token. It provides no independent access to memory.
pub struct Orphanage<'message> {
    pub(crate) identity: ArenaIdentity,
    lifetime: PhantomData<&'message mut ()>,
}
impl Orphanage<'_> {
    pub(crate) fn new(identity: ArenaIdentity) -> Self {
        Self {
            identity,
            lifetime: PhantomData,
        }
    }
    pub(crate) fn check(&self, identity: ArenaIdentity) -> Result<()> {
        if self.identity == identity {
            Ok(())
        } else {
            Err(Error::from_kind(ErrorKind::WrongArena))
        }
    }
}

/// Unique ownership of a reflected value. Dropping it releases its capabilities;
/// detached arena bytes remain allocated until the message is destroyed.
pub struct Orphan<'message> {
    pub(crate) inner: Box<Inner<'message>>,
    lifetime: PhantomData<&'message mut ()>,
}
pub(crate) struct Inner<'message> {
    ty: Type,
    identity: ArenaIdentity,
    pub(crate) value: Value<'message>,
}
pub(crate) enum Value<'message> {
    Scalar(Reader<'static>),
    Pointer(DetachedObject),
    Group(Vec<(Field, Orphan<'message>)>),
}
impl<'message> Orphan<'message> {
    pub(crate) fn new(ty: Type, token: &Orphanage<'message>, value: Value<'message>) -> Self {
        Self {
            inner: Box::new(Inner {
                ty,
                identity: token.identity,
                value,
            }),
            lifetime: PhantomData,
        }
    }
    pub fn get_type(&self) -> Type {
        self.inner.ty
    }

    pub(crate) fn check(&self, ty: Type, identity: ArenaIdentity) -> Result<()> {
        if self.inner.identity != identity {
            return Err(Error::from_kind(ErrorKind::WrongArena));
        }
        let compatible = match (ty.which(), self.inner.ty.which()) {
            (TypeVariant::AnyPointer, _) => self.inner.ty.is_pointer_type() && !self.is_group()?,
            (TypeVariant::Capability, TypeVariant::Interface(_) | TypeVariant::Capability) => true,
            (TypeVariant::Interface(target), TypeVariant::Interface(source)) => {
                crate::schema::InterfaceSchema::new(source).extends(target.into())?
            }
            _ => ty.equals(self.inner.ty)?,
        };
        if !compatible {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        match &self.inner.value {
            Value::Pointer(object) => object.check_adopt(identity),
            Value::Group(fields) => {
                for (field, orphan) in fields {
                    orphan.check(field.get_type(), identity)?;
                }
                Ok(())
            }
            Value::Scalar(_) => Ok(()),
        }
    }

    pub(crate) fn is_group(&self) -> Result<bool> {
        if let TypeVariant::Struct(_) = self.get_type().which() {
            if let crate::schema_capnp::node::Struct(s) =
                self.get_type().as_struct_schema()?.get_proto().which()?
            {
                return Ok(s.get_is_group());
            }
        }
        Ok(false)
    }
}

// Explicitly copy only scalars: a borrowed aggregate must never acquire a
// fabricated 'static lifetime just because its representation was reflected.
pub(crate) fn scalar(value: Reader<'_>) -> Result<Reader<'static>> {
    Ok(match value {
        Reader::Void => Reader::Void,
        Reader::Bool(x) => Reader::Bool(x),
        Reader::Int8(x) => Reader::Int8(x),
        Reader::Int16(x) => Reader::Int16(x),
        Reader::Int32(x) => Reader::Int32(x),
        Reader::Int64(x) => Reader::Int64(x),
        Reader::UInt8(x) => Reader::UInt8(x),
        Reader::UInt16(x) => Reader::UInt16(x),
        Reader::UInt32(x) => Reader::UInt32(x),
        Reader::UInt64(x) => Reader::UInt64(x),
        Reader::Float32(x) => Reader::Float32(x),
        Reader::Float64(x) => Reader::Float64(x),
        Reader::Enum(x) => Reader::Enum(x),
        _ => return Err(Error::from_kind(ErrorKind::TypeMismatch)),
    })
}

//! A pointer slot editor which can adopt a whole detached value. Its token and
//! orphans retain the original message borrow; views are always shorter borrows.
use super::*;
use crate::{any_pointer, traits::Owned};

pub struct Root<'message> {
    pub(super) pointer: any_pointer::Builder<'message>,
    ty: Type,
}
impl<'message> Root<'message> {
    /// Attach the expected pointer type without initializing or clearing it.
    pub fn new(pointer: any_pointer::Builder<'message>, ty: Type) -> Result<Self> {
        if !ty.is_pointer_type() {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        if let TypeVariant::Struct(_) = ty.which() {
            if let crate::schema_capnp::node::Struct(s) =
                ty.as_struct_schema()?.get_proto().which()?
            {
                if s.get_is_group() {
                    return Err(Error::from_kind(ErrorKind::TypeMismatch));
                }
            }
        }
        Ok(Self { pointer, ty })
    }
    pub fn with_orphanage(self) -> (Self, Orphanage<'message>) {
        let token = Orphanage::new(self.pointer.builder.identity());
        (self, token)
    }
    pub fn get_type(&self) -> Type {
        self.ty
    }
    pub fn get(&mut self) -> Result<crate::dynamic_value::Builder<'_>> {
        super::access::pointer_builder(self.pointer.builder.reborrow(), self.ty)
    }
    pub fn get_typed<T: Owned>(&mut self) -> Result<T::Builder<'_>> {
        if !self.ty.equals(T::introspect())? {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        self.pointer.reborrow().get_as()
    }
    pub fn as_reader(&self) -> Result<Reader<'_>> {
        super::access::pointer_reader(self.pointer.builder.as_reader(), self.ty)
    }
    pub fn is_null(&self) -> bool {
        self.pointer.is_null()
    }
    pub fn clear(&mut self) {
        self.pointer.clear();
    }
    /// Validate before replacing the root. Failure preserves both the existing
    /// root and the returned owner, including their capability references.
    pub fn adopt<'m>(
        &mut self,
        mut orphan: Orphan<'m>,
    ) -> core::result::Result<(), AdoptError<Orphan<'m>>> {
        let result = (|| {
            orphan.check(self.ty, self.pointer.builder.identity())?;
            let Value::Pointer(object) = &mut orphan.inner.value else {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            };
            self.pointer.builder.adopt(object)
        })();
        result.map_err(|error| AdoptError { error, orphan })
    }
    pub fn disown<'m>(&mut self, token: &Orphanage<'m>) -> Result<Orphan<'m>> {
        token.check(self.pointer.builder.identity())?;
        self.as_reader()?;
        Ok(Orphan::new(
            self.ty,
            token,
            Value::Pointer(self.pointer.builder.disown()?),
        ))
    }
}

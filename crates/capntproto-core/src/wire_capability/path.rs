//! Schema-typed pipeline paths backed by application memory.
use crate::field_api::{Field, GroupDescriptor, GroupSchema, Schema, Struct, WireCapability};
use core::{fmt, marker::PhantomData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity,
    UnionSelection,
    PointerOffset,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pipeline path: {self:?}")
    }
}
impl core::error::Error for Error {}
/// A rejected append returns the original path and leaves its prefix intact.
#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub path: T,
}

/// The result schema remains in `Root` while `Current` follows struct fields.
/// Scalar/list fields cannot be used as protocol pipeline operations.
pub struct Pipeline<'a, Root: Schema, Current: Schema> {
    storage: &'a mut [u16],
    length: usize,
    marker: PhantomData<(Root, Current)>,
}
impl<R: Schema, S: Schema> fmt::Debug for Pipeline<'_, R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Pipeline")
            .field(&&self.storage[..self.length])
            .finish()
    }
}
impl<'a, R: Schema> Pipeline<'a, R, R> {
    pub fn new(storage: &'a mut [u16]) -> Self {
        Self {
            storage,
            length: 0,
            marker: PhantomData,
        }
    }
}
impl<'a, R: Schema, S: Schema> Pipeline<'a, R, S> {
    fn append(&mut self, offset: Result<u16, Error>) -> Result<(), Error> {
        let offset = offset?;
        let field = self.storage.get_mut(self.length).ok_or(Error::Capacity)?;
        *field = offset;
        self.length += 1;
        Ok(())
    }
    pub fn field<C: Schema>(
        mut self,
        field: Field<S, Struct<C>>,
    ) -> Result<Pipeline<'a, R, C>, Rejected<Self>> {
        if let Err(error) = self.append(field.pipeline_offset()) {
            return Err(Rejected { error, path: self });
        }
        Ok(Pipeline {
            storage: self.storage,
            length: self.length,
            marker: PhantomData,
        })
    }
    /// Groups share their enclosing struct's pointer section and add no op.
    /// The wire protocol cannot test a union discriminant, so union groups fail.
    pub fn group<G: GroupSchema>(
        self,
        group: GroupDescriptor<S, G>,
    ) -> Result<Pipeline<'a, R, G>, Rejected<Self>> {
        if group.pipeline_requires_selection() {
            return Err(Rejected {
                error: Error::UnionSelection,
                path: self,
            });
        }
        Ok(Pipeline {
            storage: self.storage,
            length: self.length,
            marker: PhantomData,
        })
    }
    pub fn capability<I>(
        mut self,
        field: Field<S, WireCapability<I>>,
    ) -> Result<CapabilityPath<'a, R, I>, Rejected<Self>> {
        if let Err(error) = self.append(field.pipeline_offset()) {
            return Err(Rejected { error, path: self });
        }
        Ok(CapabilityPath {
            fields: &self.storage[..self.length],
            marker: PhantomData,
        })
    }
}

/// A path ending in a capability of interface `I`, starting at result schema `R`.
pub struct CapabilityPath<'a, R: Schema, I> {
    fields: &'a [u16],
    marker: PhantomData<(R, I)>,
}
impl<R: Schema, I> Copy for CapabilityPath<'_, R, I> {}
impl<R: Schema, I> Clone for CapabilityPath<'_, R, I> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R: Schema, I> fmt::Debug for CapabilityPath<'_, R, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CapabilityPath").field(&self.fields).finish()
    }
}
impl<'a, R: Schema, I> CapabilityPath<'a, R, I> {
    pub const fn fields(&self) -> &'a [u16] {
        self.fields
    }
}

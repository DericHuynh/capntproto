//! Caller-owned descriptor policy. Authentication and metadata storage remain
//! outside the wire engine; published IDs still come from the exact export batch.
use crate::rpc_capnp::api::CapDescriptor;
use capnp::field_api::FixedStruct;
use capntproto_rpc_engine::ids::{ExportId, WireId};

pub struct Descriptors<O> {
    /// Optional identity for a privately recognized owner. None keeps the
    /// generational graph identity, including for opaque policy wrappers.
    pub identity: fn(&O) -> Option<capntproto_rpc_engine::exports::Identity>,
    pub extra_bytes: fn(&O) -> capnp::Result<usize>,
    pub write: fn(&O, ExportId, FixedStruct<'_, CapDescriptor>) -> capnp::Result<()>,
}
impl<O> Copy for Descriptors<O> {}
impl<O> Clone for Descriptors<O> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<O> Default for Descriptors<O> {
    fn default() -> Self {
        Self {
            identity: |_| None,
            extra_bytes: |_| Ok(0),
            write: |_, id, mut field| {
                field.scalar(CapDescriptor::SENDER_HOSTED).set(id.to_wire());
                Ok(())
            },
        }
    }
}
impl<O> Descriptors<O> {
    pub(crate) fn bytes(&self, owner: Option<&O>, promise: bool) -> capnp::Result<usize> {
        if promise {
            Ok(0)
        } else {
            owner.map_or(Ok(0), self.extra_bytes)
        }
    }
    pub(crate) fn describe(
        &self,
        owner: Option<&O>,
        id: ExportId,
        promise: bool,
        mut field: FixedStruct<'_, CapDescriptor>,
    ) -> capnp::Result<()> {
        if promise {
            field
                .scalar(CapDescriptor::SENDER_PROMISE)
                .set(id.to_wire());
            Ok(())
        } else if let Some(owner) = owner {
            (self.write)(owner, id, field)
        } else {
            field.scalar(CapDescriptor::SENDER_HOSTED).set(id.to_wire());
            Ok(())
        }
    }
}

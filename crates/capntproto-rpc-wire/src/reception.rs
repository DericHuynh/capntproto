//! Bind received capability ownership to its actual payload and message origin.
#![forbid(unsafe_code)]
use crate::rpc_capnp;
use capntproto_rpc_engine::{
    ids::{ImportId, WireId},
    imports,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(capnp::ErrorKind),
    Imports(imports::ReceiveError),
    DescriptorCount,
    DescriptorMismatch,
    UnsupportedDescriptor,
    ForeignMessage,
    MissingCapability,
    Exports(capntproto_rpc_engine::exports::LeaseError),
    Answers(capntproto_rpc_engine::answers::PipelineError),
}
impl From<capnp::Error> for Error {
    fn from(e: capnp::Error) -> Self {
        Self::Wire(e.kind)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC input admission: {self:?}")
    }
}
impl core::error::Error for Error {}

#[must_use]
pub struct Prepared<'table, 'receipts, 'id, 'storage, 'message, T> {
    receive: imports::Receive<'table, 'receipts, 'id, 'storage, T>,
    payload: rpc_capnp::payload::Reader<'message>,
}
/// Admit senderHosted/senderPromise/none descriptors into caller-provided slots.
/// A thirdPartyHosted descriptor uses its level-1/2 forwarding vine. All
/// wire indices and descriptor identities are validated before table preflight.
/// A `new` receipt supplies an absent import's owner; repeats use `alias`.
/// Higher-level call/return admission must validate its envelope before accept.
pub fn prepare<'table, 'receipts, 'id, 'storage, 'message, T>(
    payload: rpc_capnp::payload::Reader<'message>,
    imports: &'table mut imports::Imports<'id, 'storage, T>,
    receipts: &'receipts mut [imports::ReceiveSlot<'id, T>],
) -> Result<Prepared<'table, 'receipts, 'id, 'storage, 'message, T>, Error> {
    let descriptors = payload.get_cap_table()?;
    if usize::try_from(descriptors.len()).ok() != Some(receipts.len()) {
        return Err(Error::DescriptorCount);
    }
    payload
        .get_content()
        .validate_capability_indices(descriptors.len())?;
    for (descriptor, receipt) in descriptors.iter().zip(receipts.iter()) {
        check_descriptor(descriptor, receipt)?;
    }
    let receive = imports.prepare_receive(receipts).map_err(Error::Imports)?;
    Ok(Prepared { receive, payload })
}
pub(crate) fn check_descriptor<T>(
    descriptor: rpc_capnp::cap_descriptor::Reader<'_>,
    receipt: &imports::ReceiveSlot<'_, T>,
) -> Result<(), Error> {
    let (expected, promise) = match descriptor
        .which()
        .map_err(|_| Error::UnsupportedDescriptor)?
    {
        rpc_capnp::cap_descriptor::None(()) => (None, false),
        rpc_capnp::cap_descriptor::SenderHosted(id) => (Some(ImportId::from_wire(id)), false),
        rpc_capnp::cap_descriptor::SenderPromise(id) => (Some(ImportId::from_wire(id)), true),
        rpc_capnp::cap_descriptor::ThirdPartyHosted(cap) => {
            (Some(ImportId::from_wire(cap?.get_vine_id())), false)
        }
        _ => return Err(Error::UnsupportedDescriptor),
    };
    if receipt.id() != expected || receipt.is_promise() != promise {
        return Err(Error::DescriptorMismatch);
    }
    Ok(())
}
impl<'a, 'id, 'message, T> Prepared<'_, 'a, 'id, '_, 'message, T> {
    /// Commit all references together. The resulting view enforces message
    /// origin before resolving typed pointers against the accepted descriptors.
    pub fn accept(self) -> Received<'a, 'id, 'message, T> {
        Received {
            receipts: self.receive.commit(),
            payload: self.payload,
        }
    }
}

#[must_use = "release the payload's local references or transfer them before dropping"]
pub struct Received<'a, 'id, 'message, T> {
    receipts: &'a mut [imports::ReceiveSlot<'id, T>],
    payload: rpc_capnp::payload::Reader<'message>,
}
impl<'id, 'message, T> Received<'_, 'id, 'message, T> {
    pub fn content(&self) -> capnp::any_pointer::Reader<'message> {
        self.payload.get_content()
    }
    pub fn capability<I>(
        &self,
        pointer: capnp::wire_capability::Reader<'_, I>,
    ) -> Result<&imports::Local<'id>, Error> {
        if !pointer.belongs_to(self.payload.get_content()) {
            return Err(Error::ForeignMessage);
        }
        let index = usize::try_from(pointer.index()).map_err(|_| Error::MissingCapability)?;
        self.receipts
            .get(index)
            .and_then(|r| r.local())
            .ok_or(Error::MissingCapability)
    }
    /// Resolve a normalized pipeline path using this payload's content and table.
    pub fn pipeline(&self, path: crate::pipeline::Path<'_>) -> Result<&imports::Local<'id>, Error> {
        let index = path.resolve(self.payload.get_content())?;
        let index = usize::try_from(index).map_err(|_| Error::MissingCapability)?;
        self.receipts
            .get(index)
            .and_then(|r| r.local())
            .ok_or(Error::MissingCapability)
    }
    /// Pending wire releases remain in the import ledger until output accepts
    /// them. Explicit retained aliases continue to keep their import live.
    pub fn release(self, imports: &mut imports::Imports<'id, '_, T>) {
        for receipt in self.receipts.iter_mut() {
            if let Some(local) = receipt.take_local() {
                imports.release(local);
            }
        }
    }
}

pub mod full;
#[cfg(test)]
mod tests;

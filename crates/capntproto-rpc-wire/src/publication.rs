//! Bind an export batch to descriptors decoded from one immutable framed message.
#![forbid(unsafe_code)]
use crate::rpc_capnp;
use capntproto_rpc_engine::{
    exports,
    ids::{ExportId, WireId},
    messages,
};
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(capnp::ErrorKind),
    Descriptors(exports::DescriptorError),
    Storage(messages::Error),
    TrailingBytes,
    UnsupportedMessage,
    UnsupportedDescriptor,
    InvalidReturn,
    InvalidResolution,
    ReflectionMismatch,
    Question(capntproto_rpc_engine::questions::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wire(kind) => write!(f, "invalid RPC frame: {kind:?}"),
            Self::Descriptors(e) => e.fmt(f),
            Self::Storage(e) => e.fmt(f),
            Self::TrailingBytes => f.write_str("expected exactly one RPC frame"),
            Self::UnsupportedMessage => {
                f.write_str("publication supports Call, Return results and Resolve")
            }
            Self::UnsupportedDescriptor => {
                f.write_str("publication supports senderHosted, senderPromise, thirdPartyHosted and none descriptors")
            }
            Self::InvalidReturn => f.write_str("capability results require Finish"),
            Self::InvalidResolution => {
                f.write_str("Resolve must match exactly one pending promise")
            }
            Self::ReflectionMismatch => {
                f.write_str("reflected descriptors must match retained local references")
            }
            Self::Question(e) => e.fmt(f),
        }
    }
}
impl core::error::Error for Error {}
impl From<capnp::Error> for Error {
    fn from(e: capnp::Error) -> Self {
        Self::Wire(e.kind)
    }
}
impl From<capnp::NotInSchema> for Error {
    fn from(_: capnp::NotInSchema) -> Self {
        Self::UnsupportedMessage
    }
}
impl From<exports::DescriptorError> for Error {
    fn from(e: exports::DescriptorError) -> Self {
        Self::Descriptors(e)
    }
}

/// Exclusive ownership of a validated frame, its offered send and its grant
/// batch. Drop rejects all three. There is no public send key before accept.
#[must_use = "accept into the connection's output or drop to abort publication"]
pub struct Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, T> {
    batch: Option<exports::Batch<'a, 'exports, 'table, T>>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: Option<messages::Lease<'pool>>,
    offer: Option<messages::Offer<'pool>>,
}

/// Prepare exactly one framed Call, Return(results) or Resolve. Hosted/promise
/// grants and none tombstones are supported. Every grant must match one reserved
/// count and kind; unknown IDs and missing/extra repeats fail closed. Optional
/// attachment indices do not grant authority and may be dropped by the transport.
///
/// Failure consumes/releases the supplied lease and aborts the batch. Existing
/// exports keep their old counts; new owners are drained with take_retired().
/// Scratch needs one u32 per export slot. Validation is O(slots + descriptors)
/// plus bounded wire traversal under the caller's reader limits.
/// This checks the descriptor table and reachable capability-pointer indices,
/// not application schemas, call targets or protocol lifecycle. The driver must use the same
/// connection's export table, message pool and ordered output. Low-level batch
/// and message APIs remain available; do not independently send a frame whose
/// grants are still pending. Forgetting Prepared cannot publish it, but retains
/// message storage until terminal pool teardown.
pub fn prepare<'a, 'exports, 'table, 'pool, 'storage, 'bytes, T>(
    batch: exports::Batch<'a, 'exports, 'table, T>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
) -> Result<Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, T>, Error> {
    prepare_with(batch, messages, lease, scratch, options, |_, _| Ok(false))
}

fn prepare_with<'a, 'exports, 'table, 'pool, 'storage, 'bytes, T>(
    batch: exports::Batch<'a, 'exports, 'table, T>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
    reflected: impl FnMut(rpc_capnp::cap_descriptor::Reader<'_>, usize) -> Result<bool, Error>,
) -> Result<Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, T>, Error> {
    let mut prepared = Prepared {
        batch: Some(batch),
        messages,
        lease: Some(lease),
        offer: None,
    };
    validate(
        prepared.batch.as_ref().unwrap(),
        &prepared.messages.read(prepared.lease.as_ref().unwrap()),
        scratch,
        options,
        reflected,
    )?;
    prepared.offer = Some(
        prepared
            .messages
            .offer(prepared.lease.as_ref().unwrap())
            .map_err(Error::Storage)?,
    );
    Ok(prepared)
}

pub(crate) fn validate<T>(
    batch: &exports::Batch<'_, '_, '_, T>,
    bytes: &[u8],
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
    mut reflected: impl FnMut(rpc_capnp::cap_descriptor::Reader<'_>, usize) -> Result<bool, Error>,
) -> Result<(), Error> {
    let mut input = bytes;
    let frame = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut input, options)?;
    if !input.is_empty() {
        return Err(Error::TrailingBytes);
    }
    let root = frame.get_root::<rpc_capnp::message::Reader<'_>>()?;
    let message = root.which()?;
    if let rpc_capnp::message::Resolve(resolution) = message {
        let resolution = resolution?;
        if batch.resolution_id() != Some(ExportId::from_wire(resolution.get_promise_id())) {
            return Err(Error::InvalidResolution);
        }
        let mut check = batch.descriptors(scratch)?;
        match resolution.which().map_err(|_| Error::UnsupportedMessage)? {
            rpc_capnp::resolve::Cap(descriptor) => {
                let descriptor = descriptor?;
                if matches!(descriptor.which(), Ok(rpc_capnp::cap_descriptor::None(()))) {
                    return Err(Error::InvalidResolution);
                }
                check_descriptor(descriptor, &mut check, false, |d| reflected(d, 0))?;
            }
            rpc_capnp::resolve::Exception(exception) => {
                exception?
                    .get_reason()?
                    .to_str()
                    .map_err(capnp::Error::from)?;
            }
        }
        check.finish()?;
        return Ok(());
    }
    if batch.resolution_id().is_some() {
        return Err(Error::InvalidResolution);
    }
    let (payload, no_finish) = match message {
        rpc_capnp::message::Call(call) => (call?.get_params()?, false),
        rpc_capnp::message::Return(result) => {
            let result = result?;
            let rpc_capnp::return_::Results(payload) = result.which()? else {
                return Err(Error::UnsupportedMessage);
            };
            (payload?, result.get_no_finish_needed())
        }
        _ => return Err(Error::UnsupportedMessage),
    };
    let descriptors = payload.get_cap_table()?;
    payload
        .get_content()
        .validate_capability_indices(descriptors.len())?;
    let mut check = batch.descriptors(scratch)?;
    for (i, descriptor) in descriptors.iter().enumerate() {
        check_descriptor(descriptor, &mut check, no_finish, |d| reflected(d, i))?;
    }
    check.finish()?;
    Ok(())
}
fn check_descriptor<T>(
    descriptor: rpc_capnp::cap_descriptor::Reader<'_>,
    check: &mut exports::DescriptorCheck<'_, T>,
    no_finish: bool,
    reflected: impl FnOnce(rpc_capnp::cap_descriptor::Reader<'_>) -> Result<bool, Error>,
) -> Result<(), Error> {
    let kind = descriptor
        .which()
        .map_err(|_| Error::UnsupportedDescriptor)?;
    if no_finish && !matches!(kind, rpc_capnp::cap_descriptor::None(())) {
        return Err(Error::InvalidReturn);
    }
    if reflected(descriptor)? {
        return Ok(());
    }
    let (id, promise) = match kind {
        rpc_capnp::cap_descriptor::None(()) => return Ok(()),
        rpc_capnp::cap_descriptor::SenderHosted(id) => (id, false),
        rpc_capnp::cap_descriptor::SenderPromise(id) => (id, true),
        rpc_capnp::cap_descriptor::ThirdPartyHosted(third) => {
            let third = third?;
            third.get_id().validate_capability_indices(0)?;
            (third.get_vine_id(), false)
        }
        _ => return Err(Error::UnsupportedDescriptor),
    };
    check.record_kind(ExportId::from_wire(id), promise)?;
    Ok(())
}

pub mod descriptors;
pub mod reflected;

impl<'pool, T> Prepared<'_, '_, '_, 'pool, '_, '_, T> {
    /// Bytes for output admission/inspection; neither mutation nor IO progress
    /// is exposed. Accept only after reserving a place in the connection queue.
    pub fn bytes(&self) -> core::cell::Ref<'_, [u8]> {
        self.messages.read(self.lease.as_ref().unwrap())
    }

    /// Publish grants and make this exact send available in one infallible step,
    /// without application callbacks. The returned key must be enqueued/driven;
    /// if output is subsequently lost, terminate the connection. Completion is
    /// not peer receipt, Finish, or a release of remote capability ownership.
    pub fn accept(mut self) -> messages::SendKey<'pool> {
        self.batch.take().unwrap().commit();
        let key = self.messages.accept(self.offer.take().unwrap());
        self.messages.release(self.lease.take().unwrap());
        key
    }

    /// Atomically publish into a reserved FIFO position. The queue only exposes
    /// its oldest unsent suffix, preserving framed-stream order on partial IO.
    pub fn accept_into(
        mut self,
        slot: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> messages::SendKey<'pool> {
        self.batch.take().unwrap().commit();
        let key = slot.accept(self.messages, self.offer.take().unwrap());
        self.messages.release(self.lease.take().unwrap());
        key
    }
}
impl<T> Drop for Prepared<'_, '_, '_, '_, '_, '_, T> {
    fn drop(&mut self) {
        if let Some(offer) = self.offer.take() {
            self.messages.reject(offer);
        }
        if let Some(lease) = self.lease.take() {
            self.messages.release(lease);
        }
        // Batch drops after this method, aborting unpublished grants.
    }
}

#[cfg(test)]
mod resolution_tests;
#[cfg(test)]
mod tests;

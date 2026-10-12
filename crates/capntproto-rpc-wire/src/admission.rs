//! Bind outgoing question/control transitions to validated immutable frames.
#![forbid(unsafe_code)]
use crate::rpc_capnp;
use capntproto_rpc_engine::{answers, exports, ids::WireId, imports, messages, questions};
mod third_party;
pub use third_party::{third_party_answer, ThirdPartyAnswer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(capnp::ErrorKind),
    Storage(messages::Error),
    Publication(crate::publication::Error),
    TrailingBytes,
    Mismatch,
    Unsupported,
}
impl From<capnp::Error> for Error {
    fn from(e: capnp::Error) -> Self {
        Self::Wire(e.kind)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC output admission: {self:?}")
    }
}
impl core::error::Error for Error {}
impl From<crate::publication::Error> for Error {
    fn from(e: crate::publication::Error) -> Self {
        Self::Publication(e)
    }
}

struct Frame<'a, 'pool, 'storage, 'bytes> {
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: Option<messages::Lease<'pool>>,
    offer: Option<messages::Offer<'pool>>,
}
impl<'a, 'pool, 'storage, 'bytes> Frame<'a, 'pool, 'storage, 'bytes> {
    fn new(
        messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
        lease: messages::Lease<'pool>,
    ) -> Self {
        Self {
            messages,
            lease: Some(lease),
            offer: None,
        }
    }
    fn validate(
        &self,
        options: capnp::message::ReaderOptions,
        check: impl FnOnce(rpc_capnp::message::Reader<'_>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let view = self.messages.read(self.lease.as_ref().unwrap());
        let mut bytes = &*view;
        let message = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, options)?;
        if !bytes.is_empty() {
            return Err(Error::TrailingBytes);
        }
        check(message.get_root()?)
    }
    fn offer(&mut self) -> Result<(), Error> {
        self.offer = Some(
            self.messages
                .offer(self.lease.as_ref().unwrap())
                .map_err(Error::Storage)?,
        );
        Ok(())
    }
    fn accept(mut self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) {
        queue.accept(self.messages, self.offer.take().unwrap());
        self.messages.release(self.lease.take().unwrap());
    }
}
impl Drop for Frame<'_, '_, '_, '_> {
    fn drop(&mut self) {
        if let Some(offer) = self.offer.take() {
            self.messages.reject(offer);
        }
        if let Some(lease) = self.lease.take() {
            self.messages.release(lease);
        }
    }
}

#[must_use]
pub struct Question<'a, 'id, 'questions, 'pool, 'storage, 'bytes> {
    reservation: questions::Reservation<'a, 'id, 'questions>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
pub(crate) fn check_question(
    expected: capntproto_rpc_engine::ids::QuestionId,
    root: rpc_capnp::message::Reader<'_>,
    allow_grants: bool,
    destination: questions::ResultsTo,
) -> Result<(), Error> {
    let redirected = destination != questions::ResultsTo::Caller;
    let id = match root.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::message::Bootstrap(b) if !redirected => b?.get_question_id(),
        rpc_capnp::message::Accept(a) if !redirected && !allow_grants => {
            let a = a?;
            a.get_provision().validate_capability_indices(0)?;
            a.get_embargo()?;
            a.get_question_id()
        }
        rpc_capnp::message::Join(j) if !redirected && !allow_grants => {
            let j = j?;
            check_network_target(j.get_target()?)?;
            j.get_key_part().validate_capability_indices(0)?;
            j.get_question_id()
        }
        rpc_capnp::message::Call(c) => {
            let c = c?;
            let payload = c.get_params()?;
            let descriptors = payload.get_cap_table()?;
            if c.get_only_promise_pipeline()
                || (!allow_grants && !descriptors.is_empty())
                || match c.get_send_results_to().which() {
                    Ok(rpc_capnp::call::send_results_to::Caller(())) => redirected,
                    Ok(rpc_capnp::call::send_results_to::Yourself(())) => {
                        destination != questions::ResultsTo::Yourself
                    }
                    Ok(rpc_capnp::call::send_results_to::ThirdParty(metadata)) => {
                        metadata.validate_capability_indices(0)?;
                        destination != questions::ResultsTo::ThirdParty
                    }
                    _ => true,
                }
            {
                return Err(Error::Unsupported);
            }
            c.get_target()?.which().map_err(|_| Error::Unsupported)?;
            // Granting admission composes this lifecycle check with publication,
            // which validates the entire content against its descriptor table.
            if !allow_grants {
                payload
                    .get_content()
                    .validate_capability_indices(descriptors.len())?;
            }
            c.get_question_id()
        }
        _ => return Err(Error::Unsupported),
    };
    if id != expected.to_wire() {
        return Err(Error::Mismatch);
    }
    Ok(())
}
fn check_network_target(target: rpc_capnp::message_target::Reader<'_>) -> Result<(), Error> {
    if let rpc_capnp::message_target::PromisedAnswer(answer) =
        target.which().map_err(|_| Error::Unsupported)?
    {
        for op in answer?.get_transform()? {
            op.which().map_err(|_| Error::Unsupported)?;
        }
    }
    Ok(())
}

#[must_use]
pub struct Provision<'a, 'id, 'questions, 'pool, 'storage, 'bytes> {
    reservation: questions::ProvisionReservation<'a, 'id, 'questions>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
/// Provide publication uses a distinct reservation and cannot accidentally
/// await a Return or expose a callable pipeline. Failure rolls back both owners.
pub fn provision<'a, 'id, 'questions, 'pool, 'storage, 'bytes>(
    reservation: questions::ProvisionReservation<'a, 'id, 'questions>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<Provision<'a, 'id, 'questions, 'pool, 'storage, 'bytes>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        let rpc_capnp::message::Provide(provide) = root.which().map_err(|_| Error::Unsupported)?
        else {
            return Err(Error::Mismatch);
        };
        let provide = provide?;
        if provide.get_question_id() != reservation.id().to_wire() {
            return Err(Error::Mismatch);
        }
        check_network_target(provide.get_target()?)?;
        provide.get_recipient().validate_capability_indices(0)?;
        Ok(())
    })?;
    frame.offer()?;
    Ok(Provision { reservation, frame })
}
impl<'id, 'pool> Provision<'_, 'id, '_, 'pool, '_, '_> {
    pub fn accept(
        self,
        queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> questions::Provision<'id> {
        let provision = self.reservation.accept();
        self.frame.accept(queue);
        provision
    }
}
/// Admit a Bootstrap or caller-directed Call with no capability grants. Calls
/// with descriptors need combined export publication and are rejected here.
/// Rejection/drop frees the lease, send reservation and unpublished question.
pub fn question<'a, 'id, 'questions, 'pool, 'storage, 'bytes>(
    reservation: questions::Reservation<'a, 'id, 'questions>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<Question<'a, 'id, 'questions, 'pool, 'storage, 'bytes>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        check_question(reservation.id(), root, false, reservation.destination())
    })?;
    frame.offer()?;
    Ok(Question { reservation, frame })
}

#[must_use]
pub struct GrantingQuestion<'a, 'id, 'questions, 'exports, 'table, 'pool, 'storage, 'bytes, T> {
    reservation: questions::Reservation<'a, 'id, 'questions>,
    publication: crate::publication::Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, T>,
}
/// Admit a capability-bearing Call: its question, exact export grants and FIFO
/// frame become visible in one infallible transition. Rejection or drop rolls
/// back every reservation. The driver supplies this connection's target authority.
pub fn granting_question<'a, 'id, 'questions, 'exports, 'table, 'pool, 'storage, 'bytes, T>(
    reservation: questions::Reservation<'a, 'id, 'questions>,
    batch: exports::Batch<'a, 'exports, 'table, T>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
) -> Result<
    GrantingQuestion<'a, 'id, 'questions, 'exports, 'table, 'pool, 'storage, 'bytes, T>,
    Error,
> {
    let publication = crate::publication::prepare(batch, messages, lease, scratch, options)?;
    let view = publication.bytes();
    let mut bytes = &*view;
    let message = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, options)?;
    check_question(
        reservation.id(),
        message.get_root()?,
        true,
        reservation.destination(),
    )?;

    drop(view);
    Ok(GrantingQuestion {
        reservation,
        publication,
    })
}
impl<'id, 'pool, T> GrantingQuestion<'_, 'id, '_, '_, '_, 'pool, '_, '_, T> {
    pub fn accept(
        self,
        queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> questions::Local<'id> {
        let local = self.reservation.accept();
        self.publication.accept_into(queue);
        local
    }
}
impl<'id, 'pool> Question<'_, 'id, '_, 'pool, '_, '_> {
    pub fn accept(
        self,
        queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> questions::Local<'id> {
        let local = self.reservation.accept();
        self.frame.accept(queue);
        local
    }
}

#[must_use]
pub struct Finish<'a, 'id, 'questions, 'pool, 'storage, 'bytes> {
    pending: questions::Finish<'a, 'id, 'questions>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
pub fn finish<'a, 'id, 'questions, 'pool, 'storage, 'bytes>(
    pending: questions::Finish<'a, 'id, 'questions>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<Finish<'a, 'id, 'questions, 'pool, 'storage, 'bytes>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        let rpc_capnp::message::Finish(f) = root.which().map_err(|_| Error::Unsupported)? else {
            return Err(Error::Mismatch);
        };
        let f = f?;
        if f.get_question_id() != pending.id().to_wire()
            || f.get_release_result_caps() != pending.release_result_caps()
        {
            return Err(Error::Mismatch);
        }
        Ok(())
    })?;
    frame.offer()?;
    Ok(Finish { pending, frame })
}
impl<'pool> Finish<'_, '_, '_, 'pool, '_, '_> {
    pub fn accept(self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) {
        self.pending.accept();
        self.frame.accept(queue);
    }
}

pub(crate) fn check_return<T>(
    pending: &mut answers::Return<'_, '_, '_, T>,
    root: rpc_capnp::message::Reader<'_>,
    allow_grants: bool,
) -> Result<(), Error> {
    let rpc_capnp::message::Return(r) = root.which().map_err(|_| Error::Unsupported)? else {
        return Err(Error::Mismatch);
    };
    let r = r?;
    if r.get_answer_id() != pending.id().to_wire() {
        return Err(Error::Mismatch);
    }
    // Parameter references are released explicitly. Omitted Finish is valid
    // only for ordinary successful data results with no descriptor slots.
    let no_finish = r.get_no_finish_needed();
    if r.get_release_param_caps() || (no_finish && !pending.can_omit_finish()) {
        return Err(Error::Unsupported);
    }
    if no_finish {
        let rpc_capnp::return_::Results(payload) = r.which().map_err(|_| Error::Unsupported)?
        else {
            return Err(Error::Unsupported);
        };
        if !payload?.get_cap_table()?.is_empty() {
            return Err(Error::Unsupported);
        }
    }
    match r.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::return_::Canceled(()) if pending.is_cancelled() => (),
        rpc_capnp::return_::ResultsSentElsewhere(()) if pending.is_redirected() => (),
        rpc_capnp::return_::AwaitFromThirdParty(metadata)
            if !pending.is_cancelled() && !pending.is_redirected() && pending.allows_handoff() =>
        {
            metadata.validate_capability_indices(0)?;
        }
        rpc_capnp::return_::Results(p) if !pending.is_cancelled() && !pending.is_redirected() => {
            let p = p?;
            if !allow_grants && !p.get_cap_table()?.is_empty() {
                return Err(Error::Unsupported);
            }
            if !allow_grants {
                p.get_content()
                    .validate_capability_indices(p.get_cap_table()?.len())?;
            }
        }
        rpc_capnp::return_::Exception(e) if !pending.is_cancelled() && !pending.is_redirected() => {
            e?.get_reason()?;
        }
        _ => return Err(Error::Mismatch),
    }
    if no_finish {
        pending.omit_finish().map_err(|_| Error::Unsupported)?;
    }
    Ok(())
}

#[must_use]
pub struct Answer<'a, 'id, 'answers, 'pool, 'storage, 'bytes, T> {
    pending: answers::Return<'a, 'id, 'answers, T>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
pub fn answer<'a, 'id, 'answers, 'pool, 'storage, 'bytes, T>(
    mut pending: answers::Return<'a, 'id, 'answers, T>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<Answer<'a, 'id, 'answers, 'pool, 'storage, 'bytes, T>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| check_return(&mut pending, root, false))?;
    frame.offer()?;
    Ok(Answer { pending, frame })
}
impl<'pool, T> Answer<'_, '_, '_, 'pool, '_, '_, T> {
    pub fn accept(self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) -> Option<T> {
        let retired = self.pending.accept();
        self.frame.accept(queue);
        retired
    }
}

#[must_use]
pub struct GrantingAnswer<'a, 'id, 'answers, 'exports, 'table, 'pool, 'storage, 'bytes, T, E> {
    pending: answers::Return<'a, 'id, 'answers, T>,
    publication: crate::publication::Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, E>,
}
/// Bind the answer ID, Return variant, exact export grants and immutable send
/// before publishing any of them. Failure aborts the batch and keeps Return owed.
pub fn granting_answer<'a, 'id, 'answers, 'exports, 'table, 'pool, 'storage, 'bytes, T, E>(
    mut pending: answers::Return<'a, 'id, 'answers, T>,
    batch: exports::Batch<'a, 'exports, 'table, E>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
) -> Result<GrantingAnswer<'a, 'id, 'answers, 'exports, 'table, 'pool, 'storage, 'bytes, T, E>, Error>
{
    let publication = crate::publication::prepare(batch, messages, lease, scratch, options)?;
    let view = publication.bytes();
    let mut bytes = &*view;
    let message = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, options)?;
    check_return(&mut pending, message.get_root()?, true)?;

    drop(view);
    Ok(GrantingAnswer {
        pending,
        publication,
    })
}
impl<'pool, T, E> GrantingAnswer<'_, '_, '_, '_, '_, 'pool, '_, '_, T, E> {
    pub fn accept(self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) -> Option<T> {
        let retired = self.pending.accept();
        self.publication.accept_into(queue);
        retired
    }
}

#[must_use]
pub struct Release<'a, 'pool, 'storage, 'bytes, T> {
    pending: imports::Release<'a, T>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
pub fn release<'a, 'pool, 'storage, 'bytes, T>(
    pending: imports::Release<'a, T>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<Release<'a, 'pool, 'storage, 'bytes, T>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        let rpc_capnp::message::Release(r) = root.which().map_err(|_| Error::Unsupported)? else {
            return Err(Error::Mismatch);
        };
        let r = r?;
        if r.get_id() != pending.id().to_wire() || r.get_reference_count() != pending.count() {
            return Err(Error::Mismatch);
        }
        Ok(())
    })?;
    frame.offer()?;
    Ok(Release { pending, frame })
}
impl<'pool, T> Release<'_, 'pool, '_, '_, T> {
    pub fn accept(self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) -> T {
        let retired = self.pending.commit();
        self.frame.accept(queue);
        retired
    }
}

#[cfg(test)]
mod tests;

mod embargo;
pub use embargo::{embargo, Embargo};

mod tail;
pub use tail::{tail_answer, TailAnswer};
#[cfg(test)]
mod tail_tests;

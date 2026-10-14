//! Retain the exact accepted request's grants and metadata through Return.
use super::*;
use capntproto_rpc_engine::questions;
mod adoption;
pub use adoption::IncomingAdoption;

#[must_use = "retain through Return, including after local cancellation"]
pub struct Question<'pool, 'q> {
    original: Original<'pool>,
    local: Option<questions::Local<'q>>,
    key: questions::Key<'q>,
}
enum Original<'pool> {
    Frame(Frame<'pool>),
    // A checked template has no parameter grants. Retain the method and handoff
    // policy needed by Return admission, without pinning its serialized bytes.
    DataOnly {
        method: (u64, u16),
        allows_handoff: bool,
    },
}
impl<'pool> Original<'pool> {
    fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) {
        if let Self::Frame(frame) = self {
            frame.release(messages);
        }
    }
}
impl<'pool, 'q> Question<'pool, 'q> {
    /// Internal publication of a template already checked by the invocation
    /// encoder and patched with this reservation's ID. The caller must preserve
    /// its Caller destination, empty descriptor table and reader limits. No
    /// public API accepts a raw frame through this path.
    #[inline(always)]
    pub(super) fn publish_template(
        reservation: questions::Reservation<'_, 'q, '_>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        frame: Frame<'pool>,
        queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
        method: (u64, u16),
        allows_handoff: bool,
    ) -> Result<Self, super::Error> {
        let offer = frame.offer(messages)?;
        let local = reservation.accept();
        queue.accept(messages, offer);
        // The send queue retains its own affine offer through partial writes.
        // Data-only Return admission needs no view of these bytes afterwards.
        Ok(Self {
            key: local.key(),
            local: Some(local),
            original: Original::DataOnly {
                method,
                allows_handoff,
            },
        })
    }
    /// The original frame is retained before output admission, so a Return can
    /// never be paired with a different request having the same recycled ID.
    pub fn send(
        reservation: questions::Reservation<'_, 'q, '_>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        frame: Frame<'pool>,
        queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> Result<Self, crate::admission::Error> {
        let original = match messages.retain(&frame.lease) {
            Ok(lease) => Frame::new(lease, frame.options),
            Err(error) => {
                frame.release(messages);
                return Err(crate::admission::Error::Storage(error));
            }
        };
        let accepted =
            crate::admission::question(reservation, messages, frame.lease, frame.options)
                .map(|prepared| prepared.accept(queue));
        let local = match accepted {
            Ok(local) => local,
            Err(error) => {
                original.release(messages);
                return Err(error);
            }
        };
        Ok(Self {
            original: Original::Frame(original),
            key: local.key(),
            local: Some(local),
        })
    }
    /// Exact outgoing export grants are validated and published in the same
    /// transition as question admission and queue acceptance.
    pub fn send_granting<E>(
        reservation: questions::Reservation<'_, 'q, '_>,
        batch: exports::Batch<'_, '_, '_, E>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        frame: Frame<'pool>,
        scratch: &mut [u32],
        queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> Result<Self, crate::admission::Error> {
        let original = match messages.retain(&frame.lease) {
            Ok(lease) => Frame::new(lease, frame.options),
            Err(error) => {
                frame.release(messages);
                return Err(crate::admission::Error::Storage(error));
            }
        };
        let accepted = crate::admission::granting_question(
            reservation,
            batch,
            messages,
            frame.lease,
            scratch,
            frame.options,
        )
        .map(|prepared| prepared.accept(queue));
        let local = match accepted {
            Ok(local) => local,
            Err(error) => {
                original.release(messages);
                return Err(error);
            }
        };
        Ok(Self {
            original: Original::Frame(original),
            key: local.key(),
            local: Some(local),
        })
    }
    pub fn key(&self) -> questions::Key<'q> {
        self.key
    }
    pub fn local(&self) -> Option<&questions::Local<'q>> {
        self.local.as_ref()
    }
    /// A canceled call still owns its original frame until the late Return is
    /// validated. Finish acceptance remains the connection output's obligation.
    pub fn cancel(&mut self, questions: &mut questions::Questions<'q, '_>) {
        if let Some(local) = self.local.take() {
            questions.release(local);
        }
    }
    pub(crate) fn abandon(
        mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
    ) {
        self.cancel(questions);
        self.original.release(messages);
    }
    // Rejection returns both affine frame owners inline; boxing would require
    // the allocator this API explicitly avoids.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn returned<'i, 'e, 'a, I, E, A, const N: usize>(
        self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        scratch: &mut [u32],
        make_import: impl FnMut(ImportId) -> I,
    ) -> Result<Response<'pool, 'q, 'i, 'e, 'a, I, N>, ReturnRejected<'pool, 'q>> {
        self.returned_with_capacity(
            frame,
            messages,
            questions,
            imports,
            exports,
            answers,
            scratch,
            make_import,
            Capacity::Inline,
        )
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn returned_with_capacity<'i, 'e, 'a, I, E, A, const N: usize>(
        self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        scratch: &mut [u32],
        mut make_import: impl FnMut(ImportId) -> I,
        capacity: Capacity,
    ) -> Result<Response<'pool, 'q, 'i, 'e, 'a, I, N>, ReturnRejected<'pool, 'q>> {
        self.returned_with_receiver(
            frame,
            messages,
            questions,
            imports,
            exports,
            answers,
            scratch,
            &mut make_import,
            capacity,
        )
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn returned_with_receiver<'i, 'e, 'a, I, E, A, const N: usize>(
        self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        capacity: Capacity,
    ) -> Result<Response<'pool, 'q, 'i, 'e, 'a, I, N>, ReturnRejected<'pool, 'q>> {
        let outcome = frame
            .with(messages, |root| {
                let rpc_capnp::message::Return(result) =
                    root.which().map_err(|_| Error::WrongMessage)?
                else {
                    return Err(Error::WrongMessage);
                };
                self.admit_return(
                    result?, &frame, messages, questions, imports, exports, answers, scratch,
                    receiver, capacity,
                )
            })
            .and_then(core::convert::identity);
        match outcome {
            Ok(admitted) => Ok(self.finish_return(frame, messages, admitted)),
            Err(error) => Err(ReturnRejected {
                error,
                frame,
                question: self,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn admit_return<'i, 'e, 'a, I, E, A, const N: usize>(
        &self,
        result: rpc_capnp::return_::Reader<'_>,
        _frame: &Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        capacity: Capacity,
    ) -> Result<AdmittedReturn<'i, 'e, 'a, I, N>, Error> {
        if let Some(admitted) =
            self.try_admit_plain_return(result, _frame, messages, questions, false)?
        {
            return Ok(admitted.into_general());
        }
        let receipts = Receipts::new();
        if !questions.contains(self.key) {
            return Err(Error::WrongMessage);
        }
        self.admit_complex_return(
            result, messages, questions, imports, exports, answers, scratch, receiver, receipts,
            capacity,
        )
    }

    /// The empty-descriptor transaction is shared by the ordinary receive path
    /// and its compact reply entry point. None has no ledger or owner effects.
    #[inline(always)]
    pub(super) fn prepare_plain_return<'m, 'a, 'storage>(
        &self,
        result: rpc_capnp::return_::Reader<'m>,
        questions: &'a mut questions::Questions<'q, 'storage>,
        owned_only: bool,
    ) -> Result<
        Option<(
            questions::Receive<'a, 'q, 'storage>,
            capnp::any_pointer::Reader<'m>,
        )>,
        Error,
    > {
        if result.get_answer_id() != self.key.id().to_wire() {
            return Err(Error::WrongMessage);
        }
        if !matches!(self.original, Original::DataOnly { .. }) {
            return Ok(None);
        }
        let pending = questions
            .prepare_receive_owned(self.key, result.get_no_finish_needed())
            .map_err(|error| {
                if error == questions::Error::StaleKey {
                    Error::WrongMessage
                } else {
                    Error::Admission(inbound::Error::Question(error))
                }
            })?;
        if owned_only && pending.delivery() == capntproto_rpc_engine::question::Delivery::Discard {
            return Ok(None);
        }
        use capnp::traits::IntoInternalStructReader;
        let fields = rpc_capnp::api::ReturnRef::from(result.into_internal_struct_reader());
        if fields.tag() != rpc_capnp::api::ReturnUnionTag::Results {
            return Ok(None);
        }
        let payload =
            rpc_capnp::payload::Reader::from(fields.results()?.into_internal_struct_reader());
        if payload.has_cap_table() && !payload.get_cap_table()?.is_empty() {
            return Ok(None);
        }
        let content = payload.get_content();
        content.validate_capability_indices(0).map_err(|error| {
            Error::Admission(inbound::Error::Reception(crate::reception::Error::from(
                error,
            )))
        })?;
        Ok(Some((pending, content)))
    }
    #[inline(always)]
    pub(super) fn try_admit_plain_return(
        &self,
        result: rpc_capnp::return_::Reader<'_>,
        _frame: &Frame<'pool>,
        _messages: &messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        owned_only: bool,
    ) -> Result<Option<AdmittedPlain>, Error> {
        let Some((pending, _content)) = self.prepare_plain_return(result, questions, owned_only)?
        else {
            return Ok(None);
        };
        #[cfg(feature = "alloc")]
        let plain = _frame
            .share_segments(_messages)?
            .map(|segments| {
                capnp::message::CheckedPointerReader::retain(
                    _content,
                    segments,
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: None,
                        nesting_limit: i32::MAX,
                    },
                )
                .map(|message| {
                    alloc::boxed::Box::new(PlainResults {
                        message,
                        owner: None,
                    })
                })
            })
            .transpose()?;
        #[cfg(feature = "alloc")]
        if owned_only && plain.is_none() {
            return Ok(None);
        }
        #[cfg(not(feature = "alloc"))]
        if owned_only {
            return Ok(None);
        }
        let (_, delivery) = pending.accept();
        Ok(Some(AdmittedPlain {
            discarded: delivery == capntproto_rpc_engine::question::Delivery::Discard,
            #[cfg(feature = "alloc")]
            plain,
        }))
    }

    // Keep descriptor transactions and handoff decoding out of ordinary
    // capability-free reply admission. Both paths commit the same ledgers.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn admit_complex_return<'i, 'e, 'a, I, E, A, const N: usize>(
        &self,
        result: rpc_capnp::return_::Reader<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        mut receipts: Receipts<'i, 'e, 'a, I, N>,
        capacity: Capacity,
    ) -> Result<AdmittedReturn<'i, 'e, 'a, I, N>, Error> {
        let mut descriptors = None;
        if let rpc_capnp::return_::Results(payload) =
            result.which().map_err(|_| Error::WrongMessage)?
        {
            if questions.is_live(self.key) {
                let payload = payload?;
                descriptors = Some(payload.get_cap_table()?);
                receipts.initialize(payload, imports, 0, receiver, capacity)?;
            }
        }
        let mut admit = |original: inbound::full::Original<'_>| -> Result<_, Error> {
            let method = original.method;
            let pending = inbound::full::returned_prepared(
                result,
                original,
                questions,
                imports,
                exports,
                answers,
                receipts.slots(),
                scratch,
            )
            .map_err(Error::Admission)?;
            let (_, reply) = pending.accept();
            Ok(match reply {
                inbound::full::Reply::Results(_) => Kind::Results,
                inbound::full::Reply::Exception(_) => Kind::Exception,
                inbound::full::Reply::AwaitFromThirdParty(_) => {
                    Kind::Await(method.expect("Await requires original Call"))
                }
                inbound::full::Reply::Adopted(adopted) => {
                    Kind::Adopted(adopted, method.unwrap_or((0, 0)))
                }
                inbound::full::Reply::ResultsSentElsewhere => Kind::Elsewhere,
                inbound::full::Reply::Discarded => Kind::Discarded,
            })
        };
        let kind = match &self.original {
            Original::DataOnly {
                method,
                allows_handoff,
            } => admit(inbound::full::Original::data_only(*method, *allows_handoff))?,
            Original::Frame(frame) => frame.with(messages, |original| {
                let original = inbound::full::Original::read(result.get_answer_id(), original)
                    .map_err(Error::Admission)?;
                admit(original)
            })??,
        };
        if let Some(descriptors) = descriptors {
            receipts.deliver(imports, receiver, descriptors);
        }
        Ok(AdmittedReturn {
            receipts,
            kind,
            #[cfg(feature = "alloc")]
            plain: None,
        })
    }
    #[cfg(feature = "alloc")]
    pub(super) fn finish_plain(
        self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        pending: questions::Receive<'_, 'q, '_>,
        admitted: AdmittedPlain,
    ) -> alloc::boxed::Box<PlainResults> {
        // Plain admission validated the entire payload and excluded canceled
        // delivery before retaining its immutable allocating segment owner.
        debug_assert!(!admitted.discarded);
        let plain = admitted.plain.expect("owned plain result");
        if let Some(local) = self.local {
            pending
                .accept_releasing(local)
                .expect("matching plain reply owner");
        } else {
            pending.accept();
        }
        self.original.release(messages);
        frame.release(messages);
        plain
    }
    #[inline(always)]
    pub(super) fn finish_return<'i, 'e, 'a, I, const N: usize>(
        self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        admitted: AdmittedReturn<'i, 'e, 'a, I, N>,
    ) -> Response<'pool, 'q, 'i, 'e, 'a, I, N> {
        self.original.release(messages);
        Response {
            frame,
            local: self.local,
            key: self.key,
            receipts: admitted.receipts,
            kind: admitted.kind,
            #[cfg(feature = "alloc")]
            plain: admitted.plain,
        }
    }
}
// Private proof that admission has committed for this question and frame. It
// never escapes the exclusive receive operation that owns both affine values.
pub(super) struct AdmittedPlain {
    discarded: bool,
    #[cfg(feature = "alloc")]
    plain: Option<alloc::boxed::Box<PlainResults>>,
}
impl AdmittedPlain {
    #[cfg(feature = "alloc")]
    pub(super) fn owned(
        message: capnp::message::CheckedPointerReader<
            capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
        >,
    ) -> Self {
        Self {
            discarded: false,
            plain: Some(alloc::boxed::Box::new(PlainResults {
                message,
                owner: None,
            })),
        }
    }

    pub(super) fn into_general<'i, 'e, 'a, I, const N: usize>(
        self,
    ) -> AdmittedReturn<'i, 'e, 'a, I, N> {
        AdmittedReturn {
            receipts: Receipts::new(),
            kind: if self.discarded {
                Kind::Discarded
            } else {
                Kind::Results
            },
            #[cfg(feature = "alloc")]
            plain: self.plain,
        }
    }
}
pub(super) struct AdmittedReturn<'i, 'e, 'a, I, const N: usize> {
    receipts: Receipts<'i, 'e, 'a, I, N>,
    kind: Kind<'a>,
    #[cfg(feature = "alloc")]
    plain: Option<alloc::boxed::Box<PlainResults>>,
}

/// An immutable capability-free payload retained during successful admission.
/// It is independent of the connection scope; its segment owner prevents pool
/// reuse until the final response reader has gone away.
#[cfg(feature = "alloc")]
pub struct PlainResults {
    message: capnp::message::CheckedPointerReader<
        capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
    >,
    owner: Option<alloc::rc::Rc<dyn core::any::Any>>,
}
#[cfg(feature = "alloc")]
impl PlainResults {
    /// Keep a caller's admission/transport owner alive through the final result
    /// reader, without allocating a second response wrapper. This does not grant
    /// protocol authority or change the immutable message. Attach at most once.
    pub fn retain_owner(&mut self, owner: alloc::rc::Rc<dyn core::any::Any>) {
        assert!(self.owner.is_none(), "result owner already attached");
        self.owner = Some(owner);
    }
}
#[cfg(feature = "alloc")]
impl capnp::private::capability::ResponseHook for PlainResults {
    fn get(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        Ok(self.message.root())
    }
}
#[must_use = "retry Return or terminate this connection"]
pub struct ReturnRejected<'pool, 'q> {
    pub error: Error,
    pub frame: Frame<'pool>,
    pub question: Question<'pool, 'q>,
}
enum Kind<'a> {
    Results,
    Await((u64, u16)),
    Exception,
    Adopted(answers::Adopted<'a>, (u64, u16)),
    Elsewhere,
    Discarded,
}
#[must_use = "release the result and its question through the originating ledgers"]
pub struct Response<'pool, 'q, 'i, 'e, 'a, I, const N: usize> {
    frame: Frame<'pool>,
    local: Option<questions::Local<'q>>,
    key: questions::Key<'q>,
    receipts: Receipts<'i, 'e, 'a, I, N>,
    kind: Kind<'a>,
    #[cfg(feature = "alloc")]
    plain: Option<alloc::boxed::Box<PlainResults>>,
}
pub enum Reply<'m, 'r, 'i, 'e, 'a, I, const N: usize> {
    Results(Payload<'m, 'r, 'i, 'e, 'a, I, N>),
    Exception(rpc_capnp::exception::Reader<'m>),
    Adopted(&'r answers::Adopted<'a>),
    AwaitFromThirdParty(capnp::any_pointer::Reader<'m>),
    ResultsSentElsewhere,
    Discarded,
}
pub struct Payload<'m, 'r, 'i, 'e, 'a, I, const N: usize> {
    payload: rpc_capnp::payload::Reader<'m>,
    receipts: &'r Receipts<'i, 'e, 'a, I, N>,
}
impl<'m, 'r, 'i, 'e, 'a, I, const N: usize> Payload<'m, 'r, 'i, 'e, 'a, I, N> {
    pub fn capability_count(&self) -> usize {
        self.receipts.count
    }
    /// Enumerate the exact admitted table, retaining null slots. Authority is
    /// borrowed from this payload's receipts, never reconstructed from an ID.
    pub fn capability_at(
        &self,
        index: u32,
    ) -> Result<Option<reception::full::Capability<'r, 'm, 'i, 'e, 'a>>, reception::Error> {
        if index as usize >= self.receipts.count {
            return Err(reception::Error::MissingCapability);
        }
        if matches!(
            self.payload.get_cap_table()?.get(index).which(),
            Ok(rpc_capnp::cap_descriptor::None(()))
        ) {
            Ok(None)
        } else {
            self.at(index).map(Some)
        }
    }
    pub fn content(&self) -> capnp::any_pointer::Reader<'m> {
        self.payload.get_content()
    }
    pub fn capability<T>(
        &self,
        pointer: capnp::wire_capability::Reader<'_, T>,
    ) -> Result<reception::full::Capability<'r, 'm, 'i, 'e, 'a>, reception::Error> {
        if !pointer.belongs_to(self.content()) {
            return Err(reception::Error::ForeignMessage);
        }
        self.at(pointer.index())
    }
    pub fn pipeline(
        &self,
        path: pipeline::Path<'_>,
    ) -> Result<reception::full::Capability<'r, 'm, 'i, 'e, 'a>, reception::Error> {
        self.at(path.resolve(self.content())?)
    }
    fn at(
        &self,
        index: u32,
    ) -> Result<reception::full::Capability<'r, 'm, 'i, 'e, 'a>, reception::Error> {
        let i = usize::try_from(index).map_err(|_| reception::Error::MissingCapability)?;
        if i >= self.receipts.count {
            return Err(reception::Error::MissingCapability);
        }
        reception::full::descriptor_capability(
            self.payload.get_cap_table()?.get(index),
            &self.receipts.storage.imports()[i],
            &self.receipts.storage.exports()[i],
            &self.receipts.storage.answers()[i],
        )
    }
}
impl<'pool, 'q, 'i, 'e, 'a, I, const N: usize> Response<'pool, 'q, 'i, 'e, 'a, I, N> {
    /// Transfer the cached, admitted data-only view without reparsing its
    /// envelope. Fixed-buffer messages and other reply kinds return None.
    #[cfg(feature = "alloc")]
    pub fn take_plain_results(&mut self) -> Option<alloc::boxed::Box<PlainResults>> {
        if matches!(self.kind, Kind::Results) {
            self.plain.take()
        } else {
            None
        }
    }
    pub(crate) fn no_finish_needed(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<bool, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Return(result) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            Ok(result?.get_no_finish_needed())
        })?
    }
    pub(crate) fn requires_ordered_propagation(&self) -> bool {
        // Local adoption may wait on application work and unrelated incoming
        // replies. It adds a dependency rather than redirecting a wire import.
        !matches!(
            self.kind,
            Kind::Adopted(..) | Kind::Await(_) | Kind::Discarded
        )
    }
    /// Share immutable storage only for an ordinary results frame. Adoption
    /// and third-party control replies have different ownership semantics.
    #[cfg(feature = "alloc")]
    pub fn share_results(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Option<messages::SharedBytes> {
        if matches!(self.kind, Kind::Results) {
            self.frame.share(messages)
        } else {
            None
        }
    }
    /// A successfully admitted result without descriptor owners can be handed
    /// directly to an immutable hosted reader. Admission validated the entire
    /// payload under this frame's original traversal and nesting limits.
    #[cfg(feature = "alloc")]
    pub fn share_plain_results(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Option<messages::SharedBytes> {
        if self.receipts.count == 0 {
            self.share_results(messages)
        } else {
            None
        }
    }
    pub fn key(&self) -> questions::Key<'q> {
        self.key
    }
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(Reply<'_, '_, 'i, 'e, 'a, I, N>) -> R,
    ) -> Result<R, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Return(result) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            let result = result?;
            let reply = match &self.kind {
                Kind::Results => {
                    let rpc_capnp::return_::Results(payload) =
                        result.which().map_err(|_| Error::WrongMessage)?
                    else {
                        return Err(Error::WrongMessage);
                    };
                    Reply::Results(Payload {
                        payload: payload?,
                        receipts: &self.receipts,
                    })
                }
                Kind::Exception => {
                    let rpc_capnp::return_::Exception(exception) =
                        result.which().map_err(|_| Error::WrongMessage)?
                    else {
                        return Err(Error::WrongMessage);
                    };
                    Reply::Exception(exception?)
                }
                Kind::Await(_) => {
                    let rpc_capnp::return_::AwaitFromThirdParty(metadata) =
                        result.which().map_err(|_| Error::WrongMessage)?
                    else {
                        return Err(Error::WrongMessage);
                    };
                    Reply::AwaitFromThirdParty(metadata)
                }
                Kind::Adopted(adopted, _) => Reply::Adopted(adopted),
                Kind::Elsewhere => Reply::ResultsSentElsewhere,
                Kind::Discarded => Reply::Discarded,
            };
            Ok(f(reply))
        })?
    }
    pub(crate) fn await_method(&self) -> Option<(u64, u16)> {
        if let Kind::Await(method) = self.kind {
            Some(method)
        } else {
            None
        }
    }
    pub(crate) fn adopted(&self) -> Option<(&answers::Adopted<'a>, (u64, u16))> {
        match &self.kind {
            Kind::Adopted(adopted, method) => Some((adopted, *method)),
            _ => None,
        }
    }
    pub(crate) fn local_ref(&self) -> Option<&questions::Local<'q>> {
        self.local.as_ref()
    }
    #[inline(always)]
    pub fn release<E, A>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'q, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        self.receipts.release(imports, exports, answers);
        if let Kind::Adopted(adopted, _) = self.kind {
            answers.release_adopted_deferred(adopted);
        }
        if let Some(local) = self.local {
            questions.release(local);
        }
        self.frame.release(messages);
    }
}

#[cfg(test)]
mod tests;

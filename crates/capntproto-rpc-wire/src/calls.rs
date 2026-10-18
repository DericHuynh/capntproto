//! Typed, allocation-free Call and Return construction and dispatch.
//!
//! These codecs do not allocate question IDs or publish descriptor ownership.
//! The connection ledger must admit a message before it reaches the application
//! and commit its transition together with acceptance into ordered output.
#![forbid(unsafe_code)]

use crate::rpc_capnp::{
    self,
    api::{Bootstrap, Call, Finish, Message, MessageTarget, Payload, Release, Return},
};
use capnp::{
    field_api::{FixedStruct, Schema, Struct},
    message::FixedBuilder,
    wire_capability::{Interface, Method, StreamingMethod},
};
use capntproto_rpc_engine::ids::{AnswerId, ImportId, QuestionId, WireId};

/// A borrowed incoming Call, validated against a generated method before its
/// parameters are exposed. The original message remains borrowed throughout.
pub struct Incoming<'a> {
    call: rpc_capnp::call::Reader<'a>,
}
impl<'a> Incoming<'a> {
    pub fn new(call: rpc_capnp::call::Reader<'a>) -> Self {
        Self { call }
    }
    pub fn question_id(&self) -> AnswerId {
        AnswerId::from_wire(self.call.get_question_id())
    }
    pub fn interface_id(&self) -> u64 {
        self.call.get_interface_id()
    }
    pub fn method_id(&self) -> u16 {
        self.call.get_method_id()
    }
    /// Permission applies only to ordinary replies to this authenticated caller.
    pub fn allows_third_party_tail_call(&self) -> bool {
        self.call.get_allow_third_party_tail_call()
            && matches!(
                self.call.get_send_results_to().which(),
                Ok(rpc_capnp::call::send_results_to::Caller(()))
            )
    }
    /// Server scheduling hints for this admitted Call. Redirected results must
    /// still complete for their recipient, even if the sender sets the
    /// pipeline-only hint. This matches the hosted YourSelf/tail-call path.
    pub fn hints(&self) -> DynamicCall {
        DynamicCall {
            interface: self.interface_id(),
            method: self.method_id(),
            no_promise_pipelining: self.call.get_no_promise_pipelining(),
            only_promise_pipeline: self.call.get_only_promise_pipeline()
                && matches!(
                    self.call.get_send_results_to().which(),
                    Ok(rpc_capnp::call::send_results_to::Caller(()))
                ),
        }
    }
    pub fn target(&self) -> capnp::Result<rpc_capnp::message_target::Reader<'a>> {
        self.call.get_target()
    }
    pub fn payload(&self) -> capnp::Result<rpc_capnp::payload::Reader<'a>> {
        self.call.get_params()
    }

    /// Look up generated hints before dispatch, so cancellation and streaming
    /// accounting do not depend on polling the application future first.
    pub fn service_info(
        &self,
        service: &impl capnp::wire_capability::Service,
    ) -> capnp::Result<capnp::wire_capability::MethodInfo> {
        self.results_to()?;
        service
            .method_info(self.interface_id(), self.method_id())
            .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::Unimplemented))
    }
    pub fn context_service_info<C>(
        &self,
        service: &impl capnp::wire_capability::ContextService<C>,
    ) -> capnp::Result<capnp::wire_capability::MethodInfo> {
        self.results_to()?;
        service
            .method_info(self.interface_id(), self.method_id())
            .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::Unimplemented))
    }

    /// Borrow immutable destination metadata for the caller-owned network.
    pub fn third_party_destination(&self) -> capnp::Result<Option<capnp::any_pointer::Reader<'a>>> {
        match self.call.get_send_results_to().which()? {
            rpc_capnp::call::send_results_to::ThirdParty(metadata) => {
                metadata.validate_capability_indices(0)?;
                Ok(Some(metadata))
            }
            _ => Ok(None),
        }
    }
    fn results_to(&self) -> capnp::Result<ResultsTo> {
        match self.call.get_send_results_to().which() {
            Ok(rpc_capnp::call::send_results_to::Caller(())) => Ok(ResultsTo::Caller),
            Ok(rpc_capnp::call::send_results_to::Yourself(())) => Ok(ResultsTo::Yourself),
            Ok(rpc_capnp::call::send_results_to::ThirdParty(metadata)) => {
                metadata.validate_capability_indices(0)?;
                Ok(ResultsTo::ThirdParty)
            }
            _ => Err(capnp::Error::from_kind(capnp::ErrorKind::Unimplemented)),
        }
    }

    /// Run a generated bounded service directly against the result envelope.
    /// The request reader and fixed output stay borrowed across suspension.
    /// The connection must admit the Call before execution and publish the
    /// completed reply separately. On error/cancellation, discard this output.
    /// For Yourself, retain these results locally for adoption and send a
    /// ResultsSentElsewhere control; do not transmit this results envelope.
    pub async fn serve(
        &self,
        service: &mut impl capnp::wire_capability::Service,
        output: &mut FixedBuilder<'_>,
    ) -> capnp::Result<ResultsTo> {
        let destination = self.results_to()?;
        self.service_info(service)?;
        let params = self.payload()?;
        let root = output.try_init_root::<Message>()?;
        let mut result = root.into_field(Message::RETURN).init()?;
        result
            .scalar(Return::ANSWER_ID)
            .set(self.question_id().to_wire());
        result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
        let payload = result.into_field(Return::RESULTS).init()?;
        service
            .dispatch(
                self.interface_id(),
                self.method_id(),
                params.get_content(),
                payload.into_field(Payload::CONTENT),
            )
            .await?;
        Ok(destination)
    }

    /// Dispatch with caller-selected capability/scheduling services. As with
    /// `serve`, admission precedes execution and result publication is separate.
    pub async fn serve_with_context<C>(
        &self,
        service: &mut impl capnp::wire_capability::ContextService<C>,
        output: &mut FixedBuilder<'_>,
        context: &mut C,
    ) -> capnp::Result<ResultsTo> {
        let destination = self.results_to()?;
        self.context_service_info(service)?;
        let params = self.payload()?;
        let root = output.try_init_root::<Message>()?;
        let mut result = root.into_field(Message::RETURN).init()?;
        result
            .scalar(Return::ANSWER_ID)
            .set(self.question_id().to_wire());
        result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
        let payload = result.into_field(Return::RESULTS).init()?;
        service
            .dispatch_with_context(
                self.interface_id(),
                self.method_id(),
                params.get_content(),
                payload.into_field(Payload::CONTENT),
                context,
            )
            .await?;
        Ok(destination)
    }

    /// Match both declaring interface and method ordinal before decoding. Two
    /// interfaces may legitimately use the same ordinal with different layouts.
    pub fn dispatch<I: Interface, P: Schema, R: Schema>(
        &self,
        method: Method<I, P, R>,
    ) -> capnp::Result<Dispatch<'a, I, P, R>> {
        if self.interface_id() != method.interface_id() || self.method_id() != method.ordinal() {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Unimplemented));
        }
        if !matches!(
            self.call.get_send_results_to().which(),
            Ok(rpc_capnp::call::send_results_to::Caller(()))
        ) {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Unimplemented));
        }
        let payload = self.payload()?;
        let params = method.read_params(payload.get_content())?;
        Ok(Dispatch {
            question: self.question_id(),
            method,
            params,
        })
    }
    pub fn dispatch_streaming<I: Interface, P: Schema>(
        &self,
        method: StreamingMethod<I, P>,
    ) -> capnp::Result<StreamingDispatch<'a, I, P>> {
        if self.interface_id() != method.interface_id()
            || self.method_id() != method.ordinal()
            || !matches!(
                self.call.get_send_results_to().which(),
                Ok(rpc_capnp::call::send_results_to::Caller(()))
            )
        {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Unimplemented));
        }
        let payload = self.payload()?;
        Ok(StreamingDispatch {
            question: self.question_id(),
            method,
            params: method.read_params(payload.get_content())?,
        })
    }
}

pub use capntproto_rpc_engine::questions::ResultsTo;

pub struct StreamingDispatch<'a, I, P: Schema> {
    question: AnswerId,
    method: StreamingMethod<I, P>,
    pub params: P::Ref<'a>,
}
impl<I: Interface, P: Schema> StreamingDispatch<'_, I, P> {
    pub fn question_id(&self) -> AnswerId {
        self.question
    }
    pub fn allow_cancellation(&self) -> bool {
        self.method.allow_cancellation()
    }
    /// A successful streaming call has an empty result payload. Send only when
    /// application processing completes; this acknowledgement releases credit.
    pub fn acknowledge(&self, output: &mut FixedBuilder<'_>) -> capnp::Result<()> {
        acknowledge(output, self.question)
    }
}

/// Matching an interface/method produces the corresponding parameter reader
/// and result schema. The connection owns exactly-once response admission.
pub struct Dispatch<'a, I, P: Schema, R: Schema> {
    question: AnswerId,
    method: Method<I, P, R>,
    pub params: P::Ref<'a>,
}
impl<I: Interface, P: Schema, R: Schema> Dispatch<'_, I, P, R> {
    pub fn question_id(&self) -> AnswerId {
        self.question
    }
    pub fn allow_cancellation(&self) -> bool {
        self.method.allow_cancellation()
    }
    pub fn results(
        &self,
        output: &mut FixedBuilder<'_>,
        fill: impl FnOnce(FixedStruct<'_, R>) -> capnp::Result<()>,
    ) -> capnp::Result<()> {
        results(output, self.question, self.method, fill)
    }
}

pub fn bootstrap(output: &mut FixedBuilder<'_>, id: QuestionId) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    root.field(Message::BOOTSTRAP)
        .init()?
        .scalar(Bootstrap::QUESTION_ID)
        .set(id.to_wire());
    Ok(())
}

/// Build parameters directly inside the envelope, without a second payload copy.
pub fn call<I: Interface, P: Schema, R: Schema>(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: ImportId,
    method: Method<I, P, R>,
    fill: impl FnOnce(FixedStruct<'_, P>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    call_to(output, id, Target::Imported(target), method, fill)
}

/// A wire target; higher-level clients must obtain its authority from their
/// import or question ledger. A promised target retains the source question.
#[derive(Clone, Copy)]
pub enum Target<'a> {
    Imported(ImportId),
    Promised {
        question: QuestionId,
        path: crate::pipeline::Path<'a>,
    },
}
pub fn call_to<I: Interface, P: Schema, R: Schema>(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    method: Method<I, P, R>,
    fill: impl FnOnce(FixedStruct<'_, P>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    encode_call(
        output,
        id,
        target,
        Header {
            interface: method.interface_id(),
            ordinal: method.ordinal(),
            no_pipelining: method.no_promise_pipelining(),
            yourself: false,
        },
        fill,
    )
}
pub fn streaming_call<I: Interface, P: Schema>(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    method: StreamingMethod<I, P>,
    fill: impl FnOnce(FixedStruct<'_, P>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    encode_call(
        output,
        id,
        target,
        Header {
            interface: method.interface_id(),
            ordinal: method.ordinal(),
            no_pipelining: true,
            yourself: false,
        },
        fill,
    )
}
struct Header {
    interface: u64,
    ordinal: u16,
    no_pipelining: bool,
    yourself: bool,
}

/// Method identity and forwarding hints for a dynamically generated client.
/// This carries no target authority; the driver obtains that from its graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DynamicCall {
    pub interface: u64,
    pub method: u16,
    pub no_promise_pipelining: bool,
    pub only_promise_pipeline: bool,
}

/// The untyped equivalent of `call_to`, with the same fixed-buffer contract.
pub fn dynamic_call(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    method: DynamicCall,
    fill: impl FnOnce(
        capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
    ) -> capnp::Result<()>,
) -> capnp::Result<()> {
    dynamic_call_with_handoff(output, id, target, method, false, fill)
}
pub(crate) fn dynamic_call_with_handoff(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    method: DynamicCall,
    allow_handoff: bool,
    fill: impl FnOnce(
        capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
    ) -> capnp::Result<()>,
) -> capnp::Result<()> {
    dynamic_call_payload(output, id, target, method, allow_handoff, |payload| {
        fill(payload.into_field(Payload::CONTENT))
    })
}

/// Construct the header once and let the publisher fill content and descriptors
/// through the same payload handle. No mutable envelope is reopened after copy.
pub(crate) fn dynamic_call_payload(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    method: DynamicCall,
    allow_handoff: bool,
    fill: impl FnOnce(FixedStruct<'_, Payload>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    let root = output.try_init_root::<Message>()?;
    let mut call = root.into_field(Message::CALL).init()?;
    call.scalar(Call::QUESTION_ID).set(id.to_wire());
    call.scalar(Call::INTERFACE_ID).set(method.interface);
    call.scalar(Call::METHOD_ID).set(method.method);
    call.scalar(Call::NO_PROMISE_PIPELINING)
        .set(method.no_promise_pipelining);
    call.scalar(Call::ONLY_PROMISE_PIPELINE)
        .set(method.only_promise_pipeline);
    call.scalar(Call::ALLOW_THIRD_PARTY_TAIL_CALL)
        .set(allow_handoff);
    write_target(call.field(Call::TARGET).init()?, target)?;
    fill(call.into_field(Call::PARAMS).init()?)
}
fn encode_call<P: Schema>(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    header: Header,
    fill: impl FnOnce(FixedStruct<'_, P>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut call = root.field(Message::CALL).init()?;
    call.scalar(Call::QUESTION_ID).set(id.to_wire());
    call.scalar(Call::INTERFACE_ID).set(header.interface);
    call.scalar(Call::METHOD_ID).set(header.ordinal);
    call.scalar(Call::NO_PROMISE_PIPELINING)
        .set(header.no_pipelining);
    if header.yourself {
        call.group(Call::SEND_RESULTS_TO)
            .select()
            .void(rpc_capnp::api::call::SendResultsTo::YOURSELF)
            .set();
    }
    write_target(call.field(Call::TARGET).init()?, target)?;
    let mut payload = call.field(Call::PARAMS).init()?;
    fill(payload.field(Payload::CONTENT).cast::<Struct<P>>().init()?)
}

pub fn results<I: Interface, P: Schema, R: Schema>(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    _: Method<I, P, R>,
    fill: impl FnOnce(FixedStruct<'_, R>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut result = root.field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(id.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    let mut payload = result.field(Return::RESULTS).init()?;
    fill(payload.field(Payload::CONTENT).cast::<Struct<R>>().init()?)
}

pub fn dynamic_results(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    fill: impl FnOnce(
        capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
    ) -> capnp::Result<()>,
) -> capnp::Result<()> {
    let root = output.try_init_root::<Message>()?;
    let mut result = root.into_field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(id.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    fill(
        result
            .into_field(Return::RESULTS)
            .init()?
            .into_field(Payload::CONTENT),
    )
}

pub fn finish(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    release_caps: bool,
) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut finish = root.field(Message::FINISH).init()?;
    finish.scalar(Finish::QUESTION_ID).set(id.to_wire());
    finish.scalar(Finish::RELEASE_RESULT_CAPS).set(release_caps);
    Ok(())
}
pub fn release(output: &mut FixedBuilder<'_>, id: ImportId, count: u32) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut release = root.field(Message::RELEASE).init()?;
    release.scalar(Release::ID).set(id.to_wire());
    release.scalar(Release::REFERENCE_COUNT).set(count);
    Ok(())
}

pub fn cancelled(output: &mut FixedBuilder<'_>, id: AnswerId) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut result = root.field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(id.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    result.void(Return::CANCELED).set();
    Ok(())
}

pub fn acknowledge(output: &mut FixedBuilder<'_>, id: AnswerId) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut result = root.field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(id.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    result.field(Return::RESULTS).init()?;
    Ok(())
}

/// A nonterminal method/network failure, using explicit capability cleanup.
pub fn exception(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    kind: capnp::ErrorKind,
    reason: &str,
) -> capnp::Result<()> {
    use rpc_capnp::api::{exception::Type, Exception};
    let root = output.try_init_root::<Message>()?;
    let mut result = root.into_field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(id.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    let mut exception = result.field(Return::EXCEPTION).init()?;
    exception.scalar(Exception::TYPE).set(match kind {
        capnp::ErrorKind::Disconnected => Type::Disconnected,
        capnp::ErrorKind::Overloaded => Type::Overloaded,
        capnp::ErrorKind::Unimplemented => Type::Unimplemented,
        _ => Type::Failed,
    });
    exception.field(Exception::REASON).copy_from(reason)
}

/// Shared target encoding for Calls and loopback barriers.
pub(crate) fn write_target(
    mut output: FixedStruct<'_, MessageTarget>,
    target: Target<'_>,
) -> capnp::Result<()> {
    match target {
        Target::Imported(id) => output.scalar(MessageTarget::IMPORTED_CAP).set(id.to_wire()),
        Target::Promised { question, path } => path.write(
            output.field(MessageTarget::PROMISED_ANSWER).init()?,
            question,
        )?,
    }
    Ok(())
}

pub fn sender_loopback(
    output: &mut FixedBuilder<'_>,
    id: capntproto_rpc_engine::ids::EmbargoId,
    target: Target<'_>,
) -> capnp::Result<()> {
    use rpc_capnp::api::{disembargo::Context, Disembargo};
    let mut root = output.try_init_root::<Message>()?;
    let mut barrier = root.field(Message::DISEMBARGO).init()?;
    write_target(barrier.field(Disembargo::TARGET).init()?, target)?;
    barrier
        .group(Disembargo::CONTEXT)
        .select()
        .scalar(Context::SENDER_LOOPBACK)
        .set(id.to_wire());
    Ok(())
}

/// Echo an opaque peer-owned barrier context after the original route drains.
/// Determining that ordering point belongs to the connection's call dispatcher.
pub fn receiver_loopback(
    output: &mut FixedBuilder<'_>,
    id: u32,
    target: Target<'_>,
) -> capnp::Result<()> {
    use rpc_capnp::api::{disembargo::Context, Disembargo};
    let mut root = output.try_init_root::<Message>()?;
    let mut barrier = root.field(Message::DISEMBARGO).init()?;
    write_target(barrier.field(Disembargo::TARGET).init()?, target)?;
    barrier
        .group(Disembargo::CONTEXT)
        .select()
        .scalar(Context::RECEIVER_LOOPBACK)
        .set(id);
    Ok(())
}

/// Build a terminal protocol error. The caller stops normal admission and
/// controls whether this best-effort Abort is transmitted before IO terminates.
pub fn abort(
    output: &mut FixedBuilder<'_>,
    kind: capnp::ErrorKind,
    reason: &str,
) -> capnp::Result<()> {
    use rpc_capnp::api::{exception::Type, Exception};
    let mut root = output.try_init_root::<Message>()?;
    let mut exception = root.field(Message::ABORT).init()?;
    exception.scalar(Exception::TYPE).set(match kind {
        capnp::ErrorKind::Disconnected => Type::Disconnected,
        capnp::ErrorKind::Overloaded => Type::Overloaded,
        capnp::ErrorKind::Unimplemented => Type::Unimplemented,
        _ => Type::Failed,
    });
    exception.field(Exception::REASON).copy_from(reason)
}

/// Echo a control that this peer does not implement. Bounded copying preserves
/// unknown fields and rejects capability-pointer trees; this does not publish
/// any fresh grants or invoke a capability hook.
pub fn unimplemented(
    output: &mut FixedBuilder<'_>,
    message: rpc_capnp::message::Reader<'_>,
) -> capnp::Result<()> {
    use capnp::traits::IntoInternalStructReader;
    let mut root = output.try_init_root::<Message>()?;
    root.field(Message::UNIMPLEMENTED)
        .copy_from(message.into_internal_struct_reader().into())
}

/// A same-connection tail Call asks the receiver to keep its results locally.
pub fn tail_call_to<I: Interface, P: Schema, R: Schema>(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    method: Method<I, P, R>,
    fill: impl FnOnce(FixedStruct<'_, P>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    encode_call(
        output,
        id,
        target,
        Header {
            interface: method.interface_id(),
            ordinal: method.ordinal(),
            no_pipelining: method.no_promise_pipelining(),
            yourself: true,
        },
        fill,
    )
}
/// Adopt a previously published redirected question in the same connection.
pub fn tail_results(
    output: &mut FixedBuilder<'_>,
    answer: AnswerId,
    source: QuestionId,
) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut result = root.field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(answer.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    result
        .scalar(Return::TAKE_FROM_OTHER_QUESTION)
        .set(source.to_wire());
    Ok(())
}
pub fn results_sent_elsewhere(
    output: &mut FixedBuilder<'_>,
    answer: AnswerId,
) -> capnp::Result<()> {
    let mut root = output.try_init_root::<Message>()?;
    let mut result = root.field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(answer.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    result.void(Return::RESULTS_SENT_ELSEWHERE).set();
    Ok(())
}

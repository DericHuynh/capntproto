//! Standard third-party message codecs over caller-owned buffers. Network
//! metadata is opaque; the caller's authenticated network verifies its meaning.
use crate::{
    calls::{self, Target},
    rpc_capnp::api::{
        disembargo, Accept, CapDescriptor, Disembargo, Join, Message, Payload, Provide, Return,
        ThirdPartyAnswer,
    },
};
use capnp::{
    field_api::{FixedField, Generic},
    message::FixedBuilder,
};
use capntproto_rpc_engine::{
    exports,
    ids::{AnswerId, QuestionId, WireId},
};

type Metadata<'a> = FixedField<'a, Generic<capnp::any_pointer::Owned>>;
/// A Join result carries opaque, capability-free network metadata. Publish
/// through answer admission so validation and its Finish obligation stay atomic.
pub fn join_result(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    result: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    let root = output.try_init_root::<Message>()?;
    let mut returned = root.into_field(Message::RETURN).init()?;
    returned.scalar(Return::ANSWER_ID).set(id.to_wire());
    returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    result(
        returned
            .into_field(Return::RESULTS)
            .init()?
            .into_field(Payload::CONTENT),
    )
}
/// Opt a prepared ordinary Call into authenticated third-party result handoff.
/// The network must service the driver's Await/Adopt jobs when this is enabled.
pub fn allow_third_party_tail_call(output: &mut FixedBuilder<'_>) -> capnp::Result<()> {
    output
        .try_get_root::<Message>()?
        .field(Message::CALL)
        .edit()?
        .scalar(crate::rpc_capnp::api::Call::ALLOW_THIRD_PARTY_TAIL_CALL)
        .set(true);
    Ok(())
}
/// Redirect a prepared Call to a caller-authenticated destination. Publish using
/// a third-party question reservation; other reservations reject this envelope.
pub fn send_results_to_third_party(
    output: &mut FixedBuilder<'_>,
    destination: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    destination(
        output
            .try_get_root::<Message>()?
            .field(Message::CALL)
            .edit()?
            .group(crate::rpc_capnp::api::Call::SEND_RESULTS_TO)
            .select()
            .field(crate::rpc_capnp::api::call::SendResultsTo::THIRD_PARTY),
    )
}
/// Announce a caller-authenticated third-party result rendezvous. The original
/// Call must have opted in through allowThirdPartyTailCall.
pub fn await_from_third_party(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    metadata: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    let root = output.try_init_root::<Message>()?;
    let mut returned = root.into_field(Message::RETURN).init()?;
    returned.scalar(Return::ANSWER_ID).set(id.to_wire());
    returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    metadata(returned.into_field(Return::AWAIT_FROM_THIRD_PARTY))
}
pub fn third_party_answer(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    completion: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    if !(1 << 30..1 << 31).contains(&id.to_wire()) {
        return Err(capnp::Error::from_kind(capnp::ErrorKind::Failed));
    }
    let root = output.try_init_root::<Message>()?;
    let mut answer = root.into_field(Message::THIRD_PARTY_ANSWER).init()?;
    answer.scalar(ThirdPartyAnswer::ANSWER_ID).set(id.to_wire());
    completion(answer.into_field(ThirdPartyAnswer::COMPLETION))
}
fn ordinary(id: QuestionId) -> capnp::Result<()> {
    if id.to_wire() >= 1 << 30 {
        return Err(capnp::Error::from_kind(capnp::ErrorKind::Failed));
    }
    Ok(())
}
pub fn provide(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    recipient: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    ordinary(id)?;
    let root = output.try_init_root::<Message>()?;
    let mut message = root.into_field(Message::PROVIDE).init()?;
    message.scalar(Provide::QUESTION_ID).set(id.to_wire());
    calls::write_target(message.field(Provide::TARGET).init()?, target)?;
    recipient(message.into_field(Provide::RECIPIENT))
}
pub fn accept(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    embargo: Option<&[u8]>,
    completion: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    ordinary(id)?;
    let root = output.try_init_root::<Message>()?;
    let mut message = root.into_field(Message::ACCEPT).init()?;
    message.scalar(Accept::QUESTION_ID).set(id.to_wire());
    if let Some(id) = embargo {
        message.field(Accept::EMBARGO).copy_from(id)?;
    }
    completion(message.into_field(Accept::PROVISION))
}
pub fn disembargo_accept(
    output: &mut FixedBuilder<'_>,
    target: Target<'_>,
    id: &[u8],
) -> capnp::Result<()> {
    let root = output.try_init_root::<Message>()?;
    let mut message = root.into_field(Message::DISEMBARGO).init()?;
    calls::write_target(message.field(Disembargo::TARGET).init()?, target)?;
    message
        .group(Disembargo::CONTEXT)
        .select()
        .field(disembargo::Context::ACCEPT)
        .copy_from(id)
}
pub fn join(
    output: &mut FixedBuilder<'_>,
    id: QuestionId,
    target: Target<'_>,
    key_part: impl FnOnce(Metadata<'_>) -> capnp::Result<()>,
) -> capnp::Result<()> {
    ordinary(id)?;
    let root = output.try_init_root::<Message>()?;
    let mut message = root.into_field(Message::JOIN).init()?;
    message.scalar(Join::QUESTION_ID).set(id.to_wire());
    calls::write_target(message.field(Join::TARGET).init()?, target)?;
    key_part(message.into_field(Join::KEY_PART))
}
/// Return the accepted capability itself, including the exact reserved grant.
/// Publish with `answer::try_send_granting`; encoding does not commit authority.
pub fn accepted(
    output: &mut FixedBuilder<'_>,
    id: AnswerId,
    grant: &exports::Grant<'_>,
) -> capnp::Result<()> {
    let root = output.try_init_root::<Message>()?;
    let mut result = root.into_field(Message::RETURN).init()?;
    result.scalar(Return::ANSWER_ID).set(id.to_wire());
    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
    let mut payload = result.field(Return::RESULTS).init()?;
    payload.field(Payload::CONTENT).set_capability_index(0)?;
    let mut descriptors = payload.field(Payload::CAP_TABLE).init(1)?;
    let mut descriptor = descriptors.get_mut(0).unwrap();
    if grant.is_promise() {
        descriptor
            .scalar(CapDescriptor::SENDER_PROMISE)
            .set(grant.id().to_wire());
    } else {
        descriptor
            .scalar(CapDescriptor::SENDER_HOSTED)
            .set(grant.id().to_wire());
    }
    Ok(())
}

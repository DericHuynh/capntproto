//! Announce and register a callee-selected answer in the same output transition.
use super::*;

#[must_use]
pub struct ThirdPartyAnswer<'a, 'id, 'answers, 'pool, 'storage, 'bytes, T> {
    pending: answers::ThirdPartyAdmission<'a, 'id, 'answers, T>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
pub fn third_party_answer<'a, 'id, 'answers, 'pool, 'storage, 'bytes, T>(
    pending: answers::ThirdPartyAdmission<'a, 'id, 'answers, T>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<ThirdPartyAnswer<'a, 'id, 'answers, 'pool, 'storage, 'bytes, T>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        let rpc_capnp::message::ThirdPartyAnswer(answer) =
            root.which().map_err(|_| Error::Unsupported)?
        else {
            return Err(Error::Mismatch);
        };
        let answer = answer?;
        if answer.get_answer_id() != pending.id().to_wire() {
            return Err(Error::Mismatch);
        }
        answer.get_completion().validate_capability_indices(0)?;
        Ok(())
    })?;
    frame.offer()?;
    Ok(ThirdPartyAnswer { pending, frame })
}
impl<'id, 'pool, T> ThirdPartyAnswer<'_, 'id, '_, 'pool, '_, '_, T> {
    pub fn accept(
        self,
        owner: T,
        output: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> answers::Key<'id> {
        let key = self.pending.accept(owner);
        self.frame.accept(output);
        key
    }
}

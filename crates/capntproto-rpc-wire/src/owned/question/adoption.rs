//! Synchronous reservation before an asynchronous authenticated rendezvous.
use super::*;
use capntproto_rpc_engine::ids::QuestionId;

/// Opaque completion metadata paired with the exact adopted question generation.
/// The driver keeps its response private until the caller authenticates this
/// job. Releasing the job through the driver cancels the adopted question.
#[must_use = "authenticate or release through the originating driver"]
pub struct IncomingAdoption<'pool, 'id> {
    frame: Frame<'pool>,
    key: questions::Key<'id>,
}
impl<'pool, 'id> IncomingAdoption<'pool, 'id> {
    pub fn key(&self) -> questions::Key<'id> {
        self.key
    }
    pub fn with_completion<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(capnp::any_pointer::Reader<'_>) -> R,
    ) -> Result<R, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::ThirdPartyAnswer(answer) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            Ok(f(answer?.get_completion()))
        })?
    }
    pub(crate) fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) {
        self.frame.release(messages);
    }
}
impl<'pool, 'id> Question<'pool, 'id> {
    /// Reserve a callee-selected ID before exposing completion metadata to the
    /// caller. Return may arrive before authorization. The second frame lease
    /// keeps that metadata alive independently of Return consuming the original.
    #[allow(clippy::type_complexity)]
    pub(crate) fn adopt(
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        questions: &mut questions::Questions<'id, '_>,
    ) -> Result<(Self, IncomingAdoption<'pool, 'id>), Rejected<'pool, ()>> {
        let outcome = (|| {
            let id = frame.with(messages, |root| -> Result<_, Error> {
                let rpc_capnp::message::ThirdPartyAnswer(answer) =
                    root.which().map_err(|_| Error::WrongMessage)?
                else {
                    return Err(Error::WrongMessage);
                };
                let answer = answer?;
                answer.get_completion().validate_capability_indices(0)?;
                Ok(QuestionId::from_wire(answer.get_answer_id()))
            })??;
            let reservation = questions
                .reserve_adoption(id)
                .map_err(|e| Error::Admission(inbound::Error::Question(e)))?;
            let lease = messages.retain(&frame.lease).map_err(Error::Storage)?;
            Ok((reservation.accept(), Frame::new(lease, frame.options)))
        })();
        match outcome {
            Ok((local, metadata)) => {
                let key = local.key();
                Ok((
                    Self {
                        original: super::Original::Frame(frame),
                        local: Some(local),
                        key,
                    },
                    IncomingAdoption {
                        frame: metadata,
                        key,
                    },
                ))
            }
            Err(error) => Err(Rejected {
                error,
                frame,
                owner: (),
            }),
        }
    }
}

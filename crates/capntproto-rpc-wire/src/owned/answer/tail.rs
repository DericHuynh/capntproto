//! Retain the exact tail Return alongside its validated redirected question.
use super::*;
#[allow(clippy::too_many_arguments)]
pub fn try_send_tail<'pool, 'a, 'q, A>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    source: &capntproto_rpc_engine::questions::Local<'q>,
    questions: &capntproto_rpc_engine::questions::Questions<'q, '_>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Rejected<'pool>> {
    let pending = match answers.prepare_return(key) {
        Ok(pending) => pending,
        Err(error) => {
            return Err(Rejected {
                error: Error::Admission(inbound::Error::Answer(error)),
                frame,
                grants: (),
            })
        }
    };
    let lease = match messages.retain(&frame.lease) {
        Ok(lease) => lease,
        Err(error) => {
            return Err(Rejected {
                error: Error::Storage(error),
                frame,
                grants: (),
            })
        }
    };
    let prepared = match crate::admission::tail_answer(
        pending,
        source,
        questions,
        messages,
        lease,
        frame.options,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            return Err(Rejected {
                error: Error::Output(error),
                frame,
                grants: (),
            })
        }
    };
    let retired = prepared.accept(queue);
    Ok(keep_frame(key, answers, retired, frame))
}

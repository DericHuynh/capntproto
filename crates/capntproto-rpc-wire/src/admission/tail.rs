//! Bind a same-connection tail return to a retained redirected question.
use super::*;

#[must_use]
pub struct TailAnswer<'a, 'answer, 'answers, 'question, 'questions, 'pool, 'storage, 'bytes, T> {
    pending: answers::Return<'a, 'answer, 'answers, T>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
    _source: &'a questions::Local<'question>,
    _questions: &'a questions::Questions<'question, 'questions>,
}

/// The redirected Call must have entered the same FIFO first. The caller keeps
/// its question reference in the original answer's task until Finish; this
/// binding borrows it through Return acceptance and never creates wire grants.
#[allow(clippy::too_many_arguments)]
pub fn tail_answer<'a, 'answer, 'answers, 'question, 'questions, 'pool, 'storage, 'bytes, T>(
    pending: answers::Return<'a, 'answer, 'answers, T>,
    source: &'a questions::Local<'question>,
    questions: &'a questions::Questions<'question, 'questions>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<
    TailAnswer<'a, 'answer, 'answers, 'question, 'questions, 'pool, 'storage, 'bytes, T>,
    Error,
> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        let rpc_capnp::message::Return(result) = root.which().map_err(|_| Error::Unsupported)?
        else {
            return Err(Error::Mismatch);
        };
        let result = result?;
        let rpc_capnp::return_::TakeFromOtherQuestion(id) =
            result.which().map_err(|_| Error::Unsupported)?
        else {
            return Err(Error::Mismatch);
        };
        // A protected method can tail-call after Finish. The exact parent
        // generation still owes its Return; the retained redirected child
        // supplies the same ownership transfer as legacy.
        if pending.is_redirected()
            || result.get_answer_id() != pending.id().to_wire()
            || result.get_release_param_caps()
            || result.get_no_finish_needed()
            || id != source.key().id().to_wire()
            || !questions.is_live(source.key())
            || questions.results_to(source.key().id()) != Ok(questions::ResultsTo::Yourself)
        {
            return Err(Error::Mismatch);
        }
        Ok(())
    })?;
    frame.offer()?;
    Ok(TailAnswer {
        pending,
        frame,
        _source: source,
        _questions: questions,
    })
}
impl<'pool, T> TailAnswer<'_, '_, '_, '_, '_, 'pool, '_, '_, T> {
    pub fn accept(self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) -> Option<T> {
        self.frame.accept(queue);
        self.pending.accept()
    }
}

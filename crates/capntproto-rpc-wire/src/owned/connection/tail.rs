//! Retain same-connection tail children until the original answer is retired.
use super::*;
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Publish takeFromOtherQuestion after the redirected Call. Rejected output
    /// retains both tasks and leaves the child available for exactly one retry.
    pub fn tail_answer(
        &mut self,
        parent: answers::Key<'id>,
        child: questions::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<A>, Error> {
        self.check_live()?;
        let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, child)
            .ok_or(Error::UnknownQuestion)?;
        let exchange = &mut self.exchanges[index];
        if !exchange.authorized {
            return Err(Error::PendingAuthorization);
        }
        if exchange.tail_taken {
            return Err(Error::TailAlreadyUsed);
        }
        let source = match exchange.exchange.as_ref().unwrap() {
            Exchange::Pending(question) => question.local(),
            Exchange::Returned(response) => response.local_ref(),
        }
        .ok_or(Error::PendingResponse)?;
        let output = queue
            .reserve()
            .map_err(|error| Error::Input(super::super::Error::Storage(error)))?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            calls::tail_results(builder, parent.id(), child.id())
        })
        .map_err(Error::Input)?;
        let retained = match self.ledgers.questions.retain(source) {
            Ok(retained) => retained,
            Err(error) => {
                frame.release(messages);
                return Err(Error::Question(error));
            }
        };
        match answer::try_send_tail(
            parent,
            &mut self.ledgers.answers,
            source,
            &self.ledgers.questions,
            messages,
            frame,
            output,
        ) {
            Ok(retired) => {
                exchange.tail_taken = true;
                if let Some(mut retired) = retired {
                    retired.owner.redirect = Some(retained);
                    Ok(Some(retired.release(messages).release(
                        messages,
                        &mut self.runtime,
                        |child| self.ledgers.questions.release(child),
                    )))
                } else {
                    self.ledgers
                        .answers
                        .get_mut(parent)
                        .expect("retained tail parent")
                        .owner
                        .redirect = Some(retained);
                    Ok(None)
                }
            }
            Err(rejected) => {
                self.ledgers.questions.release(retained);
                rejected.frame.release(messages);
                Err(Error::Input(rejected.error))
            }
        }
    }
    pub(super) fn poll_redirect(
        &mut self,
        index: usize,
    ) -> Result<Option<pipelines::Progress<'id>>, Error> {
        self.pipelines
            .poll_redirect(
                index,
                &mut self.ledgers.questions,
                &mut self.runtime,
                &mut self.ledgers.answers,
                self.imports.peer(),
            )
            .map_err(Error::Pipeline)
    }
}

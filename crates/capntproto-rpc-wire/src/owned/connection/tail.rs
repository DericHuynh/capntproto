//! Retain same-connection tail children until the original answer is retired.
use super::*;
struct Entry<'id> {
    parent: answers::Key<'id>,
    source: questions::Local<'id>,
}
pub struct TailSlot<'id> {
    entry: Option<Entry<'id>>,
}
impl TailSlot<'_> {
    pub const EMPTY: Self = Self { entry: None };
    pub(super) fn is_occupied(&self) -> bool {
        self.entry.is_some()
    }
}
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
        let tail = self
            .tails
            .vacant_mut(|_, slot| slot.entry.is_none(), || TailSlot::EMPTY)
            .ok_or(Error::Busy)?;
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
                tail.entry = Some(Entry {
                    parent,
                    source: retained,
                });
                Ok(retired
                    .map(|state| state.release(messages).release(messages, &mut self.runtime)))
            }
            Err(rejected) => {
                self.ledgers.questions.release(retained);
                rejected.frame.release(messages);
                Err(Error::Input(rejected.error))
            }
        }
    }
    pub(super) fn drain_tail(&mut self, index: usize) -> bool {
        let Some(slot) = self.tails.get_mut(index) else {
            return false;
        };
        // A finished parent may still own accepted pipeline dependencies. Keep
        // the redirect until that exact generation has actually been retired.
        if slot
            .entry
            .as_ref()
            .is_some_and(|entry| self.ledgers.answers.get(entry.parent).is_err())
        {
            self.ledgers
                .questions
                .release(slot.entry.take().unwrap().source);
            true
        } else {
            false
        }
    }
    pub(super) fn poll_redirect(
        &mut self,
        index: usize,
    ) -> Result<Option<pipelines::Progress<'id>>, Error> {
        for slot in &*self.tails {
            if let Some(entry) = &slot.entry {
                let progress = self
                    .pipelines
                    .poll_redirect(
                        index,
                        entry.parent,
                        &entry.source,
                        &mut self.ledgers.questions,
                        &mut self.runtime,
                        &mut self.ledgers.answers,
                        self.imports.peer(),
                    )
                    .map_err(Error::Pipeline)?;
                if progress.is_some() {
                    return Ok(progress);
                }
            }
        }
        Ok(None)
    }
    pub(super) fn drain_terminal_tail(&mut self) -> bool {
        let Some(entry) = self.tails.iter_mut().find_map(|slot| slot.entry.take()) else {
            return false;
        };
        self.ledgers.questions.release(entry.source);
        true
    }
}

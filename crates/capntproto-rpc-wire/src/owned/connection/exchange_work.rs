//! Response work is queued by its exact producer transition. A returned value
//! retains its question until ordered observers and cancellation cleanup settle.
use super::*;
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub(super) fn refresh_exchange(&mut self, key: questions::Key<'id>) {
        let Some(index) = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, key) else {
            return;
        };
        let slot = &self.exchanges[index];
        let outgoing = self.ledgers.questions.has_observers(key, false);
        let observers = outgoing || self.ledgers.questions.has_observers(key, true);
        let returned = match &slot.exchange {
            Some(Exchange::Returned(response)) => Some(response),
            _ => None,
        };
        let ordered = observers
            && returned.is_some_and(|response| {
                response.requires_ordered_propagation() || slot.handoff.is_some()
            });
        let runnable = slot.authorized
            && outgoing
            && returned.is_some_and(|response| {
                response.await_method().is_none() || slot.handoff.is_some() || slot.canceled
            });
        let canceled =
            slot.canceled && (returned.is_some() || key.id().is_pipeline_only()) && !observers;
        let completed = slot.completion_owner.is_some()
            && slot.authorized
            && !observers
            && returned.is_some_and(|response| {
                response.await_method().is_none() || slot.handoff.is_some() || slot.canceled
            });
        if completed {
            self.completion_ready
                .push(&mut self.exchanges, index, |slot| &mut slot.completion_work);
        } else {
            self.completion_ready
                .remove(&mut self.exchanges, index, |slot| &mut slot.completion_work);
        }
        if runnable {
            self.ledgers.questions.begin_observer_work(key, false);
        }
        if runnable && self.ledgers.questions.observer_work(key, false).is_some() {
            self.outgoing_ready
                .push(&mut self.exchanges, index, |slot| &mut slot.outgoing_work);
        } else {
            self.outgoing_ready
                .remove(&mut self.exchanges, index, |slot| &mut slot.outgoing_work);
        }
        if canceled {
            self.canceled_ready
                .push(&mut self.exchanges, index, |slot| &mut slot.canceled_work);
        } else {
            self.canceled_ready
                .remove(&mut self.exchanges, index, |slot| &mut slot.canceled_work);
        }
        let slot = &mut self.exchanges[index];
        if slot.ordered != ordered {
            if ordered {
                self.ordered_propagation += 1;
            } else {
                self.ordered_propagation -= 1;
            }
            slot.ordered = ordered;
        }
    }
    pub(super) fn clear_exchange_work(&mut self, key: questions::Key<'id>) {
        let Some(index) = self.ledgers.questions.owner(key) else {
            return;
        };
        self.outgoing_ready
            .remove(&mut self.exchanges, index, |slot| &mut slot.outgoing_work);
        self.canceled_ready
            .remove(&mut self.exchanges, index, |slot| &mut slot.canceled_work);
        self.completion_ready
            .remove(&mut self.exchanges, index, |slot| &mut slot.completion_work);
        if core::mem::replace(&mut self.exchanges[index].ordered, false) {
            self.ordered_propagation -= 1;
        }
        if self.exchanges[index].exchange.is_none() {
            self.free_exchanges
                .release(index, &mut self.exchanges, |slot| &mut slot.free);
        }
    }
    /// A completion is ready only after its exact observer fence settles.
    /// The caller validates the question generation against its hosted owner.
    pub fn take_ready_completion(&mut self) -> Option<(questions::Key<'id>, usize)> {
        let index = self
            .completion_ready
            .pop(&mut self.exchanges, |slot| &mut slot.completion_work)?;
        let slot = &self.exchanges[index];
        Some((
            slot.exchange.as_ref().expect("ready completion").key(),
            slot.completion_owner.expect("bound completion").get() - 1,
        ))
    }
    pub fn has_ready_completions(&self) -> bool {
        !self.completion_ready.is_empty()
    }
    pub(super) fn poll_next_outgoing(
        &mut self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<Propagation, Error> {
        // Round-robin exact abandonment, new Return traversal and capacity
        // readiness. A blocked projection cannot hide a sibling of that question.
        for _ in 0..3 {
            let source = self.outgoing_source;
            self.outgoing_source = (source + 1) % 3;
            let index = match source {
                0 => {
                    let Some(event) = self.outgoing.next_event(&mut self.runtime) else {
                        continue;
                    };
                    if let Some(question) =
                        self.outgoing
                            .abandon(event, &mut self.ledgers.questions, &mut self.runtime)
                    {
                        self.refresh_exchange(question);
                        return Ok(Propagation::Abandoned);
                    }
                    return Ok(Propagation::Unrelated);
                }
                1 => {
                    let Some(exchange) = self
                        .outgoing_ready
                        .pop(&mut self.exchanges, |s| &mut s.outgoing_work)
                    else {
                        continue;
                    };
                    let key = self.exchanges[exchange]
                        .exchange
                        .as_ref()
                        .expect("queued response")
                        .key();
                    self.ledgers
                        .questions
                        .observer_work(key, false)
                        .expect("queued observer traversal")
                }
                _ => {
                    let available = self
                        .outgoing
                        .has_waiters()
                        .then(|| self.available_resources());
                    let Some(index) = self
                        .outgoing
                        .next_retry(|r| available.as_ref().expect("active resource wait").ready(r))
                    else {
                        continue;
                    };
                    index
                }
            };
            let result = self.poll_outgoing(index, messages);
            return match result {
                Err(_) if self.outgoing.resource_waiting(index) => Ok(Propagation::Pending),
                other => other,
            };
        }
        Ok(Propagation::Unrelated)
    }
}

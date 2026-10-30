//! Bounded reply delivery using caller-owned completion slots. Poll this registry
//! alongside connection maintenance and IO. A receiver wakes its producer on
//! cancellation or consumption, including while the transport remains idle.
use super::*;
use capntproto_rpc_engine::completion::{Delivery, Sender};
use capntproto_rpc_engine::free_slots::{FreeSlots, Link as FreeLink};
use core::task::{Context, Poll};

enum ReplyDelivery<'wait, 'pool, 'id, I, const CAPS: usize> {
    Wire(Delivery<'wait, Response<'pool, 'id, I, CAPS>>),
    Output(Delivery<'wait, CallResult<'pool, 'id, I, CAPS>>),
}
impl<'pool, 'id, I, const CAPS: usize> ReplyDelivery<'_, 'pool, 'id, I, CAPS> {
    fn poll_observed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self {
            Self::Wire(d) => d.poll_observed(cx),
            Self::Output(d) => d.poll_observed(cx),
        }
    }
    fn take_abandoned(&mut self) -> Option<CallResult<'pool, 'id, I, CAPS>> {
        match self {
            Self::Wire(d) => d.take_abandoned().map(|r| Ok(CallOutput::Remote(r))),
            Self::Output(d) => d.take_abandoned(),
        }
    }
    fn cancel(&mut self) -> Option<CallResult<'pool, 'id, I, CAPS>> {
        match self {
            Self::Wire(d) => d.cancel().map(|r| Ok(CallOutput::Remote(r))),
            Self::Output(d) => d.cancel(),
        }
    }
}
struct Entry<'wait, 'pool, 'id, I, const CAPS: usize> {
    question: questions::Key<'id>,
    delivery: ReplyDelivery<'wait, 'pool, 'id, I, CAPS>,
    waiting: bool,
}
pub struct ReplySlot<'wait, 'pool, 'id, I, const CAPS: usize> {
    entry: Option<Entry<'wait, 'pool, 'id, I, CAPS>>,
    free: FreeLink,
}
impl<I, const CAPS: usize> ReplySlot<'_, '_, '_, I, CAPS> {
    pub const EMPTY: Self = Self {
        entry: None,
        free: FreeLink::EMPTY,
    };
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyError {
    NonemptyStorage,
    Full,
    AlreadyTracked,
    Connection(Error),
}
impl core::fmt::Display for ReplyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC reply delivery: {self:?}")
    }
}
impl core::error::Error for ReplyError {}
#[must_use = "retry tracking or cancel the rejected sender"]
pub struct RejectedReply<'wait, 'pool, 'id, I, const CAPS: usize> {
    pub error: ReplyError,
    pub sender: Sender<'wait, Response<'pool, 'id, I, CAPS>>,
}
#[must_use = "retry tracking or cancel the rejected sender"]
pub struct RejectedOutputReply<'wait, 'pool, 'id, I, const CAPS: usize> {
    pub error: ReplyError,
    pub sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyProgress {
    /// No binding occupied the supplied index.
    Idle,
    /// Waiting for the Return or for the receiver to observe its reply.
    Pending,
    /// The slot now owns the response; keep polling to observe its disposition.
    Delivered,
    /// The receiver took the response and owns its eventual release.
    Consumed,
    /// The receiver closed before delivery; the driver retains any late Return.
    Canceled,
    /// A dropped receiver's unclaimed response was released to the driver.
    Reclaimed,
}
/// The registry borrows both a caller-owned binding array and each reply slot.
/// Neither borrow is tied to the driver's storage lifetime. Do not manually take
/// responses for registered questions through the lower-level driver methods.
/// Cancel all live bindings before terminating the driver; return any responses
/// already taken by receivers through `Driver::release_response` as usual.
#[must_use = "drain or cancel reply bindings before terminating their connection"]
pub struct Replies<'s, 'wait, 'pool, 'id, I, const CAPS: usize> {
    slots: &'s mut [ReplySlot<'wait, 'pool, 'id, I, CAPS>],
    free: FreeSlots,
    live: usize,
}
impl<'s, 'wait, 'pool, 'id, I, const CAPS: usize> Replies<'s, 'wait, 'pool, 'id, I, CAPS> {
    pub fn new(slots: &'s mut [ReplySlot<'wait, 'pool, 'id, I, CAPS>]) -> Result<Self, ReplyError> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(ReplyError::NonemptyStorage);
        }
        Ok(Self {
            slots,
            free: FreeSlots::new(),
            live: 0,
        })
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Register an admitted outgoing question. Rejection leaves its sender
    /// intact, so the caller can retry on capacity pressure without canceling
    /// the future. This method never sends or accepts a protocol message.
    #[allow(clippy::result_large_err)]
    pub fn track<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        question: questions::Key<'id>,
        sender: Sender<'wait, Response<'pool, 'id, I, CAPS>>,
    ) -> Result<usize, RejectedReply<'wait, 'pool, 'id, I, CAPS>> {
        let checked = self.available(driver, question);
        let index = match checked {
            Ok(index) => index,
            Err(error) => return Err(RejectedReply { error, sender }),
        };
        self.slots[index].entry = Some(Entry {
            question,
            delivery: ReplyDelivery::Wire(sender.track()),
            waiting: true,
        });
        self.commit(index);
        Ok(index)
    }

    fn commit(&mut self, index: usize) {
        let committed = self.free.take(self.slots, |slot| &mut slot.free);
        debug_assert_eq!(committed, index);
        self.live += 1;
    }

    fn retire(&mut self, index: usize) {
        self.slots[index].entry = None;
        self.free.release(index, self.slots, |slot| &mut slot.free);
        self.live -= 1;
    }

    fn available<A, P: Copy + Eq, S, const PATH: usize>(
        &self,
        driver: &Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        question: questions::Key<'id>,
    ) -> Result<usize, ReplyError> {
        driver.check_live().map_err(ReplyError::Connection)?;
        if self.slots.iter().any(|slot| {
            slot.entry
                .as_ref()
                .is_some_and(|entry| entry.question == question)
        }) {
            return Err(ReplyError::AlreadyTracked);
        }
        // The question records its exchange owner, including after Return.
        // Verify that generation instead of searching unrelated exchanges.
        if !ExchangeSlot::index(&driver.exchanges, &driver.ledgers.questions, question).is_some_and(
            |index| {
                let slot = &driver.exchanges[index];
                !slot.canceled && slot.authorized
            },
        ) {
            return Err(ReplyError::Connection(Error::UnknownQuestion));
        }
        self.free
            .peek(self.slots, |slot| &slot.free)
            .ok_or(ReplyError::Full)
    }
    /// Track ordinary or third-party completion using the same caller-owned slot.
    /// The old response-only track method is for calls which do not opt into
    /// third-party tail calls. This variant also delivers network failures.
    #[allow(clippy::result_large_err)]
    pub fn track_output<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        question: questions::Key<'id>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
    ) -> Result<usize, RejectedOutputReply<'wait, 'pool, 'id, I, CAPS>> {
        let index = match self.available(driver, question) {
            Ok(index) => index,
            Err(error) => return Err(RejectedOutputReply { error, sender }),
        };
        self.slots[index].entry = Some(Entry {
            question,
            delivery: ReplyDelivery::Output(sender.track()),
            waiting: true,
        });
        self.commit(index);
        Ok(index)
    }

    /// Poll one binding. Register the same task waker that drives IO and
    /// maintenance. `Pending` does not self-wake; a Return, pipeline propagation,
    /// or receiver notification supplies the next reason to poll.
    pub fn poll_one<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        index: usize,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        cx: &mut Context<'_>,
    ) -> Result<ReplyProgress, Error> {
        let Some(slot) = self.slots.get_mut(index) else {
            return Ok(ReplyProgress::Idle);
        };
        let Some(entry) = slot.entry.as_mut() else {
            return Ok(ReplyProgress::Idle);
        };
        if let Poll::Ready(()) = entry.delivery.poll_observed(cx) {
            let progress = if entry.waiting {
                driver.cancel(entry.question)?;
                ReplyProgress::Canceled
            } else if let Some(response) = entry.delivery.take_abandoned() {
                if let Ok(output) = response {
                    driver.release_output(output, messages);
                }
                ReplyProgress::Reclaimed
            } else {
                ReplyProgress::Consumed
            };
            self.retire(index);
            return Ok(progress);
        }
        if !entry.waiting {
            return Ok(ReplyProgress::Pending);
        }
        driver.check_live()?;
        let delivered = match &mut entry.delivery {
            ReplyDelivery::Wire(delivery) => {
                let response = match driver.take_response(entry.question) {
                    Ok(response) => response,
                    Err(Error::PendingResponse | Error::PendingAuthorization) => {
                        return Ok(ReplyProgress::Pending)
                    }
                    Err(error) => return Err(error),
                };
                delivery
                    .try_send(response)
                    .map_err(|r| Ok(CallOutput::Remote(r)))
            }
            ReplyDelivery::Output(delivery) => {
                let output = match driver.take_output(entry.question, messages) {
                    Ok(output) => output,
                    Err(Error::PendingResponse | Error::PendingAuthorization) => {
                        return Ok(ReplyProgress::Pending)
                    }
                    Err(error) => return Err(error),
                };
                delivery.try_send(output)
            }
        };
        entry.waiting = false;
        if let Err(response) = delivered {
            if let Ok(output) = response {
                driver.release_output(output, messages);
            }
            self.retire(index);
            return Ok(ReplyProgress::Reclaimed);
        }
        Ok(ReplyProgress::Delivered)
    }

    /// Cancel one binding before teardown. A response still in its slot is
    /// reclaimed, including when a receiver was forgotten. A response already
    /// taken by the application must be returned separately before termination.
    pub fn cancel<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        index: usize,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<bool, Error> {
        let Some(slot) = self.slots.get_mut(index) else {
            return Ok(false);
        };
        let Some(entry) = slot.entry.as_mut() else {
            return Ok(false);
        };
        if entry.waiting {
            driver.cancel(entry.question)?;
        }
        if let Some(Ok(output)) = entry.delivery.cancel() {
            driver.release_output(output, messages);
        }
        self.retire(index);
        Ok(true)
    }
}

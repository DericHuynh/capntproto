//! Caller-polled streaming acknowledgments with bounded storage and sticky errors.
use super::*;
use core::num::NonZeroUsize;

/// Each occupied slot owns one acknowledgment future. The invocation registry
/// retains all wire receipts until delivery or canceled-consumer cleanup.
pub struct StreamingSlot<'wait, 'pool, 'id, I, const CAPS: usize> {
    reply: Option<PendingAck<'wait, 'pool, 'id, I, CAPS>>,
}
impl<I, const CAPS: usize> StreamingSlot<'_, '_, '_, I, CAPS> {
    pub const EMPTY: Self = Self { reply: None };
}

/// A bounded acknowledgment pump tied to one session. Callers continue polling
/// IO, dispatch and invocation bindings; this owner introduces no background
/// task, clock or allocator. Successful admission is not method completion.
/// `poll_drained` checks every response and releases its receipts before success.
/// The first observed failure prevents new calls while older calls still drain.
#[must_use = "poll until drained, or drop to cancel interest in outstanding acknowledgments"]
pub struct Streaming<'session, 'slots, 'wait, 'pool, 'id, D, M, Q, I, const CAPS: usize> {
    session: &'session Session<D, M, Q>,
    slots: &'slots mut [StreamingSlot<'wait, 'pool, 'id, I, CAPS>],
    pending: usize,
    cursor: usize,
    unscanned: usize,
    failure: Option<CallError>,
}
impl<D, M, Q, I, const CAPS: usize> Streaming<'_, '_, '_, '_, '_, D, M, Q, I, CAPS> {
    pub fn pending(&self) -> usize {
        self.pending
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn failure(&self) -> Option<CallError> {
        self.failure
    }
}
impl<D, M, Q, I, const CAPS: usize> Drop for Streaming<'_, '_, '_, '_, '_, D, M, Q, I, CAPS> {
    fn drop(&mut self) {
        // Cancel only consumer interest. The invocation registry still owns
        // accepted calls and reclaims abandoned values while the driver drains.
        for slot in &mut *self.slots {
            drop(slot.reply.take());
        }
    }
}

impl<
        'pool,
        'id,
        's,
        'm,
        'buffers,
        'bytes,
        'q,
        'wait,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >
    Session<
        Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>,
        &'m mut messages::Messages<'pool, 'buffers, 'bytes>,
        Invocations<'q, 'wait, 'pool, 'id, I, CAPS>,
    >
{
    #[allow(clippy::type_complexity)]
    pub fn streaming<'session, 'slots>(
        &'session self,
        slots: &'slots mut [StreamingSlot<'wait, 'pool, 'id, I, CAPS>],
    ) -> Result<
        Streaming<
            'session,
            'slots,
            'wait,
            'pool,
            'id,
            Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>,
            &'m mut messages::Messages<'pool, 'buffers, 'bytes>,
            Invocations<'q, 'wait, 'pool, 'id, I, CAPS>,
            I,
            CAPS,
        >,
        CallError,
    > {
        if slots.iter().any(|slot| slot.reply.is_some()) {
            return Err(CallError::Connection(Error::NonemptyStorage));
        }
        let unscanned = slots.len();
        Ok(Streaming {
            session: self,
            slots,
            pending: 0,
            cursor: 0,
            unscanned,
            failure: None,
        })
    }
}
impl<
        'pool,
        'id,
        's,
        'm,
        'buffers,
        'bytes,
        'q,
        'wait,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >
    Streaming<
        '_,
        '_,
        'wait,
        'pool,
        'id,
        Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>,
        &'m mut messages::Messages<'pool, 'buffers, 'bytes>,
        Invocations<'q, 'wait, 'pool, 'id, I, CAPS>,
        I,
        CAPS,
    >
{
    /// Enqueue a generated streaming call and retain its acknowledgment. Full
    /// storage or a deferred failure rejects before invoking the parameter filler.
    /// Byte-window policy remains caller supplied, independently of this limit
    /// on outstanding calls. Poll/drain errors even when no further calls are sent.
    pub fn call<IF: Interface, Params: Schema>(
        &mut self,
        completion: &'wait completion::Slot<CallResult<'pool, 'id, I, CAPS>>,
        target: &impl binding::CallTarget<'id, IF>,
        method: StreamingMethod<IF, Params>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<InvocationKey<'id>, CallError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.reply.is_none())
            .ok_or(CallError::Busy)?;
        let reply = self
            .session
            .streaming_call(completion, target, method, capabilities, fill)?;
        let task = reply.task();
        slot.reply = Some(reply);
        self.pending += 1;
        self.unscanned = self.slots.len();
        Ok(task)
    }

    /// Inspect at most `budget` slots. Pending registers on acknowledgments; a
    /// partial scan wakes the caller to finish registering the remaining slots.
    /// The outer error means the session was busy: no acknowledgment was taken.
    /// The inner result is terminal for all currently admitted calls, including
    /// failures. A failed call never makes drain complete while another is pending.
    pub fn poll_drained(
        &mut self,
        cx: &mut Context<'_>,
        budget: NonZeroUsize,
    ) -> Result<Poll<Result<(), CallError>>, CallError> {
        if self.pending == 0 {
            return Ok(Poll::Ready(self.failure.map_or(Ok(()), Err)));
        }
        // Acquire the session before taking any response: even an exceptional
        // response can own capabilities and must be released in the same step.
        let mut state = self
            .session
            .state
            .try_borrow_mut()
            .map_err(|_| CallError::Busy)?;
        let (driver, messages, _) = &mut *state;
        let before = self.pending;
        let count = budget.get().min(self.unscanned);
        for _ in 0..count {
            let index = self.cursor;
            self.cursor = (self.cursor + 1) % self.slots.len();
            let slot = &mut self.slots[index];
            self.unscanned -= 1;
            let Some(reply) = &mut slot.reply else {
                continue;
            };
            let Poll::Ready(result) = Pin::new(reply).poll(cx) else {
                continue;
            };
            let result = match result {
                Ok(ack) => {
                    let result = ack.check(messages);
                    driver.release_output(ack.into_output(), messages);
                    result
                }
                Err(error) => Err(error),
            };
            drop(slot.reply.take());
            self.pending -= 1;
            if let Err(error) = result {
                self.failure.get_or_insert(error);
            }
        }
        // Waking can run user code; no session borrow may remain at this point.
        drop(state);
        if self.pending != before {
            self.session.notify();
        }
        if self.pending == 0 {
            Ok(Poll::Ready(self.failure.map_or(Ok(()), Err)))
        } else {
            if self.unscanned == 0 {
                self.unscanned = self.slots.len();
            } else {
                cx.waker().wake_by_ref();
            }
            Ok(Poll::Pending)
        }
    }
}

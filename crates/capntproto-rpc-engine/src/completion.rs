//! A single reply in caller-owned storage. Endpoints borrow the slot, so safe
//! code cannot recycle it while an accessible endpoint still exists. Tracking a
//! sender also wakes the producer when the receiver consumes or abandons a
//! reply. User values and wakers are dropped/woken only outside state borrows.
//! The supplied waker remains the caller's allocation and scheduling policy.
use core::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
mod state;
use state::{observed, Phase};
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[cfg(feature = "alloc")]
pub mod shared;

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Busy,
    Canceled,
    Consumed,
    Undelivered,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Busy => "reply endpoints are still live",
            Self::Canceled => "reply producer was canceled",
            Self::Consumed => "reply was already consumed",
            Self::Undelivered => "previous reply must be recovered before slot reuse",
        })
    }
}
impl core::error::Error for Error {}
struct State<T> {
    phase: Phase<T>,
    waker: Option<Waker>,
    observed_waker: Option<Waker>,
}
/// The owner chooses stack, static, arena or hosted storage. No allocation is
/// required to create, reuse, complete or cancel this slot.
///
/// ```compile_fail,E0499
/// use capntproto_rpc_engine::completion::Slot;
/// let mut slot = Slot::<u32>::new();
/// let (sender, receiver) = slot.pair().unwrap();
/// let _replacement = slot.pair();
/// sender.send(41).unwrap();
/// drop(receiver);
/// ```
/// ```compile_fail,E0382
/// use capntproto_rpc_engine::completion::Slot;
/// let mut slot = Slot::new();
/// let (sender, _receiver) = slot.pair().unwrap();
/// sender.send(41).unwrap();
/// sender.send(43).unwrap();
/// ```
#[must_use = "recover undelivered values before dropping this slot or end their enclosing ownership scope"]
pub struct Slot<T> {
    state: RefCell<State<T>>,
    endpoints: Cell<u8>,
}
impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Slot<T> {
    pub const fn new() -> Self {
        Self {
            endpoints: Cell::new(0),
            state: RefCell::new(State {
                phase: Phase::Idle,
                waker: None,
                observed_waker: None,
            }),
        }
    }
    pub fn pair(&mut self) -> Result<(Sender<'_, T>, Receiver<'_, T>), Error> {
        // All old endpoints (including a Delivery) must be inaccessible before
        // the exclusive borrow. Recover affine values explicitly; generic Drop
        // cannot return an RPC response's receipts to its connection.
        self.endpoints.set(0);
        self.try_pair()
    }
    /// Claim through a shared storage reference, for caller-owned slot arrays
    /// and service contexts which must stay usable while a reply is awaited.
    /// Accessible endpoints keep this slot busy even after taking its value.
    /// A forgotten endpoint requires exclusive recovery via `pair`; it never
    /// permits a stale receiver or producer to access another call's result.
    pub fn try_pair(&self) -> Result<(Sender<'_, T>, Receiver<'_, T>), Error> {
        if !state::can_claim(self.endpoints.get()) {
            return Err(Error::Busy);
        }
        let previous = {
            let mut state = self.state.borrow_mut();
            if matches!(state.phase, Phase::Value(_) | Phase::Abandoned(_)) {
                return Err(Error::Undelivered);
            }
            self.endpoints.set(2);
            core::mem::replace(
                &mut *state,
                State {
                    phase: Phase::Pending,
                    waker: None,
                    observed_waker: None,
                },
            )
        };
        drop(previous);
        Ok((
            Sender {
                slot: Some(SlotRef::Borrowed(self)),
            },
            Receiver {
                slot: SlotRef::Borrowed(self),
            },
        ))
    }
    /// Recover a reply left by a dropped/forgotten receiver. No endpoint can
    /// remain accessible during this exclusive borrow. Release affine RPC
    /// responses through the driver before reusing the slot or message pool.
    pub fn take_undelivered(&mut self) -> Option<T> {
        self.endpoints.set(0);
        let state = self.state.get_mut();
        if matches!(state.phase, Phase::Value(_) | Phase::Abandoned(_)) {
            let (Phase::Value(value) | Phase::Abandoned(value)) =
                core::mem::replace(&mut state.phase, Phase::Idle)
            else {
                unreachable!()
            };
            Some(value)
        } else {
            None
        }
    }
}
#[must_use = "send the reply or drop to report cancellation"]
pub struct Sender<'a, T> {
    slot: Option<SlotRef<'a, T>>,
}
#[must_use = "poll or drop the receiver to cancel interest in the reply"]
pub struct Receiver<'a, T> {
    slot: SlotRef<'a, T>,
}
// The lifetime bounds the contained RPC owners even for an allocated slot.
// Moving an Rc endpoint into this interface neither extends their lifetimes nor
// creates a second producer/receiver. The fixed profile retains a single reference.
enum SlotRef<'a, T> {
    Borrowed(&'a Slot<T>),
    #[cfg(feature = "alloc")]
    Shared(alloc::rc::Rc<Slot<T>>),
}
impl<T> core::ops::Deref for SlotRef<'_, T> {
    type Target = Slot<T>;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(slot) => slot,
            #[cfg(feature = "alloc")]
            Self::Shared(slot) => slot,
        }
    }
}
impl<'a, T> Sender<'a, T> {
    pub fn is_closed(&self) -> bool {
        self.slot
            .as_ref()
            .is_none_or(|slot| observed(&slot.state.borrow().phase))
    }
    /// Acceptance transfers ownership exactly once. A closed receiver returns
    /// the value intact. A panicking wake cannot undo an accepted result.
    pub fn send(self, value: T) -> Result<(), T> {
        // Drop releases this endpoint even if a caller's wake hook unwinds.
        send(self.slot.as_ref().unwrap(), value)
    }
    /// Retain the unique producer endpoint after delivery to observe receipt or
    /// reclaim an abandoned value. Tracking consumes this sender; there cannot
    /// be competing cleanup observers for one slot.
    pub fn track(mut self) -> Delivery<'a, T> {
        Delivery {
            slot: self.slot.take().unwrap(),
        }
    }
}
#[inline]
fn send<T>(slot: &Slot<T>, value: T) -> Result<(), T> {
    let waker = {
        let mut state = slot.state.borrow_mut();
        state::send(&mut state.phase, value)?;
        state.waker.take()
    };
    wake(waker);
    Ok(())
}
fn wake(waker: Option<Waker>) {
    if let Some(waker) = waker {
        waker.wake()
    }
}
struct ReleaseEndpoint<'a, T>(&'a Slot<T>);
impl<T> Drop for ReleaseEndpoint<'_, T> {
    fn drop(&mut self) {
        self.0.endpoints.set(self.0.endpoints.get() - 1);
    }
}
fn cancel_pending<T>(slot: &Slot<T>, tracked: bool) {
    let _endpoint = ReleaseEndpoint(slot);
    let (waker, observer) = {
        let mut state = slot.state.borrow_mut();
        let waker = if matches!(state.phase, Phase::Pending) {
            state.phase = Phase::Canceled;
            state.waker.take()
        } else {
            None
        };
        (
            waker,
            if tracked {
                state.observed_waker.take()
            } else {
                None
            },
        )
    };
    wake(waker);
    drop(observer);
}
impl<T> Drop for Sender<'_, T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            cancel_pending(&slot, false)
        }
    }
}
/// A unique producer and cleanup observer over fixed or optional shared storage.
/// After publishing, poll until the receiver takes or abandons the reply. A
/// dropped receiver wakes this endpoint even when transport IO remains idle.
/// Explicitly reclaim abandoned values (or cancel during teardown) before
/// dropping a delivery that carries affine RPC resources.
#[must_use = "observe delivery and recover abandoned values, or explicitly cancel"]
pub struct Delivery<'a, T> {
    slot: SlotRef<'a, T>,
}
impl<T> Delivery<'_, T> {
    /// A second send, cancellation or a closed receiver returns the exact value.
    pub fn try_send(&mut self, value: T) -> Result<(), T> {
        send(&self.slot, value)
    }
    pub fn is_observed(&self) -> bool {
        observed(&self.slot.state.borrow().phase)
    }
    /// Register the producer task for consumption/cancellation notification.
    /// Completion of this poll never takes ownership of the reply.
    pub fn poll_observed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let (ready, previous, registered) = {
            let mut state = self.slot.state.borrow_mut();
            let ready = observed(&state.phase);
            let previous = if ready {
                state.observed_waker.take()
            } else {
                None
            };
            let registered = !ready
                && state
                    .observed_waker
                    .as_ref()
                    .is_some_and(|w| w.will_wake(cx.waker()));
            (ready, previous, registered)
        };
        drop(previous);
        if ready {
            return Poll::Ready(());
        }
        if registered {
            return Poll::Pending;
        }
        // Waker cloning can run caller code, including receiver cancellation.
        let mut candidate = Some(cx.waker().clone());
        let (ready, previous) = {
            let mut state = self.slot.state.borrow_mut();
            let ready = observed(&state.phase);
            let previous = if ready {
                state.observed_waker.take()
            } else {
                core::mem::replace(&mut state.observed_waker, candidate.take())
            };
            (ready, previous)
        };
        drop(previous);
        drop(candidate);
        if ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    /// Recover only an abandoned reply. A live receiver's value remains intact.
    pub fn take_abandoned(&mut self) -> Option<T> {
        let mut state = self.slot.state.borrow_mut();
        state::take_abandoned(&mut state.phase)
    }
    /// Shut down delivery and return an unconsumed value for explicit cleanup.
    /// A result already taken by the receiver remains its owner's obligation.
    pub fn cancel(&mut self) -> Option<T> {
        let (value, waker, observer) = {
            let mut state = self.slot.state.borrow_mut();
            let value = state::cancel(&mut state.phase);
            (value, state.waker.take(), state.observed_waker.take())
        };
        drop(observer);
        wake(waker);
        value
    }
}
impl<T> Drop for Delivery<'_, T> {
    fn drop(&mut self) {
        // An accepted value must survive dropping this observer. Slot recovery
        // remains possible after both endpoints have become inaccessible.
        cancel_pending(&self.slot, true);
    }
}
impl<T> Receiver<'_, T> {
    /// Non-registering polling for an application-owned scheduler. A successful
    /// take and a canceled producer are terminal for this receiver.
    pub fn try_take(&mut self) -> Result<Option<T>, Error> {
        let (result, waker, observer) = {
            let mut state = self.slot.state.borrow_mut();
            if matches!(state.phase, Phase::Pending) {
                return Ok(None);
            }
            let result = take(&mut state.phase);
            let (waker, observer) = if result.is_ready() {
                (state.waker.take(), state.observed_waker.take())
            } else {
                (None, None)
            };
            (result, waker, observer)
        };
        drop(waker);
        wake(observer);
        match result {
            Poll::Pending => Ok(None),
            Poll::Ready(value) => value.map(Some),
        }
    }
}
#[inline]
fn take<T>(phase: &mut Phase<T>) -> Poll<Result<T, Error>> {
    match state::take(phase) {
        Some(result) => Poll::Ready(result),
        None => Poll::Pending,
    }
}
impl<T> Future for Receiver<'_, T> {
    type Output = Result<T, Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        poll_receiver(&self.slot, cx)
    }
}
fn poll_receiver<T>(slot: &Slot<T>, cx: &mut Context<'_>) -> Poll<Result<T, Error>> {
    // Inspect the discriminant before constructing a result containing T.
    // RPC replies can be large affine owners; Pending must not shuttle their
    // storage through temporary result tuples on every scheduler scan.
    let pending = {
        let state = slot.state.borrow();
        let pending = matches!(state.phase, Phase::Pending);
        if pending
            && state
                .waker
                .as_ref()
                .is_some_and(|waker| waker.will_wake(cx.waker()))
        {
            return Poll::Pending;
        }
        pending
    };
    if !pending {
        let mut state = slot.state.borrow_mut();
        let result = take(&mut state.phase);
        let previous = state.waker.take();
        let observer = state.observed_waker.take();
        drop(state);
        drop(previous);
        wake(observer);
        return result;
    }
    // Clone may execute user code. Recheck completion afterwards.
    let mut candidate = Some(cx.waker().clone());
    let mut state = slot.state.borrow_mut();
    if matches!(state.phase, Phase::Pending) {
        let previous = core::mem::replace(&mut state.waker, candidate.take());
        drop(state);
        drop(previous);
        return Poll::Pending;
    }
    let result = take(&mut state.phase);
    let previous = state.waker.take();
    let observer = state.observed_waker.take();
    drop(state);
    drop(previous);
    drop(candidate);
    wake(observer);
    result
}
impl<T> Drop for Receiver<'_, T> {
    fn drop(&mut self) {
        drop_receiver(&self.slot);
    }
}
fn drop_receiver<T>(slot: &Slot<T>) {
    let _endpoint = ReleaseEndpoint(slot);
    let (waker, observer) = {
        let mut state = slot.state.borrow_mut();
        state::abandon(&mut state.phase);
        (state.waker.take(), state.observed_waker.take())
    };
    drop(waker);
    wake(observer);
}
#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;

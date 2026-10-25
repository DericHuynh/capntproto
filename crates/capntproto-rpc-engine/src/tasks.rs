//! Wake-driven task polling shared with legacy. Futures and wakers stay in
//! their owner's storage; notifications contain only reusable slot indices.
use crate::readiness::Queue;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// Static wake identity containing no task or executor-owner reference.
#[derive(Clone, Copy)]
pub struct Notification<const N: usize> {
    ready: &'static crate::readiness::Storage<N>,
    index: usize,
    notify: fn(),
}
impl<const N: usize> capntproto_task::Wake for Notification<N> {
    fn wake_by_ref(&self) {
        self.ready.queue().enqueue(self.index, self.notify);
    }
}
pub type Signal<const N: usize> = capntproto_task::Static<Notification<N>>;

/// Create static identities without allocation. Retain the readiness cursor
/// across task-set replacement; old wakes can only schedule empty/reused slots.
pub const fn signals<const N: usize>(
    ready: &'static crate::readiness::Storage<N>,
    notify: fn(),
) -> [Signal<N>; N] {
    assert!(N.is_power_of_two() && N <= 128);
    let mut result = [Signal::new(Notification {
        ready,
        index: 0,
        notify,
    }); N];
    let mut index = 0;
    while index < N {
        result[index] = Signal::new(Notification {
            ready,
            index,
            notify,
        });
        index += 1;
    }
    result
}

/// Projection of an owner's existing future slots and stable wake identities.
/// A slot's waker must enqueue its index into the associated readiness queue.
pub trait Slots {
    type Future: Future;
    fn get(&mut self, index: usize) -> Option<(Pin<&mut Self::Future>, &Waker)>;
    fn retire(&mut self, index: usize);
}

/// Poll at most one rotation of ready work. Finished values leave their slot
/// before destruction; stale notifications only cause a replacement to be
/// polled. The owner arms its parent notification before entering this method.
/// The counters and cursor refer to the same slot set as `ready` and `slots`.
#[inline(always)]
pub fn poll_next<S: Slots, const N: usize>(
    slots: &mut S,
    live: &mut usize,
    available: &mut u128,
    cursor: &mut usize,
    ready: Queue<'_, N>,
    cx: &mut Context<'_>,
) -> Poll<Option<<S::Future as Future>::Output>> {
    if *live == 0 {
        return Poll::Ready(None);
    }
    let mut visited = 0u128;
    while let Some(index) = ready.front(*cursor) {
        let bit = 1u128 << index;
        if visited & bit != 0 {
            cx.waker().wake_by_ref();
            break;
        }
        ready.consume(*cursor, index);
        *cursor = cursor.wrapping_add(1);
        visited |= bit;
        let Some((mut future, wake)) = slots.get(index) else {
            continue;
        };
        let mut task_cx = Context::from_waker(wake);
        if let Poll::Ready(result) = future.as_mut().poll(&mut task_cx) {
            *live -= 1;
            *available |= bit;
            slots.retire(index);
            return Poll::Ready(Some(result));
        }
    }
    Poll::Pending
}

struct ArraySlots<'a, F, const N: usize> {
    futures: Pin<&'a mut PinnedSlots<F, N>>,
    wakers: &'a [Waker; N],
}
impl<F: Future, const N: usize> Slots for ArraySlots<'_, F, N> {
    type Future = F;
    #[inline(always)]
    fn get(&mut self, index: usize) -> Option<(Pin<&mut F>, &Waker)> {
        self.futures
            .as_mut()
            .get(index)
            .map(|future| (future, &self.wakers[index]))
    }
    #[inline(always)]
    fn retire(&mut self, index: usize) {
        self.futures.as_mut().clear(index);
    }
}

pub use capntproto_task::Slots as PinnedSlots;

/// Fixed task storage using the same poll loop as hosted legacy. The caller
/// pins the complete slot array. Futures, including !Unpin futures borrowing
/// application data, are owned and dropped in place as soon as they finish.
///
/// Wakers must enqueue their matching slot and arrange parent progress. They
/// retain the caller's ownership policy, and may outlive this set only if their
/// notification backing remains valid (for example static wake storage).
/// Keep `cursor` with that backing across task-set replacement. Old wakes are
/// then harmless extra polls of empty/reused slots rather than completions.
pub struct Set<'a, F: Future, const N: usize> {
    futures: Pin<&'a mut PinnedSlots<F, N>>,
    wakers: &'a [Waker; N],
    ready: Queue<'a, N>,
    cursor: &'a mut usize,
    live: usize,
    available: u128,
}
impl<'a, F: Future, const N: usize> Set<'a, F, N> {
    pub fn new(
        futures: Pin<&'a mut PinnedSlots<F, N>>,
        ready: Queue<'a, N>,
        wakers: &'a [Waker; N],
        cursor: &'a mut usize,
    ) -> Self {
        assert!(N.is_power_of_two() && N <= 128);
        assert!(futures.is_empty(), "task storage is not empty");
        Self {
            futures,
            wakers,
            ready,
            cursor,
            live: 0,
            available: u128::MAX >> (128 - N),
        }
    }
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    /// Admit ownership once, or return the untouched future when full. Notify
    /// after commit even if an old wake already occupied the reused slot.
    pub fn try_push(&mut self, future: F, notify: impl FnOnce()) -> Result<usize, F> {
        if self.live == N {
            return Err(future);
        }
        let index = self.available.trailing_zeros() as usize;
        self.futures.as_mut().insert(index, future)?;
        self.available &= !(1u128 << index);
        self.live += 1;
        self.ready.enqueue(index, || {});
        notify();
        Ok(index)
    }
    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<F::Output>> {
        poll_next(
            &mut ArraySlots {
                futures: self.futures.as_mut(),
                wakers: self.wakers,
            },
            &mut self.live,
            &mut self.available,
            self.cursor,
            self.ready,
            cx,
        )
    }
}

impl<F: Future, const N: usize> Drop for Set<'_, F, N> {
    fn drop(&mut self) {
        self.futures.as_mut().clear_all();
    }
}

#[cfg(test)]
mod tests;

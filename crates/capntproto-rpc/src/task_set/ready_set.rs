//! Task sets reuse wake nodes with a bounded multi-producer atomic FIFO.
//! Exceptional concurrency retains the FuturesUnordered fallback.
use futures::{stream::FuturesUnordered, Stream};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use super::ready_queue::{ReadyQueue as Ready, CAPACITY as REUSABLE_TASKS};

struct Notify {
    index: usize,
    ready: Arc<Ready>,
}
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.enqueue(self.index);
    }
}
struct Slot<F> {
    // The notification belongs to the slot, not to one submitted future.
    // A stale wake can only schedule a harmless extra poll of its replacement.
    future: Option<F>,
    waker: Waker,
}
pub(crate) struct ReadySet<F: Future + Unpin> {
    ready: Arc<Ready>,
    tasks: Vec<Slot<F>>,
    queued: Option<FuturesUnordered<F>>,
    live: usize,
    next_ready: usize,
    available: u128,
}
impl<F: Future + Unpin> ReadySet<F> {
    pub(crate) fn new() -> Self {
        Self {
            ready: Arc::new(Ready::new()),
            tasks: Vec::new(),
            queued: None,
            live: 0,
            next_ready: 0,
            available: u128::MAX,
        }
    }
    pub(crate) fn len(&self) -> usize {
        self.queued
            .as_ref()
            .map_or(self.live, FuturesUnordered::len)
    }
    /// Reserve reusable slot storage before consuming a fallible hosted owner.
    /// Exceptional concurrency keeps the same FuturesUnordered allocation path.
    pub(crate) fn try_push(
        &mut self,
        future: F,
    ) -> Result<(), (std::collections::TryReserveError, F)> {
        if self.queued.is_none()
            && self.live < REUSABLE_TASKS
            && self.available.trailing_zeros() as usize == self.tasks.len()
        {
            if let Err(error) = self.tasks.try_reserve(1) {
                return Err((error, future));
            }
        }
        self.push(future);
        Ok(())
    }
    pub(crate) fn push(&mut self, future: F) {
        if let Some(queued) = &self.queued {
            queued.push(future);
            return;
        }
        if self.live == REUSABLE_TASKS {
            let queued = FuturesUnordered::new();
            for task in std::mem::take(&mut self.tasks) {
                if let Some(future) = task.future {
                    queued.push(future);
                }
            }
            queued.push(future);
            self.queued = Some(queued);
            self.live = 0;
            // Each migrated future is queued for a first poll, which replaces
            // any old readiness registration without losing a pre-poll wake.
            self.ready.parent.wake();
            drop(self.ready.parent.take());
            return;
        }
        // Reuse a vacant slot without scanning every suspended future.
        let index = self.available.trailing_zeros() as usize;
        self.available &= !(1u128 << index);
        if index == self.tasks.len() {
            self.tasks.push(Slot {
                future: Some(future),
                waker: Waker::from(Arc::new(Notify {
                    index,
                    ready: self.ready.clone(),
                })),
            });
        } else {
            self.tasks[index].future = Some(future);
        }
        self.live += 1;
        self.ready.enqueue(index);
    }
}
impl<F: Future + Unpin> Stream for ReadySet<F> {
    type Item = F::Output;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(queued) = self.queued.as_mut() {
            return Pin::new(queued).poll_next(cx);
        }
        self.ready.parent.register(cx.waker());
        if self.live == 0 {
            return Poll::Ready(None);
        }
        // A single bounded rotation prevents self-waking or mutually waking
        // tasks from monopolizing the executor. Their notifications schedule
        // another rotation, including a wake during the final task's poll.
        let mut visited = 0u128;
        // An earlier producer can still be publishing the front position.
        // It wakes the registered parent after publication; later notifications
        // cannot pass it and change FIFO ordering.
        while let Some(index) = self.ready.front(self.next_ready) {
            let bit = 1u128 << index;
            if visited & bit != 0 {
                cx.waker().wake_by_ref();
                break;
            }
            self.ready.consume(self.next_ready, index);
            self.next_ready = self.next_ready.wrapping_add(1);
            visited |= bit;
            let slot = &mut self.tasks[index];
            let Some(future) = slot.future.as_mut() else {
                continue;
            };
            let mut task_cx = Context::from_waker(&slot.waker);
            if let Poll::Ready(result) = Pin::new(future).poll(&mut task_cx) {
                let finished = slot.future.take();
                self.live -= 1;
                self.available |= bit;
                drop(finished);
                return Poll::Ready(Some(result));
            }
        }
        Poll::Pending
    }
}
impl<F: Future + Unpin> Drop for ReadySet<F> {
    fn drop(&mut self) {
        // External IO can retain a child waker after teardown. That waker must
        // neither retain the parent executor nor any of the task futures.
        drop(self.ready.parent.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::atomic::{AtomicUsize, Ordering},
    };
    #[derive(Default)]
    struct Counts(AtomicUsize);
    impl Wake for Counts {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Wait {
        value: usize,
        ready: Rc<Cell<bool>>,
        wake: Rc<RefCell<Option<Waker>>>,
        polls: Rc<Cell<usize>>,
    }
    impl Future for Wait {
        type Output = usize;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<usize> {
            self.polls.set(self.polls.get() + 1);
            *self.wake.borrow_mut() = Some(cx.waker().clone());
            if self.ready.get() {
                Poll::Ready(self.value)
            } else {
                Poll::Pending
            }
        }
    }
    #[test]
    fn readiness_survives_parent_changes_promotion_and_dropped_set() {
        for count in [1, 8, 32, REUSABLE_TASKS, REUSABLE_TASKS + 1, 256] {
            let mut set = ReadySet::new();
            let mut tasks = Vec::new();
            let parent = Arc::new(Counts::default());
            let wake = Waker::from(parent.clone());
            let mut cx = Context::from_waker(&wake);
            for value in 0..count {
                let ready = Rc::new(Cell::new(false));
                let polls = Rc::new(Cell::new(0));
                let saved = Rc::new(RefCell::new(None));
                set.push(Wait {
                    value,
                    ready: ready.clone(),
                    wake: saved.clone(),
                    polls: polls.clone(),
                });
                tasks.push((ready, saved, polls));
                assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending());
            }
            let replacement = Arc::new(Counts::default());
            let replacement_wake = Waker::from(replacement.clone());
            let mut cx = Context::from_waker(&replacement_wake);
            assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending());
            let before: Vec<_> = tasks.iter().map(|(_, _, polls)| polls.get()).collect();
            assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending());
            assert_eq!(
                before,
                tasks
                    .iter()
                    .map(|(_, _, polls)| polls.get())
                    .collect::<Vec<_>>()
            );
            for index in (0..count).rev() {
                tasks[index].0.set(true);
                let wake = tasks[index].1.borrow().as_ref().unwrap().clone();
                std::thread::spawn(move || wake.wake()).join().unwrap();
                assert_eq!(
                    Pin::new(&mut set).poll_next(&mut cx),
                    Poll::Ready(Some(index))
                );
                assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending() || set.len() == 0);
            }
            assert!(replacement.0.load(Ordering::SeqCst) > 0);
            assert_eq!(set.len(), 0);
            drop(set);
            drop(replacement_wake);
            assert_eq!(Arc::strong_count(&replacement), 1);
            // Waking old observers after destruction is harmless.
            for (_, saved, _) in tasks {
                if let Some(wake) = saved.borrow_mut().take() {
                    wake.wake();
                }
            }
        }
    }
    #[test]
    fn readiness_is_fifo_even_when_slot_order_differs() {
        let mut set = ReadySet::new();
        let mut waits = Vec::new();
        let mut cx = Context::from_waker(Waker::noop());
        for value in 0..REUSABLE_TASKS {
            let ready = Rc::new(Cell::new(false));
            let wake = Rc::new(RefCell::new(None));
            set.push(Wait {
                value,
                ready: ready.clone(),
                wake: wake.clone(),
                polls: Rc::new(Cell::new(0)),
            });
            waits.push((ready, wake));
        }
        assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending());
        let order: Vec<_> = (0..REUSABLE_TASKS)
            .map(|i| (i * 37 + 6) % REUSABLE_TASKS)
            .collect();
        for &index in &order {
            waits[index].0.set(true);
            let wake = waits[index].1.borrow().as_ref().unwrap().clone();
            wake.wake_by_ref();
            wake.wake_by_ref(); // Duplicate notifications keep their original position.
        }
        for &index in &order {
            assert_eq!(
                Pin::new(&mut set).poll_next(&mut cx),
                Poll::Ready(Some(index))
            );
        }
        assert_eq!(Pin::new(&mut set).poll_next(&mut cx), Poll::Ready(None));
    }

    #[test]
    fn self_wake_is_bounded_and_preserved() {
        let polls = Rc::new(Cell::new(0));
        let calls = polls.clone();
        let mut set = ReadySet::new();
        set.push(std::future::poll_fn(move |cx| {
            let n = calls.get() + 1;
            calls.set(n);
            cx.waker().wake_by_ref();
            if n == 3 {
                Poll::Ready(n)
            } else {
                Poll::Pending
            }
        }));
        let wake = Waker::from(Arc::new(Counts::default()));
        let mut cx = Context::from_waker(&wake);
        for n in 1..=3 {
            let result = Pin::new(&mut set).poll_next(&mut cx);
            assert_eq!(polls.get(), n);
            assert_eq!(
                result,
                if n == 3 {
                    Poll::Ready(Some(3))
                } else {
                    Poll::Pending
                }
            );
        }
    }
    #[test]
    fn completed_slots_reuse_notifications_and_drop_futures_promptly() {
        let mut set = ReadySet::new();
        let mut cx = Context::from_waker(Waker::noop());
        let saved = Rc::new(RefCell::new(None));
        let polls = Rc::new(Cell::new(0));
        let mut previous: Option<Waker> = None;
        for value in 0..64 {
            let ready = Rc::new(Cell::new(true));
            let weak = Rc::downgrade(&ready);
            set.push(Wait {
                value,
                ready,
                wake: saved.clone(),
                polls: polls.clone(),
            });
            // Notification identity survives every replacement, even while an
            // external observer retains the preceding task's waker.
            if let Some(old) = &previous {
                old.wake_by_ref();
            }
            assert_eq!(
                Pin::new(&mut set).poll_next(&mut cx),
                Poll::Ready(Some(value))
            );
            assert!(weak.upgrade().is_none());
            let current = saved.borrow().as_ref().unwrap().clone();
            if let Some(old) = previous {
                assert!(old.will_wake(&current));
            }
            previous = Some(current);
        }
        assert_eq!(polls.get(), 64);
    }

    #[test]
    fn stale_notification_cannot_complete_or_lose_the_reused_slot() {
        let mut set = ReadySet::new();
        let mut cx = Context::from_waker(Waker::noop());
        let old_wake = Rc::new(RefCell::new(None));
        set.push(Wait {
            value: 1,
            ready: Rc::new(Cell::new(true)),
            wake: old_wake.clone(),
            polls: Rc::new(Cell::new(0)),
        });
        assert_eq!(Pin::new(&mut set).poll_next(&mut cx), Poll::Ready(Some(1)));
        let ready = Rc::new(Cell::new(false));
        let wake = Rc::new(RefCell::new(None));
        set.push(Wait {
            value: 2,
            ready: ready.clone(),
            wake: wake.clone(),
            polls: Rc::new(Cell::new(0)),
        });
        assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending());
        old_wake.borrow().as_ref().unwrap().wake_by_ref();
        assert!(Pin::new(&mut set).poll_next(&mut cx).is_pending());
        ready.set(true);
        wake.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(Pin::new(&mut set).poll_next(&mut cx), Poll::Ready(Some(2)));
        old_wake.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(Pin::new(&mut set).poll_next(&mut cx), Poll::Ready(None));
    }
}

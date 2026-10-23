//! Small task sets reuse wake nodes with a bounded atomic FIFO. Larger sets retain the
//! wake-driven FuturesUnordered queue; every task keeps its original future.
use futures::{stream::FuturesUnordered, task::AtomicWaker, Stream};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

const INLINE_TASKS: usize = 8;

struct Ready {
    // Eight four-bit slot IDs (index + 1), oldest in the low nibble. A single
    // atomic update both coalesces notifications and preserves their FIFO order.
    queue: AtomicU32,
    parent: AtomicWaker,
}
impl Ready {
    fn enqueue(&self, index: usize) {
        let mut old = self.queue.load(Ordering::Acquire);
        loop {
            let mut remaining = old;
            let mut shift = 0;
            let new = loop {
                if remaining == 0 {
                    // A missing slot leaves at most seven distinct IDs queued.
                    break old | ((index as u32 + 1) << shift);
                }
                if remaining & 15 == index as u32 + 1 {
                    // Even a duplicate must publish the producer's data to
                    // the poll that consumes this notification. A load-only
                    // return can race with dequeue and lose that publication.
                    break old;
                }
                remaining >>= 4;
                shift += 4;
            };
            match self
                .queue
                .compare_exchange_weak(old, new, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    if old == 0 {
                        self.parent.wake();
                    }
                    return;
                }
                Err(current) => old = current,
            }
        }
    }
}
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
pub(super) struct ReadySet<F: Future + Unpin> {
    ready: Arc<Ready>,
    tasks: Vec<Slot<F>>,
    queued: Option<FuturesUnordered<F>>,
    live: usize,
}
impl<F: Future + Unpin> ReadySet<F> {
    pub(super) fn new() -> Self {
        Self {
            ready: Arc::new(Ready {
                queue: AtomicU32::new(0),
                parent: AtomicWaker::new(),
            }),
            tasks: Vec::new(),
            queued: None,
            live: 0,
        }
    }
    pub(super) fn len(&self) -> usize {
        self.queued
            .as_ref()
            .map_or(self.live, FuturesUnordered::len)
    }
    pub(super) fn push(&mut self, future: F) {
        if let Some(queued) = &self.queued {
            queued.push(future);
            return;
        }
        if self.live == INLINE_TASKS {
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
        let index = self
            .tasks
            .iter()
            .position(|slot| slot.future.is_none())
            .unwrap_or(self.tasks.len());
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
        let mut visited = 0u8;
        loop {
            let ready = self.ready.queue.load(Ordering::Acquire);
            if ready == 0 {
                break;
            }
            let index = (ready & 15) as usize - 1;
            let bit = 1 << index;
            if visited & bit != 0 {
                // Preserve the queue order while bounding a self-waking task.
                cx.waker().wake_by_ref();
                break;
            }
            if self
                .ready
                .queue
                .compare_exchange_weak(ready, ready >> 4, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            visited |= bit;
            let slot = &mut self.tasks[index];
            let Some(future) = slot.future.as_mut() else {
                continue;
            };
            let mut task_cx = Context::from_waker(&slot.waker);
            if let Poll::Ready(result) = Pin::new(future).poll(&mut task_cx) {
                let finished = slot.future.take();
                self.live -= 1;
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
        sync::atomic::AtomicUsize,
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
        for count in [1, INLINE_TASKS, INLINE_TASKS + 1, 32] {
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
        for value in 0..INLINE_TASKS {
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
        let order = [6, 2, 7, 1, 5, 0, 4, 3];
        for index in order {
            waits[index].0.set(true);
            let wake = waits[index].1.borrow().as_ref().unwrap().clone();
            wake.wake_by_ref();
            wake.wake_by_ref(); // Duplicate notifications keep their original position.
        }
        for index in order {
            assert_eq!(
                Pin::new(&mut set).poll_next(&mut cx),
                Poll::Ready(Some(index))
            );
        }
        assert_eq!(Pin::new(&mut set).poll_next(&mut cx), Poll::Ready(None));
    }

    #[test]
    fn concurrent_notifications_coalesce_without_losing_slots() {
        let ready = Arc::new(Ready {
            queue: AtomicU32::new(0),
            parent: AtomicWaker::new(),
        });
        std::thread::scope(|scope| {
            for index in 0..INLINE_TASKS {
                let ready = ready.clone();
                scope.spawn(move || {
                    for _ in 0..100 {
                        ready.enqueue(index);
                    }
                });
            }
        });
        let mut packed = ready.queue.load(Ordering::Acquire);
        let mut seen = 0u8;
        for _ in 0..INLINE_TASKS {
            let index = (packed & 15) - 1;
            assert!(index < INLINE_TASKS as u32);
            assert_eq!(seen & (1 << index), 0);
            seen |= 1 << index;
            packed >>= 4;
        }
        assert_eq!(packed, 0);
        assert_eq!(seen, u8::MAX);
    }

    #[test]
    fn duplicate_notifications_publish_before_the_queued_poll() {
        use std::sync::{atomic::AtomicBool, Barrier};
        let ready = Ready {
            queue: AtomicU32::new(0),
            parent: AtomicWaker::new(),
        };
        let data = AtomicBool::new(false);
        let start = Barrier::new(2);
        let done = Barrier::new(2);
        let rounds = if cfg!(miri) { 8 } else { 100_000 };
        let mut lost = None;
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..rounds {
                    start.wait();
                    data.store(true, Ordering::Release);
                    ready.enqueue(0);
                    done.wait();
                }
            });
            for round in 0..rounds {
                data.store(false, Ordering::Relaxed);
                ready.queue.store(1, Ordering::Relaxed);
                start.wait();
                // Consume an already queued notification while another thread
                // publishes data and wakes the same task. Either this poll
                // observes the data, or another notification must remain.
                let mut old = ready.queue.load(Ordering::Acquire);
                loop {
                    match ready.queue.compare_exchange_weak(
                        old,
                        old >> 4,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(current) => old = current,
                    }
                }
                let observed = data.load(Ordering::Acquire);
                done.wait();
                if !observed && ready.queue.load(Ordering::Acquire) == 0 {
                    lost.get_or_insert(round);
                }
            }
        });
        assert_eq!(lost, None, "duplicate wake lost its data publication");
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

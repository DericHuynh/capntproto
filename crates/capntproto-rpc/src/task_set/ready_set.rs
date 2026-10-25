//! Task sets reuse wake nodes with a bounded multi-producer atomic FIFO.
//! Exceptional concurrency retains the FuturesUnordered fallback.
use capntproto_rpc_engine::readiness::Queue as Notifications;
use capntproto_rpc_engine::tasks;
use futures::{stream::FuturesUnordered, task::AtomicWaker, Stream};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

// Notification storage is bounded independently of task lifetimes. A larger
// exceptional set uses FuturesUnordered rather than retaining an unbounded pool.
const REUSABLE_TASKS: usize = 128;

struct Ready {
    next: AtomicUsize,
    entries: [AtomicU8; REUSABLE_TASKS],
    queued: [AtomicBool; REUSABLE_TASKS],
    parent: AtomicWaker,
}
impl Ready {
    fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            entries: [const { AtomicU8::new(0) }; REUSABLE_TASKS],
            queued: [const { AtomicBool::new(false) }; REUSABLE_TASKS],
            parent: AtomicWaker::new(),
        }
    }
    fn enqueue(&self, index: usize) {
        Notifications::new(&self.next, &self.entries, &self.queued)
            .enqueue(index, || self.parent.wake());
    }
    #[cfg(test)]
    fn front(&self, position: usize) -> Option<usize> {
        Notifications::new(&self.next, &self.entries, &self.queued).front(position)
    }
    #[cfg(test)]
    fn consume(&self, position: usize, index: usize) {
        Notifications::new(&self.next, &self.entries, &self.queued).consume(position, index);
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
struct TaskSlots<'a, F>(&'a mut [Slot<F>]);
impl<F: Future + Unpin> tasks::Slots for TaskSlots<'_, F> {
    type Future = F;
    #[inline(always)]
    fn get(&mut self, index: usize) -> Option<(Pin<&mut F>, &Waker)> {
        let slot = &mut self.0[index];
        slot.future
            .as_mut()
            .map(|future| (Pin::new(future), &slot.waker))
    }
    #[inline(always)]
    fn retire(&mut self, index: usize) {
        drop(self.0[index].future.take());
    }
}
pub(super) struct ReadySet<F: Future + Unpin> {
    ready: Arc<Ready>,
    tasks: Vec<Slot<F>>,
    queued: Option<FuturesUnordered<F>>,
    live: usize,
    next_ready: usize,
    available: u128,
}
impl<F: Future + Unpin> ReadySet<F> {
    pub(super) fn new() -> Self {
        Self {
            ready: Arc::new(Ready::new()),
            tasks: Vec::new(),
            queued: None,
            live: 0,
            next_ready: 0,
            available: u128::MAX,
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
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some(queued) = this.queued.as_mut() {
            return Pin::new(queued).poll_next(cx);
        }
        this.ready.parent.register(cx.waker());
        tasks::poll_next(
            &mut TaskSlots(&mut this.tasks),
            &mut this.live,
            &mut this.available,
            &mut this.next_ready,
            Notifications::new(&this.ready.next, &this.ready.entries, &this.ready.queued),
            cx,
        )
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
    fn concurrent_notifications_coalesce_without_losing_slots() {
        let ready = Arc::new(Ready::new());
        std::thread::scope(|scope| {
            for worker in 0..8 {
                let ready = ready.clone();
                scope.spawn(move || {
                    for _ in 0..100 {
                        for index in (worker..REUSABLE_TASKS).step_by(8) {
                            ready.enqueue(index);
                        }
                    }
                });
            }
        });
        let mut seen = 0u128;
        for position in 0..REUSABLE_TASKS {
            let index = ready.front(position).unwrap();
            assert_eq!(seen & (1 << index), 0);
            seen |= 1 << index;
            ready.consume(position, index);
        }
        assert_eq!(ready.front(REUSABLE_TASKS), None);
        assert_eq!(seen, u128::MAX);
    }

    #[test]
    fn unpublished_reservation_preserves_fifo_and_wrapping_cursors() {
        let ready = Ready::new();
        let start = usize::MAX - 1;
        ready.next.store(start, Ordering::Relaxed);
        // Suspend a producer after reservation but before publication.
        assert!(!ready.queued[3].swap(true, Ordering::AcqRel));
        assert_eq!(ready.next.fetch_add(1, Ordering::Relaxed), start);
        ready.enqueue(7);
        ready.enqueue(11);
        assert_eq!(ready.front(start), None);
        ready.entries[start % REUSABLE_TASKS].store(4, Ordering::Release);
        for (offset, index) in [3, 7, 11].into_iter().enumerate() {
            let position = start.wrapping_add(offset);
            assert_eq!(ready.front(position), Some(index));
            ready.consume(position, index);
        }
        assert_eq!(ready.front(start.wrapping_add(3)), None);
    }

    #[test]
    fn concurrent_producers_and_consumer_reuse_every_slot() {
        let ready = Ready::new();
        let values: [AtomicUsize; REUSABLE_TASKS] = [const { AtomicUsize::new(0) }; REUSABLE_TASKS];
        let finished = AtomicUsize::new(0);
        let mut observed = [0; REUSABLE_TASKS];
        let rounds = if cfg!(miri) { 2 } else { 256 };
        std::thread::scope(|scope| {
            for worker in 0..8 {
                let (ready, values, finished) = (&ready, &values, &finished);
                scope.spawn(move || {
                    for value in 1..=rounds {
                        for index in (worker..REUSABLE_TASKS).step_by(8) {
                            values[index].store(value, Ordering::Release);
                            ready.enqueue(index);
                        }
                    }
                    finished.fetch_add(1, Ordering::Release);
                });
            }
            let mut position = 0usize;
            loop {
                if let Some(index) = ready.front(position) {
                    ready.consume(position, index);
                    position = position.wrapping_add(1);
                    observed[index] = values[index].load(Ordering::Acquire);
                } else if finished.load(Ordering::Acquire) == 8 {
                    // A producer could publish between the first front load
                    // and the completion load. Recheck after acquiring it.
                    if ready.front(position).is_none() {
                        break;
                    }
                } else {
                    std::thread::yield_now();
                }
            }
        });
        assert_eq!(observed, [rounds; REUSABLE_TASKS]);
        assert!(ready.queued.iter().all(|q| !q.load(Ordering::Acquire)));
    }

    #[test]
    fn duplicate_notifications_publish_before_the_queued_poll() {
        use std::sync::{atomic::AtomicBool, Barrier};
        let ready = Ready::new();
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
            let mut position = 0usize;
            for round in 0..rounds {
                if let Some(index) = ready.front(position) {
                    ready.consume(position, index);
                    position = position.wrapping_add(1);
                }
                data.store(false, Ordering::Relaxed);
                ready.enqueue(0);
                start.wait();
                // Consume an already queued notification while another thread
                // publishes data and wakes the same task. Either this poll
                // observes the data, or another notification must remain.
                assert_eq!(ready.front(position), Some(0));
                ready.consume(position, 0);
                position = position.wrapping_add(1);
                let observed = data.load(Ordering::Acquire);
                done.wait();
                if !observed && ready.front(position).is_none() {
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

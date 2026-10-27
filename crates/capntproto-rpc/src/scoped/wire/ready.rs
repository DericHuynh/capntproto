//! Bounded FIFO of operation identities. Wakers carry no driver references.
//! Slot generations reject notifications retained by an earlier operation.
use crate::task_set::ready_queue::{ReadyQueue, CAPACITY};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Wake, Waker};

/// A single parked admission. Each new owner receives a fresh signal, so an
/// escaped notification cannot reactivate a later request in the same lane.
#[derive(Default)]
pub(crate) struct Signal {
    ready: std::sync::atomic::AtomicBool,
    parent: futures::task::AtomicWaker,
}
impl Signal {
    pub(crate) fn register(&self, wake: &Waker) {
        self.parent.register(wake);
    }
    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn clear(&self) {
        self.ready
            .store(false, std::sync::atomic::Ordering::Release);
    }
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.ready.swap(true, std::sync::atomic::Ordering::AcqRel) {
            self.parent.wake();
        }
    }
}

#[derive(Default)]
struct Slot {
    generation: u64,
    active: bool,
    queued: bool,
    previous: Option<usize>,
    next: Option<usize>,
    notify: Option<Arc<Notify>>,
}
#[derive(Default)]
struct State {
    slots: Vec<Slot>,
    head: Option<usize>,
    tail: Option<usize>,
    closed: bool,
}
impl State {
    fn push(&mut self, index: usize) -> bool {
        let slot = &mut self.slots[index];
        if self.closed || !slot.active || slot.queued {
            return false;
        }
        slot.queued = true;
        slot.notify
            .as_ref()
            .unwrap()
            .state
            .fetch_or(RUNNABLE, Ordering::AcqRel);
        slot.previous = self.tail;
        slot.next = None;
        if let Some(tail) = self.tail {
            self.slots[tail].next = Some(index);
        } else {
            self.head = Some(index);
        }
        self.tail = Some(index);
        true
    }
    fn unlink(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        if !slot.queued {
            return;
        }
        slot.queued = false;
        let previous = slot.previous.take();
        let next = slot.next.take();
        if let Some(previous) = previous {
            self.slots[previous].next = next;
        } else {
            self.head = next;
        }
        if let Some(next) = next {
            self.slots[next].previous = previous;
        } else {
            self.tail = previous;
        }
    }
}
// Ordinary asynchronous notifications share legacy's atomic FIFO. Larger
// operation sets retain the original mailbox. The local driver owns exact
// generations and runnable membership in either representation.
struct Shared {
    fast: ReadyQueue,
    overflow: AtomicBool,
    pending: Mutex<VecDeque<Arc<Notify>>>,
    notified: AtomicBool,
    closed: AtomicBool,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            fast: ReadyQueue::new(),
            overflow: AtomicBool::new(false),
            pending: Mutex::default(),
            notified: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }
}
// Every wake linearizes against scheduling, poll entry/exit and removal on
// this one atomic. Independent flags can miss a concurrent queue-to-poll handoff.
const RUNNABLE: u8 = 1;
const POLLING: u8 = 2;
const WOKEN: u8 = 4;
const INACTIVE: u8 = 8;

struct Notify {
    shared: Weak<Shared>,
    index: usize,
    generation: u64,
    queued: AtomicBool,
    state: AtomicU8,
}
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        // A runnable owner will poll; a polling owner publishes a wake only
        // if it suspends. WOKEN coalesces repeated asynchronous notifications.
        let previous = self.state.fetch_or(WOKEN, Ordering::AcqRel);
        if previous & (RUNNABLE | POLLING | WOKEN | INACTIVE) == 0 {
            self.enqueue();
        }
    }
}
impl Notify {
    fn enqueue(self: &Arc<Self>) {
        if self.queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if shared.closed.load(Ordering::Acquire)
            || self.state.load(Ordering::Acquire) & INACTIVE != 0
        {
            return;
        }
        if !shared.overflow.load(Ordering::Acquire) {
            // The slot, rather than its generation, owns one reservation.
            // Its current Notify retains the generation-specific WOKEN bit;
            // a stale reservation cannot make a replacement runnable.
            shared.fast.enqueue(self.index);
            return;
        }
        let notify = {
            let mut pending = shared.pending.lock().unwrap();
            if shared.closed.load(Ordering::Acquire)
                || self.state.load(Ordering::Acquire) & INACTIVE != 0
            {
                return;
            }
            pending.push_back(self.clone());
            !shared.notified.swap(true, Ordering::AcqRel)
        };
        // Reentrant executors run after releasing the mailbox, and one wake
        // covers all notifications until the driver consumes that batch.
        if notify {
            shared.fast.parent.wake();
        }
    }
}
#[derive(Default)]
pub(crate) struct Queue {
    state: RefCell<State>,
    shared: Arc<Shared>,
    next_fast: Cell<usize>,
}
/// Polling is itself ownership of readiness. Synchronous completion discards
/// self-wakes; a suspended operation republishes them when its poll ends.
pub(crate) struct Ready(Arc<Notify>);
impl Ready {
    pub(crate) fn index(&self) -> usize {
        self.0.index
    }
}
impl Drop for Ready {
    fn drop(&mut self) {
        let previous = self.0.state.fetch_and(!POLLING, Ordering::AcqRel);
        if previous & (WOKEN | RUNNABLE | INACTIVE) == WOKEN {
            self.0.enqueue();
        }
    }
}
impl Queue {
    fn drain(&self) {
        {
            let mut state = self.state.borrow_mut();
            while let Some(index) = self.shared.fast.front(self.next_fast.get()) {
                self.shared.fast.consume(self.next_fast.get(), index);
                self.next_fast.set(self.next_fast.get().wrapping_add(1));
                if let Some(notify) = state.slots[index].notify.as_ref() {
                    let notified = notify.state.load(Ordering::Acquire);
                    notify.queued.store(false, Ordering::Release);
                    if notified & (WOKEN | INACTIVE) == WOKEN {
                        state.push(index);
                    }
                }
            }
        }
        // An earlier producer may still be publishing a reserved position.
        // It owns the wake; overflow must not pass it and change FIFO order.
        if self.shared.fast.pending(self.next_fast.get()) {
            return;
        }
        if !self.shared.notified.load(Ordering::Acquire) {
            return;
        }
        self.drain_locked(&mut self.shared.pending.lock().unwrap());
    }
    fn drain_locked(&self, pending: &mut VecDeque<Arc<Notify>>) {
        let mut state = self.state.borrow_mut();
        while let Some(notify) = pending.pop_front() {
            if notify.state.load(Ordering::Acquire) & INACTIVE == 0
                && state.slots[notify.index].generation == notify.generation
            {
                state.push(notify.index);
            }
            notify.queued.store(false, Ordering::Release);
        }
        self.shared.notified.store(false, Ordering::Release);
    }

    /// Bind a wakeup owner, optionally ready now. Remote requests are installed
    /// parked: publishing a question does not require polling for its absent reply.
    pub(crate) fn insert(&self, index: usize, runnable: bool) -> Waker {
        if index >= CAPACITY {
            // Promotion is permanent. New notifications use the original
            // bounded mailbox; earlier atomic reservations drain first.
            self.shared.overflow.store(true, Ordering::Release);
        }
        self.drain();
        // Synchronize even before the mailbox's notified bit is published.
        // A removed owner's in-flight wake must leave the mailbox before reuse;
        // this keeps its reserved capacity bounded to one entry per slot.
        if self.shared.overflow.load(Ordering::Acquire) {
            self.drain_locked(&mut self.shared.pending.lock().unwrap());
        }
        let mut state = self.state.borrow_mut();
        assert!(!state.closed);
        if index >= state.slots.len() {
            state.slots.resize_with(index + 1, Slot::default);
            // One coalesced mailbox entry per slot; waking never grows storage.
            if self.shared.overflow.load(Ordering::Acquire) {
                let mut pending = self.shared.pending.lock().unwrap();
                let additional = state.slots.len().saturating_sub(pending.len());
                pending.reserve(additional);
            }
        }
        let slot = &mut state.slots[index];
        assert!(!slot.active && !slot.queued);
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("operation generation exhausted");
        slot.active = true;
        let generation = slot.generation;
        let notify = match slot.notify.as_mut().and_then(Arc::get_mut) {
            Some(notify) => {
                notify.generation = generation;
                *notify.queued.get_mut() = false;
                *notify.state.get_mut() = 0;
                slot.notify.as_ref().unwrap().clone()
            }
            None => {
                let notify = Arc::new(Notify {
                    shared: Arc::downgrade(&self.shared),
                    index,
                    generation,
                    queued: AtomicBool::new(false),
                    state: AtomicU8::new(0),
                });
                slot.notify = Some(notify.clone());
                notify
            }
        };
        if runnable {
            state.push(index);
        }
        Waker::from(notify)
    }
    pub(crate) fn register(&self, parent: &Waker) {
        self.shared.fast.parent.register(parent);
    }
    pub(crate) fn notify_parent(&self) {
        self.shared.fast.parent.wake();
    }
    pub(crate) fn push(&self, index: usize) {
        self.drain();
        self.state.borrow_mut().push(index);
    }
    pub(crate) fn pop(&self) -> Option<Ready> {
        self.drain();
        let mut state = self.state.borrow_mut();
        let index = state.head?;
        let notify = state.slots[index].notify.as_ref().unwrap().clone();
        notify.state.swap(POLLING, Ordering::AcqRel);
        state.unlink(index);
        Some(Ready(notify))
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.drain();
        self.state.borrow().head.is_none()
    }
    pub(crate) fn remove(&self, index: usize) {
        let mut state = self.state.borrow_mut();
        state.slots[index]
            .notify
            .as_ref()
            .unwrap()
            .state
            .swap(INACTIVE, Ordering::AcqRel);
        state.unlink(index);
        state.slots[index].active = false;
    }
    pub(crate) fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        self.state.borrow_mut().closed = true;
        self.shared.pending.lock().unwrap().clear();
        self.shared.notified.store(false, Ordering::Release);
        self.shared.fast.parent.take();
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn ordinary_notifications_use_the_shared_atomic_fifo_without_locking() {
        let queue = Queue::default();
        let wake = queue.insert(0, false);
        let mailbox = queue.shared.pending.lock().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let producer = std::thread::spawn(move || {
            wake.wake();
            sent.send(()).unwrap();
        });
        let result = received.recv_timeout(std::time::Duration::from_secs(2));
        // Release before asserting, so the old implementation fails cleanly
        // rather than leaving a blocked producer or a poisoned Drop path.
        drop(mailbox);
        producer.join().unwrap();
        result.expect("ordinary wake tried to acquire the overflow mailbox");
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        assert!(queue.is_empty());
    }
    #[test]
    fn atomic_fifo_wraps_and_promotion_keeps_earlier_notifications_first() {
        let queue = Queue::default();
        let mut wakes: Vec<_> = (0..CAPACITY).map(|i| queue.insert(i, false)).collect();
        for _ in 0..8 {
            for wake in wakes.iter().rev() {
                wake.wake_by_ref();
            }
            for index in (0..CAPACITY).rev() {
                assert_eq!(queue.pop().map(|ready| ready.index()), Some(index));
            }
            assert!(queue.is_empty());
        }
        for wake in wakes.iter().rev() {
            wake.wake_by_ref();
        }
        wakes.push(queue.insert(CAPACITY, false));
        wakes[CAPACITY].wake_by_ref();
        for index in (0..CAPACITY).rev().chain([CAPACITY]) {
            assert_eq!(queue.pop().map(|ready| ready.index()), Some(index));
        }
        assert!(queue.is_empty());
        // After promotion every generation still has its own wake authority.
        let old = wakes[0].clone();
        queue.remove(0);
        let new = queue.insert(0, false);
        old.wake_by_ref();
        assert!(queue.is_empty());
        new.wake();
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
    }
    #[test]
    fn atomic_notifications_from_concurrent_producers_are_coalesced_without_loss() {
        let queue = Queue::default();
        let wakes: Vec<_> = (0..CAPACITY).map(|i| queue.insert(i, false)).collect();
        for _ in 0..8 {
            std::thread::scope(|threads| {
                for group in wakes.chunks(16) {
                    threads.spawn(move || {
                        for _ in 0..8 {
                            for wake in group {
                                wake.wake_by_ref();
                            }
                        }
                    });
                }
            });
            let mut seen = std::collections::BTreeSet::new();
            while let Some(ready) = queue.pop() {
                assert!(seen.insert(ready.index()));
            }
            assert_eq!(seen, (0..CAPACITY).collect());
        }
    }
    #[test]
    fn parked_owners_and_remote_notifications_preserve_fifo_and_generation() {
        let queue = Queue::default();
        let first = queue.insert(0, false);
        let old = queue.insert(1, false);
        assert!(queue.is_empty());
        std::thread::spawn(move || first.wake()).join().unwrap();
        queue.push(1);
        assert_eq!(
            queue.pop().map(|ready| ready.index()),
            Some(0),
            "earlier wake precedes local scheduling"
        );
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(1));
        old.wake_by_ref(); // Leave a mailbox event while its slot is reused.
        queue.remove(1);
        let current = queue.insert(1, false);
        assert!(
            queue.is_empty(),
            "old mailbox entry cannot wake the new owner"
        );
        std::thread::spawn(move || old.wake()).join().unwrap();
        assert!(queue.is_empty());
        current.wake();
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(1));
        assert!(queue.is_empty());
    }
    #[test]
    fn remote_wake_batches_coalesce_without_losing_owners() {
        let queue = Queue::default();
        let parent = Arc::new(Count::default());
        queue.register(&Waker::from(parent.clone()));
        let wakes: Vec<_> = (0..64).map(|i| queue.insert(i, false)).collect();
        std::thread::spawn(move || {
            for wake in wakes {
                wake.wake_by_ref();
                wake.wake();
            }
        })
        .join()
        .unwrap();
        assert_eq!(parent.0.load(Ordering::SeqCst), 1);
        for i in 0..64 {
            assert_eq!(queue.pop().map(|ready| ready.index()), Some(i));
        }
        assert!(queue.is_empty());
    }
    #[test]
    fn synchronous_completion_discards_self_wakes_but_pending_republishes_them() {
        let queue = Queue::default();
        let parent = Arc::new(Count::default());
        queue.register(&Waker::from(parent.clone()));
        let wake = queue.insert(0, true);
        let polling = queue.pop().unwrap();
        wake.wake_by_ref();
        wake.wake_by_ref();
        assert_eq!(parent.0.load(Ordering::SeqCst), 0);
        queue.remove(polling.index());
        drop(polling);
        assert_eq!(parent.0.load(Ordering::SeqCst), 0);
        assert!(queue.is_empty());

        let wake = queue.insert(0, true);
        let polling = queue.pop().unwrap();
        std::thread::spawn(move || wake.wake()).join().unwrap();
        assert_eq!(parent.0.load(Ordering::SeqCst), 0);
        drop(polling);
        assert_eq!(parent.0.load(Ordering::SeqCst), 1);
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        assert!(queue.is_empty());
    }
    #[test]
    fn wake_during_queue_to_poll_handoff_is_observed_or_requeued() {
        let queue = Queue::default();
        let wake = queue.insert(0, true);
        let published = Arc::new(AtomicUsize::new(0));
        let begin = Arc::new(std::sync::Barrier::new(2));
        let end = Arc::new(std::sync::Barrier::new(2));
        let thread = std::thread::spawn({
            let published = published.clone();
            let begin = begin.clone();
            let end = end.clone();
            move || {
                for generation in 1..=1000 {
                    begin.wait();
                    published.store(generation, Ordering::Relaxed);
                    wake.wake_by_ref();
                    end.wait();
                }
            }
        });
        for generation in 1..=1000 {
            begin.wait();
            let polling = queue.pop().expect("scheduled owner");
            // This load intentionally has no independent synchronization: the
            // wake-to-poll handoff must publish the producer's preceding work.
            let observed = published.load(Ordering::Relaxed);
            drop(polling);
            end.wait();
            let next = queue.pop();
            assert!(observed == generation || next.is_some());
            drop(next);
            queue.push(0);
        }
        thread.join().unwrap();
        queue.remove(0);
        assert!(queue.is_empty());
    }
    #[test]
    fn concurrent_wake_at_poll_exit_is_never_lost() {
        let queue = Queue::default();
        let wake = queue.insert(0, true);
        let begin = Arc::new(std::sync::Barrier::new(2));
        let end = Arc::new(std::sync::Barrier::new(2));
        let thread = std::thread::spawn({
            let begin = begin.clone();
            let end = end.clone();
            move || {
                for _ in 0..1000 {
                    begin.wait();
                    wake.wake_by_ref();
                    end.wait();
                }
            }
        });
        let mut polling = Some(queue.pop().unwrap());
        for _ in 0..1000 {
            begin.wait();
            drop(polling.take());
            end.wait();
            polling = Some(queue.pop().expect("wake survives concurrent poll exit"));
        }
        thread.join().unwrap();
        queue.remove(0);
        drop(polling);
        assert!(queue.is_empty());
    }
    #[test]
    fn fifo_coalesces_and_removes_queued_operations() {
        let queue = Queue::default();
        let a = queue.insert(0, true);
        let b = queue.insert(1, true);
        let c = queue.insert(2, true);
        b.wake_by_ref();
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        a.wake_by_ref();
        a.wake_by_ref();
        queue.remove(2); // Unlink the middle, between 1 and 0.
        c.wake_by_ref();
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(1));
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        assert_eq!(queue.pop().map(|ready| ready.index()), None);
    }
    #[test]
    fn stale_cross_thread_wakes_cannot_reactivate_reused_or_closed_slots() {
        let queue = Queue::default();
        let old = queue.insert(0, true);
        queue.remove(0);
        let current = queue.insert(0, true);
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        std::thread::spawn(move || old.wake()).join().unwrap();
        assert_eq!(queue.pop().map(|ready| ready.index()), None);
        let parent = Arc::new(Count::default());
        queue.register(&Waker::from(parent.clone()));
        std::thread::spawn({
            let current = current.clone();
            move || current.wake()
        })
        .join()
        .unwrap();
        assert_eq!(parent.0.load(Ordering::SeqCst), 1);
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        queue.close();
        current.wake();
        assert_eq!(queue.pop().map(|ready| ready.index()), None);
    }
    #[test]
    fn parent_replacement_and_reentrant_wake_keep_pending_work() {
        let queue = Queue::default();
        let operation = queue.insert(0, true);
        let first = Arc::new(Count::default());
        let second = Arc::new(Count::default());
        queue.register(&Waker::from(first.clone()));
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        queue.register(&Waker::from(second.clone()));
        operation.wake_by_ref();
        operation.wake_by_ref();
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        // A wake while an operation is being polled must survive Pending.
        queue.register(&Waker::from(second.clone()));
        operation.wake();
        assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
        assert_eq!(queue.pop().map(|ready| ready.index()), None);
        assert_eq!(second.0.load(Ordering::SeqCst), 2);
    }
}

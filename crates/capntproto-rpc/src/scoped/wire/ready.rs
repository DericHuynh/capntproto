//! Bounded FIFO of operation identities. Wakers carry no driver references.
//! Slot generations reject notifications retained by an earlier operation.
use std::sync::{Arc, Mutex, Weak};
use std::task::{Wake, Waker};

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
    live: usize,
    protocol_cursor: usize,
    protocol_remaining: usize,
    protocol_span: usize,
}
impl State {
    fn push(&mut self, index: usize) -> bool {
        let slot = &mut self.slots[index];
        if self.closed || !slot.active || slot.queued {
            return false;
        }
        slot.queued = true;
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
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    parent: futures::task::AtomicWaker,
}
struct Notify {
    shared: Weak<Shared>,
    index: usize,
    generation: u64,
}
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let queued = {
            let mut state = shared.state.lock().unwrap();
            state.slots[self.index].generation == self.generation && state.push(self.index)
        };
        // Never invoke an executor while holding the queue lock.
        if queued {
            shared.parent.wake();
        }
    }
}
#[derive(Default)]
pub(super) struct Queue(Arc<Shared>);
impl Queue {
    pub(super) fn insert(&self, index: usize) -> Waker {
        let mut state = self.0.state.lock().unwrap();
        assert!(!state.closed);
        if index == state.slots.len() {
            state.slots.push(Slot::default());
        }
        let slot = &mut state.slots[index];
        assert!(!slot.active && !slot.queued);
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("operation generation exhausted");
        slot.active = true;
        // Most synchronous methods release every waker before slot reuse.
        // Reuse their notifier allocation only when no old observer remains;
        // otherwise the old generation stays immutable for escaped wakers.
        let generation = slot.generation;
        let notify = match slot.notify.as_mut().and_then(Arc::get_mut) {
            Some(notify) => {
                notify.generation = generation;
                slot.notify.as_ref().unwrap().clone()
            }
            None => {
                let notify = Arc::new(Notify {
                    shared: Arc::downgrade(&self.0),
                    index,
                    generation,
                });
                slot.notify = Some(notify.clone());
                notify
            }
        };
        state.live += 1;
        state.push(index);
        Waker::from(notify)
    }

    pub(super) fn register(&self, parent: &Waker) {
        self.0.parent.register(parent);
    }
    pub(super) fn notify_parent(&self) {
        self.0.parent.wake();
    }
    pub(super) fn push(&self, index: usize) {
        self.0.state.lock().unwrap().push(index);
    }
    /// One ready operation or one protocol slot to inspect. A vacant protocol
    /// slot still consumes a worker budget unit; never hide a scan in this pop.
    pub(super) fn pop(&self) -> Option<usize> {
        let mut state = self.0.state.lock().unwrap();
        let index = if state.protocol_remaining != 0 {
            state.protocol_remaining -= 1;
            let index = state.protocol_cursor;
            state.protocol_cursor = if index + 1 == state.protocol_span {
                0
            } else {
                index + 1
            };
            index
        } else {
            state.head?
        };
        state.unlink(index);
        Some(index)
    }
    pub(super) fn is_empty(&self) -> bool {
        let state = self.0.state.lock().unwrap();
        state.head.is_none() && state.protocol_remaining == 0
    }
    pub(super) fn remove(&self, index: usize) {
        let mut state = self.0.state.lock().unwrap();
        state.unlink(index);
        state.slots[index].active = false;
        state.live -= 1;
        if state.live == 0 {
            state.protocol_remaining = 0;
        }
    }
    pub(super) fn invalidate(&self) {
        let mut state = self.0.state.lock().unwrap();
        // Preserve the rotating cursor under repeated IO. Freeze the span so
        // admission during a partial scan cannot cause an old slot to be missed.
        // Only explicit protocol effects enter this conservative, budgeted scan.
        if state.live != 0 {
            state.protocol_span = state.slots.len();
            state.protocol_remaining = state.protocol_span;
        }
    }
    pub(super) fn close(&self) {
        self.0.state.lock().unwrap().closed = true;
        self.0.parent.take();
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
    fn fifo_coalesces_and_removes_queued_operations() {
        let queue = Queue::default();
        let a = queue.insert(0);
        let b = queue.insert(1);
        let c = queue.insert(2);
        b.wake_by_ref();
        assert_eq!(queue.pop(), Some(0));
        a.wake_by_ref();
        a.wake_by_ref();
        queue.remove(2); // Unlink the middle, between 1 and 0.
        c.wake_by_ref();
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), None);
        queue.invalidate();
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2)); // Vacant protocol visits are charged too.
        assert_eq!(queue.pop(), None);
    }
    #[test]
    fn stale_cross_thread_wakes_cannot_reactivate_reused_or_closed_slots() {
        let queue = Queue::default();
        let old = queue.insert(0);
        queue.remove(0);
        let current = queue.insert(0);
        assert_eq!(queue.pop(), Some(0));
        std::thread::spawn(move || old.wake()).join().unwrap();
        assert_eq!(queue.pop(), None);
        let parent = Arc::new(Count::default());
        queue.register(&Waker::from(parent.clone()));
        std::thread::spawn({
            let current = current.clone();
            move || current.wake()
        })
        .join()
        .unwrap();
        assert_eq!(parent.0.load(Ordering::SeqCst), 1);
        assert_eq!(queue.pop(), Some(0));
        queue.close();
        current.wake();
        assert_eq!(queue.pop(), None);
    }
    #[test]
    fn parent_replacement_and_reentrant_wake_keep_pending_work() {
        let queue = Queue::default();
        let operation = queue.insert(0);
        let first = Arc::new(Count::default());
        let second = Arc::new(Count::default());
        queue.register(&Waker::from(first.clone()));
        assert_eq!(queue.pop(), Some(0));
        queue.register(&Waker::from(second.clone()));
        operation.wake_by_ref();
        operation.wake_by_ref();
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert_eq!(queue.pop(), Some(0));
        // A wake while an operation is being polled must survive Pending.
        queue.register(&Waker::from(second.clone()));
        operation.wake();
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), None);
        assert_eq!(second.0.load(Ordering::SeqCst), 2);
    }
}

#[cfg(test)]
#[test]
fn protocol_visits_keep_their_budget_and_cursor_across_growth_and_reinvalidation() {
    let queue = Queue::default();
    let _wakers: Vec<_> = (0..3).map(|index| queue.insert(index)).collect();
    for index in 0..3 {
        assert_eq!(queue.pop(), Some(index));
    }
    queue.invalidate();
    assert_eq!(queue.pop(), Some(0));
    queue.remove(1);
    let _new = queue.insert(3);
    assert_eq!(queue.pop(), Some(1)); // One vacant visit, not a hidden loop.
    assert_eq!(queue.pop(), Some(2)); // Growth did not change the old scan span.
    assert_eq!(queue.pop(), Some(3));
    assert!(queue.is_empty());
    queue.invalidate();
    assert_eq!(queue.pop(), Some(0));
    queue.invalidate(); // Repeated IO does not restart at slot zero.
    for index in [1, 2, 3, 0] {
        assert_eq!(queue.pop(), Some(index));
    }
    assert!(queue.is_empty());
    queue.invalidate();
    queue.remove(0);
    queue.remove(2);
    queue.remove(3);
    assert!(queue.is_empty()); // No scan debt survives the final operation.
}

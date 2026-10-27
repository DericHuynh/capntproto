//! Shared bounded atomic notification FIFO used by both hosted drivers.
use futures::task::AtomicWaker;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

// Notification storage is bounded independently of task lifetimes. A larger
// exceptional set uses FuturesUnordered rather than retaining an unbounded pool.
pub(crate) const CAPACITY: usize = 128;

pub(crate) struct ReadyQueue {
    pub(super) next: AtomicUsize,
    pub(super) entries: [AtomicU8; CAPACITY],
    pub(super) queued: [AtomicBool; CAPACITY],
    pub(crate) parent: AtomicWaker,
}
impl ReadyQueue {
    pub(crate) fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            entries: [const { AtomicU8::new(0) }; CAPACITY],
            queued: [const { AtomicBool::new(false) }; CAPACITY],
            parent: AtomicWaker::new(),
        }
    }
    pub(crate) fn enqueue(&self, index: usize) {
        // Even a duplicate publishes producer data. The consumer clears this
        // flag with acquire ordering before it polls the future.
        if self.queued[index].swap(true, Ordering::AcqRel) {
            return;
        }
        // Each slot owns at most one reservation, so at most CAPACITY
        // reservations can be outstanding. The consumer releases the entry
        // before allowing another reservation for its slot. Thus wrapping this
        // cursor cannot overwrite a live entry, even if a producer is suspended
        // between reserving its position and publishing its notification.
        let position = self.next.fetch_add(1, Ordering::Relaxed) % CAPACITY;
        let previous = self.entries[position].swap(index as u8 + 1, Ordering::AcqRel);
        debug_assert_eq!(previous, 0);
        self.parent.wake();
    }
    pub(crate) fn front(&self, position: usize) -> Option<usize> {
        let entry = self.entries[position % CAPACITY].load(Ordering::Acquire);
        (entry != 0).then(|| entry as usize - 1)
    }
    pub(crate) fn pending(&self, position: usize) -> bool {
        self.next.load(Ordering::Acquire) != position
    }
    pub(crate) fn consume(&self, position: usize, index: usize) {
        self.entries[position % CAPACITY].store(0, Ordering::Release);
        // A wake before this RMW is observed by the following poll; a wake
        // after it reserves a new position. Never replace this with a store.
        let was_queued = self.queued[index].swap(false, Ordering::AcqRel);
        debug_assert!(was_queued);
    }
}

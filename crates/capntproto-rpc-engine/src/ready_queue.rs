//! Allocator-free atomic notification storage, ported from the direct hosted driver.
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// Fixed notification storage. Callers own task lifetimes and overflow policy.
pub const CAPACITY: usize = 128;

/// Coalescing multi-producer FIFO with one caller-owned consumer cursor.
///
/// Slots are reusable operation indices below [`CAPACITY`]. Producers publish
/// one notification per slot; a single consumer releases each front entry with
/// [`consume`](Self::consume) before advancing its wrapping cursor. The caller
/// owns wakeups, task generations and all scheduling policy.
pub struct ReadyQueue {
    next: AtomicUsize,
    entries: [AtomicU8; CAPACITY],
    queued: [AtomicBool; CAPACITY],
}
impl ReadyQueue {
    /// Construct empty inline storage without an allocator.
    pub const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            entries: [const { AtomicU8::new(0) }; CAPACITY],
            queued: [const { AtomicBool::new(false) }; CAPACITY],
        }
    }
    /// Publish an index, returning false when its existing notification coalesces it.
    /// The producer must wake the consumer after a true return.
    /// Panics when `index >= CAPACITY`.
    #[inline]
    pub fn enqueue(&self, index: usize) -> bool {
        // Even a duplicate publishes producer data. The consumer clears this
        // flag with acquire ordering before it polls the future.
        if self.queued[index].swap(true, Ordering::AcqRel) {
            return false;
        }
        // Each slot owns at most one reservation, so at most CAPACITY
        // reservations can be outstanding. The consumer releases the entry
        // before allowing another reservation for its slot. Thus wrapping this
        // cursor cannot overwrite a live entry, even if a producer is suspended
        // between reserving its position and publishing its notification.
        let position = self.next.fetch_add(1, Ordering::Relaxed) % CAPACITY;
        let previous = self.entries[position].swap(index as u8 + 1, Ordering::AcqRel);
        debug_assert_eq!(previous, 0);
        true
    }
    /// Read the next published entry. An unfinished earlier reservation returns None.
    #[inline]
    pub fn front(&self, position: usize) -> Option<usize> {
        let entry = self.entries[position % CAPACITY].load(Ordering::Acquire);
        (entry != 0).then(|| entry as usize - 1)
    }
    /// Whether published or still-in-progress reservations remain after this cursor.
    #[inline]
    pub fn pending(&self, position: usize) -> bool {
        self.next.load(Ordering::Acquire) != position
    }
    /// Release the entry returned by `front(position)` before advancing the cursor.
    /// The caller must use exactly one consumer and the matching position/index.
    #[inline]
    pub fn consume(&self, position: usize, index: usize) {
        self.entries[position % CAPACITY].store(0, Ordering::Release);
        // A wake before this RMW is observed by the following poll; a wake
        // after it reserves a new position. Never replace this with a store.
        let was_queued = self.queued[index].swap(false, Ordering::AcqRel);
        debug_assert!(was_queued);
    }
}

impl Default for ReadyQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::sync::Arc;
    type Ready = ReadyQueue;
    const REUSABLE_TASKS: usize = CAPACITY;
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
    fn notification_storage_and_repeated_use_allocate_nothing() {
        let count = allocation_counter::measure(|| {
            let ready = ReadyQueue::new();
            let mut position = 0usize;
            for _ in 0..4 {
                for index in 0..CAPACITY {
                    assert!(ready.enqueue(index));
                }
                for index in 0..CAPACITY {
                    assert_eq!(ready.front(position), Some(index));
                    ready.consume(position, index);
                    position = position.wrapping_add(1);
                }
                assert!(!ready.pending(position));
            }
        });
        assert_eq!(count.count_total, 0);
        assert_eq!(count.bytes_total, 0);
    }
}

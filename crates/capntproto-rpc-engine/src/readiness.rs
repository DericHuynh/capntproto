//! Bounded, coalesced task notifications over caller-owned atomic storage.
//!
//! This is the readiness algorithm used by the hosted legacy task set. It owns
//! no tasks, wakers, executor or allocator. A single consumer keeps its cursor;
//! producers publish slot indices and notify the caller's executor. Available
//! on targets with native pointer-width and byte atomics.
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// Inline storage for `N` reusable notification slots, initially empty.
///
/// Capacity must be a power of two from 1 through 128. One notification per
/// slot can be outstanding, so notification publication cannot exhaust this
/// storage. Task admission and task storage capacity remain the caller's job.
pub struct Storage<const N: usize> {
    next: AtomicUsize,
    entries: [AtomicU8; N],
    queued: [AtomicBool; N],
}

impl<const N: usize> Storage<N> {
    pub const fn new() -> Self {
        assert!(N.is_power_of_two() && N <= 128);
        Self {
            next: AtomicUsize::new(0),
            entries: [const { AtomicU8::new(0) }; N],
            queued: [const { AtomicBool::new(false) }; N],
        }
    }

    #[inline]
    pub const fn queue(&self) -> Queue<'_, N> {
        Queue::new(&self.next, &self.entries, &self.queued)
    }
}

impl<const N: usize> Default for Storage<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Borrowed notification transitions, including for storage embedded in an
/// existing owner whose field layout must remain unchanged.
///
/// Producers may share this view. Exactly one consumer must own the cursor,
/// starting at zero for fresh storage. Consume each observed front once, then
/// advance the cursor with `wrapping_add(1)`. Clear the notification **before**
/// polling the slot, so a concurrent or reentrant wake is either observed by
/// that poll or queues another one. Slots identify reusable wake locations;
/// a stale wake may poll a replacement but never conveys task completion.
#[derive(Clone, Copy)]
pub struct Queue<'a, const N: usize> {
    next: &'a AtomicUsize,
    entries: &'a [AtomicU8; N],
    queued: &'a [AtomicBool; N],
}

impl<'a, const N: usize> Queue<'a, N> {
    /// Borrow the existing producer cursor, ring entries and membership flags.
    /// Fresh storage must contain zeros/false; the consumer cursor starts at
    /// zero. Reborrowing live storage preserves its existing consumer cursor.
    /// Do not reset or independently mutate these fields while a queue is live.
    /// Panics unless `N` is a power of two from 1 through 128.
    #[inline(always)]
    pub const fn new(
        next: &'a AtomicUsize,
        entries: &'a [AtomicU8; N],
        queued: &'a [AtomicBool; N],
    ) -> Self {
        assert!(N.is_power_of_two() && N <= 128);
        Self {
            next,
            entries,
            queued,
        }
    }

    /// Publish a slot and invoke `notify` after publication, without holding a
    /// lock. Duplicate notifications publish producer data but do not invoke
    /// the callback or reserve another position. Panics for an invalid slot.
    /// The callback must arrange progress for the single consumer.
    #[inline(always)]
    pub fn enqueue(&self, index: usize, notify: impl FnOnce()) {
        // Even a duplicate publishes producer data. The consumer clears this
        // flag with acquire ordering before it polls the future.
        if self.queued[index].swap(true, Ordering::AcqRel) {
            return;
        }
        // Each slot owns at most one reservation, so at most N reservations
        // can be outstanding. Release the entry before allowing another
        // reservation for its slot. A suspended producer cannot be overtaken
        // or overwritten, including when the cursor wraps. N divides the
        // usize wrapping modulus because it is a power of two.
        let position = self.next.fetch_add(1, Ordering::Relaxed) % N;
        let previous = self.entries[position].swap(index as u8 + 1, Ordering::AcqRel);
        debug_assert_eq!(previous, 0);
        notify();
    }

    /// Inspect the consumer's current position without passing an unpublished
    /// reservation. `None` can mean an earlier producer is still publishing;
    /// that producer invokes its notification callback after publication.
    #[inline(always)]
    pub fn front(&self, position: usize) -> Option<usize> {
        let entry = self.entries[position % N].load(Ordering::Acquire);
        (entry != 0).then(|| entry as usize - 1)
    }

    /// Release the index returned by `front(position)` before polling it.
    /// Only the single consumer may call this, once per observed entry. Advance
    /// its cursor with wrapping arithmetic after this call.
    #[inline(always)]
    pub fn consume(&self, position: usize, index: usize) {
        self.entries[position % N].store(0, Ordering::Release);
        // A wake before this RMW is observed by the following poll; a wake
        // after it reserves a new position. Never replace this with a store.
        let was_queued = self.queued[index].swap(false, Ordering::AcqRel);
        debug_assert!(was_queued);
    }
}

#[cfg(test)]
mod tests;

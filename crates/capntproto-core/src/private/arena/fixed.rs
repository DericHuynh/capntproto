//! Single-segment reservations over exclusively borrowed caller storage.
#![deny(unsafe_op_in_unsafe_fn)]

use super::{BuilderArena, ReaderArena};
use crate::{Error, ErrorKind, Result};
use core::{marker::PhantomData, slice};

pub(crate) struct FixedArena<'a> {
    // Keep provenance in a raw pointer: reborrowing the arena metadata must not
    // retag the backing slice while layout builders hold pointers into it.
    base: *mut u8,
    capacity: u32,
    used: u32,
    borrow: PhantomData<&'a mut [u8]>,
}

impl<'a> FixedArena<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(Error::from_kind(ErrorKind::InsufficientCapacity {
                requested_words: 1,
                available_words: 0,
            }));
        }
        if !cfg!(feature = "unaligned") && !(bytes.as_ptr() as usize).is_multiple_of(8) {
            return Err(Error::from_kind(ErrorKind::UnalignedSegment));
        }
        // Keep every intra-segment offset representable by the wire format.
        let capacity = u32::try_from(bytes.len() / 8)
            .ok()
            .filter(|&words| words < (1 << 29))
            .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?;
        let mut arena = Self {
            base: bytes.as_mut_ptr(),
            capacity,
            used: 0,
            borrow: PhantomData,
        };
        arena
            .allocate(0, 1)
            .expect("capacity checked for root word");
        Ok(arena)
    }

    pub(crate) fn capacity(&self) -> u32 {
        self.capacity
    }
    pub(crate) fn used(&self) -> u32 {
        self.used
    }
    pub(crate) fn remaining(&self) -> u32 {
        self.capacity - self.used
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: used words are initialized reservations in the live borrowed
        // slice. The returned borrow excludes every mutable builder operation.
        unsafe { slice::from_raw_parts(self.base, self.used as usize * 8) }
    }

    pub(crate) fn into_bytes(self) -> &'a [u8] {
        // Transfer the backing borrow to the published segment. This arena owns
        // only pointer/count metadata; skipping its clearing Drop leaks no owner.
        let this = core::mem::ManuallyDrop::new(self);
        // SAFETY: consuming the arena excludes all readers/editors. The initialized
        // prefix is within the original buffer, which remains borrowed for 'a.
        // No arena destructor or mutable view can overwrite the returned segment.
        unsafe { slice::from_raw_parts(this.base, this.used as usize * 8) }
    }

    pub(crate) fn reset(&mut self) {
        self.clear();
        self.used = 1; // The cleared root remains reserved.
    }

    fn clear(&mut self) {
        // SAFETY: exclusive arena access excludes all views; used never exceeds
        // the original slice capacity. Only reserved words are modified.
        unsafe {
            self.base.write_bytes(0, self.used as usize * 8);
        }
    }
}

impl Drop for FixedArena<'_> {
    fn drop(&mut self) {
        self.clear();
    }
}

// SAFETY: the exclusive input borrow keeps initialized reservations live and
// prevents caller mutation. Lookup and interval checks restrict every read to
// used words of segment zero; ReaderArena's default offset check retains this
// pointer's provenance and validates relative offsets.
unsafe impl ReaderArena for FixedArena<'_> {
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)> {
        if id != 0 {
            return Err(Error::from_kind(ErrorKind::InvalidSegmentId(id)));
        }
        Ok((self.base, self.used))
    }

    fn contains_interval(&self, id: u32, start: *const u8, words: usize) -> Result<()> {
        self.get_segment(id)?;
        let valid = (start as usize)
            .checked_sub(self.base as usize)
            .zip(words.checked_mul(8))
            .and_then(|(offset, size)| offset.checked_add(size))
            .is_some_and(|end| end <= self.used as usize * 8);
        if valid {
            Ok(())
        } else {
            Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ))
        }
    }

    fn amplified_read(&self, _: u64) -> Result<()> {
        Ok(())
    }
    fn nesting_limit(&self) -> i32 {
        i32::MAX
    }
    fn size_in_words(&self) -> usize {
        self.used as usize
    }
}

// SAFETY: successful reservations advance a checked monotonic cursor and zero
// disjoint words before returning them. The slice never moves or grows. Failed
// reservations write nothing. The public bounded builder never exposes a raw
// arena, pointer builder or legacy growing editor to callers.
unsafe impl BuilderArena for FixedArena<'_> {
    fn allows_capabilities(&self) -> bool {
        false
    }
    fn allocation_checkpoint(&self) -> Option<u32> {
        Some(self.used)
    }
    unsafe fn rewind_allocations(&mut self, checkpoint: u32) {
        assert!((1..=self.used).contains(&checkpoint));
        // SAFETY: the caller relinquishes every unpublished reservation since
        // this arena's checkpoint. Both bounds lie inside the original slice.
        unsafe {
            self.base
                .add(checkpoint as usize * 8)
                .write_bytes(0, (self.used - checkpoint) as usize * 8);
        }
        self.used = checkpoint;
    }

    fn allocate(&mut self, id: u32, amount: u32) -> Option<u32> {
        if id != 0 || amount > self.remaining() {
            return None;
        }
        let start = self.used;
        // SAFETY: amount fits the remaining original allocation; start may be
        // one-past-end only for zero words. No live object overlaps this range.
        unsafe {
            self.base
                .add(start as usize * 8)
                .write_bytes(0, amount as usize * 8);
        }
        self.used += amount;
        Some(start)
    }

    fn allocate_anywhere(&mut self, amount: u32) -> (u32, u32) {
        self.try_allocate_anywhere(amount)
            .expect("legacy growth is unavailable on a fixed arena")
    }

    fn try_allocate_anywhere(&mut self, amount: u32) -> Result<(u32, u32)> {
        self.allocate(0, amount)
            .map(|start| (0, start))
            .ok_or_else(|| {
                Error::from_kind(ErrorKind::InsufficientCapacity {
                    requested_words: amount,
                    available_words: self.remaining(),
                })
            })
    }

    fn get_segment_mut(&mut self, id: u32) -> (*mut u8, u32) {
        assert_eq!(id, 0);
        (self.base, self.capacity)
    }

    fn as_reader(&self) -> &dyn ReaderArena {
        self
    }
}

#[cfg(kani)]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservations_zero_dirty_words_and_reset_preserves_unused_capacity() {
        for allocate in [false, true] {
            let mut words = [crate::word(0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5); 4];
            let used;
            {
                let mut arena =
                    FixedArena::new(crate::Word::words_to_bytes_mut(&mut words[..3])).unwrap();
                // SAFETY: the initialized root reservation has eight writable bytes.
                unsafe {
                    arena.base.write_bytes(0x5a, 8);
                }
                if allocate {
                    assert_eq!(arena.allocate(0, 2), Some(1));
                    assert_eq!(&arena.bytes()[8..], &[0; 16]);
                } else {
                    assert_eq!(arena.allocate(u32::MAX, u32::MAX), None);
                }
                assert_eq!(&arena.bytes()[..8], &[0x5a; 8]);
                assert_eq!(arena.allocate(u32::MAX, u32::MAX), None);
                used = arena.used() as usize;
                arena.reset();
                assert_eq!(arena.bytes(), &[0; 8]);
            }
            assert!(crate::Word::words_to_bytes(&words)[..used * 8]
                .iter()
                .all(|&byte| byte == 0));
            assert!(crate::Word::words_to_bytes(&words)[used * 8..]
                .iter()
                .all(|&byte| byte == 0xa5));
        }
    }
}

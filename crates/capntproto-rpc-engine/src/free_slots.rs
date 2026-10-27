//! Lowest-free-slot allocation using a bitmap and caller-backed heap cells.
//! Derived from the direct driver's local-ID table. Occupied values and free
//! heap cells have independent lifetimes, so release never needs an allocator.
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Debug)]
pub struct Link(usize);
impl Link {
    pub const EMPTY: Self = Self(0);
}

#[derive(Default)]
pub struct FreeSlots {
    issued: usize,
    small: usize,
    free: usize,
}
impl FreeSlots {
    pub const fn new() -> Self {
        Self {
            issued: 0,
            small: 0,
            free: 0,
        }
    }
    #[cfg(any(test, kani))]
    pub(crate) fn from_storage<T>(
        slots: &mut [T],
        reusable: impl Fn(&T) -> bool,
        link: impl Fn(&mut T) -> &mut Link,
    ) -> Self {
        let mut result = Self {
            issued: slots.len(),
            small: 0,
            free: 0,
        };
        for index in 0..slots.len() {
            if reusable(&slots[index]) {
                result.release(index, slots, &link);
            }
        }
        result
    }
    /// Inspect a reservation without changing it. Rejection needs no rollback.
    #[inline]
    pub fn peek<T>(&self, slots: &[T], link: impl Fn(&T) -> &Link) -> Option<usize> {
        if self.small != 0 {
            Some(self.small.trailing_zeros() as usize)
        } else if self.free != 0 {
            Some(link(&slots[0]).0)
        } else {
            (self.issued < slots.len()).then_some(self.issued)
        }
    }
    /// Commit the most recently inspected reservation under an exclusive borrow.
    pub fn take<T>(&mut self, slots: &mut [T], link: impl Fn(&mut T) -> &mut Link) -> usize {
        if self.small != 0 {
            let index = self.small.trailing_zeros() as usize;
            self.small &= self.small - 1;
            return index;
        }
        if self.free == 0 {
            let index = self.issued;
            assert!(index < slots.len());
            self.issued += 1;
            return index;
        }
        let index = link(&mut slots[0]).0;
        self.free -= 1;
        if self.free != 0 {
            let last = link(&mut slots[self.free]).0;
            let mut position = 0;
            while position * 2 + 1 < self.free {
                let mut child = position * 2 + 1;
                if child + 1 < self.free
                    && link(&mut slots[child + 1]).0 < link(&mut slots[child]).0
                {
                    child += 1;
                }
                if last <= link(&mut slots[child]).0 {
                    break;
                }
                link(&mut slots[position]).0 = link(&mut slots[child]).0;
                position = child;
            }
            link(&mut slots[position]).0 = last;
        }
        index
    }
    /// Return one previously committed slot, once its entire owner has retired.
    pub fn release<T>(
        &mut self,
        index: usize,
        slots: &mut [T],
        link: impl Fn(&mut T) -> &mut Link,
    ) {
        debug_assert!(index < self.issued);
        // These IDs precede every heap entry. Reusing them needs neither a
        // storage access nor heap comparisons, including at high concurrency.
        if index < usize::BITS as usize {
            let bit = 1usize << index;
            debug_assert_eq!(self.small & bit, 0);
            self.small |= bit;
            return;
        }
        debug_assert!(self.free < self.issued);
        let mut position = self.free;
        self.free += 1;
        while position != 0 {
            let parent = (position - 1) / 2;
            let previous = link(&mut slots[parent]).0;
            if previous <= index {
                break;
            }
            link(&mut slots[position]).0 = previous;
            position = parent;
        }
        link(&mut slots[position]).0 = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_reuse_the_lowest_free_slot_without_allocation() {
        let count = allocation_counter::measure(|| {
            let mut slots = [const { Link::EMPTY }; 137];
            let mut free = FreeSlots::new();
            let mut occupied = [false; 137];
            let mut random = 17u32;
            for _ in 0..if cfg!(miri) { 100 } else { 10_000 } {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                let index = random as usize % slots.len();
                if occupied[index] {
                    occupied[index] = false;
                    free.release(index, &mut slots, |slot| slot);
                } else if let Some(expected) = occupied.iter().position(|live| !live) {
                    assert_eq!(free.peek(&slots, |slot| slot), Some(expected));
                    // Abandoned preflight must leave the next reservation intact.
                    assert_eq!(free.peek(&slots, |slot| slot), Some(expected));
                    assert_eq!(free.take(&mut slots, |slot| slot), expected);
                    occupied[expected] = true;
                } else {
                    assert_eq!(free.peek(&slots, |slot| slot), None);
                }
            }
        });
        assert_eq!(count.count_total, 0);
        assert_eq!(count.bytes_total, 0);
    }

    #[test]
    fn existing_owners_and_later_growth_preserve_reservations() {
        let mut slots = [
            (Link::EMPTY, false),
            (Link::EMPTY, true),
            (Link::EMPTY, false),
        ];
        let mut free = FreeSlots::from_storage(&mut slots, |slot| !slot.1, |slot| &mut slot.0);
        assert_eq!(free.take(&mut slots, |slot| &mut slot.0), 0);
        assert_eq!(free.take(&mut slots, |slot| &mut slot.0), 2);
        assert_eq!(free.peek(&slots, |slot| &slot.0), None);
        free.release(1, &mut slots, |slot| &mut slot.0);
        assert_eq!(free.take(&mut slots, |slot| &mut slot.0), 1);
    }

    #[test]
    fn bitmap_and_heap_share_lowest_slot_order_across_growth() {
        const WORD: usize = usize::BITS as usize;
        let mut slots = [const { Link::EMPTY }; 2 * WORD + 3];
        let mut free = FreeSlots::new();
        for index in 0..WORD + 2 {
            assert_eq!(free.take(&mut slots[..WORD + 2], |slot| slot), index);
        }
        // Heap entries are independent of the occupied values and bitmap.
        for index in [WORD + 1, WORD, WORD - 1, 0] {
            free.release(index, &mut slots, |slot| slot);
        }
        for index in [0, WORD - 1, WORD, WORD + 1] {
            assert_eq!(free.peek(&slots, |slot| slot), Some(index));
            assert_eq!(free.take(&mut slots, |slot| slot), index);
        }
        // Exhausting recycled slots resumes the unchanged issuance frontier.
        assert_eq!(free.peek(&slots[..WORD + 2], |slot| slot), None);
        assert_eq!(free.take(&mut slots, |slot| slot), WORD + 2);
    }
}

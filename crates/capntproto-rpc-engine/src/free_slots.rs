//! Lowest-free-slot allocation using heap cells embedded in caller storage.
//! Derived from the direct driver's local-ID table. Occupied values and free
//! heap cells have independent lifetimes, so release never needs an allocator.

#[derive(Debug)]
pub struct Link(usize);
impl Link {
    pub const EMPTY: Self = Self(0);
}

#[derive(Default)]
pub struct FreeSlots {
    issued: usize,
    free: usize,
}
impl FreeSlots {
    pub const fn new() -> Self {
        Self { issued: 0, free: 0 }
    }
    #[cfg(any(test, kani))]
    pub(crate) fn from_storage<T>(
        slots: &mut [T],
        reusable: impl Fn(&T) -> bool,
        link: impl Fn(&mut T) -> &mut Link,
    ) -> Self {
        let mut result = Self {
            issued: slots.len(),
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
        if self.free != 0 {
            Some(link(&slots[0]).0)
        } else {
            (self.issued < slots.len()).then_some(self.issued)
        }
    }
    /// Commit the most recently inspected reservation under an exclusive borrow.
    pub fn take<T>(&mut self, slots: &mut [T], link: impl Fn(&mut T) -> &mut Link) -> usize {
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
        debug_assert!(index < self.issued && self.free < self.issued);
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
            let mut slots = [const { Link::EMPTY }; 37];
            let mut free = FreeSlots::new();
            let mut occupied = [false; 37];
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
}

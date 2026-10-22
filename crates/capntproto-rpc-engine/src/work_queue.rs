//! Intrusive, deduplicated work over caller-owned slots. A queued owner keeps
//! its slot until removal; notifications validate generation before enqueueing.
use core::num::NonZeroUsize;

// Store slot indices plus one so absence occupies the zero niche.
// An unqueued link has no predecessor; the head names itself as predecessor.
// Queue membership therefore needs no separate flag or reserved slot index.
#[derive(Default)]
pub struct Link {
    previous: Option<NonZeroUsize>,
    next: Option<NonZeroUsize>,
}
impl Link {
    pub const EMPTY: Self = Self {
        previous: None,
        next: None,
    };
}
#[derive(Default)]
pub struct Queue {
    head: Option<NonZeroUsize>,
    tail: Option<NonZeroUsize>,
}
impl Queue {
    pub const EMPTY: Self = Self {
        head: None,
        tail: None,
    };
    pub fn head(&self) -> Option<usize> {
        self.head.map(|index| index.get() - 1)
    }
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }
    pub fn push<T>(&mut self, slots: &mut [T], index: usize, link: fn(&mut T) -> &mut Link) {
        self.push_with(slots, index, link);
    }
    pub(crate) fn push_with<T>(
        &mut self,
        slots: &mut [T],
        index: usize,
        link: impl Fn(&mut T) -> &mut Link,
    ) {
        let entry = link(&mut slots[index]);
        if entry.previous.is_some() {
            return;
        }
        let index = NonZeroUsize::new(index + 1).expect("a live slot index fits");
        entry.previous = self.tail.or(Some(index));
        entry.next = None;
        if let Some(tail) = self.tail {
            link(&mut slots[tail.get() - 1]).next = Some(index);
        } else {
            self.head = Some(index);
        }
        self.tail = Some(index);
    }
    pub fn remove<T>(&mut self, slots: &mut [T], index: usize, link: fn(&mut T) -> &mut Link) {
        self.remove_with(slots, index, link);
    }
    pub(crate) fn remove_with<T>(
        &mut self,
        slots: &mut [T],
        index: usize,
        link: impl Fn(&mut T) -> &mut Link,
    ) {
        let entry = link(&mut slots[index]);
        let Some(previous) = entry.previous.take() else {
            return;
        };
        let previous = (previous.get() - 1 != index).then_some(previous);
        let next = entry.next.take();
        if let Some(previous) = previous {
            link(&mut slots[previous.get() - 1]).next = next;
        } else {
            self.head = next;
        }
        if let Some(next) = next {
            link(&mut slots[next.get() - 1]).previous = previous.or(Some(next));
        } else {
            self.tail = previous;
        }
    }
    pub fn pop<T>(&mut self, slots: &mut [T], link: fn(&mut T) -> &mut Link) -> Option<usize> {
        self.pop_with(slots, link)
    }
    pub(crate) fn pop_with<T>(
        &mut self,
        slots: &mut [T],
        link: impl Fn(&mut T) -> &mut Link,
    ) -> Option<usize> {
        let index = self.head()?;
        self.remove_with(slots, index, link);
        Some(index)
    }
    pub(crate) fn insert_sorted_with<T>(
        &mut self,
        slots: &mut [T],
        index: usize,
        link: impl Fn(&mut T) -> &mut Link,
        value: impl Fn(&mut T) -> usize,
    ) {
        let minimum = value(&mut slots[index]);
        let index = NonZeroUsize::new(index + 1).expect("a live slot index fits");
        let mut next = self.head;
        while let Some(current) = next {
            if value(&mut slots[current.get() - 1]) > minimum {
                break;
            }
            next = link(&mut slots[current.get() - 1]).next;
        }
        let Some(next) = next else {
            self.push_with(slots, index.get() - 1, link);
            return;
        };
        let previous = link(&mut slots[next.get() - 1])
            .previous
            .expect("queued successor");
        let previous = (previous != next).then_some(previous);
        *link(&mut slots[index.get() - 1]) = Link {
            previous: previous.or(Some(index)),
            next: Some(next),
        };
        link(&mut slots[next.get() - 1]).previous = Some(index);
        if let Some(previous) = previous {
            link(&mut slots[previous.get() - 1]).next = Some(index);
        } else {
            self.head = Some(index);
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn sorted_insertion_preserves_ties_and_unlinks_head_middle_and_tail() {
        let mut slots = [5, 1, 3, 1, 9, 2].map(|value| (Link::EMPTY, value));
        let mut queue = Queue::EMPTY;
        for index in 0..slots.len() {
            queue.insert_sorted_with(&mut slots, index, |s| &mut s.0, |s| s.1);
        }
        for index in [5, 4, 1] {
            queue.remove(&mut slots, index, |s| &mut s.0);
        }
        slots[5].1 = 1;
        queue.insert_sorted_with(&mut slots, 5, |s| &mut s.0, |s| s.1);
        queue.remove(&mut slots, 2, |s| &mut s.0);
        slots[4].1 = 0;
        queue.insert_sorted_with(&mut slots, 4, |s| &mut s.0, |s| s.1);
        for index in [4, 3, 5, 0] {
            assert_eq!(queue.pop(&mut slots, |s| &mut s.0), Some(index));
        }
        assert_eq!(queue.pop(&mut slots, |s| &mut s.0), None);
    }

    #[test]
    fn arbitrary_removal_deduplication_and_slot_reuse_preserve_fifo() {
        let mut slots = [const { Link::EMPTY }; 17];
        let mut queue = Queue::EMPTY;
        let mut expected = VecDeque::new();
        let mut random = 7u32;
        for _ in 0..10_000 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let index = (random as usize >> 8) % slots.len();
            match random % 4 {
                0 | 1 => {
                    queue.push(&mut slots, index, |s| s);
                    if !expected.contains(&index) {
                        expected.push_back(index);
                    }
                }
                2 => {
                    queue.remove(&mut slots, index, |s| s);
                    expected.retain(|&i| i != index);
                }
                _ => assert_eq!(queue.pop(&mut slots, |s| s), expected.pop_front()),
            }
            assert_eq!(queue.head(), expected.front().copied());
            assert_eq!(queue.is_empty(), expected.is_empty());
        }
        while let Some(index) = expected.pop_front() {
            assert_eq!(queue.pop(&mut slots, |s| s), Some(index));
        }
        assert_eq!(queue.pop(&mut slots, |s| s), None);
    }
}

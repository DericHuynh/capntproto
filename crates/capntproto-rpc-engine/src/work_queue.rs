//! Intrusive, deduplicated work over caller-owned slots. A queued owner keeps
//! its slot until removal; notifications validate generation before enqueueing.
#[derive(Default)]
pub struct Link {
    previous: Option<usize>,
    next: Option<usize>,
    queued: bool,
}
impl Link {
    pub const EMPTY: Self = Self {
        previous: None,
        next: None,
        queued: false,
    };
}
#[derive(Default)]
pub struct Queue {
    head: Option<usize>,
    tail: Option<usize>,
}
impl Queue {
    pub const EMPTY: Self = Self {
        head: None,
        tail: None,
    };
    pub fn head(&self) -> Option<usize> {
        self.head
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
        if entry.queued {
            return;
        }
        entry.queued = true;
        entry.previous = self.tail;
        entry.next = None;
        if let Some(tail) = self.tail {
            link(&mut slots[tail]).next = Some(index);
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
        if !entry.queued {
            return;
        }
        entry.queued = false;
        let previous = entry.previous.take();
        let next = entry.next.take();
        if let Some(previous) = previous {
            link(&mut slots[previous]).next = next;
        } else {
            self.head = next;
        }
        if let Some(next) = next {
            link(&mut slots[next]).previous = previous;
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
        let index = self.head?;
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
        let mut next = self.head;
        while let Some(current) = next {
            if value(&mut slots[current]) > minimum {
                break;
            }
            next = link(&mut slots[current]).next;
        }
        let Some(next) = next else {
            self.push_with(slots, index, link);
            return;
        };
        let previous = link(&mut slots[next]).previous;
        *link(&mut slots[index]) = Link {
            previous,
            next: Some(next),
            queued: true,
        };
        link(&mut slots[next]).previous = Some(index);
        if let Some(previous) = previous {
            link(&mut slots[previous]).next = Some(index);
        } else {
            self.head = Some(index);
        }
    }
}

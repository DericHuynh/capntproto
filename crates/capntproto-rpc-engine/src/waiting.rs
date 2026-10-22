//! Intrusive runnable and resource-wait queues over caller-owned slots.
//! Failed transactions park after rollback. A wake removes the old wait, so
//! cancellation is never held behind capacity. No poll visits sleeping owners.
use crate::work_queue::{Link, Queue};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub resource: usize,
    pub minimum: usize,
}
pub struct Slot {
    link: Link,
    requirement: Option<Requirement>,
}
impl Slot {
    pub const EMPTY: Self = Self {
        link: Link::EMPTY,
        requirement: None,
    };
    pub fn is_parked(&self) -> bool {
        self.requirement.is_some()
    }
}
pub struct Waiting<const N: usize> {
    ready: Queue,
    resources: [Queue; N],
    active: u64,
    cursor: usize,
}
impl<const N: usize> Waiting<N> {
    pub fn has_waiters(&self) -> bool {
        self.active != 0
    }
    pub const EMPTY: Self = Self {
        ready: Queue::EMPTY,
        resources: [const { Queue::EMPTY }; N],
        active: 0,
        cursor: 0,
    };
    pub fn remove<T>(&mut self, slots: &mut [T], index: usize, slot: fn(&mut T) -> &mut Slot) {
        let requirement = slot(&mut slots[index]).requirement.take();
        let queue = match requirement {
            Some(r) => &mut self.resources[r.resource],
            None => &mut self.ready,
        };
        queue.remove_with(slots, index, |s| &mut slot(s).link);
        if let Some(r) = requirement {
            if queue.is_empty() {
                self.active &= !(1 << r.resource);
            }
        }
    }
    pub fn wake<T>(&mut self, slots: &mut [T], index: usize, slot: fn(&mut T) -> &mut Slot) {
        self.remove(slots, index, slot);
        self.ready.push_with(slots, index, |s| &mut slot(s).link);
    }
    pub fn park<T>(
        &mut self,
        slots: &mut [T],
        index: usize,
        requirement: Requirement,
        slot: fn(&mut T) -> &mut Slot,
    ) {
        assert!(requirement.resource < N && N < 64);
        self.remove(slots, index, slot);
        slot(&mut slots[index]).requirement = Some(requirement);
        self.active |= 1 << requirement.resource;
        // Only parking walks a resource's blocked list. Small demands cannot
        // be hidden behind a larger demand that remains unsatisfiable.
        self.resources[requirement.resource].insert_sorted_with(
            slots,
            index,
            |s| &mut slot(s).link,
            |s| slot(s).requirement.unwrap().minimum,
        );
    }
    pub fn has_ready<T>(
        &self,
        slots: &[T],
        slot: fn(&T) -> &Slot,
        mut available: impl FnMut(Requirement) -> bool,
    ) -> bool {
        if !self.ready.is_empty() {
            return true;
        }
        let mut active = self.active;
        while active != 0 {
            let class = active.trailing_zeros() as usize;
            active &= active - 1;
            let index = self.resources[class].head().unwrap();
            if available(slot(&slots[index]).requirement.unwrap()) {
                return true;
            }
        }
        false
    }
    pub fn pop<T>(
        &mut self,
        slots: &mut [T],
        slot: fn(&mut T) -> &mut Slot,
        mut available: impl FnMut(Requirement) -> bool,
    ) -> Option<usize> {
        if self.active == 0 {
            return self.ready.pop_with(slots, |s| &mut slot(s).link);
        }
        let mut candidates = self.active | (u64::from(!self.ready.is_empty()) << N);
        while candidates != 0 {
            let after = candidates & (u64::MAX << self.cursor);
            let class = (if after == 0 { candidates } else { after }).trailing_zeros() as usize;
            candidates &= !(1 << class);
            self.cursor = (class + 1) % (N + 1);
            let queue = if class == N {
                &mut self.ready
            } else {
                &mut self.resources[class]
            };
            let index = queue.head().unwrap();
            if class != N && !available(slot(&mut slots[index]).requirement.unwrap()) {
                continue;
            }
            queue.remove_with(slots, index, |s| &mut slot(s).link);
            if class != N && queue.is_empty() {
                self.active &= !(1 << class);
            }
            slot(&mut slots[index]).requirement = None;
            return Some(index);
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smaller_demands_and_cancellation_pass_blocked_owners_without_polling_them() {
        let mut slots = [const { Slot::EMPTY }; 4];
        let mut q = Waiting::<2>::EMPTY;
        q.park(
            &mut slots,
            0,
            Requirement {
                resource: 0,
                minimum: 9,
            },
            |s| s,
        );
        q.park(
            &mut slots,
            1,
            Requirement {
                resource: 0,
                minimum: 2,
            },
            |s| s,
        );
        q.park(
            &mut slots,
            2,
            Requirement {
                resource: 1,
                minimum: 1,
            },
            |s| s,
        );
        assert!(!q.has_ready(&slots, |s| s, |_| false));
        assert_eq!(
            q.pop(&mut slots, |s| s, |r| r.resource == 0 && r.minimum <= 2),
            Some(1)
        );
        assert_eq!(q.pop(&mut slots, |s| s, |_| false), None);
        q.wake(&mut slots, 0, |s| s); // cancellation bypasses the resource wait
        assert_eq!(q.pop(&mut slots, |s| s, |_| false), Some(0));
        q.remove(&mut slots, 2, |s| s); // release before slot reuse
        q.park(
            &mut slots,
            2,
            Requirement {
                resource: 0,
                minimum: 1,
            },
            |s| s,
        );
        assert_eq!(q.pop(&mut slots, |s| s, |r| r.resource == 1), None);
        assert_eq!(q.pop(&mut slots, |s| s, |r| r.resource == 0), Some(2));
        assert!(!q.has_ready(&slots, |s| s, |_| true));
    }
    #[test]
    fn runnable_and_available_resource_heads_are_fair_at_budget_one() {
        let mut slots = [const { Slot::EMPTY }; 3];
        let mut q = Waiting::<2>::EMPTY;
        q.park(
            &mut slots,
            0,
            Requirement {
                resource: 0,
                minimum: 1,
            },
            |s| s,
        );
        q.park(
            &mut slots,
            1,
            Requirement {
                resource: 1,
                minimum: 1,
            },
            |s| s,
        );
        q.wake(&mut slots, 2, |s| s);
        for expected in [0, 1, 2] {
            let next = q.pop(&mut slots, |s| s, |_| true).unwrap();
            assert_eq!(next, expected);
            if next < 2 {
                q.park(
                    &mut slots,
                    next,
                    Requirement {
                        resource: next,
                        minimum: 1,
                    },
                    |s| s,
                );
            } else {
                q.wake(&mut slots, next, |s| s);
            }
        }
    }
}

//! Reusable ownership slots over the same generation-safe FIFO used by calls.
//! The queue never owns affine protocol resources; teardown returns them to the
//! caller after disconnecting every notifier. Ordinary connections allocate none.
use super::*;
use std::task::Context;

pub(crate) struct Owners<T> {
    state: Option<Box<State<T>>>,
}
struct State<T> {
    entries: Vec<Option<Entry<T>>>,
    ready: Queue,
    live: usize,
    first_free: usize,
}
struct Entry<T> {
    value: T,
    waker: Waker,
}
impl<T> Default for Owners<T> {
    fn default() -> Self {
        Self { state: None }
    }
}
impl<T> Owners<T> {
    pub(crate) fn len(&self) -> usize {
        self.state.as_ref().map_or(0, |state| state.live)
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(crate) fn try_reserve(
        &mut self,
        additional: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        let state = self.state.get_or_insert_with(|| {
            Box::new(State {
                entries: Vec::new(),
                ready: Queue::default(),
                live: 0,
                first_free: 0,
            })
        });
        if state.first_free == state.entries.len() {
            state.entries.try_reserve(additional)
        } else {
            Ok(())
        }
    }
    pub(crate) fn insert(&mut self, value: T, runnable: bool) -> usize {
        let state = self.state.as_mut().expect("reserved owner storage");
        let index = state.first_free;
        let entry = Some(Entry {
            value,
            waker: state.ready.insert(index, runnable),
        });
        if index == state.entries.len() {
            state.entries.push(entry);
        } else {
            state.entries[index] = entry;
        }
        state.live += 1;
        state.first_free = state.entries[index + 1..]
            .iter()
            .position(Option::is_none)
            .map_or(state.entries.len(), |offset| index + 1 + offset);
        index
    }
    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        self.state
            .as_ref()?
            .entries
            .get(index)?
            .as_ref()
            .map(|entry| &entry.value)
    }
    pub(crate) fn get_mut(&mut self, index: usize) -> (&mut T, &Waker) {
        let entry = self.state.as_mut().unwrap().entries[index]
            .as_mut()
            .expect("live owner");
        (&mut entry.value, &entry.waker)
    }
    pub(crate) fn waker(&self, index: usize) -> &Waker {
        &self.state.as_ref().unwrap().entries[index]
            .as_ref()
            .expect("live owner")
            .waker
    }
    pub(crate) fn next(&self, cx: &mut Context<'_>) -> Option<Ready> {
        let state = self.state.as_ref()?;
        state.ready.register(cx.waker());
        state.ready.pop()
    }
    pub(crate) fn schedule(&self, index: usize) {
        let state = self.state.as_ref().unwrap();
        state.ready.push(index);
    }
    pub(crate) fn notify(&self, index: usize) {
        self.schedule(index);
        self.state.as_ref().unwrap().ready.notify_parent();
    }
    pub(crate) fn take(&mut self, index: usize) -> T {
        let state = self.state.as_mut().unwrap();
        state.ready.remove(index);
        let entry = state.entries[index].take().expect("live owner");
        state.live -= 1;
        state.first_free = state.first_free.min(index);
        entry.value
    }
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = T> {
        let entries = self
            .state
            .take()
            .map(|mut state| {
                let entries = std::mem::take(&mut state.entries);
                drop(state);
                entries
            })
            .unwrap_or_default();
        entries.into_iter().flatten().map(|entry| entry.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn owner_slots_keep_exact_wakes_across_overflow_reuse_and_teardown() {
        let counts: &[usize] = if cfg!(miri) { &[3] } else { &[32, 129] };
        for &count in counts {
            let mut owners = Owners::default();
            for value in 0..count {
                owners.try_reserve(1).unwrap();
                assert_eq!(owners.insert(value, false), value);
            }
            let mut cx = Context::from_waker(Waker::noop());
            assert!(owners.next(&mut cx).is_none());
            let old = owners.waker(count - 1).clone();
            let allocations = allocation_counter::measure(|| old.wake_by_ref());
            assert_eq!(allocations.count_total, 0);
            let ready = owners.next(&mut cx).unwrap();
            assert_eq!(ready.index(), count - 1);
            assert!(owners.next(&mut cx).is_none());
            assert_eq!(owners.take(ready.index()), count - 1);
            owners.try_reserve(1).unwrap();
            assert_eq!(owners.insert(count, false), count - 1);
            old.wake_by_ref();
            drop(ready);
            assert!(
                owners.next(&mut cx).is_none(),
                "retired poll woke replacement"
            );
            owners.waker(count - 1).wake_by_ref();
            assert_eq!(owners.next(&mut cx).unwrap().index(), count - 1);
            let new = owners.waker(count - 1).clone();
            let drained = owners.drain().collect::<Vec<_>>();
            assert_eq!(drained.len(), count);
            assert_eq!(*drained.last().unwrap(), count);
            old.wake();
            new.wake();
            assert!(owners.is_empty());
            assert!(owners.next(&mut cx).is_none());
        }
    }

    #[test]
    fn ownership_drain_disconnects_notifiers_before_reentrant_value_destruction() {
        struct Value(Waker, Rc<Cell<usize>>);
        impl Drop for Value {
            fn drop(&mut self) {
                self.0.wake_by_ref();
                self.1.set(self.1.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let mut owners = Owners::default();
        for _ in 0..4 {
            owners.try_reserve(1).unwrap();
            let index = owners.insert(Value(Waker::noop().clone(), drops.clone()), true);
            owners.get_mut(index).0 .0 = owners.waker(index).clone();
        }
        let drained = owners.drain();
        assert_eq!(drops.get(), 0, "draining silently destroyed affine owners");
        drop(drained);
        assert_eq!(drops.get(), 4);
        assert!(owners
            .next(&mut Context::from_waker(Waker::noop()))
            .is_none());
    }

    #[test]
    fn unused_owner_storage_allocates_nothing() {
        let measured = allocation_counter::measure(|| {
            let mut owners = Owners::<usize>::default();
            assert!(owners.is_empty());
            assert!(owners
                .next(&mut Context::from_waker(Waker::noop()))
                .is_none());
            assert_eq!(owners.drain().count(), 0);
        });
        assert_eq!(measured.count_total, 0);
    }
}

//! Pre-publication operation identity, independent of dispatch and wire IDs.
//! An operation keeps its observer list while a dispatch slot is reused. The
//! exact generation authorizes completion bindings and local result publication.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvocationKey<'id> {
    index: usize,
    generation: u64,
    brand: core::marker::PhantomData<fn(&'id ()) -> &'id ()>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State<'id> {
    Queued,
    Running,
    Sent(questions::Key<'id>),
    Completed,
    Canceled,
}
pub(super) struct Operation<'id> {
    pub task: dispatch::TaskKey<'id>,
    pub state: State<'id>,
    pub observers: Option<usize>,
    pub observer_count: usize,
    pub binding: Option<usize>,
    pub completion_owner: Option<core::num::NonZeroUsize>,
}
pub struct OperationSlot<'id> {
    generation: u64,
    entry: Option<Operation<'id>>,
    changed: work_queue::Link,
}
impl OperationSlot<'_> {
    pub const EMPTY: Self = Self {
        generation: 0,
        entry: None,
        changed: work_queue::Link::EMPTY,
    };
}
pub(super) struct Operations<'s, 'id> {
    slots: SlotStorage<'s, OperationSlot<'id>>,
    live: usize,
    changed: work_queue::Queue,
}
impl<'s, 'id> Operations<'s, 'id> {
    pub fn new(slots: SlotStorage<'s, OperationSlot<'id>>) -> Self {
        assert!(slots.iter().all(|slot| slot.entry.is_none()));
        Self {
            slots,
            live: 0,
            changed: work_queue::Queue::EMPTY,
        }
    }
    pub fn reserve(&mut self) -> Result<InvocationKey<'id>, Error> {
        let index = self
            .slots
            .vacant(
                |_, slot| slot.entry.is_none() && slot.generation != u64::MAX,
                || OperationSlot::EMPTY,
            )
            .ok_or(Error::Busy)?;
        Ok(InvocationKey {
            index,
            generation: self.slots[index].generation + 1,
            brand: core::marker::PhantomData,
        })
    }
    pub fn insert(&mut self, key: InvocationKey<'id>, task: dispatch::TaskKey<'id>) {
        let slot = &mut self.slots[key.index];
        assert!(slot.entry.is_none() && slot.generation + 1 == key.generation);
        slot.generation = key.generation;
        slot.entry = Some(Operation {
            task,
            state: State::Queued,
            observers: None,
            observer_count: 0,
            binding: None,
            completion_owner: None,
        });
        self.live += 1;
    }
    pub fn get(&self, key: InvocationKey<'id>) -> Result<&Operation<'id>, Error> {
        self.slots
            .get(key.index)
            .filter(|slot| slot.generation == key.generation)
            .and_then(|slot| slot.entry.as_ref())
            .ok_or(Error::UnknownInvocation)
    }
    pub fn get_mut(&mut self, key: InvocationKey<'id>) -> Result<&mut Operation<'id>, Error> {
        if self.get(key)?.completion_owner.is_some() {
            self.changed
                .push(&mut self.slots, key.index, |slot| &mut slot.changed);
        }
        self.slots
            .get_mut(key.index)
            .filter(|slot| slot.generation == key.generation)
            .and_then(|slot| slot.entry.as_mut())
            .ok_or(Error::UnknownInvocation)
    }
    pub fn retire(&mut self, key: InvocationKey<'id>) {
        if self.get(key).is_ok_and(|entry| {
            entry.observers.is_none()
                && entry.binding.is_none()
                && !matches!(entry.state, State::Queued | State::Running)
        }) {
            self.changed
                .remove(&mut self.slots, key.index, |slot| &mut slot.changed);
            self.slots[key.index].entry = None;
            self.live -= 1;
        }
    }
    pub fn unbind(&mut self, key: InvocationKey<'id>) {
        if let Ok(entry) = self.get_mut(key) {
            entry.binding = None;
        }
        self.retire(key);
    }
    pub fn clear(&mut self) {
        for slot in &mut *self.slots {
            slot.entry = None;
            slot.changed = work_queue::Link::EMPTY;
        }
        self.changed = work_queue::Queue::EMPTY;
        self.live = 0;
    }
}
impl Drop for Operations<'_, '_> {
    fn drop(&mut self) {
        self.clear();
    }
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn invocation_completion_owner(&self, key: InvocationKey<'id>) -> Option<usize> {
        self.operations
            .get(key)
            .ok()?
            .completion_owner
            .map(|owner| owner.get() - 1)
    }
    pub fn has_changed_invocations(&self) -> bool {
        !self.operations.changed.is_empty()
    }
    pub fn take_changed_invocation(&mut self) -> Option<(InvocationKey<'id>, usize)> {
        let index = self
            .operations
            .changed
            .pop(&mut self.operations.slots, |slot| &mut slot.changed)?;
        let slot = &self.operations.slots[index];
        let operation = slot.entry.as_ref().expect("changed invocation");
        Some((
            InvocationKey {
                index,
                generation: slot.generation,
                brand: core::marker::PhantomData,
            },
            operation.completion_owner.expect("bound operation").get() - 1,
        ))
    }
    /// The one registered completion slot for this exact operation generation.
    pub fn invocation_binding(&self, key: InvocationKey<'id>) -> Option<usize> {
        self.operations.get(key).ok()?.binding
    }
    /// The wire question published by an invocation, while its owner is live.
    pub fn invocation_question(&self, key: InvocationKey<'id>) -> Option<questions::Key<'id>> {
        match self.operations.get(key).ok()?.state {
            operations::State::Sent(question) => Some(question),
            _ => None,
        }
    }
    pub(super) fn queued_invocation(
        &self,
        key: InvocationKey<'id>,
    ) -> Result<&Invocation<'pool, 'id, CAPS>, Error> {
        let entry = self.operations.get(key)?;
        let Some(Task::Invoke(invocation)) = self.runtime.queued_owner(entry.task) else {
            return Err(Error::UnknownInvocation);
        };
        if invocation.key != key {
            return Err(Error::UnknownInvocation);
        }
        Ok(invocation)
    }
}

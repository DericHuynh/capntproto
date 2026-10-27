//! Receiver-answer descriptors and a Call's target retain one atomic batch.
use super::*;
pub struct PipelineSlot<'id> {
    wire: Option<AnswerId>,
    pipeline: Option<Pipeline<'id>>,
    plan: Option<Key<'id>>,
}
impl<'id> PipelineSlot<'id> {
    pub const fn none() -> Self {
        Self {
            wire: None,
            pipeline: None,
            plan: None,
        }
    }
    pub fn new(id: AnswerId) -> Self {
        Self {
            wire: Some(id),
            ..Self::none()
        }
    }
    pub fn id(&self) -> Option<AnswerId> {
        self.wire
    }
    pub fn pipeline(&self) -> Option<&Pipeline<'id>> {
        self.pipeline.as_ref()
    }
    pub fn take(&mut self) -> Option<Pipeline<'id>> {
        self.pipeline.take()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineError {
    Answer(Error),
    Occupied,
}
impl From<Error> for PipelineError {
    fn from(e: Error) -> Self {
        Self::Answer(e)
    }
}
impl core::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "answer pipeline batch: {self:?}")
    }
}
impl core::error::Error for PipelineError {}

#[must_use]
pub struct Pipelines<'a, 'receipts, 'id, 'storage, T> {
    answers: &'a mut Answers<'id, 'storage, T>,
    receipts: &'receipts mut [PipelineSlot<'id>],
}
impl<'id, 'storage, T> Answers<'id, 'storage, T> {
    pub fn prepare_pipelines<'a, 'receipts>(
        &'a mut self,
        receipts: &'receipts mut [PipelineSlot<'id>],
    ) -> Result<Pipelines<'a, 'receipts, 'id, 'storage, T>, PipelineError> {
        let mut prepared = Pipelines {
            answers: self,
            receipts,
        };
        prepared.plan()?;
        Ok(prepared)
    }
    pub fn prepare_admission_with_pipelines<'a, 'receipts>(
        &'a mut self,
        id: AnswerId,
        receipts: &'receipts mut [PipelineSlot<'id>],
    ) -> Result<PipelineAdmission<'a, 'receipts, 'id, 'storage, T>, PipelineError> {
        let admission = self.prepare_admission(id, None)?;
        let key = admission.key;
        let pending = admission.answers.prepare_pipelines(receipts)?;
        Ok(PipelineAdmission {
            pending,
            key,
            redirect: Redirect::None,
            allow_handoff: false,
        })
    }
}
impl<'receipts, 'id, T> Pipelines<'_, 'receipts, 'id, '_, T> {
    fn plan(&mut self) -> Result<(), PipelineError> {
        for receipt in self.receipts.iter_mut() {
            receipt.plan = None;
        }
        for position in 0..self.receipts.len() {
            let (before, current) = self.receipts.split_at_mut(position);
            let current = &mut current[0];
            if current.pipeline.is_some() {
                return Err(PipelineError::Occupied);
            }
            let Some(id) = current.wire else {
                continue;
            };
            let key = self.answers.lookup(id).ok_or(Error::StaleKey)?;
            if self.answers.record(key)?.provision {
                return Err(Error::NotCallable.into());
            }
            let count = before
                .iter()
                .filter(|r| r.plan == Some(key))
                .count()
                .checked_add(1)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(Error::ReferenceLimit)?;
            self.answers
                .record(key)?
                .pipelines
                .checked_add(count)
                .ok_or(Error::ReferenceLimit)?;
            current.plan = Some(key);
        }
        Ok(())
    }
    pub fn accept(mut self) -> &'receipts mut [PipelineSlot<'id>] {
        for receipt in self.receipts.iter_mut() {
            if let Some(key) = receipt.plan.take() {
                self.answers
                    .record_mut(key)
                    .expect("preflighted pipeline")
                    .pipelines += 1;
                receipt.pipeline = Some(Pipeline { key });
                receipt.wire = None;
            }
        }
        core::mem::take(&mut self.receipts)
    }
}
impl<T> Drop for Pipelines<'_, '_, '_, '_, T> {
    fn drop(&mut self) {
        for receipt in self.receipts.iter_mut() {
            receipt.plan = None;
        }
    }
}

#[must_use]
pub struct PipelineAdmission<'a, 'receipts, 'id, 'storage, T> {
    pending: Pipelines<'a, 'receipts, 'id, 'storage, T>,
    key: Key<'id>,
    redirect: Redirect,
    allow_handoff: bool,
}
impl<'receipts, 'id, T> PipelineAdmission<'_, 'receipts, 'id, '_, T> {
    /// Permit a network handoff only when the immutable incoming Call opted in.
    pub fn allow_handoff(mut self, allowed: bool) -> Self {
        self.allow_handoff = allowed;
        self
    }
    /// A third-party destination is never available for local tail adoption.
    pub fn third_party(mut self) -> Self {
        self.redirect = Redirect::ThirdParty;
        self
    }
    pub fn redirected(mut self) -> Self {
        self.redirect = Redirect::Available;
        self
    }
    pub fn accept(self, value: T) -> (Key<'id>, &'receipts mut [PipelineSlot<'id>]) {
        self.accept_with(value, false)
    }
    pub(super) fn accept_with(
        self,
        value: T,
        provision: bool,
    ) -> (Key<'id>, &'receipts mut [PipelineSlot<'id>]) {
        let reserved = self
            .pending
            .answers
            .free
            .take(&mut self.pending.answers.slots, |slot| &mut slot.free);
        debug_assert_eq!(reserved, self.key.index);
        self.pending.answers.displaced +=
            usize::from(usize::try_from(self.key.id.to_wire()).ok() != Some(self.key.index));
        self.pending.answers.generation = self.key.generation;
        self.pending.answers.slots[self.key.index].record = Some(Record {
            #[cfg(feature = "alloc")]
            presence: self.pending.answers.observation.enter(),
            id: self.key.id,
            generation: self.key.generation,
            state: if provision {
                State::Returned
            } else {
                State::Queued
            },
            finished: false,
            pipelines: 0,
            redirect: self.redirect,
            allow_handoff: self.allow_handoff,
            provision,
            value,
        });
        (self.key, self.pending.accept())
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::ids::WireId;
    #[kani::proof]
    #[kani::unwind(4)]
    fn receiver_answer_batch_retains_every_dependency() {
        let count: u32 = kani::any();
        let generation: u64 = kani::any();
        kani::assume(generation != 0);
        let parent = AnswerId::from_wire(17);
        let mut slots = [
            Slot {
                free: crate::free_slots::Link::EMPTY,
                retirement: crate::work_queue::Link::EMPTY,
                pipeline_work: crate::work_queue::Link::EMPTY,
                cancellation: crate::work_queue::Link::EMPTY,
                execution: crate::work_queue::Link::EMPTY,
                executing: false,
                pipeline_head: None,
                pipeline_cursor: None,
                record: Some(Record {
                    #[cfg(feature = "alloc")]
                    presence: Default::default(),
                    id: parent,
                    generation: 1,
                    state: State::Queued,
                    finished: false,
                    pipelines: count,
                    redirect: Redirect::None,
                    allow_handoff: false,
                    provision: false,
                    value: 41u8,
                }),
            },
            Slot::EMPTY,
        ];
        let mut answers = Answers {
            displaced: slots
                .iter()
                .enumerate()
                .filter(|(index, slot)| {
                    slot.record.as_ref().is_some_and(|entry| {
                        usize::try_from(entry.id.to_wire()).ok() != Some(*index)
                    })
                })
                .count(),
            free: crate::free_slots::FreeSlots::from_storage(
                &mut slots,
                |slot| slot.record.is_none(),
                |slot| &mut slot.free,
            ),
            retired: crate::work_queue::Queue::EMPTY,
            pipeline_ready: crate::work_queue::Queue::EMPTY,
            pipeline_registry: None,
            pipeline_registry_generation: 0,
            pipeline_observers: 0,
            canceled: crate::work_queue::Queue::EMPTY,
            execution_ready: crate::work_queue::Queue::EMPTY,
            #[cfg(feature = "alloc")]
            observation: Default::default(),
            slots: crate::storage::Storage::borrowed(&mut slots),
            generation,
            brand: PhantomData,
        };
        let mut receipts = [PipelineSlot::new(parent), PipelineSlot::new(parent)];
        let accepted = match answers
            .prepare_admission_with_pipelines(AnswerId::from_wire(19), &mut receipts)
        {
            Ok(pending) => {
                assert!(generation < u64::MAX && count <= u32::MAX - 2);
                if kani::any() {
                    pending.accept(47);
                    true
                } else {
                    drop(pending);
                    false
                }
            }
            Err(_) => {
                assert!(generation == u64::MAX || count > u32::MAX - 2);
                false
            }
        };
        kani::assert(
            answers.slots[0].record.as_ref().unwrap().pipelines
                == count + if accepted { 2 } else { 0 },
            "all receiver-answer dependencies are retained atomically",
        );
        assert_eq!(answers.slots[1].record.is_some(), accepted);
        assert_eq!(answers.generation, generation + u64::from(accepted));
        assert_eq!(receipts[0].pipeline().is_some(), accepted);
        assert_eq!(receipts[1].pipeline().is_some(), accepted);
    }
}

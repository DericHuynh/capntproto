//! Pipeline-only questions have bounded live storage and a non-wrapping ID space.
use super::*;

/// Bind ordinary and pipeline-only questions to separate caller-owned storage.
/// Pipeline-only IDs never wrap; exhaustion falls back to ordinary questions.
pub fn with_pipeline_questions<'s, R>(
    ordinary: &'s mut [Slot],
    pipeline: &'s mut [PipelineSlot],
    f: impl for<'id> FnOnce(&mut Questions<'id, 's>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope_with_adoptions(
        ordinary,
        pipeline,
        &mut [],
    )?))
}

impl<'id, 's> Questions<'id, 's> {
    /// Recognize late Returns without retaining unbounded tombstones. High IDs
    /// are never reused; even an unpublished gap is harmless to discard.
    pub fn pipeline_id_was_issued(&self, id: QuestionId) -> bool {
        id.is_pipeline_only() && u64::from(id.to_wire()) < self.next_pipeline_id
    }

    /// None requests an ordinary-question fallback: the bounded high storage
    /// is full/disabled, or all 2^31 high IDs have been published. A rejected
    /// reservation preserves its last unused ID for retry.
    pub fn reserve_pipeline_only(&mut self) -> Result<Option<Reservation<'_, 'id, 's>>, Error> {
        let key = self.insert_pipeline_only()?;
        Ok(key.map(|key| Reservation {
            questions: self,
            key,
            committed: false,
        }))
    }

    /// Select the ordinary protocol when high-ID storage or IDs are exhausted.
    /// The publisher must set onlyPromisePipeline exactly for high reservations.
    pub fn reserve_for_pipeline(&mut self) -> Result<Reservation<'_, 'id, 's>, Error> {
        match self.insert_pipeline_only()? {
            Some(key) => Ok(Reservation {
                questions: self,
                key,
                committed: false,
            }),
            None => self.reserve(),
        }
    }

    fn insert_pipeline_only(&mut self) -> Result<Option<Key<'id>>, Error> {
        let Ok(raw) = u32::try_from(self.next_pipeline_id) else {
            return Ok(None);
        };
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        let id = QuestionId::from_wire(raw);
        let record = Record {
            #[cfg(feature = "alloc")]
            presence: self.observation.enter(),
            #[cfg(feature = "alloc")]
            credit: None,
            id,
            generation,
            references: 1,
            lifecycle: Question::no_return(),
            finish: None,
            finish_next: None,
            published: false,
            destination: ResultsTo::Caller,
            provision: false,
            owner: None,
            observers: [0; 2],
            observer_heads: [None; 2],
            observer_work: [None; 2],
            observer_notified: [false; 2],
        };
        if self.table.try_insert_high(id, record).is_err() {
            return Ok(None);
        }
        self.next_pipeline_id += 1;
        self.generation = generation;
        Ok(Some(Key {
            id,
            generation,
            brand: PhantomData,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_ids_do_not_wrap_or_wait_for_return_after_finish() {
        let mut ordinary = [Slot::EMPTY];
        let mut high = [PipelineSlot::EMPTY];
        with_pipeline_questions(&mut ordinary, &mut high, |questions| {
            questions.next_pipeline_id = u64::from(u32::MAX) - 1;
            for expected in [u32::MAX - 1, u32::MAX] {
                let pending = questions.reserve_pipeline_only().unwrap().unwrap();
                assert_eq!(pending.id().to_wire(), expected);
                let local = pending.accept();
                let key = local.key();
                assert_eq!(questions.pipeline_id(&local).unwrap().to_wire(), expected);
                let retained = questions.retain(&local).unwrap();
                questions.release(local);
                assert!(questions.pending_finish().is_none());
                questions.release(retained);
                {
                    let finish = questions.pending_finish().unwrap();
                    assert_eq!(finish.id().to_wire(), expected);
                    assert!(finish.release_result_caps());
                }
                assert!(
                    questions.contains(key),
                    "rejected Finish retains the exact generation"
                );
                assert!(questions.reserve_pipeline_only().unwrap().is_none());
                questions.pending_finish().unwrap().accept();
                assert!(questions.is_empty(), "no Return is required");
                assert!(!questions.contains(key));
            }
            for _ in 0..3 {
                assert!(
                    questions.reserve_pipeline_only().unwrap().is_none(),
                    "exhaustion must not wrap to 0x80000000"
                );
                let pending = questions.reserve_for_pipeline().unwrap();
                assert_eq!(pending.id().to_wire(), 0);
                drop(pending);
            }
        })
        .unwrap();
    }

    #[test]
    fn rejected_last_high_id_can_be_retried_but_published_id_cannot() {
        let mut high = [PipelineSlot::EMPTY];
        with_pipeline_questions(&mut [], &mut high, |questions| {
            questions.next_pipeline_id = u64::from(u32::MAX);
            let pending = questions.reserve_pipeline_only().unwrap().unwrap();
            assert_eq!(pending.id().to_wire(), u32::MAX);
            drop(pending);
            assert!(questions.is_empty());
            let local = questions.reserve_pipeline_only().unwrap().unwrap().accept();
            assert_eq!(local.key().id().to_wire(), u32::MAX);
            questions.release(local);
            questions.pending_finish().unwrap().accept();
            assert!(questions.reserve_pipeline_only().unwrap().is_none());
        })
        .unwrap();
    }

    #[test]
    fn rollback_cannot_rewind_past_a_nested_published_high_id() {
        let mut high = [const { PipelineSlot::EMPTY }; 2];
        with_pipeline_questions(&mut [], &mut high, |questions| {
            questions.next_pipeline_id = u64::from(u32::MAX) - 1;
            let mut first = questions.reserve_pipeline_only().unwrap().unwrap();
            let second = first
                .ledger()
                .reserve_pipeline_only()
                .unwrap()
                .unwrap()
                .accept();
            drop(first);
            assert_eq!(questions.len(), 1);
            assert!(questions.reserve_pipeline_only().unwrap().is_none());
            questions.release(second);
            questions.pending_finish().unwrap().accept();
            assert!(questions.reserve_pipeline_only().unwrap().is_none());
            assert!(questions.is_empty());
        })
        .unwrap();
    }

    #[test]
    fn high_capacity_pressure_preserves_cursor_and_ordinary_capacity() {
        let mut ordinary = [Slot::EMPTY];
        let mut high = [PipelineSlot::EMPTY];
        with_pipeline_questions(&mut ordinary, &mut high, |questions| {
            let first = questions.reserve_pipeline_only().unwrap().unwrap().accept();
            assert_eq!(first.key().id().to_wire(), 1 << 31);
            assert!(questions.reserve_pipeline_only().unwrap().is_none());
            let normal = questions.reserve_for_pipeline().unwrap();
            assert_eq!(normal.id().to_wire(), 0);
            drop(normal);
            questions.release(first);
            questions.pending_finish().unwrap().accept();
            let second = questions.reserve_pipeline_only().unwrap().unwrap().accept();
            assert_eq!(second.key().id().to_wire(), (1 << 31) + 1);
            questions.release(second);
            questions.pending_finish().unwrap().accept();
        })
        .unwrap();
    }
}

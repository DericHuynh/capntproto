//! Callee-selected question IDs occupy separately budgeted sparse storage.
use super::*;

pub fn with_adoptions<'s, R>(
    ordinary: &'s mut [Slot],
    adopted: &'s mut [AdoptedSlot],
    f: impl for<'id> FnOnce(&mut Questions<'id, 's>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope_with_adoptions(ordinary, adopted)?))
}
/// Incoming ThirdPartyAnswer admission. Immutable envelope validation precedes
/// commit. Authentication may finish later: the owning driver must gate access
/// to the question and its results until the rendezvous matches.
#[must_use]
pub struct AdoptionReservation<'a, 'id, 's> {
    reservation: Reservation<'a, 'id, 's>,
}
impl<'id> AdoptionReservation<'_, 'id, '_> {
    pub fn id(&self) -> QuestionId {
        self.reservation.id()
    }
    pub fn accept(self) -> Local<'id> {
        self.reservation.accept()
    }
}
impl<'id, 's> Questions<'id, 's> {
    pub fn reserve_adoption(
        &mut self,
        id: QuestionId,
    ) -> Result<AdoptionReservation<'_, 'id, 's>, Error> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        self.table
            .try_insert_adopted(
                id,
                Record {
                    #[cfg(feature = "alloc")]
                    presence: self.observation.enter(),
                    #[cfg(feature = "alloc")]
                    credit: None,
                    id,
                    generation,
                    references: 1,
                    lifecycle: Question::new(),
                    finish: None,
                    published: false,
                    destination: ResultsTo::Caller,
                    provision: false,
                },
            )
            .map_err(|rejected| match rejected.kind {
                local_table::InsertErrorKind::InvalidAdoptedId => Error::InvalidAdoptedId,
                local_table::InsertErrorKind::Duplicate => Error::DuplicateAdoption,
                _ => Error::Full,
            })?;
        self.generation = generation;
        Ok(AdoptionReservation {
            reservation: Reservation {
                questions: self,
                key: Key {
                    id,
                    generation,
                    brand: PhantomData,
                },
                committed: false,
            },
        })
    }
}

#[cfg(any(test, kani))]
fn check(id: u32, cancel: bool) {
    let mut ordinary = [Slot::EMPTY];
    let mut adopted = [AdoptedSlot::EMPTY];
    with_adoptions(&mut ordinary, &mut adopted, |questions| {
        check_ledger(questions, id, cancel)
    })
    .unwrap();
}

#[cfg(any(test, kani))]
fn check_ledger(questions: &mut Questions<'_, '_>, id: u32, cancel: bool) {
    use crate::ids::WireId;
    let low = questions.reserve().unwrap().accept();
    assert_eq!(low.key().id().to_wire(), 0);
    for invalid in [0, (1 << 30) - 1, 1 << 31, u32::MAX] {
        assert_eq!(
            questions
                .reserve_adoption(QuestionId::from_wire(invalid))
                .err(),
            Some(Error::InvalidAdoptedId)
        );
    }
    let id = QuestionId::from_wire((id & ((1 << 30) - 1)) | (1 << 30));
    let prepared = questions.reserve_adoption(id).unwrap();
    drop(prepared);
    let adopted = questions.reserve_adoption(id).unwrap().accept();
    let old = adopted.key();
    assert_eq!(
        questions.reserve_adoption(id).err(),
        Some(Error::DuplicateAdoption)
    );
    assert_eq!(
        questions
            .reserve_adoption(QuestionId::from_wire(id.to_wire() ^ 1))
            .err(),
        Some(Error::Full)
    );
    assert_eq!(questions.pipeline_id(&adopted), Ok(id));
    if cancel {
        questions.release(adopted);
        questions.pending_finish().unwrap().accept();
        #[cfg(kani)]
        kani::assert(
            questions.reserve_adoption(id).is_err(),
            "canceled adopted question remains occupied until late Return",
        );
        #[cfg(not(kani))]
        assert!(questions.reserve_adoption(id).is_err());
        assert_eq!(
            questions.returned(id, false).unwrap().1,
            question::Delivery::Discard
        );
    } else {
        assert_eq!(
            questions.returned(id, false).unwrap().1,
            question::Delivery::Deliver
        );
        questions.release(adopted);
        let pending = questions.pending_finish().unwrap();
        assert_eq!(pending.id(), id);
        drop(pending);
        assert!(questions.reserve_adoption(id).is_err());
        questions.pending_finish().unwrap().accept();
    }
    let next = questions.reserve_adoption(id).unwrap().accept();
    #[cfg(kani)]
    kani::assert(
        next.key() != old,
        "reused adopted ID has a new local generation",
    );
    assert_ne!(next.key(), old);
    assert!(questions.contains(low.key()));
    questions.returned(id, true).unwrap();
    questions.release(next);
    questions.returned(low.key().id(), true).unwrap();
    questions.release(low);
    assert!(questions.is_empty());
}
#[cfg(test)]
mod tests {
    #[test]
    fn adopted_question_ranges_and_cleanup_are_independent() {
        let measured = allocation_counter::measure(|| {
            for id in [0, 1, (1 << 30) - 1, u32::MAX] {
                for cancel in [false, true] {
                    super::check(id, cancel)
                }
            }
        });
        assert_eq!(measured.count_total, 0);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn growing_adoptions_replay_fixed_lifecycle_and_reuse_warm_storage() {
        use crate::connection::Growth;
        let bounds = Growth {
            initial: 0,
            maximum: 1,
        };
        super::with_growing_adoptions(bounds, bounds, |questions| {
            super::check_ledger(questions, 0, false);
            let measured = allocation_counter::measure(|| {
                for id in [0, 1, (1 << 30) - 1, u32::MAX] {
                    for cancel in [false, true] {
                        super::check_ledger(questions, id, cancel);
                    }
                }
            });
            assert_eq!(measured.count_total, 0);
        })
        .unwrap();
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn adopted_growth_preserves_live_questions_and_separate_budgets() {
        use super::*;
        use crate::{connection::Growth, ids::WireId};
        let ordinary = Growth {
            initial: 0,
            maximum: 1,
        };
        let adopted = Growth {
            initial: 0,
            maximum: 3,
        };
        with_growing_adoptions(ordinary, adopted, |questions| {
            let low = questions.reserve().unwrap().accept();
            let first = QuestionId::from_wire(1 << 30);
            let last = QuestionId::from_wire((1 << 31) - 1);
            let abandoned = questions.reserve_adoption(first).unwrap().accept();
            let stale = abandoned.key();
            questions.release(abandoned);
            questions.pending_finish().unwrap().accept();
            let second = questions.reserve_adoption(last).unwrap().accept();
            let third = questions
                .reserve_adoption(QuestionId::from_wire(first.to_wire() + 1))
                .unwrap()
                .accept();
            assert!(matches!(
                questions.reserve_adoption(first),
                Err(Error::DuplicateAdoption)
            ));
            assert!(matches!(
                questions.reserve_adoption(QuestionId::from_wire(first.to_wire() + 2)),
                Err(Error::Full)
            ));
            assert!(matches!(questions.reserve(), Err(Error::Full)));
            assert_eq!(questions.len(), 4);
            assert_eq!(
                questions.returned(first, false).unwrap().1,
                question::Delivery::Discard
            );
            let next = questions.reserve_adoption(first).unwrap().accept();
            assert_ne!(next.key(), stale);
            assert!(!questions.contains(stale));
            for local in [low, second, third, next] {
                assert!(questions.is_live(local.key()));
                questions.returned(local.key().id(), true).unwrap();
                questions.release(local);
            }
            assert!(questions.is_empty());
        })
        .unwrap();
        let invalid = Growth {
            initial: 2,
            maximum: 1,
        };
        for (a, b) in [(invalid, adopted), (ordinary, invalid)] {
            assert_eq!(
                with_growing_adoptions(a, b, |_| panic!("invalid capacity")),
                Err(Error::Full)
            );
        }
        let zero = Growth {
            initial: 0,
            maximum: 0,
        };
        with_growing_adoptions(ordinary, zero, |questions| {
            assert!(matches!(
                questions.reserve_adoption(QuestionId::from_wire(1 << 30)),
                Err(Error::Full)
            ));
            assert!(questions.reserve().is_ok());
        })
        .unwrap();
    }
}
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    #[kani::unwind(6)]
    fn adopted_question_cancellation_retains_id_until_return() {
        super::check(kani::any(), kani::any());
    }
}

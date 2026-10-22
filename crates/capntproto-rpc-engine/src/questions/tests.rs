use super::*;
use crate::{ids::WireId, question::Delivery};

#[test]
fn late_return_and_output_rejection_cannot_recycle_a_cancelled_question() {
    for return_first in [false, true] {
        let mut slots = [Slot::EMPTY];
        with_questions(&mut slots, |questions| {
            let local = questions.reserve().unwrap().accept();
            let stale = local.key();
            questions.release(local);
            assert!(matches!(questions.reserve(), Err(Error::Full)));
            if return_first {
                assert_eq!(
                    questions.returned(stale.id(), false),
                    Ok((stale, Delivery::Discard))
                );
            }
            for _ in 0..3 {
                let finish = questions.pending_finish().unwrap();
                assert_eq!(finish.id(), stale.id());
                assert!(finish.release_result_caps());
            }
            assert!(matches!(questions.reserve(), Err(Error::Full)));
            questions.pending_finish().unwrap().accept();
            if !return_first {
                assert!(matches!(questions.reserve(), Err(Error::Full)));
                assert_eq!(
                    questions.returned(stale.id(), false),
                    Ok((stale, Delivery::Discard))
                );
            }
            assert!(questions.is_empty());
            let reused = questions.reserve().unwrap().accept();
            assert_eq!(reused.key().id(), stale.id());
            assert!(!questions.is_live(stale));
            assert!(questions.is_live(reused.key()));
        })
        .unwrap();
        assert!(slots[0].is_empty());
    }
}

#[test]
fn retained_pipelines_delay_finish_and_duplicate_returns_preserve_state() {
    for no_finish in [false, true] {
        let mut slots = [Slot::EMPTY];
        with_questions(&mut slots, |questions| {
            let local = questions.reserve().unwrap().accept();
            let pipeline = questions.retain(&local).unwrap();
            let key = local.key();
            questions.release(local);
            assert!(questions.pending_finish().is_none());
            assert_eq!(
                questions.returned(key.id(), no_finish),
                Ok((key, Delivery::Deliver))
            );
            assert_eq!(
                questions.returned(key.id(), !no_finish),
                Err(Error::DuplicateReturn)
            );
            questions.release(pipeline);
            if !no_finish {
                let finish = questions.pending_finish().unwrap();
                assert!(!finish.release_result_caps());
                finish.accept();
            }
            assert!(questions.is_empty());
        })
        .unwrap();
    }
}

#[test]
fn unsent_question_rollback_exhaustion_and_unknown_ids_do_not_mutate_live_calls() {
    let mut slots = [Slot::EMPTY];
    with_questions(&mut slots, |questions| {
        let reservation = questions.reserve().unwrap();
        let id = reservation.id();
        drop(reservation);
        assert!(questions.is_empty());
        let local = questions.reserve().unwrap().accept();
        assert_eq!(local.key().id(), id);
        assert_eq!(
            questions.returned(QuestionId::from_wire(u32::MAX), true),
            Err(Error::UnknownQuestion)
        );
        questions.record_mut(local.key()).unwrap().references = u32::MAX;
        assert_eq!(questions.retain(&local).unwrap_err(), Error::ReferenceLimit);
        assert_eq!(questions.record(local.key()).unwrap().references, u32::MAX);
        questions.record_mut(local.key()).unwrap().references = 1;
        questions.returned(id, true).unwrap();
        questions.release(local);
        questions.generation = u64::MAX;
        assert!(matches!(
            questions.reserve(),
            Err(Error::GenerationExhausted)
        ));
        assert!(questions.is_empty());
    })
    .unwrap();
}

#[test]
fn no_allocator_is_used_by_call_admission_or_cleanup() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [Slot::EMPTY];
        with_questions(&mut slots, |questions| {
            let local = questions.reserve().unwrap().accept();
            let id = local.key().id();
            questions.release(local);
            questions.pending_finish().unwrap().accept();
            questions.returned(id, false).unwrap();
            assert!(questions.is_empty());
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_questions_do_not_reuse_cancelled_ids_before_late_return() {
    with_growing_questions(0, 3, &mut [], |questions| {
        let first = questions.reserve().unwrap().accept();
        let key = first.key();
        questions.release(first);
        questions.pending_finish().unwrap().accept();
        let second = questions.reserve().unwrap().accept();
        let third = questions.reserve().unwrap().accept();
        assert_eq!(second.key().id().to_wire(), 1);
        assert_eq!(third.key().id().to_wire(), 2);
        assert!(matches!(questions.reserve(), Err(Error::Full)));
        assert_eq!(
            questions.returned(key.id(), false),
            Ok((key, Delivery::Discard))
        );
        let new = questions.reserve().unwrap().accept();
        assert_eq!(new.key().id(), key.id());
        assert!(!questions.contains(key));
        for local in [second, third, new] {
            questions.returned(local.key().id(), true).unwrap();
            questions.release(local);
        }
        assert!(questions.is_empty());
    })
    .unwrap();
}

#[cfg(feature = "alloc")]
#[test]
fn outgoing_credit_survives_cancellation_late_return_and_stale_generation() {
    use crate::flow_control::outgoing::Outgoing;
    use alloc::rc::Rc;
    for return_first in [false, true] {
        let counter = Rc::new(Outgoing::new(1));
        let mut slots = [Slot::EMPTY];
        let observed = with_questions(&mut slots, |questions| {
            let local = questions.reserve().unwrap().accept();
            let old = local.key();
            let observation = questions.observe_len(); // Attach to an existing record.
            let credit = Rc::new(counter.reserve_owned().unwrap());
            questions.retain_credit(old, credit.clone()).unwrap();
            questions.retain_credit(old, credit.clone()).unwrap();
            drop(credit);
            questions.release(local);
            if return_first {
                questions.returned(old.id(), false).unwrap();
            }
            assert_eq!(observation.get(), 1);
            assert!(counter.reserve().is_err());
            questions.pending_finish().unwrap().accept();
            if !return_first {
                assert!(counter.reserve().is_err());
                questions.returned(old.id(), false).unwrap();
            }
            assert_eq!(counter.used(), 0);
            assert_eq!(observation.get(), 0);
            let new = questions.reserve().unwrap().accept();
            let credit = Rc::new(counter.reserve_owned().unwrap());
            assert_eq!(
                questions.retain_credit(old, credit.clone()),
                Err(Error::StaleKey)
            );
            questions.retain_credit(new.key(), credit).unwrap();
            assert_eq!(observation.get(), 1);
            observation
        })
        .unwrap();
        assert_eq!(counter.used(), 0);
        assert_eq!(observed.get(), 0);
    }
}

#[test]
fn combined_return_release_preserves_other_observers_and_finish_backpressure() {
    for no_finish in [false, true] {
        for observers in 0..4 {
            let mut slots = [Slot::EMPTY];
            with_questions(&mut slots, |questions| {
                let local = questions.reserve().unwrap().accept();
                let key = local.key();
                let mut others = [None, None, None];
                for owner in others.iter_mut().take(observers) {
                    *owner = Some(questions.retain(&local).unwrap());
                }
                assert_eq!(
                    questions
                        .prepare_receive_owned(key, no_finish)
                        .unwrap()
                        .accept_releasing_owner(local)
                        .unwrap(),
                    (key, Delivery::Deliver)
                );
                if observers != 0 {
                    assert!(questions.contains(key));
                    assert!(!questions.has_pending_finish());
                    assert_eq!(
                        questions.returned(key.id(), no_finish),
                        Err(Error::DuplicateReturn)
                    );
                    for owner in others.into_iter().flatten() {
                        questions.release(owner);
                    }
                }
                if !no_finish {
                    assert!(matches!(questions.reserve(), Err(Error::Full)));
                    assert!(!questions.pending_finish().unwrap().release_result_caps());
                    // Dropping an unaccepted Finish keeps the ID occupied.
                    assert!(matches!(questions.reserve(), Err(Error::Full)));
                    questions.pending_finish().unwrap().accept();
                }
                assert!(questions.is_empty());
                let reused = questions.reserve().unwrap().accept();
                assert_eq!(reused.key().id(), key.id());
                assert_ne!(reused.key(), key);
                questions
                    .prepare_receive_owned(reused.key(), true)
                    .unwrap()
                    .accept_releasing_owner(reused)
                    .unwrap();
            })
            .unwrap();
        }
    }
}

#[test]
fn combined_return_rejects_another_questions_owner_transactionally() {
    let mut slots = [Slot::EMPTY, Slot::EMPTY];
    with_questions(&mut slots, |questions| {
        let first = questions.reserve().unwrap().accept();
        let second = questions.reserve().unwrap().accept();
        let first_key = first.key();
        let second_key = second.key();
        let (error, second) = questions
            .prepare_receive_owned(first_key, true)
            .unwrap()
            .accept_releasing_owner(second)
            .unwrap_err();
        assert_eq!(error, Error::StaleKey);
        assert!(questions.contains(first_key));
        assert!(questions.contains(second_key));
        questions
            .prepare_receive_owned(first_key, true)
            .unwrap()
            .accept_releasing_owner(first)
            .unwrap();
        questions
            .prepare_receive_owned(second_key, true)
            .unwrap()
            .accept_releasing_owner(second)
            .unwrap();
        assert!(questions.is_empty());
    })
    .unwrap();
}

#[test]
fn adapter_owner_pins_generation_and_stale_release_cannot_clear_reuse() {
    let mut slots = [Slot::EMPTY];
    with_questions(&mut slots, |questions| {
        let first = questions.reserve().unwrap().accept();
        let key = first.key();
        questions.bind_owner(key, 7).unwrap();
        assert_eq!(questions.bind_owner(key, 9), Err(Error::AlreadyOwned));
        assert_eq!(questions.owner_for_id(key.id()), Some((key, 7)));
        assert_eq!(
            questions.owner_for_id(crate::ids::QuestionId::from_wire(u32::MAX)),
            None
        );
        questions.release(first);
        questions.pending_finish().unwrap().accept();
        questions.returned(key.id(), true).unwrap();
        assert_eq!(questions.reserve().err(), Some(Error::Full));
        assert_eq!(questions.owner(key), Some(7));
        questions.release_owner(key);
        assert!(questions.is_empty());
        let next = questions.reserve().unwrap().accept();
        let next_key = next.key();
        assert_ne!(key, next_key);
        questions.bind_owner(next_key, 9).unwrap();
        questions.release_owner(key);
        assert_eq!(questions.owner(next_key), Some(9));
        questions
            .prepare_receive_owned(next_key, true)
            .unwrap()
            .accept_releasing_owner(next)
            .unwrap();
        assert!(questions.is_empty());
    })
    .unwrap();
}

#[test]
fn weak_observer_counts_are_separate_and_cannot_cross_question_generations() {
    let mut slots = [Slot::EMPTY];
    with_questions(&mut slots, |questions| {
        let first = questions.reserve().unwrap().accept();
        let old = first.key();
        questions.add_observer(old, false);
        questions.add_observer(old, false);
        questions.add_observer(old, true);
        questions.remove_observer(old, false);
        assert!(questions.has_observers(old, false));
        questions.remove_observer(old, false);
        assert!(!questions.has_observers(old, false));
        assert!(questions.has_observers(old, true));
        questions
            .prepare_receive_owned(old, true)
            .unwrap()
            .accept_releasing_owner(first)
            .unwrap();
        assert!(
            questions.is_empty(),
            "weak observers do not retain authority"
        );
        let next = questions.reserve().unwrap().accept();
        let key = next.key();
        assert_eq!(old.id(), key.id());
        assert_ne!(old, key);
        questions.add_observer(key, true);
        questions.remove_observer(old, true);
        assert!(!questions.has_observers(old, true));
        assert!(questions.has_observers(key, true));
        questions.remove_observer(key, true);
        assert!(!questions.has_observers(key, true));
        questions
            .prepare_receive_owned(key, true)
            .unwrap()
            .accept_releasing_owner(next)
            .unwrap();
    })
    .unwrap();
}

#[test]
fn finish_queue_preserves_sparse_retirement_order_and_rejected_output() {
    let mut slots = [const { Slot::EMPTY }; 64];
    with_questions(&mut slots, |questions| {
        let mut owners: [Option<_>; 64] =
            core::array::from_fn(|_| Some(questions.reserve().unwrap().accept()));
        let first = owners[63].take().unwrap();
        let first_id = first.key().id();
        let second = owners[1].take().unwrap();
        let second_id = second.key().id();
        questions.release(first);
        questions.release(second);
        assert_eq!(questions.pending_finish().unwrap().id(), first_id);
        questions.returned(first_id, true).unwrap();
        let pending = questions.pending_finish().unwrap();
        assert_eq!(pending.id(), first_id);
        pending.accept();
        let pending = questions.pending_finish().unwrap();
        assert_eq!(pending.id(), second_id);
        pending.accept();
        questions.returned(second_id, true).unwrap();
        assert!(!questions.has_pending_finish());
        for owner in owners.into_iter().flatten() {
            questions.returned(owner.key().id(), true).unwrap();
            questions.release(owner);
        }
        assert!(questions.is_empty());
    })
    .unwrap();
}

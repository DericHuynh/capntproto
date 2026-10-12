use super::*;
use crate::ids::WireId;

#[test]
fn no_finish_is_transactional_and_preserves_admitted_pipeline_owners() {
    let mut slots = [Slot::EMPTY, Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let id = AnswerId::from_wire(7);
        let key = answers.admit(id, 41).unwrap();
        let pipeline = answers.pipeline(id).unwrap();
        let mut rejected = answers.prepare_return(key).unwrap();
        rejected.omit_finish().unwrap();
        drop(rejected);
        assert_eq!(
            answers.lookup(id),
            Some(key),
            "unpublished hint has no effects"
        );
        let mut accepted = answers.prepare_return(key).unwrap();
        accepted.omit_finish().unwrap();
        assert!(accepted.accept().is_none());
        assert_eq!(answers.lookup(id), None);
        assert_eq!(answers.pipelined(&pipeline), &41);
        assert!(answers.finish(id, true).retired.is_none());
        let next = answers.admit(id, 42).unwrap();
        assert_eq!(answers.release_pipeline(pipeline), Some(41));
        assert_eq!(answers.get(next), Ok(&42));
        assert!(answers.finish(id, false).cancel);
        assert_eq!(answers.prepare_return(next).unwrap().accept(), Some(42));
        assert!(answers.is_empty());
    })
    .unwrap();
}

#[test]
fn redirects_callee_ids_and_cancellation_keep_explicit_finish() {
    for (id, redirected, canceled) in [
        (7, true, false),
        (1 << 30, false, false),
        (1 << 31, false, false),
        (7, false, true),
    ] {
        let mut slots = [Slot::EMPTY];
        with_answers(&mut slots, |answers| {
            let id = AnswerId::from_wire(id);
            let admission = answers.prepare_admission(id, None).unwrap();
            let admission = if redirected {
                admission.redirected()
            } else {
                admission
            };
            let (key, _) = admission.accept(41);
            if canceled {
                answers.finish(id, false);
            }
            let mut pending = answers.prepare_return(key).unwrap();
            assert!(!pending.can_omit_finish());
            assert_eq!(pending.omit_finish(), Err(Error::FinishRequired));
        })
        .unwrap();
    }
}
#[test]
fn finish_does_not_drop_an_answer_still_needed_by_admitted_pipeline_calls() {
    let mut slots = [Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let id = AnswerId::from_wire(0);
        let key = answers.admit(id, 7).unwrap();
        let first = answers.pipeline(id).unwrap();
        let second = answers.pipeline(id).unwrap();
        assert!(answers.dispatch(key, false).unwrap());
        assert!(answers.finish(id, false).retired.is_none());
        assert!(answers.lookup(id).is_none());
        assert_eq!(answers.pipeline(id).unwrap_err(), Error::Finished);
        assert!(answers.prepare_return(key).unwrap().accept().is_none());
        assert!(answers.release_pipeline(first).is_none());
        assert_eq!(answers.pipelined(&second), &7);
        assert_eq!(answers.admit(id, 9).unwrap_err().error, Error::Full);
        assert_eq!(answers.release_pipeline(second), Some(7));
        assert!(answers.is_empty());
    })
    .unwrap();
}
#[test]
fn peer_can_reuse_finished_answer_id_while_old_pipeline_keeps_its_own_owner() {
    let mut slots = [Slot::EMPTY, Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let id = AnswerId::from_wire(u32::MAX);
        let old = answers.admit(id, 7).unwrap();
        let pipeline = answers.pipeline(id).unwrap();
        assert!(!answers.finish(id, false).cancel);
        assert!(answers.dispatch(old, true).unwrap());
        assert!(answers.prepare_return(old).unwrap().accept().is_none());
        let new = answers.admit(id, 9).unwrap();
        assert_eq!(answers.lookup(id), Some(new));
        assert_eq!(answers.pipelined(&pipeline), &7);
        assert_eq!(answers.get(new), Ok(&9));
        assert_eq!(answers.release_pipeline(pipeline), Some(7));
        assert_eq!(answers.get(old), Err(Error::StaleKey));
        assert_eq!(answers.get(new), Ok(&9));
        assert!(answers.finish(id, true).retired.is_none());
        assert_eq!(answers.prepare_return(new).unwrap().accept(), Some(9));
        assert!(answers.is_empty());
    })
    .unwrap();
}
#[test]
fn pipelined_results_suppress_execution_cancellation_until_last_dependency_releases() {
    let mut slots = [Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let id = AnswerId::from_wire(0);
        let key = answers.admit(id, 7).unwrap();
        let pipeline = answers.pipeline(id).unwrap();
        answers.dispatch(key, true).unwrap();
        assert!(!answers.finish(id, false).cancel);
        assert!(!answers.cancellation_requested(key).unwrap());
        assert_eq!(answers.release_pipeline(pipeline), None);
        assert!(answers.cancellation_requested(key).unwrap());
        assert_eq!(answers.prepare_return(key).unwrap().accept(), Some(7));
    })
    .unwrap();
}
#[test]
fn finish_before_dispatch_suppresses_application_but_retains_return_obligation() {
    let mut slots = [Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let id = AnswerId::from_wire(u32::MAX);
        let key = answers.admit(id, 7).unwrap();
        let finished = answers.finish(id, true);
        assert!(finished.cancel && finished.release_result_caps && finished.retired.is_none());
        assert!(!answers.dispatch(key, false).unwrap());
        assert_eq!(answers.dispatch(key, false), Err(Error::AlreadyDispatched));
        for _ in 0..3 {
            assert!(answers.prepare_return(key).unwrap().is_cancelled());
        }
        assert_eq!(answers.admit(id, 9).unwrap_err().error, Error::Duplicate);
        assert_eq!(answers.prepare_return(key).unwrap().accept(), Some(7));
        let new = answers.admit(id, 9).unwrap();
        assert_eq!(answers.get(key), Err(Error::StaleKey));
        assert_eq!(answers.get(new), Ok(&9));
    })
    .unwrap();
}
#[test]
fn running_cancellation_obeys_policy_and_response_is_published_exactly_once() {
    for allowed in [false, true] {
        for finish_first in [false, true] {
            let mut slots = [Slot::EMPTY];
            with_answers(&mut slots, |answers| {
                let id = AnswerId::from_wire(3);
                let key = answers.admit(id, 11).unwrap();
                assert!(answers.dispatch(key, allowed).unwrap());
                if finish_first {
                    let f = answers.finish(id, true);
                    assert_eq!(f.cancel, allowed);
                    assert!(f.retired.is_none());
                    assert!(!answers.finish(id, true).release_result_caps);
                }
                let ret = answers.prepare_return(key).unwrap();
                assert_eq!(ret.is_cancelled(), finish_first);
                assert_eq!(ret.accept(), finish_first.then_some(11));
                if !finish_first {
                    assert!(matches!(
                        answers.prepare_return(key),
                        Err(Error::AlreadyReturned)
                    ));
                    let f = answers.finish(id, false);
                    assert!(!f.cancel && !f.release_result_caps);
                    assert_eq!(f.retired, Some(11));
                }
                assert!(answers.is_empty());
                assert!(answers.finish(id, true).retired.is_none());
            })
            .unwrap();
        }
    }
}
#[test]
fn high_ids_capacity_and_generation_limits_preserve_rejected_ownership() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [Slot::EMPTY];
        with_answers(&mut slots, |answers| {
            let id = AnswerId::from_wire(u32::MAX);
            let key = answers.admit(id, 7).unwrap();
            let rejected = answers.admit(AnswerId::from_wire(0), 9).unwrap_err();
            assert_eq!((rejected.error, rejected.value), (Error::Full, 9));
            assert_eq!(answers.get(key), Ok(&7));
            answers.finish(id, true);
            assert_eq!(answers.prepare_return(key).unwrap().accept(), Some(7));
            answers.generation = u64::MAX;
            let rejected = answers.admit(id, 13).unwrap_err();
            assert_eq!(
                (rejected.error, rejected.value),
                (Error::GenerationExhausted, 13)
            );
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_answers_keep_old_pipeline_owner_across_wire_id_reuse() {
    with_growing_answers(0, 2, |answers| {
        let id = AnswerId::from_wire(u32::MAX);
        let old = answers.admit(id, 7).unwrap();
        let pipeline = answers.pipeline(id).unwrap();
        assert!(!answers.finish(id, false).cancel);
        assert!(answers.dispatch(old, true).unwrap());
        assert!(answers.prepare_return(old).unwrap().accept().is_none());
        let new = answers.admit(id, 9).unwrap();
        assert_eq!(
            answers.admit(AnswerId::from_wire(0), 11).unwrap_err().value,
            11
        );
        assert_eq!(answers.lookup(id), Some(new));
        assert_eq!(answers.pipelined(&pipeline), &7);
        assert_eq!(answers.get(new), Ok(&9));
        assert_eq!(answers.release_pipeline(pipeline), Some(7));
        assert_eq!(answers.get(old), Err(Error::StaleKey));
        assert!(answers
            .prepare_admission(AnswerId::from_wire(0), Some(AnswerId::from_wire(17)))
            .is_err());
        assert_eq!(answers.len(), 1);
        assert!(answers.finish(id, true).retired.is_none());
        assert_eq!(answers.prepare_return(new).unwrap().accept(), Some(9));
        assert!(answers.is_empty());
    })
    .unwrap();
}

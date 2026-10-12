use super::*;
use crate::question::Delivery;
#[kani::proof]
#[kani::unwind(4)]
fn question_cleanup_requires_return_and_accepted_finish() {
    let return_first: bool = kani::any();
    let no_finish: bool = kani::any();
    let mut storage = [Slot::EMPTY];
    with_questions(&mut storage, |questions| {
        let local = questions.reserve().unwrap().accept();
        let key = local.key();
        questions.release(local);
        if return_first {
            assert_eq!(
                questions.returned(key.id(), no_finish),
                Ok((key, Delivery::Discard))
            );
        }
        kani::assert(questions.len() == 1, "pending Finish retains question ID");
        assert!(matches!(questions.reserve(), Err(Error::Full)));
        {
            assert!(questions.pending_finish().unwrap().release_result_caps());
        }
        kani::assert(questions.len() == 1, "rejected Finish retains question ID");
        questions.pending_finish().unwrap().accept();
        if !return_first {
            assert!(matches!(questions.reserve(), Err(Error::Full)));
            assert_eq!(
                questions.returned(key.id(), no_finish),
                Ok((key, Delivery::Discard))
            );
        }
        assert!(questions.is_empty());
        let next = questions.reserve().unwrap().accept();
        assert_eq!(next.key().id(), key.id());
        kani::assert(
            !questions.is_live(key),
            "recycled question rejects stale key",
        );
    })
    .unwrap();
}

#[kani::proof]
#[kani::unwind(4)]
fn pipeline_reflection_obeys_peer_answer_lifetime() {
    let no_finish: bool = kani::any();
    let mut storage = [Slot::EMPTY];
    with_questions(&mut storage, |questions| {
        let local = questions.reserve().unwrap().accept();
        let id = local.key().id();
        let retained = questions.retain(&local).unwrap();
        assert_eq!(questions.pipeline_id(&local), Ok(id));
        questions.returned(id, no_finish).unwrap();
        kani::assert(
            questions.pipeline_id(&local).is_ok() != no_finish,
            "pipeline reflection requires the peer to retain the answer",
        );
        questions.release(local);
        assert!(questions.pending_finish().is_none());
        kani::assert(
            questions.pipeline_id(&retained).is_ok() != no_finish,
            "retained pipeline reference obeys noFinishNeeded",
        );
        questions.release(retained);
        assert_eq!(questions.pending_finish().is_some(), !no_finish);
        if !no_finish {
            questions.pending_finish().unwrap().accept();
        }
        assert!(questions.is_empty());
    })
    .unwrap();
}

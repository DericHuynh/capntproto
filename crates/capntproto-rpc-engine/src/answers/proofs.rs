use super::*;
use crate::ids::WireId;
#[kani::proof]
#[kani::unwind(4)]
fn retained_pipeline_stays_on_original_generation() {
    let count: u32 = kani::any();
    kani::assume(count > 0);
    let id = AnswerId::from_wire(kani::any());
    let mut storage = [Slot::EMPTY, Slot::EMPTY];
    with_answers(&mut storage, |answers| {
        let old = answers.admit(id, 17u8).unwrap();
        let original = answers.pipeline(id).unwrap();
        answers.record_mut(old).unwrap().pipelines = count;
        answers.finish(id, false);
        assert!(answers.prepare_return(old).unwrap().accept().is_none());
        let reused = answers.admit(id, 23).unwrap();
        let retained = answers.retain_pipeline(&original);
        if count == u32::MAX {
            assert!(matches!(retained, Err(Error::ReferenceLimit)));
            assert_eq!(answers.record(old).unwrap().pipelines, count);
        } else {
            let retained = retained.unwrap();
            kani::assert(
                answers.record(old).unwrap().pipelines == count + 1,
                "retained pipeline increments the original generation dependency",
            );
            assert_eq!(answers.pipelined(&retained), &17);
            assert!(answers.release_pipeline(original).is_none());
            assert_eq!(answers.record(old).unwrap().pipelines, count);
            let retired = answers.release_pipeline(retained);
            assert_eq!(retired, if count == 1 { Some(17) } else { None });
        }
        assert_eq!(answers.get(reused), Ok(&23));
        assert_eq!(answers.record(reused).unwrap().pipelines, 0);
    })
    .unwrap();
}
#[kani::proof]
#[kani::unwind(4)]
fn input_admission_preserves_parent_on_rejection() {
    let count: u32 = kani::any();
    let generation: u64 = kani::any();
    let accept: bool = kani::any();
    let mut storage = [Slot::EMPTY, Slot::EMPTY];
    with_answers(&mut storage, |answers| {
        let parent = answers.admit(AnswerId::from_wire(1), 17u8).unwrap();
        answers.record_mut(parent).unwrap().pipelines = count;
        answers.generation = generation;
        let result = answers.prepare_admission(AnswerId::from_wire(2), Some(parent.id()));
        if count == u32::MAX || generation == u64::MAX {
            assert!(result.is_err());
            drop(result);
            assert_eq!(answers.record(parent).unwrap().pipelines, count);
            assert_eq!(answers.generation, generation);
            assert_eq!(answers.len(), 1);
        } else if accept {
            let (key, pipeline) = result.unwrap().accept(23);
            kani::assert(
                answers.record(parent).unwrap().pipelines == count + 1,
                "accepted call retains exactly one parent dependency",
            );
            assert_eq!(answers.generation, generation + 1);
            assert_eq!(answers.get(key), Ok(&23));
            assert!(!answers.finish(parent.id(), false).cancel);
            assert_eq!(answers.release_pipeline(pipeline.unwrap()), None);
            assert_eq!(answers.record(parent).unwrap().pipelines, count);
        } else {
            drop(result);
            assert_eq!(answers.len(), 1);
            assert_eq!(answers.generation, generation);
            kani::assert(
                answers.record(parent).unwrap().pipelines == count,
                "rejected call preserves parent dependency count",
            );
        }
    })
    .unwrap();
}
#[kani::proof]
#[kani::unwind(4)]
fn admitted_pipeline_retains_answer_across_finish_and_return() {
    let allowed: bool = kani::any();
    let return_first: bool = kani::any();
    let id = AnswerId::from_wire(kani::any());
    let mut storage = [Slot::EMPTY];
    with_answers(&mut storage, |answers| {
        let key = answers.admit(id, 17u8).unwrap();
        let pipeline = answers.pipeline(id).unwrap();
        assert!(answers.dispatch(key, allowed).unwrap());
        if return_first {
            let mut pending = answers.prepare_return(key).unwrap();
            if kani::any() {
                let eligible = pending.can_omit_finish();
                assert_eq!(pending.omit_finish().is_ok(), eligible);
            }
            assert!(pending.accept().is_none());
        }
        let finished = answers.finish(id, kani::any());
        kani::assert(
            !finished.cancel,
            "admitted pipeline prevents execution cancellation",
        );
        kani::assert(
            finished.retired.is_none(),
            "Finish retains pipelined answer owner",
        );
        if !return_first {
            assert!(answers.prepare_return(key).unwrap().accept().is_none());
        }
        kani::assert(
            answers.get(key) == Ok(&17),
            "published Return retains pipelined answer owner",
        );
        assert_eq!(answers.pipelined(&pipeline), &17);
        assert_eq!(answers.release_pipeline(pipeline), Some(17));
        assert!(answers.is_empty());
    })
    .unwrap();
}

#[kani::proof]
#[kani::unwind(4)]
fn peer_reuse_cannot_redirect_an_older_pipeline_lease() {
    let mut storage = [Slot::EMPTY, Slot::EMPTY];
    let id = AnswerId::from_wire(kani::any());
    let mut outputs = 0;
    with_answers(&mut storage, |answers| {
        let old = answers.admit(id, 17u8).unwrap();
        let pipeline = answers.pipeline(id).unwrap();
        assert!(!answers.finish(id, false).cancel);
        kani::assert(
            answers.dispatch(old, true).unwrap(),
            "queued pipeline still executes after Finish",
        );
        assert!(answers.prepare_return(old).unwrap().accept().is_none());
        let new = answers.admit(id, 23);
        kani::assert(new.is_ok(), "retired answer ID accepts reuse");
        let new = new.unwrap();
        kani::assert(
            answers.pipelined(&pipeline) == &17,
            "reused wire ID preserves original pipeline owner",
        );
        assert_eq!(answers.lookup(id), Some(new));
        outputs += answers.release_pipeline(pipeline).unwrap();
        assert_eq!(answers.get(old), Err(Error::StaleKey));
        kani::assert(
            answers.get(new) == Ok(&23),
            "old pipeline release preserves new answer",
        );
        answers.finish(id, false);
        outputs += answers.prepare_return(new).unwrap().accept().unwrap();
        assert!(answers.is_empty());
    })
    .unwrap();
    assert_eq!(outputs, 40);
}

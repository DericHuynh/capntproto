use super::*;
use capnp::{
    message::{FixedBuilder, Reader},
    word,
};
use rpc_capnp::api::{CapDescriptor, Finish, Message, Payload, Release, Return};

#[test]
fn finish_validates_all_result_releases_before_retiring_any_owner() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..9 {
            let mut as_ = [answers::Slot::EMPTY];
            let mut es = [exports::Slot::EMPTY];
            answers::with_answers(&mut as_, |answers| {
                exports::with_exports(&mut es, |exports| {
                    let id = AnswerId::from_wire(17);
                    let answer = answers.admit(id, 41).unwrap();
                    let pipeline = answers.pipeline(id).unwrap();
                    let mut batch = exports.batch();
                    let first = batch.export(43).unwrap();
                    let _repeated = batch.grant(first.key()).unwrap();
                    batch.commit();
                    let local = exports.lease(first.id()).unwrap();
                    if variant != 8 {
                        assert_eq!(answers.prepare_return(answer).unwrap().accept(), None);
                    }
                    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
                    let mut original = FixedBuilder::new(&mut words).unwrap();
                    let mut root = original.try_init_root::<Message>().unwrap();
                    let mut result = root.field(Message::RETURN).init().unwrap();
                    result
                        .scalar(Return::ANSWER_ID)
                        .set(if variant == 2 { 18 } else { 17 });
                    let mut payload = result.field(Return::RESULTS).init().unwrap();
                    let count = if variant == 1 { 3 } else { 2 };
                    let mut caps = payload.field(Payload::CAP_TABLE).init(count).unwrap();
                    for i in 0..count {
                        caps.get_mut(i)
                            .unwrap()
                            .scalar(CapDescriptor::SENDER_HOSTED)
                            .set(first.id().to_wire());
                    }
                    let original =
                        Reader::new(original.get_segments_for_output(), Default::default());
                    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 8];
                    let mut finish = FixedBuilder::new(&mut words).unwrap();
                    let mut root = finish.try_init_root::<Finish>().unwrap();
                    root.scalar(Finish::QUESTION_ID)
                        .set(if variant == 6 { 99 } else { 17 });
                    root.scalar(Finish::RELEASE_RESULT_CAPS).set(variant != 5);
                    let finish = Reader::new(finish.get_segments_for_output(), Default::default());
                    let mut scratch = [0];
                    let pending = finished(
                        finish.get_root().unwrap(),
                        if matches!(variant, 3 | 8) {
                            None
                        } else {
                            Some(original.get_root().unwrap())
                        },
                        answers,
                        exports,
                        &mut scratch,
                    );
                    if matches!(variant, 1..=4) {
                        if variant == 4 {
                            drop(pending.unwrap());
                        } else {
                            assert!(pending.is_err());
                            drop(pending);
                        }
                        assert!(answers.prepare_finish(id, false).is_some());
                        assert_eq!(exports.release(first.id(), 2), Ok(None));
                        answers.finish(id, false);
                    } else {
                        let pending = pending.unwrap();
                        assert_eq!(pending.is_ignored(), variant == 6);
                        let result = pending.accept();
                        assert_eq!(result.is_none(), variant == 6);
                        assert_eq!(exports.take_retired(0), None);
                        if matches!(variant, 5 | 6 | 8) {
                            assert_eq!(exports.release(first.id(), 2), Ok(None));
                        }
                        if variant == 6 {
                            answers.finish(id, false);
                        }
                        if variant == 7 {
                            let duplicate = finished(
                                finish.get_root().unwrap(),
                                Some(original.get_root().unwrap()),
                                answers,
                                exports,
                                &mut [],
                            )
                            .unwrap();
                            assert!(duplicate.is_ignored());
                            assert!(duplicate.accept().is_none());
                        }
                    }
                    if variant == 8 {
                        assert_eq!(answers.prepare_return(answer).unwrap().accept(), None);
                    }
                    assert_eq!(exports.get(&local), &43);
                    assert_eq!(exports.release_local(local), Some(43));
                    assert_eq!(answers.pipelined(&pipeline), &41);
                    assert_eq!(answers.release_pipeline(pipeline), Some(41));
                    assert!(answers.is_empty() && exports.is_empty());
                })
                .unwrap();
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn explicit_release_rejection_preserves_counts_and_defers_owner_destruction() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..4 {
            let mut es = [exports::Slot::EMPTY];
            exports::with_exports(&mut es, |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(41).unwrap();
                batch.commit();
                let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 8];
                let mut builder = FixedBuilder::new(&mut words).unwrap();
                let mut root = builder.try_init_root::<Release>().unwrap();
                root.scalar(Release::ID).set(if variant == 1 {
                    99
                } else {
                    grant.id().to_wire()
                });
                root.scalar(Release::REFERENCE_COUNT)
                    .set(if variant == 2 { 2 } else { 1 });
                let reader = Reader::new(builder.get_segments_for_output(), Default::default());
                let mut scratch = [0];
                let pending = released(reader.get_root().unwrap(), exports, &mut scratch);
                if variant == 0 {
                    pending.unwrap().accept();
                    assert_eq!(exports.take_retired(0), Some(41));
                } else {
                    if variant == 3 {
                        drop(pending.unwrap());
                    } else {
                        assert!(pending.is_err());
                        drop(pending);
                    }
                    assert_eq!(exports.release(grant.id(), 1), Ok(Some(41)));
                }
                assert!(exports.is_empty());
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

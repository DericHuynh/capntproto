use super::*;
use capnp::{message::FixedBuilder, word};
use rpc_capnp::api::{Call, CapDescriptor, Exception, Message, Payload, Return};
fn frames(original: &mut FixedBuilder<'_>, response: &mut FixedBuilder<'_>, mode: u8) {
    let mut root = original.try_init_root::<Message>().unwrap();
    let mut call = root.field(Message::CALL).init().unwrap();
    call.scalar(Call::QUESTION_ID).set(u32::from(mode == 1));
    let mut params = call.field(Call::PARAMS).init().unwrap();
    params
        .field(Payload::CONTENT)
        .set_capability_index(0)
        .unwrap();
    let mut table = params
        .field(Payload::CAP_TABLE)
        .init(if mode == 5 { 2 } else { 1 })
        .unwrap();
    for i in 0..table.len() {
        table
            .get_mut(i)
            .unwrap()
            .scalar(CapDescriptor::SENDER_HOSTED)
            .set(0);
    }
    let mut root = response.try_init_root::<Message>().unwrap();
    let mut result = root.field(Message::RETURN).init().unwrap();
    result.scalar(Return::ANSWER_ID).set(u32::from(mode == 4));
    if mode == 7 {
        result.scalar(Return::NO_FINISH_NEEDED).set(true);
    }
    if mode == 10 {
        result.void(Return::CANCELED).set();
        return;
    }
    if mode == 11 {
        result
            .field(Return::EXCEPTION)
            .init()
            .unwrap()
            .field(Exception::REASON)
            .copy_from("failed")
            .unwrap();
        return;
    }
    let mut results = result.field(Return::RESULTS).init().unwrap();
    results
        .field(Payload::CONTENT)
        .set_capability_index(u32::from(mode == 2))
        .unwrap();
    results
        .field(Payload::CAP_TABLE)
        .init(1)
        .unwrap()
        .get_mut(0)
        .unwrap()
        .scalar(CapDescriptor::SENDER_HOSTED)
        .set(7);
}
#[test]
fn reply_admission_never_partially_releases_parameters_or_imports_results() {
    let measured = allocation_counter::measure(|| {
        for mode in 0..9 {
            let mut ow = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
            let mut rw = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
            let mut original = FixedBuilder::new(&mut ow).unwrap();
            let mut response = FixedBuilder::new(&mut rw).unwrap();
            frames(&mut original, &mut response, mode);
            let original =
                capnp::message::Reader::new(original.get_segments_for_output(), Default::default());
            let response =
                capnp::message::Reader::new(response.get_segments_for_output(), Default::default());
            let rpc_capnp::message::Return(result) = response
                .get_root::<rpc_capnp::message::Reader<'_>>()
                .unwrap()
                .which()
                .unwrap()
            else {
                unreachable!()
            };
            let mut es = [exports::Slot::EMPTY];
            let mut is = [imports::Slot::EMPTY];
            let mut qs = [questions::Slot::EMPTY];
            exports::with_exports(&mut es, |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(17).unwrap();
                batch.commit();
                let lease = exports.lease(grant.id()).unwrap();
                imports::with_imports(&mut is, |imports| {
                    questions::with_questions(&mut qs, |questions| {
                        let question = questions.reserve().unwrap().accept();
                        let mut receipts = [imports::ReceiveSlot::new(
                            capntproto_rpc_engine::ids::ImportId::from_wire(if mode == 3 {
                                9
                            } else {
                                7
                            }),
                            23,
                        )];
                        let mut scratch = [0];
                        let pending = returned(
                            result.unwrap(),
                            original.get_root().unwrap(),
                            questions,
                            exports,
                            imports,
                            &mut receipts,
                            &mut scratch[..usize::from(mode != 6)],
                        );
                        if mode == 0 {
                            let (key, reply) = pending.unwrap().accept();
                            assert_eq!(key, question.key());
                            assert!(matches!(
                                questions.prepare_receive(key.id(), false),
                                Err(questions::Error::DuplicateReturn)
                            ));
                            let Reply::Results(payload) = reply else {
                                unreachable!()
                            };
                            let pointer = payload
                                .content()
                                .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                                .unwrap();
                            assert_eq!(imports.get(payload.capability(pointer).unwrap()), &23);
                            payload.release(imports);
                            assert_eq!(imports.pending_release(0).unwrap().commit(), 23);
                            assert!(exports.resolve(grant.key()).is_none());
                        } else {
                            if mode == 8 {
                                drop(pending.unwrap());
                            } else {
                                assert!(pending.is_err());
                                drop(pending);
                            }
                            assert!(imports.is_empty());
                            assert_eq!(receipts[0].owner(), Some(&23));
                            assert!(receipts[0].local().is_none());
                            assert_eq!(exports.resolve(grant.key()), Some(&17));
                            assert_eq!(
                                questions
                                    .prepare_receive(question.key().id(), false)
                                    .unwrap()
                                    .delivery(),
                                Delivery::Deliver
                            );
                            assert_eq!(exports.release(grant.id(), 1), Ok(None));
                        }
                        questions.release(question);
                    })
                    .unwrap();
                })
                .unwrap();
                assert_eq!(exports.get(&lease), &17);
                assert_eq!(exports.release_local(lease), Some(17));
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}
#[test]
fn canceled_questions_discard_results_without_importing_but_release_parameters() {
    for mode in [0, 10, 11] {
        let mut ow = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
        let mut rw = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
        let mut original = FixedBuilder::new(&mut ow).unwrap();
        let mut response = FixedBuilder::new(&mut rw).unwrap();
        frames(&mut original, &mut response, mode);
        let original =
            capnp::message::Reader::new(original.get_segments_for_output(), Default::default());
        let response =
            capnp::message::Reader::new(response.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Return(result) = response
            .get_root::<rpc_capnp::message::Reader<'_>>()
            .unwrap()
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let mut es = [exports::Slot::EMPTY];
        let mut qs = [questions::Slot::EMPTY];
        exports::with_exports(&mut es, |exports| {
            let mut batch = exports.batch();
            let grant = batch.export(17).unwrap();
            batch.commit();
            imports::with_imports::<(), _>(&mut [], |imports| {
                questions::with_questions(&mut qs, |questions| {
                    let question = questions.reserve().unwrap().accept();
                    let old = question.key();
                    questions.release(question);
                    let (key, reply) = returned(
                        result.unwrap(),
                        original.get_root().unwrap(),
                        questions,
                        exports,
                        imports,
                        &mut [],
                        &mut [0],
                    )
                    .unwrap()
                    .accept();
                    assert_eq!(key, old);
                    assert!(matches!(reply, Reply::Discarded));
                    assert!(imports.is_empty());
                    assert_eq!(exports.resolve(grant.key()), None);
                    assert_eq!(exports.take_retired(0), Some(17));
                    assert!(questions.pending_finish().unwrap().release_result_caps());
                    questions.pending_finish().unwrap().accept();
                    assert!(questions.is_empty());
                })
                .unwrap();
            })
            .unwrap();
        })
        .unwrap();
    }
}
#[test]
fn unsolicited_cancellation_preserves_live_question_and_parameter_grant() {
    let mut ow = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
    let mut rw = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
    let mut original = FixedBuilder::new(&mut ow).unwrap();
    let mut response = FixedBuilder::new(&mut rw).unwrap();
    frames(&mut original, &mut response, 10);
    let original =
        capnp::message::Reader::new(original.get_segments_for_output(), Default::default());
    let response =
        capnp::message::Reader::new(response.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Return(result) = response
        .get_root::<rpc_capnp::message::Reader<'_>>()
        .unwrap()
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let mut es = [exports::Slot::EMPTY];
    let mut qs = [questions::Slot::EMPTY];
    exports::with_exports(&mut es, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(17).unwrap();
        batch.commit();
        imports::with_imports::<(), _>(&mut [], |imports| {
            questions::with_questions(&mut qs, |questions| {
                let question = questions.reserve().unwrap().accept();
                assert!(matches!(
                    returned(
                        result.unwrap(),
                        original.get_root().unwrap(),
                        questions,
                        exports,
                        imports,
                        &mut [],
                        &mut [0]
                    ),
                    Err(Error::UnexpectedCancellation)
                ));
                assert_eq!(
                    questions
                        .prepare_receive(question.key().id(), false)
                        .unwrap()
                        .delivery(),
                    Delivery::Deliver
                );
                questions.release(question);
            })
            .unwrap();
        })
        .unwrap();
        assert_eq!(exports.release(grant.id(), 1), Ok(Some(17)));
    })
    .unwrap();
}

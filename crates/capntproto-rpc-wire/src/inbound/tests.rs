use super::*;
use capnp::{message::FixedBuilder, word};
use rpc_capnp::api::{Call, CapDescriptor, Message, MessageTarget, Payload};
fn encode(builder: &mut FixedBuilder<'_>, question: u32, target: Option<u32>, variant: u8) {
    let mut root = builder.try_init_root::<Message>().unwrap();
    let mut call = root.field(Message::CALL).init().unwrap();
    call.scalar(Call::QUESTION_ID).set(question);
    let mut address = call.field(Call::TARGET).init().unwrap();
    if let Some(parent) = target {
        pipeline::Path::new(&[0])
            .write(
                address
                    .field(MessageTarget::PROMISED_ANSWER)
                    .init()
                    .unwrap(),
                capntproto_rpc_engine::ids::QuestionId::from_wire(parent),
            )
            .unwrap();
    } else {
        address.scalar(MessageTarget::IMPORTED_CAP).set(0);
    }
    let mut payload = call.field(Call::PARAMS).init().unwrap();
    payload
        .field(Payload::CONTENT)
        .set_capability_index(if variant == 1 { 1 } else { 0 })
        .unwrap();
    payload
        .field(Payload::CAP_TABLE)
        .init(1)
        .unwrap()
        .get_mut(0)
        .unwrap()
        .scalar(CapDescriptor::SENDER_HOSTED)
        .set(7);
}
#[test]
fn rejected_inputs_never_partially_acquire_target_answer_or_imports() {
    let counts = allocation_counter::measure(|| {
        for variant in 0..7 {
            let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
            let mut builder = FixedBuilder::new(&mut words).unwrap();
            encode(&mut builder, 11, (variant == 4).then_some(91), variant);
            let reader =
                capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
            let rpc_capnp::message::Call(wire) = reader
                .get_root::<rpc_capnp::message::Reader<'_>>()
                .unwrap()
                .which()
                .unwrap()
            else {
                unreachable!()
            };
            let wire = wire.unwrap();
            let mut es = [exports::Slot::EMPTY];
            let mut as_ = [answers::Slot::<u32>::EMPTY];
            let mut is = [imports::Slot::EMPTY];
            exports::with_exports(&mut es, |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(41).unwrap();
                let id = grant.id();
                batch.commit();
                answers::with_answers(&mut as_[..usize::from(variant != 2)], |answers| {
                    let old =
                        (variant == 3).then(|| answers.admit(AnswerId::from_wire(11), 23).unwrap());
                    imports::with_imports(&mut is[..usize::from(variant != 5)], |imports| {
                        let mut receipts = [imports::ReceiveSlot::new(
                            capntproto_rpc_engine::ids::ImportId::from_wire(if variant == 6 {
                                9
                            } else {
                                7
                            }),
                            17,
                        )];
                        let pending = call(wire, answers, exports, imports, &mut receipts, &mut []);
                        if variant == 0 {
                            drop(pending.unwrap());
                        } else {
                            assert!(pending.is_err());
                            drop(pending);
                        }
                        assert!(imports.is_empty());
                        assert_eq!(receipts[0].owner(), Some(&17));
                        assert!(receipts[0].local().is_none());
                    })
                    .unwrap();
                    assert_eq!(answers.len(), usize::from(old.is_some()));
                    if let Some(old) = old {
                        assert_eq!(answers.get(old), Ok(&23));
                    }
                })
                .unwrap();
                // An accidentally retained target would keep this owner alive.
                assert_eq!(exports.release(id, 1), Ok(Some(41)));
            })
            .unwrap();
        }
    });
    assert_eq!(counts.count_total, 0);
}
#[test]
fn accepted_hosted_call_owns_all_three_effects_without_allocation() {
    let counts = allocation_counter::measure(|| {
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
        let mut builder = FixedBuilder::new(&mut words).unwrap();
        encode(&mut builder, 11, None, 0);
        let reader =
            capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Call(wire) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()
            .unwrap()
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let mut es = [exports::Slot::EMPTY];
        let mut as_ = [answers::Slot::EMPTY];
        let mut is = [imports::Slot::EMPTY];
        exports::with_exports(&mut es, |exports| {
            let mut batch = exports.batch();
            let grant = batch.export(41).unwrap();
            let id = grant.id();
            batch.commit();
            answers::with_answers(&mut as_, |answers| {
                imports::with_imports(&mut is, |imports| {
                    let mut receipts = [imports::ReceiveSlot::new(
                        capntproto_rpc_engine::ids::ImportId::from_wire(7),
                        17,
                    )];
                    let admitted = call(
                        wire.unwrap(),
                        answers,
                        exports,
                        imports,
                        &mut receipts,
                        &mut [],
                    )
                    .unwrap()
                    .accept(33);
                    assert_eq!(answers.get(admitted.answer), Ok(&33));
                    let pipeline::Target::Hosted(target) = admitted.target else {
                        unreachable!()
                    };
                    assert_eq!(exports.release(id, 1), Ok(None));
                    assert_eq!(exports.get(&target), &41);
                    let pointer = admitted
                        .payload
                        .content()
                        .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                        .unwrap();
                    assert_eq!(
                        imports.get(admitted.payload.capability(pointer).unwrap()),
                        &17
                    );
                    admitted.payload.release(imports);
                    assert_eq!(imports.pending_release(0).unwrap().count(), 1);
                    assert_eq!(imports.pending_release(0).unwrap().commit(), 17);
                    answers.finish(admitted.answer.id(), false);
                    assert_eq!(
                        answers.prepare_return(admitted.answer).unwrap().accept(),
                        Some(33)
                    );
                    assert_eq!(exports.release_local(target), Some(41));
                })
                .unwrap();
            })
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}
#[test]
fn accepted_pipeline_keeps_parent_running_after_early_finish() {
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    encode(&mut builder, 11, Some(9), 0);
    let reader = capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Call(wire) = reader
        .get_root::<rpc_capnp::message::Reader<'_>>()
        .unwrap()
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let mut as_ = [const { answers::Slot::EMPTY }; 2];
    let mut is = [imports::Slot::EMPTY];
    exports::with_exports::<(), _>(&mut [], |exports| {
        answers::with_answers(&mut as_, |answers| {
            let parent = answers.admit(AnswerId::from_wire(9), 41).unwrap();
            imports::with_imports(&mut is, |imports| {
                let mut receipts = [imports::ReceiveSlot::new(
                    capntproto_rpc_engine::ids::ImportId::from_wire(7),
                    17,
                )];
                let mut scratch = [0];
                let admitted = call(
                    wire.unwrap(),
                    answers,
                    exports,
                    imports,
                    &mut receipts,
                    &mut scratch,
                )
                .unwrap()
                .accept(33);
                let pipeline::Target::Promised { answer, path } = admitted.target else {
                    unreachable!()
                };
                assert_eq!(path.fields(), &[0]);
                assert!(!answers.finish(parent.id(), false).cancel);
                assert!(answers.dispatch(parent, true).unwrap());
                assert!(answers.prepare_return(parent).unwrap().accept().is_none());
                assert_eq!(answers.pipelined(&answer), &41);
                assert_eq!(answers.release_pipeline(answer), Some(41));
                admitted.payload.release(imports);
                imports.pending_release(0).unwrap().commit();
                answers.finish(admitted.answer.id(), false);
                assert_eq!(
                    answers.prepare_return(admitted.answer).unwrap().accept(),
                    Some(33)
                );
            })
            .unwrap();
            assert!(answers.is_empty());
        })
        .unwrap();
    })
    .unwrap();
}

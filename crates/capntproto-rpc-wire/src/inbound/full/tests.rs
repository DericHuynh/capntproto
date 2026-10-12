use super::*;
use capnp::{
    message::{FixedBuilder, Reader},
    word,
};
use capntproto_rpc_engine::ids::QuestionId;
use rpc_capnp::api::{Call as WireCall, CapDescriptor, MessageTarget, Payload};

#[test]
fn target_and_reflected_descriptors_share_atomic_call_admission() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..6 {
            let mut imported = [imports::Slot::<i32>::EMPTY];
            let mut exported = [exports::Slot::EMPTY];
            let mut answered = [const { answers::Slot::EMPTY }; 2];
            imports::with_imports(&mut imported, |imports| {
                exports::with_exports(&mut exported, |exports| {
                    answers::with_answers(
                        &mut answered[..if variant == 3 { 1 } else { 2 }],
                        |answers| {
                            let mut batch = exports.batch();
                            let grant = batch.export(41).unwrap();
                            batch.commit();
                            let parent = answers.admit(AnswerId::from_wire(3), 43).unwrap();
                            let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 48];
                            let mut builder = FixedBuilder::new(&mut words).unwrap();
                            let mut root = builder.try_init_root::<WireCall>().unwrap();
                            root.scalar(WireCall::ONLY_PROMISE_PIPELINE)
                                .set(variant == 5);
                            root.scalar(WireCall::QUESTION_ID).set(if variant == 2 {
                                3
                            } else {
                                5
                            });
                            let mut target = root.field(WireCall::TARGET).init().unwrap();
                            if variant == 1 {
                                pipeline::Path::new(&[])
                                    .write(
                                        target
                                            .field(MessageTarget::PROMISED_ANSWER)
                                            .init()
                                            .unwrap(),
                                        QuestionId::from_wire(3),
                                    )
                                    .unwrap();
                            } else {
                                target
                                    .scalar(MessageTarget::IMPORTED_CAP)
                                    .set(grant.id().to_wire());
                            }
                            let mut payload = root.field(WireCall::PARAMS).init().unwrap();
                            payload
                                .field(Payload::CONTENT)
                                .set_capability_index(0)
                                .unwrap();
                            let mut table = payload.field(Payload::CAP_TABLE).init(2).unwrap();
                            table
                                .get_mut(0)
                                .unwrap()
                                .scalar(CapDescriptor::RECEIVER_HOSTED)
                                .set(grant.id().to_wire());
                            pipeline::Path::new(&[])
                                .write(
                                    table
                                        .get_mut(1)
                                        .unwrap()
                                        .field(CapDescriptor::RECEIVER_ANSWER)
                                        .init()
                                        .unwrap(),
                                    QuestionId::from_wire(3),
                                )
                                .unwrap();
                            let reader =
                                Reader::new(builder.get_segments_for_output(), Default::default());
                            let mut ir = [const { imports::ReceiveSlot::none() }; 2];
                            let mut er = [
                                exports::LeaseSlot::new(grant.id()),
                                exports::LeaseSlot::none(),
                                if variant == 1 {
                                    exports::LeaseSlot::none()
                                } else {
                                    exports::LeaseSlot::new(if variant == 4 {
                                        ExportId::from_wire(99)
                                    } else {
                                        grant.id()
                                    })
                                },
                            ];
                            let mut ar = [
                                answers::PipelineSlot::none(),
                                answers::PipelineSlot::new(parent.id()),
                                if variant == 1 {
                                    answers::PipelineSlot::new(parent.id())
                                } else {
                                    answers::PipelineSlot::none()
                                },
                            ];
                            let mut path = [0; 2];
                            let pending = call(
                                reader.get_root().unwrap(),
                                imports,
                                exports,
                                answers,
                                Slots {
                                    imports: &mut ir,
                                    exports: &mut er,
                                    answers: &mut ar,
                                },
                                &mut path,
                            );
                            if (2..5).contains(&variant) {
                                assert!(pending.is_err());
                                drop(pending);
                                assert_eq!(answers.len(), 1);
                                assert_eq!(exports.release(grant.id(), 1), Ok(Some(41)));
                                answers.finish(parent.id(), false);
                                assert!(!answers.dispatch(parent, true).unwrap());
                                assert_eq!(
                                    answers.prepare_return(parent).unwrap().accept(),
                                    Some(43)
                                );
                            } else {
                                let accepted = pending.unwrap().accept(47);
                                assert_eq!(answers.len(), 2);
                                assert_eq!(exports.release(grant.id(), 1), Ok(None));
                                answers.finish(parent.id(), false);
                                assert!(answers.dispatch(parent, true).unwrap());
                                assert_eq!(answers.prepare_return(parent).unwrap().accept(), None);
                                accepted.payload.release(imports, exports, answers);
                                match accepted.target {
                                    pipeline::Target::Hosted(local) => {
                                        assert_eq!(exports.take_retired(0), None);
                                        assert_eq!(exports.get(&local), &41);
                                        assert_eq!(exports.release_local(local), Some(41));
                                        assert_eq!(answers.take_finished(0), Some(43));
                                    }
                                    pipeline::Target::Promised { answer, .. } => {
                                        assert_eq!(answers.take_finished(0), None);
                                        assert_eq!(answers.pipelined(&answer), &43);
                                        assert_eq!(answers.release_pipeline(answer), Some(43));
                                        assert_eq!(exports.take_retired(0), Some(41));
                                    }
                                }
                                answers.finish(accepted.answer.id(), false);
                                assert_eq!(
                                    answers.prepare_return(accepted.answer).unwrap().accept(),
                                    Some(47)
                                );
                                assert!(
                                    answers.is_empty() && exports.is_empty() && imports.is_empty()
                                );
                            }
                        },
                    )
                    .unwrap();
                })
                .unwrap();
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

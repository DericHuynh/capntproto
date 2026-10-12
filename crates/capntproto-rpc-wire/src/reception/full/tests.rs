use super::*;
use capnp::{
    message::{FixedBuilder, Reader},
    word,
};
use capntproto_rpc_engine::ids::{ImportId, QuestionId};
use rpc_capnp::api::{CapDescriptor, Payload};

#[test]
fn mixed_descriptors_acquire_all_owners_or_none_without_allocation() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..5 {
            let mut imports_storage = [imports::Slot::EMPTY];
            let mut exports_storage = [exports::Slot::EMPTY];
            let mut answers_storage = [const { answers::Slot::EMPTY }; 2];
            imports::with_imports(&mut imports_storage, |imports| {
                exports::with_exports(&mut exports_storage, |exports| {
                    answers::with_answers(&mut answers_storage, |answers| {
                        let mut batch = exports.batch();
                        let exported = batch.export(41).unwrap();
                        batch.commit();
                        let parent = answers.admit(AnswerId::from_wire(3), 43).unwrap();
                        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 48];
                        let mut builder = FixedBuilder::new(&mut words).unwrap();
                        let mut payload = builder.try_init_root::<Payload>().unwrap();
                        payload
                            .field(Payload::CONTENT)
                            .set_capability_index(1)
                            .unwrap();
                        let mut table = payload.field(Payload::CAP_TABLE).init(3).unwrap();
                        table
                            .get_mut(0)
                            .unwrap()
                            .scalar(CapDescriptor::SENDER_HOSTED)
                            .set(7);
                        let local_id = if variant == 2 {
                            ExportId::from_wire(99)
                        } else {
                            exported.id()
                        };
                        table
                            .get_mut(1)
                            .unwrap()
                            .scalar(CapDescriptor::RECEIVER_HOSTED)
                            .set(local_id.to_wire());
                        let answer_id = AnswerId::from_wire(if variant == 3 { 99 } else { 3 });
                        crate::pipeline::Path::new(&[1, 2])
                            .write(
                                table
                                    .get_mut(2)
                                    .unwrap()
                                    .field(CapDescriptor::RECEIVER_ANSWER)
                                    .init()
                                    .unwrap(),
                                QuestionId::from_wire(answer_id.to_wire()),
                            )
                            .unwrap();
                        if variant == 4 {
                            table
                                .get_mut(2)
                                .unwrap()
                                .scalar(CapDescriptor::ATTACHED_FD)
                                .set(0);
                        }
                        let reader =
                            Reader::new(builder.get_segments_for_output(), Default::default());
                        let mut imported = [
                            imports::ReceiveSlot::new(
                                ImportId::from_wire(if variant == 1 { 9 } else { 7 }),
                                47,
                            ),
                            imports::ReceiveSlot::none(),
                            imports::ReceiveSlot::none(),
                        ];
                        let mut local = [
                            exports::LeaseSlot::none(),
                            exports::LeaseSlot::new(local_id),
                            exports::LeaseSlot::none(),
                        ];
                        let mut pipelines = [
                            answers::PipelineSlot::none(),
                            answers::PipelineSlot::none(),
                            answers::PipelineSlot::new(answer_id),
                        ];
                        let prepared = prepare(
                            reader.get_root().unwrap(),
                            imports,
                            exports,
                            answers,
                            Slots {
                                imports: &mut imported,
                                exports: &mut local,
                                answers: &mut pipelines,
                            },
                        );
                        if matches!(variant, 0 | 4) {
                            let received = prepared.unwrap().accept();
                            let Capability::Remote(remote) = received.at(0).unwrap() else {
                                unreachable!()
                            };
                            assert_eq!(imports.get(remote), &47);
                            let Capability::Local(local) = received
                                .capability(
                                    received
                                        .content()
                                        .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                                        .unwrap(),
                                )
                                .unwrap()
                            else {
                                unreachable!()
                            };
                            assert_eq!(exports.get(local), &41);
                            assert_eq!(exports.release(exported.id(), 1), Ok(None));
                            answers.finish(parent.id(), false);
                            assert!(answers.dispatch(parent, true).unwrap());
                            assert_eq!(answers.prepare_return(parent).unwrap().accept(), None);
                            let newer = answers.admit(parent.id(), 53).unwrap();
                            let Capability::Promised { answer, path } = received.at(2).unwrap()
                            else {
                                unreachable!()
                            };
                            assert_eq!(answers.pipelined(answer), &43);
                            assert_eq!(
                                crate::pipeline::Path::decode(path, &mut [0; 2])
                                    .unwrap()
                                    .fields(),
                                &[1, 2]
                            );
                            received.release(imports, exports, answers);
                            assert_eq!(exports.take_retired(0), Some(41));
                            assert_eq!(answers.take_finished(0), Some(43));
                            assert_eq!(answers.get(newer), Ok(&53));
                            assert_eq!(imports.pending_release(0).unwrap().commit(), 47);
                        } else {
                            assert!(prepared.is_err());
                            drop(prepared);
                            assert_eq!(imported[0].owner(), Some(&47));
                            assert!(imports.is_empty());
                            assert!(
                                local[1].lease().is_none() && pipelines[2].pipeline().is_none()
                            );
                            assert_eq!(exports.release(exported.id(), 1), Ok(Some(41)));
                            answers.finish(parent.id(), false);
                            assert!(!answers.dispatch(parent, true).unwrap());
                            assert_eq!(answers.prepare_return(parent).unwrap().accept(), Some(43));
                        }
                    })
                    .unwrap();
                })
                .unwrap();
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

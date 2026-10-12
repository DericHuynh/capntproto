use super::*;
use capnp::{
    message::{FixedBuilder, Reader},
    word,
};
use rpc_capnp::api::{CapDescriptor, Resolve};

#[test]
fn reflected_resolution_preserves_all_owners_on_rejection_and_abandonment() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..9 {
            let mut ir = [imports::Slot::EMPTY];
            let mut er = [exports::Slot::EMPTY];
            let mut ar = [answers::Slot::EMPTY];
            imports::with_imports(&mut ir, |imports| {
                exports::with_exports(&mut er, |exports| {
                    answers::with_answers(&mut ar, |answers| {
                        let id = ImportId::from_wire(17);
                        let mut initial = [imports::ReceiveSlot::new_promise(id, 43)];
                        let abandoned = matches!(variant, 2 | 3);
                        if !abandoned {
                            imports.prepare_receive(&mut initial).unwrap().commit();
                        }
                        let original = initial[0].local().map(|local| local.key());
                        let mut batch = exports.batch();
                        let grant = batch.export(41).unwrap();
                        batch.commit();
                        let parent = answers.admit(AnswerId::from_wire(3), 45).unwrap();
                        let local = variant % 2 == 0;
                        let export_id = if variant == 4 {
                            ExportId::from_wire(99)
                        } else {
                            grant.id()
                        };
                        let answer_id = AnswerId::from_wire(if variant == 5 { 99 } else { 3 });
                        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
                        let mut builder = FixedBuilder::new(&mut words).unwrap();
                        let mut root = builder.try_init_root::<Resolve>().unwrap();
                        root.scalar(Resolve::PROMISE_ID).set(id.to_wire());
                        let mut cap = root.field(Resolve::CAP).init().unwrap();
                        if variant == 8 {
                            cap.scalar(CapDescriptor::SENDER_PROMISE).set(id.to_wire());
                        } else if local {
                            cap.scalar(CapDescriptor::RECEIVER_HOSTED)
                                .set(export_id.to_wire());
                        } else {
                            cap.field(CapDescriptor::RECEIVER_ANSWER)
                                .init()
                                .unwrap()
                                .scalar(rpc_capnp::api::PromisedAnswer::QUESTION_ID)
                                .set(answer_id.to_wire());
                        }
                        if variant == 7 {
                            cap.scalar(CapDescriptor::ATTACHED_FD).set(0);
                        }
                        let reader =
                            Reader::new(builder.get_segments_for_output(), Default::default());
                        let mut ir = [if variant == 8 {
                            imports::ReceiveSlot::alias_promise(id)
                        } else {
                            imports::ReceiveSlot::none()
                        }];
                        let mut er = [if local && variant != 8 {
                            exports::LeaseSlot::new(export_id)
                        } else {
                            exports::LeaseSlot::none()
                        }];
                        let mut ar = [if !local {
                            answers::PipelineSlot::new(answer_id)
                        } else {
                            answers::PipelineSlot::none()
                        }];
                        let pending = resolve(
                            reader.get_root().unwrap(),
                            imports,
                            exports,
                            answers,
                            Slots {
                                imports: &mut ir,
                                exports: &mut er,
                                answers: &mut ar,
                            },
                        );
                        if variant < 4 || variant == 7 {
                            let received = pending.unwrap().accept();
                            assert_eq!(received.target(), original);
                            match received.replacement().unwrap().unwrap() {
                                capabilities::Capability::Local(lease) => {
                                    assert!(local);
                                    assert_eq!(exports.get(lease), &41);
                                }
                                capabilities::Capability::Promised { answer, .. } => {
                                    assert!(!local);
                                    assert_eq!(answers.pipelined(answer), &45);
                                }
                                _ => unreachable!(),
                            }
                            assert_eq!(
                                exports.release(grant.id(), 1),
                                Ok(if local { None } else { Some(41) })
                            );
                            answers.finish(parent.id(), false);
                            assert_eq!(
                                answers.prepare_return(parent).unwrap().accept(),
                                if local { Some(45) } else { None }
                            );
                            received.release(imports, exports, answers);
                            assert_eq!(
                                exports.take_retired(0),
                                if local { Some(41) } else { None }
                            );
                            assert_eq!(
                                answers.take_finished(0),
                                if local { None } else { Some(45) }
                            );
                        } else {
                            if variant == 6 {
                                drop(pending.unwrap());
                            } else {
                                assert!(pending.is_err());
                                drop(pending);
                            }
                            imports.prepare_resolution(id, &mut []).unwrap().commit();
                            assert_eq!(exports.release(grant.id(), 1), Ok(Some(41)));
                            answers.finish(parent.id(), false);
                            assert_eq!(answers.prepare_return(parent).unwrap().accept(), Some(45));
                        }
                        if let Some(local) = initial[0].take_local() {
                            imports.release(local);
                            let cleanup = imports.pending_release(0).unwrap();
                            assert_eq!(cleanup.count(), 1);
                            assert_eq!(cleanup.commit(), 43);
                        }
                        assert!(imports.is_empty() && exports.is_empty() && answers.is_empty());
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

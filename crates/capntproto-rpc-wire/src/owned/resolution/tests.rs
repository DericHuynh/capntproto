use super::*;
use crate::rpc_capnp::api::{CapDescriptor, Message, Resolve};
use capnp::{word, Word};

#[test]
fn abandoned_resolution_still_retains_and_releases_its_replacement() {
    let measured = allocation_counter::measure(|| {
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            imports::with_imports(&mut [imports::Slot::EMPTY], |imports| {
                exports::with_exports(&mut [], |exports: &mut exports::Exports<'_, '_, ()>| {
                    answers::with_answers(&mut [], |answers: &mut answers::Answers<'_, '_, ()>| {
                        let original = ImportId::from_wire(3);
                        let mut receipts = [imports::ReceiveSlot::new_promise(original, 41)];
                        imports.prepare_receive(&mut receipts).unwrap().commit();
                        imports.release(receipts[0].take_local().unwrap());
                        assert_eq!(imports.pending_release(0).unwrap().commit(), 41);
                        let frame = Frame::build(messages, 512, Default::default(), |builder| {
                            let mut root = builder.try_init_root::<Message>()?;
                            let mut resolution = root.field(Message::RESOLVE).init()?;
                            resolution
                                .scalar(Resolve::PROMISE_ID)
                                .set(original.to_wire());
                            resolution
                                .field(Resolve::CAP)
                                .init()?
                                .scalar(CapDescriptor::SENDER_HOSTED)
                                .set(9);
                            Ok(())
                        })
                        .unwrap();
                        let resolution =
                            Resolution::admit(frame, messages, imports, exports, answers, |id| {
                                assert_eq!(id.to_wire(), 9);
                                43
                            })
                            .unwrap_or_else(|_| panic!("late Resolve"));
                        assert!(resolution.target().is_none());
                        assert!(imports.pending_release(0).is_none());
                        resolution
                            .with(messages, |view| {
                                let Some(reception::full::Capability::Remote(reference)) =
                                    view.replacement().unwrap()
                                else {
                                    panic!("remote")
                                };
                                assert_eq!(imports.wire_id(reference).to_wire(), 9);
                            })
                            .unwrap();
                        resolution.release(messages, imports, exports, answers);
                        let release = imports.pending_release(0).unwrap();
                        assert_eq!(release.count(), 1);
                        assert_eq!(release.commit(), 43);
                        assert!(imports.is_empty());
                        let writer = messages.reserve(512).unwrap();
                        messages.discard(writer);
                    })
                    .unwrap();
                })
                .unwrap();
            })
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn replacement_capacity_failure_preserves_unresolved_import_and_exact_frame() {
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        imports::with_imports(&mut [imports::Slot::EMPTY], |imports| {
            exports::with_exports(&mut [], |exports: &mut exports::Exports<'_, '_, ()>| {
                answers::with_answers(&mut [], |answers: &mut answers::Answers<'_, '_, ()>| {
                    let original = ImportId::from_wire(3);
                    let mut receipts = [imports::ReceiveSlot::new_promise(original, ())];
                    imports.prepare_receive(&mut receipts).unwrap().commit();
                    let reference = receipts[0].take_local().unwrap();
                    let frame = Frame::build(messages, 512, Default::default(), |builder| {
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut resolution = root.field(Message::RESOLVE).init()?;
                        resolution
                            .scalar(Resolve::PROMISE_ID)
                            .set(original.to_wire());
                        resolution
                            .field(Resolve::CAP)
                            .init()?
                            .scalar(CapDescriptor::SENDER_HOSTED)
                            .set(9);
                        Ok(())
                    })
                    .unwrap();
                    let rejected =
                        Resolution::admit(frame, messages, imports, exports, answers, |_| ())
                            .err()
                            .unwrap();
                    assert_eq!(
                        imports.promise_state(&reference),
                        capntproto_rpc_engine::promise::ImportState::Pending
                    );
                    rejected
                        .frame
                        .with(messages, |root| {
                            assert!(matches!(
                                root.which().unwrap(),
                                rpc_capnp::message::Resolve(_)
                            ))
                        })
                        .unwrap();
                    rejected.frame.release(messages);
                    imports.release(reference);
                    imports.pending_release(0).unwrap().commit();
                    assert!(imports.is_empty());
                })
                .unwrap();
            })
            .unwrap();
        })
        .unwrap();
    })
    .unwrap();
}

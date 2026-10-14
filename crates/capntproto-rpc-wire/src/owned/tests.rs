use super::*;
use capnp::{message::FixedBuilder, word, Word};
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use rpc_capnp::api::{Call as WireCall, CapDescriptor, Message, MessageTarget, Payload};
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);

#[test]
fn complete_frame_copy_validates_framing_limits_and_recovers_fixed_storage() {
    let mut words = [ZERO; 16];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
    let mut source = [0u8; 32];
    source[4..8].copy_from_slice(&2u32.to_le_bytes());
    messages::with_messages(&mut buffers, &mut [], |messages| {
        for (length, limit, valid) in [
            (7, 2, false),
            (16, 2, false),
            (24, 1, false),
            (32, 2, false),
            (24, 2, true),
        ] {
            let result = Frame::copy_from(
                messages,
                &source[..length],
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(limit),
                    ..Default::default()
                },
            );
            assert_eq!(result.is_ok(), valid);
            if let Ok(frame) = result {
                assert_eq!(frame.size_in_words(messages), Ok(2));
                assert!(frame.single_bytes.get().is_some());
                frame.with(messages, |_| ()).unwrap();
                frame.release(messages);
            }
            let writer = messages
                .reserve(128)
                .expect("every rejected or released copy recovers storage");
            messages.discard(writer);
        }
    })
    .unwrap();
}

#[test]
fn cached_frame_layout_preserves_framing_and_pointer_validation() {
    for (length, limit) in [(24, 2), (32, 2), (24, 1)] {
        let mut words = [ZERO; 8];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let writer = messages.reserve(32).unwrap();
            {
                let mut bytes = messages.write(&writer);
                bytes[..32].fill(0);
                bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
                // Well-framed, but its nonempty root struct points beyond the segment.
                bytes[8..12].copy_from_slice(&400u32.to_le_bytes());
                bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
            }
            let frame = Frame::new(
                messages.seal(writer, length).unwrap(),
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(limit),
                    ..Default::default()
                },
            );
            for _ in 0..3 {
                let size = frame.size_in_words(messages);
                if length == 24 && limit == 2 {
                    assert_eq!(size, Ok(2));
                    assert!(frame.single_bytes.get().is_some());
                } else {
                    assert!(size.is_err());
                    assert!(frame.single_bytes.get().is_none());
                }
                assert!(frame.with(messages, |_| ()).is_err());
                assert!(frame.with_size(messages, |_, _| ()).is_err());
            }
            frame.release(messages);
        })
        .unwrap();
    }
}

#[test]
fn empty_descriptor_call_rejects_capability_pointer_before_ledger_commit() {
    let mut words = [ZERO; 128];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        let mut ee = [exports::Slot::EMPTY];
        let mut aa = [answers::Slot::EMPTY];
        capntproto_rpc_engine::connection::with_connection(
            capntproto_rpc_engine::connection::Storage {
                imports: &mut [],
                exports: &mut ee,
                answers: &mut aa,
                questions: &mut [],
            },
            |connection: &mut capntproto_rpc_engine::connection::Connection<'_, '_, (), (), ()>| {
                let mut batch = connection.exports.batch();
                let grant = batch.export(()).unwrap();
                batch.commit();
                let frame = Frame::build(messages, 512, Default::default(), |builder| {
                    calls::dynamic_call(
                        builder,
                        capntproto_rpc_engine::ids::QuestionId::from_wire(5),
                        calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                        calls::DynamicCall::default(),
                        |content| content.set_capability_index(0),
                    )
                })
                .unwrap();
                let Err(rejected) = Call::<_, 1, 0>::admit(
                    frame,
                    messages,
                    &mut connection.imports,
                    &mut connection.exports,
                    &mut connection.answers,
                    (),
                    |_| panic!("empty descriptor factory"),
                ) else {
                    panic!("ungranted capability admitted");
                };
                assert!(connection.imports.is_empty() && connection.answers.is_empty());
                rejected.frame.release(messages);
                assert_eq!(connection.exports.release(grant.id(), 1), Ok(Some(())));
                let writer = messages.reserve(512).unwrap();
                messages.discard(writer);
            },
        )
        .unwrap();
    })
    .unwrap();
}

#[test]
fn frame_word_accounting_includes_unreachable_segments_without_heap_allocation() {
    let mut bytes = [ZERO; 16];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        let writer = messages.reserve(64).unwrap();
        {
            let mut view = messages.write(&writer);
            view[..64].fill(0);
            view[..4].copy_from_slice(&1u32.to_le_bytes()); // two segments
            view[4..8].copy_from_slice(&1u32.to_le_bytes()); // null root
            view[8..12].copy_from_slice(&5u32.to_le_bytes()); // unreachable words
        }
        let frame = Frame::new(messages.seal(writer, 64).unwrap(), Default::default());
        assert_eq!(frame.size_in_words(messages).unwrap(), 6);
        assert_eq!(frame.size_in_words(messages).unwrap(), 6);
        assert!(frame.single_bytes.get().is_none());
        frame.release(messages);
    })
    .unwrap();
}

fn encode<'p>(
    messages: &mut messages::Messages<'p, '_, '_>,
    target: ExportId,
    caps: bool,
    promised: bool,
) -> Frame<'p> {
    let mut source = [ZERO; 80];
    let mut builder = FixedBuilder::new(&mut source).unwrap();
    let mut root = builder.try_init_root::<Message>().unwrap();
    let mut call = root.field(Message::CALL).init().unwrap();
    call.scalar(WireCall::QUESTION_ID).set(5);
    call.scalar(WireCall::INTERFACE_ID).set(19);
    call.scalar(WireCall::METHOD_ID).set(7);
    let mut destination = call.field(WireCall::TARGET).init().unwrap();
    if promised {
        pipeline::Path::new(&[1, 2])
            .write(
                destination
                    .field(MessageTarget::PROMISED_ANSWER)
                    .init()
                    .unwrap(),
                capntproto_rpc_engine::ids::QuestionId::from_wire(3),
            )
            .unwrap();
    } else {
        destination
            .scalar(MessageTarget::IMPORTED_CAP)
            .set(target.to_wire());
    }
    if caps {
        let mut payload = call.field(WireCall::PARAMS).init().unwrap();
        payload
            .field(Payload::CONTENT)
            .set_capability_index(0)
            .unwrap();
        let mut table = payload.field(Payload::CAP_TABLE).init(4).unwrap();
        table
            .get_mut(0)
            .unwrap()
            .scalar(CapDescriptor::SENDER_PROMISE)
            .set(7);
        table
            .get_mut(1)
            .unwrap()
            .scalar(CapDescriptor::SENDER_HOSTED)
            .set(7);
        table
            .get_mut(2)
            .unwrap()
            .scalar(CapDescriptor::RECEIVER_HOSTED)
            .set(target.to_wire());
        pipeline::Path::new(&[2])
            .write(
                table
                    .get_mut(3)
                    .unwrap()
                    .field(CapDescriptor::RECEIVER_ANSWER)
                    .init()
                    .unwrap(),
                capntproto_rpc_engine::ids::QuestionId::from_wire(3),
            )
            .unwrap();
    }
    let writer = messages.reserve(81 * 8).unwrap();
    let mut view = messages.write(&writer);
    let capacity = view.len();
    let mut bytes = &mut *view;
    capnp::serialize::write_message_segments(&mut bytes, &builder.get_segments_for_output())
        .unwrap();
    let used = capacity - bytes.len();
    drop(view);
    Frame::new(messages.seal(writer, used).unwrap(), Default::default())
}

#[test]
fn owned_request_retains_frame_receipts_and_target_while_other_io_progresses() {
    let measured = allocation_counter::measure(|| {
        let mut a = [ZERO; 81];
        let mut b = [ZERO; 81];
        let mut buffers = [
            messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
            messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        ];
        let mut sends = [messages::SendSlot::EMPTY];
        messages::with_messages(&mut buffers, &mut sends, |messages| {
            let mut ii = [imports::Slot::EMPTY];
            let mut ee = [exports::Slot::EMPTY];
            let mut aa = [const { answers::Slot::EMPTY }; 2];
            capntproto_rpc_engine::connection::with_connection(
                capntproto_rpc_engine::connection::Storage {
                    imports: &mut ii,
                    exports: &mut ee,
                    answers: &mut aa,
                    questions: &mut [],
                },
                |connection| {
                    let capntproto_rpc_engine::connection::Connection {
                        imports,
                        exports,
                        answers,
                        ..
                    } = connection;
                    let mut batch = exports.batch();
                    let grant = batch.export(41).unwrap();
                    batch.commit();
                    let parent = answers.admit(AnswerId::from_wire(3), 43).unwrap();
                    let frame = encode(messages, grant.id(), true, true);
                    let mut made = 0;
                    let request = Call::<_, 5, 2>::admit(
                        frame,
                        messages,
                        imports,
                        exports,
                        answers,
                        47,
                        |_| {
                            made += 1;
                            53
                        },
                    )
                    .unwrap_or_else(|e| panic!("{}", e.error));
                    assert_eq!(made, 1);
                    assert_eq!(request.answer().id().to_wire(), 5);
                    let TargetRef::Promised { path, .. } = request.target() else {
                        panic!("promised target")
                    };
                    assert_eq!(path.fields(), &[1, 2]);
                    assert_eq!(exports.release(grant.id(), 1), Ok(None));
                    assert!(!answers.finish(parent.id(), false).cancel);
                    assert!(answers.dispatch(parent, true).unwrap());
                    assert_eq!(answers.prepare_return(parent).unwrap().accept(), None);
                    {
                        let future = request.with_async(messages, async |view| {
                            let pointer = view
                                .incoming()
                                .payload()
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                                .unwrap();
                            let reception::full::Capability::Remote(local) =
                                view.capability(pointer).unwrap()
                            else {
                                panic!("import")
                            };
                            assert_eq!(imports.wire_id(local).to_wire(), 7);
                            assert_eq!(
                                imports.promise_state(local),
                                capntproto_rpc_engine::promise::ImportState::Pending
                            );
                            let mut yielded = false;
                            core::future::poll_fn(|cx| {
                                if yielded {
                                    Poll::Ready(())
                                } else {
                                    yielded = true;
                                    cx.waker().wake_by_ref();
                                    Poll::Pending
                                }
                            })
                            .await;
                            assert_eq!(view.incoming().interface_id(), 19);
                            assert_eq!(view.incoming().method_id(), 7);
                            assert!(matches!(
                                view.capability(pointer),
                                Ok(reception::full::Capability::Remote(_))
                            ));
                        });
                        let mut future = core::pin::pin!(future);
                        let mut cx = Context::from_waker(Waker::noop());
                        assert!(future.as_mut().poll(&mut cx).is_pending());
                        let other = messages.reserve(81 * 8).unwrap();
                        assert_eq!(messages.reserve(1).err(), Some(messages::Error::NoBuffer));
                        messages.write(&other)[..4].copy_from_slice(b"side");
                        let lease = messages.seal(other, 4).unwrap();
                        let offer = messages.offer(&lease).unwrap();
                        let send = messages.accept(offer);
                        messages.release(lease);
                        assert!(!messages.send_window(send).unwrap().advance(1).unwrap());
                        assert!(messages.send_window(send).unwrap().advance(3).unwrap());
                        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
                    }
                    let key = request.answer();
                    request.release(messages, imports, exports, answers);
                    assert_eq!(exports.take_retired(0), Some(41));
                    assert_eq!(answers.take_finished(0), Some(43));
                    // Resolve is still valid until the last wire Release commits,
                    // even though no local observer remains.
                    imports
                        .prepare_resolution(ImportId::from_wire(7), &mut [])
                        .unwrap()
                        .commit();
                    let release = imports.pending_release(0).unwrap();
                    assert_eq!(release.count(), 2);
                    assert_eq!(release.commit(), 53);
                    answers.finish(key.id(), false);
                    assert_eq!(answers.prepare_return(key).unwrap().accept(), Some(47));
                    let free = messages.reserve(81 * 8).unwrap();
                    messages.discard(free);
                    assert!(imports.is_empty() && exports.is_empty() && answers.is_empty());
                },
            )
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_plain_call_keeps_target_alive_without_allocating_receipt_storage() {
    let measured = allocation_counter::measure(|| {
        for promised in [false, true] {
            let mut words = [ZERO; 81];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
            messages::with_messages(&mut buffers, &mut [], |messages| {
                let mut ee = [exports::Slot::EMPTY];
                let mut aa = [const { answers::Slot::EMPTY }; 2];
                capntproto_rpc_engine::connection::with_connection(
                    capntproto_rpc_engine::connection::Storage {
                        imports: &mut [],
                        exports: &mut ee,
                        answers: &mut aa,
                        questions: &mut [],
                    },
                    |connection: &mut capntproto_rpc_engine::connection::Connection<
                        '_,
                        '_,
                        (),
                        u32,
                        u32,
                    >| {
                        let mut batch = connection.exports.batch();
                        let grant = batch.export(41).unwrap();
                        batch.commit();
                        let parent = connection
                            .answers
                            .admit(AnswerId::from_wire(3), 43)
                            .unwrap();
                        let frame = encode(messages, grant.id(), false, promised);
                        // A temporary target still consumes the public receipt
                        // budget, despite no longer needing persistent storage.
                        let rejected = Call::<_, 0, 2>::admit_with_capacity(
                            frame,
                            messages,
                            &mut connection.imports,
                            &mut connection.exports,
                            &mut connection.answers,
                            47,
                            |_| panic!("no descriptors"),
                            Capacity::Growing { maximum: 0 },
                            Capacity::Inline,
                        )
                        .err()
                        .expect("target exceeds zero budget");
                        assert_eq!(rejected.error, Error::Capacity);
                        assert_eq!(rejected.owner, 47);
                        let call = Call::<_, 0, 2>::admit_with_capacity(
                            rejected.frame,
                            messages,
                            &mut connection.imports,
                            &mut connection.exports,
                            &mut connection.answers,
                            rejected.owner,
                            |_| panic!("no descriptors"),
                            Capacity::Growing { maximum: 1 },
                            Capacity::Inline,
                        )
                        .unwrap_or_else(|e| panic!("{}", e.error));
                        match call.target() {
                            TargetRef::Hosted(_) => assert!(!promised),
                            TargetRef::Promised { path, .. } => {
                                assert!(promised);
                                assert_eq!(path.fields(), &[1, 2]);
                            }
                        }
                        assert_eq!(
                            connection.exports.release(grant.id(), 1),
                            Ok(promised.then_some(41))
                        );
                        connection.answers.finish(parent.id(), false);
                        assert_eq!(connection.answers.dispatch(parent, true).unwrap(), promised);
                        assert_eq!(
                            connection.answers.prepare_return(parent).unwrap().accept(),
                            (!promised).then_some(43)
                        );
                        let key = call.answer();
                        call.release(
                            messages,
                            &mut connection.imports,
                            &mut connection.exports,
                            &mut connection.answers,
                        );
                        if promised {
                            assert_eq!(connection.answers.take_finished(0), Some(43));
                        } else {
                            assert_eq!(connection.exports.take_retired(0), Some(41));
                        }
                        connection.answers.finish(key.id(), false);
                        assert_eq!(
                            connection.answers.prepare_return(key).unwrap().accept(),
                            Some(47)
                        );
                        assert!(connection.imports.is_empty());
                        assert!(connection.exports.is_empty());
                        assert!(connection.answers.is_empty());
                        let writer = messages.reserve(81 * 8).unwrap();
                        messages.discard(writer);
                    },
                )
                .unwrap();
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn rejected_owned_admission_preserves_frame_answer_owner_and_all_tables() {
    for failure in 0..5 {
        let mut bytes = [ZERO; 81];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let mut ii = [imports::Slot::<u32>::EMPTY];
            let mut ee = [exports::Slot::EMPTY];
            let mut aa = [const { answers::Slot::EMPTY }; 2];
            capntproto_rpc_engine::connection::with_connection(
                capntproto_rpc_engine::connection::Storage {
                    imports: &mut ii,
                    exports: &mut ee,
                    answers: &mut aa,
                    questions: &mut [],
                },
                |connection| {
                    let capntproto_rpc_engine::connection::Connection {
                        imports,
                        exports,
                        answers,
                        ..
                    } = connection;
                    let mut batch = exports.batch();
                    let grant = batch.export(41).unwrap();
                    batch.commit();
                    let parent = answers
                        .admit(AnswerId::from_wire(if failure == 2 { 5 } else { 3 }), 43)
                        .unwrap();
                    let old =
                        (failure == 4).then(|| imports.admit(ImportId::from_wire(99), 59).unwrap());
                    let target = if failure == 3 {
                        ExportId::from_wire(99)
                    } else {
                        grant.id()
                    };
                    let frame = encode(messages, target, true, failure == 1);
                    let result = if failure == 0 {
                        Call::<_, 4, 1>::admit(
                            frame,
                            messages,
                            imports,
                            exports,
                            answers,
                            47,
                            |_| 53,
                        )
                    } else {
                        Call::<_, 5, 1>::admit(
                            frame,
                            messages,
                            imports,
                            exports,
                            answers,
                            47,
                            |_| 53,
                        )
                        .map(|_| panic!("expected admission rejection"))
                    };
                    let Err(rejected) = result else {
                        panic!("unexpected acceptance")
                    };
                    assert_eq!(rejected.owner, 47);
                    assert_eq!(imports.len(), usize::from(failure == 4));
                    assert_eq!(answers.len(), 1);
                    assert_eq!(exports.release(grant.id(), 1), Ok(Some(41)));
                    assert_eq!(messages.reserve(1).err(), Some(messages::Error::NoBuffer));
                    rejected
                        .frame
                        .with(messages, |root| {
                            assert!(matches!(root.which(), Ok(rpc_capnp::message::Call(_))))
                        })
                        .unwrap();
                    rejected.frame.release(messages);
                    let writer = messages.reserve(1).unwrap();
                    messages.discard(writer);
                    answers.finish(parent.id(), false);
                    assert_eq!(answers.prepare_return(parent).unwrap().accept(), Some(43));
                    if let Some(old) = old {
                        assert_eq!(*imports.get(&old), 59);
                        imports.release(old);
                        assert_eq!(imports.pending_release(0).unwrap().commit(), 59);
                    }
                },
            )
            .unwrap();
        })
        .unwrap();
    }
}

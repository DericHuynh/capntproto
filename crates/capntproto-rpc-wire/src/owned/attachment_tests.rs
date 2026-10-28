use super::*;
use crate::attachments::{ImportReceiver, Receiver};
use capntproto_rpc_engine::{connection, questions};
use core::cell::Cell;
use rpc_capnp::api::{
    Call as WireCall, CapDescriptor, Message, MessageTarget, Payload, Resolve, Return,
};

#[derive(Debug)]
struct Resource<'a>(u32, &'a Cell<u32>);
impl Drop for Resource<'_> {
    fn drop(&mut self) {
        self.1.set(self.1.get() | self.0);
    }
}
fn keep_first<'a>(owner: &mut Option<Resource<'a>>, resource: Resource<'a>) {
    if owner.is_none() {
        *owner = Some(resource);
    }
}

#[test]
fn rejected_call_preserves_attachments_and_accepted_aliases_deliver_once_without_allocating() {
    let measured = allocation_counter::measure(|| {
        let dropped = Cell::new(0);
        let mut resources = core::array::from_fn::<_, 4, _>(|i| Some(Resource(1 << i, &dropped)));
        let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 128];
        let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
            &mut words,
        ))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let mut ii = [const { imports::Slot::EMPTY }; 3];
            let mut ee = [const { exports::Slot::EMPTY }; 2];
            let mut aa = [answers::Slot::EMPTY];
            connection::with_connection(
                connection::Storage {
                    imports: &mut ii,
                    exports: &mut ee,
                    answers: &mut aa,
                    questions: &mut [],
                    pipeline_questions: &mut [],
                },
                |connection| {
                    let connection::Connection {
                        imports,
                        exports,
                        answers,
                        ..
                    } = connection;
                    let mut batch = exports.batch();
                    let target = batch.export(()).unwrap();
                    batch.commit();
                    let frame = Frame::build(messages, 1024, Default::default(), |builder| {
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut call = root.field(Message::CALL).init()?;
                        call.scalar(WireCall::QUESTION_ID).set(5);
                        call.field(WireCall::TARGET)
                            .init()?
                            .scalar(MessageTarget::IMPORTED_CAP)
                            .set(target.id().to_wire());
                        let mut payload = call.field(WireCall::PARAMS).init()?;
                        let mut table = payload.field(Payload::CAP_TABLE).init(7)?;
                        for (index, (id, attachment)) in
                            [(7, 0), (7, 1), (0, 2), (8, 0), (8, 9), (9, 255), (1, 255)]
                                .into_iter()
                                .enumerate()
                        {
                            let mut descriptor = table.get_mut(index).unwrap();
                            match index {
                                0 => descriptor.scalar(CapDescriptor::SENDER_PROMISE).set(id),
                                2 => descriptor.void(CapDescriptor::NONE).set(),
                                6 => descriptor.scalar(CapDescriptor::RECEIVER_HOSTED).set(id),
                                _ => descriptor.scalar(CapDescriptor::SENDER_HOSTED).set(id),
                            }
                            descriptor
                                .scalar(CapDescriptor::ATTACHED_FD)
                                .set(attachment);
                        }
                        Ok(())
                    })
                    .unwrap();
                    // The final reflected descriptor is missing. Every prior import
                    // factory may run, but no resource or remote reference commits.
                    let rejected = Call::<_, 8, 0>::admit_with_receiver(
                        frame,
                        messages,
                        imports,
                        exports,
                        answers,
                        (),
                        &mut Receiver::new(&mut resources, |_| None, keep_first),
                        Capacity::Inline,
                        Capacity::Inline,
                    )
                    .err()
                    .unwrap();
                    assert!(resources.iter().all(Option::is_some));
                    assert_eq!(dropped.get(), 0);
                    assert!(imports.is_empty());
                    let mut batch = exports.batch();
                    let reflected = batch.export(()).unwrap();
                    assert_eq!(reflected.id().to_wire(), 1);
                    batch.commit();
                    let call = Call::<_, 8, 0>::admit_with_receiver(
                        rejected.frame,
                        messages,
                        imports,
                        exports,
                        answers,
                        (),
                        &mut Receiver::new(&mut resources, |_| None, keep_first),
                        Capacity::Inline,
                        Capacity::Inline,
                    )
                    .unwrap_or_else(|e| panic!("{}", e.error));
                    assert!(resources[..3].iter().all(Option::is_none));
                    assert!(resources[3].is_some());
                    assert_eq!(dropped.get(), 2 | 4);
                    let retained = call
                        .with(messages, |view| {
                            let Some(reception::full::Capability::Remote(local)) =
                                view.capability_at(0).unwrap()
                            else {
                                panic!("remote")
                            };
                            assert_eq!(imports.get(local).as_ref().unwrap().0, 1);
                            assert_eq!(
                                imports.promise_state(local),
                                capntproto_rpc_engine::promise::ImportState::Pending
                            );
                            imports.retain(local).unwrap()
                        })
                        .unwrap();
                    let answer = call.answer();
                    call.release(messages, imports, exports, answers);
                    // Frame reuse and dropping the receive buffer's leftovers do
                    // not close the resource kept by an independent capability.
                    let writer = messages.reserve(1024).unwrap();
                    messages.discard(writer);
                    drop(resources[3].take());
                    assert_eq!(dropped.get(), 2 | 4 | 8);
                    assert_eq!(imports.get(&retained).as_ref().unwrap().0, 1);
                    imports.release(retained);
                    for index in 0..imports.capacity() {
                        if let Some(release) = imports.pending_release(index) {
                            drop(release.commit());
                        }
                    }
                    assert_eq!(dropped.get(), 15);
                    assert!(imports.is_empty());
                    answers.finish(answer.id(), false);
                    let _ = exports.release(target.id(), 1);
                    let _ = exports.release(reflected.id(), 1);
                },
            )
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn attachment_sentinel_and_duplicate_indices_never_duplicate_ownership() {
    let mut slots = [Some(7), Some(11)];
    let mut owner = 0;
    let mut receiver = Receiver::new(
        &mut slots,
        |_| 0,
        |value: &mut u32, resource| *value += resource,
    );
    receiver.attach(Some(&mut owner), 255);
    receiver.attach(Some(&mut owner), 2);
    receiver.attach(Some(&mut owner), 0);
    receiver.attach(Some(&mut owner), 0);
    receiver.attach(None, 1);
    receiver.attach(Some(&mut owner), 1);
    assert_eq!(owner, 7);
    assert_eq!(slots, [None, None]);
    // Sentinel remains absent even if an adapter supplied too many resources.
    let mut oversized = [Some(1); 256];
    Receiver::new(
        &mut oversized,
        |_| 0,
        |value: &mut u32, resource| *value += resource,
    )
    .attach(Some(&mut owner), 255);
    assert_eq!(owner, 7);
    assert_eq!(oversized[255], Some(1));
}

#[test]
fn resolve_delivers_later_alias_attachment_without_replacing_the_import_owner() {
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
        &mut words,
    ))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        imports::with_imports(&mut [const { imports::Slot::EMPTY }; 2], |imports| {
            exports::with_exports(&mut [], |exports: &mut exports::Exports<'_, '_, ()>| {
                answers::with_answers(&mut [], |answers: &mut answers::Answers<'_, '_, ()>| {
                    let mut receipts = [
                        imports::ReceiveSlot::new_promise(ImportId::from_wire(3), None),
                        imports::ReceiveSlot::new(ImportId::from_wire(9), None),
                    ];
                    imports.prepare_receive(&mut receipts).unwrap().commit();
                    let pending = receipts[0].take_local().unwrap();
                    let existing = receipts[1].take_local().unwrap();
                    let frame = Frame::build(messages, 512, Default::default(), |builder| {
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut resolution = root.field(Message::RESOLVE).init()?;
                        resolution.scalar(Resolve::PROMISE_ID).set(3);
                        let mut descriptor = resolution.field(Resolve::CAP).init()?;
                        descriptor.scalar(CapDescriptor::SENDER_HOSTED).set(9);
                        descriptor.scalar(CapDescriptor::ATTACHED_FD).set(0);
                        Ok(())
                    })
                    .unwrap();
                    let mut resources = [Some(123)];
                    let resolution = Resolution::admit_with_receiver(
                        frame,
                        messages,
                        imports,
                        exports,
                        answers,
                        &mut Receiver::new(
                            &mut resources,
                            |_| panic!("existing import"),
                            |owner: &mut Option<u32>, value| {
                                *owner = Some(value);
                            },
                        ),
                    )
                    .unwrap_or_else(|e| panic!("{}", e.error));
                    assert_eq!(imports.get(&existing), &Some(123));
                    assert_eq!(resources, [None]);
                    resolution.release(messages, imports, exports, answers);
                    assert_eq!(imports.get(&existing), &Some(123));
                    imports.release(pending);
                    imports.release(existing);
                    for index in 0..imports.capacity() {
                        imports.pending_release(index).unwrap().commit();
                    }
                })
                .unwrap();
            })
            .unwrap();
        })
        .unwrap();
    })
    .unwrap();
}

#[test]
fn returned_resources_commit_with_live_results_and_remain_with_canceled_input() {
    let measured = allocation_counter::measure(|| {
        for canceled in [false, true] {
            let mut a = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
            let mut b = a;
            let mut buffers = [
                messages::Buffer::new(capnp::Word::words_to_bytes_mut(&mut a)),
                messages::Buffer::new(capnp::Word::words_to_bytes_mut(&mut b)),
            ];
            messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
                connection::with_connection(
                    connection::Storage {
                        imports: &mut [imports::Slot::EMPTY],
                        exports: &mut [],
                        answers: &mut [],
                        questions: &mut [questions::Slot::EMPTY],
                        pipeline_questions: &mut [],
                    },
                    |connection: &mut connection::Connection<'_, '_, Option<u32>, (), ()>| {
                        let connection::Connection {
                            imports,
                            exports,
                            answers,
                            questions,
                        } = connection;
                        let reservation = questions.reserve().unwrap();
                        let id = reservation.id();
                        let frame = Frame::build(messages, 512, Default::default(), |builder| {
                            calls::dynamic_call(
                                builder,
                                id,
                                calls::Target::Imported(ImportId::from_wire(9)),
                                calls::DynamicCall::default(),
                                |_| Ok(()),
                            )
                        })
                        .unwrap();
                        let mut queue_slots = [None];
                        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                        let mut question =
                            Question::send(reservation, messages, frame, queue.reserve().unwrap())
                                .unwrap();
                        while let Some(window) = queue.window(messages).unwrap() {
                            let n = window.bytes().len();
                            window.advance(n).unwrap();
                        }
                        if canceled {
                            question.cancel(questions);
                        }
                        let frame = Frame::build(messages, 512, Default::default(), |builder| {
                            let mut root = builder.try_init_root::<Message>()?;
                            let mut result = root.field(Message::RETURN).init()?;
                            result.scalar(Return::ANSWER_ID).set(id.to_wire());
                            let mut payload = result.field(Return::RESULTS).init()?;
                            let mut table = payload.field(Payload::CAP_TABLE).init(1)?;
                            let mut descriptor = table.get_mut(0).unwrap();
                            descriptor.scalar(CapDescriptor::SENDER_HOSTED).set(17);
                            descriptor.scalar(CapDescriptor::ATTACHED_FD).set(0);
                            Ok(())
                        })
                        .unwrap();
                        let mut resources = [Some(321)];
                        let response = question
                            .returned_with_receiver::<_, _, _, 1>(
                                frame,
                                messages,
                                questions,
                                imports,
                                exports,
                                answers,
                                &mut [],
                                &mut Receiver::new(
                                    &mut resources,
                                    |_| None,
                                    |owner: &mut Option<u32>, value| {
                                        *owner = Some(value);
                                    },
                                ),
                                Capacity::Inline,
                            )
                            .unwrap_or_else(|e| panic!("{}", e.error));
                        assert_eq!(resources[0], if canceled { Some(321) } else { None });
                        assert_eq!(imports.len(), usize::from(!canceled));
                        response.release(messages, questions, imports, exports, answers);
                        if !canceled {
                            assert_eq!(imports.pending_release(0).unwrap().commit(), Some(321));
                        }
                    },
                )
                .unwrap();
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

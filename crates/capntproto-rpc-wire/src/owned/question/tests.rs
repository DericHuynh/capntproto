use super::*;
use capnp::{message::FixedBuilder, word, Word};
use capntproto_rpc_engine::connection;
use rpc_capnp::api::{Call, CapDescriptor, Message, MessageTarget, Payload, Return};
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);

#[cfg(feature = "alloc")]
#[test]
fn retained_plain_results_pin_the_pool_and_survive_connection_teardown() {
    use capnp::private::capability::ResponseHook;
    let owner = alloc::rc::Rc::new(());
    let weak = alloc::rc::Rc::downgrade(&owner);
    let mut pool = messages::GrowingBuffers::new(1, 392, 399);
    let growth = connection::Growth {
        initial: 0,
        maximum: 1,
    };
    let plain = messages::with_growing_messages(&mut pool, growth, |messages| {
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        connection::with_connection(
            connection::Storage {
                imports: &mut [],
                exports: &mut [],
                questions: &mut [questions::Slot::EMPTY],
                answers: &mut [],
            },
            |connection: &mut connection::Connection<'_, '_, (), (), ()>| {
                let connection::Connection {
                    questions,
                    imports,
                    exports,
                    answers,
                } = connection;
                let reservation = questions.reserve().unwrap();
                let id = reservation.id();
                let frame = encode(messages, |b| {
                    calls::dynamic_call(
                        b,
                        id,
                        calls::Target::Imported(ImportId::from_wire(9)),
                        calls::DynamicCall {
                            interface: 7,
                            method: 2,
                            no_promise_pipelining: true,
                            only_promise_pipeline: false,
                        },
                        |_| Ok(()),
                    )
                    .unwrap();
                });
                let question = Question::publish_template(
                    reservation,
                    messages,
                    frame,
                    queue.reserve().unwrap(),
                    (7, 2),
                    false,
                    false,
                )
                .unwrap();
                flush(messages, &mut queue);
                let frame = encode(messages, |b| {
                    calls::dynamic_results(b, AnswerId::from_wire(id.to_wire()), |field| {
                        field
                            .cast::<capnp::field_api::Data>()
                            .init(8)?
                            .copy_from_slice(b"retained");
                        Ok(())
                    })
                    .unwrap();
                });
                let mut response = question
                    .returned::<_, _, _, 0>(
                        frame,
                        messages,
                        questions,
                        imports,
                        exports,
                        answers,
                        &mut [],
                        |_| (),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                let mut plain = response.take_plain_results().unwrap();
                let info = allocation_counter::measure(|| plain.retain_owner(owner));
                assert_eq!(info.count_total, 0);
                assert!(response.take_plain_results().is_none());
                response.release(messages, questions, imports, exports, answers);
                questions.pending_finish().unwrap().accept();
                assert!(questions.is_empty());
                assert_eq!(messages.reserve(8).unwrap_err(), messages::Error::NoBuffer);
                plain
            },
        )
        .unwrap()
    })
    .unwrap();
    messages::with_growing_messages(&mut pool, growth, |messages| {
        assert_eq!(messages.reserve(8).unwrap_err(), messages::Error::NoBuffer);
    })
    .unwrap();
    drop(pool);
    assert_eq!(
        plain
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader<'_>>()
            .unwrap(),
        b"retained"
    );
    assert!(weak.upgrade().is_some());
    drop(plain);
    assert!(weak.upgrade().is_none());
}

#[test]
fn empty_descriptor_results_validate_content_for_live_and_canceled_questions() {
    for plain in [false, true] {
        for canceled in [false, true] {
            let mut words = [[ZERO; 49]; 2];
            let mut buffers = words
                .each_mut()
                .map(|words| messages::Buffer::new(Word::words_to_bytes_mut(words)));
            messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
                let mut queue_slots = [None];
                let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                let mut qq = [questions::Slot::EMPTY];
                connection::with_connection(
                    connection::Storage {
                        imports: &mut [],
                        exports: &mut [],
                        questions: &mut qq,
                        answers: &mut [],
                    },
                    |connection: &mut connection::Connection<'_, '_, (), (), ()>| {
                        let connection::Connection {
                            questions,
                            imports,
                            exports,
                            answers,
                        } = connection;
                        let reservation = questions.reserve().unwrap();
                        let id = reservation.id();
                        let mut question = if plain {
                            let frame = encode(messages, |b| {
                                calls::dynamic_call(
                                    b,
                                    id,
                                    calls::Target::Imported(ImportId::from_wire(9)),
                                    calls::DynamicCall {
                                        interface: 7,
                                        method: 2,
                                        no_promise_pipelining: true,
                                        only_promise_pipeline: false,
                                    },
                                    |_| Ok(()),
                                )
                                .unwrap()
                            });
                            Question::publish_template(
                                reservation,
                                messages,
                                frame,
                                queue.reserve().unwrap(),
                                (7, 2),
                                false,
                                false,
                            )
                            .unwrap()
                        } else {
                            let frame = encode(messages, |b| calls::bootstrap(b, id).unwrap());
                            Question::send(reservation, messages, frame, queue.reserve().unwrap())
                                .unwrap()
                        };
                        flush(messages, &mut queue);
                        if plain {
                            // After physical output, a data-only question keeps
                            // its admitted metadata but no longer pins either
                            // buffer. Reuse both before validating a late Return.
                            let first = messages.reserve(49 * 8).unwrap();
                            let second = messages.reserve(49 * 8).unwrap();
                            messages.discard(first);
                            messages.discard(second);
                        }
                        if canceled {
                            question.cancel(questions);
                        }
                        let bad = encode(messages, |b| {
                            calls::dynamic_results(b, AnswerId::from_wire(id.to_wire()), |field| {
                                field.set_capability_index(0)
                            })
                            .unwrap()
                        });
                        let Err(rejected) = question.returned::<_, _, _, 0>(
                            bad,
                            messages,
                            questions,
                            imports,
                            exports,
                            answers,
                            &mut [],
                            |_| panic!("empty descriptor factory"),
                        ) else {
                            panic!("ungranted capability admitted");
                        };
                        assert_eq!(questions.is_live(rejected.question.key()), !canceled);
                        assert!(imports.is_empty() && exports.is_empty() && answers.is_empty());
                        rejected.frame.release(messages);
                        let good = encode(messages, |b| {
                            calls::acknowledge(b, AnswerId::from_wire(id.to_wire())).unwrap()
                        });
                        let response = rejected
                            .question
                            .returned::<_, _, _, 0>(
                                good,
                                messages,
                                questions,
                                imports,
                                exports,
                                answers,
                                &mut [],
                                |_| (),
                            )
                            .unwrap_or_else(|r| panic!("{}", r.error));
                        response.release(messages, questions, imports, exports, answers);
                        questions.pending_finish().unwrap().accept();
                        assert!(questions.is_empty());
                    },
                )
                .unwrap();
            })
            .unwrap();
        }
    }
}

#[test]
fn stale_owning_question_cannot_apply_return_to_recycled_wire_id() {
    let mut a = [ZERO; 49];
    let mut b = [ZERO; 49];
    let mut c = [ZERO; 49];
    let mut buffers = [
        messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut c)),
    ];
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
        let mut qq = [questions::Slot::EMPTY];
        connection::with_connection(
            connection::Storage {
                imports: &mut [],
                exports: &mut [],
                questions: &mut qq,
                answers: &mut [],
            },
            |connection: &mut connection::Connection<'_, '_, (), (), ()>| {
                let connection::Connection {
                    questions,
                    imports,
                    exports,
                    answers,
                } = connection;
                let reservation = questions.reserve().unwrap();
                let id = reservation.id();
                let frame = encode(messages, |b| calls::bootstrap(b, id).unwrap());
                let mut old =
                    Question::send(reservation, messages, frame, queue.reserve().unwrap()).unwrap();
                flush(messages, &mut queue);
                old.cancel(questions);
                questions.pending_finish().unwrap().accept();
                // A lower-level consumer has already processed this Return.
                questions.returned(id, false).unwrap();
                let reservation = questions.reserve().unwrap();
                assert_eq!(reservation.id(), id);
                let frame = encode(messages, |b| calls::bootstrap(b, id).unwrap());
                let new =
                    Question::send(reservation, messages, frame, queue.reserve().unwrap()).unwrap();
                flush(messages, &mut queue);
                let key = new.key();
                let frame = encode(messages, |b| {
                    calls::acknowledge(b, AnswerId::from_wire(id.to_wire())).unwrap()
                });
                let Err(rejected) = old.returned::<_, _, _, 0>(
                    frame,
                    messages,
                    questions,
                    imports,
                    exports,
                    answers,
                    &mut [],
                    |_| (),
                ) else {
                    panic!("stale Return accepted")
                };
                assert_eq!(rejected.error, Error::WrongMessage);
                assert!(questions.is_live(key));
                // This deliberately mixed low-level fixture detaches its orphan;
                // public callers must terminate on this inconsistent driver state.
                rejected.question.original.release(messages);
                let response = new
                    .returned::<_, _, _, 0>(
                        rejected.frame,
                        messages,
                        questions,
                        imports,
                        exports,
                        answers,
                        &mut [],
                        |_| (),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                response.release(messages, questions, imports, exports, answers);
                questions.pending_finish().unwrap().accept();
                assert!(questions.is_empty());
            },
        )
        .unwrap();
    })
    .unwrap();
}

fn encode<'p>(
    messages: &mut messages::Messages<'p, '_, '_>,
    f: impl FnOnce(&mut FixedBuilder<'_>),
) -> Frame<'p> {
    let mut words = [ZERO; 48];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    f(&mut builder);
    let writer = messages.reserve(49 * 8).unwrap();
    let mut view = messages.write(&writer);
    let capacity = view.len();
    let mut bytes = &mut *view;
    capnp::serialize::write_message_segments(&mut bytes, &builder.get_segments_for_output())
        .unwrap();
    let used = capacity - bytes.len();
    drop(view);
    Frame::new(messages.seal(writer, used).unwrap(), Default::default())
}
fn flush<'p>(messages: &mut messages::Messages<'p, '_, '_>, queue: &mut messages::Queue<'p, '_>) {
    while let Some(window) = queue.window(messages).unwrap() {
        let length = window.bytes().len();
        assert!(window.advance(length).unwrap());
    }
}

#[test]
fn exact_request_survives_mismatched_return_and_reflected_final_param_release() {
    let measured = allocation_counter::measure(|| {
        let mut a = [ZERO; 49];
        let mut b = [ZERO; 49];
        let mut buffers = [
            messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
            messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        ];
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            let mut queue_slots = [None];
            let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
            let mut ii = [imports::Slot::<()>::EMPTY];
            let mut ee = [exports::Slot::EMPTY];
            let mut qq = [questions::Slot::EMPTY];
            connection::with_connection(
                connection::Storage {
                    imports: &mut ii,
                    exports: &mut ee,
                    questions: &mut qq,
                    answers: &mut [],
                },
                |connection: &mut connection::Connection<'_, '_, (), i32, ()>| {
                    let connection::Connection {
                        questions,
                        imports,
                        exports,
                        answers,
                    } = connection;
                    let reservation = questions.reserve().unwrap();
                    let id = reservation.id();
                    let mut batch = exports.batch();
                    let grant = batch.export(41).unwrap();
                    let request = encode(messages, |builder| {
                        let mut root = builder.try_init_root::<Message>().unwrap();
                        let mut call = root.field(Message::CALL).init().unwrap();
                        call.scalar(Call::QUESTION_ID).set(id.to_wire());
                        call.field(Call::TARGET)
                            .init()
                            .unwrap()
                            .scalar(MessageTarget::IMPORTED_CAP)
                            .set(9);
                        let mut payload = call.field(Call::PARAMS).init().unwrap();
                        payload
                            .field(Payload::CONTENT)
                            .set_capability_index(0)
                            .unwrap();
                        payload
                            .field(Payload::CAP_TABLE)
                            .init(1)
                            .unwrap()
                            .get_mut(0)
                            .unwrap()
                            .scalar(CapDescriptor::SENDER_HOSTED)
                            .set(grant.id().to_wire());
                    });
                    let question = Question::send_granting(
                        reservation,
                        batch,
                        messages,
                        request,
                        &mut [0],
                        queue.reserve().unwrap(),
                    )
                    .unwrap();
                    let key = question.key();
                    flush(messages, &mut queue);
                    let wrong = encode(messages, |builder| {
                        calls::acknowledge(builder, AnswerId::from_wire(id.to_wire() + 1)).unwrap();
                    });
                    let Err(rejected) = question.returned::<_, _, _, 1>(
                        wrong,
                        messages,
                        questions,
                        imports,
                        exports,
                        answers,
                        &mut [0],
                        |_| (),
                    ) else {
                        panic!("wrong Return accepted")
                    };
                    assert_eq!(rejected.error, Error::WrongMessage);
                    assert!(questions.is_live(key));
                    assert!(exports.lookup(grant.id()).is_some());
                    rejected.frame.release(messages);
                    let response = encode(messages, |builder| {
                        let mut root = builder.try_init_root::<Message>().unwrap();
                        let mut result = root.field(Message::RETURN).init().unwrap();
                        result.scalar(Return::ANSWER_ID).set(id.to_wire());
                        // releaseParamCaps defaults to true.
                        let mut payload = result.field(Return::RESULTS).init().unwrap();
                        payload
                            .field(Payload::CONTENT)
                            .set_capability_index(0)
                            .unwrap();
                        payload
                            .field(Payload::CAP_TABLE)
                            .init(1)
                            .unwrap()
                            .get_mut(0)
                            .unwrap()
                            .scalar(CapDescriptor::RECEIVER_HOSTED)
                            .set(grant.id().to_wire());
                    });
                    let response = rejected
                        .question
                        .returned::<_, _, _, 1>(
                            response,
                            messages,
                            questions,
                            imports,
                            exports,
                            answers,
                            &mut [0],
                            |_| (),
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error));
                    assert!(exports.lookup(grant.id()).is_none());
                    assert_eq!(exports.take_retired(0), None);
                    response
                        .with(messages, |reply| {
                            let Reply::Results(payload) = reply else {
                                panic!("results")
                            };
                            let pointer = payload
                                .content()
                                .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                                .unwrap();
                            let reception::full::Capability::Local(local) =
                                payload.capability(pointer).unwrap()
                            else {
                                panic!("local")
                            };
                            assert_eq!(*exports.get(local), 41);
                        })
                        .unwrap();
                    // The original request is reclaimed before the response owner.
                    let free = messages.reserve(49 * 8).unwrap();
                    messages.discard(free);
                    response.release(messages, questions, imports, exports, answers);
                    assert_eq!(exports.take_retired(0), Some(41));
                    assert_eq!(questions.reserve().err(), Some(questions::Error::Full));
                    let pending = questions.pending_finish().unwrap();
                    assert_eq!(pending.id(), id);
                    pending.accept();
                    assert!(questions.is_empty());
                    assert!(imports.is_empty() && exports.is_empty());
                },
            )
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn canceled_late_results_need_no_receipt_capacity_and_keep_original_until_return() {
    let mut a = [ZERO; 49];
    let mut b = [ZERO; 49];
    let mut buffers = [
        messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
    ];
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
        let mut qq = [questions::Slot::EMPTY];
        connection::with_connection(
            connection::Storage {
                imports: &mut [],
                exports: &mut [],
                questions: &mut qq,
                answers: &mut [],
            },
            |connection: &mut connection::Connection<'_, '_, (), (), ()>| {
                let connection::Connection {
                    questions,
                    imports,
                    exports,
                    answers,
                } = connection;
                let reservation = questions.reserve().unwrap();
                let id = reservation.id();
                let frame = encode(messages, |b| calls::bootstrap(b, id).unwrap());
                let mut question =
                    Question::send(reservation, messages, frame, queue.reserve().unwrap()).unwrap();
                flush(messages, &mut queue);
                question.cancel(questions);
                questions.pending_finish().unwrap().accept();
                assert_eq!(questions.reserve().err(), Some(questions::Error::Full));
                let frame = encode(messages, |builder| {
                    let mut root = builder.try_init_root::<Message>().unwrap();
                    let mut result = root.field(Message::RETURN).init().unwrap();
                    result.scalar(Return::ANSWER_ID).set(id.to_wire());
                    let mut payload = result.field(Return::RESULTS).init().unwrap();
                    payload
                        .field(Payload::CONTENT)
                        .set_capability_index(0)
                        .unwrap();
                    payload
                        .field(Payload::CAP_TABLE)
                        .init(1)
                        .unwrap()
                        .get_mut(0)
                        .unwrap()
                        .scalar(CapDescriptor::SENDER_HOSTED)
                        .set(73);
                });
                assert_eq!(messages.reserve(1).err(), Some(messages::Error::NoBuffer));
                let response = question
                    .returned::<_, _, _, 0>(
                        frame,
                        messages,
                        questions,
                        imports,
                        exports,
                        answers,
                        &mut [],
                        |_| panic!("canceled results must not import"),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                response
                    .with(messages, |r| assert!(matches!(r, Reply::Discarded)))
                    .unwrap();
                assert!(questions.is_empty() && imports.is_empty());
                response.release(messages, questions, imports, exports, answers);
                let first = messages.reserve(1).unwrap();
                let second = messages.reserve(1).unwrap();
                messages.discard(first);
                messages.discard(second);
            },
        )
        .unwrap();
    })
    .unwrap();
}

#[test]
fn rejected_send_releases_both_original_and_offered_frame() {
    let mut words = [ZERO; 49];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        let mut slots = [questions::Slot::EMPTY];
        questions::with_questions(&mut slots, |questions| {
            let mut queue_slots = [None];
            let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
            let reservation = questions.reserve().unwrap();
            let frame = encode(messages, |b| calls::bootstrap(b, reservation.id()).unwrap());
            assert!(matches!(
                Question::send(reservation, messages, frame, queue.reserve().unwrap()),
                Err(crate::admission::Error::Storage(
                    messages::Error::NoSendSlot
                ))
            ));
            assert!(questions.is_empty() && queue.is_empty());
            let writer = messages.reserve(1).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    })
    .unwrap();
}

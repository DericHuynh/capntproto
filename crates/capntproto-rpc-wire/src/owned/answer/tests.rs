use super::*;
use capnp::{word, Word};
use capntproto_rpc_engine::ids::QuestionId;
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);

#[test]
fn no_finish_reply_commits_only_with_fifo_capacity_and_rejects_non_data_results() {
    use crate::rpc_capnp::api::{Message, Payload, Return};
    for invalid in 0..4 {
        let measured = allocation_counter::measure(|| {
            let mut a = [ZERO; 64];
            let mut b = [ZERO; 64];
            let mut buffers = [
                messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
                messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
            ];
            messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
                answers::with_answers(&mut [answers::Slot::EMPTY], |answers| {
                    let id = AnswerId::from_wire(if invalid == 3 { 1 << 30 } else { 7 });
                    let key = answers
                        .admit(id, State::new(41))
                        .unwrap_or_else(|_| panic!("admit"));
                    let mut slots = [None; 2];
                    let mut queue = messages::Queue::new(&mut slots).unwrap();
                    let busy = Frame::build(messages, 512, Default::default(), |builder| {
                        calls::acknowledge(builder, AnswerId::from_wire(9))
                    })
                    .unwrap();
                    let offer = messages.offer(&busy.lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                    busy.release(messages);
                    let frame = Frame::build(messages, 512, Default::default(), |builder| {
                        if invalid == 2 {
                            calls::exception(builder, id, capnp::ErrorKind::Failed, "failure")?;
                        } else {
                            calls::acknowledge(builder, id)?;
                        }
                        let mut result = builder
                            .try_get_root::<Message>()?
                            .into_field(Message::RETURN)
                            .edit()?;
                        result.scalar(Return::NO_FINISH_NEEDED).set(true);
                        if invalid == 1 {
                            result
                                .into_field(Return::RESULTS)
                                .edit()?
                                .into_field(Payload::CAP_TABLE)
                                .init(1)?;
                        }
                        Ok(())
                    })
                    .unwrap();
                    let rejected =
                        try_send(key, answers, messages, frame, queue.reserve().unwrap())
                            .err()
                            .expect("reject before commit");
                    assert_eq!(answers.lookup(id), Some(key));
                    let window = queue.window(messages).unwrap().unwrap();
                    let length = window.bytes().len();
                    window.advance(length).unwrap();
                    if invalid != 0 {
                        assert_eq!(
                            rejected.error,
                            Error::Output(crate::admission::Error::Unsupported)
                        );
                        rejected.frame.release(messages);
                        return;
                    }
                    assert_eq!(rejected.error, Error::Storage(messages::Error::NoSendSlot));
                    let retired = try_send(
                        key,
                        answers,
                        messages,
                        rejected.frame,
                        queue.reserve().unwrap(),
                    )
                    .unwrap_or_else(|_| panic!("retry"))
                    .expect("no Finish needed");
                    assert_eq!(retired.release(messages), 41);
                    assert!(answers.is_empty());
                    assert!(answers.finish(id, true).retired.is_none());
                    let window = queue.window(messages).unwrap().unwrap();
                    let length = window.bytes().len();
                    window.advance(length).unwrap();
                })
                .unwrap();
            })
            .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn output_backpressure_preserves_completed_reply_for_retry() {
    let measured = allocation_counter::measure(|| {
        let mut a = [ZERO; 64];
        let mut b = [ZERO; 64];
        let mut buffers = [
            messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
            messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        ];
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            answers::with_answers(&mut [answers::Slot::EMPTY], |answers| {
                let id = AnswerId::from_wire(3);
                let key = answers
                    .admit(id, State::new(41))
                    .unwrap_or_else(|_| panic!("answer"));
                let mut slots = [None; 2];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                let busy = Frame::build(messages, 512, Default::default(), |b| {
                    calls::acknowledge(b, AnswerId::from_wire(4))
                })
                .unwrap();
                let offer = messages.offer(&busy.lease).unwrap();
                queue.reserve().unwrap().accept(messages, offer);
                busy.release(messages);
                let frame = Frame::build(messages, 512, Default::default(), |b| {
                    calls::acknowledge(b, id)
                })
                .unwrap();
                let rejected = try_send(key, answers, messages, frame, queue.reserve().unwrap())
                    .err()
                    .unwrap();
                assert_eq!(rejected.error, Error::Storage(messages::Error::NoSendSlot));
                assert_eq!(queue.len(), 1);
                assert!(answers
                    .get(key)
                    .unwrap()
                    .with_result(messages, |_| ())
                    .unwrap()
                    .is_none());
                let window = queue.window(messages).unwrap().unwrap();
                let length = window.bytes().len();
                window.advance(length).unwrap();
                assert!(try_send(
                    key,
                    answers,
                    messages,
                    rejected.frame,
                    queue.reserve().unwrap()
                )
                .unwrap_or_else(|_| panic!("retry"))
                .is_none());
                let window = queue.window(messages).unwrap().unwrap();
                let length = window.bytes().len();
                window.advance(length).unwrap();
                assert_eq!(
                    answers.finish(id, false).retired.unwrap().release(messages),
                    41
                );
                let a = messages.reserve(512).unwrap();
                let b = messages.reserve(512).unwrap();
                messages.discard(a);
                messages.discard(b);
            })
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn backpressured_capability_reply_keeps_grants_and_handles_finish_before_retry() {
    use crate::rpc_capnp::api::{CapDescriptor, Message, Payload, Return};
    for cancel in [false, true] {
        let measured = allocation_counter::measure(|| {
            let mut a = [ZERO; 64];
            let mut b = [ZERO; 64];
            let mut buffers = [
                messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
                messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
            ];
            messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
                answers::with_answers(&mut [answers::Slot::EMPTY], |answers| {
                    exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                        let id = AnswerId::from_wire(3);
                        let key = answers
                            .admit(id, State::new(41))
                            .unwrap_or_else(|_| panic!("answer"));
                        let mut slots = [None; 2];
                        let mut queue = messages::Queue::new(&mut slots).unwrap();
                        let busy = Frame::build(messages, 512, Default::default(), |b| {
                            calls::acknowledge(b, AnswerId::from_wire(4))
                        })
                        .unwrap();
                        let offer = messages.offer(&busy.lease).unwrap();
                        queue.reserve().unwrap().accept(messages, offer);
                        busy.release(messages);
                        let mut batch = exports.batch();
                        let grant = batch.export(42).unwrap();
                        let frame = Frame::build(messages, 512, Default::default(), |b| {
                            let mut root = b.try_init_root::<Message>()?;
                            let mut result = root.field(Message::RETURN).init()?;
                            result.scalar(Return::ANSWER_ID).set(id.to_wire());
                            result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                            let mut payload = result.field(Return::RESULTS).init()?;
                            payload.field(Payload::CONTENT).set_capability_index(0)?;
                            payload
                                .field(Payload::CAP_TABLE)
                                .init(1)?
                                .get_mut(0)
                                .unwrap()
                                .scalar(CapDescriptor::SENDER_HOSTED)
                                .set(grant.id().to_wire());
                            Ok(())
                        })
                        .unwrap();
                        let rejected = try_send_granting(
                            key,
                            answers,
                            batch,
                            messages,
                            frame,
                            &mut [0],
                            queue.reserve().unwrap(),
                        )
                        .err()
                        .unwrap();
                        assert_eq!(rejected.error, Error::Storage(messages::Error::NoSendSlot));
                        assert_eq!(rejected.grants.pending_owner(grant.id()), Some(&42));
                        assert_eq!(queue.len(), 1);
                        let window = queue.window(messages).unwrap().unwrap();
                        let length = window.bytes().len();
                        window.advance(length).unwrap();
                        if cancel {
                            assert!(answers.finish(id, true).cancel);
                        }
                        let (_, frame, grants) = rejected.into_parts();
                        let retried = try_send_granting(
                            key,
                            answers,
                            grants,
                            messages,
                            frame,
                            &mut [0],
                            queue.reserve().unwrap(),
                        );
                        if cancel {
                            let rejected = retried.err().unwrap();
                            assert_eq!(
                                rejected.error,
                                Error::Output(crate::admission::Error::Mismatch)
                            );
                            assert_eq!(rejected.grants.pending_owner(grant.id()), Some(&42));
                            let (_, frame, grants) = rejected.into_parts();
                            frame.release(messages);
                            drop(grants);
                            assert_eq!(exports.take_retired(0), Some(42));
                            assert!(queue.is_empty());
                            let canceled = Frame::build(messages, 512, Default::default(), |b| {
                                calls::cancelled(b, id)
                            })
                            .unwrap();
                            let retired = try_send(
                                key,
                                answers,
                                messages,
                                canceled,
                                queue.reserve().unwrap(),
                            )
                            .unwrap_or_else(|_| panic!("canceled"))
                            .unwrap();
                            assert_eq!(retired.release(messages), 41);
                        } else {
                            assert!(retried.unwrap_or_else(|_| panic!("retry")).is_none());
                            assert_eq!(exports.resolve(grant.key()), Some(&42));
                            assert_eq!(exports.release(grant.id(), 1).unwrap(), Some(42));
                            assert_eq!(
                                answers.finish(id, false).retired.unwrap().release(messages),
                                41
                            );
                        }
                        let window = queue.window(messages).unwrap().unwrap();
                        let length = window.bytes().len();
                        window.advance(length).unwrap();
                        assert!(answers.is_empty());
                        assert!(exports.is_empty());
                        let a = messages.reserve(512).unwrap();
                        let b = messages.reserve(512).unwrap();
                        messages.discard(a);
                        messages.discard(b);
                    })
                    .unwrap();
                })
                .unwrap();
            })
            .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn published_answer_frame_survives_finish_and_wire_id_reuse_through_pipeline_lease() {
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
            let mut slots = [const { answers::Slot::EMPTY }; 2];
            answers::with_answers(&mut slots, |answers| {
                exports::with_exports(&mut [], |exports: &mut exports::Exports<'_, '_, ()>| {
                    let id = AnswerId::from_wire(3);
                    let key = answers
                        .admit(id, State::new(41))
                        .unwrap_or_else(|_| panic!("answer"));
                    let pipeline = answers.pipeline(id).unwrap();
                    let frame = Frame::build(messages, 49 * 8, Default::default(), |b| {
                        calls::acknowledge(b, id)
                    })
                    .unwrap();
                    assert!(
                        send(key, answers, messages, frame, queue.reserve().unwrap())
                            .unwrap()
                            .is_none()
                    );
                    let window = queue.window(messages).unwrap().unwrap();
                    let length = window.bytes().len();
                    window.advance(length).unwrap();
                    let finish = Frame::build(messages, 49 * 8, Default::default(), |b| {
                        calls::finish(b, QuestionId::from_wire(3), true)
                    })
                    .unwrap();
                    assert!(finished(&finish, messages, answers, exports, &mut [])
                        .unwrap()
                        .unwrap()
                        .retired
                        .is_none());
                    finish.release(messages);
                    let next = answers
                        .admit(id, State::new(43))
                        .unwrap_or_else(|_| panic!("reused answer"));
                    assert_ne!(next, key);
                    let retained = answers.pipelined(&pipeline);
                    assert_eq!(retained.owner, 41);
                    retained
                        .with_result(messages, |root| {
                            let rpc_capnp::message::Return(result) = root.which().unwrap() else {
                                panic!("retained Return")
                            };
                            assert_eq!(result.unwrap().get_answer_id(), 3);
                        })
                        .unwrap()
                        .unwrap();
                    assert!(answers
                        .get(next)
                        .unwrap()
                        .with_result(messages, |_| ())
                        .unwrap()
                        .is_none());
                    assert_eq!(
                        answers
                            .release_pipeline(pipeline)
                            .unwrap()
                            .release(messages),
                        41
                    );
                    answers.finish(id, false);
                    assert_eq!(
                        answers
                            .prepare_return(next)
                            .unwrap()
                            .accept()
                            .unwrap()
                            .release(messages),
                        43
                    );
                    assert!(answers.is_empty());
                    let first = messages.reserve(49 * 8).unwrap();
                    let second = messages.reserve(49 * 8).unwrap();
                    messages.discard(first);
                    messages.discard(second);
                })
                .unwrap();
            })
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

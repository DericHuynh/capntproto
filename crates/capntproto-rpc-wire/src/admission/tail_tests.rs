use super::*;
use capnp::{message::FixedBuilder, word, Word};
use capntproto_rpc_engine::ids::AnswerId;
use rpc_capnp::api::{Message, Return};

#[test]
fn tail_publication_binds_both_roles_and_rolls_back_rejected_output() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..8 {
            let mut as_ = [answers::Slot::EMPTY];
            let mut qs = [questions::Slot::EMPTY];
            let mut wire = [word(0, 0, 0, 0, 0, 0, 0, 0); 25];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut wire))];
            let mut sends = [messages::SendSlot::EMPTY];
            let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
            answers::with_answers(&mut as_, |answers| {
                questions::with_questions(&mut qs, |questions| {
                    let question = if variant == 4 {
                        questions.reserve()
                    } else {
                        questions.reserve_redirected()
                    }
                    .unwrap()
                    .accept();
                    let plan = answers
                        .prepare_admission(AnswerId::from_wire(17), None)
                        .unwrap();
                    let (answer, _) = if variant == 5 {
                        plan.redirected()
                    } else {
                        plan
                    }
                    .accept(41);
                    if variant == 6 {
                        answers.finish(answer.id(), false);
                    }
                    messages::with_messages(
                        &mut buffers,
                        &mut sends[..usize::from(variant != 7)],
                        |messages| {
                            let mut builder = FixedBuilder::new(&mut source).unwrap();
                            let mut root = builder.try_init_root::<Message>().unwrap();
                            let mut result = root.field(Message::RETURN).init().unwrap();
                            result.scalar(Return::ANSWER_ID).set(answer.id().to_wire());
                            result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                            result
                                .scalar(Return::TAKE_FROM_OTHER_QUESTION)
                                .set(if variant == 1 {
                                    99
                                } else {
                                    question.key().id().to_wire()
                                });
                            result.scalar(Return::NO_FINISH_NEEDED).set(variant == 3);
                            let writer = messages.reserve(25 * 8).unwrap();
                            let length = builder.size_in_words() * 8 + 8;
                            capnp::serialize::write_message_segments(
                                &mut &mut messages.write(&writer)[..length],
                                &builder.get_segments_for_output(),
                            )
                            .unwrap();
                            let lease = messages.seal(writer, length).unwrap();
                            let pending = tail_answer(
                                answers.prepare_return(answer).unwrap(),
                                &question,
                                questions,
                                messages,
                                lease,
                                Default::default(),
                            );
                            if variant == 0 || variant == 6 {
                                let mut queue_slots = [None];
                                let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                                // Finish does not revoke a live protected
                                // producer's once-only tail publication.
                                assert_eq!(
                                    pending.unwrap().accept(queue.reserve().unwrap()),
                                    (variant == 6).then_some(41)
                                );
                                assert!(answers.prepare_return(answer).is_err());
                                let window = queue.window(messages).unwrap().unwrap();
                                let length = window.bytes().len();
                                window.advance(length).unwrap();
                                assert_eq!(
                                    answers.finish(answer.id(), false).retired,
                                    (variant == 0).then_some(41)
                                );
                            } else {
                                if variant == 2 {
                                    drop(pending.unwrap());
                                } else {
                                    assert!(pending.is_err());
                                    drop(pending);
                                }
                                answers.finish(answer.id(), false);
                                assert_eq!(
                                    answers.prepare_return(answer).unwrap().accept(),
                                    Some(41)
                                );
                            }
                            assert!(questions.is_live(question.key()));
                            let id = question.key().id();
                            questions.release(question);
                            questions.pending_finish().unwrap().accept();
                            questions.returned(id, false).unwrap();
                            assert!(answers.is_empty() && questions.is_empty());
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

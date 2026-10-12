use super::*;
use capnp::{
    message::{FixedBuilder, Reader},
    word,
};
use capntproto_rpc_engine::{ids::QuestionId, questions};
use rpc_capnp::api::{Call, Message, Return};

#[test]
fn tail_return_admission_is_atomic_and_once_even_after_cancellation() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..8 {
            let mut qs = [questions::Slot::EMPTY];
            let mut as_ = [answers::Slot::EMPTY];
            let mut es = [exports::Slot::<i32>::EMPTY];
            let mut is = [imports::Slot::<i32>::EMPTY];
            questions::with_questions(&mut qs, |questions| {
                answers::with_answers(&mut as_, |answers| {
                    exports::with_exports(&mut es, |exports| {
                        imports::with_imports(&mut is, |imports| {
                            let question = if variant == 7 {
                                questions.reserve_redirected()
                            } else {
                                questions.reserve()
                            }
                            .unwrap()
                            .accept();
                            let id = question.key().id();
                            let parent_id = AnswerId::from_wire(17);
                            let admission = answers.prepare_admission(parent_id, None).unwrap();
                            let admission = if variant == 3 {
                                admission
                            } else {
                                admission.redirected()
                            };
                            let (parent, _) = admission.accept(41);
                            if variant == 4 {
                                answers.prepare_adoption(parent_id).unwrap().discard();
                            }
                            let mut original_words = [word(0, 0, 0, 0, 0, 0, 0, 0); 32];
                            let mut original = FixedBuilder::new(&mut original_words).unwrap();
                            let mut root = original.try_init_root::<Message>().unwrap();
                            let mut call = root.field(Message::CALL).init().unwrap();
                            call.scalar(Call::QUESTION_ID).set(id.to_wire());
                            if variant == 7 {
                                call.group(Call::SEND_RESULTS_TO)
                                    .select()
                                    .void(rpc_capnp::api::call::SendResultsTo::YOURSELF)
                                    .set();
                            }
                            let original =
                                Reader::new(original.get_segments_for_output(), Default::default());
                            let mut result_words = [word(0, 0, 0, 0, 0, 0, 0, 0); 16];
                            let mut result = FixedBuilder::new(&mut result_words).unwrap();
                            let mut root = result.try_init_root::<Return>().unwrap();
                            root.scalar(Return::ANSWER_ID).set(if variant == 1 {
                                99
                            } else {
                                id.to_wire()
                            });
                            root.scalar(Return::TAKE_FROM_OTHER_QUESTION)
                                .set(if variant == 2 {
                                    99
                                } else {
                                    parent_id.to_wire()
                                });
                            let result =
                                Reader::new(result.get_segments_for_output(), Default::default());
                            let local = if variant == 6 {
                                questions.release(question);
                                None
                            } else {
                                Some(question)
                            };
                            let mut scratch = [0];
                            let prepared = returned(
                                result.get_root().unwrap(),
                                original.get_root().unwrap(),
                                questions,
                                imports,
                                exports,
                                answers,
                                Slots {
                                    imports: &mut [],
                                    exports: &mut [],
                                    answers: &mut [],
                                },
                                &mut scratch,
                            );
                            if matches!(variant, 0 | 6) {
                                let (_, reply) = prepared.unwrap().accept();
                                assert!(matches!(
                                    answers.prepare_adoption(parent_id),
                                    Err(answers::Error::AlreadyAdopted)
                                ));
                                let adopted = match reply {
                                    Reply::Adopted(adopted) => Some(adopted),
                                    Reply::Discarded => None,
                                    _ => unreachable!(),
                                };
                                let finish = answers.finish(parent_id, true);
                                assert_eq!(finish.cancel, variant == 6);
                                assert_eq!(
                                    answers.prepare_return(parent).unwrap().accept(),
                                    if variant == 6 { Some(41) } else { None }
                                );
                                if let Some(adopted) = adopted {
                                    assert_eq!(answers.adopted(&adopted), &41);
                                    assert_eq!(answers.release_adopted(adopted), Some(41));
                                }
                            } else {
                                if variant == 5 {
                                    drop(prepared.unwrap());
                                } else {
                                    assert!(prepared.is_err());
                                    drop(prepared);
                                }
                                drop(questions.prepare_receive(id, false).unwrap());
                                if !matches!(variant, 3 | 4) {
                                    drop(answers.prepare_adoption(parent_id).unwrap());
                                }
                                answers.finish(parent_id, false);
                                assert_eq!(
                                    answers.prepare_return(parent).unwrap().accept(),
                                    Some(41)
                                );
                            }
                            if let Some(local) = local {
                                questions.release(local);
                            }
                            questions.pending_finish().unwrap().accept();
                            if !matches!(variant, 0 | 6) {
                                questions
                                    .returned(QuestionId::from_wire(id.to_wire()), false)
                                    .unwrap();
                            }
                            assert!(answers.is_empty() && questions.is_empty());
                        })
                        .unwrap();
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

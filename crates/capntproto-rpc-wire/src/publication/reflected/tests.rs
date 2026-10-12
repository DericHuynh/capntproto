use super::*;
use capnp::{message::FixedBuilder, word, Word};
use capntproto_rpc_engine::{ids::ImportId, question};
use rpc_capnp::api::{CapDescriptor, Message, Payload, Return};

#[test]
fn reflected_publication_retains_references_until_ordered_output_acceptance() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..12 {
            let mut export_slots = [const { exports::Slot::EMPTY }; 1];
            let mut import_slots = [const { imports::Slot::EMPTY }; 1];
            let mut question_slots = [const { questions::Slot::EMPTY }; 1];
            let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
            let mut wire = [word(0, 0, 0, 0, 0, 0, 0, 0); 65];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut wire))];
            let mut sends = [messages::SendSlot::EMPTY];
            exports::with_exports(&mut export_slots, |exports| {
                imports::with_imports(&mut import_slots, |imports| {
                    questions::with_questions(&mut question_slots, |questions| {
                        let local = imports.admit(ImportId::from_wire(42), 73).unwrap();
                        let local_key = local.key();
                        let question = questions.reserve().unwrap().accept();
                        let question_id = question.key().id();
                        if variant == 4 {
                            questions.returned(question_id, true).unwrap();
                        }
                        let mut references = [
                            Reference::Hosted(local),
                            Reference::Answer {
                                question,
                                path: Path::new(&[2, 5]),
                            },
                            Reference::None,
                            Reference::None,
                        ];
                        messages::with_messages(
                            &mut buffers,
                            &mut sends[..usize::from(variant != 11)],
                            |messages| {
                                let mut batch = exports.batch();
                                let grant = batch.export(91).unwrap();
                                let mut builder = FixedBuilder::new(&mut source).unwrap();
                                let mut root = builder.try_init_root::<Message>().unwrap();
                                let mut result = root.field(Message::RETURN).init().unwrap();
                                result.scalar(Return::NO_FINISH_NEEDED).set(variant == 6);
                                let mut payload = result.field(Return::RESULTS).init().unwrap();
                                let mut caps = payload.field(Payload::CAP_TABLE).init(3).unwrap();
                                let mut hosted = caps.get_mut(0).unwrap();
                                if variant == 7 {
                                    hosted.void(CapDescriptor::NONE).set();
                                } else {
                                    hosted
                                        .scalar(CapDescriptor::RECEIVER_HOSTED)
                                        .set(if variant == 1 { 43 } else { 42 });
                                }
                                hosted
                                    .scalar(CapDescriptor::ATTACHED_FD)
                                    .set(if variant == 5 { 0 } else { u8::MAX });
                                let mut promised = caps.get_mut(1).unwrap();
                                let mut answer = promised
                                    .field(CapDescriptor::RECEIVER_ANSWER)
                                    .init()
                                    .unwrap();
                                Path::new(if variant == 3 { &[2, 6] } else { &[2, 5] })
                                    .write(answer.reborrow(), question_id)
                                    .unwrap();
                                if variant == 2 {
                                    answer
                                        .scalar(rpc_capnp::api::PromisedAnswer::QUESTION_ID)
                                        .set(99);
                                }
                                caps.get_mut(2)
                                    .unwrap()
                                    .scalar(CapDescriptor::SENDER_HOSTED)
                                    .set(grant.id().to_wire());
                                let writer = messages.reserve(65 * 8).unwrap();
                                let length = builder.size_in_words() * 8 + 8;
                                capnp::serialize::write_message_segments(
                                    &mut &mut messages.write(&writer)[..length],
                                    &builder.get_segments_for_output(),
                                )
                                .unwrap();
                                let lease = messages.seal(writer, length).unwrap();
                                let count = match variant {
                                    8 => 4,
                                    9 => 2,
                                    _ => 3,
                                };
                                let publication = prepare(
                                    batch,
                                    messages,
                                    lease,
                                    &mut [0; 1],
                                    Default::default(),
                                    References {
                                        imports,
                                        questions,
                                        slots: &mut references[..count],
                                    },
                                );
                                if matches!(variant, 0 | 5) {
                                    let mut queue_slots = [None];
                                    let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                                    publication.unwrap().accept_into(queue.reserve().unwrap());
                                    assert!(references
                                        .iter()
                                        .all(|r| matches!(r, Reference::None)));
                                    // Cleanup became eligible only after the capability frame
                                    // occupied the FIFO. No slot can put it ahead of that frame.
                                    assert!(queue.reserve().is_err());
                                    let release = imports.pending_release(0).unwrap();
                                    assert_eq!(release.id().to_wire(), 42);
                                    assert_eq!(release.count(), 1);
                                    assert_eq!(release.commit(), 73);
                                    let finish = questions.pending_finish().unwrap();
                                    assert!(finish.release_result_caps());
                                    finish.accept();
                                    assert_eq!(
                                        questions.returned(question_id, false).unwrap().1,
                                        question::Delivery::Discard
                                    );
                                    let window = queue.window(messages).unwrap().unwrap();
                                    let length = window.bytes().len();
                                    assert!(window.advance(length).unwrap());
                                    assert_eq!(exports.release(grant.id(), 1), Ok(Some(91)));
                                } else {
                                    if variant == 10 {
                                        drop(publication.unwrap());
                                    } else {
                                        assert!(publication.is_err());
                                        drop(publication);
                                    }
                                    assert_eq!(imports.resolve(local_key), Some(&73));
                                    assert!(imports.pending_release(0).is_none());
                                    assert!(questions.pending_finish().is_none());
                                    assert_eq!(exports.take_retired(0), Some(91));
                                    for reference in &mut references {
                                        match core::mem::replace(reference, Reference::None) {
                                            Reference::Hosted(local) => imports.release(local),
                                            Reference::Answer { question, .. } => {
                                                questions.release(question)
                                            }
                                            Reference::None => (),
                                        }
                                    }
                                    assert_eq!(imports.pending_release(0).unwrap().commit(), 73);
                                    if variant != 4 {
                                        questions.pending_finish().unwrap().accept();
                                        questions.returned(question_id, false).unwrap();
                                    }
                                }
                                assert!(imports.is_empty());
                                assert!(exports.is_empty());
                                assert!(questions.is_empty());
                                let writer = messages.reserve(65 * 8).unwrap();
                                messages.discard(writer);
                            },
                        )
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

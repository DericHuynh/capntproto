use super::*;
use crate::pipeline::Path;
use capnp::{message::FixedBuilder, word, Word};
use capntproto_rpc_engine::ids::ImportId;
use rpc_capnp::api::{Call as CallSchema, CapDescriptor, Message, Payload, PromisedAnswer};

#[test]
fn reflected_call_admits_target_params_and_question_as_one_transaction() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..7 {
            let mut export_slots = [const { exports::Slot::EMPTY }; 1];
            let mut import_slots = [const { imports::Slot::EMPTY }; 1];
            let mut question_slots = [const { questions::Slot::EMPTY }; 2];
            let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
            let mut wire = [word(0, 0, 0, 0, 0, 0, 0, 0); 65];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut wire))];
            let mut sends = [messages::SendSlot::EMPTY];
            exports::with_exports(&mut export_slots, |exports| {
                imports::with_imports(&mut import_slots, |imports| {
                    questions::with_questions(&mut question_slots, |questions| {
                        let local = imports.admit(ImportId::from_wire(42), 73).unwrap();
                        let local_key = local.key();
                        let parent = questions.reserve().unwrap().accept();
                        let parent_id = parent.key().id();
                        let mut target = Reference::Answer {
                            question: parent,
                            path: Path::new(&[2, 5]),
                        };
                        let mut params = [Reference::Hosted(local), Reference::None];
                        messages::with_messages(
                            &mut buffers,
                            &mut sends[..usize::from(variant != 6)],
                            |messages| {
                                let reservation = questions.reserve().unwrap();
                                let question_id = reservation.id();
                                let mut batch = exports.batch();
                                let grant = batch.export(91).unwrap();
                                let mut builder = FixedBuilder::new(&mut source).unwrap();
                                let mut root = builder.try_init_root::<Message>().unwrap();
                                let mut call = root.field(Message::CALL).init().unwrap();
                                call.scalar(CallSchema::QUESTION_ID).set(if variant == 1 {
                                    99
                                } else {
                                    question_id.to_wire()
                                });
                                let mut destination =
                                    call.field(CallSchema::TARGET).init().unwrap();
                                let mut answer = destination
                                    .field(rpc_capnp::api::MessageTarget::PROMISED_ANSWER)
                                    .init()
                                    .unwrap();
                                Path::new(if variant == 3 { &[2, 6] } else { &[2, 5] })
                                    .write(answer.reborrow(), parent_id)
                                    .unwrap();
                                if variant == 2 {
                                    answer
                                        .scalar(PromisedAnswer::QUESTION_ID)
                                        .set(question_id.to_wire());
                                }
                                let mut payload = call.field(CallSchema::PARAMS).init().unwrap();
                                let mut caps = payload.field(Payload::CAP_TABLE).init(2).unwrap();
                                caps.get_mut(0)
                                    .unwrap()
                                    .scalar(CapDescriptor::RECEIVER_HOSTED)
                                    .set(42);
                                caps.get_mut(1)
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
                                let count = if variant == 4 { 1 } else { 2 };
                                let publication = question(
                                    reservation,
                                    batch,
                                    messages,
                                    lease,
                                    &mut [0; 1],
                                    Default::default(),
                                    CallReferences {
                                        imports,
                                        target: &mut target,
                                        params: &mut params[..count],
                                    },
                                );
                                if variant == 0 {
                                    let mut queue_slots = [None];
                                    let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                                    let local =
                                        publication.unwrap().accept_into(queue.reserve().unwrap());
                                    assert_eq!(local.key().id(), question_id);
                                    assert!(questions.is_live(local.key()));
                                    assert!(matches!(target, Reference::None));
                                    assert!(params.iter().all(|r| matches!(r, Reference::None)));
                                    assert!(queue.reserve().is_err());
                                    assert_eq!(imports.pending_release(0).unwrap().commit(), 73);
                                    let finish = questions.pending_finish().unwrap();
                                    assert_eq!(finish.id(), parent_id);
                                    finish.accept();
                                    questions.returned(parent_id, false).unwrap();
                                    let window = queue.window(messages).unwrap().unwrap();
                                    let length = window.bytes().len();
                                    window.advance(length).unwrap();
                                    questions.release(local);
                                    questions.pending_finish().unwrap().accept();
                                    questions.returned(question_id, false).unwrap();
                                    assert_eq!(exports.release(grant.id(), 1), Ok(Some(91)));
                                } else {
                                    if variant == 5 {
                                        drop(publication.unwrap());
                                    } else {
                                        assert!(publication.is_err());
                                        drop(publication);
                                    }
                                    assert_eq!(questions.len(), 1);
                                    assert_eq!(imports.resolve(local_key), Some(&73));
                                    assert!(imports.pending_release(0).is_none());
                                    assert!(questions.pending_finish().is_none());
                                    assert_eq!(exports.take_retired(0), Some(91));
                                    release_references(
                                        core::slice::from_mut(&mut target),
                                        imports,
                                        questions,
                                    );
                                    release_references(&mut params, imports, questions);
                                    assert_eq!(imports.pending_release(0).unwrap().commit(), 73);
                                    questions.pending_finish().unwrap().accept();
                                    questions.returned(parent_id, false).unwrap();
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

use super::*;
use capnp::{field_api::FixedStruct, message::FixedBuilder, word, Word};
use capntproto_rpc_engine::ids::{AnswerId, ImportId, QuestionId};
use rpc_capnp::api::{Bootstrap, Call, CapDescriptor, Message, Payload, Return};

fn seal<'pool>(
    pool: &mut messages::Messages<'pool, '_, '_>,
    fill: impl FnOnce(FixedStruct<'_, Message>),
    trim: usize,
    extra: usize,
) -> messages::Lease<'pool> {
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    fill(builder.try_init_root::<Message>().unwrap());
    let length = (builder.size_in_words() + 1) * 8;
    let writer = pool.reserve(length + extra).unwrap();
    capnp::serialize::write_message_segments(
        &mut &mut pool.write(&writer)[..length],
        &builder.get_segments_for_output(),
    )
    .unwrap();
    pool.seal(writer, length - trim + extra).unwrap()
}
fn bootstrap(root: FixedStruct<'_, Message>, id: u32) {
    let mut root = root;
    root.field(Message::BOOTSTRAP)
        .init()
        .unwrap()
        .scalar(Bootstrap::QUESTION_ID)
        .set(id);
}
fn assert_pool_reusable(pool: &mut messages::Messages<'_, '_, '_>) {
    let w = pool.reserve(1).unwrap();
    let l = pool.seal(w, 1).unwrap();
    let offer = pool.offer(&l).unwrap();
    pool.reject(offer);
    pool.release(l);
}

#[test]
fn malformed_questions_rollback_identity_buffer_and_send_reservation() {
    for variant in 0..6 {
        let mut slots = [questions::Slot::EMPTY];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [messages::SendSlot::EMPTY];
        questions::with_questions(&mut slots, |questions| {
            messages::with_messages(&mut buffers, &mut sends, |pool| {
                let reservation = questions.reserve().unwrap();
                let id = reservation.id().to_wire();
                let lease = seal(
                    pool,
                    |mut root| {
                        if variant < 3 {
                            bootstrap(root, id + u32::from(variant == 0));
                        } else {
                            let mut call = root.field(Message::CALL).init().unwrap();
                            call.scalar(Call::QUESTION_ID).set(id);
                            call.scalar(Call::ONLY_PROMISE_PIPELINE).set(variant == 3);
                            let mut payload = call.field(Call::PARAMS).init().unwrap();
                            if variant == 4 {
                                payload
                                    .field(Payload::CAP_TABLE)
                                    .init(1)
                                    .unwrap()
                                    .get_mut(0)
                                    .unwrap()
                                    .scalar(CapDescriptor::SENDER_HOSTED)
                                    .set(7);
                            } else if variant == 5 {
                                payload
                                    .field(Payload::CONTENT)
                                    .set_capability_index(0)
                                    .unwrap();
                            }
                        }
                    },
                    usize::from(variant == 1),
                    usize::from(variant == 2) * 8,
                );
                assert!(question(reservation, pool, lease, Default::default()).is_err());
                assert!(questions.is_empty());
                assert_pool_reusable(pool);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn failed_send_reservation_and_rejected_fifo_leave_question_unpublished() {
    for send_capacity in [0, 1] {
        let mut slots = [questions::Slot::EMPTY];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [messages::SendSlot::EMPTY];
        questions::with_questions(&mut slots, |questions| {
            messages::with_messages(&mut buffers, &mut sends[..send_capacity], |pool| {
                let reservation = questions.reserve().unwrap();
                let id = reservation.id().to_wire();
                let lease = seal(pool, |root| bootstrap(root, id), 0, 0);
                {
                    let prepared = question(reservation, pool, lease, Default::default());
                    if send_capacity == 0 {
                        assert!(matches!(
                            prepared,
                            Err(Error::Storage(messages::Error::NoSendSlot))
                        ));
                    } else {
                        let prepared = prepared.unwrap();
                        let mut queue = messages::Queue::new(&mut []).unwrap();
                        assert!(queue.reserve().is_err());
                        drop(prepared);
                    }
                }
                assert!(questions.is_empty());
                let writer = pool.reserve(1).unwrap();
                pool.discard(writer);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn capability_bearing_question_commits_all_grants_or_none_without_allocation() {
    let measured = allocation_counter::measure(|| {
        for accept in [false, true] {
            let mut question_slots = [questions::Slot::EMPTY];
            let mut export_slots = [exports::Slot::EMPTY];
            let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
            let mut sends = [messages::SendSlot::EMPTY];
            questions::with_questions(&mut question_slots, |questions| {
                exports::with_exports(&mut export_slots, |exports| {
                    messages::with_messages(&mut buffers, &mut sends, |pool| {
                        let mut entries = [None];
                        let mut queue = messages::Queue::new(&mut entries).unwrap();
                        let reservation = questions.reserve().unwrap();
                        let id = reservation.id();
                        let mut batch = exports.batch();
                        let grant = batch.export(42).unwrap();
                        let lease = seal(
                            pool,
                            |mut root| {
                                let mut call = root.field(Message::CALL).init().unwrap();
                                call.scalar(Call::QUESTION_ID).set(id.to_wire());
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
                            },
                            0,
                            0,
                        );
                        let prepared = granting_question(
                            reservation,
                            batch,
                            pool,
                            lease,
                            &mut [0],
                            Default::default(),
                        )
                        .unwrap();
                        if accept {
                            let local = prepared.accept(queue.reserve().unwrap());
                            assert!(questions.is_live(local.key()));
                            assert_eq!(exports.resolve(grant.key()), Some(&42));
                            assert_eq!(queue.len(), 1);
                            let window = queue.window(pool).unwrap().unwrap();
                            let length = window.bytes().len();
                            assert!(window.advance(length).unwrap());
                            questions.release(local);
                            questions.pending_finish().unwrap().accept();
                            questions.returned(id, false).unwrap();
                            assert_eq!(exports.release(grant.id(), 1), Ok(Some(42)));
                        } else {
                            drop(prepared);
                            assert_eq!(exports.resolve(grant.key()), None);
                            assert_eq!(exports.take_retired(0), Some(42));
                            assert!(queue.is_empty());
                        }
                        assert!(questions.is_empty());
                        assert_pool_reusable(pool);
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

#[test]
fn malformed_answer_cannot_consume_return_obligation() {
    for variant in 0..5 {
        let mut slots = [answers::Slot::EMPTY];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [messages::SendSlot::EMPTY];
        answers::with_answers(&mut slots, |answers| {
            let key = answers.admit(AnswerId::from_wire(7), 42).unwrap();
            assert!(answers.dispatch(key, false).unwrap());
            messages::with_messages(&mut buffers, &mut sends, |pool| {
                let lease = seal(
                    pool,
                    |mut root| {
                        let mut result = root.field(Message::RETURN).init().unwrap();
                        result
                            .scalar(Return::ANSWER_ID)
                            .set(if variant == 0 { 8 } else { 7 });
                        result.scalar(Return::NO_FINISH_NEEDED).set(variant == 1);
                        result.scalar(Return::RELEASE_PARAM_CAPS).set(variant == 2);
                        if variant == 1 {
                            // Data-only success may now omit Finish; exceptions
                            // retain their pipeline failure until explicit Finish.
                            result.field(Return::EXCEPTION).init().unwrap();
                        } else if variant == 3 {
                            result.void(Return::CANCELED).set();
                        } else {
                            let mut payload = result.field(Return::RESULTS).init().unwrap();
                            if variant == 4 {
                                payload
                                    .field(Payload::CONTENT)
                                    .set_capability_index(0)
                                    .unwrap();
                            }
                        }
                    },
                    0,
                    0,
                );
                assert!(answer(
                    answers.prepare_return(key).unwrap(),
                    pool,
                    lease,
                    Default::default()
                )
                .is_err());
                assert!(answers.prepare_return(key).is_ok());
                assert_eq!(answers.get(key), Ok(&42));
                assert_pool_reusable(pool);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn rejected_cleanup_preserves_exact_finish_and_release_obligations() {
    let mut questions_storage = [questions::Slot::EMPTY];
    let mut imports_storage = [imports::Slot::EMPTY];
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
    let mut sends = [messages::SendSlot::EMPTY];
    questions::with_questions(&mut questions_storage, |questions| {
        imports::with_imports(&mut imports_storage, |imports| {
            messages::with_messages(&mut buffers, &mut sends, |pool| {
                let local = questions.reserve().unwrap().accept();
                let id = local.key().id();
                questions.release(local);
                for variant in [0, 1] {
                    let lease = seal(
                        pool,
                        |mut root| {
                            let mut f = root.field(Message::FINISH).init().unwrap();
                            f.scalar(rpc_capnp::api::Finish::QUESTION_ID)
                                .set(id.to_wire() + u32::from(variant == 0));
                            f.scalar(rpc_capnp::api::Finish::RELEASE_RESULT_CAPS)
                                .set(variant == 0);
                        },
                        0,
                        0,
                    );
                    assert!(matches!(
                        finish(
                            questions.pending_finish().unwrap(),
                            pool,
                            lease,
                            Default::default()
                        ),
                        Err(Error::Mismatch)
                    ));
                }
                assert_eq!(
                    questions.pending_finish().unwrap().id(),
                    QuestionId::from_wire(0)
                );
                let local = imports.admit(ImportId::from_wire(9), 42).unwrap();
                imports.release(local);
                for count in [0, 2, u32::MAX] {
                    let lease = seal(
                        pool,
                        |mut root| {
                            let mut r = root.field(Message::RELEASE).init().unwrap();
                            r.scalar(rpc_capnp::api::Release::ID).set(9);
                            r.scalar(rpc_capnp::api::Release::REFERENCE_COUNT)
                                .set(count);
                        },
                        0,
                        0,
                    );
                    assert!(matches!(
                        release(
                            imports.pending_release(0).unwrap(),
                            pool,
                            lease,
                            Default::default()
                        ),
                        Err(Error::Mismatch)
                    ));
                    assert_eq!(imports.pending_release(0).unwrap().count(), 1);
                }
                assert_pool_reusable(pool);
            })
            .unwrap();
        })
        .unwrap();
    })
    .unwrap();
}

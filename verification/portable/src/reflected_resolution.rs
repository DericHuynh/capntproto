use super::*;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ExportId, ImportId, WireId},
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    inbound::full,
    pipeline::Path,
    publication::reflected::{self, Reference, References},
    reception::full::{Capability, Slots},
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Resolve},
    },
};

/// A reflected Resolve travels through partial, Pending and Interrupted IO.
/// Both local and promised-answer replacements survive the following cleanup,
/// including when the original observer was already abandoned.
pub fn bounded_reflected_resolution(chunk: usize, promised: bool, abandoned: bool) -> Result<()> {
    let mut se = [exports::Slot::EMPTY];
    let mut si = [imports::Slot::EMPTY];
    let mut sq = [questions::Slot::EMPTY];
    let mut pe = [exports::Slot::EMPTY];
    let mut pi = [imports::Slot::EMPTY];
    let mut pa = [answers::Slot::EMPTY];
    let mut output = [ZERO; 65];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut output))];
    let mut sends = [messages::SendSlot::EMPTY];
    exports::with_exports(&mut se, |sender_exports| {
        imports::with_imports(&mut si, |sender_imports| {
            questions::with_questions(&mut sq, |sender_questions| {
                exports::with_exports(&mut pe, |peer_exports| {
                    imports::with_imports(&mut pi, |peer_imports| {
                        answers::with_answers(&mut pa, |peer_answers| {
                            messages::with_messages(
                                &mut buffers,
                                &mut sends,
                                |messages| -> Result<()> {
                                    let mut queue_slots = [None];
                                    let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                                    let mut batch = sender_exports.batch();
                                    let promise = batch.export_promise(41).unwrap();
                                    batch.commit();
                                    let promise_id = ImportId::from_wire(promise.id().to_wire());
                                    let mut initial =
                                        [imports::ReceiveSlot::new_promise(promise_id, 43)];
                                    peer_imports.prepare_receive(&mut initial).unwrap().commit();
                                    let expected_target = initial[0].local().unwrap().key();
                                    if abandoned {
                                        peer_imports.release(initial[0].take_local().unwrap());
                                        assert_eq!(
                                            peer_imports.pending_release(0).unwrap().commit(),
                                            43
                                        );
                                    }
                                    let mut batch = peer_exports.batch();
                                    let cap = batch.export(47).unwrap();
                                    batch.commit();
                                    let imported = sender_imports
                                        .admit(ImportId::from_wire(cap.id().to_wire()), 53)
                                        .unwrap();
                                    let question = sender_questions.reserve().unwrap().accept();
                                    let question_id = question.key().id();
                                    let parent = peer_answers
                                        .admit(AnswerId::from_wire(question_id.to_wire()), 59)
                                        .unwrap();
                                    let mut refs = [if promised {
                                        sender_imports.release(imported);
                                        Reference::Answer {
                                            question,
                                            path: Path::new(&[2, 5]),
                                        }
                                    } else {
                                        sender_questions.release(question);
                                        Reference::Hosted(imported)
                                    }];
                                    let mut source = [ZERO; 64];
                                    let mut builder = message::FixedBuilder::new(&mut source)?;
                                    let mut root = builder.try_init_root::<Message>()?;
                                    let mut resolution = root.field(Message::RESOLVE).init()?;
                                    resolution
                                        .scalar(Resolve::PROMISE_ID)
                                        .set(promise.id().to_wire());
                                    let mut descriptor = resolution.field(Resolve::CAP).init()?;
                                    if promised {
                                        Path::new(&[2, 5]).write(
                                            descriptor
                                                .field(CapDescriptor::RECEIVER_ANSWER)
                                                .init()?,
                                            question_id,
                                        )?;
                                    } else {
                                        descriptor
                                            .scalar(CapDescriptor::RECEIVER_HOSTED)
                                            .set(cap.id().to_wire());
                                    }
                                    for accepted in [false, true] {
                                        let mut batch = sender_exports.batch();
                                        batch.resolve_promise(promise.key()).unwrap();
                                        let lease = super::rpc::seal(messages, &builder);
                                        let publication = reflected::prepare(
                                            batch,
                                            messages,
                                            lease,
                                            &mut [0],
                                            Default::default(),
                                            References {
                                                imports: sender_imports,
                                                questions: sender_questions,
                                                slots: &mut refs,
                                            },
                                        )
                                        .unwrap();
                                        if accepted {
                                            publication.accept_into(queue.reserve().unwrap());
                                        } else {
                                            drop(publication);
                                        }
                                    }
                                    assert!(matches!(refs[0], Reference::None));
                                    let mut input = [ZERO; 65];
                                    let receiver = super::rpc::receive(
                                        &mut queue, messages, &mut input, chunk,
                                    );
                                    let frame = receiver.message()?.unwrap();
                                    let rpc_capnp::message::Resolve(resolution) = frame
                                        .get_root::<rpc_capnp::message::Reader<'_>>()?
                                        .which()
                                        .unwrap()
                                    else {
                                        unreachable!()
                                    };
                                    let mut ir = [imports::ReceiveSlot::none()];
                                    let mut er = [if promised {
                                        exports::LeaseSlot::none()
                                    } else {
                                        exports::LeaseSlot::new(cap.id())
                                    }];
                                    let mut ar = [if promised {
                                        answers::PipelineSlot::new(parent.id())
                                    } else {
                                        answers::PipelineSlot::none()
                                    }];
                                    let received = full::resolve(
                                        resolution?,
                                        peer_imports,
                                        peer_exports,
                                        peer_answers,
                                        Slots {
                                            imports: &mut ir,
                                            exports: &mut er,
                                            answers: &mut ar,
                                        },
                                    )
                                    .unwrap()
                                    .accept();
                                    assert_eq!(
                                        received.target(),
                                        if abandoned {
                                            None
                                        } else {
                                            Some(expected_target)
                                        }
                                    );
                                    let release = sender_imports.pending_release(0).unwrap();
                                    assert_eq!(
                                        peer_exports.release(
                                            ExportId::from_wire(release.id().to_wire()),
                                            release.count()
                                        ),
                                        Ok(if promised { Some(47) } else { None })
                                    );
                                    assert_eq!(release.commit(), 53);
                                    let finish = sender_questions.pending_finish().unwrap();
                                    peer_answers.finish(
                                        AnswerId::from_wire(finish.id().to_wire()),
                                        finish.release_result_caps(),
                                    );
                                    finish.accept();
                                    assert_eq!(
                                        peer_answers.prepare_return(parent).unwrap().accept(),
                                        if promised { None } else { Some(59) }
                                    );
                                    sender_questions.returned(question_id, false).unwrap();
                                    match received.replacement().unwrap().unwrap() {
                                        Capability::Local(lease) => {
                                            assert!(!promised);
                                            assert_eq!(peer_exports.get(lease), &47);
                                        }
                                        Capability::Promised { answer, path } => {
                                            assert!(promised);
                                            assert_eq!(peer_answers.pipelined(answer), &59);
                                            assert_eq!(
                                                Path::decode(path, &mut [0; 2]).unwrap().fields(),
                                                &[2, 5]
                                            );
                                        }
                                        _ => unreachable!(),
                                    }
                                    received.release(peer_imports, peer_exports, peer_answers);
                                    assert_eq!(
                                        peer_exports.take_retired(0),
                                        if promised { None } else { Some(47) }
                                    );
                                    assert_eq!(
                                        peer_answers.take_finished(0),
                                        if promised { Some(59) } else { None }
                                    );
                                    if let Some(local) = initial[0].take_local() {
                                        peer_imports.release(local);
                                        assert_eq!(
                                            peer_imports.pending_release(0).unwrap().commit(),
                                            43
                                        );
                                    }
                                    assert_eq!(
                                        sender_exports.release(promise.id(), 1),
                                        Ok(Some(41))
                                    );
                                    assert!(
                                        sender_exports.is_empty()
                                            && sender_imports.is_empty()
                                            && sender_questions.is_empty()
                                    );
                                    assert!(
                                        peer_exports.is_empty()
                                            && peer_imports.is_empty()
                                            && peer_answers.is_empty()
                                    );
                                    Ok(())
                                },
                            )
                            .unwrap()
                        })
                        .unwrap()
                    })
                    .unwrap()
                })
                .unwrap()
            })
            .unwrap()
        })
        .unwrap()
    })
    .unwrap()
}

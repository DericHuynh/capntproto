//! Third-party metadata and registration ownership without heap or IO policy.
use super::ZERO;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ImportId, QuestionId, WireId},
    messages, provision,
};
use capntproto_rpc_wire::{
    calls, network,
    owned::{self, answer, provision::Provide, Frame, TargetRef},
    pipeline::Path,
    rpc_capnp,
};

fn build<'p>(
    messages: &mut messages::Messages<'p, '_, '_>,
    promised: bool,
    imported: ImportId,
    invalid: bool,
) -> Frame<'p> {
    Frame::build(messages, 520, Default::default(), |builder| {
        network::provide(
            builder,
            QuestionId::from_wire(5),
            if promised {
                calls::Target::Promised {
                    question: QuestionId::from_wire(3),
                    path: Path::new(&[1, 2]),
                }
            } else {
                calls::Target::Imported(imported)
            },
            |recipient| {
                if invalid {
                    recipient.set_capability_index(0)
                } else {
                    recipient
                        .cast::<capnp::field_api::Data>()
                        .copy_from(b"recipient")
                }
            },
        )
    })
    .unwrap()
}
fn finish<'p>(
    messages: &mut messages::Messages<'p, '_, '_>,
    answers: &mut answers::Answers<'_, '_, answer::State<'p, u8>>,
    exports: &mut exports::Exports<'_, '_, u8>,
) {
    let frame = Frame::build(messages, 520, Default::default(), |builder| {
        calls::finish(builder, QuestionId::from_wire(5), true)
    })
    .unwrap();
    let finished = answer::finished(&frame, messages, answers, exports, &mut [0; 2])
        .unwrap()
        .unwrap();
    assert!(!finished.cancel && !finished.release_result_caps);
    assert_eq!(finished.retired.unwrap().release(messages), 47);
    frame.release(messages);
}

pub fn bounded_provision_lifecycle() {
    for promised in [false, true] {
        for finish_first in [false, true] {
            let mut words = [[ZERO; 65]; 2];
            let mut buffers = words
                .each_mut()
                .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
            messages::with_messages(&mut buffers, &mut [], |messages| {
                let mut exports_storage = [exports::Slot::EMPTY];
                exports::with_exports(&mut exports_storage, |exports| {
                    let mut answer_storage = [const { answers::Slot::EMPTY }; 2];
                    answers::with_answers(&mut answer_storage, |answers| {
                        let mut batch = exports.batch();
                        let grant = batch.export(41u8).unwrap();
                        batch.commit();
                        let parent = answers
                            .admit(AnswerId::from_wire(3), answer::State::new(43u8))
                            .unwrap_or_else(|e| panic!("{}", e.error));
                        let imported = ImportId::from_wire(grant.id().to_wire());
                        let frame = build(messages, promised, imported, true);
                        let rejected = match Provide::<2>::admit(
                            frame,
                            messages,
                            exports,
                            answers,
                            answer::State::new(47),
                        ) {
                            Err(rejected) => rejected,
                            Ok(_) => panic!("capability-bearing network metadata admitted"),
                        };
                        assert_eq!(rejected.owner.release(messages), 47);
                        rejected.frame.release(messages);
                        assert_eq!(answers.len(), 1);
                        let frame = build(messages, promised, imported, false);
                        let provided = Provide::<2>::admit(
                            frame,
                            messages,
                            exports,
                            answers,
                            answer::State::new(47),
                        )
                        .unwrap_or_else(|e| panic!("{}", e.error));
                        assert_eq!(provided.key().id().to_wire(), 5);
                        provided
                            .with_recipient(messages, |metadata| {
                                assert_eq!(
                                    metadata.get_as::<capnp::data::Reader<'_>>().unwrap(),
                                    b"recipient"
                                );
                            })
                            .unwrap();
                        match provided.target() {
                            TargetRef::Hosted(lease) => {
                                assert!(!promised);
                                assert_eq!(*exports.get(lease), 41);
                            }
                            TargetRef::Promised { path, .. } => {
                                assert!(promised);
                                assert_eq!(path.fields(), &[1, 2]);
                            }
                        }
                        let duplicate = build(messages, promised, imported, false);
                        let rejected = match Provide::<2>::admit(
                            duplicate,
                            messages,
                            exports,
                            answers,
                            answer::State::new(53),
                        ) {
                            Err(rejected) => rejected,
                            Ok(_) => panic!("duplicate Provide"),
                        };
                        assert_eq!(rejected.owner.release(messages), 53);
                        rejected.frame.release(messages);
                        assert_eq!(
                            exports.release(grant.id(), 1),
                            Ok(if promised { Some(41) } else { None })
                        );
                        assert_eq!(
                            answers.pipeline(provided.key().id()).err(),
                            Some(answers::Error::NotCallable)
                        );
                        answers.finish(parent.id(), false);
                        let retired_parent = answers.prepare_return(parent).unwrap().accept();
                        assert_eq!(
                            retired_parent.map(|state| state.release(messages)),
                            if promised { None } else { Some(43) }
                        );
                        if finish_first {
                            finish(messages, answers, exports);
                        }
                        provided.release(messages, exports, answers);
                        if !finish_first {
                            finish(messages, answers, exports);
                        }
                        assert_eq!(
                            exports.take_retired(0),
                            if promised { None } else { Some(41) }
                        );
                        assert_eq!(
                            answers
                                .take_finished(0)
                                .map(|state| state.release(messages)),
                            if promised { Some(43) } else { None }
                        );
                        assert!(answers.is_empty() && exports.is_empty());
                        let first = messages.reserve(520).unwrap();
                        let second = messages.reserve(520).unwrap();
                        messages.discard(first);
                        messages.discard(second);
                    })
                    .unwrap();
                })
                .unwrap();
            })
            .unwrap();
        }
    }
    let mut words = [ZERO; 65];
    messages::with_messages(
        &mut [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
            &mut words,
        ))],
        &mut [],
        |messages| {
            for embargo in [None, Some(&b""[..]), Some(&b"abc"[..])] {
                let frame = Frame::build(messages, 520, Default::default(), |builder| {
                    network::accept(builder, QuestionId::from_wire(7), embargo, |completion| {
                        completion
                            .cast::<capnp::field_api::Data>()
                            .copy_from(b"completion")
                    })
                })
                .unwrap();
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Accept(accept) = root.which().unwrap() else {
                            panic!("Accept")
                        };
                        let accept = accept.unwrap();
                        assert_eq!(accept.has_embargo(), embargo.is_some());
                        assert_eq!(accept.get_embargo().unwrap(), embargo.unwrap_or_default());
                        assert_eq!(
                            accept
                                .get_provision()
                                .get_as::<capnp::data::Reader<'_>>()
                                .unwrap(),
                            b"completion"
                        );
                    })
                    .unwrap();
                frame.release(messages);
            }
            let frame = Frame::build(messages, 520, Default::default(), |builder| {
                network::disembargo_accept(
                    builder,
                    calls::Target::Promised {
                        question: QuestionId::from_wire(5),
                        path: Path::new(&[]),
                    },
                    b"abc",
                )
            })
            .unwrap();
            frame
                .with(messages, |root| {
                    let rpc_capnp::message::Disembargo(message) = root.which().unwrap() else {
                        panic!("Disembargo")
                    };
                    let message = message.unwrap();
                    let rpc_capnp::disembargo::context::Accept(id) =
                        message.get_context().which().unwrap()
                    else {
                        panic!("context")
                    };
                    let mut slots = [const { provision::Slot::<3>::EMPTY }; 1];
                    provision::with_embargoes(&mut slots, |embargoes| {
                        embargoes.release(id.unwrap()).unwrap();
                        let claim = embargoes.claim(b"abc").unwrap();
                        embargoes.complete(claim).unwrap();
                        assert_eq!(
                            embargoes.claim(b"abc").unwrap_err(),
                            provision::Error::DuplicateAccept
                        );
                    })
                    .unwrap();
                })
                .unwrap();
            frame.release(messages);
            for id in [1 << 30, 1 << 31, u32::MAX] {
                assert!(matches!(
                    Frame::build(
                        messages,
                        520,
                        Default::default(),
                        |builder| network::accept(
                            builder,
                            QuestionId::from_wire(id),
                            None,
                            |_| panic!("invalid question ID must not run callback")
                        )
                    ),
                    Err(owned::Error::Wire(_))
                ));
            }
            let free = messages.reserve(520).unwrap();
            messages.discard(free);
        },
    )
    .unwrap();
}

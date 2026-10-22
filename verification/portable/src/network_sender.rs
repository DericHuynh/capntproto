//! Two independent domains exchange an announced answer over one-byte IO.
use super::{driver::with_owned_peer_storage, driver_invocation::close, owning::exchange, ZERO};
use capntproto_rpc_engine::{
    ids::{AnswerId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls,
    owned::{
        connection::{Error, NetworkRequest, Received},
        Frame, Reply,
    },
    rpc_capnp::api::{Bootstrap, Call, Message},
};

pub fn owning_adoption_sender(mode: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut output = [None];
        let mut queue = super::driver::message_queue(&mut output).unwrap();
        with_owned_peer_storage::<2, 1, _>(|mut caller| {
            with_owned_peer_storage::<2, 0, _>(|mut callee| {
                // A caller cannot reserve the callee's answer range using Call or Bootstrap.
                for is_call in [false, true] {
                    let frame = Frame::build(messages, 776, Default::default(), |b| {
                        let root = b.try_init_root::<Message>()?;
                        if is_call {
                            root.into_field(Message::CALL)
                                .init()?
                                .scalar(Call::QUESTION_ID)
                                .set(1 << 30);
                        } else {
                            root.into_field(Message::BOOTSTRAP)
                                .init()?
                                .scalar(Bootstrap::QUESTION_ID)
                                .set(1 << 30);
                        }
                        Ok(())
                    })
                    .unwrap();
                    let rejected = callee
                        .receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| panic!("invalid caller ID"),
                            || 41,
                        )
                        .err()
                        .unwrap();
                    assert_eq!(rejected.producer, Some(41));
                    assert_eq!(
                        rejected.error,
                        Error::Input(capntproto_rpc_wire::owned::Error::Admission(
                            capntproto_rpc_wire::inbound::Error::Unsupported
                        ))
                    );
                    rejected.frame.release(messages);
                    assert!(callee.ledgers().answers.is_empty());
                }
                // Validation failure is invisible to the peer and preserves the producer.
                let rejected = callee
                    .send_adopted_answer(
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                        41,
                        |token| token.set_capability_index(0),
                    )
                    .err()
                    .unwrap();
                assert_eq!(rejected.producer, 41);
                assert!(callee.ledgers().answers.is_empty());
                assert!(queue.is_empty());
                let key = callee
                    .send_adopted_answer(
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                        43,
                        |token| {
                            token
                                .cast::<capnp::field_api::Data>()
                                .copy_from(b"matched network")
                        },
                    )
                    .unwrap_or_else(|r| panic!("publication: {}", r.error));
                assert_eq!(key.id(), AnswerId::from_wire(1 << 30));
                let rejected = callee
                    .send_adopted_answer(messages, &mut queue, 776, Default::default(), 47, |_| {
                        panic!("full output runs no metadata builder")
                    })
                    .err()
                    .unwrap();
                assert_eq!(rejected.producer, 47);
                assert_eq!(callee.ledgers().answers.len(), 1);
                // Return cannot overtake the ThirdPartyAnswer notice.
                assert!(queue.reserve().is_err());
                let notice = exchange(messages, &mut queue, 1);
                assert!(matches!(
                    caller
                        .receive(notice, messages, &mut [0; 4], |_| (), || panic!("notice"))
                        .unwrap_or_else(|r| panic!("{}", r.error)),
                    Received::Network
                ));
                let NetworkRequest::Adopt(job) = caller.take_network().unwrap() else {
                    panic!("adoption")
                };
                let question = job.key();
                let mut job = Some(job);
                if mode == 0 {
                    caller
                        .authorize_adoption(job.take().unwrap(), messages, |token| {
                            assert_eq!(
                                token.get_as::<capnp::data::Reader<'_>>().unwrap(),
                                b"matched network"
                            );
                            Ok::<_, ()>(())
                        })
                        .unwrap_or_else(|_| panic!("authorization"));
                } else if mode == 2 {
                    caller.release_network(NetworkRequest::Adopt(job.take().unwrap()), messages);
                    assert!(caller
                        .send_pending_finish(messages, &mut queue, 776, Default::default())
                        .unwrap());
                    let finish = exchange(messages, &mut queue, 1);
                    assert!(matches!(
                        callee
                            .receive(finish, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(None)
                    ));
                }
                let frame = Frame::build(messages, 776, Default::default(), |b| {
                    if mode == 2 {
                        calls::cancelled(b, key.id())
                    } else {
                        calls::exception(
                            b,
                            key.id(),
                            capnp::ErrorKind::Overloaded,
                            "remote application",
                        )
                    }
                })
                .unwrap();
                let retired = callee
                    .try_publish_answer_frame(key, messages, frame, queue.reserve().unwrap())
                    .unwrap_or_else(|r| panic!("{}", r.error));
                assert_eq!(
                    retired.map(|state| callee.release_answer(state, messages)),
                    if mode == 2 { Some(43) } else { None }
                );
                let frame = exchange(messages, &mut queue, 1);
                caller
                    .receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("exception"),
                        || panic!("Return"),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                if mode == 1 {
                    assert_eq!(
                        caller.take_response(question).err(),
                        Some(Error::PendingAuthorization)
                    );
                    caller
                        .authorize_adoption(job.take().unwrap(), messages, |_| Ok::<_, ()>(()))
                        .unwrap_or_else(|_| panic!("authorization"));
                }
                if mode == 2 {
                    assert!(caller.drain_canceled_response(messages));
                } else {
                    let response = caller.take_response(question).unwrap();
                    response
                        .with(messages, |r| {
                            let Reply::Exception(e) = r else {
                                panic!("exception")
                            };
                            assert_eq!(e.get_reason().unwrap(), "remote application");
                        })
                        .unwrap();
                    caller.release_response(response, messages);
                    assert!(caller
                        .send_pending_finish(messages, &mut queue, 776, Default::default())
                        .unwrap());
                    let frame = exchange(messages, &mut queue, 1);
                    assert!(matches!(
                        callee
                            .receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(Some(43))
                    ));
                }
                assert!(caller.ledgers().questions.is_empty());
                assert!(callee.ledgers().answers.is_empty());
                assert_eq!(close(caller, messages, &mut queue), 0);
                assert_eq!(close(callee, messages, &mut queue), 0);
                let free: [_; 8] = core::array::from_fn(|_| messages.reserve(776).unwrap());
                for writer in free {
                    messages.discard(writer);
                }
            })
        });
    })
    .unwrap();
}

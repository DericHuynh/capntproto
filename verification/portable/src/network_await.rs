//! Original question ownership through third-party completion and old-route fences.
use super::{
    driver::with_owned_peer,
    driver_invocation::close,
    owning::exchange,
    portable_capnp::{
        api::echo::{CapabilityResults, EchoResults},
        echo,
    },
    ZERO,
};
use capntproto_rpc_engine::{
    clients::{self, Capabilities},
    ids::{AnswerId, ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{CallOutput, Error, HandoffError, NetworkRequest, Received},
        Frame,
    },
    pipeline,
    rpc_capnp::{
        self,
        api::{Call, Message, Return},
    },
};

/// 0 result, 1 ordered capability, 2 failed authentication retry, 3 canceled
/// before Return, 4 canceled after Return, 5 terminal, 6 wrong method, 7 failure,
/// 8 malformed metadata, 9 unrequested handoff, 10 canceled live pipeline,
/// 11 dropped output receiver, 12 completed then disconnected, 13 completed then canceled,
/// 14 scalar without Finish, 15 capability result contradicting noFinishNeeded.
pub fn owning_network_await(mode: u8) {
    let mut words = [[ZERO; 97]; 10];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer(|mut peer| {
            use capntproto_rpc_wire::owned::connection::{Replies, ReplyProgress, ReplySlot};
            let mut completion = capntproto_rpc_engine::completion::Slot::new();
            let (sender, receiver) = completion.pair().unwrap();
            let mut receiver = Some(receiver);
            let mut reply_slots = [ReplySlot::EMPTY];
            let mut replies = Replies::new(&mut reply_slots).unwrap();
            let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
            let tracked = matches!(mode, 0 | 7 | 11);
            let cap = matches!(mode, 1 | 10 | 15);
            let key = peer
                .send(messages, &mut queue, 776, Default::default(), |b, id| {
                    if cap {
                        calls::call(
                            b,
                            id,
                            ImportId::from_wire(0),
                            echo::Owned::CAPABILITY,
                            |_| Ok(()),
                        )?;
                    } else {
                        calls::call(b, id, ImportId::from_wire(0), echo::Owned::ECHO, |_| Ok(()))?;
                    }
                    b.try_get_root::<Message>()?
                        .field(Message::CALL)
                        .edit()?
                        .scalar(Call::ALLOW_THIRD_PARTY_TAIL_CALL)
                        .set(mode != 9);
                    Ok(())
                })
                .unwrap();
            if tracked {
                replies
                    .track_output(&peer, key, sender)
                    .unwrap_or_else(|_| panic!("track handoff"));
            } else {
                drop(sender);
            }
            exchange(messages, &mut queue, 1).release(messages);
            let observer = matches!(mode, 1 | 10)
                .then(|| peer.pipeline(key, pipeline::Path::new(&[0])).unwrap());
            if mode == 1 {
                let observer = observer.as_ref().unwrap();
                peer.enqueue_call(
                    observer,
                    echo::Owned::ECHO,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
                let sent = peer
                    .send_invocation(messages, &mut queue, 776, Default::default(), &mut [0; 4])
                    .unwrap()
                    .unwrap();
                exchange(messages, &mut queue, 1).release(messages);
                let frame = Frame::build(messages, 776, Default::default(), |b| {
                    calls::exception(
                        b,
                        AnswerId::from_wire(sent.question.id().to_wire()),
                        capnp::ErrorKind::Failed,
                        "older call completed",
                    )
                })
                .unwrap();
                peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Return"))
                    .unwrap_or_else(|r| panic!("{}", r.error));
                let response = peer.take_response(sent.question).unwrap();
                peer.release_response(response, messages);
                assert!(peer
                    .send_pending_finish(messages, &mut queue, 776, Default::default())
                    .unwrap());
                exchange(messages, &mut queue, 1).release(messages);
            }
            if mode == 3 {
                peer.cancel(key).unwrap();
            }
            if matches!(mode, 8 | 9) {
                let frame = announcement(messages, key.id().to_wire(), mode == 8, false);
                let rejected = peer
                    .receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("metadata"),
                        || panic!("Return"),
                    )
                    .err()
                    .unwrap();
                rejected.frame.release(messages);
                assert_eq!(
                    peer.take_response(key).err(),
                    Some(Error::PendingResponse),
                    "rejected handoff preserves original question"
                );
            }
            if mode == 9 {
                let frame = Frame::build(messages, 776, Default::default(), |b| {
                    calls::exception(
                        b,
                        AnswerId::from_wire(key.id().to_wire()),
                        capnp::ErrorKind::Unimplemented,
                        "no handoff",
                    )
                })
                .unwrap();
                peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Return"))
                    .unwrap_or_else(|r| panic!("{}", r.error));
                let output = peer.take_output(key, messages).unwrap().unwrap();
                peer.release_output(output, messages);
            } else {
                let frame = announcement(messages, key.id().to_wire(), false, mode >= 14);
                let received = peer
                    .receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("metadata"),
                        || panic!("Return"),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                if mode == 3 {
                    assert!(matches!(received, Received::Returned(k, _) if k == key));
                    assert!(
                        peer.take_network().is_none(),
                        "canceled call starts no rendezvous"
                    );
                    assert!(peer.drain_canceled_response(messages));
                } else {
                    assert!(matches!(received, Received::Network));
                    let NetworkRequest::Await(mut awaiting) = peer.take_network().unwrap() else {
                        panic!("await")
                    };
                    assert_eq!(awaiting.key(), key);
                    assert_eq!(
                        peer.take_output(key, messages).err(),
                        Some(Error::PendingResponse),
                        "unmatched handoff stays private"
                    );
                    if cap {
                        for i in 0..peer.outgoing_capacity() {
                            peer.poll_outgoing(i, messages).unwrap();
                        }
                    }
                    let authority = if cap {
                        Some(
                            peer.capabilities()
                                .endpoint(binding::Route::local(41), clients::Location::Local)
                                .unwrap(),
                        )
                    } else {
                        None
                    };
                    let mut result = if mode == 7 {
                        Err(capnp::ErrorKind::Overloaded)
                    } else if let Some(authority) = &authority {
                        Ok(peer
                            .local_result(
                                echo::Owned::CAPABILITY,
                                &[authority],
                                messages,
                                776,
                                Default::default(),
                                |mut r| r.field(CapabilityResults::VALUE).set_index(0),
                            )
                            .unwrap())
                    } else {
                        Ok(peer
                            .local_result(
                                echo::Owned::ECHO,
                                &[],
                                messages,
                                776,
                                Default::default(),
                                |mut r| {
                                    r.scalar(EchoResults::VALUE).set(47);
                                    Ok(())
                                },
                            )
                            .unwrap())
                    };
                    if mode == 15 {
                        let rejected = peer
                            .complete_await(awaiting, result, messages, |_| -> Result<(), ()> {
                                panic!("noFinishNeeded cannot authorize capability handoff")
                            })
                            .expect_err("capability handoff requires Finish");
                        assert!(matches!(
                            rejected.error,
                            HandoffError::Connection(Error::Input(_))
                        ));
                        if let Ok(result) = rejected.result {
                            result.release(messages, &mut peer.capabilities());
                        }
                        awaiting = rejected.awaiting;
                        result = Err(capnp::ErrorKind::Failed);
                    }
                    if mode == 6 {
                        let wrong = peer
                            .local_result(
                                echo::Owned::CAPABILITY,
                                &[],
                                messages,
                                776,
                                Default::default(),
                                |_| Ok(()),
                            )
                            .unwrap();
                        let rejected = peer
                            .complete_await(awaiting, Ok(wrong), messages, |_| -> Result<(), ()> {
                                panic!("wrong-method callback")
                            })
                            .err()
                            .unwrap();
                        assert!(matches!(
                            rejected.error,
                            HandoffError::Connection(Error::Input(_))
                        ));
                        if let Ok(result) = rejected.result {
                            result.release(messages, &mut peer.capabilities());
                        }
                        awaiting = rejected.awaiting;
                    }
                    if mode == 2 {
                        let outcome =
                            peer.complete_await(awaiting, result, messages, |_| Err(13u8));
                        assert!(
                            outcome.is_err(),
                            "unauthenticated handoff preserves both owners"
                        );
                        let rejected = outcome.err().unwrap();
                        assert!(matches!(rejected.error, HandoffError::Network(13)));
                        awaiting = rejected.awaiting;
                        result = rejected.result;
                        assert_eq!(
                            peer.take_output(key, messages).err(),
                            Some(Error::PendingResponse)
                        );
                    }
                    if matches!(mode, 4 | 5 | 10) {
                        if mode == 5 {
                            peer.disconnect(capnp::ErrorKind::Disconnected);
                        } else {
                            peer.cancel(key).unwrap();
                        }
                        let rejected = peer
                            .complete_await(awaiting, result, messages, |_| -> Result<(), ()> {
                                panic!("revoked handoff callback")
                            })
                            .err()
                            .unwrap();
                        if let Ok(result) = rejected.result {
                            result.release(messages, &mut peer.capabilities());
                        }
                        peer.release_network(NetworkRequest::Await(rejected.awaiting), messages);
                        if !matches!(mode, 5 | 12) {
                            for i in 0..peer.outgoing_capacity() {
                                peer.poll_outgoing(i, messages).unwrap();
                            }
                            assert!(
                                peer.drain_canceled_response(messages),
                                "canceling rendezvous breaks retained pipelines"
                            );
                        }
                    } else {
                        peer.complete_await(awaiting, result, messages, |metadata| {
                            assert_eq!(
                                metadata.get_as::<capnp::data::Reader<'_>>().unwrap(),
                                b"authenticated rendezvous"
                            );
                            Ok::<_, ()>(())
                        })
                        .unwrap_or_else(|_| panic!("handoff"));
                        if mode == 12 {
                            peer.disconnect(capnp::ErrorKind::Disconnected);
                        } else if mode == 13 {
                            peer.cancel(key).unwrap();
                            assert!(peer.drain_canceled_response(messages));
                        } else {
                            if let Some(observer) = &observer {
                                assert_eq!(
                                    peer.take_output(key, messages).err(),
                                    Some(Error::PendingResponse)
                                );
                                for i in 0..peer.outgoing_capacity() {
                                    let _ = peer.poll_outgoing(i, messages);
                                }
                                assert!(
                                    matches!(
                                        peer.runtime().inspect(observer).unwrap(),
                                        clients::Destination::Waiting(_)
                                    ),
                                    "handoff cannot overtake old route"
                                );
                                for _ in 0..200 {
                                    if !queue.is_empty() {
                                        break;
                                    }
                                    peer.poll_maintenance(
                                        messages,
                                        &mut queue,
                                        776,
                                        Default::default(),
                                        &mut [0; 4],
                                        core::num::NonZeroUsize::new(1).unwrap(),
                                    )
                                    .unwrap();
                                }
                                let frame = exchange(messages, &mut queue, 1);
                                let embargo = frame
                                    .with(messages, |root| {
                                        let rpc_capnp::message::Disembargo(barrier) =
                                            root.which().unwrap()
                                        else {
                                            panic!("Finish must not overtake old-route fence")
                                        };
                                        let rpc_capnp::disembargo::context::SenderLoopback(id) =
                                            barrier.unwrap().get_context().which().unwrap()
                                        else {
                                            panic!("sender fence")
                                        };
                                        id
                                    })
                                    .unwrap();
                                frame.release(messages);
                                let frame = Frame::build(messages, 776, Default::default(), |b| {
                                    calls::receiver_loopback(
                                        b,
                                        embargo,
                                        calls::Target::Imported(ImportId::from_wire(0)),
                                    )
                                })
                                .unwrap();
                                peer.receive(
                                    frame,
                                    messages,
                                    &mut [0; 4],
                                    |_| panic!("echo"),
                                    || panic!("echo"),
                                )
                                .unwrap_or_else(|r| panic!("{}", r.error));
                                assert!(matches!(
                                    peer.runtime().inspect(observer).unwrap(),
                                    clients::Destination::Endpoint {
                                        owner: binding::Route::Capability(
                                            binding::Endpoint::Local(41)
                                        ),
                                        ..
                                    }
                                ));
                            }
                            let output = if tracked {
                                assert_eq!(
                                    replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                                    ReplyProgress::Delivered
                                );
                                if mode == 11 {
                                    drop(receiver.take());
                                    assert_eq!(
                                        replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                                        ReplyProgress::Reclaimed
                                    );
                                    Err(capnp::ErrorKind::Disconnected)
                                } else {
                                    let output =
                                        receiver.as_mut().unwrap().try_take().unwrap().unwrap();
                                    assert_eq!(
                                        replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                                        ReplyProgress::Consumed
                                    );
                                    output
                                }
                            } else {
                                peer.take_output(key, messages).unwrap()
                            };
                            if mode == 11 {
                                assert_eq!(output.err(), Some(capnp::ErrorKind::Disconnected));
                            } else if mode == 15 {
                                assert_eq!(output.err(), Some(capnp::ErrorKind::Failed));
                            } else if mode == 7 {
                                assert_eq!(output.err(), Some(capnp::ErrorKind::Overloaded));
                            } else {
                                let output = output.unwrap();
                                let CallOutput::Local(result) = &output else {
                                    panic!("bridged output")
                                };
                                if !cap {
                                    result
                                        .with(messages, |r| {
                                            assert_eq!(
                                                r.results(echo::Owned::ECHO).unwrap().value(),
                                                47
                                            )
                                        })
                                        .unwrap();
                                }
                                peer.release_output(output, messages);
                            }
                        }
                    }
                    if let Some(authority) = authority {
                        peer.capabilities().release(authority);
                    }
                }
            }
            if let Some(observer) = observer {
                peer.capabilities().release(observer);
            }
            let mut servers = 0;
            while let Some(server) = peer.drain_endpoint() {
                if let Some(s) = server {
                    servers += s;
                }
            }
            if !matches!(mode, 5 | 12) {
                assert_eq!(
                    peer.send_pending_finish(messages, &mut queue, 776, Default::default())
                        .unwrap(),
                    mode < 14
                );
                if mode < 14 {
                    exchange(messages, &mut queue, 1).release(messages);
                }
                assert!(
                    peer.ledgers().questions.is_empty(),
                    "handoff releases original ID exactly once"
                );
            }
            servers += close(peer, messages, &mut queue);
            assert_eq!(servers, if cap { 41 } else { 0 });
            let free: [_; 10] = core::array::from_fn(|_| messages.reserve(776).unwrap());
            for w in free {
                messages.discard(w);
            }
        });
    })
    .unwrap();
}

fn announcement<'p>(
    messages: &mut messages::Messages<'p, '_, '_>,
    id: u32,
    malformed: bool,
    no_finish: bool,
) -> Frame<'p> {
    Frame::build(messages, 776, Default::default(), |b| {
        network::await_from_third_party(b, AnswerId::from_wire(id), |token| {
            if malformed {
                token.set_capability_index(0)
            } else {
                token
                    .cast::<capnp::field_api::Data>()
                    .copy_from(b"authenticated rendezvous")
            }
        })?;
        b.try_get_root::<Message>()?
            .field(Message::RETURN)
            .edit()?
            .scalar(Return::NO_FINISH_NEEDED)
            .set(no_finish);
        Ok(())
    })
    .unwrap()
}

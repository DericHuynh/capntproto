//! The acceptance gate cannot overtake older calls or survive Provide revocation.
use super::{
    driver::with_owned_peer_storage,
    driver_invocation::close,
    portable_capnp::{api::echo::EchoParams, echo},
    ZERO,
};
use capntproto_rpc_engine::{
    clients,
    ids::{ImportId, QuestionId, WireId},
    messages, provision,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{EmbargoError, Error, NetworkRequest, Received, Task},
        Frame,
    },
    pipeline,
};

pub fn owning_network_barrier(mode: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut output = [None];
        let mut queue = super::driver::message_queue(&mut output).unwrap();
        provision::with_embargoes(&mut [provision::Slot::<4>::EMPTY], |embargoes| {
            let claim = embargoes.claim(b"").unwrap();
            with_owned_peer_storage::<4, 1, _>(|mut peer| {
                let service = peer
                    .runtime
                    .endpoint(binding::Route::local(41), clients::Location::Local)
                    .unwrap();
                let mut batch = peer.ledgers.exports.batch();
                let grant = binding::stage(&mut peer.runtime, &mut batch, &service).unwrap();
                batch.commit();
                let provide = Frame::build(messages, 776, Default::default(), |b| {
                    network::provide(
                        b,
                        QuestionId::from_wire(17),
                        calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                        |_| Ok(()),
                    )
                })
                .unwrap();
                peer.receive(provide, messages, &mut [0; 4], |_| (), || 43)
                    .unwrap_or_else(|r| panic!("{}", r.error));
                let NetworkRequest::Provide(provide) = peer.take_network().unwrap() else {
                    panic!("Provide")
                };
                peer.register_provide(provide, messages, |_, _, _| Ok::<_, ()>(()))
                    .unwrap_or_else(|_| panic!("register"));
                // Neither a transformed Provide nor an ordinary target authorizes this gate.
                for target in [
                    calls::Target::Promised {
                        question: QuestionId::from_wire(17),
                        path: pipeline::Path::new(&[0]),
                    },
                    calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                ] {
                    let invalid = Frame::build(messages, 776, Default::default(), |b| {
                        network::disembargo_accept(b, target, b"")
                    })
                    .unwrap();
                    let rejected = peer
                        .receive(invalid, messages, &mut [0; 4], |_| (), || panic!("marker"))
                        .err()
                        .unwrap();
                    assert_eq!(rejected.error, Error::InvalidNetworkTarget);
                    rejected.frame.release(messages);
                }
                let count = if mode == 2 { 3 } else { 2 };
                for value in 0..count {
                    peer.enqueue_call(
                        &service,
                        echo::Owned::ECHO,
                        &[],
                        messages,
                        776,
                        Default::default(),
                        |mut p| {
                            p.scalar(EchoParams::VALUE).set(value);
                            Ok(())
                        },
                    )
                    .unwrap();
                }
                let marker = Frame::build(messages, 776, Default::default(), |b| {
                    network::disembargo_accept(
                        b,
                        calls::Target::Promised {
                            question: QuestionId::from_wire(17),
                            path: pipeline::Path::new(&[]),
                        },
                        b"",
                    )
                })
                .unwrap();
                let (marker, first) = if mode == 2 {
                    let rejected = peer
                        .receive(marker, messages, &mut [0; 4], |_| (), || panic!("marker"))
                        .err()
                        .unwrap();
                    assert_eq!(rejected.error, Error::Busy);
                    let ready = peer.runtime.next_ready().unwrap().unwrap();
                    let running = ready.accept();
                    let Task::Invoke(call) = peer.runtime.complete(running) else {
                        panic!("older call")
                    };
                    call.release(messages, &mut peer.runtime);
                    (rejected.frame, 1)
                } else {
                    (marker, 0)
                };
                assert!(matches!(
                    peer.receive(marker, messages, &mut [0; 4], |_| (), || panic!("marker"))
                        .unwrap_or_else(|r| panic!("{}", r.error)),
                    Received::QueuedNetwork
                ));
                for _ in first..count {
                    assert!(
                        !peer.poll_network().unwrap(),
                        "accept marker cannot overtake an older ready call"
                    );
                    assert!(peer.take_network().is_none());
                    assert_eq!(embargoes.readiness(&claim), Err(provision::Error::Pending));
                    let ready = peer.runtime.next_ready().unwrap().unwrap();
                    let running = ready.accept();
                    let Task::Invoke(call) = peer.runtime.complete(running) else {
                        panic!("older call")
                    };
                    call.release(messages, &mut peer.runtime);
                }
                fn finish<'p>(messages: &mut messages::Messages<'p, '_, '_>) -> Frame<'p> {
                    Frame::build(messages, 776, Default::default(), |b| {
                        calls::finish(b, QuestionId::from_wire(17), false)
                    })
                    .unwrap()
                }
                let ended = matches!(mode, 1 | 3);
                if ended {
                    let frame = finish(messages);
                    assert!(matches!(
                        peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(Some(43))
                    ));
                    embargoes.close();
                }
                if mode == 1 {
                    // The same wire ID now names a different registration.
                    let replacement = Frame::build(messages, 776, Default::default(), |b| {
                        network::provide(
                            b,
                            QuestionId::from_wire(17),
                            calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                            |_| Ok(()),
                        )
                    })
                    .unwrap();
                    peer.receive(replacement, messages, &mut [0; 4], |_| (), || 59)
                        .unwrap_or_else(|r| panic!("{}", r.error));
                    let NetworkRequest::Provide(replacement) = peer.take_network().unwrap() else {
                        panic!("replacement Provide")
                    };
                    peer.register_provide(replacement, messages, |_, _, _| Ok::<_, ()>(()))
                        .unwrap_or_else(|_| panic!("replacement registration"));
                }
                if mode == 3 {
                    // Terminal cleanup owns a still-queued marker's frame.
                    peer.disconnect(capnp::ErrorKind::Disconnected);
                } else {
                    if mode == 4 {
                        let notice = Frame::build(messages, 776, Default::default(), |b| {
                            network::third_party_answer(
                                b,
                                capntproto_rpc_engine::ids::AnswerId::from_wire(1 << 30),
                                |_| Ok(()),
                            )
                        })
                        .unwrap();
                        peer.receive(notice, messages, &mut [0; 4], |_| (), || panic!("notice"))
                            .unwrap_or_else(|r| panic!("{}", r.error));
                        assert!(
                            !peer.poll_network().unwrap(),
                            "occupied network slot retains marker"
                        );
                        let job = peer.take_network().unwrap();
                        peer.release_network(job, messages);
                    }
                    assert!(peer.poll_network().unwrap());
                    let NetworkRequest::Disembargo(mut barrier) = peer.take_network().unwrap()
                    else {
                        panic!("ordered marker")
                    };
                    if mode == 1 {
                        let rejected = peer
                            .release_accept_embargo(barrier, messages, |_, _| -> Result<(), ()> {
                                panic!("revoked callback")
                            })
                            .err()
                            .unwrap();
                        assert!(matches!(
                            rejected.error,
                            EmbargoError::Connection(Error::InvalidNetworkTarget)
                        ));
                        peer.release_network(
                            NetworkRequest::Disembargo(rejected.barrier),
                            messages,
                        );
                    } else {
                        let rejected = peer
                            .release_accept_embargo(barrier, messages, |_, _| Err(13u8))
                            .err()
                            .unwrap();
                        assert!(matches!(rejected.error, EmbargoError::Network(13)));
                        barrier = rejected.barrier;
                        assert_eq!(embargoes.readiness(&claim), Err(provision::Error::Pending));
                        peer.release_accept_embargo(barrier, messages, |id, owner| {
                            assert_eq!(*owner, 43);
                            assert!(id.is_empty());
                            embargoes.release(id)
                        })
                        .unwrap_or_else(|_| panic!("release gate"));
                        assert_eq!(embargoes.readiness(&claim), Ok(()));
                    }
                }
                if !ended {
                    let frame = finish(messages);
                    assert!(matches!(
                        peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(Some(43))
                    ));
                }
                if mode == 1 {
                    let frame = finish(messages);
                    assert!(matches!(
                        peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(Some(59))
                    ));
                }
                peer.runtime.release(service);
                assert_eq!(close(peer, messages, &mut queue), 41);
                let free: [_; 8] = core::array::from_fn(|_| messages.reserve(776).unwrap());
                for w in free {
                    messages.discard(w);
                }
            });
            if matches!(mode, 1 | 3) {
                embargoes.cancel(claim);
            } else {
                embargoes
                    .complete(claim)
                    .unwrap_or_else(|_| panic!("ready claim"));
            }
        })
        .unwrap();
    })
    .unwrap();
}

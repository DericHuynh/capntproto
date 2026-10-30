//! Accepted capabilities share publication, pipeline and cleanup with Calls.
use super::{driver::with_owned_peer, owning::exchange, ZERO};
use capntproto_rpc_engine::clients::Capabilities;
use capntproto_rpc_engine::{
    clients,
    ids::{ExportId, QuestionId, WireId},
    messages, provision,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{NetworkRequest, Received, Termination},
        Frame,
    },
    pipeline::Path,
    rpc_capnp,
};

pub fn owning_network_acceptance(mode: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_owned_peer(|mut peer| {
            let mut embargo_slots = [const { provision::Slot::<0>::EMPTY }; 1];
            provision::with_embargoes(&mut embargo_slots, |embargoes| {
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    network::accept(
                        builder,
                        QuestionId::from_wire(17),
                        if matches!(mode, 1 | 6) {
                            Some(b"")
                        } else {
                            None
                        },
                        |_| Ok(()),
                    )
                })
                .unwrap();
                assert!(matches!(
                    peer.receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("Accept imports"),
                        || 43
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error)),
                    Received::Network
                ));
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    network::accept(builder, QuestionId::from_wire(19), None, |_| Ok(()))
                })
                .unwrap();
                let rejected = peer
                    .receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("busy imports"),
                        || panic!("busy producer must not execute"),
                    )
                    .err()
                    .unwrap();
                assert_eq!(
                    rejected.error,
                    capntproto_rpc_wire::owned::connection::Error::Busy
                );
                rejected.frame.release(messages);
                let NetworkRequest::Accept(accept) = peer.take_network().unwrap() else {
                    panic!("Accept")
                };
                let key = accept.key();
                let pipeline = if mode == 3 {
                    Some(peer.answer_pipeline(key.id()).unwrap())
                } else {
                    None
                };
                if matches!(mode, 2 | 3) {
                    let frame = Frame::build(messages, 776, Default::default(), |builder| {
                        calls::finish(builder, QuestionId::from_wire(17), true)
                    })
                    .unwrap();
                    assert!(matches!(
                        peer.receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| panic!("Finish"),
                            || panic!("Finish")
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished { producer: None, .. } | Received::IgnoredFinish
                    ));
                }
                let mut server_count = 0;
                let retired = if mode == 4 {
                    peer.reject_accept(
                        accept,
                        capnp::ErrorKind::Failed,
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error))
                } else {
                    let authority = peer
                        .capabilities()
                        .endpoint(binding::Route::local(41), clients::Location::Local)
                        .unwrap();
                    let pending = accept
                        .authorize(messages, embargoes, authority)
                        .unwrap_or_else(|_| panic!("authorize"));
                    if mode == 6 {
                        embargoes.close();
                        let rejected = pending.poll(embargoes).err().unwrap();
                        assert_eq!(rejected.error, provision::Error::Closed);
                        let (accept, authority) = rejected.pending.cancel(embargoes);
                        peer.capabilities().release(authority);
                        assert_eq!(peer.drain_endpoint(), Some(Some(41)));
                        server_count = 41;
                        peer.reject_accept(
                            accept,
                            capnp::ErrorKind::Disconnected,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error))
                    } else {
                        let ready = if mode == 1 {
                            let rejected = pending.poll(embargoes).err().unwrap();
                            assert_eq!(rejected.error, provision::Error::Pending);
                            assert!(peer.capabilities().next_ready().unwrap().is_none());
                            embargoes.release(b"").unwrap();
                            rejected
                                .pending
                                .poll(embargoes)
                                .unwrap_or_else(|_| panic!("released"))
                        } else {
                            pending
                                .poll(embargoes)
                                .unwrap_or_else(|_| panic!("no embargo"))
                        };
                        let ready = if mode == 5 {
                            assert!(peer
                                .send(messages, &mut queue, 776, Default::default(), |b, id| {
                                    calls::bootstrap(b, id)
                                })
                                .is_ok());
                            let rejected = peer
                                .complete_accept(
                                    ready,
                                    messages,
                                    &mut queue,
                                    776,
                                    Default::default(),
                                    &mut [0; 4],
                                )
                                .err()
                                .unwrap();
                            assert!(matches!(
                                rejected.error,
                                capntproto_rpc_wire::owned::connection::Error::Input(
                                    capntproto_rpc_wire::owned::Error::Storage(_)
                                )
                            ));
                            exchange(messages, &mut queue, 1).release(messages);
                            rejected.ready
                        } else {
                            ready
                        };
                        peer.complete_accept(
                            ready,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            &mut [0; 4],
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error))
                    }
                };
                assert_eq!(retired, if mode == 2 { Some(43) } else { None });
                let output = exchange(messages, &mut queue, 1);
                let export = output
                    .with(messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("Return")
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), 17);
                        match returned.which().unwrap() {
                            rpc_capnp::return_::Results(payload) => {
                                assert!(!matches!(mode, 2 | 3 | 4 | 6));
                                let payload = payload.unwrap();
                                assert_eq!(
                                    Path::new(&[]).resolve(payload.get_content()).unwrap(),
                                    0
                                );
                                let rpc_capnp::cap_descriptor::SenderHosted(id) =
                                    payload.get_cap_table().unwrap().get(0).which().unwrap()
                                else {
                                    panic!("hosted")
                                };
                                Some(ExportId::from_wire(id))
                            }
                            rpc_capnp::return_::Canceled(()) => {
                                assert!(matches!(mode, 2 | 3));
                                None
                            }
                            rpc_capnp::return_::Exception(exception) => {
                                assert!(matches!(mode, 4 | 6));
                                assert_eq!(
                                    exception.unwrap().get_type().unwrap(),
                                    if mode == 6 {
                                        rpc_capnp::exception::Type::Disconnected
                                    } else {
                                        rpc_capnp::exception::Type::Failed
                                    }
                                );
                                None
                            }
                            _ => panic!("unexpected Return"),
                        }
                    })
                    .unwrap();
                output.release(messages);
                if let Some(pipeline) = pipeline {
                    let (mut graph, ledgers) = peer.split_graph();
                    let target = ledgers
                        .answers
                        .pipelined(&pipeline)
                        .owner
                        .target(Path::new(&[]), messages, &mut graph)
                        .unwrap()
                        .unwrap();
                    assert!(matches!(
                        peer.runtime().inspect(&target).unwrap(),
                        clients::Destination::Endpoint {
                            owner: binding::Route::Capability(binding::Endpoint::Local(41)),
                            ..
                        }
                    ));
                    peer.capabilities().release(target);
                    assert_eq!(
                        {
                            let answer = peer.release_answer_pipeline(pipeline).unwrap();
                            peer.release_answer(answer, messages)
                        },
                        43
                    );
                }
                if !matches!(mode, 2 | 3) {
                    let frame = Frame::build(messages, 776, Default::default(), |builder| {
                        calls::finish(builder, QuestionId::from_wire(17), false)
                    })
                    .unwrap();
                    assert!(matches!(
                        peer.receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| panic!("Finish"),
                            || panic!("Finish")
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished { producer: Some(43), .. }
                    ));
                }
                if let Some(id) = export {
                    let frame = Frame::build(messages, 776, Default::default(), |builder| {
                        calls::release(
                            builder,
                            capntproto_rpc_engine::ids::ImportId::from_wire(id.to_wire()),
                            1,
                        )
                    })
                    .unwrap();
                    assert!(matches!(
                        peer.receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| panic!("Release"),
                            || panic!("Release")
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Released
                    ));
                }
                for index in 0..4 {
                    peer.drain_export(index);
                }
                while let Some(owner) = peer.drain_endpoint() {
                    server_count += owner.unwrap_or(0)
                }
                assert_eq!(
                    server_count,
                    if mode == 4 { 0 } else { 41 },
                    "acceptance authority retires before terminal cleanup"
                );
            })
            .unwrap();
            queue.fail();
            while queue.discard_one(messages).unwrap() {}
            let mut closing = peer.terminate();
            let mut done = false;
            for _ in 0..200 {
                match closing
                    .poll(messages, core::num::NonZeroUsize::new(1).unwrap())
                    .unwrap()
                {
                    Termination::Pending => (),
                    Termination::Done => {
                        done = true;
                        break;
                    }
                    _ => panic!("network owners must be cleaned before termination"),
                }
            }
            assert!(done);
            let free: [_; 8] = core::array::from_fn(|_| messages.reserve(776).unwrap());
            for writer in free {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

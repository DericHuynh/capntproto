//! Registration work is attached to exact driver answer generations.
use super::{driver::with_owned_peer, ZERO};
use capntproto_rpc_engine::clients::Capabilities as _;
use capntproto_rpc_engine::{
    clients,
    ids::{ImportId, QuestionId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{NetworkRequest, Received, RegistrationError, Termination},
        Frame,
    },
};

pub fn owning_network_registration(mode: u8) {
    let mut words = [[ZERO; 97]; 4];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_owned_peer(|mut peer| {
            let service = peer
                .capabilities()
                .endpoint(binding::Route::local(41), clients::Location::Local)
                .unwrap();
            let (mut graph, mut batch) = peer.export_batch();
            let grant = binding::stage(&mut graph, &mut batch, &service).unwrap();
            batch.commit();
            peer.capabilities().release(service);
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                network::provide(
                    builder,
                    QuestionId::from_wire(17),
                    calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                    |recipient| {
                        recipient
                            .cast::<capnp::field_api::Data>()
                            .copy_from(b"verified peer")
                    },
                )
            })
            .unwrap();
            assert!(matches!(
                peer.receive(
                    frame,
                    messages,
                    &mut [0; 4],
                    |_| panic!("Provide imports"),
                    || 43
                )
                .unwrap_or_else(|r| panic!("{}", r.error)),
                Received::Network
            ));
            assert!(peer.release_export(grant.id(), 1).unwrap().is_none());
            assert!(!peer.drain_export(0));
            let mut finish_owner = None;
            if mode != 3 {
                let NetworkRequest::Provide(mut provide) = peer.take_network().unwrap() else {
                    panic!("Provide")
                };
                let key = provide.key();
                if mode == 1 {
                    let finish = Frame::build(messages, 776, Default::default(), |builder| {
                        calls::finish(builder, QuestionId::from_wire(17), true)
                    })
                    .unwrap();
                    let Received::Finished { producer: owner, .. } = peer
                        .receive(
                            finish,
                            messages,
                            &mut [0; 4],
                            |_| panic!("Finish"),
                            || panic!("Finish"),
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error))
                    else {
                        panic!("Finish")
                    };
                    finish_owner = owner;
                    let rejected = peer
                        .register_provide(provide, messages, |_, _, _| -> Result<(), u8> {
                            panic!("stale registration callback")
                        })
                        .err()
                        .unwrap();
                    assert!(matches!(rejected.error, RegistrationError::Answer(_)));
                    peer.release_network(NetworkRequest::Provide(rejected.provide), messages);
                } else {
                    if mode == 2 {
                        let rejected = peer
                            .register_provide(provide, messages, |_, _, _| Err(7u8))
                            .err()
                            .unwrap();
                        assert!(matches!(rejected.error, RegistrationError::Network(7)));
                        provide = rejected.provide;
                        assert!(!peer.drain_export(0));
                    }
                    peer.register_provide(provide, messages, |recipient, _, producer| {
                        assert_eq!(
                            recipient.get_as::<capnp::data::Reader<'_>>().unwrap(),
                            b"verified peer"
                        );
                        assert_eq!(*producer, 43);
                        *producer = 53;
                        Ok::<_, u8>(())
                    })
                    .unwrap_or_else(|_| panic!("register"));
                    assert!(peer.drain_export(0));
                    assert!(
                        peer.drain_endpoint().is_none(),
                        "registration owns authority after export retires"
                    );
                    if mode != 4 {
                        let authority = peer.retain_provided(key).unwrap();
                        let finish = Frame::build(messages, 776, Default::default(), |builder| {
                            calls::finish(builder, QuestionId::from_wire(17), true)
                        })
                        .unwrap();
                        let Received::Finished { producer: owner, .. } = peer
                            .receive(
                                finish,
                                messages,
                                &mut [0; 4],
                                |_| panic!("Finish"),
                                || panic!("Finish"),
                            )
                            .unwrap_or_else(|r| panic!("{}", r.error))
                        else {
                            panic!("Finish")
                        };
                        finish_owner = owner;
                        assert!(
                            peer.retain_provided(key).is_err(),
                            "ended registration rejects completion"
                        );
                        assert!(
                            peer.drain_endpoint().is_none(),
                            "accepted authority survives Provide Finish"
                        );
                        peer.capabilities().release(authority);
                    }
                }
            }
            let mut retired_server = 0;
            if matches!(mode, 0..=2) {
                assert_eq!(finish_owner, Some(if mode == 1 { 43 } else { 53 }));
                while peer.drain_export(0) {}
                assert_eq!(
                    peer.drain_endpoint(),
                    Some(Some(41)),
                    "normal Finish releases the last provided authority before teardown"
                );
                retired_server = 41;
            }
            peer.disconnect(capnp::ErrorKind::Disconnected);
            queue.fail();
            while queue.discard_one(messages).unwrap() {}
            let mut closing = peer.terminate();
            let mut retired_producer = 0;
            let mut done = false;
            for _ in 0..200 {
                match closing
                    .poll(messages, core::num::NonZeroUsize::new(1).unwrap())
                    .unwrap()
                {
                    Termination::Pending => (),
                    Termination::Producer(value) => retired_producer += value,
                    Termination::Server(value) => retired_server += value,
                    Termination::Done => {
                        done = true;
                        break;
                    }
                    _ => panic!("unexpected import"),
                }
            }
            assert!(done);
            assert_eq!(retired_server, 41);
            assert_eq!(
                retired_producer,
                match mode {
                    3 => 43,
                    4 => 53,
                    _ => 0,
                }
            );
            let free: [_; 4] = core::array::from_fn(|_| messages.reserve(776).unwrap());
            for writer in free {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

pub fn owning_provision_sender() {
    let mut words = [[ZERO; 97]; 4];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer(|mut peer| {
            let vine = peer
                .send_provide(
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    |builder, id| {
                        network::provide(
                            builder,
                            id,
                            calls::Target::Imported(ImportId::from_wire(31)),
                            |_| Ok(()),
                        )
                    },
                )
                .unwrap();
            let id = vine.key().id();
            assert!(peer
                .pipeline(vine.key(), capntproto_rpc_wire::pipeline::Path::new(&[]))
                .is_err());
            assert!(peer
                .send_provide(
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    |_, _| panic!("full output must not run builder")
                )
                .is_err());
            peer.release_provision(vine);
            assert!(!peer.ledgers().questions.is_empty());
            assert!(peer
                .send_pending_finish(messages, &mut queue, 776, Default::default())
                .is_err());
            let frame = super::owning::exchange(messages, &mut queue, 1);
            frame
                .with(messages, |root| {
                    let capntproto_rpc_wire::rpc_capnp::message::Provide(provide) =
                        root.which().unwrap()
                    else {
                        panic!("Provide before Finish")
                    };
                    assert_eq!(provide.unwrap().get_question_id(), id.to_wire());
                })
                .unwrap();
            frame.release(messages);
            assert!(peer
                .send_pending_finish(messages, &mut queue, 776, Default::default())
                .unwrap());
            let frame = super::owning::exchange(messages, &mut queue, 1);
            frame
                .with(messages, |root| {
                    let capntproto_rpc_wire::rpc_capnp::message::Finish(finish) =
                        root.which().unwrap()
                    else {
                        panic!("Finish")
                    };
                    let finish = finish.unwrap();
                    assert_eq!(finish.get_question_id(), id.to_wire());
                    assert!(!finish.get_release_result_caps());
                })
                .unwrap();
            frame.release(messages);
            assert!(peer.ledgers().questions.is_empty());
            assert_eq!(
                super::driver_invocation::close(peer, messages, &mut queue),
                0
            );
            let free: [_; 4] = core::array::from_fn(|_| messages.reserve(776).unwrap());
            for writer in free {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

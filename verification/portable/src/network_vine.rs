//! An explicit vine retains its generation and forwards markers in call order.
use super::{
    driver::{with_owned_peer, with_owned_peer_storage},
    driver_invocation::close,
    owning::exchange,
    portable_capnp::{api::echo::EchoParams, echo},
    ZERO,
};
use capntproto_rpc_engine::{
    clients,
    ids::{ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{EmbargoError, Error, NetworkRequest, Task},
        Frame,
    },
    rpc_capnp,
};

pub fn owning_network_vine(mode: u8) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut output = [None];
        let mut queue = super::driver::message_queue(&mut output).unwrap();
        with_owned_peer_storage::<4, 0, _>(|mut proxy| {
            with_owned_peer(|mut upstream| {
                let vine = proxy
                    .runtime
                    .endpoint(binding::Route::Vine(67), clients::Location::Local)
                    .unwrap();
                let service = proxy
                    .runtime
                    .endpoint(binding::Route::local(71), clients::Location::Local)
                    .unwrap();
                let mut batch = proxy.ledgers.exports.batch();
                let grant = binding::stage(&mut proxy.runtime, &mut batch, &vine).unwrap();
                let ordinary = binding::stage(&mut proxy.runtime, &mut batch, &service).unwrap();
                batch.commit();
                let invalid = Frame::build(messages, 776, Default::default(), |b| {
                    network::disembargo_accept(
                        b,
                        calls::Target::Imported(ImportId::from_wire(ordinary.id().to_wire())),
                        b"gate",
                    )
                })
                .unwrap();
                let rejected =
                    proxy.receive(invalid, messages, &mut [0; 4], |_| (), || panic!("control"));
                assert!(
                    rejected.is_err(),
                    "ordinary exports cannot forward acceptance markers"
                );
                let rejected = rejected.err().unwrap();
                assert_eq!(
                    rejected.error,
                    Error::InvalidNetworkTarget,
                    "ordinary exports cannot forward acceptance markers"
                );
                rejected.frame.release(messages);

                // A real, affine provision on a separate connection domain.
                let provision = upstream
                    .send_provide(messages, &mut queue, 776, Default::default(), |b, id| {
                        network::provide(
                            b,
                            id,
                            calls::Target::Imported(ImportId::from_wire(3)),
                            |_| Ok(()),
                        )
                    })
                    .unwrap();
                let provided = exchange(messages, &mut queue, 1);
                provided.release(messages);
                for value in 0..2 {
                    proxy
                        .enqueue_call(
                            &vine,
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
                        calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                        b"gate",
                    )
                })
                .unwrap();
                proxy
                    .receive(marker, messages, &mut [0; 4], |_| (), || panic!("control"))
                    .unwrap_or_else(|r| panic!("{}", r.error));
                for value in 0..2 {
                    assert!(
                        !proxy.poll_network().unwrap(),
                        "vine marker cannot overtake an older call"
                    );
                    let running = proxy.runtime.next_ready().unwrap().unwrap().accept();
                    let Task::Invoke(call) = proxy.runtime.complete(running) else {
                        panic!("older call")
                    };
                    call.release(messages, &mut proxy.runtime);
                    // The network proxy accepts the older call into upstream FIFO
                    // before it allows the dispatcher to expose the next marker.
                    upstream
                        .send(messages, &mut queue, 776, Default::default(), |b, id| {
                            calls::call(
                                b,
                                id,
                                ImportId::from_wire(3),
                                echo::Owned::ECHO,
                                |mut p| {
                                    p.scalar(EchoParams::VALUE).set(value);
                                    Ok(())
                                },
                            )
                        })
                        .unwrap();
                    if value == 0 {
                        exchange(messages, &mut queue, 1).release(messages);
                    }
                }
                if mode == 3 {
                    assert!(proxy
                        .ledgers
                        .exports
                        .release(grant.id(), 1)
                        .unwrap()
                        .is_none());
                    assert!(proxy.ledgers.exports.resolve(grant.key()).is_none());
                    let mut batch = proxy.ledgers.exports.batch();
                    let newer = binding::stage(&mut proxy.runtime, &mut batch, &service).unwrap();
                    assert_ne!(
                        newer.id(),
                        grant.id(),
                        "queued vine pins its exact export slot"
                    );
                    batch.commit();
                }
                if mode == 4 {
                    proxy.disconnect(capnp::ErrorKind::Disconnected);
                } else {
                    assert!(proxy.poll_network().unwrap());
                    let NetworkRequest::ForwardEmbargo(mut barrier) = proxy.take_network().unwrap()
                    else {
                        panic!("vine marker")
                    };
                    assert_eq!(barrier.key(), grant.key());
                    if mode == 5 {
                        proxy.disconnect(capnp::ErrorKind::Disconnected);
                    }
                    if mode == 5 {
                        let rejected = proxy
                            .forward_vine_embargo(
                                barrier,
                                messages,
                                &mut [0; 4],
                                |_, _, _| -> Result<(), u8> { panic!("revoked vine callback") },
                            )
                            .err()
                            .unwrap();
                        assert!(matches!(rejected.error, EmbargoError::Connection(_)));
                        proxy.release_network(
                            NetworkRequest::ForwardEmbargo(rejected.barrier),
                            messages,
                        );
                    } else {
                        if mode == 1 {
                            let rejected = proxy
                                .forward_vine_embargo(barrier, messages, &mut [0; 4], |_, _, _| {
                                    Err(17u8)
                                })
                                .err()
                                .unwrap();
                            assert!(matches!(rejected.error, EmbargoError::Network(17)));
                            barrier = rejected.barrier;
                        }
                        if mode == 2 {
                            let rejected = proxy
                                .forward_vine_embargo(
                                    barrier,
                                    messages,
                                    &mut [0; 3],
                                    |_, _, _| -> Result<(), u8> { panic!("short scratch") },
                                )
                                .err()
                                .unwrap();
                            assert!(matches!(
                                rejected.error,
                                EmbargoError::Connection(Error::Input(
                                    capntproto_rpc_wire::owned::Error::Capacity
                                ))
                            ));
                            barrier = rejected.barrier;
                        }
                        // Last old call still occupies the bounded upstream FIFO.
                        let rejected = proxy
                            .forward_vine_embargo(
                                barrier,
                                messages,
                                &mut [0; 4],
                                |id, owner, messages| {
                                    assert_eq!(*owner, 67);
                                    upstream.send_provision_embargo(
                                        &provision,
                                        id,
                                        messages,
                                        &mut queue,
                                        776,
                                        Default::default(),
                                    )
                                },
                            )
                            .err()
                            .unwrap();
                        assert!(
                            matches!(rejected.error, EmbargoError::Network(_)),
                            "full upstream FIFO retains the marker"
                        );
                        let old = exchange(messages, &mut queue, 1);
                        old.with(messages, |root| {
                            assert!(matches!(root.which(), Ok(rpc_capnp::message::Call(_))))
                        })
                        .unwrap();
                        old.release(messages);
                        proxy
                            .forward_vine_embargo(
                                rejected.barrier,
                                messages,
                                &mut [0; 4],
                                |id, owner, messages| {
                                    assert_eq!(id, b"gate");
                                    assert_eq!(*owner, 67);
                                    upstream.send_provision_embargo(
                                        &provision,
                                        id,
                                        messages,
                                        &mut queue,
                                        776,
                                        Default::default(),
                                    )
                                },
                            )
                            .unwrap_or_else(|_| panic!("forward"));
                        let sent = exchange(messages, &mut queue, 1);
                        sent.with(messages, |root| {
                            let rpc_capnp::message::Disembargo(d) = root.which().unwrap() else {
                                panic!("forwarded marker")
                            };
                            let d = d.unwrap();
                            let rpc_capnp::disembargo::context::Accept(id) =
                                d.get_context().which().unwrap()
                            else {
                                panic!("accept")
                            };
                            assert_eq!(id.unwrap(), b"gate");
                            let rpc_capnp::message_target::PromisedAnswer(target) =
                                d.get_target().unwrap().which().unwrap()
                            else {
                                panic!("provision target")
                            };
                            let target = target.unwrap();
                            assert_eq!(target.get_question_id(), provision.key().id().to_wire());
                            assert!(target.get_transform().unwrap().is_empty());
                        })
                        .unwrap();
                        sent.release(messages);
                    }
                }
                proxy.runtime.release(vine);
                proxy.runtime.release(service);
                upstream.release_provision(provision);
                assert_eq!(
                    close(proxy, messages, &mut queue),
                    138,
                    "terminal cleanup returns the vine owner"
                );
                let mut empty = [];
                let mut other = super::driver::message_queue(&mut empty).unwrap();
                assert_eq!(close(upstream, messages, &mut other), 0);
            });
        });
        let mut recovered = [const { None }; 12];
        for slot in &mut recovered {
            *slot = Some(messages.reserve(776).unwrap());
        }
        for slot in recovered {
            messages.discard(slot.unwrap());
        }
    })
    .unwrap();
}

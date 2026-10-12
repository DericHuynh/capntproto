//! Join metadata is supplied by the network; RPC owns its exact wire lifecycle.
use super::{driver::with_owned_peer, driver_invocation::close, owning::exchange, ZERO};
use capntproto_rpc_engine::{
    clients,
    ids::{ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{NetworkRequest, Received},
        controls, Reply,
    },
};

pub fn owning_network_join(mode: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut output = [None];
        let mut queue = super::driver::message_queue(&mut output).unwrap();
        with_owned_peer(|mut caller| {
            with_owned_peer(|mut server| {
                let service = server
                    .runtime
                    .endpoint(binding::Route::local(41), clients::Location::Local)
                    .unwrap();
                let mut batch = server.ledgers.exports.batch();
                let grant = binding::stage(&mut server.runtime, &mut batch, &service).unwrap();
                batch.commit();
                server.runtime.release(service);
                let question = caller
                    .send(messages, &mut queue, 776, Default::default(), |b, id| {
                        network::join(
                            b,
                            id,
                            calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())),
                            |part| {
                                part.cast::<capnp::field_api::Data>()
                                    .copy_from(b"authenticated part")
                            },
                        )
                    })
                    .unwrap();
                let frame = exchange(messages, &mut queue, 1);
                assert!(matches!(
                    server
                        .receive(frame, messages, &mut [0; 4], |_| (), || 43)
                        .unwrap_or_else(|r| panic!("{}", r.error)),
                    Received::Network
                ));
                let NetworkRequest::Join(mut join) = server.take_network().unwrap() else {
                    panic!("Join")
                };
                let key = join.key();
                join.with_key_part(messages, |part| {
                    assert_eq!(
                        part.get_as::<capnp::data::Reader<'_>>().unwrap(),
                        b"authenticated part"
                    )
                })
                .unwrap();
                assert!(server
                    .ledgers
                    .exports
                    .release(grant.id(), 1)
                    .unwrap()
                    .is_none());
                assert!(
                    server.retain_provided(key).is_err(),
                    "Join is not a Provide registration"
                );
                let pipeline = if mode == 5 {
                    Some(server.ledgers.answers.pipeline(key.id()).unwrap())
                } else {
                    None
                };
                if matches!(mode, 2 | 5) {
                    caller.cancel(question).unwrap();
                    assert!(controls::finish_one(
                        &mut caller.ledgers.questions,
                        messages,
                        &mut queue,
                        776,
                        Default::default()
                    )
                    .unwrap());
                    let frame = exchange(messages, &mut queue, 1);
                    assert!(matches!(
                        server
                            .receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(None)
                    ));
                }
                if mode == 1 {
                    caller
                        .send(
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            calls::bootstrap,
                        )
                        .unwrap();
                    let rejected = server
                        .complete_join(join, messages, &mut queue, 776, Default::default(), |_| {
                            panic!("full output must preserve metadata")
                        })
                        .err()
                        .unwrap();
                    join = rejected.join;
                    exchange(messages, &mut queue, 1).release(messages);
                }
                if mode == 4 {
                    let rejected = server
                        .complete_join(
                            join,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            |field| field.set_capability_index(0),
                        )
                        .err()
                        .unwrap();
                    join = rejected.join;
                    assert!(queue.is_empty());
                    assert_eq!(server.ledgers.answers.len(), 1);
                }
                let retired = if mode == 3 {
                    server
                        .reject_join(
                            join,
                            capnp::ErrorKind::Unimplemented,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error))
                } else {
                    server
                        .complete_join(
                            join,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            |field| {
                                assert!(
                                    !matches!(mode, 2 | 5),
                                    "canceled Join must not construct network result"
                                );
                                field
                                    .cast::<capnp::field_api::Data>()
                                    .copy_from(b"verified result metadata")
                            },
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error))
                };
                assert_eq!(retired, if mode == 2 { Some(43) } else { None });
                assert!(server.drain_export(0));
                let released_before_finish = matches!(mode, 2 | 3 | 5);
                assert_eq!(
                    server.drain_endpoint(),
                    released_before_finish.then_some(Some(41)),
                    "successful Join retains target through Finish"
                );
                let frame = exchange(messages, &mut queue, 1);
                caller
                    .receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("Join cannot grant capabilities"),
                        || panic!("Return"),
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                if matches!(mode, 2 | 5) {
                    assert!(caller.drain_canceled_response(messages));
                } else {
                    let response = caller.take_response(question).unwrap();
                    response
                        .with(messages, |reply| match reply {
                            Reply::Results(payload) => assert_eq!(
                                payload
                                    .content()
                                    .get_as::<capnp::data::Reader<'_>>()
                                    .unwrap(),
                                b"verified result metadata"
                            ),
                            Reply::Exception(e) => {
                                assert_eq!(mode, 3);
                                assert_eq!(
                                    e.get_type().unwrap(),
                                    capntproto_rpc_wire::rpc_capnp::exception::Type::Unimplemented
                                );
                            }
                            _ => panic!("Join response"),
                        })
                        .unwrap();
                    caller.release_response(response, messages);
                    assert!(controls::finish_one(
                        &mut caller.ledgers.questions,
                        messages,
                        &mut queue,
                        776,
                        Default::default()
                    )
                    .unwrap());
                    let frame = exchange(messages, &mut queue, 1);
                    assert!(matches!(
                        server
                            .receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
                            .unwrap_or_else(|r| panic!("{}", r.error)),
                        Received::Finished(Some(43))
                    ));
                }
                if let Some(pipeline) = pipeline {
                    let result = server.ledgers.answers.pipelined(&pipeline).owner.target(
                        capntproto_rpc_wire::pipeline::Path::new(&[]),
                        messages,
                        &mut server.runtime,
                    );
                    assert!(result.is_err());
                    assert_eq!(
                        server
                            .ledgers
                            .answers
                            .release_pipeline(pipeline)
                            .unwrap()
                            .release(messages)
                            .release(messages, &mut server.runtime),
                        43
                    );
                }
                if !released_before_finish {
                    assert_eq!(server.drain_endpoint(), Some(Some(41)));
                }
                assert!(server.ledgers.answers.is_empty());
                assert_eq!(close(caller, messages, &mut queue), 0);
                assert_eq!(close(server, messages, &mut queue), 0);
                let free: [_; 8] = core::array::from_fn(|_| messages.reserve(776).unwrap());
                for w in free {
                    messages.discard(w);
                }
            })
        });
    })
    .unwrap();
}

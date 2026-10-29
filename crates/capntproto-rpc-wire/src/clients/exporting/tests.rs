use super::*;
use capnp::{word, Word};
use capntproto_rpc_engine::clients::{Capabilities, Client, Destination, Graph, Location, Source};

fn owner<'g>(graph: &Graph<'g, '_, Endpoint<'_, u8, u32>, u8>, client: &Client<'g>) -> u32 {
    let Destination::Endpoint {
        owner: Endpoint::Local(value),
        ..
    } = graph.inspect(client).unwrap()
    else {
        panic!("endpoint")
    };
    *value
}

#[test]
fn private_export_identity_reuses_distinct_graph_routes_without_observer_capacity() {
    let counts = allocation_counter::measure(|| {
        clients::with_graph(
            &mut [const { clients::Slot::EMPTY }; 2],
            clients::NotificationStorage {
                listeners: &mut [],
                subscriptions: &mut [],
            },
            |graph: &mut Graph<'_, '_, u32, u8>| {
                let first = graph.endpoint(7, Location::Local).unwrap();
                let alias = graph.endpoint(7, Location::Local).unwrap();
                assert_ne!(first.key(), alias.key());
                exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                    let mut bindings = Bindings::new(&mut []).unwrap();
                    let mut batch = exports.batch();
                    let identify =
                        |owner: &u32| Some(exports::Identity::for_owner(1, *owner as usize));
                    let original = bindings
                        .stage_using(graph, &mut batch, &first, identify)
                        .unwrap();
                    let duplicate = bindings
                        .stage_using(graph, &mut batch, &alias, identify)
                        .unwrap();
                    assert_eq!(original.id(), duplicate.id());
                    batch.commit();
                    graph.release(alias);
                    assert_eq!(graph.take_retired(), Some(7));
                    assert!(exports.release(original.id(), 1).unwrap().is_none());
                    graph.release(exports.release(original.id(), 1).unwrap().unwrap());
                    assert!(exports.is_empty());
                })
                .unwrap();
                graph.release(first);
                assert_eq!(graph.take_retired(), Some(7));
                assert!(graph.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn direct_and_observed_stage_share_known_route_exports_without_allocating() {
    let counts = allocation_counter::measure(|| {
        clients::with_graph(
            &mut [clients::Slot::EMPTY],
            clients::NotificationStorage {
                listeners: &mut [],
                subscriptions: &mut [],
            },
            |graph: &mut Graph<'_, '_, u8, u8>| {
                let source = graph.endpoint(7, Location::Local).unwrap();
                exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                    let mut bindings = Bindings::new(&mut []).unwrap();
                    let mut batch = exports.batch();
                    let first = crate::clients::stage(graph, &mut batch, &source).unwrap();
                    let second = bindings.stage(graph, &mut batch, &source).unwrap();
                    assert_eq!(first.id(), second.id());
                    batch.commit();
                    graph.release(exports.release(first.id(), 2).unwrap().unwrap());
                })
                .unwrap();
                graph.release(source);
                assert_eq!(graph.take_retired(), Some(7));
                assert!(graph.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn repeated_pending_exports_share_one_observer_and_rollback_only_the_new_grants() {
    let counts = allocation_counter::measure(|| {
        clients::with_graph(
            &mut [const { clients::Slot::EMPTY }; 2],
            clients::NotificationStorage {
                listeners: &mut [const { clients::ListenerSlot::EMPTY }; 1],
                subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 2],
            },
            |graph: &mut Graph<'_, '_, Endpoint<'_, u8, ()>, u8>| {
                exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                    let source = graph.promise(&mut None).unwrap();
                    let mut slots = [Slot::EMPTY];
                    let mut bindings = Bindings::new(&mut slots).unwrap();
                    let mut batch = exports.batch();
                    let first = bindings.stage(graph, &mut batch, &source).unwrap();
                    assert!(first.is_promise());
                    for _ in 0..7 {
                        let alias = bindings.stage(graph, &mut batch, &source).unwrap();
                        assert_eq!(alias.id(), first.id());
                        assert!(
                            !alias.is_promise(),
                            "aliases match legacy senderHosted descriptors"
                        );
                    }
                    batch.commit();
                    assert_eq!(bindings.live, 1);
                    for forgotten in [false, true] {
                        let mut batch = exports.batch();
                        let alias = bindings.stage(graph, &mut batch, &source).unwrap();
                        assert_eq!(alias.id(), first.id());
                        assert!(!alias.is_promise());
                        if forgotten {
                            core::mem::forget(batch);
                        } else {
                            drop(batch);
                        }
                        assert!(exports.pop_retired().is_none());
                        assert_eq!(bindings.live, 1);
                    }
                    assert_eq!(
                        exports.release(first.id(), 9).unwrap_err(),
                        exports::Error::OverRelease
                    );
                    graph.release(exports.release(first.id(), 8).unwrap().unwrap());
                    graph.release(source);
                    while bindings.drain_one(graph) {}
                    assert!(bindings.is_empty());
                    assert!(graph.is_empty());
                    assert!(exports.is_empty());
                })
                .unwrap();
            },
        )
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn exported_promise_freezes_once_before_retrying_resolve_output() {
    let measured = allocation_counter::measure(|| {
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            clients::with_graph(
                &mut [const { clients::Slot::EMPTY }; 6],
                capntproto_rpc_engine::clients::NotificationStorage {
                    listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
                    subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
                },
                |graph: &mut Graph<'_, '_, Endpoint<'_, u8, u32>, u8>| {
                    exports::with_exports(&mut [const { exports::Slot::EMPTY }; 2], |exports| {
                        let source = graph.promise(&mut None).unwrap();
                        let mut slots = [const { Slot::EMPTY }; 16];
                        let mut bindings = Bindings::new(&mut slots).unwrap();
                        assert!(bindings.is_empty());
                        let mut batch = exports.batch();
                        let grant = bindings.stage(graph, &mut batch, &source).unwrap();
                        assert!(!bindings.is_empty());
                        assert!(grant.is_promise());
                        assert_ne!(batch.pending_owner(grant.id()).unwrap().key(), source.key());
                        batch.commit();
                        let mut queue_slots = [None];
                        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                        assert_eq!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 2]
                            ),
                            Ok(Progress::Pending)
                        );
                        let original = graph
                            .endpoint(Endpoint::Local(10), Location::Remote(1))
                            .unwrap();
                        let remote = graph.promise(&mut Some(original)).unwrap();
                        let later = graph.retain(&remote).unwrap();
                        graph
                            .prepare_resolution(source.key(), &mut Some(remote), Source::Direct)
                            .unwrap()
                            .accept();
                        let busy = messages.reserve(512).unwrap();
                        assert!(matches!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 2]
                            ),
                            Err(Error::Output(owned::Error::Storage(
                                messages::Error::NoBuffer
                            )))
                        ));
                        assert!(queue.is_empty());
                        // Consume the exact resolution notification, then the
                        // buffer wait sleeps without revisiting this owner.
                        while let Some(index) =
                            bindings.next_ready(graph, exports, messages, &queue)
                        {
                            assert!(matches!(
                                bindings.poll_one(
                                    index,
                                    graph,
                                    exports,
                                    messages,
                                    &mut queue,
                                    512,
                                    Default::default(),
                                    &mut [0; 2]
                                ),
                                Err(Error::Output(owned::Error::Storage(
                                    messages::Error::NoBuffer
                                )))
                            ));
                        }
                        for _ in 0..32 {
                            assert!(!bindings.has_ready(graph, exports, messages, &queue));
                        }
                        // Retry still owns the observer.
                        assert!(!bindings.is_empty());
                        // A nested promise resolves again while output is blocked.
                        // Neither the original export nor its eventual Resolve may
                        // silently switch from advertised route 10 to route 20.
                        let local = graph
                            .endpoint(Endpoint::Local(20), Location::Local)
                            .unwrap();
                        graph
                            .prepare_resolution(later.key(), &mut Some(local), Source::Direct)
                            .unwrap()
                            .accept();
                        assert_eq!(owner(graph, &source), 20);
                        let lease = exports.lease(grant.id()).unwrap();
                        assert_eq!(owner(graph, exports.get(&lease)), 10);
                        assert!(exports.release_local(lease).is_none());
                        messages.discard(busy);
                        assert!(bindings.has_ready(graph, exports, messages, &queue));
                        assert_eq!(
                            bindings.next_ready(graph, exports, messages, &queue),
                            Some(0)
                        );
                        assert_eq!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 2]
                            ),
                            Ok(Progress::Published)
                        );
                        assert!(bindings.is_empty());
                        let window = queue.window(messages).unwrap().unwrap();
                        let length = window.bytes().len();
                        let replacement = {
                            let mut bytes = window.bytes();
                            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                                &mut bytes,
                                Default::default(),
                            )
                            .unwrap();
                            let root = reader
                                .get_root::<crate::rpc_capnp::message::Reader<'_>>()
                                .unwrap();
                            let crate::rpc_capnp::message::Resolve(resolve) = root.which().unwrap()
                            else {
                                panic!("Resolve")
                            };
                            let resolve = resolve.unwrap();
                            assert_eq!(resolve.get_promise_id(), grant.id().to_wire());
                            let crate::rpc_capnp::resolve::Cap(cap) = resolve.which().unwrap()
                            else {
                                panic!("replacement")
                            };
                            let crate::rpc_capnp::cap_descriptor::SenderHosted(id) =
                                cap.unwrap().which().unwrap()
                            else {
                                panic!("hosted route")
                            };
                            ExportId::from_wire(id)
                        };
                        window.advance(length).unwrap();
                        let lease = exports.lease(replacement).unwrap();
                        assert_eq!(owner(graph, exports.get(&lease)), 10);
                        assert!(exports.release_local(lease).is_none());
                        assert_eq!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 2]
                            ),
                            Ok(Progress::Vacant)
                        );
                        graph.release(exports.release(grant.id(), 1).unwrap().unwrap());
                        graph.release(exports.release(replacement, 1).unwrap().unwrap());
                        graph.release(source);
                        graph.release(later);
                        let mut total = 0;
                        while let Some(endpoint) = graph.take_retired() {
                            let Endpoint::Local(value) = endpoint else {
                                panic!("local owner")
                            };
                            total += value;
                        }
                        assert_eq!(total, 30);
                        assert!(graph.is_empty());
                        assert!(exports.is_empty());
                        assert!(!bindings.drain_one(graph));
                    })
                    .unwrap();
                },
            )
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn released_or_aborted_export_does_not_keep_its_producer_alive() {
    for published in [false, true] {
        clients::with_graph(
            &mut [const { clients::Slot::EMPTY }; 2],
            capntproto_rpc_engine::clients::NotificationStorage {
                listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
                subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
            },
            |graph: &mut Graph<'_, '_, Endpoint<'_, u8, ()>, u8>| {
                exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                    let source = graph.promise(&mut None).unwrap();
                    let mut slots = [Slot::EMPTY];
                    let mut bindings = Bindings::new(&mut slots).unwrap();
                    let mut batch = exports.batch();
                    let grant = bindings.stage(graph, &mut batch, &source).unwrap();
                    if published {
                        batch.commit();
                        graph.release(exports.release(grant.id(), 1).unwrap().unwrap());
                    } else {
                        drop(batch);
                        graph.release(exports.take_retired(0).unwrap());
                    }
                    graph.release(source);
                    messages::with_messages(&mut [], &mut [], |messages| {
                        let mut queue = messages::Queue::new(&mut []).unwrap();
                        assert_eq!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut []
                            ),
                            Ok(Progress::Abandoned)
                        );
                    })
                    .unwrap();
                    assert!(graph.is_empty());
                    assert!(exports.is_empty());
                })
                .unwrap();
            },
        )
        .unwrap();
    }
}

#[test]
fn callable_local_promise_publishes_sender_promise_then_failed_owner_exception() {
    let measured = allocation_counter::measure(|| {
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            clients::with_graph(
                &mut [const { clients::Slot::EMPTY }; 4],
                capntproto_rpc_engine::clients::NotificationStorage {
                    listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
                    subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
                },
                |graph: &mut Graph<'_, '_, Endpoint<'_, u8, u32>, u8>| {
                    exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                        let route = graph.endpoint(Endpoint::Local(7), Location::Local).unwrap();
                        let source = graph.promise(&mut Some(route)).unwrap();
                        let mut slots = [Slot::EMPTY];
                        let mut bindings = Bindings::new(&mut slots).unwrap();
                        let mut batch = exports.batch();
                        let grant = bindings.stage(graph, &mut batch, &source).unwrap();
                        assert!(
                            grant.is_promise(),
                            "routable local hook still owes resolution"
                        );
                        batch.commit();
                        let lease = exports.lease(grant.id()).unwrap();
                        let advertised = exports.get(&lease);
                        assert_eq!(
                            owner(graph, advertised),
                            7,
                            "accepted calls can use the exported proxy before resolution"
                        );
                        assert!(
                            !graph.is_settled(advertised).unwrap(),
                            "a callable route does not certify final resolution"
                        );
                        assert!(exports.release_local(lease).is_none());

                        let mut queue_slots = [None];
                        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                        assert_eq!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 2]
                            ),
                            Ok(Progress::Pending)
                        );
                        assert!(queue.is_empty());
                        let failed = graph
                            .endpoint(
                                Endpoint::Failed {
                                    kind: capnp::ErrorKind::Overloaded,
                                    owner: 11,
                                },
                                Location::Broken,
                            )
                            .unwrap();
                        graph
                            .resolve(source.key(), &mut Some(failed), Source::Direct)
                            .unwrap();
                        assert_eq!(
                            bindings.poll_one(
                                0,
                                graph,
                                exports,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 2]
                            ),
                            Ok(Progress::Published)
                        );
                        let window = queue.window(messages).unwrap().unwrap();
                        let length = window.bytes().len();
                        {
                            let mut bytes = window.bytes();
                            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                                &mut bytes,
                                Default::default(),
                            )
                            .unwrap();
                            let root = reader
                                .get_root::<crate::rpc_capnp::message::Reader<'_>>()
                                .unwrap();
                            let crate::rpc_capnp::message::Resolve(resolve) = root.which().unwrap()
                            else {
                                panic!("Resolve")
                            };
                            let resolve = resolve.unwrap();
                            assert_eq!(resolve.get_promise_id(), grant.id().to_wire());
                            let crate::rpc_capnp::resolve::Exception(error) =
                                resolve.which().unwrap()
                            else {
                                panic!("Exception")
                            };
                            assert_eq!(
                                error.unwrap().get_type().unwrap(),
                                crate::rpc_capnp::exception::Type::Overloaded
                            );
                        }
                        window.advance(length).unwrap();
                        graph.release(exports.release(grant.id(), 1).unwrap().unwrap());
                        graph.release(source);
                        assert!(matches!(graph.take_retired(), Some(Endpoint::Local(7))));
                        assert!(matches!(
                            graph.take_retired(),
                            Some(Endpoint::Failed { owner: 11, .. })
                        ));
                        assert!(graph.is_empty());
                        assert!(exports.is_empty());
                        assert!(!bindings.drain_one(graph));
                    })
                    .unwrap();
                },
            )
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

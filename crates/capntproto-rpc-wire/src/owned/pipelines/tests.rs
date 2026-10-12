use super::*;
use crate::rpc_capnp::api::{CapDescriptor, Message, Payload, Return};
use capnp::{word, Word};
use capntproto_rpc_engine::clients::{dispatch, Destination, Graph};
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);

fn result<'p>(
    pool: &mut messages::Messages<'p, '_, '_>,
    id: AnswerId,
    export: ExportId,
) -> Frame<'p> {
    Frame::build(pool, 512, Default::default(), |builder| {
        let mut root = builder.try_init_root::<Message>()?;
        let mut result = root.field(Message::RETURN).init()?;
        result.scalar(Return::ANSWER_ID).set(id.to_wire());
        let mut payload = result.field(Return::RESULTS).init()?;
        payload.field(Payload::CONTENT).set_capability_index(0)?;
        payload
            .field(Payload::CAP_TABLE)
            .init(1)?
            .get_mut(0)
            .unwrap()
            .scalar(CapDescriptor::SENDER_HOSTED)
            .set(export.to_wire());
        Ok(())
    })
    .unwrap()
}
fn value<'g>(graph: &mut Graph<'g, '_, u32, u8>, client: &Client<'g>) -> u32 {
    let call = graph.prepare_call(client).unwrap();
    let Destination::Endpoint { owner, .. } = call.destination() else {
        panic!("waiting")
    };
    let value = *owner;
    let retained = call.accept();
    graph.release(retained);
    value
}

#[test]
fn published_pipeline_keeps_advertised_route_after_remote_promise_resolves() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 64];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [], |pool| {
            clients::with_graph(
                &mut [const { clients::Slot::EMPTY }; 4],
                |graph: &mut Graph<'_, '_, u32, u8>| {
                    exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                        let remote = graph.endpoint(10, Location::Remote(1)).unwrap();
                        let promise = graph.promise(&mut Some(remote)).unwrap();
                        let mut batch = exports.batch();
                        let grant = crate::clients::stage(graph, &mut batch, &promise).unwrap();
                        let frame = result(pool, AnswerId::from_wire(4), grant.id());
                        let published =
                            Published::<1>::from_exports(&frame, pool, &batch, graph, &mut [0])
                                .unwrap();
                        batch.commit();
                        frame.release(pool);
                        let local = graph.endpoint(20, Location::Local).unwrap();
                        graph
                            .prepare_resolution(promise.key(), &mut Some(local), Source::Direct)
                            .unwrap()
                            .accept();
                        assert_eq!(value(graph, &promise), 20);
                        let export_lease = exports.lease(grant.id()).unwrap();
                        let peer_target = graph.retain(exports.get(&export_lease)).unwrap();
                        assert_eq!(value(graph, &peer_target), 10);
                        graph.release(peer_target);
                        assert!(exports.release_local(export_lease).is_none());
                        let target = published
                            .target(pipeline::Path::new(&[]), pool, graph)
                            .unwrap();
                        assert_eq!(value(graph, &target), 10);
                        graph.release(target);
                        graph.release(promise);
                        graph.release(exports.release(grant.id(), 1).unwrap().unwrap());
                        answers::with_answers(
                            &mut [const { answers::Slot::EMPTY }; 2],
                            |answers| {
                                let wrong = answers
                                    .admit(
                                        AnswerId::from_wire(5),
                                        super::super::answer::State::new(Results::<_, 1>::new(())),
                                    )
                                    .unwrap_or_else(|_| panic!("wrong"));
                                let rejected =
                                    complete(answers, wrong, Ok(published)).err().unwrap();
                                assert_eq!(rejected.error, Error::Mismatch);
                                let correct = answers
                                    .admit(
                                        AnswerId::from_wire(4),
                                        super::super::answer::State::new(Results::<_, 1>::new(())),
                                    )
                                    .unwrap_or_else(|_| panic!("correct"));
                                assert!(complete(answers, correct, rejected.value).is_ok());
                                assert_eq!(
                                    complete(answers, correct, Err(capnp::ErrorKind::Failed))
                                        .err()
                                        .unwrap()
                                        .error,
                                    Error::AlreadyCompleted
                                );
                                for key in [wrong, correct] {
                                    answers.finish(key.id(), false);
                                    answers
                                        .prepare_return(key)
                                        .unwrap()
                                        .accept()
                                        .unwrap()
                                        .release(pool)
                                        .release(pool, graph);
                                }
                            },
                        )
                        .unwrap();
                        while graph.take_retired().is_some() {}
                        assert!(graph.is_empty());
                        let writer = pool.reserve(512).unwrap();
                        pool.discard(writer);
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
fn finished_parent_and_reused_wire_id_still_resolve_the_admitted_child() {
    for invalid_path in [false, true] {
        let measured = allocation_counter::measure(|| {
            let mut words = [ZERO; 64];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
            messages::with_messages(&mut buffers, &mut [], |pool| {
                clients::with_graph(
                    &mut [const { clients::Slot::EMPTY }; 4],
                    |graph: &mut Graph<'_, '_, u32, u8>| {
                        exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                            answers::with_answers(
                                &mut [const { answers::Slot::EMPTY }; 2],
                                |answers| {
                                    let id = AnswerId::from_wire(7);
                                    let parent = answers
                                        .admit(
                                            id,
                                            super::super::answer::State::new(Results::<_, 1>::new(
                                                41,
                                            )),
                                        )
                                        .unwrap_or_else(|_| panic!("parent"));
                                    let admitted = answers.pipeline(id).unwrap();
                                    let mut slots = [Slot::<1>::EMPTY];
                                    let mut bindings = Bindings::new(&mut slots).unwrap();
                                    let mut tasks = [dispatch::TaskSlot::EMPTY];
                                    let mut runtime =
                                        dispatch::borrow_runtime(graph, &mut tasks).unwrap();
                                    let child = bindings
                                        .bind(
                                            &mut runtime,
                                            answers,
                                            &admitted,
                                            pipeline::Path::new(if invalid_path {
                                                &[0]
                                            } else {
                                                &[]
                                            }),
                                        )
                                        .unwrap();
                                    answers.release_pipeline_deferred(admitted);
                                    runtime
                                        .prepare_call(&child, &mut Some(99))
                                        .unwrap_or_else(|_| panic!("child"))
                                        .accept();
                                    runtime.release(child);
                                    assert!(!answers.finish(id, true).cancel);
                                    assert!(answers.dispatch(parent, true).unwrap());
                                    assert!(matches!(
                                        bindings
                                            .poll_one(0, &mut runtime, answers, pool, |_| 0)
                                            .unwrap(),
                                        Progress::Pending
                                    ));
                                    assert!(runtime.next_ready().unwrap().is_none());
                                    let mut batch = exports.batch();
                                    let grant = batch
                                        .export(runtime.endpoint(42, Location::Local).unwrap())
                                        .unwrap();
                                    let frame = result(pool, id, grant.id());
                                    let frozen = Published::<1>::from_exports(
                                        &frame,
                                        pool,
                                        &batch,
                                        &mut runtime,
                                        &mut [0],
                                    )
                                    .unwrap();
                                    // Finish suppresses sending results, but local child calls still
                                    // own the result. Abort its unsent grants and keep the snapshot.
                                    drop(batch);
                                    runtime.release(exports.take_retired(0).unwrap());
                                    frame.release(pool);
                                    assert!(complete(answers, parent, Ok(frozen)).is_ok());
                                    assert!(answers
                                        .prepare_return(parent)
                                        .unwrap()
                                        .accept()
                                        .is_none());
                                    let reused = answers
                                        .admit(
                                            id,
                                            super::super::answer::State::new(Results::<_, 1>::new(
                                                43,
                                            )),
                                        )
                                        .unwrap_or_else(|_| panic!("reused"));
                                    assert_ne!(parent, reused);
                                    assert!(matches!(
                                        bindings
                                            .poll_one(0, &mut runtime, answers, pool, |_| 0)
                                            .unwrap(),
                                        Progress::Resolved(None)
                                    ));
                                    let ready = runtime.next_ready().unwrap().unwrap();
                                    assert_eq!(ready.owner(), &99);
                                    if invalid_path {
                                        assert!(matches!(
                                            ready.destination(),
                                            Destination::Endpoint {
                                                owner: &0,
                                                location: Location::Broken,
                                                ..
                                            }
                                        ));
                                    } else {
                                        assert!(matches!(
                                            ready.destination(),
                                            Destination::Endpoint { owner: &42, .. }
                                        ));
                                    }
                                    let running = ready.accept();
                                    assert_eq!(runtime.complete(running), 99);
                                    let retired = answers.take_finished(0).unwrap();
                                    assert_eq!(
                                        retired.release(pool).release(pool, &mut runtime),
                                        41
                                    );
                                    answers.finish(id, true);
                                    let retired =
                                        answers.prepare_return(reused).unwrap().accept().unwrap();
                                    assert_eq!(
                                        retired.release(pool).release(pool, &mut runtime),
                                        43
                                    );
                                    if invalid_path {
                                        let mut retired = [
                                            runtime.take_retired().unwrap(),
                                            runtime.take_retired().unwrap(),
                                        ];
                                        retired.sort_unstable();
                                        assert_eq!(retired, [0, 42]);
                                    } else {
                                        assert_eq!(runtime.take_retired(), Some(42));
                                    }
                                    assert!(runtime.is_empty());
                                    assert!(answers.is_empty());
                                },
                            )
                            .unwrap();
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
}

#[test]
fn canceled_observer_releases_parent_and_capacity_failures_preserve_dependencies() {
    for graph_capacity in [0, 1] {
        messages::with_messages(&mut [], &mut [], |pool| {
            let mut nodes = [clients::Slot::EMPTY];
            clients::with_graph(
                &mut nodes[..graph_capacity],
                |graph: &mut Graph<'_, '_, u32, u8>| {
                    answers::with_answers(&mut [answers::Slot::EMPTY], |answers| {
                        let id = AnswerId::from_wire(3);
                        let key = answers
                            .admit(
                                id,
                                super::super::answer::State::new(Results::<_, 0>::new(())),
                            )
                            .unwrap_or_else(|_| panic!("parent"));
                        let lease = answers.pipeline(id).unwrap();
                        let mut slots = [Slot::<1>::EMPTY];
                        let mut bindings = Bindings::new(&mut slots).unwrap();
                        assert!(matches!(
                            bindings.bind(graph, answers, &lease, pipeline::Path::new(&[0, 1])),
                            Err(Error::Capacity)
                        ));
                        let observed =
                            bindings.bind(graph, answers, &lease, pipeline::Path::new(&[]));
                        match observed {
                            Ok(client) => {
                                assert!(matches!(
                                    bindings.bind(graph, answers, &lease, pipeline::Path::new(&[])),
                                    Err(Error::Capacity)
                                ));
                                graph.release(client);
                            }
                            Err(error) => assert_eq!(error, Error::Graph(clients::Error::Full)),
                        }
                        answers.release_pipeline_deferred(lease);
                        answers.finish(id, false);
                        if graph_capacity == 1 {
                            assert!(!answers.cancellation_requested(key).unwrap());
                            assert!(matches!(
                                bindings
                                    .poll_one(0, graph, answers, pool, |_| panic!("abandoned"))
                                    .unwrap(),
                                Progress::Abandoned
                            ));
                        }
                        assert!(answers.cancellation_requested(key).unwrap());
                        assert!(!answers.dispatch(key, true).unwrap());
                        assert!(answers.prepare_return(key).unwrap().accept().is_some());
                        assert!(!bindings.drain_one(answers));
                        assert!(graph.is_empty());
                    })
                    .unwrap();
                },
            )
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn broken_producer_preserves_observer_until_graph_capacity_is_available() {
    messages::with_messages(&mut [], &mut [], |pool| {
        let mut nodes = [const { clients::Slot::EMPTY }; 2];
        clients::with_graph(&mut nodes, |graph: &mut Graph<'_, '_, u32, u8>| {
            answers::with_answers(&mut [answers::Slot::EMPTY], |answers| {
                let id = AnswerId::from_wire(3);
                let key = answers
                    .admit(
                        id,
                        super::super::answer::State::new(Results::<_, 0>::new(())),
                    )
                    .unwrap_or_else(|_| panic!("parent"));
                let lease = answers.pipeline(id).unwrap();
                let mut slots = [Slot::<0>::EMPTY];
                let mut bindings = Bindings::new(&mut slots).unwrap();
                let child = bindings
                    .bind(graph, answers, &lease, pipeline::Path::new(&[]))
                    .unwrap();
                answers.release_pipeline_deferred(lease);
                let occupying = graph.endpoint(9, Location::Local).unwrap();
                assert!(complete(answers, key, Err(capnp::ErrorKind::Unimplemented)).is_ok());
                assert!(matches!(
                    bindings.poll_one(0, graph, answers, pool, |_| panic!(
                        "factory called before capacity reservation"
                    )),
                    Err(Error::Graph(clients::Error::Full))
                ));
                graph.release(occupying);
                assert_eq!(graph.take_retired(), Some(9));
                assert!(matches!(
                    bindings
                        .poll_one(0, graph, answers, pool, |error| {
                            assert_eq!(error, capnp::ErrorKind::Unimplemented);
                            10
                        })
                        .unwrap(),
                    Progress::Resolved(None)
                ));
                let call = graph.prepare_call(&child).unwrap();
                assert!(matches!(
                    call.destination(),
                    Destination::Endpoint {
                        location: Location::Broken,
                        owner: &10,
                        ..
                    }
                ));
                let target = call.accept();
                graph.release(target);
                graph.release(child);
                assert_eq!(graph.take_retired(), Some(10));
                answers.finish(id, false);
                assert!(answers.prepare_return(key).unwrap().accept().is_some());
                assert!(graph.is_empty());
            })
            .unwrap();
        })
        .unwrap();
    })
    .unwrap();
}

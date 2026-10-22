use super::*;

#[test]
fn immediate_original_routes_preserve_rejection_and_reflected_ordering() {
    let measured = allocation_counter::measure(|| {
        for accepted in [false, true] {
            let mut clients = [const { Slot::<u32, u8>::EMPTY }; 5];
            let mut tasks = [TaskSlot::EMPTY];
            with_runtime(
                Storage {
                    notifications: crate::clients::NotificationStorage::empty(),
                    clients: &mut clients,
                    tasks: &mut tasks,
                },
                |runtime| {
                    let original = runtime.endpoint(41, Location::Remote(1)).unwrap();
                    let route = runtime.promise(&mut Some(original)).unwrap();
                    let waiting = runtime.promise(&mut None).unwrap();
                    assert!(runtime
                        .prepare_immediate_routed(&waiting)
                        .unwrap()
                        .is_none());
                    assert!(runtime.prepare_immediate(&route).unwrap().is_none());
                    let older = runtime
                        .prepare_call(&waiting, &mut Some(17))
                        .unwrap()
                        .accept();
                    assert!(runtime.prepare_immediate_routed(&route).unwrap().is_none());
                    assert_eq!(runtime.cancel(older), Ok(17));
                    let sequence = runtime.graph.dispatch_sequence;
                    drop(runtime.prepare_immediate_routed(&route).unwrap().unwrap());
                    assert_eq!(runtime.graph.dispatch_sequence, sequence);
                    if accepted {
                        runtime
                            .prepare_immediate_routed(&route)
                            .unwrap()
                            .unwrap()
                            .accept_published();
                        assert_eq!(runtime.graph.dispatch_sequence, sequence + 1);
                    }
                    let local = runtime.endpoint(73, Location::Local).unwrap();
                    let barrier = runtime
                        .prepare_resolution(route.key(), &mut Some(local), Source::Direct)
                        .unwrap()
                        .accept();
                    assert_eq!(barrier.is_some(), accepted);
                    if let Some(barrier) = barrier {
                        assert!(runtime.prepare_immediate_routed(&route).unwrap().is_none());
                        assert_eq!(*runtime.barrier_route(&barrier), 41);
                        runtime.open(barrier);
                    }
                    let ticket = runtime.prepare_immediate_routed(&route).unwrap().unwrap();
                    assert!(matches!(
                        ticket.destination(),
                        Destination::Endpoint {
                            owner: &73,
                            location: Location::Local,
                            ..
                        }
                    ));
                    drop(ticket);
                    runtime.release(waiting);
                    runtime.release(route);
                    assert_eq!(runtime.take_retired(), Some(41));
                    assert_eq!(runtime.take_retired(), Some(73));
                    assert!(runtime.is_empty());
                },
            )
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn immediate_dispatch_is_transactional_and_never_overtakes_queued_work() {
    let measured = allocation_counter::measure(|| {
        let mut clients = [const { Slot::<u32, u8>::EMPTY }; 3];
        let mut tasks = [TaskSlot::EMPTY];
        with_runtime(
            Storage {
                notifications: crate::clients::NotificationStorage::empty(),
                clients: &mut clients,
                tasks: &mut tasks,
            },
            |runtime| {
                let endpoint = runtime.endpoint(41, Location::Remote(1)).unwrap();
                let pending = runtime.promise(&mut None).unwrap();
                assert!(runtime.prepare_immediate(&pending).unwrap().is_none());
                let older = runtime
                    .prepare_call(&pending, &mut Some(17))
                    .unwrap()
                    .accept();
                assert!(runtime.prepare_immediate(&endpoint).unwrap().is_none());
                assert_eq!(runtime.cancel(older), Ok(17));
                let sequence = runtime.graph.dispatch_sequence;
                drop(runtime.prepare_immediate(&endpoint).unwrap().unwrap());
                assert_eq!(runtime.graph.dispatch_sequence, sequence);
                let ticket = runtime.prepare_immediate(&endpoint).unwrap().unwrap();
                assert!(matches!(
                    ticket.destination(),
                    Destination::Endpoint { owner: &41, .. }
                ));
                let retained = ticket.accept();
                assert_eq!(runtime.graph.dispatch_sequence, sequence + 1);
                runtime.release(retained);
                runtime.release(endpoint);
                runtime.release(pending);
                assert_eq!(runtime.take_retired(), Some(41));
                assert!(runtime.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn immediate_dispatch_checks_exhaustion_before_offering_publication() {
    let mut clients = [Slot::<u32, u8>::EMPTY];
    let mut tasks = [TaskSlot::<()>::EMPTY];
    with_runtime(
        Storage {
            notifications: crate::clients::NotificationStorage::empty(),
            clients: &mut clients,
            tasks: &mut tasks,
        },
        |runtime| {
            let endpoint = runtime.endpoint(41, Location::Remote(1)).unwrap();
            runtime.graph.dispatch_sequence = u64::MAX;
            assert!(matches!(
                runtime.prepare_immediate(&endpoint),
                Err(DispatchError::SequencesExhausted)
            ));
            runtime.graph.dispatch_sequence = 0;
            runtime.graph.slots[endpoint.key.raw.index]
                .entry
                .as_mut()
                .unwrap()
                .refs = u32::MAX;
            assert!(matches!(
                runtime.prepare_immediate(&endpoint),
                Err(DispatchError::Graph(Error::ReferencesExhausted))
            ));
            runtime.graph.slots[endpoint.key.raw.index]
                .entry
                .as_mut()
                .unwrap()
                .refs = 1;
            runtime.release(endpoint);
            assert_eq!(runtime.take_retired(), Some(41));
        },
    )
    .unwrap();
}

#[test]
fn published_immediate_call_marks_aliases_without_retaining_the_destination() {
    let measured = allocation_counter::measure(|| {
        let mut clients = [const { Slot::<u32, u8>::EMPTY }; 3];
        let mut tasks = [TaskSlot::<()>::EMPTY];
        with_runtime(
            Storage {
                notifications: crate::clients::NotificationStorage::empty(),
                clients: &mut clients,
                tasks: &mut tasks,
            },
            |runtime| {
                let endpoint = runtime.endpoint(41, Location::Remote(1)).unwrap();
                let first = runtime.promise(&mut None).unwrap();
                let second = runtime.promise(&mut None).unwrap();
                assert!(runtime
                    .prepare_resolution(first.key(), &mut Some(endpoint), Source::Direct)
                    .unwrap()
                    .accept()
                    .is_none());
                assert!(runtime
                    .prepare_resolution(second.key(), &mut Some(first), Source::Direct)
                    .unwrap()
                    .accept()
                    .is_none());
                let sequence = runtime.graph.dispatch_sequence;
                drop(runtime.prepare_immediate(&second).unwrap().unwrap());
                assert_eq!(runtime.graph.dispatch_sequence, sequence);
                for index in [1, 2] {
                    assert!(matches!(
                        runtime.graph.slots[index].entry.as_ref().unwrap().kind,
                        Kind::Promise { called: false, .. }
                    ));
                }
                runtime
                    .prepare_immediate(&second)
                    .unwrap()
                    .unwrap()
                    .accept_published();
                assert_eq!(runtime.graph.dispatch_sequence, sequence + 1);
                for index in [1, 2] {
                    assert!(matches!(
                        runtime.graph.slots[index].entry.as_ref().unwrap().kind,
                        Kind::Promise { called: true, .. }
                    ));
                }
                runtime.release(second);
                assert_eq!(runtime.take_retired(), Some(41));
                assert!(runtime.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn shutdown_drains_unresolved_owners_without_application_callbacks() {
    let mut clients = [Slot::<u32, u8>::EMPTY];
    let mut tasks = [TaskSlot::EMPTY];
    with_runtime(
        Storage {
            notifications: crate::clients::NotificationStorage::empty(),
            clients: &mut clients,
            tasks: &mut tasks,
        },
        |runtime| {
            let promise = runtime.promise(&mut None).unwrap();
            runtime
                .prepare_call(&promise, &mut Some(41))
                .unwrap()
                .accept();
            assert!(runtime.next_ready().unwrap().is_none());
            assert_eq!(runtime.drain_one(), Some(41));
            assert_eq!(runtime.drain_one(), None);
            runtime.release(promise);
            assert!(runtime.is_empty());
        },
    )
    .unwrap();
}

#[test]
fn borrowed_dispatch_scopes_preserve_clients_and_never_reuse_task_ids() {
    let measured = allocation_counter::measure(|| {
        let mut clients = [Slot::EMPTY];
        with_graph(
            &mut clients,
            crate::clients::NotificationStorage::empty(),
            |graph: &mut Graph<'_, '_, u32, u8>| {
                let client = graph.endpoint(41, Location::Local).unwrap();
                let mut tasks = [TaskSlot::EMPTY];
                let old = {
                    let mut runtime = borrow_runtime(graph, &mut tasks).unwrap();
                    let old = runtime
                        .prepare_call(&client, &mut Some(43))
                        .unwrap()
                        .accept();
                    assert_eq!(runtime.cancel(old), Ok(43));
                    old
                };
                assert_eq!(
                    graph.prepare_call(&client).unwrap().destination(),
                    Destination::Endpoint {
                        key: client.key(),
                        owner: &41,
                        location: Location::Local
                    }
                );
                let mut runtime = borrow_runtime(graph, &mut tasks).unwrap();
                let next = runtime
                    .prepare_call(&client, &mut Some(47))
                    .unwrap()
                    .accept();
                assert_ne!(next, old);
                assert_eq!(runtime.cancel(old), Err(DispatchError::Missing));
                let ready = runtime.next_ready().unwrap().unwrap().accept();
                assert_eq!(runtime.complete(ready), 47);
                drop(runtime);
                graph.release(client);
                assert_eq!(graph.take_retired(), Some(41));
                assert!(graph.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn borrowed_dispatcher_teardown_releases_queued_graph_references() {
    let mut clients = [Slot::EMPTY];
    with_graph(
        &mut clients,
        crate::clients::NotificationStorage::empty(),
        |graph: &mut Graph<'_, '_, u32, u8>| {
            let client = graph.endpoint(41, Location::Local).unwrap();
            let mut tasks = [TaskSlot::EMPTY];
            {
                let mut runtime = borrow_runtime(graph, &mut tasks).unwrap();
                runtime
                    .prepare_call(&client, &mut Some(43))
                    .unwrap()
                    .accept();
                runtime.release(client);
            }
            assert!(tasks[0].entry.is_none());
            assert_eq!(graph.take_retired(), Some(41));
            assert!(graph.is_empty());
        },
    )
    .unwrap();
}
#[test]
fn queued_calls_and_echo_keep_order_across_resolution_and_backpressure() {
    let measured = allocation_counter::measure(|| {
        let mut clients = [const { Slot::EMPTY }; 4];
        let mut tasks = [const { TaskSlot::EMPTY }; 6];
        with_runtime(
            Storage {
                notifications: crate::clients::NotificationStorage::empty(),
                clients: &mut clients,
                tasks: &mut tasks,
            },
            |r| {
                let remote = r.endpoint(10u32, Location::Remote(1u8)).unwrap();
                let p = r.promise(&mut Some(remote)).unwrap();
                // This call owns the old route even though it has not been written.
                r.prepare_call(&p, &mut Some(1u32)).unwrap().accept();
                let local = r.endpoint(20, Location::Local).unwrap();
                let barrier = r
                    .prepare_resolution(p.key(), &mut Some(local), Source::Direct)
                    .unwrap()
                    .accept()
                    .unwrap();
                r.prepare_call(&p, &mut Some(2)).unwrap().accept();
                let unrelated = r.endpoint(30, Location::Remote(2)).unwrap();
                r.prepare_call(&unrelated, &mut Some(3)).unwrap().accept();
                let original = r.retain_barrier_route(&barrier).unwrap();
                r.prepare_call(&original, &mut Some(4)).unwrap().accept(); // senderLoopback
                r.release(original);
                for expected in [1, 3, 4] {
                    for _ in 0..3 {
                        let ready = r.next_ready().unwrap().unwrap();
                        assert_eq!(*ready.owner(), expected);
                        drop(ready); // output not ready, preserve sequence and references
                    }
                    let ready = r.next_ready().unwrap().unwrap();
                    let Destination::Endpoint { owner, .. } = ready.destination() else {
                        unreachable!()
                    };
                    assert_eq!(*owner, if expected == 3 { 30 } else { 10 });
                    let task = ready.accept();
                    assert_eq!(r.complete(task), expected);
                }
                assert!(r.next_ready().unwrap().is_none());
                assert_eq!(r.queued(), 1);
                r.open(barrier); // called only after the wire embargo accepts its echo
                r.prepare_call(&p, &mut Some(5)).unwrap().accept();
                for expected in [2, 5] {
                    let ready = r.next_ready().unwrap().unwrap();
                    assert_eq!(*ready.owner(), expected);
                    let Destination::Endpoint { owner, .. } = ready.destination() else {
                        unreachable!()
                    };
                    assert_eq!(*owner, 20);
                    let task = ready.accept();
                    assert_eq!(r.complete(task), expected);
                }
                r.release(p);
                r.release(unrelated);
                while r.take_retired().is_some() {}
                assert!(r.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
#[test]
fn capacity_rejection_cancellation_and_stale_keys_do_not_consume_call_owners() {
    let mut clients = [Slot::EMPTY];
    let mut tasks = [TaskSlot::EMPTY];
    with_runtime(
        Storage {
            notifications: crate::clients::NotificationStorage::empty(),
            clients: &mut clients,
            tasks: &mut tasks,
        },
        |r: &mut Runtime<'_, '_, '_, u8, u8, u8>| {
            let c = r.endpoint(1, Location::Local).unwrap();
            let mut owner = Some(2);
            drop(r.prepare_call(&c, &mut owner).unwrap());
            assert_eq!(owner, Some(2));
            assert_eq!(r.queued(), 0);
            let old = r.prepare_call(&c, &mut owner).unwrap().accept();
            assert_eq!(owner, None);
            owner = Some(3);
            assert_eq!(
                r.prepare_call(&c, &mut owner).err().unwrap(),
                DispatchError::Full
            );
            assert_eq!(owner, Some(3));
            assert_eq!(r.cancel(old), Ok(2));
            let new = r.prepare_call(&c, &mut owner).unwrap().accept();
            assert_ne!(old, new);
            assert_eq!(r.cancel(old), Err(DispatchError::Missing));
            assert_eq!(r.cancel(new), Ok(3));
            r.graph.dispatch_sequence = u64::MAX;
            owner = Some(4);
            assert_eq!(
                r.prepare_call(&c, &mut owner).err().unwrap(),
                DispatchError::SequencesExhausted
            );
            assert_eq!(owner, Some(4));
            r.release(c);
            assert_eq!(r.take_retired(), Some(1));
            assert!(r.is_empty());
        },
    )
    .unwrap();
}
#[test]
fn terminal_teardown_drops_call_and_endpoint_owners_once() {
    use core::cell::Cell;
    struct Owner<'a>(&'a Cell<u32>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let dropped = Cell::new(0);
    let mut clients = [const { Slot::EMPTY }; 2];
    let mut tasks = [TaskSlot::EMPTY];
    with_runtime(
        Storage {
            notifications: crate::clients::NotificationStorage::empty(),
            clients: &mut clients,
            tasks: &mut tasks,
        },
        |r: &mut Runtime<'_, '_, '_, Owner<'_>, u8, Owner<'_>>| {
            let endpoint = r
                .endpoint(Owner(&dropped), Location::Remote(1))
                .ok()
                .unwrap();
            let p = r.promise(&mut Some(endpoint)).unwrap();
            r.prepare_call(&p, &mut Some(Owner(&dropped)))
                .unwrap()
                .accept();
            assert_eq!(dropped.get(), 0);
        },
    )
    .unwrap();
    assert_eq!(dropped.get(), 2);
    assert!(clients.iter().all(|s| s.entry.is_none()));
    assert!(tasks.iter().all(|s| s.entry.is_none()));
}

#[test]
fn taking_ownership_preserves_graph_and_dispatch_identity_on_rejection() {
    let measured = allocation_counter::measure(|| {
        let mut nodes = [Slot::EMPTY];
        let mut graph =
            crate::clients::new_scope(&mut nodes, crate::clients::NotificationStorage::empty())
                .unwrap();
        let client = graph.endpoint(41u32, Location::<u8>::Local).unwrap();
        let mut tasks = [TaskSlot::EMPTY];
        let old = {
            let mut borrowed = borrow_runtime(&mut graph, &mut tasks).unwrap();
            let old = borrowed
                .prepare_call(&client, &mut Some(43))
                .unwrap()
                .accept();
            assert_eq!(borrowed.cancel(old), Ok(43));
            old
        };
        // Reject occupied task storage without dropping the established graph.
        let retained = graph.retain(&client).unwrap();
        tasks[0].entry = Some(Queued {
            target: retained.key.raw,
            sequence: 0,
            reserved_control: false,
            owner: 47,
            waiting: None,
            previous: None,
            next: None,
        });
        let rejected = own_runtime(graph, &mut tasks).err().unwrap();
        assert_eq!(rejected.error, Error::NonemptyStorage);
        let mut graph = rejected.value;
        let entry = tasks[0].entry.take().unwrap();
        graph.release_raw(entry.target);
        let mut runtime =
            own_runtime(graph, &mut tasks).unwrap_or_else(|_| panic!("empty task storage"));
        let next = runtime
            .prepare_call(&client, &mut Some(53))
            .unwrap()
            .accept();
        assert_ne!(old, next);
        assert_eq!(runtime.cancel(old), Err(DispatchError::Missing));
        let ready = runtime.next_ready().unwrap().unwrap();
        assert!(matches!(
            ready.destination(),
            Destination::Endpoint { owner: &41, .. }
        ));
        let running = ready.accept();
        assert_eq!(runtime.complete(running), 53);
        runtime.release(client);
        assert_eq!(runtime.take_retired(), Some(41));
        assert!(runtime.is_empty());
    });
    assert_eq!(measured.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_dispatch_preserves_order_rollback_and_cancellation_identity() {
    let mut nodes = [Slot::<u32, u8>::EMPTY];
    let graph = super::super::new_scope(&mut nodes, NotificationStorage::empty()).unwrap();
    let tasks = crate::storage::Storage::growing(0, 3, || TaskSlot::EMPTY).unwrap();
    let mut runtime = own_runtime_with_storage(graph, tasks).unwrap_or_else(|_| panic!("fresh"));
    let client = runtime.endpoint(41, Location::Local).unwrap();
    assert_eq!(runtime.task_capacity(), 0);
    let mut first = Some(43);
    drop(runtime.prepare_call(&client, &mut first).unwrap());
    assert_eq!(first, Some(43), "rejected admission retains the owner");
    assert_eq!(runtime.queued(), 0);
    let old = runtime.prepare_call(&client, &mut first).unwrap().accept();
    let second = runtime
        .prepare_call(&client, &mut Some(47))
        .unwrap()
        .accept();
    assert_eq!(runtime.cancel(old), Ok(43));
    let new = runtime
        .prepare_call(&client, &mut Some(53))
        .unwrap()
        .accept();
    let third = runtime
        .prepare_call(&client, &mut Some(59))
        .unwrap()
        .accept();
    assert_eq!(runtime.task_capacity(), 3);
    let mut rejected = Some(61);
    assert!(matches!(
        runtime.prepare_call(&client, &mut rejected),
        Err(DispatchError::Full)
    ));
    assert_eq!(rejected, Some(61));
    assert_eq!(runtime.cancel(old), Err(DispatchError::Missing));
    assert!(!runtime.reserve_task_capacity(usize::MAX));
    for (key, value) in [(second, 47), (new, 53), (third, 59)] {
        let ready = runtime.next_ready().unwrap().unwrap();
        assert_eq!(
            ready.key(),
            key,
            "slot reuse must not overtake an older task"
        );
        drop(ready);
        let running = runtime.next_ready().unwrap().unwrap().accept();
        assert_eq!(runtime.complete(running), value);
    }
    runtime.release(client);
    assert_eq!(runtime.take_retired(), Some(41));
    assert!(runtime.is_empty());
}

#[test]
fn one_resolution_visits_one_parked_task_after_large_growth() {
    let mut nodes = [const { Slot::EMPTY }; 1026];
    let mut tasks = [const { TaskSlot::EMPTY }; 1024];
    with_runtime(
        Storage {
            notifications: crate::clients::NotificationStorage::empty(),
            clients: &mut nodes,
            tasks: &mut tasks,
        },
        |runtime: &mut Runtime<'_, '_, '_, u32, u8, u32>| {
            let mut clients = [const { None }; 1024];
            let mut keys = [None; 1024];
            for index in 0..1024 {
                let client = runtime.promise(&mut None).unwrap();
                keys[index] = Some(
                    runtime
                        .prepare_call(&client, &mut Some(index as u32))
                        .unwrap()
                        .accept(),
                );
                clients[index] = Some(client);
            }
            assert!(runtime.next_ready().unwrap().is_none());
            assert_eq!(runtime.routing_visits, 0);
            let endpoint = runtime.endpoint(91, Location::Local).unwrap();
            let replacement = runtime.retain(&endpoint).unwrap();
            runtime
                .prepare_resolution(
                    clients[777].as_ref().unwrap().key(),
                    &mut Some(replacement),
                    Source::Direct,
                )
                .unwrap()
                .accept();
            let ready = runtime.next_ready().unwrap().unwrap();
            assert_eq!(*ready.owner(), 777);
            let running = ready.accept();
            assert_eq!(runtime.complete(running), 777);
            assert_eq!(runtime.routing_visits, 1);
            assert!(runtime.next_ready().unwrap().is_none());
            assert_eq!(runtime.routing_visits, 1);
            for (index, key) in keys.into_iter().enumerate() {
                if index != 777 {
                    assert_eq!(runtime.cancel(key.unwrap()), Ok(index as u32));
                }
            }
            for client in clients.into_iter().flatten() {
                runtime.release(client);
            }
            runtime.release(endpoint);
            assert_eq!(runtime.take_retired(), Some(91));
            assert!(runtime.is_empty());
        },
    )
    .unwrap();
}

#[test]
fn canceling_changed_promise_unlinks_before_graph_slot_reuse() {
    let mut nodes = [const { Slot::EMPTY }; 3];
    let mut tasks = [const { TaskSlot::EMPTY }; 2];
    with_runtime(
        Storage {
            notifications: crate::clients::NotificationStorage::empty(),
            clients: &mut nodes,
            tasks: &mut tasks,
        },
        |runtime: &mut Runtime<'_, '_, '_, u32, u8, u32>| {
            let source = runtime.promise(&mut None).unwrap();
            let middle = runtime.promise(&mut None).unwrap();
            let first = runtime
                .prepare_call(&source, &mut Some(11))
                .unwrap()
                .accept();
            let second = runtime
                .prepare_call(&middle, &mut Some(13))
                .unwrap()
                .accept();
            let replacement = runtime.retain(&middle).unwrap();
            runtime
                .prepare_resolution(source.key(), &mut Some(replacement), Source::Direct)
                .unwrap()
                .accept();
            assert!(runtime.next_ready().unwrap().is_none());
            assert_eq!(runtime.routing_visits, 1);
            let endpoint = runtime.endpoint(91, Location::Local).unwrap();
            runtime
                .prepare_resolution(middle.key(), &mut Some(endpoint), Source::Direct)
                .unwrap()
                .accept();
            assert_eq!(runtime.cancel(first), Ok(11));
            assert_eq!(runtime.cancel(second), Ok(13));
            runtime.release(source);
            runtime.release(middle);
            assert!(runtime.graph.dispatch_changes.head.is_none());
            let fresh = runtime.endpoint(97, Location::Local).unwrap();
            runtime
                .prepare_call(&fresh, &mut Some(17))
                .unwrap()
                .accept();
            let running = runtime.next_ready().unwrap().unwrap().accept();
            assert_eq!(runtime.complete(running), 17);
            runtime.release(fresh);
            assert_eq!(runtime.take_retired(), Some(91));
            assert_eq!(runtime.take_retired(), Some(97));
            assert!(runtime.is_empty());
        },
    )
    .unwrap();
}

#[test]
fn reserved_control_accounting_is_transactional_across_dispatch_cancel_and_reuse() {
    let measured = allocation_counter::measure(|| {
        let mut nodes = [Slot::EMPTY];
        let mut tasks = [const { TaskSlot::EMPTY }; 2];
        with_runtime(
            Storage {
                clients: &mut nodes,
                notifications: NotificationStorage::empty(),
                tasks: &mut tasks,
            },
            |runtime: &mut Runtime<'_, '_, '_, u8, u8, u8>| {
                let target = runtime.endpoint(7, Location::Local).unwrap();
                drop(
                    runtime
                        .prepare_call(&target, &mut Some(1))
                        .unwrap()
                        .reserved_control(),
                );
                assert_eq!((runtime.queued(), runtime.ordinary_queued()), (0, 0));
                let control = runtime
                    .prepare_call(&target, &mut Some(2))
                    .unwrap()
                    .reserved_control()
                    .accept();
                let ordinary = runtime
                    .prepare_call(&target, &mut Some(3))
                    .unwrap()
                    .accept();
                assert_eq!((runtime.queued(), runtime.ordinary_queued()), (2, 1));
                assert_eq!(runtime.cancel(ordinary), Ok(3));
                assert_eq!((runtime.queued(), runtime.ordinary_queued()), (1, 0));
                let running = runtime.next_ready().unwrap().unwrap().accept();
                assert_eq!(runtime.complete(running), 2);
                assert_eq!(runtime.cancel(control), Err(DispatchError::Missing));
                assert_eq!((runtime.queued(), runtime.ordinary_queued()), (0, 0));
                runtime
                    .prepare_call(&target, &mut Some(4))
                    .unwrap()
                    .accept();
                assert_eq!(runtime.drain_one(), Some(4));
                assert_eq!((runtime.queued(), runtime.ordinary_queued()), (0, 0));
                runtime.release(target);
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

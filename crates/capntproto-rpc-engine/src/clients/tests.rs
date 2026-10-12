use super::*;

#[test]
fn retired_fifo_survives_live_neighbors_reuse_and_reentry() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [const { Slot::EMPTY }; 4];
        with_graph(&mut slots, |g: &mut Graph<'_, '_, u32, u8>| {
            let first = endpoint(g, 1, Location::Local);
            let neighbor = endpoint(g, 2, Location::Local);
            let last = endpoint(g, 3, Location::Local);
            assert!(!g.has_retired());
            g.release(last);
            g.release(first);
            assert!(g.has_retired());
            assert_eq!(g.take_retired(), Some(3));
            let reused = endpoint(g, 4, Location::Local);
            g.release(reused);
            assert_eq!(g.take_retired(), Some(1));
            assert_eq!(g.take_retired(), Some(4));
            assert!(!g.has_retired());
            assert_eq!(call_value(g, &neighbor), 2);
            g.release(neighbor);
            assert_eq!(g.take_retired(), Some(2));
            assert!(g.is_empty());
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_graph_preserves_live_routes_and_stale_generations() {
    with_growing_graph::<u32, u8, _>(0, 64, |g| {
        let route = endpoint(g, 17, Location::Remote(1));
        let initial = g.retain(&route).unwrap();
        let promise = promise(g, initial);
        assert_eq!(call_value(g, &promise), 17);
        let frozen = g.snapshot(&promise).unwrap();
        let mut owners = alloc::vec::Vec::new();
        for n in 0..40 {
            owners.push(endpoint(g, n + 100, Location::Local));
            assert_eq!(call_value(g, &promise), 17);
            assert_eq!(call_value(g, &frozen), 17);
        }
        let released = owners.pop().unwrap();
        let stale = released.key();
        g.release(released);
        assert_eq!(g.take_retired(), Some(139));
        let reused = endpoint(g, 211, Location::Local);
        assert_ne!(stale, reused.key());
        assert!(matches!(g.retain_key(stale), Err(Error::Stale)));
        for owner in owners {
            g.release(owner);
        }
        g.release(reused);
        g.release(promise);
        g.release(frozen);
        g.release(route);
        let mut count = 0;
        while g.take_retired().is_some() {
            count += 1;
        }
        assert_eq!(count, 41);
        assert!(g.is_empty());
    })
    .unwrap();
}

#[cfg(feature = "alloc")]
#[test]
fn growing_imported_promise_reserves_both_nodes_before_taking_owner() {
    with_growing_graph::<u32, u8, _>(1, 2, |g| {
        let existing = endpoint(g, 17, Location::Local);
        let rejected = g.imported_promise(19, 1).unwrap_err();
        assert_eq!(rejected.error, Error::Full);
        assert_eq!(rejected.value, 19);
        assert_eq!(g.slots.len(), 1, "rejected pair cannot partially grow");
        g.release(existing);
        assert_eq!(g.take_retired(), Some(17));
        let imported = g.imported_promise(rejected.value, 1).unwrap();
        assert_eq!(g.slots.len(), 2);
        assert_eq!(call_value(g, &imported), 19);
        assert_eq!(
            g.endpoint_with(Location::Local, || panic!("limit factory"))
                .unwrap_err(),
            Error::Full
        );
        g.release(imported);
        assert_eq!(g.take_retired(), Some(19));
        assert!(g.is_empty());
    })
    .unwrap();
}

#[cfg(feature = "alloc")]
#[test]
fn growing_graph_allocation_rejection_and_fixed_no_fallback() {
    assert_eq!(
        with_growing_graph::<u32, u8, ()>(2, 1, |_| panic!("invalid bounds")),
        Err(Error::Full)
    );
    assert_eq!(
        with_growing_graph::<u32, u8, ()>(usize::MAX, usize::MAX, |_| panic!("invalid layout")),
        Err(Error::Full)
    );
    let measured = allocation_counter::measure(|| {
        with_graph::<u32, u8, _>(&mut [], |g| {
            assert_eq!(
                g.endpoint_with(Location::Local, || panic!("fixed factory"))
                    .unwrap_err(),
                Error::Full
            );
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
fn endpoint<'id>(
    g: &mut Graph<'id, '_, u32, u8>,
    value: u32,
    location: Location<u8>,
) -> Client<'id> {
    g.endpoint(value, location).unwrap()
}
fn promise<'id>(g: &mut Graph<'id, '_, u32, u8>, route: Client<'id>) -> Client<'id> {
    g.promise(&mut Some(route)).unwrap()
}
fn call_value<'id>(g: &mut Graph<'id, '_, u32, u8>, c: &Client<'id>) -> u32 {
    // The explicit lifetime below is needed for invariant graph domains.
    fn run<'id>(g: &mut Graph<'id, '_, u32, u8>, c: &Client<'id>) -> u32 {
        let pending = g.prepare_call(c).unwrap();
        let Destination::Endpoint { owner, .. } = pending.destination() else {
            panic!("blocked")
        };
        let result = *owner;
        let owned = pending.accept();
        g.release(owned);
        result
    }
    run(g, c)
}
#[test]
fn advertised_route_does_not_follow_a_later_remote_resolution() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [const { Slot::EMPTY }; 6];
        with_graph(&mut slots, |g| {
            let remote = endpoint(g, 10, Location::Remote(1));
            let remote_promise = promise(g, remote);
            let mut frozen = Some(g.snapshot(&remote_promise).unwrap());
            let exported = g.promise(&mut None).unwrap();
            g.prepare_resolution(exported.key(), &mut frozen, Source::Direct)
                .unwrap()
                .accept();
            assert!(frozen.is_none());
            let local = endpoint(g, 20, Location::Local);
            // The old export has promised to keep using remote endpoint 10.
            g.prepare_resolution(remote_promise.key(), &mut Some(local), Source::Direct)
                .unwrap()
                .accept();
            assert_eq!(call_value(g, &remote_promise), 20);
            assert_eq!(call_value(g, &exported), 10);
            g.release(remote_promise);
            g.release(exported);
            let mut retired = [g.take_retired().unwrap(), g.take_retired().unwrap()];
            retired.sort_unstable();
            assert_eq!(retired, [10, 20]);
            assert!(g.is_empty());
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
#[test]
fn reflected_same_peer_still_waits_for_original_route_echo() {
    for reflected in [false, true] {
        let mut slots = [const { Slot::EMPTY }; 3];
        with_graph(&mut slots, |g| {
            let initial = endpoint(g, 10, Location::Remote(1));
            let p = promise(g, initial);
            assert_eq!(call_value(g, &p), 10);
            let replacement = endpoint(g, 20, Location::Remote(1));
            let barrier = g
                .prepare_resolution(
                    p.key(),
                    &mut Some(replacement),
                    if reflected {
                        Source::Reflected
                    } else {
                        Source::Direct
                    },
                )
                .unwrap()
                .accept();
            assert_eq!(barrier.is_some(), reflected);
            if let Some(barrier) = barrier {
                assert_eq!(*g.barrier_route(&barrier), 10);
                assert!(matches!(
                    g.prepare_call(&p).unwrap().destination(),
                    Destination::Waiting(_)
                ));
                assert!(g.take_retired().is_none());
                g.open(barrier);
            }
            assert_eq!(call_value(g, &p), 20);
            g.release(p);
            assert_eq!(g.take_retired(), Some(10));
            assert_eq!(g.take_retired(), Some(20));
            assert!(g.is_empty());
        })
        .unwrap();
    }
}
#[test]
fn canceled_observer_does_not_destroy_an_outstanding_barrier() {
    let mut slots = [const { Slot::EMPTY }; 3];
    with_graph(&mut slots, |g| {
        let initial = endpoint(g, 10, Location::Remote(1));
        let p = promise(g, initial);
        assert_eq!(call_value(g, &p), 10);
        let local = endpoint(g, 20, Location::Local);
        let barrier = g
            .prepare_resolution(p.key(), &mut Some(local), Source::Direct)
            .unwrap()
            .accept()
            .unwrap();
        let old = p.key();
        g.release(p);
        assert_eq!(*g.barrier_route(&barrier), 10);
        assert!(g.take_retired().is_none());
        g.open(barrier);
        assert_eq!(g.retain_key(old).unwrap_err(), Error::Stale);
        assert_eq!(g.take_retired(), Some(10));
        assert_eq!(g.take_retired(), Some(20));
        assert!(g.is_empty());
    })
    .unwrap();
}
#[test]
fn resolution_faults_preserve_replacement_and_original_route() {
    let mut slots = [const { Slot::EMPTY }; 4];
    with_graph(&mut slots, |g| {
        let initial = endpoint(g, 10, Location::Remote(1));
        let p = promise(g, initial);
        assert_eq!(call_value(g, &p), 10);
        let mut replacement = Some(endpoint(g, 20, Location::Local));
        drop(
            g.prepare_resolution(p.key(), &mut replacement, Source::Direct)
                .unwrap(),
        );
        assert!(replacement.is_some());
        assert_eq!(call_value(g, &p), 10);
        let refs = g.slots[p.key.raw.index].entry.as_ref().unwrap().refs;
        g.slots[p.key.raw.index].entry.as_mut().unwrap().refs = u32::MAX;
        assert_eq!(
            g.prepare_resolution(p.key(), &mut replacement, Source::Direct)
                .err()
                .unwrap(),
            Error::ReferencesExhausted
        );
        assert!(replacement.is_some());
        g.slots[p.key.raw.index].entry.as_mut().unwrap().refs = refs;
        let barrier = g
            .prepare_resolution(p.key(), &mut replacement, Source::Direct)
            .unwrap()
            .accept()
            .unwrap();
        replacement = Some(endpoint(g, 30, Location::Local));
        assert_eq!(
            g.prepare_resolution(p.key(), &mut replacement, Source::Direct)
                .err()
                .unwrap(),
            Error::AlreadyResolved
        );
        g.open(barrier);
        assert_eq!(call_value(g, &p), 20);
        g.release(replacement.take().unwrap());
        g.release(p);
        while g.take_retired().is_some() {}
        assert!(g.is_empty());
    })
    .unwrap();
}
#[test]
fn cyclic_resolution_is_rejected_without_losing_ownership() {
    let mut slots = [const { Slot::EMPTY }; 3];
    with_graph(&mut slots, |g: &mut Graph<'_, '_, u32, u8>| {
        let a = g.promise(&mut None).unwrap();
        let b = g.promise(&mut None).unwrap();
        let mut a_ref = Some(g.retain(&a).unwrap());
        let mut b_ref = Some(g.retain(&b).unwrap());
        g.prepare_resolution(a.key(), &mut b_ref, Source::Direct)
            .unwrap()
            .accept();
        assert_eq!(
            g.prepare_resolution(b.key(), &mut a_ref, Source::Direct)
                .err()
                .unwrap(),
            Error::Cycle
        );
        assert!(a_ref.is_some());
        g.release(a_ref.take().unwrap());
        g.release(a);
        g.release(b);
        assert!(g.is_empty());
    })
    .unwrap();
}
#[test]
fn rejected_call_does_not_mark_promise_used_and_broken_needs_no_embargo() {
    for broken in [false, true] {
        let mut slots = [const { Slot::EMPTY }; 3];
        with_graph(&mut slots, |g| {
            let initial = endpoint(g, 10, Location::Remote(1));
            let p = promise(g, initial);
            if broken {
                assert_eq!(call_value(g, &p), 10);
            } else {
                drop(g.prepare_call(&p).unwrap());
            }
            let local = endpoint(
                g,
                20,
                if broken {
                    Location::Broken
                } else {
                    Location::Local
                },
            );
            assert!(g
                .prepare_resolution(p.key(), &mut Some(local), Source::Direct)
                .unwrap()
                .accept()
                .is_none());
            assert_eq!(call_value(g, &p), 20);
            g.release(p);
            while g.take_retired().is_some() {}
            assert!(g.is_empty());
        })
        .unwrap();
    }
}
#[test]
fn pending_local_chains_have_owned_calls_and_iterative_cleanup() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [const { Slot::EMPTY }; 64];
        with_graph(&mut slots, |g: &mut Graph<'_, '_, u32, u8>| {
            let mut first = g.promise(&mut None).unwrap();
            let original = g.retain(&first).unwrap();
            for _ in 0..62 {
                let next = g.promise(&mut None).unwrap();
                let mut owned = Some(g.retain(&next).unwrap());
                g.prepare_resolution(first.key(), &mut owned, Source::Direct)
                    .unwrap()
                    .accept();
                g.release(first);
                first = next;
            }
            let waiting = g.prepare_call(&original).unwrap();
            assert!(matches!(waiting.destination(), Destination::Waiting(_)));
            let call = waiting.accept();
            g.release(original);
            let leaf = endpoint(g, 77, Location::Local);
            g.prepare_resolution(first.key(), &mut Some(leaf), Source::Direct)
                .unwrap()
                .accept();
            g.release(first);
            assert_eq!(call_value(g, &call), 77);
            g.release(call);
            assert_eq!(g.take_retired(), Some(77));
            assert!(g.is_empty());
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
#[test]
fn stale_keys_capacity_and_generation_exhaustion_preserve_owners() {
    let mut slots = [Slot::EMPTY];
    with_graph(&mut slots, |g| {
        let c = endpoint(g, 1, Location::Local);
        let old = c.key();
        assert_eq!(g.endpoint(2, Location::Local).unwrap_err().value, 2);
        g.release(c);
        assert_eq!(
            g.endpoint(3, Location::Local).unwrap_err().error,
            Error::Full
        );
        assert_eq!(g.take_retired(), Some(1));
        let c = endpoint(g, 4, Location::Local);
        assert_eq!(g.retain_key(old).unwrap_err(), Error::Stale);
        g.release(c);
        assert_eq!(g.take_retired(), Some(4));
        g.slots[0].generation = u64::MAX;
        let failure = g.endpoint(5, Location::Local).unwrap_err();
        assert_eq!(failure.error, Error::GenerationsExhausted);
        assert_eq!(failure.value, 5);
    })
    .unwrap();
}

#[test]
fn callable_local_promise_exports_resolution_obligation_until_settled() {
    let mut slots = [const { Slot::EMPTY }; 3];
    with_graph::<u8, u8, _>(&mut slots, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let key = route.key();
        let promise = graph.promise(&mut Some(route)).unwrap();
        let (snapshot, pending) = graph.snapshot_for_export(&promise).unwrap();
        assert!(pending);
        assert_eq!(snapshot.key(), promise.key());
        assert!(!graph.is_settled(&promise).unwrap());
        let call = graph.prepare_call(&promise).unwrap().accept();
        assert_eq!(call.key(), key, "calls retain the original proxy");
        let mut replacement = Some(graph.retain_key(key).unwrap());
        assert!(graph
            .resolve(promise.key(), &mut replacement, Source::Direct)
            .unwrap()
            .is_none());
        assert!(graph.is_settled(&snapshot).unwrap());
        let (settled, pending) = graph.snapshot_for_export(&snapshot).unwrap();
        assert!(!pending);
        assert_eq!(settled.key(), key);
        for client in [settled, call, snapshot, promise] {
            graph.release(client);
        }
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

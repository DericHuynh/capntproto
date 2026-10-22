use super::*;
#[kani::proof]
#[kani::unwind(4)]
fn frozen_route_and_reflected_barrier_preserve_order() {
    frozen_route_case(true, true);
}
#[kani::proof]
#[kani::unwind(4)]
#[kani::solver(kissat)]
fn frozen_route_direct_after_call() {
    frozen_route_case(true, false);
}
#[kani::proof]
#[kani::unwind(4)]
fn frozen_route_reflected_before_call() {
    frozen_route_case(false, true);
}
#[kani::proof]
#[kani::unwind(4)]
fn frozen_route_direct_before_call() {
    frozen_route_case(false, false);
}

// Prove the full boolean cross-product separately. A single symbolic history
// builds all four large ownership graphs at once before propositional slicing,
// exhausting hosted-runner memory. The peer remains arbitrary in every case.
fn frozen_route_case(called: bool, reflected: bool) {
    let peer: u8 = kani::any();
    let mut slots = [const { Slot::EMPTY }; 3];
    with_graph(
        &mut slots,
        crate::clients::NotificationStorage::empty(),
        |graph| {
            let initial = graph.endpoint(11u8, Location::Remote(peer)).unwrap();
            let p = graph.promise(&mut Some(initial)).unwrap();
            kani::assert(
                !graph.is_settled(&p).unwrap(),
                "routable imported promise still owes resolution",
            );
            let frozen = graph.snapshot(&p).unwrap();
            kani::assert(
                graph.is_settled(&frozen).unwrap(),
                "frozen endpoint is independent of promise readiness",
            );
            if called {
                let target = graph.prepare_call(&p).unwrap().accept();
                graph.release(target);
            }
            let replacement = graph.endpoint(22, Location::Remote(peer)).unwrap();
            let barrier = graph
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
            kani::assert(
                barrier.is_some() == (called && reflected),
                "reflected resolution retains its loopback barrier",
            );
            let call = graph.prepare_call(&frozen).unwrap();
            kani::assert(
                matches!(call.destination(), Destination::Endpoint { owner: &11, .. }),
                "frozen route preserves the advertised endpoint",
            );
            drop(call);
            if let Some(barrier) = barrier {
                kani::assert(
                    !graph.is_settled(&p).unwrap(),
                    "readiness cannot bypass a reflected barrier",
                );
                kani::assert(
                    matches!(
                        graph.prepare_call(&p).unwrap().destination(),
                        Destination::Waiting(_)
                    ),
                    "new calls cannot bypass the loopback barrier",
                );
                graph.open(barrier);
            }
            kani::assert(
                graph.is_settled(&p).unwrap(),
                "only the opened route is settled",
            );
            kani::assert(
                matches!(
                    graph.prepare_call(&p).unwrap().destination(),
                    Destination::Endpoint { owner: &22, .. }
                ),
                "released barrier exposes the replacement",
            );
            graph.release(p);
            graph.release(frozen);
            let retired = (graph.take_retired(), graph.take_retired());
            assert!(matches!(
                retired,
                (Some(11), Some(22)) | (Some(22), Some(11))
            ));
            assert!(graph.is_empty());
        },
    )
    .unwrap();
}
#[kani::proof]
#[kani::unwind(4)]
fn full_width_reference_overflow_preserves_resolution() {
    let refs: u32 = kani::any();
    kani::assume(refs > 0);
    let mut slots = [const { Slot::EMPTY }; 3];
    with_graph(
        &mut slots,
        crate::clients::NotificationStorage::empty(),
        |graph: &mut Graph<'_, '_, u8, u8>| {
            let initial = graph.endpoint(11, Location::Remote(1)).unwrap();
            let p = graph.promise(&mut Some(initial)).unwrap();
            let call = graph.prepare_call(&p).unwrap().accept();
            graph.release(call);
            let mut replacement = Some(graph.endpoint(22, Location::Local).unwrap());
            graph.slots[p.key.raw.index].entry.as_mut().unwrap().refs = refs;
            match graph.prepare_resolution(p.key(), &mut replacement, Source::Direct) {
                Err(e) => {
                    kani::assert(
                        refs == u32::MAX && e == Error::ReferencesExhausted,
                        "barrier overflow is rejected exactly at capacity",
                    );
                    assert!(replacement.is_some());
                    assert!(matches!(
                        graph.prepare_call(&p).unwrap().destination(),
                        Destination::Endpoint { owner: &11, .. }
                    ));
                }
                Ok(prepared) => {
                    assert!(refs < u32::MAX);
                    let barrier = prepared.accept().unwrap();
                    kani::assert(
                        graph.slots[p.key.raw.index].entry.as_ref().unwrap().refs == refs + 1,
                        "barrier owns a checked reference",
                    );
                    graph.open(barrier);
                    assert_eq!(
                        graph.slots[p.key.raw.index].entry.as_ref().unwrap().refs,
                        refs
                    );
                }
            }
        },
    )
    .unwrap();
}

#[kani::proof]
#[kani::unwind(4)]
fn imported_promise_admission_preserves_owner_and_both_slots() {
    let first: u64 = kani::any();
    let second: u64 = kani::any();
    let owner: u8 = kani::any();
    let mut slots = [const { Slot::EMPTY }; 2];
    slots[0].generation = first;
    slots[1].generation = second;
    with_graph(&mut slots, crate::clients::NotificationStorage::empty(), |graph: &mut Graph<'_, '_, u8, u8>| {
        match graph.imported_promise(owner, 1) {
            Ok(client) => {
                assert!(first != u64::MAX && second != u64::MAX);
                let Kind::Promise { target: Some(route), .. } = graph.live(client.key.raw).unwrap().kind else { unreachable!() };
                kani::assert(route.index != client.key.raw.index && matches!(graph.live(route).unwrap().kind, Kind::Endpoint { owner: value, location: Location::Remote(1) } if value == owner), "imported promise preserves a distinct stable endpoint");
                graph.release(client);
                assert_eq!(graph.take_retired(), Some(owner));
            }
            Err(rejected) => {
                assert!(first == u64::MAX || second == u64::MAX);
                kani::assert(rejected.value == owner && graph.is_empty() && graph.slots[0].generation == first && graph.slots[1].generation == second, "rejected imported promise preserves owner and storage");
            }
        }
        assert!(graph.is_empty());
    }).unwrap();
}

#[kani::proof]
#[kani::unwind(4)]
fn callable_proxy_exports_preserve_local_resolution_obligations() {
    let local: bool = kani::any();
    let called: bool = kani::any();
    let owner: u8 = kani::any();
    let mut slots = [const { Slot::EMPTY }; 2];
    with_graph::<u8, u8, _>(
        &mut slots,
        crate::clients::NotificationStorage::empty(),
        |graph| {
            let route = graph
                .endpoint(
                    owner,
                    if local {
                        Location::Local
                    } else {
                        Location::Remote(1)
                    },
                )
                .unwrap();
            let endpoint = route.key();
            let promise = graph.promise(&mut Some(route)).unwrap();
            if called {
                let call = graph.prepare_call(&promise).unwrap().accept();
                assert_eq!(call.key(), endpoint);
                graph.release(call);
            }
            let (snapshot, pending) = graph.snapshot_for_export(&promise).unwrap();
            kani::assert(
                pending == local,
                "callable local proxy must retain its resolution obligation",
            );
            kani::assert(
                snapshot.key() == if local { promise.key() } else { endpoint },
                "remote routes freeze while local observers follow resolution",
            );
            let mut replacement = Some(graph.retain_key(endpoint).unwrap());
            assert!(graph
                .resolve(promise.key(), &mut replacement, Source::Direct)
                .unwrap()
                .is_none());
            assert!(graph.is_settled(&snapshot).unwrap());
            let (settled, pending) = graph.snapshot_for_export(&snapshot).unwrap();
            assert!(!pending);
            assert_eq!(settled.key(), endpoint);
            graph.release(settled);
            graph.release(snapshot);
            graph.release(promise);
            assert_eq!(graph.take_retired(), Some(owner));
            assert!(graph.is_empty());
        },
    )
    .unwrap();
}

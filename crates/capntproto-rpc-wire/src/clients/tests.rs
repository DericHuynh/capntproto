use super::*;
use capntproto_rpc_engine::{
    clients::{Destination, Location, Source},
    ids::WireId,
};
#[test]
fn hosted_aliases_share_pending_resolution_and_preserve_the_import_until_retirement() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [imports::Slot::EMPTY];
        imports::with_imports(&mut slots, |imports| {
            let id = ImportId::from_wire(7);
            let mut receipts = [
                imports::ReceiveSlot::new_promise(id, ()),
                imports::ReceiveSlot::alias(id),
            ];
            imports.prepare_receive(&mut receipts).unwrap().commit();
            let first = receipts[0].take_local().unwrap();
            let second = receipts[1].take_local().unwrap();
            let mut nodes = [const { clients::Slot::EMPTY }; 3];
            clients::with_graph(
                &mut nodes,
                capntproto_rpc_engine::clients::NotificationStorage {
                    listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
                    subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
                },
                |graph: &mut clients::Graph<'_, '_, Endpoint<'_, u8, u32>, u8>| {
                    let mut slots = [ImportSlot::EMPTY];
                    let mut map = Imports::new(&mut slots, 1).unwrap();
                    let a = map.bind(graph, imports, &first).unwrap();
                    let b = map.bind(graph, imports, &second).unwrap();
                    assert_eq!(a.key(), b.key());
                    assert_eq!(map.resolution_target(first.key()), Some(a.key()));
                    imports.release(first);
                    imports.release(second);
                    assert!(imports.pending_release(0).is_none());
                    let call = graph.prepare_call(&a).unwrap().accept();
                    graph.release(call);
                    let replacement = graph
                        .endpoint(Endpoint::Local(42), Location::Local)
                        .unwrap();
                    let barrier = graph
                        .prepare_resolution(a.key(), &mut Some(replacement), Source::Reflected)
                        .unwrap()
                        .accept()
                        .unwrap();
                    assert!(matches!(
                        graph.prepare_call(&b).unwrap().destination(),
                        Destination::Waiting(_)
                    ));
                    graph.open(barrier);
                    assert!(matches!(
                        graph.prepare_call(&b).unwrap().destination(),
                        Destination::Endpoint {
                            owner: Endpoint::Local(42),
                            ..
                        }
                    ));
                    graph.release(a);
                    graph.release(b);
                    while let Some(owner) = graph.take_retired() {
                        owner.release(imports);
                    }
                    assert!(graph.is_empty());
                    let release = imports.pending_release(0).unwrap();
                    assert_eq!(release.count(), 2);
                    release.commit();
                    assert!(imports.is_empty());
                },
            )
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
#[test]
fn capacity_failure_preserves_receipt_and_creates_no_partial_promise() {
    let mut slots = [imports::Slot::EMPTY];
    imports::with_imports(&mut slots, |imports| {
        let id = ImportId::from_wire(7);
        let mut receipts = [imports::ReceiveSlot::new_promise(id, ())];
        imports.prepare_receive(&mut receipts).unwrap().commit();
        let local = receipts[0].take_local().unwrap();
        let mut nodes = [clients::Slot::EMPTY];
        clients::with_graph(
            &mut nodes,
            capntproto_rpc_engine::clients::NotificationStorage {
                listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
                subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
            },
            |graph: &mut clients::Graph<'_, '_, Endpoint<'_, u8, ()>, u8>| {
                let mut slots = [ImportSlot::EMPTY];
                let mut map = Imports::new(&mut slots, 1).unwrap();
                assert_eq!(
                    map.bind(graph, imports, &local).unwrap_err(),
                    Error::Graph(clients::Error::Full)
                );
                assert!(graph.is_empty());
                assert!(map.resolution_target(local.key()).is_none());
                assert!(imports.pending_release(0).is_none());
                imports.release(local);
                assert_eq!(imports.pending_release(0).unwrap().count(), 1);
                imports.pending_release(0).unwrap().commit();
            },
        )
        .unwrap();
    })
    .unwrap();
}

#[test]
fn reflected_client_outlives_both_peer_release_and_its_local_export_lease() {
    use capntproto_rpc_engine::exports;
    let mut nodes = [clients::Slot::EMPTY];
    clients::with_graph(
        &mut nodes,
        capntproto_rpc_engine::clients::NotificationStorage {
            listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
            subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
        },
        |graph: &mut clients::Graph<'_, '_, u32, u8>| {
            let local = graph.endpoint(41, Location::Local).unwrap();
            let owned = graph.retain(&local).unwrap();
            let mut slots = [exports::Slot::EMPTY];
            exports::with_exports(&mut slots, |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(owned).unwrap();
                batch.commit();
                let lease = exports.lease(grant.id()).unwrap();
                let retained = reflected(graph, exports, &lease).unwrap();
                assert_eq!(local.key(), retained.key());
                graph.release(local);
                assert!(exports.release(grant.id(), 1).unwrap().is_none());
                graph.release(exports.release_local(lease).unwrap());
                let call = graph.prepare_call(&retained).unwrap();
                assert!(matches!(
                    call.destination(),
                    Destination::Endpoint { owner: &41, .. }
                ));
                let running = call.accept();
                graph.release(retained);
                assert!(graph.take_retired().is_none());
                graph.release(running);
                assert_eq!(graph.take_retired(), Some(41));
                assert!(graph.is_empty());
            })
            .unwrap();
        },
    )
    .unwrap();
}

#[test]
fn binding_into_the_dispatcher_cannot_bypass_its_owned_task_queue() {
    use capntproto_rpc_engine::clients::dispatch::{self, TaskSlot};
    let mut imports_slots = [imports::Slot::EMPTY];
    imports::with_imports(&mut imports_slots, |imports| {
        let local = imports.admit(ImportId::from_wire(7), ()).unwrap();
        let mut clients = [clients::Slot::EMPTY];
        let mut tasks = [TaskSlot::EMPTY];
        dispatch::with_runtime(
            dispatch::Storage {
                notifications: capntproto_rpc_engine::clients::NotificationStorage {
                    listeners: &mut [const { clients::ListenerSlot::EMPTY }; 8],
                    subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 64],
                },
                clients: &mut clients,
                tasks: &mut tasks,
            },
            |runtime: &mut dispatch::Runtime<'_, '_, '_, Endpoint<'_, u8, ()>, u8, u32>| {
                let mut slots = [ImportSlot::EMPTY];
                let mut map = Imports::new(&mut slots, 1).unwrap();
                let client = map.bind(runtime, imports, &local).unwrap();
                imports.release(local);
                runtime
                    .prepare_call(&client, &mut Some(42))
                    .unwrap()
                    .accept();
                runtime.release(client);
                assert!(imports.pending_release(0).is_none());
                let ready = runtime.next_ready().unwrap().unwrap();
                let Destination::Endpoint {
                    owner: Endpoint::Imported(remote),
                    ..
                } = ready.destination()
                else {
                    unreachable!()
                };
                assert_eq!(remote.id(), ImportId::from_wire(7));
                let running = ready.accept();
                assert_eq!(runtime.complete(running), 42);
                runtime.take_retired().unwrap().release(imports);
                assert!(runtime.is_empty());
                assert_eq!(imports.pending_release(0).unwrap().count(), 1);
                imports.pending_release(0).unwrap().commit();
            },
        )
        .unwrap();
    })
    .unwrap();
}

use super::*;
use crate::clients::{self, Location};

#[test]
fn empty_and_absent_source_views_skip_owner_identification() {
    use clients::Capabilities;
    clients::with_graph(
        &mut [],
        clients::NotificationStorage {
            listeners: &mut [],
            subscriptions: &mut [],
        },
        |graph: &mut clients::Graph<'_, '_, u8, u8>| {
            let mut bare = clients::Access::new(&mut *graph);
            assert!(bare
                .retain_exported(|| panic!("an absent index cannot use an owner identity"))
                .unwrap()
                .is_none());
            with_exports(&mut [Slot::EMPTY], |exports| {
                let mut access = clients::Access::with_export_sources(graph, Some(&*exports));
                assert!(access
                    .retain_exported(|| panic!("an empty index cannot use an owner identity"))
                    .unwrap()
                    .is_none());
            })
            .unwrap();
        },
    )
    .unwrap();
}

#[test]
fn borrowed_sources_use_original_identity_and_reject_stale_graph_generations() {
    let counts = allocation_counter::measure(|| {
        clients::with_graph(
            &mut [const { clients::Slot::EMPTY }; 2],
            clients::NotificationStorage {
                listeners: &mut [],
                subscriptions: &mut [],
            },
            |graph: &mut clients::Graph<'_, '_, u8, u8>| {
                let source = graph.promise(&mut None).unwrap();
                let source_key = source.key();
                let independent = graph.promise(&mut None).unwrap();
                let independent_key = independent.key();
                with_exports(&mut [Slot::EMPTY], |exports| {
                    let identity = Identity::for_owner(1, 17);
                    let mut batch = exports.batch();
                    let grant = batch
                        .export_canonical_promise(identity, source_key, independent)
                        .unwrap();
                    batch.commit();
                    let mut access = clients::Access::with_export_sources(graph, Some(&*exports));
                    use clients::Capabilities;
                    let retained = access.retain_exported(|| Some(identity)).unwrap().unwrap();
                    assert_eq!(retained.key(), source_key);
                    assert_ne!(
                        retained.key(),
                        independent_key,
                        "the independent export forwarding route must not bypass original ordering"
                    );
                    access.release(retained);
                    graph.release(source);
                    let replacement = graph.endpoint(9, Location::Local).unwrap();
                    assert_ne!(replacement.key(), source_key);
                    let mut access = clients::Access::with_export_sources(graph, Some(&*exports));
                    assert!(
                        access.retain_exported(|| Some(identity)).unwrap().is_none(),
                        "a stale weak source must not retain the replacement generation"
                    );
                    graph.release(exports.release(grant.id(), 1).unwrap().unwrap());
                    graph.release(replacement);
                    assert_eq!(graph.take_retired(), Some(9));
                })
                .unwrap();
                assert!(graph.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn owner_namespaces_and_graph_generations_are_distinct() {
    with_exports(&mut [const { Slot::EMPTY }; 4], |exports| {
        let identities = [
            Identity::new(7, 1),
            Identity::new(7, 2),
            Identity::for_owner(1, 7),
            Identity::for_owner(2, 7),
        ];
        for (i, identity) in identities.iter().copied().enumerate() {
            let mut batch = exports.batch();
            let grant = batch.export(i).unwrap();
            batch.exports.insert_canonical(
                grant.key.index,
                identity,
                clients::Raw {
                    index: 0,
                    generation: 1,
                },
            );
            batch.commit();
        }
        for (i, identity) in identities.iter().copied().enumerate() {
            assert_eq!(exports.find_canonical(identity), Some(i));
            assert_eq!(
                exports.release(ExportId::from_wire(i as u32), 1),
                Ok(Some(i))
            );
        }
    })
    .unwrap();
}

#[test]
fn collisions_release_rollback_and_slot_reuse_do_not_allocate() {
    let counts = allocation_counter::measure(|| {
        for order in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 2, 0, 3]] {
            with_exports(&mut [const { Slot::EMPTY }; 4], |exports| {
                // Four full identities collide in the same fixed bucket.
                let mut candidate = 0;
                let identities = core::array::from_fn::<_, 4, _>(|_| loop {
                    let identity = Identity::new(candidate, 1);
                    candidate += 1;
                    if identity.bucket(4) == 0 {
                        break identity;
                    }
                });
                let mut keys = [None; 4];
                for (i, identity) in identities.iter().copied().enumerate() {
                    let mut batch = exports.batch();
                    let grant = batch.export(i).unwrap();
                    batch.exports.insert_canonical(
                        grant.key.index,
                        identity,
                        clients::Raw {
                            index: 0,
                            generation: 1,
                        },
                    );
                    keys[i] = Some(grant.key);
                    batch.commit();
                }
                for i in order {
                    assert_eq!(
                        exports.find_canonical(identities[i]),
                        Some(keys[i].unwrap().index)
                    );
                    assert_eq!(
                        exports.release(ExportId::from_wire(keys[i].unwrap().index as u32), 1),
                        Ok(Some(i))
                    );
                    keys[i] = None;
                    assert!(exports.find_canonical(identities[i]).is_none());
                    for j in 0..4 {
                        assert_eq!(
                            exports.find_canonical(identities[j]),
                            keys[j].map(|key| key.index)
                        );
                    }
                }
                assert_eq!(exports.canonical_count, 0);
                for forgotten in [false, true] {
                    let mut batch = exports.batch();
                    let grant = batch.export(7).unwrap();
                    batch.exports.insert_canonical(
                        grant.key.index,
                        identities[0],
                        clients::Raw {
                            index: 0,
                            generation: 1,
                        },
                    );
                    if forgotten {
                        core::mem::forget(batch);
                    } else {
                        drop(batch);
                    }
                    assert_eq!(exports.pop_retired(), Some(7));
                    assert_eq!(exports.canonical_count, 0);
                    assert!(exports.find_canonical(identities[0]).is_none());
                }
            })
            .unwrap();
        }
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn canonical_grants_share_counts_and_final_release_invalidates_before_local_leases() {
    let counts = allocation_counter::measure(|| {
        clients::with_graph(
            &mut [const { clients::Slot::EMPTY }; 2],
            clients::NotificationStorage {
                listeners: &mut [],
                subscriptions: &mut [],
            },
            |graph: &mut clients::Graph<'_, '_, u8, u8>| {
                with_exports(&mut [Slot::EMPTY], |exports| {
                    let original = graph.endpoint(7, Location::Local).unwrap();
                    let original_key = original.key();
                    let mut batch = exports.batch();
                    assert!(batch.grant_canonical(original_key).unwrap().is_none());
                    let first = batch
                        .export_canonical(original_key, graph.retain(&original).unwrap())
                        .unwrap();
                    assert_eq!(
                        batch.grant_canonical(original_key).unwrap().unwrap().id(),
                        first.id()
                    );
                    batch.commit();
                    let mut aborted = exports.batch();
                    assert_eq!(
                        aborted.grant_canonical(original_key).unwrap().unwrap().id(),
                        first.id()
                    );
                    drop(aborted);
                    assert_eq!(
                        exports.release(first.id(), 3).unwrap_err(),
                        Error::OverRelease
                    );
                    let lease = exports.lease(first.id()).unwrap();
                    assert!(exports.release(first.id(), 2).unwrap().is_none());
                    assert!(exports
                        .batch()
                        .grant_canonical(original_key)
                        .unwrap()
                        .is_none());
                    graph.release(exports.release_local(lease).unwrap());
                    graph.release(original);
                    assert_eq!(graph.take_retired(), Some(7));
                    let replacement = graph.endpoint(9, Location::Local).unwrap();
                    assert_ne!(replacement.key(), original_key);
                    let mut batch = exports.batch();
                    assert!(batch.grant_canonical(original_key).unwrap().is_none());
                    let next = batch
                        .export_canonical(replacement.key(), graph.retain(&replacement).unwrap())
                        .unwrap();
                    assert_eq!(next.id(), first.id());
                    assert_ne!(next.key(), first.key());
                    batch.commit();
                    graph.release(exports.release(next.id(), 1).unwrap().unwrap());
                    graph.release(replacement);
                    assert_eq!(graph.take_retired(), Some(9));
                })
                .unwrap();
                assert!(graph.is_empty());
            },
        )
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_slots_rehash_and_terminal_cleanup_preserves_every_bucket() {
    with_growing_exports(0, 64, |exports| {
        let mut keys = [None; 64];
        for i in 0..keys.len() {
            let mut batch = exports.batch();
            let grant = batch.export(i).unwrap();
            batch.exports.insert_canonical(
                grant.key.index,
                Identity::new(i * 4, 1),
                clients::Raw {
                    index: 0,
                    generation: 1,
                },
            );
            keys[i] = Some(grant.key);
            batch.commit();
            for (j, previous) in keys[..=i].iter().enumerate() {
                assert_eq!(
                    exports.find_canonical(Identity::new(j * 4, 1)),
                    Some(previous.unwrap().index)
                );
            }
        }
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(exports.take_terminal_owner(key.unwrap().index), Some(i));
            assert!(exports.find_canonical(Identity::new(i * 4, 1)).is_none());
        }
        assert_eq!(exports.canonical_count, 0);
    })
    .unwrap();
}

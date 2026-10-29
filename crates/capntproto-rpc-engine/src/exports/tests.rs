use super::*;
use core::cell::Cell;

#[test]
fn retirement_queue_observes_deferred_retirement_and_forgotten_batches() {
    let mut slots = [const { Slot::EMPTY }; 1];
    with_exports(&mut slots, |exports| {
        assert!(exports.retirement_is_idle());
        assert!(exports.retirement_is_idle());
        let mut batch = exports.batch();
        let grant = batch.export(31).unwrap();
        batch.commit();
        assert!(exports.retirement_is_idle());
        let lease = exports.lease(grant.id()).unwrap();
        assert_eq!(exports.release(grant.id(), 1), Ok(None));
        assert!(exports.retirement_is_idle());
        exports.release_local_deferred(lease);
        assert!(!exports.retirement_is_idle());
        assert_eq!(exports.take_retired(0), Some(31));
        assert!(exports.retirement_is_idle());

        let mut batch = exports.batch();
        let _ = batch.export(37).unwrap();
        core::mem::forget(batch);
        assert!(!exports.retirement_is_idle());
        assert_eq!(exports.take_retired(0), Some(37));
        assert!(exports.retirement_is_idle());

        // An unpublished grant on an existing export also needs rollback,
        // even though there is no retired owner to return.
        let mut batch = exports.batch();
        let grant = batch.export(41).unwrap();
        batch.commit();
        let mut batch = exports.batch();
        let _ = batch.grant(grant.key()).unwrap();
        core::mem::forget(batch);
        assert!(!exports.retirement_is_idle());
        assert_eq!(exports.take_retired(0), None);
        assert!(exports.retirement_is_idle());
        assert_eq!(exports.release(grant.id(), 1), Ok(Some(41)));

        let mut batch = exports.batch();
        let _ = batch.export(43).unwrap();
        drop(batch);
        assert!(!exports.retirement_is_idle());
        assert_eq!(exports.take_retired(0), Some(43));
        assert!(exports.retirement_is_idle());
    })
    .unwrap();
}

#[test]
fn committed_grants_match_peer_releases_and_local_leases_keep_owners_alive() {
    let mut slots = [const { Slot::EMPTY }; 2];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(31).unwrap();
        let key = grant.key();
        let id = grant.id();
        assert_eq!(batch.grant(key).unwrap().id(), id);
        batch.commit();
        assert_eq!(exports.lookup(id), Some(key));
        let lease = exports.lease(id).unwrap();
        let alias = exports.retain(&lease).unwrap();
        assert_eq!(exports.release(id, 3), Err(Error::OverRelease));
        assert_eq!(exports.release(id, 1), Ok(None));
        assert_eq!(exports.resolve(key), Some(&31));
        assert_eq!(exports.release(id, 1), Ok(None));
        assert_eq!(exports.lookup(id), None);
        assert!(exports.resolve(key).is_none());
        assert_eq!(exports.release(id, 0), Err(Error::Missing));
        assert_eq!(exports.lease(id).unwrap_err(), Error::Missing);
        assert!(exports.take_retired(0).is_none());
        assert_eq!(exports.get(&lease), &31);
        assert_eq!(exports.release_local(lease), None);
        let extra = exports.retain(&alias).unwrap();
        assert_eq!(exports.release_local(alias), None);
        assert_eq!(exports.release_local(extra), Some(31));
        let mut batch = exports.batch();
        let replacement = batch.export(37).unwrap();
        assert_eq!(replacement.id(), id);
        assert_eq!(batch.grant(key).unwrap_err(), Error::StaleKey);
        batch.commit();
        assert!(exports.resolve(key).is_none());
        assert_eq!(exports.release(id, 1), Ok(Some(37)));
        assert!(exports.is_empty());
    })
    .unwrap();
}

#[test]
fn failed_batches_rollback_new_and_repeated_grants_at_each_position() {
    for fail_at in 0..=4 {
        let mut slots = [const { Slot::EMPTY }; 3];
        with_exports(&mut slots, |exports| {
            let mut batch = exports.batch();
            let first = batch.export(11).unwrap();
            let saturated = batch.export(13).unwrap();
            batch.commit();
            let State::Live(refs) = &mut exports.slots[1].entry.as_mut().unwrap().state else {
                unreachable!()
            };
            refs.retain(u32::MAX - 1).unwrap();
            let mut new_key = None;
            {
                let mut batch = exports.batch();
                for step in 0..=4 {
                    if step == fail_at {
                        assert_eq!(
                            batch.grant(saturated.key()).unwrap_err(),
                            Error::RemoteReferencesExhausted
                        );
                        break;
                    }
                    match step {
                        0 | 2 => {
                            let _ = batch.grant(first.key()).unwrap();
                        }
                        1 => {
                            new_key = Some(batch.export(17).unwrap().key());
                        }
                        3 => {
                            let _ = batch.grant(new_key.unwrap()).unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
            }
            assert_eq!(exports.release(first.id(), 1), Ok(Some(11)));
            assert_eq!(exports.release(saturated.id(), u32::MAX), Ok(Some(13)));
            if let Some(key) = new_key {
                assert!(exports.resolve(key).is_none());
                assert_eq!(exports.lookup(ExportId::from_wire(2)), None);
                assert_eq!(exports.take_retired(2), Some(17));
            }
            assert!(exports.is_empty());
        })
        .unwrap();
    }
}

#[test]
fn dropped_or_forgotten_batch_retains_owners_until_explicit_draining() {
    let drops = Cell::new(0);
    let mut slots = [const { Slot::EMPTY }; 1];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(Owner(&drops)).ok().unwrap();
        let id = grant.id();
        let key = grant.key();
        core::mem::forget(batch);
        assert_eq!(exports.lookup(id), None);
        assert!(exports.resolve(key).is_none());
        assert_eq!(drops.get(), 0);
        assert_eq!(exports.lease(id).unwrap_err(), Error::Missing);
        let rejected = exports.batch().export(Owner(&drops)).err().unwrap();
        assert_eq!(rejected.error, Error::Full);
        assert_eq!(drops.get(), 0);
        drop(rejected.value);
        let owner = exports.take_retired(0).unwrap();
        assert_eq!(drops.get(), 1);
        assert!(exports.is_empty());
        drop(owner);
        assert_eq!(drops.get(), 2);
    })
    .unwrap();
}

#[test]
fn forgotten_existing_grants_never_become_published() {
    let mut slots = [const { Slot::EMPTY }; 1];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(19).unwrap();
        batch.commit();
        let mut batch = exports.batch();
        let _ = batch.grant(grant.key()).unwrap();
        core::mem::forget(batch);
        assert_eq!(exports.release(grant.id(), 2), Err(Error::OverRelease));
        assert_eq!(exports.release(grant.id(), 1), Ok(Some(19)));
    })
    .unwrap();
}

#[test]
fn generation_retirement_and_capacity_rejection_preserve_ownership() {
    let mut empty = [];
    with_exports(&mut empty, |exports| {
        let rejected = exports.batch().export(19).unwrap_err();
        assert_eq!((rejected.error, rejected.value), (Error::Full, 19));
        assert!(exports.take_retired(usize::MAX).is_none());
        assert_eq!(
            exports.release(ExportId::from_wire(u32::MAX), 1),
            Err(Error::Missing)
        );
    })
    .unwrap();
    let mut slots = [const { Slot::EMPTY }; 2];
    slots[0].generation = u64::MAX - 1;
    slots[1].generation = u64::MAX;
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(23).unwrap();
        batch.commit();
        assert_eq!(exports.release(grant.id(), 1), Ok(Some(23)));
        let rejected = exports.batch().export(29).unwrap_err();
        assert_eq!(
            (rejected.error, rejected.value),
            (Error::GenerationExhausted, 29)
        );
        assert_eq!(exports.resolve(grant.key()), None);
    })
    .unwrap();
    assert_eq!(slots[0].generation, u64::MAX);
}

#[test]
fn count_exhaustion_checks_published_plus_pending_and_local_leases() {
    let mut slots = [const { Slot::EMPTY }; 1];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(31).unwrap();
        batch.commit();
        let State::Live(refs) = &mut exports.slots[0].entry.as_mut().unwrap().state else {
            unreachable!()
        };
        refs.retain(u32::MAX - 2).unwrap();
        let mut batch = exports.batch();
        let _ = batch.grant(grant.key()).unwrap();
        assert_eq!(
            batch.grant(grant.key()).unwrap_err(),
            Error::RemoteReferencesExhausted
        );
        batch.commit();
        let lease = exports.lease(grant.id()).unwrap();
        exports.slots[0].entry.as_mut().unwrap().local = u32::MAX;
        assert_eq!(
            exports.lease(grant.id()).unwrap_err(),
            Error::LocalReferencesExhausted
        );
        assert_eq!(
            exports.retain(&lease).unwrap_err(),
            Error::LocalReferencesExhausted
        );
        assert_eq!(exports.slots[0].entry.as_ref().unwrap().local, u32::MAX);
        exports.slots[0].entry.as_mut().unwrap().local = 1;
        assert_eq!(exports.release(grant.id(), u32::MAX), Ok(None));
        assert_eq!(exports.release_local(lease), Some(31));
    })
    .unwrap();
}

struct Owner<'a>(&'a Cell<u32>);
impl Drop for Owner<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn nonempty_storage_rejection_and_terminal_unwind_preserve_ownership() {
    extern crate std;
    let drops = Cell::new(0);
    let mut slots = [Slot {
        free: crate::free_slots::Link::EMPTY,
        retirement: crate::work_queue::Link::EMPTY,
        publication: crate::work_queue::Link::EMPTY,
        canonical: None,
        bucket: None,
        generation: 9,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            value: Owner(&drops),
            state: State::Retired,
            promise: promise::ImportState::Hosted,
            local: 0,
            pending: 0,
        }),
    }];
    assert_eq!(
        with_exports(&mut slots, |_| unreachable!()),
        Err::<(), _>(Error::NonemptyStorage)
    );
    assert_eq!(drops.get(), 0);
    assert_eq!(slots[0].generation, 9);
    drop(slots[0].entry.take());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = with_exports(&mut slots, |exports| {
            let mut batch = exports.batch();
            let descriptor = batch.export(Owner(&drops)).ok().unwrap();
            batch.commit();
            let _lease = exports.lease(descriptor.id()).unwrap();
            panic!("injected connection failure");
        });
    }));
    assert!(result.is_err());
    assert_eq!(drops.get(), 2);
    with_exports(&mut slots, |exports| assert!(exports.is_empty())).unwrap();
}

#[test]
fn caller_unwind_aborts_publication_without_dropping_owners_in_the_batch() {
    extern crate std;
    let drops = Cell::new(0);
    let mut slots = [const { Slot::EMPTY }; 2];
    with_exports(&mut slots, |exports| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut batch = exports.batch();
            let _ = batch.export(Owner(&drops)).ok().unwrap();
            panic!("injected serialization failure");
        }));
        assert!(result.is_err());
        assert_eq!(drops.get(), 0);
        drop(exports.take_retired(0).unwrap());
        assert_eq!(drops.get(), 1);
        let mut batch = exports.batch();
        let grant = batch.export(Owner(&drops)).ok().unwrap();
        batch.commit();
        let _retained = exports.lease(grant.id()).unwrap();
    })
    .unwrap();
    assert_eq!(drops.get(), 2);
    with_exports(&mut slots, |exports| assert!(exports.is_empty())).unwrap();
}

#[test]
fn publication_reuse_and_cleanup_do_not_allocate() {
    let counts = allocation_counter::measure(|| {
        let mut slots = [const { Slot::EMPTY }; 3];
        with_exports(&mut slots, |exports| {
            let mut stale = None;
            for i in 0..256 {
                let mut batch = exports.batch();
                let first = batch.export(i).unwrap();
                let second = batch.export(i + 1).unwrap();
                let _ = batch.grant(first.key()).unwrap();
                batch.commit();
                if let Some(stale) = stale {
                    assert_eq!(exports.resolve(stale), None);
                }
                stale = Some(first.key());
                let lease = exports.lease(first.id()).unwrap();
                assert_eq!(exports.release(second.id(), 1), Ok(Some(i + 1)));
                assert_eq!(exports.get(&lease), &i);
                assert_eq!(exports.release(first.id(), 2), Ok(None));
                assert_eq!(exports.release_local(lease), Some(i));
            }
        })
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_export_rollback_retains_owners_until_explicit_cleanup() {
    with_growing_exports(0, 3, |exports| {
        let mut batch = exports.batch();
        let live = batch.export(11).unwrap();
        batch.commit();
        let lease = exports.lease(live.id()).unwrap();
        let (second, third) = {
            let mut batch = exports.batch();
            let second = batch.export(13).unwrap();
            let third = batch.export(17).unwrap();
            let rejected = batch.export(19).unwrap_err();
            assert_eq!((rejected.error, rejected.value), (Error::Full, 19));
            (second, third)
        };
        assert!(exports.resolve(second.key()).is_none());
        assert!(exports.resolve(third.key()).is_none());
        assert_eq!(exports.resolve(live.key()), Some(&11));
        assert_eq!(exports.take_retired(1), Some(13));
        assert_eq!(exports.take_retired(2), Some(17));
        let mut batch = exports.batch();
        let reused = batch.export(23).unwrap();
        batch.commit();
        assert_eq!(reused.id(), second.id());
        assert!(exports.resolve(second.key()).is_none());
        assert_eq!(exports.release(reused.id(), 1), Ok(Some(23)));
        assert_eq!(exports.release(live.id(), 1), Ok(None));
        assert_eq!(exports.get(&lease), &11);
        assert_eq!(exports.release_local(lease), Some(11));
        assert!(exports.is_empty());
    })
    .unwrap();
}

#[test]
fn deferred_peer_release_checks_before_mutation_and_preserves_local_owners() {
    use core::cell::Cell;
    struct Owner<'a>(&'a Cell<usize>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let dropped = Cell::new(0);
    with_exports(&mut [Slot::EMPTY], |exports| {
        let mut batch = exports.batch();
        let grant = batch
            .export(Owner(&dropped))
            .unwrap_or_else(|_| panic!("export"));
        let _ = batch.grant(grant.key()).unwrap();
        batch.commit();
        let lease = exports.lease(grant.id()).unwrap();
        assert_eq!(
            exports.release_deferred(grant.id(), 3),
            Err(Error::OverRelease)
        );
        assert!(exports.lookup(grant.id()).is_some());
        assert_eq!(exports.release_deferred(grant.id(), 2), Ok(()));
        assert!(exports.lookup(grant.id()).is_none());
        assert!(exports.take_retired(0).is_none());
        assert_eq!(dropped.get(), 0);
        exports.release_local_deferred(lease);
        assert_eq!(dropped.get(), 0);
        drop(
            exports
                .take_retired(0)
                .expect("retired after final local owner"),
        );
        assert_eq!(dropped.get(), 1);
        assert!(exports.is_empty());
    })
    .unwrap();
}

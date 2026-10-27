use super::*;

#[test]
fn cleanup_presence_tracks_revival_rejection_and_terminal_detachment() {
    let mut storage = [Slot::EMPTY];
    with_imports(&mut storage, |imports| {
        let id = ImportId::from_wire(7);
        let local = imports.admit(id, 17u8).unwrap();
        assert!(!imports.has_pending_release());
        imports.release(local);
        assert!(imports.has_pending_release());
        let mut aliases = [ReceiveSlot::alias(id), ReceiveSlot::alias(id)];
        drop(imports.prepare_receive(&mut aliases).unwrap());
        assert!(imports.has_pending_release());
        imports.prepare_receive(&mut aliases).unwrap().commit();
        assert!(!imports.has_pending_release());
        imports.release(aliases[0].take_local().unwrap());
        assert!(!imports.has_pending_release());
        imports.release(aliases[1].take_local().unwrap());
        drop(imports.pending_release(0).unwrap());
        assert!(imports.has_pending_release());
        assert_eq!(imports.pending_release(0).unwrap().commit(), 17);
        assert!(!imports.has_pending_release());
        let local = imports.admit(id, 19).unwrap();
        imports.release(local);
        assert_eq!(imports.take_terminal_owner(0), Some(19));
        assert!(!imports.has_pending_release());
    })
    .unwrap();
}
use crate::ids::WireId;
use core::cell::Cell;

#[test]
fn indexed_import_lookup_preserves_dense_sparse_displaced_and_pending_release_ids() {
    for ids in [[0, 1, 2, u32::MAX], [2, 0, 1, u32::MAX]] {
        let mut slots = [const { Slot::EMPTY }; 4];
        with_imports(&mut slots, |imports| {
            let owners = ids.map(|id| imports.admit(ImportId::from_wire(id), id).unwrap());
            let allocations = allocation_counter::measure(|| {
                for id in ids {
                    let alias = imports.receive(ImportId::from_wire(id)).unwrap();
                    assert_eq!(imports.get(&alias), &id);
                    imports.release(alias);
                }
                assert!(!imports.contains(ImportId::from_wire(3)));
            });
            assert_eq!(allocations.count_total, 0);
            assert_eq!(allocations.bytes_total, 0);
            for owner in owners {
                imports.release(owner);
            }
            // Releasing the local owner does not invalidate an ID until its
            // actual wire Release is accepted, including the indexed ID zero.
            for id in ids {
                assert!(imports.contains(ImportId::from_wire(id)));
            }
            while let Some(release) = imports.next_release() {
                release.commit();
            }
            assert!(imports.is_empty());
        })
        .unwrap();
    }
}

#[test]
fn local_aliases_wire_grants_and_rejected_cleanup_have_distinct_lifetimes() {
    let mut storage = [const { Slot::EMPTY }; 2];
    with_imports(&mut storage, |imports| {
        let id = ImportId::from_wire(u32::MAX);
        let first = imports.admit(id, 19).unwrap();
        let key = first.key();
        let clone = imports.retain(&first).unwrap();
        let received = imports.receive(id).unwrap();
        assert_eq!(imports.get(&received), &19);
        imports.release(first);
        imports.release(clone);
        assert_eq!(
            imports.binding_revision(),
            0,
            "temporary aliases do not wake blocked bindings"
        );
        assert!(imports.pending_release(0).is_none());
        assert_eq!(imports.resolve(key), Some(&19));
        imports.release(received);
        assert_eq!(imports.resolve(key), None);
        for _ in 0..4 {
            let cleanup = imports.pending_release(0).unwrap();
            assert_eq!(cleanup.id(), id);
            assert_eq!(cleanup.count(), 2); // Local cloning grants nothing on the wire.
        }
        let revision = imports.binding_revision();
        assert!(revision > 0);
        let mut receipts = [ReceiveSlot::alias(id)];
        drop(imports.prepare_receive(&mut receipts).unwrap());
        assert_eq!(
            imports.binding_revision(),
            revision,
            "rejected descriptor admission cannot create a retry cycle"
        );
        let revived = imports.receive(id).unwrap();
        assert!(imports.binding_revision() > revision);
        assert_ne!(revived.key(), key);
        assert_eq!(imports.resolve(key), None);
        assert_eq!(imports.get(&revived), &19);
        imports.release(revived);
        let cleanup = imports.pending_release(0).unwrap();
        assert_eq!(cleanup.count(), 3);
        assert_eq!(cleanup.commit(), 19);
        assert!(imports.is_empty());
        let reused = imports.admit(id, 23).unwrap();
        assert_ne!(reused.key(), key);
        assert_eq!(imports.resolve(key), None);
        imports.release(reused);
        assert_eq!(imports.pending_release(0).unwrap().commit(), 23);
    })
    .unwrap();
}

#[test]
fn cleanup_reserves_capacity_and_failed_admission_returns_ownership() {
    let mut empty = [];
    with_imports(&mut empty, |imports| {
        assert_eq!(
            imports.admit(ImportId::from_wire(0), 7).unwrap_err().error,
            Error::Full
        );
        assert!(imports.pending_release(0).is_none());
    })
    .unwrap();
    let mut storage = [const { Slot::EMPTY }; 1];
    with_imports(&mut storage, |imports| {
        let id = ImportId::from_wire(0);
        let local = imports.admit(id, 31).unwrap();
        imports.release(local);
        let duplicate = imports.admit(id, 41).unwrap_err();
        assert_eq!((duplicate.error, duplicate.value), (Error::Duplicate, 41));
        let full = imports.admit(ImportId::from_wire(1), 43).unwrap_err();
        assert_eq!((full.error, full.value), (Error::Full, 43));
        assert_eq!(
            imports.receive(ImportId::from_wire(1)).unwrap_err(),
            Error::Missing
        );
        assert_eq!(imports.len(), 1);
        assert_eq!(imports.pending_release(0).unwrap().commit(), 31);
        assert!(imports.is_empty());
    })
    .unwrap();
}

#[test]
fn generation_exhaustion_retires_slots_without_losing_pending_cleanup() {
    let mut storage = [const { Slot::EMPTY }; 2];
    storage[0].generation = u64::MAX - 1;
    storage[1].generation = u64::MAX;
    with_imports(&mut storage, |imports| {
        let id = ImportId::from_wire(9);
        let local = imports.admit(id, 17).unwrap();
        let key = local.key();
        // Existing aliases remain valid at the maximum generation.
        let alias = imports.receive(id).unwrap();
        imports.release(local);
        imports.release(alias);
        assert_eq!(imports.receive(id).unwrap_err(), Error::GenerationExhausted);
        assert_eq!(imports.resolve(key), None);
        let cleanup = imports.pending_release(0).unwrap();
        assert_eq!(cleanup.count(), 2);
        assert_eq!(cleanup.commit(), 17);
        let rejected = imports.admit(id, 29).unwrap_err();
        assert_eq!(
            (rejected.error, rejected.value),
            (Error::GenerationExhausted, 29)
        );
    })
    .unwrap();
    assert_eq!(storage[0].generation, u64::MAX);
}

#[test]
fn local_and_remote_overflow_preserve_the_entire_entry() {
    let mut storage = [const { Slot::EMPTY }; 1];
    with_imports(&mut storage, |imports| {
        let id = ImportId::from_wire(3);
        let local = imports.admit(id, 13).unwrap();
        imports.slots[0].entry.as_mut().unwrap().local = u32::MAX;
        assert_eq!(
            imports.retain(&local).unwrap_err(),
            Error::LocalReferencesExhausted
        );
        assert_eq!(
            imports.receive(id).unwrap_err(),
            Error::LocalReferencesExhausted
        );
        let entry = imports.slots[0].entry.as_mut().unwrap();
        assert_eq!(entry.remote.count(), 1);
        entry.local = 1;
        entry.remote.receive(u32::MAX - 1).unwrap();
        assert_eq!(
            imports.receive(id).unwrap_err(),
            Error::RemoteReferencesExhausted
        );
        assert_eq!(imports.get(&local), &13);
        imports.release(local);
        let generation = imports.slots[0].generation;
        assert_eq!(
            imports.receive(id).unwrap_err(),
            Error::RemoteReferencesExhausted
        );
        assert_eq!(imports.slots[0].generation, generation);
        let cleanup = imports.pending_release(0).unwrap();
        assert_eq!(cleanup.count(), u32::MAX);
        assert_eq!(cleanup.commit(), 13);
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
fn cleanup_and_reuse_preserve_other_live_slots() {
    let mut storage = [const { Slot::EMPTY }; 3];
    with_imports(&mut storage, |imports| {
        let first = imports.admit(ImportId::from_wire(u32::MAX), 11).unwrap();
        let second = imports.admit(ImportId::from_wire(0), 13).unwrap();
        let third = imports.admit(ImportId::from_wire(7), 17).unwrap();
        let stale = second.key();
        imports.release(second);
        assert!(imports.pending_release(0).is_none());
        assert!(imports.pending_release(2).is_none());
        assert!(imports.pending_release(usize::MAX).is_none());
        assert_eq!(imports.pending_release(1).unwrap().commit(), 13);
        let replacement = imports.admit(ImportId::from_wire(9), 19).unwrap();
        assert!(imports.resolve(stale).is_none());
        assert_eq!(imports.get(&first), &11);
        assert_eq!(imports.get(&third), &17);
        assert_eq!(imports.get(&replacement), &19);
        imports.release(third);
        imports.release(first);
        imports.release(replacement);
        for (index, value) in [11, 19, 17].into_iter().enumerate() {
            assert_eq!(imports.pending_release(index).unwrap().commit(), value);
        }
        assert!(imports.is_empty());
    })
    .unwrap();
}

#[test]
fn terminal_unwind_drops_retained_owners_and_allows_storage_reuse() {
    extern crate std;
    let drops = Cell::new(0);
    let mut storage = [const { Slot::EMPTY }; 2];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = with_imports(&mut storage, |imports| {
            let local = imports
                .admit(ImportId::from_wire(1), Owner(&drops))
                .ok()
                .unwrap();
            let _retained = imports
                .admit(ImportId::from_wire(2), Owner(&drops))
                .ok()
                .unwrap();
            imports.release(local);
            panic!("injected driver failure");
        });
    }));
    assert!(result.is_err());
    assert_eq!(drops.get(), 2);
    with_imports(&mut storage, |imports| assert!(imports.is_empty())).unwrap();
}

#[test]
fn owners_drop_after_detachment_or_terminal_scope_teardown() {
    let drops = Cell::new(0);
    let mut storage = [const { Slot::EMPTY }; 2];
    let detached = with_imports(&mut storage, |imports| {
        let local = imports
            .admit(ImportId::from_wire(1), Owner(&drops))
            .ok()
            .unwrap();
        imports.release(local);
        let detached = imports.pending_release(0).unwrap().commit();
        assert_eq!(drops.get(), 0);
        let forgotten = imports
            .admit(ImportId::from_wire(2), Owner(&drops))
            .ok()
            .unwrap();
        let _forgotten = forgotten;
        assert_eq!(imports.len(), 1);
        detached
    })
    .unwrap();
    assert_eq!(drops.get(), 1);
    drop(detached);
    assert_eq!(drops.get(), 2);
    with_imports(&mut storage, |imports| assert!(imports.is_empty())).unwrap();
}

#[test]
fn stale_keys_never_follow_reused_slots_and_operations_do_not_allocate() {
    let counts = allocation_counter::measure(|| {
        let mut storage = [const { Slot::EMPTY }; 3];
        with_imports(&mut storage, |imports| {
            let id = ImportId::from_wire(u32::MAX);
            let first = imports.admit(id, 0).unwrap();
            let stale = first.key();
            imports.release(first);
            assert_eq!(imports.pending_release(0).unwrap().commit(), 0);
            for value in 1..256 {
                let local = imports.admit(id, value).unwrap();
                assert_eq!(imports.resolve(stale), None);
                let alias = imports.retain(&local).unwrap();
                imports.release(local);
                assert_eq!(imports.get(&alias), &value);
                imports.release(alias);
                assert_eq!(imports.pending_release(0).unwrap().commit(), value);
            }
        })
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn nonempty_backing_is_rejected_without_dropping_its_owner() {
    let drops = Cell::new(0);
    let mut remote = ImportRefs::new();
    remote.receive(1).unwrap();
    let mut storage = [Slot {
        release_previous: None,
        release_next: None,
        generation: 7,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            wire: ImportId::from_wire(1),
            value: Owner(&drops),
            local: 1,
            remote,
            promise: promise::ImportState::Hosted,
        }),
    }];
    assert_eq!(
        with_imports(&mut storage, |_| unreachable!()),
        Err::<(), _>(Error::NonemptyStorage)
    );
    assert_eq!(drops.get(), 0);
    assert_eq!(storage[0].generation, 7);
    drop(storage);
    assert_eq!(drops.get(), 1);
}

#[cfg(feature = "alloc")]
#[test]
fn growing_import_batch_rollback_preserves_owners_and_generations() {
    with_growing_imports(0, 3, |imports| {
        let live = imports.admit(ImportId::from_wire(9), 19).unwrap();
        let stale = live.key();
        let mut receipts = [
            ReceiveSlot::alias(ImportId::from_wire(9)),
            ReceiveSlot::new(ImportId::from_wire(10), 23),
            ReceiveSlot::new(ImportId::from_wire(11), 29),
            ReceiveSlot::new(ImportId::from_wire(12), 31),
        ];
        assert!(matches!(
            imports.prepare_receive(&mut receipts),
            Err(ReceiveError::Import(Error::Full))
        ));
        assert_eq!(imports.len(), 1);
        assert_eq!(imports.get(&live), &19);
        for (receipt, owner) in receipts[1..].iter().zip([23, 29, 31]) {
            assert_eq!(receipt.owner(), Some(&owner));
            assert!(receipt.local().is_none());
        }
        drop(imports.prepare_receive(&mut receipts[..3]).unwrap());
        assert_eq!(imports.len(), 1);
        imports
            .prepare_receive(&mut receipts[..3])
            .unwrap()
            .commit();
        assert_eq!(imports.len(), 3);
        for receipt in &mut receipts[..3] {
            imports.release(receipt.take_local().unwrap());
        }
        imports.release(live);
        assert!(imports.resolve(stale).is_none());
        assert_eq!(imports.pending_release(0).unwrap().count(), 2);
        for (index, owner) in [19, 23, 29].into_iter().enumerate() {
            assert_eq!(imports.pending_release(index).unwrap().commit(), owner);
        }
        let new = imports.admit(ImportId::from_wire(9), 37).unwrap();
        assert_ne!(new.key(), stale);
        assert!(imports.resolve(stale).is_none());
        imports.release(new);
        assert_eq!(imports.pending_release(0).unwrap().commit(), 37);
    })
    .unwrap();
}

#[test]
fn release_queue_preserves_rejection_and_unlinks_revived_head_middle_and_tail() {
    let mut slots = [const { Slot::EMPTY }; 64];
    with_imports(&mut slots, |imports| {
        let mut owners: [Option<_>; 64] = core::array::from_fn(|i| {
            Some(imports.admit(ImportId::from_wire(i as u32), i).unwrap())
        });
        for i in [63, 17, 1, 42] {
            imports.release(owners[i].take().unwrap());
        }
        assert_eq!(
            imports.next_release().unwrap().id(),
            ImportId::from_wire(63)
        );
        // Revive the head, an interior item, and the tail without disturbing 17.
        for i in [63, 1, 42] {
            owners[i] = Some(imports.receive(ImportId::from_wire(i as u32)).unwrap());
        }
        assert_eq!(imports.next_release().unwrap().commit(), 17);
        assert!(!imports.has_pending_release());
        for i in [42, 1, 63] {
            imports.release(owners[i].take().unwrap());
        }
        for i in [42, 1, 63] {
            let pending = imports.next_release().unwrap();
            assert_eq!(pending.count(), 2);
            assert_eq!(pending.commit(), i);
        }
        for owner in owners.into_iter().flatten() {
            imports.release(owner);
        }
        while let Some(pending) = imports.next_release() {
            pending.commit();
        }
        assert!(imports.is_empty());
    })
    .unwrap();
    // Borrowed storage can be reused after terminal teardown with queued work.
    with_imports(&mut slots, |imports| {
        let owner = imports.admit(ImportId::from_wire(8), 8).unwrap();
        imports.release(owner);
    })
    .unwrap();
    with_imports(&mut slots, |imports| {
        let owner = imports.admit(ImportId::from_wire(9), 9).unwrap();
        imports.release(owner);
        assert_eq!(imports.next_release().unwrap().commit(), 9);
    })
    .unwrap();
}

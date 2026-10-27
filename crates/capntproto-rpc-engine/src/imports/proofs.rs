use super::*;
use crate::ids::WireId;

/// One slot, arbitrary full-width initial generation and wire ID. Native traces
/// and compiler contracts separately cover multiple slots and domain separation.
#[kani::proof]
#[kani::unwind(3)]
fn import_generation_and_cleanup_lifecycle() {
    let generation: u64 = kani::any();
    let wire = ImportId::from_wire(kani::any());
    let mut storage = [Slot {
        free: crate::free_slots::Link::EMPTY,
        release_previous: None,
        release_next: None,
        generation,
        entry: None,
    }];
    with_imports(&mut storage, |imports| {
        let result = imports.admit(wire, 17u8);
        if generation == u64::MAX {
            let rejected = result.unwrap_err();
            assert_eq!(rejected.error, Error::GenerationExhausted);
            assert_eq!(rejected.value, 17);
            assert!(imports.is_empty());
            return;
        }
        let local = result.unwrap();
        let stale = local.key();
        let alias = imports.retain(&local).unwrap();
        imports.release(local);
        assert_eq!(imports.resolve(stale), Some(&17));
        assert!(imports.pending_release(0).is_none());
        imports.release(alias);
        assert!(imports.resolve(stale).is_none());
        {
            let cleanup = imports.pending_release(0).unwrap();
            assert_eq!(cleanup.count(), 1);
            assert_eq!(cleanup.id(), wire);
        }
        assert_eq!(imports.len(), 1);
        let revived = imports.receive(wire);
        if generation == u64::MAX - 1 {
            assert_eq!(revived.unwrap_err(), Error::GenerationExhausted);
        } else {
            let revived = revived.unwrap();
            kani::assert(
                imports.resolve(stale).is_none(),
                "revived import rejects stale key",
            );
            assert_eq!(imports.get(&revived), &17);
            imports.release(revived);
        }
        let cleanup = imports.pending_release(0).unwrap();
        assert_eq!(
            cleanup.count(),
            if generation == u64::MAX - 1 { 1 } else { 2 }
        );
        assert_eq!(cleanup.commit(), 17);
        kani::assert(imports.is_empty(), "accepted import cleanup detaches owner");
        let reused = imports.admit(wire, 29u8);
        if generation >= u64::MAX - 2 {
            assert_eq!(reused.unwrap_err().error, Error::GenerationExhausted);
        } else {
            let reused = reused.unwrap();
            assert_eq!(imports.resolve(stale), None);
            assert_eq!(imports.get(&reused), &29);
            imports.release(reused);
            assert_eq!(imports.pending_release(0).unwrap().commit(), 29);
        }
    })
    .unwrap();
    assert!(storage[0].entry.is_none());
    assert!(storage[0].generation >= generation);
}

/// Inductive count admission from arbitrary valid full-width reference counts,
/// including a pending release with no local handles. Failure is atomic.
#[kani::proof]
#[kani::unwind(3)]
fn import_receive_is_atomic() {
    let local: u32 = kani::any();
    let remote: u32 = kani::any();
    let generation: u64 = kani::any();
    kani::assume(remote != 0 && generation != 0);
    let wire = ImportId::from_wire(kani::any());
    let mut refs = ImportRefs::new();
    refs.receive(remote).unwrap();
    let mut storage = [Slot {
        free: crate::free_slots::Link::EMPTY,
        release_previous: None,
        release_next: None,
        generation,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            wire,
            value: 23u8,
            local,
            remote: refs,
            promise: promise::ImportState::Hosted,
        }),
    }];
    let mut imports = Imports {
        free: crate::free_slots::FreeSlots::from_storage(
            &mut storage,
            |slot| slot.entry.is_none() && slot.generation != u64::MAX,
            |slot| &mut slot.free,
        ),
        binding_revision: 0,
        release_head: (local == 0).then(|| core::num::NonZeroUsize::new(1).unwrap()),
        release_tail: (local == 0).then(|| core::num::NonZeroUsize::new(1).unwrap()),
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut storage),
        brand: PhantomData,
    };
    let result = imports.receive(wire);
    let expected_error = if local == u32::MAX {
        Some(Error::LocalReferencesExhausted)
    } else if local == 0 && generation == u64::MAX {
        Some(Error::GenerationExhausted)
    } else if remote == u32::MAX {
        Some(Error::RemoteReferencesExhausted)
    } else {
        None
    };
    let slot = &imports.slots[0];
    let entry = slot.entry.as_ref().unwrap();
    if let Some(error) = expected_error {
        assert_eq!(result.unwrap_err(), error);
        assert_eq!(entry.local, local);
        assert_eq!(entry.remote.count(), remote);
        assert_eq!(slot.generation, generation);
    } else {
        let received = result.unwrap();
        assert_eq!(entry.local, local + 1);
        assert_eq!(entry.remote.count(), remote + 1);
        assert_eq!(slot.generation, generation + u64::from(local == 0));
        assert_eq!(imports.get(&received), &23);
    }
}

/// Application metadata is reachable only through an owning receipt. Updating
/// it preserves full-width ownership counts, promise state and generation.
#[kani::proof]
#[kani::unwind(3)]
fn receipt_metadata_preserves_ownership() {
    let local: u32 = kani::any();
    let remote: u32 = kani::any();
    let generation: u64 = kani::any();
    kani::assume(local != 0 && remote != 0 && generation != 0);
    let wire = ImportId::from_wire(kani::any());
    let mut refs = ImportRefs::new();
    refs.receive(remote).unwrap();
    let state = if kani::any() {
        promise::ImportState::Pending
    } else {
        promise::ImportState::Hosted
    };
    let mut storage = [Slot {
        free: crate::free_slots::Link::EMPTY,
        release_previous: None,
        release_next: None,
        generation,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            wire,
            value: 17u32,
            local,
            remote: refs,
            promise: state,
        }),
    }];
    let mut imports = Imports {
        free: crate::free_slots::FreeSlots::from_storage(
            &mut storage,
            |slot| slot.entry.is_none() && slot.generation != u64::MAX,
            |slot| &mut slot.free,
        ),
        binding_revision: 0,
        release_head: (local == 0).then(|| core::num::NonZeroUsize::new(1).unwrap()),
        release_tail: (local == 0).then(|| core::num::NonZeroUsize::new(1).unwrap()),
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut storage),
        brand: PhantomData,
    };
    let reference = Local {
        key: Key {
            index: 0,
            generation,
            brand: PhantomData,
        },
    };
    let value: u32 = kani::any();
    *imports.get_mut(&reference) = value;
    assert_eq!(*imports.get(&reference), value);
    assert_eq!(imports.wire_id(&reference), wire);
    assert_eq!(imports.promise_state(&reference), state);
    let entry = imports.slots[0].entry.as_ref().unwrap();
    assert_eq!(entry.local, local);
    assert_eq!(entry.remote.count(), remote);
    assert_eq!(imports.slots[0].generation, generation);
    imports.release(reference);
    assert_eq!(imports.slots[0].entry.as_ref().unwrap().local, local - 1);
}

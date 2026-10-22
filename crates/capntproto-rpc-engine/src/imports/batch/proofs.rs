use super::*;

/// Existing promise with an arbitrary nonzero wire reference count. Resolving
/// consumes one obligation, never a wire reference, and is atomic with a new
/// replacement import. Rejected preflight leaves its owner in caller storage.
#[kani::proof]
#[kani::unwind(4)]
fn resolution_is_once_and_preserves_promise_references() {
    let count: u32 = kani::any();
    kani::assume(count != 0);
    let mut refs = ImportRefs::new();
    refs.receive(count).unwrap();
    let id = ImportId::from_wire(17);
    let mut slots = [
        Slot {
            release_previous: None,
            release_next: None,
            generation: 1,
            entry: Some(Entry {
                #[cfg(feature = "alloc")]
                presence: Default::default(),
                wire: id,
                value: 41u8,
                local: 1,
                remote: refs,
                promise: promise::ImportState::Pending,
            }),
        },
        Slot::EMPTY,
    ];
    let mut imports = Imports {
        binding_revision: 0,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut slots),
        release_head: None,
        release_tail: None,
        brand: PhantomData,
    };
    let mut receipts = [ReceiveSlot::new(ImportId::from_wire(23), 47)];
    let accept: bool = kani::any();
    let prepared = imports.prepare_resolution(id, &mut receipts).unwrap();
    if accept {
        prepared.commit();
    } else {
        drop(prepared);
    }
    kani::assert(
        imports.slots[0].entry.as_ref().unwrap().remote.count() == count,
        "Resolve adds no promise references",
    );
    let again = imports.prepare_resolution(id, &mut []);
    kani::assert(
        again.is_ok() != accept,
        "promise resolution is committed exactly once",
    );
    drop(again);
    if accept {
        assert_eq!(imports.get(receipts[0].local().unwrap()), &47);
        assert!(receipts[0].owner().is_none());
    } else {
        assert_eq!(receipts[0].owner(), Some(&47));
        assert_eq!(imports.len(), 1);
    }
}
use crate::ids::WireId;

#[kani::proof]
#[kani::unwind(5)]
fn duplicate_batch_admission_is_atomic_for_all_reference_counts() {
    let local: u32 = kani::any();
    let remote: u32 = kani::any();
    let generation: u64 = kani::any();
    kani::assume(remote != 0 && generation != 0);
    let wire = ImportId::from_wire(kani::any());
    let mut refs = ImportRefs::new();
    refs.receive(remote).unwrap();
    let mut slots = [Slot {
        release_previous: None,
        release_next: None,
        generation,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            wire,
            value: 17u8,
            local,
            remote: refs,
            promise: promise::ImportState::Hosted,
        }),
    }];
    let mut imports = Imports {
        binding_revision: 0,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut slots),
        release_head: (local == 0).then(|| core::num::NonZeroUsize::new(1).unwrap()),
        release_tail: (local == 0).then(|| core::num::NonZeroUsize::new(1).unwrap()),
        brand: PhantomData,
    };
    let mut receipts = [ReceiveSlot::alias(wire), ReceiveSlot::alias(wire)];
    let accepted = {
        let prepared = imports.prepare_receive(&mut receipts);
        match prepared {
            Ok(prepared) => {
                prepared.commit();
                true
            }
            Err(_) => false,
        }
    };
    let expected =
        local <= u32::MAX - 2 && remote <= u32::MAX - 2 && (local != 0 || generation < u64::MAX);
    kani::assert(
        accepted == expected,
        "batch accepts exactly representable references",
    );
    let entry = imports.slots[0].entry.as_ref().unwrap();
    kani::assert(
        entry.local == local + if accepted { 2 } else { 0 },
        "batch reference rejection is atomic",
    );
    assert_eq!(entry.remote.count(), remote + if accepted { 2 } else { 0 });
    assert_eq!(imports.has_pending_release(), !accepted && local == 0);
    assert_eq!(
        imports.slots[0].generation,
        generation + u64::from(accepted && local == 0)
    );
    if accepted {
        let first = receipts[0].take_local().unwrap();
        let second = receipts[1].take_local().unwrap();
        assert_eq!(first.key(), second.key());
        assert_eq!(imports.get(&first), &17);
        imports.release(first);
        imports.release(second);
    } else {
        assert!(receipts
            .iter()
            .all(|r| r.local().is_none() && r.plan.is_none()));
    }
}

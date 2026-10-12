extern crate std;

use super::*;
use crate::ids::{AnswerId, ImportId, WireId};
use core::cell::Cell;
use std::collections::BTreeMap;

fn compare_with_map<const LOW: usize, const SPARSE: usize>() {
    let mut storage = [const { Slot::EMPTY }; SPARSE];
    let mut table = PeerTable::<AnswerId, u64, _, LOW>::try_new(BorrowedSparse::new(&mut storage))
        .expect("fresh storage");
    let mut reference = BTreeMap::new();
    let ids = [
        0,
        1,
        7,
        15,
        16,
        17,
        (1 << 30) - 1,
        1 << 30,
        1 << 31,
        u32::MAX,
    ];
    let mut seed = 0x7891_9bcau64;
    for step in 0..4096u64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let raw = ids[(seed >> 16) as usize % ids.len()];
        let id = AnswerId::from_wire(raw);
        let present = reference.contains_key(&raw);
        let fits = (raw as u64) < LOW as u64
            || present
            || reference
                .keys()
                .filter(|&&key| (key as u64) >= LOW as u64)
                .count()
                < SPARSE;
        match (seed >> 32) % 6 {
            0 => {
                let expected = if fits {
                    Ok(reference.insert(raw, step))
                } else {
                    Err(step)
                };
                assert_eq!(table.try_insert(id, step), expected);
            }
            1 => {
                let expected = if present {
                    Err(InsertError {
                        kind: InsertErrorKind::Duplicate,
                        value: step,
                    })
                } else if fits {
                    reference.insert(raw, step);
                    Ok(())
                } else {
                    Err(InsertError {
                        kind: InsertErrorKind::Full,
                        value: step,
                    })
                };
                assert_eq!(table.try_insert_unique(id, step), expected);
            }
            2 => assert_eq!(table.remove(&id), reference.remove(&raw)),
            3 => {
                for value in [table.get_mut(&id), reference.get_mut(&raw)]
                    .into_iter()
                    .flatten()
                {
                    *value += 1;
                }
            }
            4 => {
                for value in table.values_mut().chain(reference.values_mut()) {
                    *value += 1;
                }
            }
            _ => assert_eq!(table.get(&id), reference.get(&raw)),
        }
        assert_eq!(table.len(), reference.len());
        assert_eq!(table.is_empty(), reference.is_empty());
        assert_eq!(
            table.values().copied().sum::<u64>(),
            reference.values().copied().sum::<u64>()
        );
        for raw in ids {
            assert_eq!(table.get(&AnswerId::from_wire(raw)), reference.get(&raw));
            assert_eq!(
                table.contains_key(&AnswerId::from_wire(raw)),
                reference.contains_key(&raw)
            );
        }
    }
}

#[test]
fn bounded_tables_match_map_with_capacity_faults_and_full_wire_id_range() {
    compare_with_map::<0, 0>();
    compare_with_map::<0, 1>();
    compare_with_map::<0, 4>();
    compare_with_map::<1, 1>();
    compare_with_map::<16, 0>();
    compare_with_map::<16, 1>();
    compare_with_map::<16, 4>();
}

struct Owner<'a>(&'a Cell<usize>);
impl Drop for Owner<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn ownership_is_returned_on_rejection_replacement_and_removal() {
    let drops = Cell::new(0);
    let mut storage = [Slot::EMPTY];
    for raw in [0, u32::MAX] {
        let id = ImportId::from_wire(raw);
        let mut table = PeerTable::<ImportId, _, _, 1>::try_new(BorrowedSparse::new(&mut storage))
            .expect("storage was not cleared");
        assert!(table.try_insert_unique(id, Owner(&drops)).is_ok());
        let before = drops.get();
        let duplicate = table.try_insert_unique(id, Owner(&drops)).err().unwrap();
        assert_eq!(duplicate.kind, InsertErrorKind::Duplicate);
        let previous = table.try_insert(id, Owner(&drops)).ok().unwrap();
        let removed = table.remove(&id);
        assert_eq!(drops.get(), before);
        assert!(table.remove(&id).is_none());
        drop((duplicate, previous, removed));
        assert_eq!(drops.get(), before + 3);
        assert!(table.try_insert_unique(id, Owner(&drops)).is_ok());
        assert_eq!(
            table
                .try_insert_unique(ImportId::from_wire(7), Owner(&drops))
                .is_ok(),
            raw == 0
        );
        // Rejected temporaries are released by the caller; retained owners by teardown.
        drop(table);
    }
    assert_eq!(drops.get(), 10);
    let table = PeerTable::<ImportId, Owner<'_>, _, 1>::try_new(BorrowedSparse::new(&mut storage))
        .expect("storage was not cleared");
    assert!(table.is_empty());
}

#[test]
fn bounded_success_failure_and_teardown_never_allocate() {
    let drops = Cell::new(0);
    let counts = allocation_counter::measure(|| {
        let mut slots = [const { Slot::EMPTY }; 2];
        for _ in 0..128 {
            let mut table =
                PeerTable::<ImportId, _, _, 1>::try_new(BorrowedSparse::new(&mut slots))
                    .expect("storage was not cleared");
            for raw in [0, 1, u32::MAX] {
                assert!(table
                    .try_insert_unique(ImportId::from_wire(raw), Owner(&drops))
                    .is_ok());
            }
            let duplicate = table
                .try_insert_unique(ImportId::from_wire(0), Owner(&drops))
                .err()
                .unwrap();
            assert_eq!(duplicate.kind, InsertErrorKind::Duplicate);
            let full = table
                .try_insert_unique(ImportId::from_wire(2), Owner(&drops))
                .err()
                .unwrap();
            assert_eq!(full.kind, InsertErrorKind::Full);
            assert_eq!(table.values().count(), 3);
            drop(table.remove(&ImportId::from_wire(u32::MAX)));
            assert!(table
                .try_insert_unique(ImportId::from_wire(2), full.value)
                .is_ok());
            drop(duplicate);
            // Dropping the table releases the three remaining owners and
            // returns empty caller-owned slots, ready for the next iteration.
        }
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(drops.get(), 128 * 5);
}

#[test]
fn full_storage_returns_the_owner_without_consuming_or_dropping_it() {
    let drops = Cell::new(0);
    let mut storage = [Slot::EMPTY];
    let mut table = PeerTable::<ImportId, _, _, 0>::try_new(BorrowedSparse::new(&mut storage))
        .expect("fresh storage");
    assert!(table
        .try_insert_unique(ImportId::from_wire(u32::MAX), Owner(&drops))
        .is_ok());
    let rejected = table
        .try_insert(ImportId::from_wire(0), Owner(&drops))
        .err()
        .unwrap();
    let unique = table
        .try_insert_unique(ImportId::from_wire(1), Owner(&drops))
        .err()
        .unwrap();
    assert_eq!(unique.kind, InsertErrorKind::Full);
    assert_eq!(drops.get(), 0);
    assert_eq!(table.len(), 1);
    assert!(table.get(&ImportId::from_wire(u32::MAX)).is_some());
    let removed = table.remove(&ImportId::from_wire(u32::MAX));
    assert!(table
        .try_insert_unique(ImportId::from_wire(0), rejected)
        .is_ok());
    drop((removed, unique, table));
    assert_eq!(drops.get(), 3);
}

#[test]
fn nonempty_backing_is_returned_unchanged_and_borrowed_storage_is_reusable() {
    let drops = Cell::new(0);
    let mut slots = [Slot::EMPTY];
    let mut backing = BorrowedSparse::new(&mut slots);
    let id = ImportId::from_wire(1);
    assert!(backing.try_insert(id, Owner(&drops)).is_ok());
    // Simulate a provider forgotten by its owner, then reattach the same slice.
    core::mem::forget(backing);
    let backing = BorrowedSparse::new(&mut slots);
    let mut backing = PeerTable::<ImportId, _, _, 16>::try_new(backing)
        .err()
        .unwrap();
    assert_eq!(backing.len(), 1);
    // Owner intentionally has no Debug implementation: diagnostics expose
    // resource metadata, without touching or formatting capability payloads.
    assert_eq!(
        std::format!("{backing:?}"),
        "BorrowedSparse { len: 1, capacity: 1 }"
    );
    assert_eq!(drops.get(), 0);
    assert!(backing.get(&id).is_some());
    for _ in backing.values_mut() {}
    assert_eq!(backing.values().count(), 1);
    drop(backing);
    assert_eq!(drops.get(), 1);
    let table = PeerTable::<ImportId, Owner<'_>, _, 16>::try_new(BorrowedSparse::new(&mut slots))
        .expect("storage was not cleared");
    assert!(table.is_empty());
}

#[cfg(feature = "alloc")]
#[test]
fn growing_sparse_bounds_entry_count_instead_of_wire_id() {
    let mut slots = super::SparseSlots::growing(0, 2).unwrap();
    let high = AnswerId::from_wire(u32::MAX);
    let low = AnswerId::from_wire(0);
    let other = AnswerId::from_wire(1);
    assert_eq!(slots.try_insert(high, 41), Ok(None));
    assert_eq!(slots.try_insert(low, 43), Ok(None));
    assert_eq!(slots.try_insert(other, 47), Err(47));
    assert_eq!(slots.try_insert(high, 53), Ok(Some(41)));
    assert_eq!(slots.len(), 2);
    assert_eq!(slots.remove(&low), Some(43));
    assert_eq!(slots.try_insert(other, 59), Ok(None));
    assert_eq!(slots.get(&high), Some(&53));
    assert_eq!(slots.get(&other), Some(&59));
    assert!(super::SparseSlots::<AnswerId, u32>::growing(3, 2).is_none());
    assert!(super::SparseSlots::<AnswerId, u32>::growing(usize::MAX, usize::MAX).is_none());
}

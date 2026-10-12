extern crate std;

use super::*;
use crate::{
    ids::ExportId,
    peer_table::{BorrowedSparse, Slot as SparseSlot},
};
use core::cell::Cell;
use std::{collections::BTreeMap, vec::Vec};

fn compare<const N: usize, const H: usize, const A: usize>() {
    let mut dense = [const { Slot::EMPTY }; N];
    let mut high = [const { SparseSlot::EMPTY }; H];
    let mut adopted = [const { SparseSlot::EMPTY }; A];
    let mut table = LocalTable::<QuestionId, u64, _, _, _>::try_new(
        BorrowedSlots::new(&mut dense),
        BorrowedSparse::new(&mut high),
        BorrowedSparse::new(&mut adopted),
    )
    .unwrap();
    let mut reference = BTreeMap::<u32, u64>::new();
    let mut high_cursor = HIGH_BIT;
    let mut seed = 0x1274_6991_37ab_u64;
    for step in 0..8192u64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let raw = match seed % 5 {
            0 => u32::MAX,
            1 => (1 << 30) + ((seed >> 24) % 5) as u32,
            2 => HIGH_BIT + ((seed >> 24) % 128) as u32,
            _ => ((seed >> 24) % (N + 2) as u64) as u32,
        };
        let id = QuestionId::from_wire(raw);
        let fail = |kind| Err(InsertError { kind, value: step });
        match (seed >> 16) % 8 {
            0 | 1 => {
                let free = (0..N as u32).find(|key| !reference.contains_key(key));
                let expected = match free {
                    Some(key) => {
                        reference.insert(key, step);
                        Ok(QuestionId::from_wire(key))
                    }
                    None => fail(InsertErrorKind::Full),
                };
                assert_eq!(table.try_push(step), expected);
            }
            2 => {
                let expected = if reference.keys().filter(|&&k| k & HIGH_BIT != 0).count() == H {
                    fail(InsertErrorKind::Full)
                } else {
                    while reference.contains_key(&high_cursor) {
                        high_cursor += 1;
                    }
                    let next = high_cursor;
                    high_cursor += 1;
                    reference.insert(next, step);
                    Ok(QuestionId::from_wire(next))
                };
                assert_eq!(table.try_push_high(step), expected);
            }
            3 => {
                let error = if !id.is_adopted() {
                    Some(InsertErrorKind::InvalidAdoptedId)
                } else if reference.contains_key(&raw) {
                    Some(InsertErrorKind::Duplicate)
                } else if reference
                    .keys()
                    .filter(|&&k| (1 << 30..HIGH_BIT).contains(&k))
                    .count()
                    == A
                {
                    Some(InsertErrorKind::Full)
                } else {
                    None
                };
                let expected = match error {
                    Some(kind) => Err(InsertError { kind, value: step }),
                    None => {
                        reference.insert(raw, step);
                        Ok(())
                    }
                };
                assert_eq!(table.try_insert_adopted(id, step), expected);
            }
            4 => assert_eq!(table.remove(id), reference.remove(&raw)),
            5 => {
                for value in [table.get_mut(id), reference.get_mut(&raw)]
                    .into_iter()
                    .flatten()
                {
                    *value += 1;
                }
            }
            6 => {
                for value in table.values_mut().chain(reference.values_mut()) {
                    *value += 1;
                }
            }
            _ => assert_eq!(table.get(id), reference.get(&raw)),
        }
        assert_eq!(table.len(), reference.len());
        assert_eq!(table.is_empty(), reference.is_empty());
        assert_eq!(
            table.adopted_len(),
            reference
                .keys()
                .filter(|&&k| (1 << 30..HIGH_BIT).contains(&k))
                .count()
        );
        let mut values: Vec<_> = table.values().copied().collect();
        let mut expected: Vec<_> = reference.values().copied().collect();
        values.sort_unstable();
        expected.sort_unstable();
        assert_eq!(values, expected);
        for (&key, value) in &reference {
            assert_eq!(table.get(QuestionId::from_wire(key)), Some(value));
        }
        assert_eq!(table.get(id), reference.get(&raw));
    }
}

#[test]
fn bounded_local_tables_match_reference_with_capacity_faults() {
    compare::<0, 0, 0>();
    compare::<1, 1, 1>();
    compare::<2, 0, 2>();
    compare::<3, 3, 0>();
    compare::<7, 2, 3>();
    compare::<16, 4, 4>();
    compare::<65, 1, 1>();
}

#[test]
fn lowest_free_id_survives_arbitrary_removal_order_and_duplicate_removals() {
    let mut dense = [const { Slot::EMPTY }; 129];
    let mut h = [];
    let mut a = [];
    let mut table = LocalTable::<ExportId, _, _, _, _>::try_new(
        BorrowedSlots::new(&mut dense),
        BorrowedSparse::new(&mut h),
        BorrowedSparse::new(&mut a),
    )
    .unwrap();
    for id in 0..129 {
        assert_eq!(table.try_push(id).unwrap().to_wire(), id);
    }
    for cycle in 0..8 {
        // Multiplication by 37 permutes 129 slots and exercises both heap children.
        for n in 0..129 {
            let id = ExportId::from_wire((n * 37 + cycle) % 129);
            assert_eq!(table.remove(id), Some(id.to_wire()));
            assert_eq!(table.remove(id), None);
        }
        assert!(table.is_empty());
        for raw in 0..129 {
            assert_eq!(table.try_push(raw).unwrap().to_wire(), raw);
        }
        assert_eq!(table.try_push(999).unwrap_err().kind, InsertErrorKind::Full);
    }
}

#[test]
fn high_wrap_skips_live_ids_and_failure_does_not_advance_cursor() {
    let mut dense = [Slot::EMPTY];
    let mut high = [const { SparseSlot::EMPTY }; 3];
    let mut adopted = [];
    let mut table = LocalTable::try_new(
        BorrowedSlots::new(&mut dense),
        BorrowedSparse::new(&mut high),
        BorrowedSparse::new(&mut adopted),
    )
    .unwrap();
    let first = table.try_push_high(10).unwrap();
    assert_eq!(first.to_wire(), HIGH_BIT);
    table.high_counter = u32::MAX;
    assert_eq!(table.try_push_high(20).unwrap().to_wire(), u32::MAX);
    assert_eq!(table.try_push_high(30).unwrap().to_wire(), HIGH_BIT + 1);
    let cursor = table.high_counter;
    assert_eq!(
        table.try_push_high(40),
        Err(InsertError {
            kind: InsertErrorKind::Full,
            value: 40
        })
    );
    assert_eq!(table.high_counter, cursor);
    assert_eq!(table.remove(first), Some(10));
    assert_eq!(table.try_push_high(40).unwrap().to_wire(), HIGH_BIT + 2);
    assert_eq!(table.try_push(50).unwrap().to_wire(), 0);
    assert_eq!(table.get(QuestionId::from_wire(u32::MAX)), Some(&20));
    assert_eq!(
        table.get_mut(QuestionId::from_wire(HIGH_BIT + 1)),
        Some(&mut 30)
    );
}

struct Owner<'a>(&'a Cell<usize>);
impl Drop for Owner<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn rejection_and_teardown_preserve_ownership_and_storage_reuse() {
    let drops = Cell::new(0);
    let mut dense = [Slot::EMPTY];
    let mut high = [SparseSlot::EMPTY];
    let mut adopted = [SparseSlot::EMPTY];
    for _ in 0..2 {
        let mut table = LocalTable::try_new(
            BorrowedSlots::new(&mut dense),
            BorrowedSparse::new(&mut high),
            BorrowedSparse::new(&mut adopted),
        )
        .unwrap();
        assert!(table.try_push(Owner(&drops)).is_ok());
        let before = drops.get();
        let rejected = table.try_push(Owner(&drops)).err().unwrap();
        assert_eq!(rejected.kind, InsertErrorKind::Full);
        assert_eq!(drops.get(), before);
        drop(rejected.value);
        assert!(table.try_push_high(Owner(&drops)).is_ok());
        let id = QuestionId::from_wire(1 << 30);
        assert!(table.try_insert_adopted(id, Owner(&drops)).is_ok());
        for (raw, kind) in [
            (1 << 30, InsertErrorKind::Duplicate),
            (0, InsertErrorKind::InvalidAdoptedId),
            ((1 << 30) + 1, InsertErrorKind::Full),
        ] {
            let rejected = table
                .try_insert_adopted(QuestionId::from_wire(raw), Owner(&drops))
                .err()
                .unwrap();
            assert_eq!(rejected.kind, kind);
            drop(rejected.value);
        }
        let owner = table.remove(id).unwrap();
        assert!(table.try_insert_adopted(id, owner).is_ok());
        assert_eq!(table.len(), 3);
        drop(table);
    }
    assert_eq!(drops.get(), 14);
}

#[test]
fn nonempty_storage_is_returned_and_rejected_without_dropping_values() {
    for range in 0..3 {
        let drops = Cell::new(0);
        let mut d = [Slot::EMPTY];
        let mut h = [SparseSlot::EMPTY];
        let mut a = [SparseSlot::EMPTY];
        let mut table = LocalTable::try_new(
            BorrowedSlots::new(&mut d),
            BorrowedSparse::new(&mut h),
            BorrowedSparse::new(&mut a),
        )
        .unwrap();
        match range {
            0 => {
                assert!(table.try_push(Owner(&drops)).is_ok());
            }
            1 => {
                assert!(table.try_push_high(Owner(&drops)).is_ok());
            }
            _ => {
                assert!(table
                    .try_insert_adopted(QuestionId::from_wire(1 << 30), Owner(&drops))
                    .is_ok());
            }
        }
        core::mem::forget(table);
        let rejected = LocalTable::<QuestionId, _, _, _, _>::try_new(
            BorrowedSlots::new(&mut d),
            BorrowedSparse::new(&mut h),
            BorrowedSparse::new(&mut a),
        )
        .err()
        .unwrap();
        assert_eq!(drops.get(), 0);
        drop(rejected);
        assert_eq!(drops.get(), 1);
    }
}

#[test]
fn allocation_and_cleanup_paths_do_not_allocate() {
    let stats = allocation_counter::measure(|| {
        let mut d = [const { Slot::EMPTY }; 8];
        let mut h = [SparseSlot::EMPTY];
        let mut a = [SparseSlot::EMPTY];
        for _ in 0..128 {
            let mut table = LocalTable::<QuestionId, _, _, _, _>::try_new(
                BorrowedSlots::new(&mut d),
                BorrowedSparse::new(&mut h),
                BorrowedSparse::new(&mut a),
            )
            .unwrap();
            for id in 0..8 {
                assert_eq!(table.try_push(id).unwrap().to_wire(), id);
            }
            let full = table.try_push(99).unwrap_err();
            assert_eq!(full.kind, InsertErrorKind::Full);
            for id in [7, 3, 1, 6] {
                assert!(table.remove(QuestionId::from_wire(id)).is_some());
            }
            for id in [1, 3, 6, 7] {
                assert_eq!(table.try_push(full.value).unwrap().to_wire(), id);
            }
            assert!(table.try_push_high(8).is_ok());
            assert!(table.try_push_high(9).is_err());
            let id = QuestionId::from_wire(1 << 30);
            table.try_insert_adopted(id, 10).unwrap();
            assert!(table.try_insert_adopted(id, 11).is_err());
            assert!(table
                .try_insert_adopted(QuestionId::from_wire(0), 11)
                .is_err());
            assert!(table
                .try_insert_adopted(QuestionId::from_wire((1 << 30) + 1), 11)
                .is_err());
        }
    });
    assert_eq!(stats.count_total, 0);
    assert_eq!(stats.bytes_total, 0);
}

#[test]
fn local_wire_space_exhaustion_returns_the_value_without_mutation() {
    // Exercise the protocol ceiling without allocating a billion slots. This
    // state only reaches the guard; no slot is indexed or accessed on rejection.
    let mut d = [];
    let mut h = [];
    let mut a = [];
    let mut table = LocalTable::<ExportId, _, _, _, _>::try_new(
        BorrowedSlots::new(&mut d),
        BorrowedSparse::new(&mut h),
        BorrowedSparse::new(&mut a),
    )
    .unwrap();
    table.issued = LOW_LIMIT as usize;
    assert_eq!(
        table.try_push(42),
        Err(InsertError {
            kind: InsertErrorKind::IdSpaceExhausted,
            value: 42
        })
    );
    assert_eq!(table.issued, LOW_LIMIT as usize);
    assert_eq!(table.free_len, 0);
}

#[test]
fn failed_growth_can_retry_without_skipping_an_id_or_losing_free_slots() {
    struct Growing {
        slots: [Slot<u32>; 4],
        visible: usize,
        fail: bool,
    }
    impl Storage<u32> for Growing {
        fn slots(&self) -> &[Slot<u32>] {
            &self.slots[..self.visible]
        }
        fn slots_mut(&mut self) -> &mut [Slot<u32>] {
            &mut self.slots[..self.visible]
        }
        fn try_grow(&mut self) -> bool {
            if self.fail || self.visible == self.slots.len() {
                return false;
            }
            self.visible += 1;
            true
        }
    }
    let mut h = [];
    let mut a = [];
    let mut table = LocalTable::<ExportId, _, _, _, _>::try_new(
        Growing {
            slots: [const { Slot::EMPTY }; 4],
            visible: 0,
            fail: false,
        },
        BorrowedSparse::new(&mut h),
        BorrowedSparse::new(&mut a),
    )
    .ok()
    .unwrap();
    let first = table.try_push(10).unwrap();
    table.dense.fail = true;
    assert_eq!(
        table.try_push(20),
        Err(InsertError {
            kind: InsertErrorKind::Full,
            value: 20
        })
    );
    assert_eq!(table.get(first), Some(&10));
    assert_eq!(table.remove(first), Some(10));
    assert_eq!(table.try_push(11).unwrap(), first);
    assert_eq!(table.dense.visible, 1);
    table.dense.fail = false;
    for raw in 1..4 {
        assert_eq!(table.try_push(raw).unwrap().to_wire(), raw);
    }
    assert_eq!(table.len(), 4);
    assert_eq!(table.try_push(99).unwrap_err().kind, InsertErrorKind::Full);
}

#[test]
fn diagnostics_do_not_require_or_format_owner_debug() {
    struct Opaque;
    let mut slots = [Slot::<Opaque>::EMPTY];
    let storage = BorrowedSlots::new(&mut slots);
    assert_eq!(std::format!("{storage:?}"), "BorrowedSlots { capacity: 1 }");
}

#[cfg(feature = "alloc")]
#[test]
fn growing_local_ids_preserve_owners_limits_and_lowest_free_reuse() {
    use crate::peer_table::NoSparse;
    assert!(GrowingSlots::<u32>::try_with_capacity(2, 1).is_none());
    assert!(GrowingSlots::<u32>::try_with_capacity(0, LOW_LIMIT as usize + 1).is_none());
    let dense = GrowingSlots::try_with_capacity(0, 17).unwrap();
    let mut table = LocalTable::<ExportId, _, _, _, _>::try_new(dense, NoSparse, NoSparse)
        .ok()
        .unwrap();
    for n in 0..17 {
        assert_eq!(table.try_push(n), Ok(ExportId::from_wire(n)));
    }
    assert_eq!(table.try_push(99).unwrap_err().value, 99);
    for n in [14, 2, 9] {
        assert_eq!(table.remove(ExportId::from_wire(n)), Some(n));
    }
    for n in [2, 9, 14] {
        assert_eq!(table.try_push(n + 100), Ok(ExportId::from_wire(n)));
    }
    for n in 0..17 {
        assert_eq!(
            table.get(ExportId::from_wire(n)),
            Some(&if [2, 9, 14].contains(&n) { n + 100 } else { n })
        );
    }
    assert_eq!(table.try_push(103).unwrap_err().kind, InsertErrorKind::Full);
}

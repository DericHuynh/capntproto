//! Bounded proofs of the real storage/free-heap implementation.
use super::*;
use crate::{
    ids::ExportId,
    peer_table::{BorrowedSparse, Slot as SparseSlot},
};

type Fixed<'a> = LocalTable<
    QuestionId,
    u8,
    BorrowedSlots<'a, u8>,
    BorrowedSparse<'a, QuestionId, u8>,
    BorrowedSparse<'a, QuestionId, u8>,
>;

fn dense_invariant(table: &Fixed<'_>) -> bool {
    let slots = table.dense.slots();
    if table.issued > slots.len() || table.free_len > table.issued {
        return false;
    }
    let mut free = 0;
    for (i, slot) in slots.iter().enumerate() {
        if i >= table.issued {
            if slot.value.is_some() {
                return false;
            }
        } else if slot.value.is_none() {
            free += 1;
        }
    }
    if free != table.free_len {
        return false;
    }
    for i in 0..table.free_len {
        let raw = slots[i].free_id as usize;
        if raw >= table.issued || slots[raw].value.is_some() {
            return false;
        }
        if i > 0 && slots[(i - 1) / 2].free_id > slots[i].free_id {
            return false;
        }
        for j in 0..i {
            if slots[j].free_id == slots[i].free_id {
                return false;
            }
        }
    }
    true
}

#[kani::proof]
#[kani::unwind(4)]
fn local_dense_reuse_and_domain_isolation() {
    let mut slots = [const { Slot::EMPTY }; 3];
    let mut high = [];
    let mut adopted = [];
    let mut table = LocalTable::<QuestionId, u8, _, _, _>::try_new(
        BorrowedSlots::new(&mut slots),
        BorrowedSparse::new(&mut high),
        BorrowedSparse::new(&mut adopted),
    )
    .unwrap();
    // Induction base, then every state satisfying the heap/occupancy invariant.
    assert!(dense_invariant(&table));
    table.issued = kani::any();
    table.free_len = kani::any();
    for slot in table.dense.slots_mut() {
        slot.value = kani::any();
        slot.free_id = kani::any();
    }
    kani::assume(dense_invariant(&table));
    let mut reference = core::array::from_fn::<_, 3, _>(|i| table.dense.slots()[i].value);
    let mut export_slots = [const { Slot::EMPTY }; 1];
    let mut export_high = [];
    let mut export_adopted = [];
    let mut exports = LocalTable::<ExportId, _, _, _, _>::try_new(
        BorrowedSlots::new(&mut export_slots),
        BorrowedSparse::new(&mut export_high),
        BorrowedSparse::new(&mut export_adopted),
    )
    .unwrap();
    let owner: u8 = kani::any();
    let export = exports.try_push(owner).unwrap();
    let value: u8 = kani::any();
    let raw: u32 = kani::any();
    let id = QuestionId::from_wire(raw);
    if kani::any() {
        let expected = reference.iter().position(Option::is_none);
        match (table.try_push(value), expected) {
            (Ok(actual), Some(index)) => {
                assert_eq!(actual.to_wire(), index as u32);
                reference[index] = Some(value);
            }
            (Err(error), None) => {
                assert_eq!(error.kind, InsertErrorKind::Full);
                assert_eq!(error.value, value);
            }
            _ => panic!("allocation differs from lowest vacancy"),
        }
    } else {
        let expected = if raw < 3 {
            reference[raw as usize].take()
        } else {
            None
        };
        assert_eq!(table.remove(id), expected);
        assert_eq!(table.remove(id), None);
    }
    assert!(dense_invariant(&table), "local free heap invariant");
    for (index, expected) in reference.iter().enumerate() {
        assert_eq!(
            table.get(QuestionId::from_wire(index as u32)),
            expected.as_ref()
        );
    }
    let count = reference.iter().filter(|v| v.is_some()).count();
    assert_eq!(table.len(), count);
    assert_eq!(table.values().count(), count);
    assert_eq!(table.is_empty(), count == 0);
    assert_eq!(exports.get(export), Some(&owner));
    assert_eq!(exports.len(), 1);
}

#[kani::proof]
#[kani::unwind(4)]
fn local_high_wrap_and_capacity_rollback() {
    let mut slots = [const { Slot::EMPTY }; 1];
    let mut high = [const { SparseSlot::EMPTY }; 2];
    let mut adopted = [const { SparseSlot::EMPTY }; 1];
    let mut table = LocalTable::<QuestionId, u8, _, _, _>::try_new(
        BorrowedSlots::new(&mut slots),
        BorrowedSparse::new(&mut high),
        BorrowedSparse::new(&mut adopted),
    )
    .unwrap();
    let low = table.try_push(11).unwrap();
    let adopted_id = QuestionId::from_wire(1 << 30);
    table.try_insert_adopted(adopted_id, 12).unwrap();
    // Any cursor, including u32::MAX. Force a collision with a live ID after
    // wrapping, rather than requiring billions of allocations in a proof.
    let cursor: u32 = kani::any();
    table.high_counter = cursor;
    let first = table.try_push_high(13).unwrap();
    table.high_counter = cursor;
    let second = table.try_push_high(14).unwrap();
    assert_ne!(first, second);
    assert_eq!(first.to_wire(), cursor | HIGH_BIT);
    assert_eq!(
        second.to_wire(),
        (cursor | HIGH_BIT).wrapping_add(1) | HIGH_BIT
    );
    let before = table.high_counter;
    let error = table.try_push_high(15).unwrap_err();
    assert_eq!(error.kind, InsertErrorKind::Full);
    assert_eq!(error.value, 15);
    assert_eq!(table.high_counter, before);
    assert_eq!(table.get(first), Some(&13));
    assert_eq!(table.get(second), Some(&14));
    assert_eq!(table.get(low), Some(&11));
    assert_eq!(table.get(adopted_id), Some(&12));
    assert_eq!(table.remove(first), Some(13));
    table.try_push_high(15).unwrap();
    assert_eq!(table.len(), 4);
}

#[kani::proof]
#[kani::unwind(3)]
fn local_adopted_range_and_owner_preservation() {
    let mut slots = [const { Slot::EMPTY }; 1];
    let mut high = [const { SparseSlot::EMPTY }; 1];
    let mut adopted = [const { SparseSlot::EMPTY }; 1];
    let mut table = LocalTable::<QuestionId, u8, _, _, _>::try_new(
        BorrowedSlots::new(&mut slots),
        BorrowedSparse::new(&mut high),
        BorrowedSparse::new(&mut adopted),
    )
    .unwrap();
    let low = table.try_push(1).unwrap();
    let pipeline = table.try_push_high(2).unwrap();
    let raw: u32 = kani::any();
    let id = QuestionId::from_wire(raw);
    let value: u8 = kani::any();
    let result = table.try_insert_adopted(id, value);
    if (1 << 30..1 << 31).contains(&raw) {
        assert!(result.is_ok());
        assert_eq!(table.get(id), Some(&value));
        let duplicate = table.try_insert_adopted(id, 99);
        kani::assert(duplicate.is_err(), "adopted duplicate preserves owner");
        let duplicate = duplicate.unwrap_err();
        assert_eq!(duplicate.kind, InsertErrorKind::Duplicate);
        assert_eq!(duplicate.value, 99);
        let other = QuestionId::from_wire(raw ^ 1);
        assert_eq!(
            table.try_insert_adopted(other, 98).unwrap_err().kind,
            InsertErrorKind::Full
        );
        assert_eq!(table.get(id), Some(&value));
        assert_eq!(table.remove(id), Some(value));
    } else {
        let error = result.unwrap_err();
        assert_eq!(error.kind, InsertErrorKind::InvalidAdoptedId);
        assert_eq!(error.value, value);
    }
    assert_eq!(table.get(low), Some(&1));
    assert_eq!(table.get(pipeline), Some(&2));
    assert_eq!(table.len(), 2);
}

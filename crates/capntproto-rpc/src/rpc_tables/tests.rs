use super::super::rpc_ids::ExportId;
use super::*;

#[test]
fn peer_slots_match_sparse_map_across_low_boundary_replacement_and_removal() {
    use super::super::rpc_ids::AnswerId;
    let mut slots = PeerSlots::<AnswerId, u64>::default();
    let mut reference = std::collections::BTreeMap::new();
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
    let mut seed = 0x7891_9bca_u64;
    for step in 0..4096u64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let id = AnswerId::from_wire(ids[(seed >> 16) as usize % ids.len()]);
        match (seed >> 32) % 4 {
            0 | 1 => assert_eq!(slots.insert(id, step), reference.insert(id, step)),
            2 => assert_eq!(slots.remove(&id), reference.remove(&id)),
            _ => {
                for value in [slots.get_mut(&id), reference.get_mut(&id)]
                    .into_iter()
                    .flatten()
                {
                    *value += 1;
                }
            }
        }
        assert_eq!(slots.len(), reference.len());
        assert_eq!(slots.is_empty(), reference.is_empty());
        assert_eq!(
            slots.values().copied().sum::<u64>(),
            reference.values().copied().sum::<u64>()
        );
        assert_eq!(
            slots.high.len(),
            reference.keys().filter(|id| id.to_wire() >= 16).count()
        );
        for raw in ids {
            let id = AnswerId::from_wire(raw);
            assert_eq!(slots.get(&id), reference.get(&id));
            assert_eq!(slots.contains_key(&id), reference.contains_key(&id));
        }
    }
    for value in slots.values_mut() {
        *value = 42;
    }
    assert_eq!(
        slots.values().filter(|&&value| value == 42).count(),
        reference.len()
    );
    let detached = std::mem::take(&mut slots);
    assert!(slots.is_empty());
    assert_eq!(detached.len(), reference.len());
}

#[test]
fn peer_slot_owners_can_reenter_after_replacement_removal_and_detachment() {
    use super::super::rpc_ids::ImportId;
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };
    struct Owner {
        slots: Weak<RefCell<PeerSlots<ImportId, Owner>>>,
        drops: Rc<Cell<usize>>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            if let Some(slots) = self.slots.upgrade() {
                assert!(slots.try_borrow_mut().is_ok());
            }
            self.drops.set(self.drops.get() + 1);
        }
    }
    let slots = Rc::new(RefCell::new(PeerSlots::default()));
    let drops = Rc::new(Cell::new(0));
    for raw in [0, 15, 16, u32::MAX] {
        let id = ImportId::from_wire(raw);
        let owner = || Owner {
            slots: Rc::downgrade(&slots),
            drops: drops.clone(),
        };
        assert!(slots.borrow_mut().insert(id, owner()).is_none());
        let replaced = slots.borrow_mut().insert(id, owner());
        drop(replaced);
        let removed = slots.borrow_mut().remove(&id);
        drop(removed);
        assert!(slots.borrow_mut().remove(&id).is_none());
        assert!(slots.borrow_mut().insert(id, owner()).is_none());
    }
    assert_eq!(drops.get(), 8);
    let detached = std::mem::take(&mut *slots.borrow_mut());
    assert!(slots.borrow().is_empty());
    drop(detached);
    assert_eq!(drops.get(), 12);
    slots.borrow_mut().insert(
        ImportId::from_wire(0),
        Owner {
            slots: Rc::downgrade(&slots),
            drops: drops.clone(),
        },
    );
    drop(slots);
    assert_eq!(drops.get(), 13);
}

#[test]
fn wrap_skips_live_high_ids_and_keeps_low_allocator_independent() {
    let mut table = LocalTable::new();
    assert!(table.is_empty());
    let first = table.push_high("first");
    assert!(!table.is_empty());
    assert_eq!(first.to_wire(), 0x8000_0000);
    table.high_counter = 0xffff_ffff;
    assert_eq!(table.push_high("last").to_wire(), 0xffff_ffff);
    assert_eq!(table.push_high("wrapped").to_wire(), 0x8000_0001);
    assert_eq!(table.get(first), Some(&"first"));
    assert_eq!(table.get(QuestionId::from_wire(u32::MAX)), Some(&"last"));
    assert_eq!(
        table.get(QuestionId::from_wire(0x8000_0001)),
        Some(&"wrapped")
    );
    assert_eq!(table.get(QuestionId::from_wire(0x8000_0002)), None);
    let low = table.push("low");
    assert_eq!(low.to_wire(), 0);
    table.erase(first);
    assert_eq!(table.get(first), None);
    table.erase(low);
    assert_eq!(table.push("reused low"), low);
    assert_eq!(table.push_high("next high").to_wire(), 0x8000_0002);
    assert_eq!(
        table.find(QuestionId::from_wire(0xffff_ffff)),
        Some(&mut "last")
    );
    assert_eq!(table.iter().count(), 4);
    table.erase(low);
    assert!(!table.is_empty());
    for id in [0xffff_ffff, 0x8000_0001, 0x8000_0002] {
        table.erase(QuestionId::from_wire(id));
    }
    assert!(table.is_empty());
}

#[test]
fn absent_removal_cannot_duplicate_a_free_slot_or_overwrite_a_live_entry() {
    let mut table = LocalTable::<ExportId, _>::new();
    let id = table.push("old");
    assert_eq!(table.remove(id), Some("old"));
    assert_eq!(table.remove(id), None);
    assert_eq!(table.remove(ExportId::from_wire(100)), None);
    assert!(table.is_empty());
    assert_eq!(table.push("replacement"), id);
    let other = table.push("other");
    assert_ne!(other, id);
    assert_eq!(table.get(id), Some(&"replacement"));
    assert_eq!(table.get(other), Some(&"other"));
}

#[test]
fn adopted_insertion_is_checked_and_cannot_replace_an_owner() {
    let mut table = LocalTable::new();
    for id in [0, (1 << 30) - 1, 1 << 31, u32::MAX] {
        assert_eq!(
            table.insert_adopted(QuestionId::from_wire(id), "invalid"),
            Err("invalid")
        );
        assert!(table.is_empty());
    }
    for raw in [1 << 30, (1 << 31) - 1] {
        let id = QuestionId::from_wire(raw);
        assert_eq!(table.insert_adopted(id, "owner"), Ok(()));
        assert_eq!(table.insert_adopted(id, "intruder"), Err("intruder"));
        assert_eq!(table.get(id), Some(&"owner"));
        assert_eq!(table.remove(id), Some("owner"));
        assert_eq!(table.remove(id), None);
        assert_eq!(table.adopted_len(), 0);
        assert!(table.is_empty());
    }
    assert_eq!(table.push("low").to_wire(), 0);
}

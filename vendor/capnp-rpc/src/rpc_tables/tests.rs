use super::super::rpc_ids::ExportId;
use super::*;

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
    let low = table.push("low");
    assert_eq!(low.to_wire(), 0);
    table.erase(first);
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

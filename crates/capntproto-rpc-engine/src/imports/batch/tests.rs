use super::*;
use crate::ids::WireId;

#[test]
fn partial_capacity_rejection_and_drop_preserve_every_owner_and_reference() {
    let mut slots = [Slot::EMPTY];
    with_imports(&mut slots, |imports| {
        let id = ImportId::from_wire(7);
        let other = ImportId::from_wire(u32::MAX);
        let mut receipts = [ReceiveSlot::new(id, 17), ReceiveSlot::new(other, 23)];
        assert!(matches!(
            imports.prepare_receive(&mut receipts),
            Err(ReceiveError::Import(Error::Full))
        ));
        assert!(imports.is_empty());
        assert_eq!(receipts[0].owner(), Some(&17));
        assert_eq!(receipts[1].owner(), Some(&23));
        drop(imports.prepare_receive(&mut receipts[..1]).unwrap());
        assert!(imports.is_empty());
        assert!(receipts[0].local().is_none());
        imports
            .prepare_receive(&mut receipts[..1])
            .unwrap()
            .commit();
        let local = receipts[0].take_local().unwrap();
        assert!(receipts[0].owner().is_none());
        let mut aliases = [ReceiveSlot::alias(id), ReceiveSlot::new(other, 23)];
        assert!(imports.prepare_receive(&mut aliases).is_err());
        assert!(aliases[0].local().is_none());
        imports.release(local);
        let cleanup = imports.pending_release(0).unwrap();
        assert_eq!(cleanup.count(), 1);
        assert_eq!(cleanup.commit(), 17);
    })
    .unwrap();
}

#[test]
fn repeated_descriptors_and_tombstones_are_atomic_and_allocate_nothing() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [Slot::EMPTY, Slot::EMPTY];
        with_imports(&mut slots, |imports| {
            let id = ImportId::from_wire(7);
            let other = ImportId::from_wire(u32::MAX);
            let mut receipts = [
                ReceiveSlot::new(id, 17),
                ReceiveSlot::none(),
                ReceiveSlot::alias(id),
                ReceiveSlot::new(other, 23),
            ];
            imports.prepare_receive(&mut receipts).unwrap().commit();
            assert_eq!(imports.len(), 2);
            assert!(receipts[1].local().is_none());
            assert!(matches!(
                imports.prepare_receive(&mut receipts),
                Err(ReceiveError::OccupiedReceipt)
            ));
            for receipt in &mut receipts {
                if let Some(local) = receipt.take_local() {
                    imports.release(local);
                }
            }
            let cleanup = imports.pending_release(0).unwrap();
            assert_eq!(cleanup.count(), 2);
            assert_eq!(cleanup.commit(), 17);
            let cleanup = imports.pending_release(1).unwrap();
            assert_eq!(cleanup.count(), 1);
            assert_eq!(cleanup.commit(), 23);
            assert!(imports.is_empty());
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn revival_updates_generation_once_and_rejects_stale_handles() {
    let mut slots = [Slot::EMPTY];
    with_imports(&mut slots, |imports| {
        let id = ImportId::from_wire(7);
        let local = imports.admit(id, 17).unwrap();
        let stale = local.key();
        imports.release(local);
        let mut receipts = [ReceiveSlot::alias(id), ReceiveSlot::alias(id)];
        core::mem::forget(imports.prepare_receive(&mut receipts).unwrap());
        assert!(imports.resolve(stale).is_none());
        imports.prepare_receive(&mut receipts).unwrap().commit();
        assert!(imports.resolve(stale).is_none());
        let first = receipts[0].take_local().unwrap();
        let second = receipts[1].take_local().unwrap();
        assert_eq!(first.key(), second.key());
        assert_eq!(imports.get(&first), &17);
        imports.release(first);
        imports.release(second);
        let cleanup = imports.pending_release(0).unwrap();
        assert_eq!(cleanup.count(), 3);
        assert_eq!(cleanup.commit(), 17);
    })
    .unwrap();
}

#[test]
fn bad_owner_requests_never_run_destructors_or_change_imports() {
    let mut slots = [Slot::EMPTY];
    with_imports(&mut slots, |imports| {
        let id = ImportId::from_wire(7);
        let mut missing = [ReceiveSlot::alias(id)];
        assert!(matches!(
            imports.prepare_receive(&mut missing),
            Err(ReceiveError::MissingOwner)
        ));
        let local = imports.admit(id, 17).unwrap();
        let mut unexpected = [ReceiveSlot::new(id, 23)];
        assert!(matches!(
            imports.prepare_receive(&mut unexpected),
            Err(ReceiveError::UnexpectedOwner)
        ));
        assert_eq!(unexpected[0].take_owner(), Some(23));
        imports.release(local);
        assert_eq!(imports.pending_release(0).unwrap().count(), 1);
    })
    .unwrap();
}

use super::*;
use crate::ids::WireId;

#[test]
fn promise_resolution_and_replacement_admission_are_atomic_and_allocation_free() {
    let measured = allocation_counter::measure(|| {
        for available in 1..=2 {
            let mut slots = [const { Slot::EMPTY }; 2];
            with_imports(&mut slots[..available], |imports| {
                let id = ImportId::from_wire(17);
                let mut initial = [ReceiveSlot::new_promise(id, 41), ReceiveSlot::alias(id)];
                imports.prepare_receive(&mut initial).unwrap().commit();
                let original = initial[0].local().unwrap().key();
                let mut replacement = [ReceiveSlot::new(ImportId::from_wire(23), 47)];
                if available == 1 {
                    assert!(matches!(
                        imports.prepare_resolution(id, &mut replacement),
                        Err(ReceiveError::Import(Error::Full))
                    ));
                    assert_eq!(replacement[0].owner(), Some(&47));
                    // Failed replacement admission must leave Resolve retryable.
                    imports.prepare_resolution(id, &mut []).unwrap().commit();
                } else {
                    drop(imports.prepare_resolution(id, &mut replacement).unwrap());
                    assert_eq!(replacement[0].owner(), Some(&47));
                    let (key, receipts) = imports
                        .prepare_resolution(id, &mut replacement)
                        .unwrap()
                        .commit();
                    assert_eq!(key, Some(original));
                    assert_eq!(imports.get(receipts[0].local().unwrap()), &47);
                    imports.release(receipts[0].take_local().unwrap());
                }
                assert!(matches!(
                    imports.prepare_resolution(id, &mut []),
                    Err(ReceiveError::Promise(promise::Error::AlreadyResolved))
                ));
                let mut duplicate = [ReceiveSlot::alias_promise(id)];
                assert!(matches!(
                    imports.prepare_receive(&mut duplicate),
                    Err(ReceiveError::Promise(promise::Error::AlreadyResolved))
                ));
                for receipt in &mut initial {
                    imports.release(receipt.take_local().unwrap());
                }
                let cleanup = imports.pending_release(0).unwrap();
                assert_eq!(
                    cleanup.count(),
                    2,
                    "Resolve and its rejections add no promise references"
                );
                assert_eq!(cleanup.commit(), 41);
                // Only accepted Release permits the same ID to denote a new promise.
                let mut reused = [ReceiveSlot::new_promise(id, 53)];
                imports.prepare_receive(&mut reused).unwrap().commit();
                assert!(imports.resolve(original).is_none());
                imports.prepare_resolution(id, &mut []).unwrap().commit();
                imports.release(reused[0].take_local().unwrap());
                assert_eq!(imports.pending_release(0).unwrap().commit(), 53);
                if available == 2 {
                    assert_eq!(imports.pending_release(1).unwrap().commit(), 47);
                }
                assert!(imports.is_empty());
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn late_resolution_and_unobserved_promise_preserve_replacement_cleanup() {
    let mut slots = [const { Slot::EMPTY }; 2];
    with_imports(&mut slots, |imports| {
        let id = ImportId::from_wire(17);
        let mut initial = [ReceiveSlot::new_promise(id, 41)];
        imports.prepare_receive(&mut initial).unwrap().commit();
        imports.release(initial[0].take_local().unwrap());
        for released in [false, true] {
            if released {
                imports.pending_release(0).unwrap().commit();
            }
            let mut replacement = [ReceiveSlot::new(ImportId::from_wire(23), 47)];
            let resolution = imports.prepare_resolution(id, &mut replacement).unwrap();
            assert!(!resolution.is_observed());
            let (target, receipts) = resolution.commit();
            assert!(target.is_none());
            imports.release(receipts[0].take_local().unwrap());
            let index = usize::from(!released);
            let cleanup = imports.pending_release(index).unwrap();
            assert_eq!(cleanup.count(), 1);
            assert_eq!(cleanup.commit(), 47);
        }
        assert!(imports.is_empty());
    })
    .unwrap();
}

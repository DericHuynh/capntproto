use super::*;

#[test]
fn descriptor_multiplicities_are_exact_poisoned_and_do_not_change_reservations() {
    let mut slots = [const { Slot::EMPTY }; 2];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let a = batch.export(7).unwrap();
        let b = batch.export(9).unwrap();
        let _ = batch.grant(a.key()).unwrap();
        let mut small = [19];
        assert!(matches!(
            batch.descriptors(&mut small),
            Err(DescriptorError::ScratchTooSmall)
        ));
        assert_eq!(small, [19]);
        let mut scratch = [29; 3];
        for ids in [[0, 0, 1], [1, 0, 0], [0, 1, 0]] {
            let mut check = batch.descriptors(&mut scratch).unwrap();
            for id in ids {
                check.record(ExportId::from_wire(id)).unwrap();
            }
            assert_eq!(check.finish(), Ok(()));
            assert_eq!(scratch, [2, 1, 29]);
        }
        let mut check = batch.descriptors(&mut scratch).unwrap();
        check.record(a.id()).unwrap();
        check.record(b.id()).unwrap();
        assert_eq!(check.finish(), Err(DescriptorError::Missing));
        let mut check = batch.descriptors(&mut scratch).unwrap();
        assert_eq!(
            check.record(ExportId::from_wire(u32::MAX)),
            Err(DescriptorError::Unreserved)
        );
        assert_eq!(check.record(a.id()), Err(DescriptorError::Unreserved));
        assert_eq!(check.finish(), Err(DescriptorError::Unreserved));
        let mut check = batch.descriptors(&mut scratch).unwrap();
        check.record(a.id()).unwrap();
        check.record(a.id()).unwrap();
        check.record(b.id()).unwrap();
        assert_eq!(check.record(a.id()), Err(DescriptorError::Unreserved));
        assert_eq!(check.finish(), Err(DescriptorError::Unreserved));
        batch.commit();
        assert_eq!(exports.release(a.id(), 2), Ok(Some(7)));
        assert_eq!(exports.release(b.id(), 1), Ok(Some(9)));
    })
    .unwrap();
    let info = allocation_counter::measure(|| {
        with_exports::<(), _>(&mut [], |exports| {
            let batch = exports.batch();
            assert_eq!(batch.descriptors(&mut []).unwrap().finish(), Ok(()));
        })
        .unwrap();
    });
    assert_eq!(info.count_total, 0);
}

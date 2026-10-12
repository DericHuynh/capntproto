use super::*;
#[test]
fn release_batch_is_atomic_sticky_and_preserves_call_owners() {
    let measured = allocation_counter::measure(|| {
        for mode in 0..5 {
            let mut slots = [const { Slot::EMPTY }; 2];
            with_exports(&mut slots, |exports| {
                let mut batch = exports.batch();
                let first = batch.export(17).unwrap();
                let again = batch.grant(first.key()).unwrap();
                let second = batch.export(23).unwrap();
                batch.commit();
                let lease = exports.lease(first.id()).unwrap();
                {
                    let mut scratch = [0; 2];
                    let mut release = exports.prepare_releases(&mut scratch).unwrap();
                    release.record(first.id(), 1).unwrap();
                    if mode == 0 {
                        let _unaccepted = release;
                    } else if mode == 1 {
                        assert_eq!(
                            release.record(first.id(), 2),
                            Err(ReleaseError::Export(Error::OverRelease))
                        );
                        assert_eq!(release.record(second.id(), 1), Err(ReleaseError::Rejected));
                        assert!(matches!(release.finish(), Err(ReleaseError::Rejected)));
                    } else if mode == 2 {
                        assert_eq!(
                            release.record(ExportId::from_wire(u32::MAX), 1),
                            Err(ReleaseError::Export(Error::Missing))
                        );
                        assert!(matches!(release.finish(), Err(ReleaseError::Rejected)));
                    } else {
                        release.record(again.id(), 1).unwrap();
                        release.record(second.id(), 1).unwrap();
                        let pending = release.finish().unwrap();
                        if mode == 3 {
                            let _unaccepted = pending;
                        } else {
                            pending.accept();
                        }
                    }
                }
                if mode == 4 {
                    assert!(exports.resolve(first.key()).is_none());
                    assert!(exports.resolve(second.key()).is_none());
                    assert_eq!(exports.take_retired(1), Some(23));
                    assert_eq!(exports.take_retired(0), None);
                } else {
                    assert_eq!(exports.release(first.id(), 2), Ok(None));
                    assert_eq!(exports.release(second.id(), 1), Ok(Some(23)));
                }
                assert_eq!(exports.get(&lease), &17);
                assert_eq!(exports.release_local(lease), Some(17));
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}

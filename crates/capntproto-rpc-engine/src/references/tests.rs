use super::*;

#[test]
fn export_boundaries_and_terminal_release_are_atomic() {
    let mut refs = ExportRefs::new();
    assert_eq!(refs.release(0), Ok(false));
    refs.retain(u32::MAX - 1).unwrap();
    assert_eq!(refs.retain(1), Err(Error::Overflow));
    assert_eq!(refs.count(), u32::MAX);
    assert_eq!(refs.release(u32::MAX - 1), Ok(false));
    assert_eq!(refs.release(2), Err(Error::OverRelease));
    assert_eq!(refs.count(), 1);
    assert_eq!(refs.release(1), Ok(true));
    assert_eq!(refs.release(0), Err(Error::Released));
    assert_eq!(refs.retain(1), Err(Error::Released));
    assert_eq!(refs.count(), 0);
}

#[test]
fn import_cleanup_survives_backpressure_and_only_commits_once() {
    let mut refs = ImportRefs::new();
    assert!(refs.pending_release().is_none());
    refs.receive(u32::MAX).unwrap();
    assert_eq!(refs.receive(1), Err(Error::Overflow));
    for _ in 0..4 {
        let ticket = refs.pending_release().unwrap();
        assert_eq!(ticket.count(), u32::MAX);
        // The caller could not enqueue cleanup yet.
    }
    refs.pending_release().unwrap().commit();
    assert!(refs.pending_release().is_none());
    refs.receive(2).unwrap();
    assert_eq!(refs.pending_release().unwrap().count(), 2);
    refs.receive(3).unwrap();
    let ticket = refs.pending_release().unwrap();
    assert_eq!(ticket.count(), 5);
    ticket.commit();
    assert_eq!(refs.count(), 0);
}

#[test]
fn reference_transitions_do_not_allocate() {
    let counts = allocation_counter::measure(|| {
        let mut export = ExportRefs::new();
        let mut import = ImportRefs::new();
        for _ in 0..16 {
            export.retain(1).unwrap();
            import.receive(1).unwrap();
        }
        assert_eq!(export.release(u32::MAX), Err(Error::OverRelease));
        let ticket = import.pending_release().unwrap();
        assert_eq!(export.release(ticket.count()), Ok(false));
        ticket.commit();
        assert_eq!(export.release(1), Ok(true));
    });
    assert_eq!(counts.count_total, 0);
}

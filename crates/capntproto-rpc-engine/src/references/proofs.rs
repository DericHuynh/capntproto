//! One-step induction over every u32 count, plus explicit cleanup acceptance.
use super::*;

#[kani::proof]
fn export_reference_transition_preserves_ownership() {
    let before: u32 = kani::any();
    let amount: u32 = kani::any();
    let mut refs = ExportRefs { count: before };
    if kani::any() {
        let result = refs.retain(amount);
        if before == 0 {
            assert_eq!(result, Err(Error::Released));
            assert_eq!(refs.count(), before);
        } else if u64::from(before) + u64::from(amount) > u64::from(u32::MAX) {
            assert_eq!(result, Err(Error::Overflow));
            assert_eq!(refs.count(), before);
        } else {
            assert_eq!(result, Ok(()));
            assert_eq!(
                u64::from(refs.count()),
                u64::from(before) + u64::from(amount)
            );
        }
    } else {
        let result = refs.release(amount);
        if before == 0 {
            assert_eq!(result, Err(Error::Released));
            assert_eq!(refs.count(), before);
        } else if amount > before {
            assert_eq!(result, Err(Error::OverRelease));
            assert_eq!(refs.count(), before);
        } else {
            assert_eq!(result, Ok(amount == before));
            assert_eq!(refs.count(), before - amount);
        }
    }
}

#[kani::proof]
fn import_references_wait_for_release_acceptance() {
    let before: u32 = kani::any();
    let amount: u32 = kani::any();
    let mut refs = ImportRefs { count: before };
    let result = refs.receive(amount);
    if u64::from(before) + u64::from(amount) > u64::from(u32::MAX) {
        assert_eq!(result, Err(Error::Overflow));
        assert_eq!(refs.count(), before);
    } else {
        assert_eq!(result, Ok(()));
        assert_eq!(refs.count(), before + amount);
    }
    let pending = refs.count();
    {
        let ticket = refs.pending_release();
        assert_eq!(ticket.is_some(), pending != 0);
        if let Some(ticket) = ticket {
            assert_eq!(ticket.count(), pending);
        }
    }
    kani::assert(
        refs.count() == pending,
        "unaccepted release retains remote references",
    );
    if let Some(ticket) = refs.pending_release() {
        ticket.commit();
    }
    kani::assert(
        refs.count() == 0,
        "accepted release consumes pending references",
    );
    assert!(refs.pending_release().is_none());
}

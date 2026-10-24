extern crate std;

use super::*;

#[test]
fn retained_guards_and_storage_reuse_need_no_allocation() {
    let measured = allocation_counter::measure(|| {
        let count = Cell::new(0);
        let claimed = Cell::new(false);
        let ownership = ReturnOwnership::new(&count, &claimed);
        ownership.retain();
        ownership.retain();
        assert!(!ownership.release());
        assert!(!claimed.get());
        assert!(ownership.release());
        assert_eq!(count.get(), 0);
        assert!(claimed.get());

        // The previous answer has no remaining guards before caller reuse.
        claimed.set(false);
        ownership.retain();
        ownership.mark_sent();
        ownership.retain();
        assert!(!ownership.release());
        assert!(!ownership.release());
        assert_eq!(count.get(), 0);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn explicit_return_at_every_guard_release_boundary_suppresses_cancellation() {
    // Results, pipeline and task owners can end before or after explicit
    // completion. Count only the final unsatisfied release as a publisher.
    for guards in 1..=4 {
        for complete_before_release in 0..=guards {
            let count = Cell::new(0);
            let claimed = Cell::new(false);
            let ownership = ReturnOwnership::new(&count, &claimed);
            for _ in 0..guards {
                ownership.retain();
            }
            let mut publishers = 0;
            for release in 0..guards {
                if release == complete_before_release {
                    ownership.mark_sent();
                }
                publishers += usize::from(ownership.release());
                assert_eq!(count.get(), guards - release - 1);
            }
            assert_eq!(publishers, usize::from(complete_before_release == guards));
            assert!(claimed.get());
        }
    }
}

#[test]
fn final_release_claims_return_before_reentrant_cleanup() {
    let count = Cell::new(0);
    let claimed = Cell::new(false);
    let ownership = ReturnOwnership::new(&count, &claimed);
    ownership.retain();
    let publish = ownership.release();
    assert!(publish);
    assert!(claimed.get());
    // A reentrant observer cannot publish a second Return, even if it retains
    // and releases another context while the first publication is in progress.
    ownership.retain();
    assert!(!ownership.release());
    ownership.mark_sent();
    assert_eq!(count.get(), 0);
}

#[test]
fn invalid_reference_counts_panic_without_mutating_either_cell() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let count = Cell::new(usize::MAX);
    let claimed = Cell::new(false);
    let ownership = ReturnOwnership::new(&count, &claimed);
    assert!(catch_unwind(AssertUnwindSafe(|| ownership.retain())).is_err());
    assert_eq!(count.get(), usize::MAX);
    assert!(!claimed.get());
    count.set(0);
    assert!(catch_unwind(AssertUnwindSafe(|| ownership.release())).is_err());
    assert_eq!(count.get(), 0);
    assert!(!claimed.get());
}

use super::*;

#[test]
fn rejection_revocation_and_completion_allocate_nothing() {
    let allocations = allocation_counter::measure(|| {
        for revoke in [false, true] {
            let mut s = HandoffState::default();
            assert!(!s.drain());
            assert!(!s.lift());
            assert!(!s.accept(true));
            assert!(!s.invoke_direct());
            assert!(s.enqueue());
            assert!(s.provide());
            let before = s;
            assert!(!s.provide());
            assert!(!s.enqueue());
            assert!(!s.accept(false));
            assert_eq!(s, before);
            assert!(s.accept(true));
            assert!(s.accept(true));
            assert!(!s.lift());
            if revoke {
                assert!(s.revoke());
                assert!(!s.revoke());
                assert!(!s.accept(true));
            }
            assert!(s.drain());
            assert_eq!(s.lift(), !revoke);
            assert_eq!(s.invoke_direct(), !revoke);
            assert_eq!(s.direct_calls(), u64::from(!revoke));
            assert_eq!(s.drained(), 1);
            assert!(s.accepted());
        }
        // Defensive overflow branch; public transitions cannot create this state.
        let mut invalid = HandoffState {
            pending: 1,
            drained: u64::MAX,
            ..Default::default()
        };
        let before = invalid;
        assert!(!invalid.drain());
        assert_eq!(invalid, before);
    });
    assert_eq!(allocations.count_total, 0);
}

#[test]
fn enqueue_reserves_drain_capacity_before_reporting_success() {
    let mut state = HandoffState {
        drained: u64::MAX - 1,
        ..Default::default()
    };
    assert!(state.enqueue());
    let full = state;
    assert!(!state.enqueue());
    assert_eq!(state, full);
    assert!(state.drain());
    assert_eq!(state.drained(), u64::MAX);
    let exhausted = state;
    assert!(!state.enqueue());
    assert_eq!(state, exhausted);
    assert!(state.provide());
    assert!(state.accept(true));
    assert!(state.lift());
    assert!(state.direct_ready());

    // Pending-count overflow and direct-count overflow are separate limits.
    let mut pending = HandoffState {
        pending: u64::MAX,
        ..Default::default()
    };
    let before = pending;
    assert!(!pending.enqueue());
    assert_eq!(pending, before);
    assert!(pending.drain());
    assert!(!pending.enqueue());
    let mut direct = HandoffState {
        phase: HandoffPhase::Direct,
        direct: u64::MAX,
        ..Default::default()
    };
    let before = direct;
    assert!(!direct.invoke_direct());
    assert_eq!(direct, before);
    assert!(direct.accept(true)); // An idempotent accept does not allocate a new counter slot.
}

//! Inductive safety over every valid state, not a truncated counter model.
use super::*;

fn invariant(s: HandoffState) -> bool {
    u128::from(s.pending) + u128::from(s.drained) <= u128::from(u64::MAX)
        && (s.phase != HandoffPhase::Direct || s.pending == 0)
        && (s.phase == HandoffPhase::Direct || s.direct == 0)
}

#[kani::proof]
fn handoff_initial_state() {
    let s = HandoffState::default();
    assert!(invariant(s), "handoff reserves every future drain");
    assert_eq!(s.phase(), HandoffPhase::Proxying);
    assert!(!s.accepted() && !s.direct_ready() && !s.is_revoked());
    assert_eq!(s.pending(), 0);
    assert_eq!(s.drained(), 0);
    assert_eq!(s.direct_calls(), 0);
}

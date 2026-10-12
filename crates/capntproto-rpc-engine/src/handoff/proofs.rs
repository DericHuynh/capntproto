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

#[kani::proof]
fn handoff_inductive_transition() {
    let phase: u8 = kani::any();
    kani::assume(phase < 4);
    let mut s = HandoffState {
        phase: match phase {
            0 => HandoffPhase::Proxying,
            1 => HandoffPhase::Offered,
            2 => HandoffPhase::Embargoed,
            _ => HandoffPhase::Direct,
        },
        pending: kani::any(),
        drained: kani::any(),
        direct: kani::any(),
        revoked: kani::any(),
    };
    kani::assume(invariant(s));
    let before = s;
    let op: u8 = kani::any();
    kani::assume(op < 8);
    let authorized: bool = kani::any();
    let total = u128::from(before.pending) + u128::from(before.drained);
    let expected = match op {
        0 => phase == 0 && !before.revoked && total < u128::from(u64::MAX),
        1 => before.pending != 0,
        2 => phase == 0 && !before.revoked,
        3 => authorized && phase != 0 && !before.revoked,
        4 => phase == 2 && before.pending == 0 && !before.revoked,
        5 => phase == 3 && !before.revoked && before.direct < u64::MAX,
        6 => !before.revoked,
        _ => phase == 3 && !before.revoked,
    };
    let actual = match op {
        0 => s.enqueue(),
        1 => s.drain(),
        2 => s.provide(),
        3 => s.accept(authorized),
        4 => s.lift(),
        5 => s.invoke_direct(),
        6 => s.revoke(),
        _ => s.direct_ready(),
    };
    kani::assert(invariant(s), "handoff reserves every future drain");
    assert_eq!(actual, expected);
    if !actual || op == 7 {
        assert_eq!(s, before);
    }
    assert_eq!(
        u128::from(s.pending) + u128::from(s.drained),
        total + u128::from(op == 0 && actual)
    );
    assert_eq!(
        u128::from(s.drained),
        u128::from(before.drained) + u128::from(op == 1 && actual)
    );
    assert_eq!(
        u128::from(s.direct),
        u128::from(before.direct) + u128::from(op == 5 && actual)
    );
    assert_eq!(s.revoked, before.revoked || op == 6);
    let expected_phase = if actual {
        match op {
            2 => HandoffPhase::Offered,
            3 if phase == 1 => HandoffPhase::Embargoed,
            4 => HandoffPhase::Direct,
            _ => before.phase,
        }
    } else {
        before.phase
    };
    assert_eq!(s.phase(), expected_phase);
    assert_eq!(
        s.accepted(),
        matches!(s.phase(), HandoffPhase::Embargoed | HandoffPhase::Direct)
    );
    assert_eq!(
        s.direct_ready(),
        s.phase() == HandoffPhase::Direct && !s.revoked
    );
}

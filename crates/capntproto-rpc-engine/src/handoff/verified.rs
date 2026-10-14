// Contracts shared by implementation verification and temporal reasoning.
// This module is absent from ordinary Rust and embedded builds.
use verus_temporal_logic::{defs::*, rules::*};

verus! {
pub closed spec fn phase_spec(s: HandoffState) -> HandoffPhase { s.phase }
pub closed spec fn pending_spec(s: HandoffState) -> u64 { s.pending }
pub closed spec fn drained_spec(s: HandoffState) -> u64 { s.drained }
pub closed spec fn direct_spec(s: HandoffState) -> u64 { s.direct }
pub closed spec fn revoked_spec(s: HandoffState) -> bool { s.revoked }
pub closed spec fn accepted_spec(s: HandoffState) -> bool {
    s.phase == HandoffPhase::Embargoed || s.phase == HandoffPhase::Direct
}
pub closed spec fn ready_spec(s: HandoffState) -> bool {
    s.phase == HandoffPhase::Direct && !s.revoked
}
pub closed spec fn valid(s: HandoffState) -> bool {
    s.pending as int + s.drained as int <= u64::MAX
        && (s.phase == HandoffPhase::Direct ==> s.pending == 0)
        && (s.phase != HandoffPhase::Direct ==> s.direct == 0)
}
pub closed spec fn initial(s: HandoffState) -> bool {
    s.phase == HandoffPhase::Proxying && s.pending == 0 && s.drained == 0
        && s.direct == 0 && !s.revoked
}
pub closed spec fn admitted(s: HandoffState, op: u8, authorized: bool) -> bool {
    match op {
        0 => s.phase == HandoffPhase::Proxying && !s.revoked
            && (s.pending as int + s.drained as int) < u64::MAX,
        1 => s.pending > 0,
        2 => s.phase == HandoffPhase::Proxying && !s.revoked,
        3 => authorized && s.phase != HandoffPhase::Proxying && !s.revoked,
        4 => s.phase == HandoffPhase::Embargoed && s.pending == 0 && !s.revoked,
        5 => s.phase == HandoffPhase::Direct && !s.revoked && s.direct < u64::MAX,
        6 => !s.revoked,
        _ => false,
    }
}
pub closed spec fn transition(s: HandoffState, t: HandoffState, op: u8, authorized: bool, ok: bool) -> bool {
    &&& ok == admitted(s, op, authorized)
    &&& (!ok ==> t == s)
    &&& t.pending as int == s.pending as int + (if op == 0 && ok { 1int } else { 0int })
        - (if op == 1 && ok { 1int } else { 0int })
    &&& t.drained as int == s.drained as int + (if op == 1 && ok { 1int } else { 0int })
    &&& t.direct as int == s.direct as int + (if op == 5 && ok { 1int } else { 0int })
    &&& t.revoked == (s.revoked || op == 6)
    &&& t.phase == (if ok && op == 2 { HandoffPhase::Offered }
        else if ok && op == 3 && s.phase == HandoffPhase::Offered { HandoffPhase::Embargoed }
        else if ok && op == 4 { HandoffPhase::Direct } else { s.phase })
}

pub closed spec fn next(s: HandoffState, t: HandoffState) -> bool {
    s == t || exists|v: (u8, bool, bool)| v.0 <= 6 && #[trigger] transition(s, t, v.0, v.1, v.2)
}
pub proof fn preserves(s: HandoffState, t: HandoffState)
    requires valid(s), next(s, t),
    ensures valid(t),
{
    if s != t {
        let (op, auth, ok) = choose|v: (u8, bool, bool)| v.0 <= 6 && #[trigger] transition(s, t, v.0, v.1, v.2);
    }
}

/// Induction covers arbitrarily long executions, including rejection/stuttering.
pub proof fn safety(spec: TempPred<HandoffState>)
    requires spec.entails(lift_state(|s| initial(s))), spec.entails(always(lift_action(|s, t| next(s, t)))),
    ensures spec.entails(always(lift_state(|s| valid(s)))),
{
    assert forall|s, t| valid(s) && #[trigger] next(s, t) implies valid(t) by {
        preserves(s, t);
    };
    init_invariant(spec, |s| initial(s), |s, t| next(s, t), |s| valid(s));
}

pub closed spec fn waiting(s: HandoffState, n: nat) -> bool {
    valid(s) && s.phase == HandoffPhase::Embargoed && !s.revoked && s.pending == n
}
pub closed spec fn done(s: HandoffState) -> bool {
    s.revoked || s.phase == HandoffPhase::Direct
}
pub closed spec fn drain_step(s: HandoffState, t: HandoffState) -> bool {
    transition(s, t, 1, false, true)
}
pub closed spec fn lift_step(s: HandoffState, t: HandoffState) -> bool {
    transition(s, t, 4, false, true)
}

/// Unbounded induction on the admitted backlog. Weak fairness is a caller
/// obligation: continuously enabled draining/lifting must eventually be polled.
/// Revocation is a terminal outcome; this does not promise a network response.
pub proof fn progress(spec: TempPred<HandoffState>, n: nat)
    requires
        spec.entails(always(lift_action(|s, t| next(s, t)))),
        spec.entails(weak_fairness(|s, t| drain_step(s, t))),
        spec.entails(weak_fairness(|s, t| lift_step(s, t))),
    ensures spec.entails(lift_state(|s| waiting(s, n)).leads_to(lift_state(|s| done(s)))),
    decreases n,
{
    let p = |s| waiting(s, n);
    let end = |s| done(s);
    let forward = |s, t| if n == 0 { lift_step(s, t) } else { drain_step(s, t) };
    let q = |s| done(s) || (n > 0 && waiting(s, (n - 1) as nat));
    if n == 0 {
        assert(forward =~= (|s, t| lift_step(s, t)));
    } else {
        assert(forward =~= (|s, t| drain_step(s, t)));
    }
    assert forall|s, t| p(s) && #[trigger] next(s, t) implies p(t) || q(t) by {
        preserves(s, t);
        if s != t {
            let (op, auth, ok) = choose|v: (u8, bool, bool)| v.0 <= 6 && #[trigger] transition(s, t, v.0, v.1, v.2);
        }
    };
    assert forall|s: HandoffState| #[trigger] p(s) implies enabled(forward)(s) by {
        let t = if n == 0 {
            HandoffState { phase: HandoffPhase::Direct, ..s }
        } else {
            HandoffState { pending: (s.pending - 1) as u64, drained: (s.drained + 1) as u64, ..s }
        };
        assert(forward(s, t));
    };
    wf1(spec, |s, t| next(s, t), forward, p, q);
    if n > 0 {
        progress(spec, (n - 1) as nat);
        leads_to_self(lift_state(end));
        or_leads_to(spec, lift_state(end), lift_state(|s| waiting(s, (n - 1) as nat)), lift_state(end));
        temp_pred_equality(lift_state(q), lift_state(end).or(lift_state(|s| waiting(s, (n - 1) as nat))));
        leads_to_trans(spec, lift_state(p), lift_state(q), lift_state(end));
    } else {
        temp_pred_equality(lift_state(q), lift_state(end));
    }
}
}

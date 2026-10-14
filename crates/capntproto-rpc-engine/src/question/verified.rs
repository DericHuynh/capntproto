// Production transition contracts and an unbounded history of their effects.
// Erased from ordinary Rust, including no_std/no_alloc targets.
use verus_temporal_logic::{defs::*, rules::*};
verus! {
pub closed spec fn awaiting_spec(q: Question) -> bool { awaiting(q.state) }
pub closed spec fn pipeline_spec(q: Question) -> bool { pipeline(q.state) }
pub closed spec fn delivery_spec(q: Question) -> Result<Delivery, DuplicateReturn> { delivery(q.state) }
pub closed spec fn returned_contract(s: Question, t: Question, no_finish: bool, r: Result<Delivery, DuplicateReturn>) -> bool { returned_step(s.state, t.state, no_finish, r) }
pub closed spec fn release_contract(s: Question, t: Question, r: Release) -> bool { release_step(s.state, t.state, r) }
spec fn state_spec(q: Question) -> State { q.state }
spec fn awaiting(s: State) -> bool {
    s is Awaiting || s is Cancelled
}
spec fn pipeline(s: State) -> bool {
    s is Awaiting || s == State::Returned { finish: true }
}
spec fn delivery(s: State) -> Result<Delivery, DuplicateReturn> {
    match s {
        State::Awaiting => Ok(Delivery::Deliver),
        State::Cancelled => Ok(Delivery::Discard),
        _ => Err(DuplicateReturn),
    }
}
spec fn returned_step(s: State, t: State, no_finish: bool, r: Result<Delivery, DuplicateReturn>) -> bool {
    r == delivery(s) && t == match s {
        State::Awaiting => State::Returned { finish: !no_finish },
        State::Cancelled => State::Released,
        _ => s,
    }
}
spec fn release_effect(s: State) -> Release {
    match s {
        State::Awaiting => Release { finish: Some(Finish { release_result_caps: true }), reclaim: false },
        State::Cancelled => Release { finish: None, reclaim: false },
        State::Returned { finish } => Release {
            finish: if finish { Some(Finish { release_result_caps: false }) } else { None }, reclaim: true },
        State::Released => Release { finish: None, reclaim: true },
    }
}
spec fn release_step(s: State, t: State, r: Release) -> bool {
    r == release_effect(s) && t == match s {
        State::Awaiting => State::Cancelled,
        State::Cancelled => State::Cancelled,
        _ => State::Released,
    }
}

// Ghost history; these fields are never allocated by the RPC engine.
pub struct Trace {
    state: State,
    finishes: nat,
    returned: bool,
    released: bool,
}
pub closed spec fn initial(s: Trace) -> bool {
    s.finishes == 0 && !s.released &&
        ((s.state is Awaiting && !s.returned)
            || (s.state == State::Returned { finish: true } && s.returned))
}
pub closed spec fn valid(s: Trace) -> bool {
    &&& s.finishes <= 1
    &&& s.returned == !awaiting(s.state)
    &&& s.released == (s.state is Cancelled || s.state is Released)
    &&& (!s.released ==> s.finishes == 0)
    &&& (s.state is Cancelled ==> s.finishes == 1)
}
pub closed spec fn return_step(s: Trace, t: Trace, no_finish: bool) -> bool {
    &&& returned_step(s.state, t.state, no_finish, delivery(s.state))
    &&& t.finishes == s.finishes
    &&& t.returned == true
    &&& t.released == s.released
}
pub closed spec fn return_trace(s: Trace, t: Trace) -> bool {
    return_step(s, t, false) || return_step(s, t, true)
}
pub closed spec fn release_trace(s: Trace, t: Trace) -> bool {
    &&& release_step(s.state, t.state, release_effect(s.state))
    &&& t.finishes == s.finishes + if release_effect(s.state).finish is Some { 1nat } else { 0nat }
    &&& t.returned == s.returned
    &&& t.released == true
}
pub closed spec fn next(s: Trace, t: Trace) -> bool {
    s == t || return_trace(s, t) || release_trace(s, t)
}
pub proof fn preserves(s: Trace, t: Trace)
    requires valid(s), next(s, t),
    ensures valid(t),
{}
/// One local reference can emit at most one Finish, regardless of duplicate
/// operations. Reclamation requires Return (or the no-return constructor).
pub proof fn safety(spec: TempPred<Trace>)
    requires spec.entails(lift_state(|s| initial(s))), spec.entails(always(lift_action(|s, t| next(s, t)))),
    ensures spec.entails(always(lift_state(|s| valid(s)))),
{
    assert forall|s, t| valid(s) && #[trigger] next(s, t) implies valid(t) by { preserves(s, t); };
    init_invariant(spec, |s| initial(s), |s, t| next(s, t), |s| valid(s));
}
proof fn safe_effect(s: Trace, t: Trace, effect: Release)
    requires valid(s), release_step(s.state, t.state, effect),
    ensures effect.reclaim ==> s.returned,
        effect.finish is Some ==> !s.released && s.finishes == 0,
        effect.finish is Some ==> effect.finish.unwrap().release_result_caps == !s.returned,
{}

/// Conditional liveness: eventual Return processing and release of local
/// interest require caller fairness; no network-delivery assumption is hidden.
proof fn progress(spec: TempPred<Trace>)
    requires
        spec.entails(always(lift_action(|s, t| next(s, t)))),
        spec.entails(weak_fairness(|s, t| return_trace(s, t))),
        spec.entails(weak_fairness(|s, t| release_trace(s, t))),
    ensures spec.entails(lift_state(|s| valid(s)).leads_to(lift_state(|s: Trace| s.state is Released))),
{
    let n = |s, t| next(s, t);
    let p = |s| valid(s);
    let q = |s: Trace| valid(s) && s.returned;
    let end = |s: Trace| s.state is Released;
    let ret = |s, t| return_trace(s, t);
    let rel = |s, t| release_trace(s, t);
    assert forall|s, t| p(s) && #[trigger] n(s, t) implies p(t) || q(t) by { preserves(s, t); };
    assert forall|s: Trace| #[trigger] p(s) implies enabled(ret)(s) by {
        let t = Trace { state: match s.state {
            State::Awaiting => State::Returned { finish: true },
            State::Cancelled => State::Released,
            _ => s.state,
        }, returned: true, ..s };
        assert(ret(s, t));
    };
    wf1(spec, n, ret, p, q);
    assert forall|s, t| q(s) && #[trigger] n(s, t) implies q(t) || end(t) by { preserves(s, t); };
    assert forall|s: Trace| #[trigger] q(s) implies enabled(rel)(s) by {
        let t = Trace { state: State::Released, released: true,
            finishes: s.finishes + if release_effect(s.state).finish is Some { 1nat } else { 0nat }, ..s };
        assert(rel(s, t));
    };
    wf1(spec, n, rel, q, end);
    leads_to_trans(spec, lift_state(p), lift_state(q), lift_state(end));
}
}

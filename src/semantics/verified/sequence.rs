use verus_temporal_logic::{defs::*, rules::*};
verus! {
pub closed spec fn valid(next: Option<u64>) -> bool {
    next is Some ==> next.unwrap() > 0
}
pub closed spec fn allocation(s: Option<u64>, t: Option<u64>, r: Option<u64>) -> bool {
    r == s && t == match s {
        Some(n) => if n == u64::MAX { None } else { Some((n + 1) as u64) },
        None => None,
    }
}
// Ghost count includes identifiers whose observers have already been dropped.
pub struct Trace { next: Option<u64>, issued: nat }
pub closed spec fn initial(s: Trace) -> bool { s.next == Some(1) && s.issued == 0 }
pub closed spec fn valid_trace(s: Trace) -> bool {
    s.issued <= u64::MAX && s.next ==
        if s.issued == u64::MAX { None } else { Some((s.issued + 1) as u64) }
}
pub closed spec fn take(s: Trace, t: Trace) -> bool {
    allocation(s.next, t.next, s.next) &&
        t.issued == s.issued + if s.next is Some { 1nat } else { 0nat }
}
pub closed spec fn next(s: Trace, t: Trace) -> bool { s == t || take(s, t) }
proof fn preserves(s: Trace, t: Trace)
    requires valid_trace(s), next(s, t),
    ensures valid_trace(t), t.issued >= s.issued,
{}
proof fn fresh(s: Trace, t: Trace, r: Option<u64>)
    requires valid_trace(s), allocation(s.next, t.next, r),
    ensures valid(t.next), r is Some ==> r.unwrap() > s.issued,
        s.next is None ==> t.next is None && r is None,
{}
proof fn safety(spec: TempPred<Trace>)
    requires spec.entails(lift_state(|s| initial(s))), spec.entails(always(lift_action(|s,t| next(s,t)))),
    ensures spec.entails(always(lift_state(|s| valid_trace(s)))),
{
    assert forall|s,t| valid_trace(s) && #[trigger] next(s,t) implies valid_trace(t) by { preserves(s,t); };
    init_invariant(spec, |s| initial(s), |s,t| next(s,t), |s| valid_trace(s));
}
// Unlike the old three-ID model, this covers the entire u64 domain. Fair
// allocation is explicit: releasing an observer never advances this sequence.
proof fn progress(spec: TempPred<Trace>, remaining: nat)
    requires spec.entails(always(lift_action(|s,t| next(s,t)))),
        spec.entails(weak_fairness(|s,t| take(s,t))),
    ensures spec.entails(lift_state(|s: Trace| valid_trace(s) && u64::MAX - s.issued == remaining)
        .leads_to(lift_state(|s: Trace| s.next is None))),
    decreases remaining,
{
    let p = |s: Trace| valid_trace(s) && u64::MAX - s.issued == remaining;
    let end = |s: Trace| s.next is None;
    let advance = |s,t| take(s,t);
    if remaining == 0 {
        entails_implies_leads_to(spec, lift_state(p), lift_state(end));
    } else {
        let q = |s: Trace| valid_trace(s) && u64::MAX - s.issued == remaining - 1;
        assert forall|s,t| p(s) && #[trigger] next(s,t) implies p(t) || q(t) by { preserves(s,t); };
        assert forall|s: Trace| #[trigger] p(s) implies enabled(advance)(s) by {
            let t = Trace { next: if s.next.unwrap() == u64::MAX { None }
                else { Some((s.next.unwrap() + 1) as u64) }, issued: s.issued + 1 };
            assert(advance(s,t));
        };
        wf1(spec, |s,t| next(s,t), advance, p, q);
        progress(spec, (remaining - 1) as nat);
        leads_to_trans(spec, lift_state(p), lift_state(q), lift_state(end));
    }
}
}

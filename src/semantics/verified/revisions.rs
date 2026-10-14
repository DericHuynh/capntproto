use verus_temporal_logic::{defs::*, rules::*};
verus! {
pub closed spec fn value(r: Revision) -> u64 { r.0 }
pub closed spec fn head_spec(s: Revisions) -> Revision { s.head }
pub closed spec fn published_spec(s: Revisions) -> Revision { s.published }
pub closed spec fn valid(s: Revisions) -> bool { s.published.0 <= s.head.0 }
pub closed spec fn successor(s: Revision, r: Option<Revision>) -> bool {
    r == if s.0 == u64::MAX { None } else { Some(Revision((s.0 + 1) as u64)) }
}
pub closed spec fn construct(h: Revision, p: Revision, r: Option<Revisions>) -> bool {
    r == if p.0 <= h.0 { Some(Revisions { head: h, published: p }) } else { None }
}
pub closed spec fn stage_contract(s: Revisions, expected: Revision, r: Option<Revisions>) -> bool {
    r == if s.head == expected && s.head.0 < u64::MAX {
        Some(Revisions { head: Revision((s.head.0 + 1) as u64), ..s })
    } else { None }
}
pub closed spec fn publish_contract(s: Revisions, revision: Revision, expected: Revision, r: Option<Revisions>) -> bool {
    r == if expected == s.published && revision.0 > expected.0 && revision.0 <= s.head.0 {
        Some(Revisions { published: revision, ..s })
    } else { None }
}
pub closed spec fn next(s: Revisions, t: Revisions) -> bool {
    s == t || (exists|e| stage_contract(s,e,Some(t))) || (exists|r,e| publish_contract(s,r,e,Some(t)))
}
proof fn preserves(s: Revisions, t: Revisions)
    requires valid(s), next(s,t),
    ensures valid(t), t.head.0 >= s.head.0, t.published.0 >= s.published.0,
{}
proof fn safety(spec: TempPred<Revisions>)
    requires spec.entails(lift_state(|s| valid(s))), spec.entails(always(lift_action(|s,t| next(s,t)))),
    ensures spec.entails(always(lift_state(|s| valid(s)))),
{
    assert forall|s,t| valid(s) && #[trigger] next(s,t) implies valid(t) by { preserves(s,t); };
    init_invariant(spec, |s| valid(s), |s,t| next(s,t), |s| valid(s));
}
}

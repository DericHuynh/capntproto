use verus_temporal_logic::{defs::*, rules::*};
verus! {
pub closed spec fn state_spec(s: NativeStreamGate) -> NativeStreamState { s.state }
pub closed spec fn auth_step(s: NativeStreamGate, t: NativeStreamGate) -> bool {
    t.state == match s.state {
        NativeStreamState::Handshaking(StreamRole::Initiator) => NativeStreamState::SendPreface,
        NativeStreamState::Handshaking(StreamRole::Responder) => NativeStreamState::ReceivePreface,
        state => state,
    }
}
pub closed spec fn sent_step(s: NativeStreamGate, t: NativeStreamGate, r: bool) -> bool {
    r == (s.state is SendPreface) && t.state == if r { NativeStreamState::Ready } else { s.state }
}
pub closed spec fn receive_step(s: NativeStreamGate, t: NativeStreamGate, byte: u8, r: bool) -> bool {
    r == (s.state is ReceivePreface && byte == 82) && t.state ==
        if s.state is ReceivePreface { if byte == 82 { NativeStreamState::Ready } else { NativeStreamState::Failed } }
        else { s.state }
}
pub closed spec fn next(s: NativeStreamGate, t: NativeStreamGate) -> bool {
    s == t || auth_step(s,t) || (exists|r| sent_step(s,t,r)) || (exists|b,r| receive_step(s,t,b,r))
}
// Authorization here means the caller invoked authenticate(), not a proof of TLS.
pub struct Trace { gate: NativeStreamGate, authenticated: bool, preface: bool }
pub closed spec fn initial(s: Trace) -> bool {
    s.gate.state is Handshaking && !s.authenticated && !s.preface
}
pub closed spec fn valid_trace(s: Trace) -> bool {
    &&& (s.gate.state is Handshaking) == !s.authenticated
    &&& (s.gate.state is Ready) == s.preface
    &&& s.preface ==> s.authenticated
}
pub closed spec fn trace_step(s: Trace, t: Trace) -> bool {
    s == t || (auth_step(s.gate,t.gate) && t.authenticated && t.preface == s.preface)
        || (exists|r| sent_step(s.gate,t.gate,r) && t.authenticated == s.authenticated && t.preface == (s.preface || r))
        || (exists|b,r| receive_step(s.gate,t.gate,b,r) && t.authenticated == s.authenticated && t.preface == (s.preface || r))
}
proof fn terminal(s: NativeStreamGate, t: NativeStreamGate)
    requires next(s,t), s.state is Failed || s.state is Ready,
    ensures t == s,
{}
proof fn preserves(s: Trace, t: Trace)
    requires valid_trace(s), trace_step(s,t),
    ensures valid_trace(t), s.gate.state is Failed ==> t.gate.state is Failed,
{}
proof fn safety(spec: TempPred<Trace>)
    requires spec.entails(lift_state(|s| initial(s))), spec.entails(always(lift_action(|s,t| trace_step(s,t)))),
    ensures spec.entails(always(lift_state(|s| valid_trace(s)))),
{
    assert forall|s,t| valid_trace(s) && #[trigger] trace_step(s,t) implies valid_trace(t) by { preserves(s,t); };
    init_invariant(spec, |s| initial(s), |s,t| trace_step(s,t), |s| valid_trace(s));
}
}

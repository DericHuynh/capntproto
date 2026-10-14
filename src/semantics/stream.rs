#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("verified/stream.rs");

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamRole {
    Initiator,
    Responder,
}

/// Exactly one state of the authenticated stream-opening gate. Failed preface
/// validation is terminal; another authentication notification cannot reset it.
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeStreamState {
    Handshaking(StreamRole),
    SendPreface,
    ReceivePreface,
    Ready,
    Failed,
}

/// Publication gate for stream 0 in native multiparty Native RPC. The stream is
/// initiator-opened even when the first RPC call comes from the responder.
///
/// ```compile_fail,E0451
/// use capntproto::semantics::{NativeStreamGate, NativeStreamState};
/// let gate = NativeStreamGate { state: NativeStreamState::Ready };
/// ```
/// ```compile_fail,E0308
/// let gate = capntproto::semantics::NativeStreamGate::new(false);
/// ```
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeStreamGate {
    state: NativeStreamState,
}
#[cfg_attr(capntproto_verus, verus_spec(r => ensures state_spec(r) == NativeStreamState::Handshaking(role)))]
fn initial_gate(role: StreamRole) -> NativeStreamGate {
    NativeStreamGate {
        state: NativeStreamState::Handshaking(role),
    }
}
impl NativeStreamGate {
    #[must_use]
    pub fn new(role: StreamRole) -> Self {
        initial_gate(role)
    }
    #[cfg_attr(capntproto_verus, verus_spec(ensures auth_step(*old(self), *final(self))))]
    pub fn authenticate(&mut self) {
        self.state = match self.state {
            NativeStreamState::Handshaking(StreamRole::Initiator) => NativeStreamState::SendPreface,
            NativeStreamState::Handshaking(StreamRole::Responder) => {
                NativeStreamState::ReceivePreface
            }
            state => state,
        };
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (state_spec(*self) is SendPreface)))]
    pub fn needs_send(&self) -> bool {
        matches!(self.state, NativeStreamState::SendPreface)
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (state_spec(*self) is ReceivePreface)))]
    pub fn needs_receive(&self) -> bool {
        matches!(self.state, NativeStreamState::ReceivePreface)
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures sent_step(*old(self), *final(self), r)))]
    pub fn sent(&mut self) -> bool {
        if !self.needs_send() {
            return false;
        }
        self.state = NativeStreamState::Ready;
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures receive_step(*old(self), *final(self), byte, r)))]
    pub fn receive(&mut self, byte: u8) -> bool {
        if !self.needs_receive() {
            return false;
        }
        self.state = if byte == b'R' {
            NativeStreamState::Ready
        } else {
            NativeStreamState::Failed
        };
        self.ready()
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (state_spec(*self) is Ready)))]
    pub fn ready(&self) -> bool {
        matches!(self.state, NativeStreamState::Ready)
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == state_spec(*self)))]
    pub fn state(&self) -> NativeStreamState {
        self.state
    }
}

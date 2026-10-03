#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamRole {
    Initiator,
    Responder,
}

/// Exactly one state of the authenticated stream-opening gate. Failed preface
/// validation is terminal; another authentication notification cannot reset it.
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
/// use reproto::semantics::{NativeStreamGate, NativeStreamState};
/// let gate = NativeStreamGate { state: NativeStreamState::Ready };
/// ```
/// ```compile_fail,E0308
/// let gate = reproto::semantics::NativeStreamGate::new(false);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeStreamGate {
    state: NativeStreamState,
}
impl NativeStreamGate {
    #[must_use]
    pub fn new(role: StreamRole) -> Self {
        Self {
            state: NativeStreamState::Handshaking(role),
        }
    }
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
    pub fn needs_send(&self) -> bool {
        self.state == NativeStreamState::SendPreface
    }
    #[must_use]
    pub fn needs_receive(&self) -> bool {
        self.state == NativeStreamState::ReceivePreface
    }
    #[must_use]
    pub fn sent(&mut self) -> bool {
        if !self.needs_send() {
            return false;
        }
        self.state = NativeStreamState::Ready;
        true
    }
    #[must_use]
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
    pub fn ready(&self) -> bool {
        self.state == NativeStreamState::Ready
    }
    #[must_use]
    pub fn state(&self) -> NativeStreamState {
        self.state
    }
}

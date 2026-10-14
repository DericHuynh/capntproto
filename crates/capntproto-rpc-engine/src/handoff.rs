#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("handoff/verified.rs");

/// Progress of an application handoff. Revocation is orthogonal: a revoked
/// handoff retains its phase and may still drain already admitted proxy calls.
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub enum HandoffPhase {
    #[default]
    Proxying,
    Offered,
    Embargoed,
    Direct,
}

/// Pure transition policy, not evidence of transport authentication. Live
/// introductions use the hosted `capntproto::handoff` serving boundary.
///
/// Fields are private and this type cannot be deserialized. Acceptance is
/// derived from the phase; callers cannot create a direct, unaccepted state.
///
/// ```compile_fail,E0616
/// let mut state = capntproto_rpc_engine::handoff::HandoffState::default();
/// state.phase = capntproto_rpc_engine::handoff::HandoffPhase::Direct;
/// ```
/// ```compile_fail,E0451
/// use capntproto_rpc_engine::handoff::{HandoffPhase, HandoffState};
/// let state = HandoffState {
///     phase: HandoffPhase::Direct, pending: 1, ..Default::default()
/// };
/// ```
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct HandoffState {
    phase: HandoffPhase,
    pending: u64,
    drained: u64,
    direct: u64,
    revoked: bool,
}
impl Default for HandoffState {
    fn default() -> Self {
        initial_state()
    }
}
#[cfg_attr(capntproto_verus, verus_spec(r => ensures initial(r)))]
fn initial_state() -> HandoffState {
    HandoffState {
        phase: HandoffPhase::Proxying,
        pending: 0,
        drained: 0,
        direct: 0,
        revoked: false,
    }
}
impl HandoffState {
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == phase_spec(*self)))]
    pub fn phase(&self) -> HandoffPhase {
        self.phase
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == pending_spec(*self)))]
    pub fn pending(&self) -> u64 {
        self.pending
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == drained_spec(*self)))]
    pub fn drained(&self) -> u64 {
        self.drained
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == direct_spec(*self)))]
    pub fn direct_calls(&self) -> u64 {
        self.direct
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == accepted_spec(*self)))]
    pub fn accepted(&self) -> bool {
        matches!(self.phase, HandoffPhase::Embargoed | HandoffPhase::Direct)
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == revoked_spec(*self)))]
    pub fn is_revoked(&self) -> bool {
        self.revoked
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == ready_spec(*self)))]
    pub fn direct_ready(&self) -> bool {
        matches!(self.phase, HandoffPhase::Direct) && !self.revoked
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 0, false, r)))]
    pub fn enqueue(&mut self) -> bool {
        if !matches!(self.phase, HandoffPhase::Proxying) || self.revoked {
            return false;
        }
        // Reserve space in the cumulative drain counter before admission.
        // Otherwise a valid pending call could become impossible to finish.
        let Some(pending) = self.pending.checked_add(1) else {
            return false;
        };
        if pending.checked_add(self.drained).is_none() {
            return false;
        }
        self.pending = pending;
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 1, false, r)))]
    pub fn drain(&mut self) -> bool {
        if self.pending == 0 {
            return false;
        }
        let Some(drained) = self.drained.checked_add(1) else {
            return false;
        };
        self.pending -= 1;
        self.drained = drained;
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 2, false, r)))]
    pub fn provide(&mut self) -> bool {
        if !matches!(self.phase, HandoffPhase::Proxying) || self.revoked {
            return false;
        }
        self.phase = HandoffPhase::Offered;
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 3, authorized, r)))]
    pub fn accept(&mut self, authorized: bool) -> bool {
        if !authorized || self.revoked || matches!(self.phase, HandoffPhase::Proxying) {
            return false;
        }
        if matches!(self.phase, HandoffPhase::Offered) {
            self.phase = HandoffPhase::Embargoed;
        }
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 4, false, r)))]
    pub fn lift(&mut self) -> bool {
        if !matches!(self.phase, HandoffPhase::Embargoed) || self.pending != 0 || self.revoked {
            return false;
        }
        self.phase = HandoffPhase::Direct;
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 5, false, r)))]
    pub fn invoke_direct(&mut self) -> bool {
        if !self.direct_ready() {
            return false;
        }
        let Some(direct) = self.direct.checked_add(1) else {
            return false;
        };
        self.direct = direct;
        true
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(*old(self)), ensures transition(*old(self), *final(self), 6, false, r)))]
    pub fn revoke(&mut self) -> bool {
        if self.revoked {
            return false;
        }
        self.revoked = true;
        true
    }
}

#[cfg(test)]
#[path = "handoff/tests.rs"]
mod tests;

#[cfg(kani)]
#[path = "handoff/proofs.rs"]
mod proofs;

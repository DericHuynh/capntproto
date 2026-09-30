/// Progress of an application handoff. Revocation is orthogonal: a revoked
/// handoff retains its phase and may still drain already admitted proxy calls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub enum HandoffPhase {
    #[default]
    Proxying,
    Offered,
    Embargoed,
    Direct,
}

/// Pure transition policy, not evidence of transport authentication. Live
/// introductions use the restricted serving boundary in `handoff`.
///
/// Fields are private and this type cannot be deserialized. Acceptance is
/// derived from the phase; callers cannot create a direct, unaccepted state.
///
/// ```compile_fail,E0616
/// let mut state = reproto::semantics::HandoffState::default();
/// state.phase = reproto::semantics::HandoffPhase::Direct;
/// ```
/// ```compile_fail,E0451
/// use reproto::semantics::{HandoffPhase, HandoffState};
/// let state = HandoffState {
///     phase: HandoffPhase::Direct, pending: 1, ..Default::default()
/// };
/// ```
/// ```compile_fail,E0277
/// let state: reproto::semantics::HandoffState = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct HandoffState {
    phase: HandoffPhase,
    pending: u64,
    drained: u64,
    direct: u64,
    revoked: bool,
}
impl HandoffState {
    #[must_use]
    pub fn phase(&self) -> HandoffPhase {
        self.phase
    }
    #[must_use]
    pub fn pending(&self) -> u64 {
        self.pending
    }
    #[must_use]
    pub fn drained(&self) -> u64 {
        self.drained
    }
    #[must_use]
    pub fn direct_calls(&self) -> u64 {
        self.direct
    }
    #[must_use]
    pub fn accepted(&self) -> bool {
        matches!(self.phase, HandoffPhase::Embargoed | HandoffPhase::Direct)
    }
    #[must_use]
    pub fn is_revoked(&self) -> bool {
        self.revoked
    }
    #[must_use]
    pub fn direct_ready(&self) -> bool {
        self.phase == HandoffPhase::Direct && !self.revoked
    }
    #[must_use]
    pub fn enqueue(&mut self) -> bool {
        if self.phase != HandoffPhase::Proxying || self.revoked {
            return false;
        }
        // Reserve space in the cumulative drain counter before admission.
        // Otherwise a valid pending call could become impossible to finish.
        let Some(pending) = self
            .pending
            .checked_add(1)
            .filter(|pending| pending.checked_add(self.drained).is_some())
        else {
            return false;
        };
        self.pending = pending;
        true
    }
    #[must_use]
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
    pub fn provide(&mut self) -> bool {
        if self.phase != HandoffPhase::Proxying || self.revoked {
            return false;
        }
        self.phase = HandoffPhase::Offered;
        true
    }
    #[must_use]
    pub fn accept(&mut self, authorized: bool) -> bool {
        if !authorized || self.revoked || self.phase == HandoffPhase::Proxying {
            return false;
        }
        if self.phase == HandoffPhase::Offered {
            self.phase = HandoffPhase::Embargoed;
        }
        true
    }
    #[must_use]
    pub fn lift(&mut self) -> bool {
        if self.phase != HandoffPhase::Embargoed || self.pending != 0 || self.revoked {
            return false;
        }
        self.phase = HandoffPhase::Direct;
        true
    }
    #[must_use]
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
    pub fn revoke(&mut self) -> bool {
        if self.revoked {
            return false;
        }
        self.revoked = true;
        true
    }
}

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;

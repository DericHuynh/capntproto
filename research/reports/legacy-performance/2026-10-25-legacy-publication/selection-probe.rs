#![no_std]
// Exact old helper and new publication type; standalone selection wrappers.
#[inline(always)]
fn postpone_last(busy: bool, turns: &mut u8, deferred: &mut bool) -> bool {
    // These inputs are plain values. Select both next-state fields before
    // branching around the callback, leaving one control-flow decision.
    let postpone = (busy & (*turns < 32)) | !*deferred;
    // Keep the selection as a mask: LLVM can turn an equivalent conditional
    // expression back into short-circuit branches around these stores.
    let keep_turns = 0u8.wrapping_sub(u8::from(postpone));
    *turns = turns.saturating_add(1) & keep_turns;
    *deferred = postpone;
    postpone
}
/// Deferred-publication fairness state. Zero has not yielded to a response
/// consumer; 1..=32 counts deferred turns. No separate deferred flag is needed.
#[derive(Default)]
pub struct Publication {
    turns: u8,
}

impl Publication {
    pub const fn new() -> Self {
        Self { turns: 0 }
    }

    #[inline(always)]
    fn postpone(&mut self, busy: bool) -> bool {
        let postpone = (self.turns == 0) | (busy & (self.turns < 32));
        // The increment is selected only below 32. Discarding the other value
        // avoids both an overflow branch and a redundant saturation select.
        self.turns = core::hint::select_unpredictable(postpone, self.turns.wrapping_add(1), 0);
        postpone
    }
}

#[no_mangle]
pub fn baseline(busy: bool, turns: &mut u8, deferred: &mut bool, defer: fn()) {
    if postpone_last(busy, turns, deferred) { defer(); }
}
#[no_mangle]
pub fn candidate(busy: bool, publication: &mut Publication, defer: fn()) {
    if publication.postpone(busy) { defer(); }
}

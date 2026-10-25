#![no_std]
// The candidate helper below is extracted verbatim from scheduler.rs.
// This is a local instruction-selection probe, not the complete scheduler.
#[inline(always)]
fn postpone_last(busy: bool, turns: &mut u8, deferred: &mut bool) -> bool {
    // These inputs are plain values. Select both next-state fields before
    // branching around the callback, leaving one control-flow decision.
    let postpone = (busy & (*turns < 32)) | !*deferred;
    *turns = if postpone { turns.saturating_add(1) } else { 0 };
    *deferred = postpone;
    postpone
}
#[no_mangle]
pub fn candidate(busy: bool, turns: &mut u8, deferred: &mut bool, defer: fn()) {
    if postpone_last(busy, turns, deferred) { defer(); }
}
#[no_mangle]
pub fn baseline(busy: bool, turns: &mut u8, deferred: &mut bool, defer: fn()) {
    if (busy && *turns < 32) || !*deferred {
        *turns = turns.saturating_add(1);
        *deferred = true;
        defer();
    } else {
        *turns = 0;
        *deferred = false;
    }
}

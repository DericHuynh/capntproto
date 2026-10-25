#![no_std]
// The candidate helper below is extracted verbatim from scheduler.rs.
// This is a local instruction-selection probe, not the complete scheduler.
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

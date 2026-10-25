#![no_std]
#![allow(dead_code)]
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
        let limit = if busy { 32 } else { 1 };
        // The caller already branches around its callback. Share that decision
        // with the counter update instead of forcing a separate selection.
        if self.turns < limit {
            self.turns = self.turns.wrapping_add(1);
            true
        } else {
            self.turns = 0;
            false
        }
    }
}

#[no_mangle]
pub fn selection(busy: bool, publication: &mut Publication, defer: fn()) {
    if publication.postpone(busy) { defer(); }
}

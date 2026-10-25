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
        let postpone = (self.turns == 0) | (busy & (self.turns < 32));
        // The increment is selected only below 32. Discarding the other value
        // avoids both an overflow branch and a redundant saturation select.
        self.turns = core::hint::select_unpredictable(postpone, self.turns.wrapping_add(1), 0);
        postpone
    }
}

#[no_mangle]
pub fn selection(busy: bool, publication: &mut Publication, defer: fn()) {
    if publication.postpone(busy) { defer(); }
}

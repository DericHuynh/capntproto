//! Return ownership borrowed from an existing answer record.
use core::cell::Cell;

/// Counts retained result/pipeline guards and claims the final unsent Return.
///
/// This view stores no owner or protocol entry. The caller retains the answer
/// storage and pairs each successful `retain` with one `release`. Returning
/// `true` from `release` claims publication before any caller cleanup can
/// reenter; it does not send a message or retire the answer itself. Explicit
/// completion calls `mark_sent` so later guard destruction cannot send again.
///
/// End all previous guards before resetting these cells for another answer.
pub struct ReturnOwnership<'a> {
    guards: &'a Cell<usize>,
    claimed: &'a Cell<bool>,
}

impl<'a> ReturnOwnership<'a> {
    #[inline]
    pub const fn new(guards: &'a Cell<usize>, claimed: &'a Cell<bool>) -> Self {
        Self { guards, claimed }
    }

    /// Add one retained guard. Panics on overflow without changing the state.
    #[inline]
    pub fn retain(&self) {
        self.guards.set(
            self.guards
                .get()
                .checked_add(1)
                .expect("return guard overflow"),
        );
    }

    /// Release one guard and claim an unsent Return if it was the last guard.
    /// The caller must publish or perform terminal cleanup when this is true.
    /// Panics if no guard was retained, without changing either cell.
    #[inline]
    pub fn release(&self) -> bool {
        let remaining = self
            .guards
            .get()
            .checked_sub(1)
            .expect("return guard underflow");
        self.guards.set(remaining);
        remaining == 0 && !self.claimed.replace(true)
    }

    /// Record explicit completion without waiting for retained guards to end.
    #[inline]
    pub fn mark_sent(&self) {
        self.claimed.set(true);
    }
}

#[cfg(test)]
mod tests;

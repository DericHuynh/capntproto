//! Incoming Call backpressure. No IO, allocator, clock or executor is required.
//! Charge admitted Calls until their Return is published. The caller checks
//! `paused` between messages and supplies wakeups when policy/credit changes.
use super::Error;
use core::cell::Cell;

pub struct Incoming {
    words: Cell<usize>,
    limit: Cell<usize>,
    paused: Cell<bool>,
}
impl Incoming {
    pub const fn new(limit: usize) -> Self {
        Self {
            words: Cell::new(0),
            limit: Cell::new(limit),
            paused: Cell::new(false),
        }
    }
    pub fn words(&self) -> usize {
        self.words.get()
    }
    pub fn limit(&self) -> usize {
        self.limit.get()
    }
    pub fn set_limit(&self, words: usize) {
        self.limit.set(words);
    }
    /// Admit the message that crosses the threshold, then pause all input.
    /// Once paused, resume strictly below the limit. At zero, a later limit
    /// increase is required even after every charged call completes.
    pub fn paused(&self) -> bool {
        let paused = if self.paused.get() {
            self.words.get() >= self.limit.get()
        } else {
            self.words.get() > self.limit.get()
        };
        self.paused.set(paused);
        paused
    }
    fn add(&self, words: usize) -> Result<(), Error> {
        let total = self
            .words
            .get()
            .checked_add(words)
            .ok_or(Error::CounterOverflow)?;
        self.words.set(total);
        if total > self.limit.get() {
            self.paused.set(true);
        }
        Ok(())
    }
    fn release(&self, words: usize) {
        self.words.set(self.words.get() - words);
    }
    pub fn charge(&self, words: usize) -> Result<Charge<'_>, Error> {
        self.add(words)?;
        Ok(Charge { owner: self, words })
    }
    #[cfg(feature = "alloc")]
    pub fn charge_owned(self: &alloc::rc::Rc<Self>, words: usize) -> Result<OwnedCharge, Error> {
        self.add(words)?;
        Ok(OwnedCharge {
            owner: self.clone(),
            words,
        })
    }
}

/// An affine charge bound to its issuing counter. Drop releases it once.
#[must_use = "retain until Return publication or terminal cleanup"]
pub struct Charge<'a> {
    owner: &'a Incoming,
    words: usize,
}
impl Drop for Charge<'_> {
    fn drop(&mut self) {
        self.owner.release(self.words);
    }
}
/// Hosted ownership over the same counter; construction clones an existing Rc.
#[cfg(feature = "alloc")]
#[must_use = "retain until Return publication or terminal cleanup"]
pub struct OwnedCharge {
    owner: alloc::rc::Rc<Incoming>,
    words: usize,
}
#[cfg(feature = "alloc")]
impl Drop for OwnedCharge {
    fn drop(&mut self) {
        self.owner.release(self.words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_credit_crosses_then_resumes_strictly_below_threshold() {
        let flow = Incoming::new(8);
        let first = flow.charge(8).unwrap();
        assert!(!flow.paused());
        let second = flow.charge(1).unwrap();
        assert!(flow.paused());
        drop(second);
        assert!(flow.paused(), "equality cannot resume a paused reader");
        drop(first);
        assert!(!flow.paused());
        flow.set_limit(0);
        let zero = flow.charge(1).unwrap();
        assert!(flow.paused());
        drop(zero);
        assert!(flow.paused());
        flow.set_limit(1);
        assert!(!flow.paused());
    }
    #[test]
    fn incoming_overflow_preserves_existing_charge_and_other_counters() {
        let flow = Incoming::new(usize::MAX);
        let other = Incoming::new(0);
        let charge = flow.charge(usize::MAX).unwrap();
        assert!(matches!(flow.charge(1), Err(Error::CounterOverflow)));
        assert_eq!(flow.words(), usize::MAX);
        assert_eq!(other.words(), 0);
        drop(charge);
        assert_eq!(flow.words(), 0);
    }
    #[cfg(feature = "alloc")]
    #[test]
    fn borrowed_and_owned_charges_share_accounting_without_retaining_scope() {
        let flow = alloc::rc::Rc::new(Incoming::new(2));
        let borrowed = flow.charge(1).unwrap();
        let owned = flow.charge_owned(2).unwrap();
        assert!(flow.paused());
        drop(borrowed);
        assert!(flow.paused());
        drop(owned);
        assert_eq!(flow.words(), 0);
        assert!(!flow.paused());
        assert_eq!(alloc::rc::Rc::strong_count(&flow), 1);
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    #[kani::proof]
    fn incoming_charges_preserve_counts_and_hysteresis() {
        let limit: usize = kani::any();
        let a: usize = kani::any();
        let b: usize = kani::any();
        let flow = Incoming::new(limit);
        let first = flow.charge(a).unwrap();
        assert_eq!(flow.paused(), a > limit);
        match flow.charge(b) {
            Ok(second) => {
                assert_eq!(flow.words(), a + b);
                let paused = flow.paused();
                drop(second);
                assert_eq!(flow.words(), a);
                kani::assert(
                    flow.paused() == if paused { a >= limit } else { a > limit },
                    "paused input resumes strictly below its limit",
                );
            }
            Err(_) => assert_eq!(flow.words(), a),
        }
        drop(first);
        assert_eq!(flow.words(), 0);
        flow.set_limit(1);
        assert!(!flow.paused());
    }
}

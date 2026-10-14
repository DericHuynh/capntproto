//! Caller-owned outgoing admission. Reserving never waits or allocates; the
//! optional hosted guard only clones an existing counter owner.
use core::cell::Cell;

pub struct Outgoing {
    limit: Cell<usize>,
    used: Cell<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;
impl Outgoing {
    pub const fn new(limit: usize) -> Self {
        Self {
            limit: Cell::new(limit),
            used: Cell::new(0),
        }
    }
    pub fn limit(&self) -> usize {
        self.limit.get()
    }
    pub fn used(&self) -> usize {
        self.used.get()
    }
    pub fn set_limit(&self, limit: usize) {
        self.limit.set(limit);
    }
    fn acquire(&self) -> Result<(), Full> {
        let next = super::policy::acquire(self.used.get(), self.limit.get()).ok_or(Full)?;
        self.used.set(next);
        Ok(())
    }
    fn release(&self) {
        self.used.set(super::policy::release(self.used.get(), 1));
    }
    pub fn reserve(&self) -> Result<Permit<'_>, Full> {
        self.acquire()?;
        Ok(Permit(self))
    }
    #[cfg(feature = "alloc")]
    pub fn reserve_owned(self: &alloc::rc::Rc<Self>) -> Result<OwnedPermit, Full> {
        self.acquire()?;
        Ok(OwnedPermit(self.clone()))
    }
}
#[must_use = "hold through request, question and output completion"]
pub struct Permit<'a>(&'a Outgoing);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}
/// Cloning an Rc around this guard shares one charge, rather than reserving
/// again. Dropping it only changes the counter; it invokes no user callbacks.
#[cfg(feature = "alloc")]
pub struct OwnedPermit(alloc::rc::Rc<Outgoing>);
#[cfg(feature = "alloc")]
impl Drop for OwnedPermit {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reduction_preserves_accepted_work_and_zero_rejects_new_work() {
        let counter = Outgoing::new(2);
        let first = counter.reserve().unwrap();
        let second = counter.reserve().unwrap();
        assert!(counter.reserve().is_err());
        counter.set_limit(0);
        drop(first);
        assert_eq!(counter.used(), 1);
        assert!(counter.reserve().is_err());
        drop(second);
        assert_eq!(counter.used(), 0);
        assert!(counter.reserve().is_err());
        counter.set_limit(1);
        assert!(counter.reserve().is_ok());
        assert_eq!(counter.used(), 0);
    }
    #[cfg(feature = "alloc")]
    #[test]
    fn shared_request_question_and_writer_release_one_charge() {
        let counter = alloc::rc::Rc::new(Outgoing::new(1));
        let request = alloc::rc::Rc::new(counter.reserve_owned().unwrap());
        let question = request.clone();
        let writer = request.clone();
        drop(request);
        drop(question);
        assert!(counter.reserve().is_err());
        drop(writer);
        assert_eq!(counter.used(), 0);
    }
}
#[cfg(kani)]
mod proofs {
    use super::*;
    #[kani::proof]
    fn outgoing_full_width_admission_never_wraps_or_revokes_accepted_credit() {
        let counter = Outgoing::new(kani::any());
        let used: usize = kani::any();
        counter.used.set(used);
        match counter.reserve() {
            Ok(permit) => {
                kani::assert(
                    used < counter.limit(),
                    "accepted admission stays within its limit",
                );
                kani::assert(counter.used() > used, "admission never wraps its counter");
                counter.set_limit(0);
                kani::assert(
                    counter.reserve().is_err(),
                    "zero rejects new work without revocation",
                );
                drop(permit);
                kani::assert(
                    counter.used() == used,
                    "release preserves other reservations",
                );
            }
            Err(_) => {
                kani::assert(
                    used >= counter.limit(),
                    "admission rejects only at or above its limit",
                );
                kani::assert(
                    counter.used() == used,
                    "rejected admission preserves all reservations",
                );
            }
        };
    }
}

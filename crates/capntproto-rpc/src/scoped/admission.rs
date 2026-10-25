//! Unbranded ownership shared by requests, response readers and pipelines.
use super::*;
use crate::admission::{Admission, Permit};

pub(super) struct Credit {
    admission: Rc<Admission>,
    permit: core::cell::OnceCell<Rc<Permit>>,
    received: Cell<bool>,
}
impl Credit {
    pub(super) fn new(admission: Rc<Admission>) -> Rc<Self> {
        Rc::new(Self {
            admission,
            permit: core::cell::OnceCell::new(),
            received: Cell::new(false),
        })
    }
    #[inline(always)]
    pub(super) fn ensure(&self) -> capnp::Result<()> {
        if self.permit.get().is_none() {
            self.acquire()?;
        }
        Ok(())
    }
    #[inline(always)]
    pub(super) fn acquire(&self) -> capnp::Result<Rc<Permit>> {
        if let Some(permit) = self.permit.get() {
            return Ok(permit.clone());
        }
        let permit = self.admission.reserve()?;
        // Publish once. Even a reentrant allocator cannot replace a reservation
        // already installed by another acquire on this same credit.
        if self.permit.set(permit.clone()).is_err() {
            return Ok(self.permit.get().expect("installed reservation").clone());
        }
        Ok(permit)
    }
    pub(super) fn received(&self) {
        if !self.received.replace(true) {
            let count = &self.admission.held_responses;
            count.set(count.get() + 1);
        }
    }
}
impl Drop for Credit {
    fn drop(&mut self) {
        if self.received.get() {
            let count = &self.admission.held_responses;
            count.set(count.get() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_admission_and_receipt_share_one_credit_until_the_last_owner() {
        let admission = Admission::new(1);
        let credit = Credit::new(admission.clone());
        credit.ensure().unwrap();
        let permit = credit.acquire().unwrap();
        assert!(Rc::ptr_eq(&permit, &credit.acquire().unwrap()));
        credit.ensure().unwrap();
        assert_eq!(admission.used(), 1);
        let blocked = Credit::new(admission.clone());
        assert!(blocked.ensure().is_err());
        for _ in 0..3 {
            credit.received();
        }
        assert_eq!(admission.held_responses.get(), 1);
        drop(credit);
        assert_eq!(admission.held_responses.get(), 0);
        assert_eq!(admission.used(), 1);
        drop(permit);
        assert_eq!(admission.used(), 0);
        blocked.ensure().unwrap();
        drop(blocked);
        assert_eq!(admission.used(), 0);
        assert_eq!(admission.held_responses.get(), 0);
    }
}

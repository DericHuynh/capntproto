//! Unbranded ownership shared by requests, response readers and pipelines.
use super::*;
use crate::admission::{Admission, Permit};

pub(super) struct Credit {
    admission: Rc<Admission>,
    permit: RefCell<Option<Rc<Permit>>>,
    received: Cell<bool>,
}
impl Credit {
    pub(super) fn new(admission: Rc<Admission>) -> Rc<Self> {
        Rc::new(Self {
            admission,
            permit: RefCell::default(),
            received: Cell::new(false),
        })
    }
    pub(super) fn acquire(&self) -> capnp::Result<Rc<Permit>> {
        if let Some(permit) = self.permit.borrow().as_ref() {
            return Ok(permit.clone());
        }
        let permit = self.admission.reserve()?;
        *self.permit.borrow_mut() = Some(permit.clone());
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

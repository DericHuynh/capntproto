//! Single-thread queue ownership. The FIFO engine releases all guards before
//! invoking clocks, wakers, message destructors or cooperative IO callbacks.
//! These types intentionally make the local sender and driver non-Send.
use std::{
    cell::{BorrowMutError, Cell, RefCell, RefMut},
    sync::atomic::Ordering,
    task::Waker,
};

pub(super) struct QueueCell<T>(RefCell<T>);
impl<T> QueueCell<T> {
    pub(super) fn new(value: T) -> Self {
        Self(RefCell::new(value))
    }
    pub(super) fn lock(&self) -> Result<RefMut<'_, T>, BorrowMutError> {
        self.0.try_borrow_mut()
    }
}

pub(super) struct Flag(Cell<bool>);
impl Flag {
    pub(super) fn new(value: bool) -> Self {
        Self(Cell::new(value))
    }
    pub(super) fn load(&self, _: Ordering) -> bool {
        self.0.get()
    }
    pub(super) fn store(&self, value: bool, _: Ordering) {
        self.0.set(value);
    }
    pub(super) fn compare_exchange(
        &self,
        current: bool,
        new: bool,
        _: Ordering,
        _: Ordering,
    ) -> Result<bool, bool> {
        let old = self.0.get();
        if old == current {
            self.0.set(new);
            Ok(old)
        } else {
            Err(old)
        }
    }
}

pub(super) struct Notify(RefCell<Option<Waker>>);
impl Notify {
    pub(super) fn new() -> Self {
        Self(RefCell::new(None))
    }
    pub(super) fn register(&self, wake: &Waker) {
        if self
            .0
            .borrow()
            .as_ref()
            .is_some_and(|old| old.will_wake(wake))
        {
            return;
        }
        // Cloning and dropping caller wakers can reenter the queue.
        let wake = wake.clone();
        let old = self.0.replace(Some(wake));
        drop(old);
    }
    pub(super) fn take(&self) -> Option<Waker> {
        self.0.borrow_mut().take()
    }
    pub(super) fn wake(&self) {
        let wake = self.take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{rc::Rc, sync::Arc, task::Wake};
    thread_local! {
        static CALLBACK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }
    fn callback() {
        let action = CALLBACK.with(|slot| slot.borrow_mut().take());
        if let Some(action) = action {
            action();
        }
    }
    struct Reenter;
    impl Wake for Reenter {
        fn wake(self: Arc<Self>) {
            callback();
        }
    }
    impl Drop for Reenter {
        fn drop(&mut self) {
            callback();
        }
    }

    #[test]
    fn waker_drop_and_wake_can_reenter_local_registration() {
        let notify = Rc::new(Notify::new());
        let observed = Rc::new(Cell::new(0));
        for replacing in [true, false] {
            notify.register(&Waker::from(Arc::new(Reenter)));
            let notify2 = notify.clone();
            let observed2 = observed.clone();
            CALLBACK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    observed2.set(observed2.get() + 1);
                    notify2.register(Waker::noop());
                    notify2.wake();
                }))
            });
            if replacing {
                notify.register(Waker::noop());
            } else {
                notify.wake();
            }
            assert!(notify.take().is_none());
        }
        assert_eq!(observed.get(), 2);
    }
}

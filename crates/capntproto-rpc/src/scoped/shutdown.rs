//! Local broadcast of the connection's terminal error. Receivers share no
//! cross-thread mutex or atomic reference count. Waiter slots are recycled when
//! calls finish, and every callback runs after releasing the state borrow.
use super::{completion, Cell, Context, Error, Future, Pin, Poll, Rc, RefCell, Waker};
use alloc::vec::Vec;

enum Notification {
    Task(Waker),
    Response(alloc::rc::Weak<super::response::State>),
}
impl Notification {
    fn wake(self) {
        match self {
            Self::Task(wake) => wake.wake(),
            Self::Response(state) => {
                if let Some(state) = state.upgrade() {
                    super::response::notify_closed(&state);
                }
            }
        }
    }
}
struct Waiter {
    wake: Option<Notification>,
    next: Option<usize>,
}
#[derive(Default)]
struct Inner {
    reason: Option<Error>,
    waiters: Vec<Waiter>,
    free: Option<usize>,
}
#[derive(Default)]
struct State {
    // Terminal status is monotonic. Polling cancellation should not borrow the
    // callback-bearing waiter table merely to inspect this independent bit.
    closed: Cell<bool>,
    inner: RefCell<Inner>,
}
pub(super) struct Receiver {
    state: Rc<State>,
    slot: Option<usize>,
}
impl Clone for Receiver {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            slot: None,
        }
    }
}
impl Receiver {
    pub(super) fn is_closed(&self) -> bool {
        self.state.closed.get()
    }
    /// Direct completions already retain their receiving task's waker. Register
    /// that state weakly instead of cloning a second copy of the same waker.
    pub(super) fn watch_response(&mut self, response: &Rc<super::response::State>) {
        if self.slot.is_some() {
            return;
        }
        let mut state = self.state.inner.borrow_mut();
        if state.reason.is_some() {
            return;
        }
        let slot = match state.free {
            Some(slot) => {
                state.free = state.waiters[slot].next;
                slot
            }
            None => {
                let slot = state.waiters.len();
                state.waiters.push(Waiter {
                    wake: None,
                    next: None,
                });
                slot
            }
        };
        state.waiters[slot].wake = Some(Notification::Response(Rc::downgrade(response)));
        self.slot = Some(slot);
    }
    pub(super) fn reason(&self) -> Option<Error> {
        self.state.inner.borrow().reason.clone()
    }
    pub(super) fn new() -> Self {
        Self {
            state: Rc::default(),
            slot: None,
        }
    }
    pub(super) fn close(&self, error: Error) {
        let waiters = {
            let mut state = self.state.inner.borrow_mut();
            if state.reason.is_some() {
                return;
            }
            state.reason = Some(error);
            self.state.closed.set(true);
            state.free = None;
            core::mem::take(&mut state.waiters)
        };
        for wake in waiters.into_iter().filter_map(|waiter| waiter.wake) {
            wake.wake();
        }
    }
}
impl Future for Receiver {
    type Output = Result<Error, completion::Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        {
            let state = self.state.inner.borrow();
            if let Some(reason) = &state.reason {
                return Poll::Ready(Ok(reason.clone()));
            }
            if self.slot.is_some_and(|slot| {
                state.waiters[slot].wake.as_ref().is_some_and(
                    |wake| matches!(wake, Notification::Task(wake) if wake.will_wake(cx.waker())),
                )
            }) {
                return Poll::Pending;
            }
        }
        // Waker::clone can run caller code, including closing the connection.
        let wake = cx.waker().clone();
        let (slot, old) = {
            let mut state = self.state.inner.borrow_mut();
            if let Some(reason) = &state.reason {
                return Poll::Ready(Ok(reason.clone()));
            }
            let slot = match self.slot {
                Some(slot) => slot,
                None => match state.free {
                    Some(slot) => {
                        state.free = state.waiters[slot].next;
                        slot
                    }
                    None => {
                        let slot = state.waiters.len();
                        state.waiters.push(Waiter {
                            wake: None,
                            next: None,
                        });
                        slot
                    }
                },
            };
            (
                slot,
                state.waiters[slot].wake.replace(Notification::Task(wake)),
            )
        };
        self.slot = Some(slot);
        drop(old);
        // Replacing a waker can also close the connection synchronously.
        match &self.state.inner.borrow().reason {
            Some(reason) => Poll::Ready(Ok(reason.clone())),
            None => Poll::Pending,
        }
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        let Some(slot) = self.slot.take() else {
            return;
        };
        let old = {
            let mut state = self.state.inner.borrow_mut();
            if state.reason.is_some() {
                return;
            }
            let next = state.free;
            state.free = Some(slot);
            let waiter = &mut state.waiters[slot];
            waiter.next = next;
            waiter.wake.take()
        };
        drop(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Wakes(AtomicUsize);
    thread_local! {
        static CALLBACK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    }
    fn callback() {
        let action = CALLBACK.with(|slot| slot.borrow_mut().take());
        if let Some(action) = action {
            action();
        }
    }
    struct Reenter;
    impl std::task::Wake for Reenter {
        fn wake(self: Arc<Self>) {
            callback();
        }
    }
    struct ReenterOnDrop;
    // This test exercises a waker's destructor reentering shutdown. The static
    // noop waker has no destructor and cannot exercise that ownership boundary.
    #[allow(clippy::manual_noop_waker)]
    impl std::task::Wake for ReenterOnDrop {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for ReenterOnDrop {
        fn drop(&mut self) {
            callback();
        }
    }
    #[test]
    fn notification_and_waker_destruction_can_reenter_shutdown() {
        let source = Receiver::new();
        let mut receiver = source.clone();
        let late = source.clone();
        let observed = Rc::new(std::cell::Cell::new(false));
        let flag = observed.clone();
        CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                let mut late = late;
                assert!(Pin::new(&mut late)
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_ready());
                flag.set(true);
            }))
        });
        let waker = Waker::from(Arc::new(Reenter));
        assert!(Pin::new(&mut receiver)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        source.close(Error::disconnected("notify".into()));
        assert!(observed.get());

        let source = Receiver::new();
        let mut receiver = source.clone();
        {
            let waker = Waker::from(Arc::new(ReenterOnDrop));
            assert!(Pin::new(&mut receiver)
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
        }
        let closing = source.clone();
        CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                closing.close(Error::disconnected("drop".into()))
            }))
        });
        let Poll::Ready(Ok(reason)) =
            Pin::new(&mut receiver).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("reentrant close");
        };
        assert_eq!(reason.extra, "drop");
    }
    impl std::task::Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn closing_wakes_each_live_receiver_and_preserves_the_first_reason() {
        let source = Receiver::new();
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut a = source.clone();
        let mut b = source.clone();
        assert!(Pin::new(&mut a)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        assert!(Pin::new(&mut b)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        source.close(Error::disconnected("first".into()));
        source.close(Error::disconnected("second".into()));
        assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
        for mut receiver in [a, b, source.clone()] {
            let Poll::Ready(Ok(reason)) =
                Pin::new(&mut receiver).poll(&mut Context::from_waker(&waker))
            else {
                panic!("closed");
            };
            assert_eq!(reason.extra, "first");
        }
    }
    #[test]
    fn abandoned_waiters_are_recycled_and_never_woken() {
        let source = Receiver::new();
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        for _ in 0..1000 {
            let mut receiver = source.clone();
            assert!(Pin::new(&mut receiver)
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
        }
        assert_eq!(source.state.inner.borrow().waiters.len(), 1);
        source.close(Error::disconnected("closed".into()));
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    }
}

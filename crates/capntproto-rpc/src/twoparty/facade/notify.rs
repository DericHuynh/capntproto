//! Poll shutdown observers only after their own notification. Ordinary RPC
//! progress must not repeatedly enter Shared's synchronized waiter registry.
use futures::task::AtomicWaker;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

struct Signal {
    ready: AtomicBool,
    parent: AtomicWaker,
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        self.parent.wake();
    }
}

pub(super) struct WakeDriven<F> {
    future: F,
    signal: Arc<Signal>,
    child: Waker,
    parent: Option<Waker>,
}
impl<F> WakeDriven<F> {
    pub(super) fn new(future: F) -> Self {
        let signal = Arc::new(Signal {
            ready: AtomicBool::new(true),
            parent: AtomicWaker::new(),
        });
        Self {
            future,
            child: Waker::from(signal.clone()),
            signal,
            parent: None,
        }
    }
}
impl<F: Future + Unpin> Future for WakeDriven<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this
            .parent
            .as_ref()
            .is_none_or(|previous| !previous.will_wake(cx.waker()))
        {
            this.signal.parent.register(cx.waker());
            this.parent = Some(cx.waker().clone());
        }
        if !this.signal.ready.load(Ordering::Acquire) {
            return Poll::Pending;
        }
        // AtomicWaker::wake consumes its registration. Renew it on each child
        // notification even when the enclosing task's waker is unchanged.
        this.signal.parent.register(cx.waker());
        this.signal.ready.swap(false, Ordering::AcqRel);
        Pin::new(&mut this.future).poll(&mut Context::from_waker(&this.child))
    }
}
impl<F> Drop for WakeDriven<F> {
    fn drop(&mut self) {
        // An external completion may retain the child waker after cancellation.
        // It must not retain or schedule the parent task after this observer dies.
        drop(self.signal.parent.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::atomic::AtomicUsize,
    };
    #[derive(Default)]
    struct Parent(AtomicUsize);
    impl Wake for Parent {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn unrelated_polls_do_not_repoll_observers_and_notifications_follow_the_current_parent() {
        let child = Rc::new(RefCell::new(None::<Waker>));
        let polls = Cell::new(0);
        let done = Cell::new(false);
        let mut observer = WakeDriven::new(std::future::poll_fn(|cx| {
            polls.set(polls.get() + 1);
            *child.borrow_mut() = Some(cx.waker().clone());
            if done.get() {
                Poll::Ready(42)
            } else {
                Poll::Pending
            }
        }));
        let first = Arc::new(Parent::default());
        let second = Arc::new(Parent::default());
        let first_waker = Waker::from(first.clone());
        let second_waker = Waker::from(second.clone());
        for _ in 0..20 {
            assert!(Pin::new(&mut observer)
                .poll(&mut Context::from_waker(&first_waker))
                .is_pending());
        }
        assert_eq!(polls.get(), 1);
        assert!(Pin::new(&mut observer)
            .poll(&mut Context::from_waker(&second_waker))
            .is_pending());
        child.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(first.0.load(Ordering::Relaxed), 0);
        assert_eq!(second.0.load(Ordering::Relaxed), 1);
        assert!(Pin::new(&mut observer)
            .poll(&mut Context::from_waker(&second_waker))
            .is_pending());
        done.set(true);
        child.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(second.0.load(Ordering::Relaxed), 2);
        assert_eq!(
            Pin::new(&mut observer).poll(&mut Context::from_waker(&second_waker)),
            Poll::Ready(42)
        );
        assert_eq!(polls.get(), 3);
        drop(observer);
        child.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(second.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn notification_during_poll_is_preserved() {
        let mut polls = 0;
        let mut observer = WakeDriven::new(std::future::poll_fn(|cx| {
            polls += 1;
            if polls == 1 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }));
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(Pin::new(&mut observer).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut observer).poll(&mut cx).is_ready());
    }
}

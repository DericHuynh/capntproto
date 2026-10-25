//! Poll terminal futures only after their own notification. IO readiness and
//! cooperative work yields do not require polling unrelated shutdown futures.
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{atomic::Ordering, Arc};
use std::task::{Context, Poll, Waker};

struct Wake {
    ready: AtomicBool,
    parent: futures::task::AtomicWaker,
}
impl std::task::Wake for Wake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        self.parent.wake();
    }
}

pub(super) struct Notified<F> {
    future: F,
    wake: Arc<Wake>,
    waker: Waker,
    parent: Option<Waker>,
}
impl<F> Notified<F> {
    pub(super) fn new(future: F) -> Self {
        let wake = Arc::new(Wake {
            ready: AtomicBool::new(true),
            parent: futures::task::AtomicWaker::new(),
        });
        let waker = Waker::from(wake.clone());
        Self {
            future,
            wake,
            waker,
            parent: None,
        }
    }
}
impl<F: Future + Unpin> Future for Notified<F> {
    type Output = F::Output;
    #[inline(always)]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        // With no notification, the registered parent is still current. A
        // concurrent wake after this load notifies that same task; every poll
        // which observes readiness re-registers before consuming it below.
        if this
            .parent
            .as_ref()
            .is_some_and(|parent| parent.will_wake(cx.waker()))
            && !this.wake.ready.load(Ordering::Acquire)
        {
            return Poll::Pending;
        }
        this.poll_notified(cx)
    }
}
impl<F: Future + Unpin> Notified<F> {
    // Keep registration and arbitrary future polling out of the overwhelmingly
    // common unnotified path, including its return-value and unwind storage.
    #[inline(never)]
    fn poll_notified(&mut self, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self;
        // Register first: a concurrent notification must wake the current task,
        // even when the preceding poll used a different task's waker.
        this.wake.parent.register(cx.waker());
        if this
            .parent
            .as_ref()
            .is_none_or(|parent| !parent.will_wake(cx.waker()))
        {
            this.parent = Some(cx.waker().clone());
        }
        if !this.wake.ready.swap(false, Ordering::AcqRel) {
            return Poll::Pending;
        }
        Pin::new(&mut this.future).poll(&mut Context::from_waker(&this.waker))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn notifications_follow_the_latest_task_and_survive_polling() {
        let polls = Cell::new(0);
        let inner_waker = RefCell::new(None);
        let future = std::future::poll_fn(|cx| {
            polls.set(polls.get() + 1);
            *inner_waker.borrow_mut() = Some(cx.waker().clone());
            match polls.get() {
                1 => Poll::Pending,
                2 => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                _ => Poll::Ready(17),
            }
        });
        let mut future = Notified::new(future);
        let first = Arc::new(Count::default());
        let second = Arc::new(Count::default());
        let first_waker = Waker::from(first.clone());
        let second_waker = Waker::from(second.clone());
        assert!(Pin::new(&mut future)
            .poll(&mut Context::from_waker(&first_waker))
            .is_pending());
        for _ in 0..100 {
            assert!(Pin::new(&mut future)
                .poll(&mut Context::from_waker(&second_waker))
                .is_pending());
        }
        assert_eq!(polls.get(), 1);
        inner_waker.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert!(Pin::new(&mut future)
            .poll(&mut Context::from_waker(&second_waker))
            .is_pending());
        assert_eq!(
            Pin::new(&mut future).poll(&mut Context::from_waker(&second_waker)),
            Poll::Ready(17)
        );
        assert_eq!(polls.get(), 3);
    }
}

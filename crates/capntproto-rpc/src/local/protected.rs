//! Two execution interests over one local protected method. Only wake metadata
//! crosses threads; the method and its owners remain on the local executor.
use capnp::Error;
use futures::task::AtomicWaker;
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};
struct Notify {
    ready: AtomicBool,
    branches: [AtomicWaker; 2],
}
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        // Publish even duplicate notifications before a consumer clears ready.
        self.ready.store(true, Ordering::Release);
        for branch in &self.branches {
            branch.wake();
        }
    }
}
struct State<F> {
    future: Option<F>,
    result: Option<Result<(), Error>>,
}
struct Shared<F> {
    state: RefCell<State<F>>,
    notify: Arc<Notify>,
    waker: Waker,
}
pub(super) struct Branch<F> {
    shared: Rc<Shared<F>>,
    index: usize,
}
pub(super) fn pair<F>(future: F) -> (Branch<F>, Branch<F>) {
    let notify = Arc::new(Notify {
        ready: AtomicBool::new(true),
        branches: [AtomicWaker::new(), AtomicWaker::new()],
    });
    let shared = Rc::new(Shared {
        state: RefCell::new(State {
            future: Some(future),
            result: None,
        }),
        waker: Waker::from(notify.clone()),
        notify,
    });
    (
        Branch {
            shared: shared.clone(),
            index: 0,
        },
        Branch { shared, index: 1 },
    )
}
impl<F: Future<Output = Result<(), Error>> + Unpin> Future for Branch<F> {
    type Output = Result<(), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let shared = &self.shared;
        shared.notify.branches[self.index].register(cx.waker());
        let Ok(mut state) = shared.state.try_borrow_mut() else {
            // A reentrant wake cannot poll a method already on this stack.
            return Poll::Pending;
        };
        if let Some(result) = &state.result {
            return Poll::Ready(result.clone());
        }
        if !shared.notify.ready.swap(false, Ordering::AcqRel) {
            return Poll::Pending;
        }
        let result = match Pin::new(state.future.as_mut().expect("live protected method"))
            .poll(&mut Context::from_waker(&shared.waker))
        {
            Poll::Pending => {
                drop(state);
                // A synchronous wake may have reentered a branch while the
                // method was borrowed. Publish another poll after releasing it.
                if shared.notify.ready.load(Ordering::Acquire) {
                    for branch in &shared.notify.branches {
                        branch.wake();
                    }
                }
                return Poll::Pending;
            }
            Poll::Ready(result) => result,
        };
        state.result = Some(result.clone());
        let completed = state.future.take();
        drop(state);
        drop(completed);
        shared.notify.branches[1 - self.index].wake();
        Poll::Ready(result)
    }
}
impl<F> Drop for Branch<F> {
    fn drop(&mut self) {
        // Do not retain a canceled caller/executor waker with the other owner.
        drop(self.shared.notify.branches[self.index].take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::atomic::AtomicUsize};
    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Work {
        receiver: futures::channel::oneshot::Receiver<()>,
        drops: Rc<Cell<usize>>,
    }
    impl Drop for Work {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    impl Future for Work {
        type Output = Result<(), Error>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.receiver)
                .poll(cx)
                .map(|r| r.map_err(|_| Error::failed("closed".into())))
        }
    }
    #[test]
    fn either_owner_can_finish_after_other_drops_and_foreign_wake() {
        for retained in 0..2 {
            let (send, receive) = futures::channel::oneshot::channel();
            let drops = Rc::new(Cell::new(0));
            let (a, b) = pair(Work {
                receiver: receive,
                drops: drops.clone(),
            });
            let mut branches = [Some(a), Some(b)];
            let wakes = [
                Arc::new(Wakes(AtomicUsize::new(0))),
                Arc::new(Wakes(AtomicUsize::new(0))),
            ];
            for i in 0..2 {
                let waker = Waker::from(wakes[i].clone());
                assert!(Pin::new(branches[i].as_mut().unwrap())
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending());
            }
            drop(branches[1 - retained].take());
            assert_eq!(drops.get(), 0);
            std::thread::spawn(move || send.send(()).unwrap())
                .join()
                .unwrap();
            assert_eq!(wakes[1 - retained].0.load(Ordering::SeqCst), 0);
            assert_eq!(wakes[retained].0.load(Ordering::SeqCst), 1);
            assert!(Pin::new(branches[retained].as_mut().unwrap())
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready());
            assert_eq!(
                drops.get(),
                1,
                "completion releases method before branch storage"
            );
            drop(branches);
            assert_eq!(drops.get(), 1);
        }
    }
}

//! Single-thread task admission. No user future, destructor or waker runs while
//! the queue is borrowed. TaskSet already owns non-Send futures and a local reaper.
#![forbid(unsafe_code)]

use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    task::{Context, Poll, Waker},
};

struct State<T> {
    pending: Option<VecDeque<T>>,
    senders: usize,
    waker: Option<Waker>,
}
pub(super) struct Sender<T>(Rc<RefCell<State<T>>>);
pub(super) struct Receiver<T>(Rc<RefCell<State<T>>>);

pub(super) fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Rc::new(RefCell::new(State {
        pending: Some(VecDeque::new()),
        senders: 1,
        waker: None,
    }));
    (Sender(state.clone()), Receiver(state))
}

impl<T> Sender<T> {
    pub(super) fn send(&self, value: T) -> Result<(), T> {
        let wake = {
            let mut state = self.0.borrow_mut();
            let Some(pending) = &mut state.pending else {
                return Err(value);
            };
            pending.push_back(value);
            state.waker.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }
}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.0.borrow_mut().senders += 1;
        Self(self.0.clone())
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let wake = {
            let mut state = self.0.borrow_mut();
            state.senders -= 1;
            if state.senders == 0 {
                state.waker.take()
            } else {
                None
            }
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
impl<T> Receiver<T> {
    pub(super) fn is_empty(&self) -> bool {
        self.0
            .borrow()
            .pending
            .as_ref()
            .is_none_or(|queue| queue.is_empty())
    }

    pub(super) fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        // Clone a replacement outside the borrow: custom wakers may reenter.
        let same_waker = self
            .0
            .borrow()
            .waker
            .as_ref()
            .is_some_and(|wake| wake.will_wake(cx.waker()));
        let mut replacement = (!same_waker).then(|| cx.waker().clone());
        let (result, retired) = {
            let mut state = self.0.borrow_mut();
            let pending = state.pending.as_mut().expect("live receiver");
            let next = pending.pop_front();
            // Retain ordinary admission capacity, but release exceptional peaks.
            let retired =
                (pending.is_empty() && pending.capacity() > 1024).then(|| std::mem::take(pending));
            let result = if next.is_some() || state.senders == 0 {
                Poll::Ready(next)
            } else {
                if replacement.is_some() {
                    std::mem::swap(&mut replacement, &mut state.waker);
                }
                Poll::Pending
            };
            (result, retired)
        };
        drop(replacement);
        drop(retired);
        result
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let (pending, waker) = {
            let mut state = self.0.borrow_mut();
            (state.pending.take(), state.waker.take())
        };
        drop(waker);
        drop(pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::Arc, task::Wake};

    thread_local! {
        static ON_WAKE: RefCell<Option<Rc<dyn Fn()>>> = RefCell::new(None);
    }
    struct Reenter;
    impl Wake for Reenter {
        fn wake(self: Arc<Self>) {
            ON_WAKE.with(|hook| {
                let callback = hook.borrow().clone().unwrap();
                callback();
            });
        }
    }

    #[test]
    fn enqueue_and_last_sender_drop_wake_after_releasing_the_queue() {
        let (sender, receiver) = channel();
        let receiver = Rc::new(RefCell::new(receiver));
        let observed = Rc::new(RefCell::new(Vec::new()));
        let wake = Waker::from(Arc::new(Reenter));
        ON_WAKE.with(|hook| {
            let receiver = receiver.clone();
            let observed = observed.clone();
            let wake = wake.clone();
            *hook.borrow_mut() = Some(Rc::new(move || {
                let mut receiver = receiver.borrow_mut();
                let mut cx = Context::from_waker(&wake);
                observed.borrow_mut().push(receiver.poll_next(&mut cx));
                let _ = receiver.poll_next(&mut cx); // register for the next wake
            }));
        });
        assert!(receiver
            .borrow_mut()
            .poll_next(&mut Context::from_waker(&wake))
            .is_pending());
        let clone = sender.clone();
        sender.send(17).unwrap();
        clone.send(23).unwrap();
        drop(sender);
        drop(clone);
        assert_eq!(
            observed.borrow().as_slice(),
            &[
                Poll::Ready(Some(17)),
                Poll::Ready(Some(23)),
                Poll::Ready(None),
            ]
        );
        ON_WAKE.with(|hook| hook.borrow_mut().take());
    }

    #[test]
    fn receiver_drop_closes_before_running_queued_destructors() {
        struct EnqueueOnDrop {
            sender: Option<Sender<Self>>,
            dropped: Rc<Cell<usize>>,
        }
        impl Drop for EnqueueOnDrop {
            fn drop(&mut self) {
                self.dropped.set(self.dropped.get() + 1);
                if let Some(sender) = self.sender.take() {
                    assert!(sender
                        .send(Self {
                            sender: None,
                            dropped: self.dropped.clone(),
                        })
                        .is_err());
                }
            }
        }
        let (sender, receiver) = channel();
        let dropped = Rc::new(Cell::new(0));
        assert!(sender
            .send(EnqueueOnDrop {
                sender: Some(sender.clone()),
                dropped: dropped.clone(),
            })
            .is_ok());
        drop(receiver);
        assert_eq!(dropped.get(), 2);
    }

    #[test]
    fn fifo_survives_wraparound_and_releases_large_idle_capacity() {
        let (sender, mut receiver) = channel();
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        for round in 0..3 {
            for n in 0..1100 {
                sender.send((round, n)).unwrap();
            }
            for n in 0..1100 {
                assert_eq!(receiver.poll_next(&mut cx), Poll::Ready(Some((round, n))));
            }
            assert!(receiver.0.borrow().pending.as_ref().unwrap().capacity() <= 1024);
            assert!(receiver.poll_next(&mut cx).is_pending());
        }
    }
}

//! Hosted ownership for the shared core admission algorithm.
#![forbid(unsafe_code)]
use capntproto_rpc_engine::admission;
pub(super) type Sender<T> = admission::Sender<admission::Shared<T>>;
pub(super) type Receiver<T> = admission::Receiver<admission::Shared<T>>;
pub(super) fn channel<T>() -> (Sender<T>, Receiver<T>) {
    admission::shared()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::Arc, task::Wake};
    use std::{
        cell::RefCell,
        rc::Rc,
        task::{Context, Poll, Waker},
        vec::Vec,
    };

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
    fn fifo_survives_wraparound() {
        let (sender, mut receiver) = channel();
        let mut cx = Context::from_waker(Waker::noop());
        for round in 0..3 {
            for n in 0..1100 {
                sender.send((round, n)).unwrap();
            }
            for n in 0..1100 {
                assert_eq!(receiver.poll_next(&mut cx), Poll::Ready(Some((round, n))));
            }
            assert!(receiver.poll_next(&mut cx).is_pending());
        }
    }
}

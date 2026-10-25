extern crate std;
use super::*;
use core::cell::Cell;

fn poll<O: Owner>(receiver: &mut Receiver<O>) -> Poll<Option<<O::Queue as Buffer>::Item>> {
    receiver.poll_next(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn borrowed_fifo_wrap_capacity_and_reuse_need_no_allocations() {
    let measured = allocation_counter::measure(|| {
        let mut slot = Slot::<usize, 3>::new();
        for _ in 0..4 {
            let (sender, mut receiver) = slot.pair();
            for round in 0..16 {
                for value in 0..3 {
                    assert_eq!(sender.send(value + round * 3), Ok(()));
                }
                assert_eq!(sender.send(99), Err(99));
                assert_eq!(poll(&mut receiver), Poll::Ready(Some(round * 3)));
                assert_eq!(sender.send(100), Ok(()));
                for value in [round * 3 + 1, round * 3 + 2, 100] {
                    assert_eq!(poll(&mut receiver), Poll::Ready(Some(value)));
                }
                assert!(receiver.is_empty());
                assert!(poll(&mut receiver).is_pending());
            }
            let second = sender.clone();
            drop(sender);
            assert!(poll(&mut receiver).is_pending());
            drop(second);
            assert_eq!(poll(&mut receiver), Poll::Ready(None));
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn full_and_closed_rejection_return_borrowed_values_without_dropping_them() {
    struct Value<'a>(&'a Cell<usize>);
    impl Drop for Value<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let dropped = Cell::new(0);
    let mut slot = Slot::<Value<'_>, 1>::new();
    let (sender, receiver) = slot.pair();
    assert!(sender.send(Value(&dropped)).is_ok());
    let rejected = sender.send(Value(&dropped)).err().unwrap();
    assert_eq!(dropped.get(), 0);
    drop(receiver);
    assert_eq!(dropped.get(), 1);
    let closed = sender.send(Value(&dropped)).err().unwrap();
    assert_eq!(dropped.get(), 1);
    drop((rejected, closed));
    assert_eq!(dropped.get(), 3);
}

#[test]
fn zero_capacity_returns_ownership_and_finishes_when_senders_end() {
    let mut slot = Slot::<&str, 0>::new();
    let (sender, mut receiver) = slot.pair();
    assert_eq!(sender.send("retained"), Err("retained"));
    assert!(poll(&mut receiver).is_pending());
    drop(sender);
    assert_eq!(poll(&mut receiver), Poll::Ready(None));
}

#[test]
fn only_final_sender_shutdown_notifies_and_drain_precedes_end() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut slot = Slot::<usize, 2>::new();
    let (sender, mut receiver) = slot.pair();
    let count = Arc::new(Count(AtomicUsize::new(0)));
    let wake = Waker::from(count.clone());
    let mut cx = Context::from_waker(&wake);
    assert!(receiver.poll_next(&mut cx).is_pending());
    let second = sender.clone();
    drop(sender);
    assert_eq!(count.0.load(Ordering::Relaxed), 0);
    assert_eq!(second.send(7), Ok(()));
    assert_eq!(count.0.load(Ordering::Relaxed), 1);
    drop(second);
    assert_eq!(receiver.poll_next(&mut cx), Poll::Ready(Some(7)));
    assert_eq!(receiver.poll_next(&mut cx), Poll::Ready(None));
}

#[test]
fn sender_overflow_panics_before_changing_live_count() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let mut slot = Slot::<(), 1>::new();
    let (sender, _receiver) = slot.pair();
    sender.0.state().borrow_mut().senders = usize::MAX;
    assert!(catch_unwind(AssertUnwindSafe(|| sender.clone())).is_err());
    assert_eq!(sender.0.state().borrow().senders, usize::MAX);
    sender.0.state().borrow_mut().senders = 1;
}

#[cfg(feature = "alloc")]
#[test]
fn hosted_fifo_releases_exceptional_capacity_after_drain() {
    let (sender, mut receiver) = shared();
    for round in 0..3 {
        for value in 0..1100 {
            assert_eq!(sender.send((round, value)), Ok(()));
        }
        for value in 0..1100 {
            assert_eq!(poll(&mut receiver), Poll::Ready(Some((round, value))));
        }
        assert!(receiver.0.borrow().pending.as_ref().unwrap().capacity() <= 1024);
    }
}

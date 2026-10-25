extern crate std;
use super::*;
use crate::completion;
use core::{
    cell::Cell,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

#[test]
fn borrowed_calls_drain_in_order_and_old_cancellation_cannot_remove_new_work() {
    let measured = allocation_counter::measure(|| {
        let mut storage = Storage::with_map(Fixed::<u32, (), 2>::new());
        let queue = storage.borrow();
        let first = queue.try_push(17, ()).ok().unwrap();
        let second = queue.try_push(23, ()).ok().unwrap();
        let third = queue.try_push(29, ()).ok().unwrap();
        drop(first);
        let (value, producer) = queue.try_push(31, ()).err().unwrap();
        assert_eq!((value, producer), (31, ()));
        drop(second);
        let fourth = queue.try_push(value, producer).ok().unwrap();
        assert_eq!(
            fourth.identity(),
            3,
            "capacity rejection cannot consume an identity"
        );
        let mut drained = queue.drain();
        assert_eq!(drained.next(), Some((29, ())));
        assert_eq!(drained.next(), Some((31, ())));
        assert_eq!(drained.next(), None);
        let new = queue.try_push(37, ()).ok().unwrap();
        drop(third);
        drop(fourth);
        assert_eq!(queue.drain().next(), Some((37, ())));
        assert!(new.identity() > 3);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn identity_exhaustion_is_terminal_even_after_drain_and_returns_both_owners() {
    let measured = allocation_counter::measure(|| {
        let mut state = State::new(Fixed::<u32, u32, 1>::new());
        state.next_id = Some(u64::MAX);
        assert_eq!(state.try_insert(41, 43), Ok(u64::MAX));
        assert_eq!(state.try_insert(47, 53), Err((47, 53)));
        let (first, map) = state.take();
        assert_eq!(Drain::new(first, map.into_iter()).next(), Some((41, 43)));
        assert_eq!(state.try_insert(59, 61), Err((59, 61)));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn zero_overflow_capacity_retains_legacy_inline_entry_without_consuming_rejection() {
    let drops = Cell::new(0);
    struct Value<'a>(&'a Cell<usize>);
    impl Drop for Value<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let measured = allocation_counter::measure(|| {
        let mut storage = Storage::with_map(Fixed::<_, _, 0>::new());
        let queue = storage.borrow();
        let old = queue.try_push(Value(&drops), Value(&drops)).ok().unwrap();
        let (input, sender) = queue.try_push(Value(&drops), Value(&drops)).err().unwrap();
        assert_eq!(drops.get(), 0);
        drop(old);
        assert_eq!(drops.get(), 2);
        let replacement = queue.try_push(input, sender).ok().unwrap();
        assert_eq!(replacement.identity(), 1);
        drop(replacement);
        assert_eq!(drops.get(), 4);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn observed_borrowed_reply_finishes_and_cancels_without_a_boxed_continuation() {
    let measured = allocation_counter::measure(|| {
        let mut reply = completion::Slot::new();
        let mut canceled_reply = completion::Slot::new();
        let (sender, receiver) = reply.pair().unwrap();
        let (canceled_sender, canceled_receiver) = canceled_reply.pair().unwrap();
        let mut storage = Storage::with_map(Fixed::<_, _, 1>::new());
        let queue = storage.borrow();
        let cancel = queue.try_push(17, sender).ok().unwrap();
        let mut observed = Observed::new(receiver, cancel);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut observed).poll(&mut cx).is_pending());
        let (input, sender) = queue.drain().next().unwrap();
        assert_eq!(input, 17);
        sender.send(23).unwrap();
        assert_eq!(Pin::new(&mut observed).poll(&mut cx), Poll::Ready(Ok(23)));
        let cancel = queue.try_push(29, canceled_sender).ok().unwrap();
        let canceled = Observed::new(canceled_receiver, cancel);
        drop(canceled);
        assert!(queue.drain().next().is_none());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn canceled_value_and_reply_destructors_can_reenter_after_detachment() {
    struct Reenter(fn());
    impl Drop for Reenter {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    std::thread_local! {
        static STATE: RefCell<State<Reenter, Reenter, Fixed<Reenter, Reenter, 1>>> = const { RefCell::new(State::new(Fixed::new())) };
        static DROPS: Cell<usize> = const { Cell::new(0) };
    }
    fn callback() {
        STATE.with(|state| assert!(state.try_borrow_mut().is_ok()));
        DROPS.with(|drops| drops.set(drops.get() + 1));
    }
    STATE.with(|state| {
        DROPS.with(|_| {});
        let measured = allocation_counter::measure(|| {
            let id = state
                .borrow_mut()
                .try_insert(Reenter(callback), Reenter(callback))
                .ok()
                .unwrap();
            let remover = Remover::new(id, state);
            drop(remover);
            assert!(state.borrow().first.is_none());
        });
        assert_eq!(measured.count_total, 0);
        assert_eq!(measured.bytes_total, 0);
        assert_eq!(DROPS.with(Cell::get), 2);
    });
}

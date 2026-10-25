//! Concrete deferred owners preserve borrowed resources without boxing futures.
use super::Promise;
use core::{
    cell::{Cell, RefCell, RefMut},
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    task::{Context, Poll, Waker},
};

#[test]
fn concrete_promise_owns_a_pending_storage_guard_and_releases_it_on_drop() {
    struct Operation<'a> {
        guard: RefMut<'a, u32>,
        drops: &'a Cell<usize>,
    }
    impl Future for Operation<'_> {
        type Output = Result<u32, ()>;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            assert_eq!(*self.guard, 42);
            Poll::Pending
        }
    }
    impl Drop for Operation<'_> {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    let measured = allocation_counter::measure(|| {
        let data = RefCell::new(42);
        let drops = Cell::new(0);
        let mut promise = Promise::from_owner(Operation {
            guard: data.borrow_mut(),
            drops: &drops,
        });
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut promise).poll(&mut cx).is_pending());
        assert!(data.try_borrow_mut().is_err());
        drop(promise);
        assert_eq!(drops.get(), 1);
        assert!(data.try_borrow_mut().is_ok());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn caller_pinned_non_unpin_future_keeps_its_address_when_the_promise_moves() {
    struct Operation<'a> {
        address: Cell<usize>,
        ready: &'a Cell<bool>,
        value: &'a u32,
        _pin: PhantomPinned,
    }
    impl Future for Operation<'_> {
        type Output = Result<&'static str, ()>;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            let address = core::ptr::from_ref(self.as_ref().get_ref()).addr();
            let old = self.address.replace(address);
            assert!(old == 0 || old == address);
            assert_eq!(*self.value, 42);
            if self.ready.get() {
                Poll::Ready(Ok("kept"))
            } else {
                Poll::Pending
            }
        }
    }
    let measured = allocation_counter::measure(|| {
        let value = 42;
        let ready = Cell::new(false);
        let mut future = core::pin::pin!(Operation {
            address: Cell::new(0),
            ready: &ready,
            value: &value,
            _pin: PhantomPinned,
        });
        let mut promise = Promise::from_owner(future.as_mut());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut promise).poll(&mut cx).is_pending());
        let mut moved = promise;
        ready.set(true);
        assert_eq!(Pin::new(&mut moved).poll(&mut cx), Poll::Ready(Ok("kept")));
        // The Pin reference borrows the future; it does not acquire destruction
        // responsibility. A caller task-place owner provides that separately.
        drop(moved);
        assert_ne!(future.address.get(), 0);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn concrete_future_errors_do_not_allocate_even_with_the_alloc_feature_enabled() {
    let measured = allocation_counter::measure(|| {
        let error = [17u8; 257];
        let mut promise = Promise::from_owner(core::future::ready(Err::<(), _>(error)));
        assert_eq!(
            Pin::new(&mut promise).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(error))
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn immediate_non_unpin_outputs_are_moved_without_pinning_the_result() {
    struct Output<'a>(&'a Cell<usize>, PhantomPinned);
    impl Drop for Output<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let measured = allocation_counter::measure(|| {
        let drops = Cell::new(0);
        let mut promise = Promise::<_, ()>::ok(Output(&drops, PhantomPinned));
        let Poll::Ready(Ok(output)) =
            Pin::new(&mut promise).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("immediate output lost")
        };
        drop(promise);
        assert_eq!(drops.get(), 0);
        drop(output);
        assert_eq!(drops.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[cfg(not(feature = "alloc"))]
#[test]
fn fixed_immediate_errors_and_result_conversion_need_no_allocator() {
    let measured = allocation_counter::measure(|| {
        let mut rejected = Promise::<(), _>::err([42u8; 257]);
        let mut converted = Promise::<u32, ()>::from(Ok(42));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(
            Pin::new(&mut rejected).poll(&mut cx),
            Poll::Ready(Err([42u8; 257]))
        );
        assert_eq!(Pin::new(&mut converted).poll(&mut cx), Poll::Ready(Ok(42)));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

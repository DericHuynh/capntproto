extern crate std;
use super::*;
use crate::readiness::Storage;
use core::{
    cell::{Cell, RefCell},
    marker::PhantomPinned,
};
use std::{sync::Arc, task::Wake};

struct Notify<const N: usize> {
    index: usize,
    ready: Arc<Storage<N>>,
}
impl<const N: usize> Wake for Notify<N> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.queue().enqueue(self.index, || {});
    }
}
fn wakers<const N: usize>(ready: &Arc<Storage<N>>) -> [Waker; N] {
    core::array::from_fn(|index| {
        Waker::from(Arc::new(Notify {
            index,
            ready: ready.clone(),
        }))
    })
}

struct Borrowed<'a> {
    id: usize,
    ready: &'a Cell<bool>,
    saved: &'a RefCell<Option<Waker>>,
    address: Cell<usize>,
    _pinned: PhantomPinned,
}
impl Future for Borrowed<'_> {
    type Output = usize;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<usize> {
        let address = core::ptr::from_ref(self.as_ref().get_ref()).addr();
        let previous = self.address.replace(address);
        assert!(previous == 0 || previous == address, "pinned future moved");
        if self.ready.get() {
            Poll::Ready(self.id)
        } else {
            *self.saved.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[test]
fn caller_pinned_heterogeneous_loans_suspend_wake_and_complete_without_allocating() {
    let storage = Arc::new(Storage::<4>::new());
    let wakes = wakers(&storage);
    let flags = [const { Cell::new(false) }; 4];
    let saved = [const { RefCell::new(None) }; 4];
    let mut first = core::pin::pin!(Borrowed {
        id: 0,
        ready: &flags[0],
        saved: &saved[0],
        address: Cell::new(0),
        _pinned: PhantomPinned
    });
    let mut second = core::pin::pin!(Borrowed {
        id: 1,
        ready: &flags[1],
        saved: &saved[1],
        address: Cell::new(0),
        _pinned: PhantomPinned
    });
    let mut third = core::pin::pin!(Borrowed {
        id: 2,
        ready: &flags[2],
        saved: &saved[2],
        address: Cell::new(0),
        _pinned: PhantomPinned
    });
    let mut fourth = core::pin::pin!(Borrowed {
        id: 3,
        ready: &flags[3],
        saved: &saved[3],
        address: Cell::new(0),
        _pinned: PhantomPinned
    });
    let mut cursor = 0;
    let measured = allocation_counter::measure(|| {
        let loans: [Pin<&mut dyn Future<Output = usize>>; 4] = [
            first.as_mut(),
            second.as_mut(),
            third.as_mut(),
            fourth.as_mut(),
        ];
        let mut pinned = core::pin::pin!(PinnedSlots::new());
        let mut set = Set::new(pinned.as_mut(), storage.queue(), &wakes, &mut cursor);
        for loan in loans {
            assert!(set.try_push(loan, || {}).is_ok());
        }
        let mut cx = Context::from_waker(Waker::noop());
        assert!(set.poll_next(&mut cx).is_pending());
        for index in (0..4).rev() {
            flags[index].set(true);
            saved[index].borrow_mut().take().unwrap().wake();
        }
        for index in (0..4).rev() {
            assert_eq!(set.poll_next(&mut cx), Poll::Ready(Some(index)));
        }
        assert_eq!(set.poll_next(&mut cx), Poll::Ready(None));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn full_capacity_returns_the_future_without_polling_or_dropping_it() {
    struct Task<'a>(&'a Cell<usize>);
    impl Future for Task<'_> {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            panic!("not polled");
        }
    }
    impl Drop for Task<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let storage = Storage::<1>::new();
    let wakes = [Waker::noop().clone()];
    let mut cursor = 0;
    let drops = Cell::new(0);
    let mut pinned = core::pin::pin!(PinnedSlots::new());
    let mut set = Set::new(pinned.as_mut(), storage.queue(), &wakes, &mut cursor);
    assert!(set.try_push(Task(&drops), || {}).is_ok());
    let rejected = set
        .try_push(Task(&drops), || panic!("not admitted"))
        .err()
        .unwrap();
    assert_eq!(drops.get(), 0);
    drop(rejected);
    assert_eq!(drops.get(), 1);
    drop(set);
    assert_eq!(drops.get(), 2);
}

#[test]
fn self_waking_task_cannot_monopolize_a_poll_rotation() {
    let storage = Arc::new(Storage::<1>::new());
    let wakes = wakers(&storage);
    let mut cursor = 0;
    let polls = Cell::new(0);
    let mut pinned = core::pin::pin!(PinnedSlots::new());
    let mut set = Set::new(pinned.as_mut(), storage.queue(), &wakes, &mut cursor);
    assert!(set
        .try_push(
            core::future::poll_fn(|cx| {
                let n = polls.get() + 1;
                polls.set(n);
                cx.waker().wake_by_ref();
                if n == 3 {
                    Poll::Ready(n)
                } else {
                    Poll::Pending
                }
            }),
            || {}
        )
        .is_ok());
    let mut cx = Context::from_waker(Waker::noop());
    for n in 1..=3 {
        assert_eq!(
            set.poll_next(&mut cx),
            if n == 3 {
                Poll::Ready(Some(3))
            } else {
                Poll::Pending
            }
        );
        assert_eq!(polls.get(), n);
    }
}

#[test]
fn escaped_old_wake_only_polls_a_replacement_and_never_completes_it() {
    let storage = Arc::new(Storage::<1>::new());
    let wakes = wakers(&storage);
    let old = wakes[0].clone();
    let mut cursor = 0;
    {
        let mut pinned = core::pin::pin!(PinnedSlots::new());
        let mut set = Set::new(pinned.as_mut(), storage.queue(), &wakes, &mut cursor);
        assert!(set.try_push(core::future::ready(7), || {}).is_ok());
        assert_eq!(
            set.poll_next(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Some(7))
        );
    }
    old.wake_by_ref();
    let ready = Cell::new(false);
    let polls = Cell::new(0);
    let notified = Cell::new(0);
    let mut pinned = core::pin::pin!(PinnedSlots::new());
    let mut replacement = Set::new(pinned.as_mut(), storage.queue(), &wakes, &mut cursor);
    assert!(replacement
        .try_push(
            core::future::poll_fn(|_| {
                polls.set(polls.get() + 1);
                if ready.get() {
                    Poll::Ready(9)
                } else {
                    Poll::Pending
                }
            }),
            || notified.set(notified.get() + 1)
        )
        .is_ok());
    assert_eq!(notified.get(), 1);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(replacement.poll_next(&mut cx).is_pending());
    old.wake_by_ref();
    assert!(replacement.poll_next(&mut cx).is_pending());
    assert_eq!(polls.get(), 2);
    ready.set(true);
    wakes[0].wake_by_ref();
    assert_eq!(replacement.poll_next(&mut cx), Poll::Ready(Some(9)));
}

#[test]
fn static_wakers_and_task_storage_allocate_nothing_even_after_owner_teardown() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static READY: Storage<1> = Storage::new();
    static NOTIFIED: AtomicUsize = AtomicUsize::new(0);
    fn notify() {
        NOTIFIED.fetch_add(1, Ordering::Relaxed);
    }
    static SIGNALS: [Signal<1>; 1] = signals(&READY, notify);
    let measured = allocation_counter::measure(|| {
        let wakes = [SIGNALS[0].waker()];
        let escaped = wakes[0].clone();
        let mut cursor = 0;
        let polls = Cell::new(0);
        let mut cx = Context::from_waker(Waker::noop());
        {
            let mut pinned = core::pin::pin!(PinnedSlots::new());
            let mut set = Set::new(pinned.as_mut(), READY.queue(), &wakes, &mut cursor);
            assert!(set
                .try_push(
                    core::future::poll_fn(|cx| {
                        polls.set(polls.get() + 1);
                        cx.waker().wake_by_ref();
                        if polls.get() == 3 {
                            Poll::Ready(7)
                        } else {
                            Poll::Pending
                        }
                    }),
                    notify
                )
                .is_ok());
            assert!(set.poll_next(&mut cx).is_pending());
            assert!(set.poll_next(&mut cx).is_pending());
            assert_eq!(set.poll_next(&mut cx), Poll::Ready(Some(7)));
        }
        escaped.wake_by_ref();
        {
            let mut pinned = core::pin::pin!(PinnedSlots::new());
            let mut replacement = Set::new(pinned.as_mut(), READY.queue(), &wakes, &mut cursor);
            assert!(replacement
                .try_push(core::future::ready(99), notify)
                .is_ok());
            assert_eq!(replacement.poll_next(&mut cx), Poll::Ready(Some(99)));
        }
        escaped.wake();
        assert_eq!(READY.queue().front(cursor), Some(0));
        READY.queue().consume(cursor, 0);
    });
    assert!(NOTIFIED.load(Ordering::Relaxed) > 0);
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn owned_pinned_futures_release_resources_immediately_on_completion_and_cancel() {
    struct Owned<'a> {
        drops: &'a Cell<usize>,
        address: Cell<usize>,
        _pin: PhantomPinned,
    }
    impl Future for Owned<'_> {
        type Output = usize;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<usize> {
            self.address
                .set(core::ptr::from_ref(self.as_ref().get_ref()).addr());
            Poll::Ready(7)
        }
    }
    impl Drop for Owned<'_> {
        fn drop(&mut self) {
            if self.address.get() != 0 {
                assert_eq!(self.address.get(), core::ptr::from_ref(self).addr());
            }
            self.drops.set(self.drops.get() + 1);
        }
    }
    let ready = Storage::<1>::new();
    let wakes = [Waker::noop().clone()];
    let drops = Cell::new(0);
    let mut pinned = core::pin::pin!(PinnedSlots::new());
    let mut cursor = 0;
    let measured = allocation_counter::measure(|| {
        let mut set = Set::new(pinned.as_mut(), ready.queue(), &wakes, &mut cursor);
        for expected in 1..=4 {
            assert!(set
                .try_push(
                    Owned {
                        drops: &drops,
                        address: Cell::new(0),
                        _pin: PhantomPinned
                    },
                    || {}
                )
                .is_ok());
            assert_eq!(
                set.poll_next(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Some(7))
            );
            assert_eq!(drops.get(), expected);
        }
        assert!(set
            .try_push(
                Owned {
                    drops: &drops,
                    address: Cell::new(0),
                    _pin: PhantomPinned
                },
                || {}
            )
            .is_ok());
        drop(set);
        assert_eq!(drops.get(), 5);
    });
    assert!(pinned.is_empty());
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

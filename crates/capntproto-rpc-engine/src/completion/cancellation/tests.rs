extern crate std;

use super::*;
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Wake,
};

#[derive(Default)]
struct Storage {
    canceled: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}
impl Storage {
    fn cancellation(&self) -> Cancellation<'_> {
        Cancellation::new(&self.canceled, &self.waker)
    }
}
impl Owner for Storage {
    fn cancellation(&self) -> Cancellation<'_> {
        Storage::cancellation(self)
    }
}

#[test]
fn owned_task_and_borrowed_cancel_interest_do_not_allocate_or_repoll_canceled_work() {
    let measured = allocation_counter::measure(|| {
        let storage = Storage::default();
        let polls = Cell::new(0);
        let work = core::future::poll_fn(|_| {
            polls.set(polls.get() + 1);
            Poll::<Result<(), ()>>::Pending
        });
        let mut task = Task::new(&storage, work);
        let interest = Cancel::new(&storage);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
        assert!(storage.waker.borrow().is_some());
        drop(interest);
        assert!(storage.canceled.get());
        assert_eq!(Pin::new(&mut task).poll(&mut cx), Poll::Ready(Ok(())));
        assert_eq!(polls.get(), 1);
        drop(task);
        assert!(storage.waker.borrow().is_none());
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}

#[test]
fn owned_task_clears_before_status_and_future_destruction_even_unpolled() {
    struct Status<'a>(&'a Storage, &'a Cell<u8>);
    impl Owner for Status<'_> {
        fn cancellation(&self) -> Cancellation<'_> {
            self.0.cancellation()
        }
    }
    impl Drop for Status<'_> {
        fn drop(&mut self) {
            assert!(self.0.waker.try_borrow_mut().unwrap().is_none());
            assert_eq!(self.1.replace(1), 0);
        }
    }
    struct Work<'a>(&'a Storage, &'a Cell<u8>);
    impl Future for Work<'_> {
        type Output = Result<(), ()>;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }
    impl Drop for Work<'_> {
        fn drop(&mut self) {
            assert!(self.0.waker.try_borrow_mut().unwrap().is_none());
            assert_eq!(self.1.replace(2), 1);
        }
    }
    for polled in [false, true] {
        let storage = Storage::default();
        let events = Cell::new(0);
        let mut task = Task::new(Status(&storage, &events), Work(&storage, &events));
        if polled {
            assert!(Pin::new(&mut task)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            assert!(storage.waker.borrow().is_some());
        }
        drop(task);
        assert_eq!(events.get(), 2);
    }
}

#[test]
fn owned_task_discards_inner_error_after_publication_and_clears_registration() {
    struct Error<'a>(&'a Cell<usize>);
    impl Drop for Error<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let storage = Storage::default();
    let dropped = Cell::new(0);
    let mut task = Task::new(&storage, core::future::ready(Err::<(), _>(Error(&dropped))));
    assert!(matches!(
        Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(dropped.get(), 1);
    assert!(storage.waker.borrow().is_none());
}

#[test]
fn borrowed_task_registration_cancellation_and_reuse_do_not_allocate() {
    let measured = allocation_counter::measure(|| {
        let storage = Storage::default();
        let cancellation = storage.cancellation();
        let polls = Cell::new(0);
        let ready = Cell::new(false);
        let mut task = core::future::poll_fn(|_| {
            assert!(storage.waker.try_borrow_mut().is_ok());
            polls.set(polls.get() + 1);
            if ready.get() {
                Poll::Ready(Err::<(), _>(17))
            } else {
                Poll::Pending
            }
        });
        let mut cx = Context::from_waker(Waker::noop());
        assert!(cancellation
            .poll_task(Pin::new(&mut task), &mut cx)
            .is_pending());
        assert!(cancellation
            .poll_task(Pin::new(&mut task), &mut cx)
            .is_pending());
        assert_eq!(polls.get(), 2);
        cancellation.cancel();
        assert!(cancellation
            .poll_task(Pin::new(&mut task), &mut cx)
            .is_ready());
        assert_eq!(polls.get(), 2);
        assert!(storage.waker.borrow().is_none());

        // The previous task is no longer observed before reusing caller storage.
        cancellation.clear();
        storage.canceled.set(false);
        ready.set(true);
        assert_eq!(
            cancellation.poll_task(Pin::new(&mut task), &mut cx),
            Poll::Ready(())
        );
        assert_eq!(polls.get(), 3);
        assert!(storage.waker.borrow().is_none());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn repeated_poll_reuses_registration_and_cancellation_wakes_once() {
    let storage = Storage::default();
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut task = core::future::pending::<()>();
    for _ in 0..3 {
        assert!(storage
            .cancellation()
            .poll_task(Pin::new(&mut task), &mut cx)
            .is_pending());
        assert_eq!(Arc::strong_count(&wakes), 3);
    }
    storage.cancellation().cancel();
    storage.cancellation().cancel();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(Arc::strong_count(&wakes), 2);
}

// Safe Wake adapters reenter the single-threaded cells through thread-local
// test storage, without weakening the engine's unsafe-code prohibition.
std::thread_local! {
    static REENTRANT: RefCell<Option<Rc<Storage>>> = const { RefCell::new(None) };
}
fn storage() -> Rc<Storage> {
    REENTRANT.with(|slot| slot.borrow().as_ref().unwrap().clone())
}
struct CancelOnDrop;
#[allow(clippy::manual_noop_waker)] // The destructor must run; Waker::noop has none.
impl Wake for CancelOnDrop {
    fn wake(self: Arc<Self>) {}
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        storage().cancellation().cancel();
    }
}

#[test]
fn replacing_registration_can_cancel_before_the_future_is_polled() {
    let storage = Rc::new(Storage::default());
    REENTRANT.with(|slot| *slot.borrow_mut() = Some(storage.clone()));
    *storage.waker.borrow_mut() = Some(Waker::from(Arc::new(CancelOnDrop)));
    let mut task =
        core::future::poll_fn(|_| -> Poll<()> { panic!("canceled task must not be polled") });
    assert!(storage
        .cancellation()
        .poll_task(Pin::new(&mut task), &mut Context::from_waker(Waker::noop()))
        .is_ready());
    assert!(storage.canceled.get());
    assert!(storage.waker.borrow().is_none());
    REENTRANT.with(|slot| slot.borrow_mut().take());
}

#[test]
fn teardown_drops_registration_outside_the_cell_borrow() {
    let storage = Rc::new(Storage::default());
    REENTRANT.with(|slot| *slot.borrow_mut() = Some(storage.clone()));
    *storage.waker.borrow_mut() = Some(Waker::from(Arc::new(CancelOnDrop)));
    storage.cancellation().clear();
    assert!(storage.canceled.get());
    assert!(storage.waker.borrow().is_none());
    REENTRANT.with(|slot| slot.borrow_mut().take());
}

struct ReenterOnWake;
impl Wake for ReenterOnWake {
    fn wake(self: Arc<Self>) {
        let storage = storage();
        assert!(storage.canceled.get());
        assert!(storage.waker.try_borrow_mut().unwrap().is_none());
        storage.cancellation().cancel();
        let mut task = core::future::poll_fn(|_| -> Poll<()> {
            panic!("reentrant poll must see cancellation")
        });
        assert!(storage
            .cancellation()
            .poll_task(Pin::new(&mut task), &mut Context::from_waker(Waker::noop()))
            .is_ready());
    }
}

#[test]
fn wake_can_reenter_after_cancellation_is_published() {
    let storage = Rc::new(Storage::default());
    REENTRANT.with(|slot| *slot.borrow_mut() = Some(storage.clone()));
    *storage.waker.borrow_mut() = Some(Waker::from(Arc::new(ReenterOnWake)));
    storage.cancellation().cancel();
    REENTRANT.with(|slot| slot.borrow_mut().take());
}

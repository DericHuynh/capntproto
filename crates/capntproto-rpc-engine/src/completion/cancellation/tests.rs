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

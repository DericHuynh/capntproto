extern crate std;
use super::*;
use core::cell::Cell;
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Wake,
};

struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
#[test]
fn inline_reply_has_no_allocation_and_reuses_its_registered_waker() {
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let measured = allocation_counter::measure(|| {
        let owner = RefCell::new(Reply::new());
        let mut cx = Context::from_waker(&waker);
        assert!(poll(&owner, &mut cx).is_pending());
        assert!(poll(&owner, &mut cx).is_pending());
        assert_eq!(Arc::strong_count(&wakes), 3);
        assert!(deliver(&owner, core::future::ready(42)).is_none());
        assert_eq!(poll(&owner, &mut cx), Poll::Ready(42));
        release(&owner);
        assert!(deliver(&owner, core::future::ready(7)).is_some());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
}

struct Deferred {
    owner: std::rc::Weak<RefCell<Reply<Self>>>,
    ready: Rc<Cell<bool>>,
    drops: Rc<Cell<usize>>,
}
impl Future for Deferred {
    type Output = u32;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<u32> {
        assert!(self.owner.upgrade().unwrap().try_borrow_mut().is_ok());
        if self.ready.get() {
            Poll::Ready(9)
        } else {
            Poll::Pending
        }
    }
}
impl Drop for Deferred {
    fn drop(&mut self) {
        assert!(self.owner.upgrade().unwrap().try_borrow_mut().is_ok());
        self.drops.set(self.drops.get() + 1);
    }
}
#[test]
fn suspended_reply_is_retained_and_user_code_runs_outside_owner_borrows() {
    // Explicit release ends the borrowed payload before its slot is destroyed.
    let ready = Rc::new(Cell::new(false));
    let drops = Rc::new(Cell::new(0));
    let owner = Rc::new(RefCell::new(Reply::new()));
    assert!(deliver(
        &*owner,
        Deferred {
            owner: Rc::downgrade(&owner),
            ready: ready.clone(),
            drops: drops.clone()
        }
    )
    .is_none());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(poll(&*owner, &mut cx).is_pending());
    assert!(poll(&*owner, &mut cx).is_pending());
    ready.set(true);
    assert_eq!(poll(&*owner, &mut cx), Poll::Ready(9));
    assert_eq!(drops.get(), 1);
    release(&*owner);
}

// Reenter a single-threaded owner through a safe, thread-local Wake adapter.
type ReentrantOwner = Rc<RefCell<Reply<core::future::Ready<u32>>>>;
std::thread_local! {
    static REENTRANT: RefCell<Option<ReentrantOwner>> = const { RefCell::new(None) };
}
struct Reenter;
impl Wake for Reenter {
    fn wake(self: Arc<Self>) {
        REENTRANT.with(|slot| {
            let owner = slot.borrow().as_ref().unwrap().clone();
            assert_eq!(
                poll(&*owner, &mut Context::from_waker(Waker::noop())),
                Poll::Ready(17)
            );
        });
    }
}
#[test]
fn publication_can_synchronously_reenter_the_question() {
    let owner = Rc::new(RefCell::new(Reply::new()));
    REENTRANT.with(|slot| *slot.borrow_mut() = Some(owner.clone()));
    let waker = Waker::from(Arc::new(Reenter));
    assert!(poll(&*owner, &mut Context::from_waker(&waker)).is_pending());
    assert!(deliver(&*owner, core::future::ready(17)).is_none());
    release(&*owner);
    REENTRANT.with(|slot| slot.borrow_mut().take());
}
#[test]
fn cancellation_and_replacement_return_affine_values_for_cleanup() {
    let owner = RefCell::new(Reply::<u32>::new());
    assert_eq!(deliver(&owner, 3), None);
    assert_eq!(deliver(&owner, 5), Some(3));
    release(&owner);
    assert_eq!(deliver(&owner, 7), Some(7));
    assert!(owner.borrow().value.is_none());
}

use super::*;
use crate::{
    completion::direct::{self, Reply},
    question::{
        release::{Last, Owner as ReleaseOwner, Publication},
        Finish, Question, Release,
    },
    wake::{Static, Wake},
};
use core::{
    cell::{Cell, RefCell, RefMut},
    sync::atomic::{AtomicUsize, Ordering},
    task::Waker,
};
struct Count(&'static AtomicUsize);
impl Wake for Count {
    fn wake_by_ref(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
struct Operation<'a, F> {
    reply: RefCell<Reply<F>>,
    question: RefCell<Question>,
    params: Option<&'a RefCell<u32>>,
    releases: Cell<usize>,
    finishes: Cell<usize>,
    reclaimed: Cell<bool>,
    release_caps: Cell<bool>,
}
impl<'a, F> Operation<'a, F> {
    fn new(params: Option<&'a RefCell<u32>>) -> Self {
        Self {
            reply: RefCell::new(Reply::new()),
            question: RefCell::new(Question::new()),
            params,
            releases: Cell::new(0),
            finishes: Cell::new(0),
            reclaimed: Cell::new(false),
            release_caps: Cell::new(false),
        }
    }
}
impl<F> ReleaseOwner for Operation<'_, F> {
    type Key = u32;
    fn release(&self, key: &u32) -> Option<Release> {
        assert_eq!(*key, 1);
        // Receiver cancellation must finish destroying the user future and its
        // storage guard before last-interest cleanup invokes protocol effects.
        let reply = self.reply.borrow();
        assert!(reply.observed);
        assert!(reply.value.is_none());
        drop(reply);
        if let Some(params) = self.params {
            assert!(params.try_borrow_mut().is_ok());
        }
        self.releases.set(self.releases.get() + 1);
        Some(self.question.borrow_mut().release())
    }
    fn finish(&self, _: &u32, finish: Finish, _: bool) -> Publication {
        assert!(self.reply.try_borrow_mut().is_ok());
        assert!(self.question.try_borrow_mut().is_ok());
        self.finishes.set(self.finishes.get() + 1);
        self.release_caps.set(finish.release_result_caps);
        Publication::Accepted
    }
    fn reclaim(&self, _: &u32) {
        self.reclaimed.set(true);
    }
    fn idle(&self) {
        assert!(self.reply.try_borrow_mut().is_ok());
    }
}
// The finalizer already retains the stable context. No duplicated pointer,
// reference count, allocation or manually erased ownership is needed.
struct Lease<'s, 'a, F> {
    last: Last<'s, Operation<'a, F>>,
}
impl<F> Deref for Lease<'_, '_, F> {
    type Target = RefCell<Reply<F>>;
    fn deref(&self) -> &Self::Target {
        &self.last.owner().reply
    }
}

#[test]
fn borrowed_receiver_observes_real_wake_and_retains_output_after_question_release() {
    static WAKES: AtomicUsize = AtomicUsize::new(0);
    static SIGNAL: Static<Count> = Static::new(Count(&WAKES));
    let measured = allocation_counter::measure(|| {
        let bytes = [17u8; 32];
        let operation = Operation::new(None);
        let lease = Lease {
            last: Last::new(&operation, 1),
        };
        let mut receiver = Receiver::new(lease);
        let waker = SIGNAL.waker();
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
        assert_eq!(WAKES.load(Ordering::Relaxed), 0);
        operation.question.borrow_mut().returned(false).unwrap();
        assert!(direct::deliver(&operation.reply, core::future::ready(&bytes[..])).is_none());
        assert_eq!(WAKES.load(Ordering::Relaxed), 1);
        let Poll::Ready(response) = Pin::new(&mut receiver).poll(&mut cx) else {
            panic!("response not ready")
        };
        assert_eq!(operation.releases.get(), 1);
        assert_eq!(operation.finishes.get(), 1);
        assert!(!operation.release_caps.get());
        assert!(operation.reclaimed.get());
        drop(receiver);
        assert_eq!(response, &[17u8; 32]);
        assert_eq!(operation.releases.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn canceling_receiver_destroys_suspended_future_before_borrowed_protocol_cleanup() {
    struct Waiting<'a> {
        guard: RefMut<'a, u32>,
        drops: &'a Cell<usize>,
    }
    impl Future for Waiting<'_> {
        type Output = u32;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<u32> {
            assert_eq!(*self.guard, 42);
            Poll::Pending
        }
    }
    impl Drop for Waiting<'_> {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    let measured = allocation_counter::measure(|| {
        let params = RefCell::new(42);
        let drops = Cell::new(0);
        let operation = Operation::new(Some(&params));
        let mut receiver = Receiver::new(Lease {
            last: Last::new(&operation, 1),
        });
        assert!(direct::deliver(
            &operation.reply,
            Waiting {
                guard: params.borrow_mut(),
                drops: &drops
            }
        )
        .is_none());
        assert!(Pin::new(&mut receiver)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(params.try_borrow_mut().is_err());
        drop(receiver);
        assert_eq!(drops.get(), 1);
        assert_eq!(operation.releases.get(), 1);
        assert!(operation.release_caps.get());
        assert!(!operation.reclaimed.get());
        assert_eq!(operation.finishes.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn transfer_keeps_unique_observation_without_canceling_success_or_error() {
    let measured = allocation_counter::measure(|| {
        for result in [Ok(42u8), Err(7u8)] {
            let operation = Operation::new(None);
            let receiver = Receiver::new(Lease {
                last: Last::new(&operation, 1),
            });
            let transferred = receiver.into_owner();
            assert_eq!(operation.releases.get(), 0);
            assert!(!operation.reply.borrow().observed);
            operation.question.borrow_mut().returned(true).unwrap();
            assert!(direct::deliver(&operation.reply, core::future::ready(result)).is_none());
            let mut receiver = Receiver::new(transferred);
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(Pin::new(&mut receiver).poll(&mut cx), Poll::Ready(result));
            assert_eq!(operation.releases.get(), 1);
            assert_eq!(operation.finishes.get(), 0);
            assert!(operation.reclaimed.get());
            let mut late =
                direct::deliver(&operation.reply, core::future::ready(Err(99u8))).unwrap();
            assert_eq!(Pin::new(&mut late).poll(&mut cx), Poll::Ready(Err(99)));
            drop(receiver);
            assert_eq!(operation.releases.get(), 1);
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn borrowed_erased_storage_uses_the_same_receiver_without_allocating_a_wrapper() {
    let measured = allocation_counter::measure(|| {
        let storage = RefCell::new(Reply::new());
        let erased: &dyn direct::Owner<core::future::Ready<u32>> = &storage;
        let mut receiver = Receiver::new(erased);
        assert!(direct::deliver(erased, core::future::ready(42)).is_none());
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(Pin::new(&mut receiver).poll(&mut cx), Poll::Ready(42));
        drop(receiver);
        let mut rejected = direct::deliver(erased, core::future::ready(99)).unwrap();
        assert_eq!(Pin::new(&mut rejected).poll(&mut cx), Poll::Ready(99));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

use super::*;
use futures::Future;
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

struct State(Box<dyn Fn()>);
impl Connection for State {
    fn disconnect(&self, _: Error) {
        (self.0)();
    }
}
fn reserve(registry: &Connections<State>, id: usize) -> Registration<State> {
    match registry.acquire(id).unwrap() {
        Acquired::New(registration) => registration,
        Acquired::Existing(_) => panic!("expected a new registration"),
    }
}

#[test]
fn capacity_counts_initializing_and_detached_tasks_but_allows_live_reuse() {
    let registry = Connections::<State>::new();
    let Acquired::New(first) = registry.acquire_bounded(1, 1).unwrap() else {
        panic!("new identity");
    };
    assert!(matches!(registry.acquire_bounded(2, 1),
        Err(error) if error.kind == capnp::ErrorKind::Overloaded));
    let state = state();
    first.publish(state.clone()).unwrap();
    assert!(matches!(
        registry.acquire_bounded(1, 1),
        Ok(Acquired::Existing(_))
    ));
    first.detach();
    assert!(matches!(registry.acquire_bounded(2, 1),
        Err(error) if error.kind == capnp::ErrorKind::Overloaded));
    first.complete(Ok(()));
    let Acquired::New(second) = registry.acquire_bounded(2, 1).unwrap() else {
        panic!("completed task must return capacity");
    };
    second.complete(Ok(()));
}
fn state() -> Rc<State> {
    Rc::new(State(Box::new(|| {})))
}
fn poll<T>(future: &mut (impl Future<Output = T> + Unpin)) -> Poll<T> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}
#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn all_shutdown_observers_wake_and_receive_the_same_failure() {
    let registry = Connections::<State>::new();
    let first = reserve(&registry, 1);
    let second = reserve(&registry, 2);
    let mut left = registry.closed();
    let mut right = registry.closed();
    let left_wakes = Arc::new(Wakes::default());
    let right_wakes = Arc::new(Wakes::default());
    let lw = Waker::from(left_wakes.clone());
    let rw = Waker::from(right_wakes.clone());
    assert!(Pin::new(&mut left)
        .poll(&mut Context::from_waker(&lw))
        .is_pending());
    assert!(Pin::new(&mut right)
        .poll(&mut Context::from_waker(&rw))
        .is_pending());
    registry.begin_close();
    first.complete(Err(Error::failed("first failure".into())));
    assert!(poll(&mut left).is_pending());
    // Restore the real waker after checking that one pending registration blocks close.
    assert!(Pin::new(&mut left)
        .poll(&mut Context::from_waker(&lw))
        .is_pending());
    let before_left = left_wakes.0.load(Ordering::Relaxed);
    let before_right = right_wakes.0.load(Ordering::Relaxed);
    second.complete(Err(Error::failed("later failure".into())));
    assert!(left_wakes.0.load(Ordering::Relaxed) > before_left);
    assert!(right_wakes.0.load(Ordering::Relaxed) > before_right);
    for future in [&mut left, &mut right] {
        let Poll::Ready(Err(error)) = poll(future) else {
            panic!("missing failure")
        };
        assert_eq!(error.extra, "first failure");
    }
}

#[test]
fn cancellation_before_first_poll_retires_reservation() {
    let registry = Connections::<State>::new();
    let registration = reserve(&registry, 1);
    let task = async move {
        futures::future::pending::<()>().await;
        registration.complete(Ok(()));
    };
    registry.begin_close();
    let mut closed = registry.closed();
    assert!(poll(&mut closed).is_pending());
    drop(task);
    assert!(registry.0.entries.borrow().is_empty());
    assert!(matches!(poll(&mut closed), Poll::Ready(Err(error))
        if error.kind == capnp::ErrorKind::Disconnected));
}

#[test]
fn detached_connection_is_counted_until_transport_finishes() {
    let registry = Connections::<State>::new();
    let registration = reserve(&registry, 1);
    let state = state();
    registration.publish(state.clone()).unwrap();
    registry.detach(1, &state);
    assert!(registry.states().is_empty());
    assert!(registry.begin_close().is_empty());
    let mut closed = registry.closed();
    assert!(poll(&mut closed).is_pending());
    registration.complete(Ok(()));
    assert!(matches!(poll(&mut closed), Poll::Ready(Ok(()))));
}

#[test]
fn stale_completion_cannot_remove_reused_transport_identity() {
    let registry = Connections::<State>::new();
    let old = reserve(&registry, 7);
    let old_state = state();
    old.publish(old_state.clone()).unwrap();
    registry.detach(7, &old_state);
    let replacement = reserve(&registry, 7);
    let new_state = state();
    replacement.publish(new_state.clone()).unwrap();
    registry.detach(7, &old_state);
    old.complete(Ok(()));
    let Acquired::Existing(found) = registry.acquire(7).unwrap() else {
        panic!("lost replacement")
    };
    assert!(Rc::ptr_eq(&found, &new_state));
    assert_eq!(registry.begin_close().len(), 1);
    let mut closed = registry.closed();
    assert!(poll(&mut closed).is_pending());
    registry.detach(7, &new_state);
    replacement.complete(Ok(()));
    assert!(matches!(poll(&mut closed), Poll::Ready(Ok(()))));
}

#[test]
fn closing_during_initialization_blocks_publication_but_waits_for_shutdown() {
    let registry = Connections::<State>::new();
    let registration = reserve(&registry, 1);
    assert!(
        matches!(registry.acquire(1), Err(error) if error.kind == capnp::ErrorKind::Overloaded)
    );
    assert!(registry.begin_close().is_empty());
    assert!(registration.publish(state()).is_err());
    assert!(registry.acquire(2).is_err());
    let mut closed = registry.closed();
    assert!(poll(&mut closed).is_pending());
    registration.complete(Ok(()));
    assert!(matches!(poll(&mut closed), Poll::Ready(Ok(()))));
}

#[test]
fn canceled_live_state_can_reenter_registry_during_disconnect() {
    let registry = Connections::<State>::new();
    let reentrant = registry.clone();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let registration = reserve(&registry, 1);
    registration
        .publish(Rc::new(State(Box::new(move || {
            assert!(reentrant.states().is_empty());
            // Cancellation removed the old ID before invoking its protocol owner.
            let replacement = reserve(&reentrant, 1);
            replacement.complete(Ok(()));
            observed.set(observed.get() + 1);
        }))))
        .unwrap();
    drop(registration);
    assert_eq!(calls.get(), 1);
    registry.begin_close();
    assert!(matches!(poll(&mut registry.closed()), Poll::Ready(Err(_))));
}

#[test]
fn registration_does_not_keep_registry_or_protocol_state_alive() {
    let registry = Connections::<State>::new();
    let weak = Rc::downgrade(&registry.0);
    let registration = reserve(&registry, 1);
    let state = state();
    let weak_state = Rc::downgrade(&state);
    registration.publish(state).unwrap();
    drop(registry);
    assert!(weak.upgrade().is_none());
    assert!(weak_state.upgrade().is_none());
    drop(registration);
}

#[test]
fn empty_registry_closes_once_and_rejects_new_work() {
    let registry = Connections::<State>::new();
    registry.begin_close();
    assert!(matches!(poll(&mut registry.closed()), Poll::Ready(Ok(()))));
    assert!(registry.begin_close().is_empty());
    assert!(matches!(poll(&mut registry.closed()), Poll::Ready(Ok(()))));
    assert!(registry.acquire(1).is_err());
}

#[test]
fn unexpected_live_completion_is_not_successful_transport_shutdown() {
    let registry = Connections::<State>::new();
    let registration = reserve(&registry, 1);
    registration.publish(state()).unwrap();
    registration.complete(Ok(()));
    assert!(registry.states().is_empty());
    registry.begin_close();
    assert!(matches!(poll(&mut registry.closed()), Poll::Ready(Err(_))));
}

#[test]
fn stale_cancellation_does_not_disconnect_replacement() {
    let registry = Connections::<State>::new();
    let old = reserve(&registry, 1);
    let old_state = state();
    old.publish(old_state.clone()).unwrap();
    registry.detach(1, &old_state);
    let replacement = reserve(&registry, 1);
    let new_state = Rc::new(State(Box::new(|| panic!("replacement disconnected"))));
    replacement.publish(new_state.clone()).unwrap();
    drop(old);
    assert!(Rc::ptr_eq(&registry.states()[0], &new_state));
    registry.detach(1, &new_state);
    replacement.complete(Ok(()));
    registry.begin_close();
    assert!(matches!(poll(&mut registry.closed()), Poll::Ready(Err(_))));
}

#[test]
fn registration_identity_does_not_allocate_after_map_capacity_is_reserved() {
    let registry = Connections::<State>::new();
    registry.0.entries.borrow_mut().reserve(4);
    let mut acquired = None;
    let count = allocation_counter::measure(|| {
        acquired = Some(reserve(&registry, 7));
    });
    assert_eq!(count.count_total, 0);
    assert_eq!(count.bytes_total, 0);
    acquired.take().unwrap().complete(Ok(()));
}

#[test]
fn the_final_identity_is_valid_and_exhaustion_never_reuses_it() {
    let registry = Connections::<State>::new();
    registry.0.next_identity.set(NonZeroU64::new(u64::MAX));
    assert!(registry.acquire_bounded(6, 0).is_err());
    assert_eq!(registry.0.next_identity.get(), NonZeroU64::new(u64::MAX));
    let final_registration = reserve(&registry, 7);
    let live = state();
    final_registration.publish(live.clone()).unwrap();
    // Lookup of a published state never consumes another identity.
    assert!(
        matches!(registry.acquire(7), Ok(Acquired::Existing(found)) if Rc::ptr_eq(&found, &live))
    );
    final_registration.detach();
    assert!(
        matches!(registry.acquire(7), Err(error) if error.kind == capnp::ErrorKind::Overloaded)
    );
    assert!(registry.0.entries.borrow().is_empty());
    assert_eq!(registry.0.completion.remaining.get(), 1);
    final_registration.complete(Ok(()));
    assert_eq!(registry.0.completion.remaining.get(), 0);
    assert!(
        matches!(registry.acquire(8), Err(error) if error.kind == capnp::ErrorKind::Overloaded)
    );
}

#[test]
fn concurrent_and_reused_registration_tokens_do_not_allocate() {
    let registry = Connections::<State>::new();
    registry.0.entries.borrow_mut().reserve(32);
    let count = allocation_counter::measure(|| {
        for _ in 0..4 {
            let registrations: [Registration<State>; 32] =
                core::array::from_fn(|id| reserve(&registry, id));
            for registration in registrations {
                registration.complete(Ok(()));
            }
        }
    });
    assert_eq!(count.count_total, 0);
    assert_eq!(count.bytes_total, 0);
    assert!(registry.0.entries.borrow().is_empty());
    assert_eq!(registry.0.completion.remaining.get(), 0);
}

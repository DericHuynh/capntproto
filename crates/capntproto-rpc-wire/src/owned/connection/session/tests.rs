extern crate std;
use super::*;
use std::{
    boxed::Box,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Wake,
};

std::thread_local! {
    static CALLBACK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}
#[derive(Default)]
struct Signal(AtomicUsize);
impl Signal {
    fn count(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
        let callback = CALLBACK.with(|callback| callback.borrow_mut().take());
        if let Some(callback) = callback {
            callback();
        }
    }
}

#[test]
fn drive_registration_replacement_ready_and_drop() {
    let first = Arc::new(Signal::default());
    let second = Arc::new(Signal::default());
    let first_waker = Waker::from(first.clone());
    let second_waker = Waker::from(second.clone());
    let session = Session::new(0u32, (), (), 16, Default::default());
    let measured = allocation_counter::measure(|| {
        let mut drive = session.drive().unwrap();
        assert!(matches!(session.drive(), Err(CallError::Busy)));
        for wake in [&first_waker, &second_waker] {
            assert_eq!(
                drive
                    .poll_with(&mut Context::from_waker(wake), |_, _, _, _| {
                        Poll::<()>::Pending
                    })
                    .unwrap(),
                Poll::Pending
            );
        }
        assert_eq!(first.count(), 0, "polling an idle driver cannot spin");
        assert_eq!(second.count(), 0);
        session.access(|state, _, _| *state = 1).unwrap();
        assert_eq!(first.count(), 0, "a moved driver replaces its registration");
        assert_eq!(second.count(), 1);
        session.access(|state, _, _| *state = 2).unwrap();
        assert_eq!(
            second.count(),
            1,
            "notifications coalesce until another poll"
        );
        assert_eq!(
            drive
                .poll_with(&mut Context::from_waker(&first_waker), |state, _, _, _| {
                    Poll::Ready(*state)
                })
                .unwrap(),
            Poll::Ready(2)
        );
        session.access(|_, _, _| ()).unwrap();
        assert_eq!(first.count(), 0, "Ready unregisters the driver");
        assert!(drive
            .poll_with(&mut Context::from_waker(&first_waker), |_, _, _, _| {
                Poll::<()>::Pending
            })
            .unwrap()
            .is_pending());
        drop(drive);
        session.access(|_, _, _| ()).unwrap();
        assert_eq!(first.count(), 0, "dropping a driver unregisters it");
        drop(session.drive().unwrap());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(session.into_parts().0, 2);
}

#[test]
fn driver_wake_reenters_after_mutation_releases_its_borrow() {
    let signal = Arc::new(Signal::default());
    let wake = Waker::from(signal.clone());
    let session = Rc::new(Session::new(0u32, (), (), 16, Default::default()));
    let mut drive = session.drive().unwrap();
    assert!(drive
        .poll_with(&mut Context::from_waker(&wake), |_, _, _, _| {
            Poll::<()>::Pending
        })
        .unwrap()
        .is_pending());
    let reentrant = session.clone();
    CALLBACK.with(|callback| {
        *callback.borrow_mut() = Some(Box::new(move || {
            reentrant
                .access(|state, _, _| {
                    assert_eq!(*state, 7, "committed state is visible before waking");
                    *state = 11;
                })
                .expect("wake callback must run outside the session borrow");
        }))
    });
    session.access(|state, _, _| *state = 7).unwrap();
    assert_eq!(signal.count(), 1);
    drop(drive);
    assert_eq!(Rc::try_unwrap(session).ok().unwrap().into_parts().0, 11);
}

#[test]
fn busy_driver_preserves_operation_and_existing_wake() {
    let signal = Arc::new(Signal::default());
    let wake = Waker::from(signal.clone());
    let session = Session::new(0u32, (), (), 16, Default::default());
    let mut drive = session.drive().unwrap();
    assert!(drive
        .poll_with(&mut Context::from_waker(&wake), |_, _, _, _| {
            Poll::<()>::Pending
        })
        .unwrap()
        .is_pending());
    let owner = Box::new(13);
    let rejected = session
        .access(|_, _, _| {
            drive
                .poll_with(
                    &mut Context::from_waker(Waker::noop()),
                    move |_, _, _, _| Poll::Ready(owner),
                )
                .expect_err("recursive polling must reject")
        })
        .unwrap();
    assert_eq!(rejected.error, CallError::Busy);
    assert_eq!(
        signal.count(),
        1,
        "the outer mutation retains its wake obligation"
    );
    assert_eq!(
        drive
            .poll_with(&mut Context::from_waker(&wake), rejected.operation)
            .unwrap(),
        Poll::Ready(Box::new(13))
    );
}

#[test]
fn replacing_driver_waker_drops_it_outside_the_session_borrow() {
    struct OnDrop;
    // A real owned waker is required to exercise its user-defined destructor.
    #[allow(clippy::manual_noop_waker)]
    impl Wake for OnDrop {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for OnDrop {
        fn drop(&mut self) {
            let callback = CALLBACK.with(|callback| callback.borrow_mut().take());
            if let Some(callback) = callback {
                callback();
            }
        }
    }
    let session = Rc::new(Session::new(0u32, (), (), 16, Default::default()));
    let mut drive = session.drive().unwrap();
    let old = Waker::from(Arc::new(OnDrop));
    assert!(drive
        .poll_with(&mut Context::from_waker(&old), |_, _, _, _| {
            Poll::<()>::Pending
        })
        .unwrap()
        .is_pending());
    drop(old);
    let reentrant = session.clone();
    CALLBACK.with(|callback| {
        *callback.borrow_mut() = Some(Box::new(move || {
            reentrant
                .access(|state, _, _| *state = 17)
                .expect("waker destructor must run outside the session borrow");
        }))
    });
    assert!(drive
        .poll_with(&mut Context::from_waker(Waker::noop()), |_, _, _, _| {
            Poll::<()>::Pending
        })
        .unwrap()
        .is_pending());
    drop(drive);
    assert_eq!(Rc::try_unwrap(session).ok().unwrap().into_parts().0, 17);
}

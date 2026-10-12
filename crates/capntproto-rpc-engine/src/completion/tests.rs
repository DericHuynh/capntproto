use super::*;
#[test]
fn completion_cancellation_and_reuse_do_not_allocate() {
    let counted = allocation_counter::measure(|| {
        let mut slot = Slot::new();
        for cancel in [false, true] {
            let (sender, mut receiver) = slot.pair().unwrap();
            assert_eq!(receiver.try_take(), Ok(None));
            assert!(!sender.is_closed());
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(Pin::new(&mut receiver).poll(&mut cx), Poll::Pending);
            if cancel {
                drop(sender);
                assert_eq!(
                    Pin::new(&mut receiver).poll(&mut cx),
                    Poll::Ready(Err(Error::Canceled))
                );
            } else {
                assert_eq!(sender.send(41), Ok(()));
                assert_eq!(Pin::new(&mut receiver).poll(&mut cx), Poll::Ready(Ok(41)));
            }
            assert_eq!(receiver.try_take(), Err(Error::Consumed));
        }
        let (sender, receiver) = slot.pair().unwrap();
        drop(receiver);
        assert!(sender.is_closed());
        assert_eq!(sender.send(43), Err(43));
        let (sender, receiver) = slot.pair().unwrap();
        assert_eq!(sender.send(47), Ok(()));
        core::mem::forget(receiver);
        assert!(matches!(slot.pair(), Err(Error::Undelivered)));
        assert_eq!(slot.take_undelivered(), Some(47));
        let (sender, mut receiver) = slot.pair().unwrap();
        assert_eq!(sender.send(53), Ok(()));
        assert_eq!(receiver.try_take(), Ok(Some(53)));
    });
    assert_eq!(counted.count_total, 0);
}
#[test]
fn cancellation_and_undelivered_results_drop_the_owner_once() {
    use core::cell::Cell;
    struct Owner<'a>(&'a Cell<u8>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1)
        }
    }
    let drops = Cell::new(0);
    let mut slot = Slot::new();
    {
        let (sender, receiver) = slot.pair().unwrap();
        assert!(sender.send(Owner(&drops)).is_ok());
        assert_eq!(drops.get(), 0);
        drop(receiver);
    }
    assert_eq!(drops.get(), 0);
    assert!(matches!(slot.pair(), Err(Error::Undelivered)));
    drop(slot.take_undelivered().unwrap());
    assert_eq!(drops.get(), 1);
    {
        let (sender, receiver) = slot.pair().unwrap();
        drop(receiver);
        let rejected = sender.send(Owner(&drops)).unwrap_err();
        assert_eq!(drops.get(), 1);
        drop(rejected);
    }
    assert_eq!(drops.get(), 2);
    {
        let (sender, receiver) = slot.pair().unwrap();
        assert!(sender.send(Owner(&drops)).is_ok());
        core::mem::forget(receiver);
    }
    {
        assert!(matches!(slot.pair(), Err(Error::Undelivered)));
        assert_eq!(drops.get(), 2);
        drop(slot.take_undelivered().unwrap());
        let (sender, receiver) = slot.pair().unwrap();
        assert_eq!(drops.get(), 3);
        drop(sender);
        drop(receiver);
    }
    assert_eq!(drops.get(), 3);
}
#[test]
fn wake_and_waker_drop_can_reenter_after_the_transition() {
    extern crate std;
    use std::{sync::Arc, task::Wake};
    std::thread_local! { static SLOT:Slot<u32>=const {Slot::new()}; static WAKES:core::cell::Cell<u32>=const {core::cell::Cell::new(0)}; }
    struct Probe;
    impl Drop for Probe {
        fn drop(&mut self) {
            SLOT.with(|slot| {
                assert!(
                    slot.state.try_borrow_mut().is_ok(),
                    "waker destructor ran inside state borrow"
                );
            })
        }
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            SLOT.with(|slot| {
                let state = slot
                    .state
                    .try_borrow()
                    .expect("wake ran inside state borrow");
                assert!(
                    matches!(state.phase, Phase::Value(59)),
                    "publish before wake"
                );
            })
        }
    }
    SLOT.with(|slot| {
        for cancel in [false, true] {
            // Private test setup permits a safe 'static Wake implementation to
            // inspect this thread's slot. Production constructs pairs exclusively.
            slot.endpoints.set(2);
            *slot.state.borrow_mut() = State {
                phase: Phase::Pending,
                waker: None,
                observed_waker: None,
            };
            let sender = Sender {
                slot: Some(SlotRef::Borrowed(slot)),
            };
            let mut receiver = Receiver {
                slot: SlotRef::Borrowed(slot),
            };
            let waker = Waker::from(Arc::new(Probe));
            assert_eq!(
                Pin::new(&mut receiver).poll(&mut Context::from_waker(&waker)),
                Poll::Pending
            );
            drop(waker); // the slot now holds the last waker reference
            if cancel {
                drop(receiver);
                assert_eq!(sender.send(61), Err(61));
            } else {
                assert_eq!(sender.send(59), Ok(()));
                assert_eq!(receiver.try_take(), Ok(Some(59)));
            }
        }
    });
}

#[test]
fn tracked_delivery_reclaims_only_unobserved_values_without_allocation() {
    let measured = allocation_counter::measure(|| {
        let mut slot = Slot::new();
        for mode in 0..5 {
            let (sender, mut receiver) = slot.pair().unwrap();
            let mut delivery = sender.track();
            let mut cx = Context::from_waker(Waker::noop());
            assert!(delivery.poll_observed(&mut cx).is_pending());
            assert_eq!(delivery.take_abandoned(), None);
            if mode == 0 {
                drop(receiver);
                assert!(delivery.poll_observed(&mut cx).is_ready());
                assert_eq!(delivery.try_send(61), Err(61));
                assert_eq!(delivery.cancel(), None);
                continue;
            }
            assert_eq!(delivery.try_send(61), Ok(()));
            assert_eq!(delivery.try_send(67), Err(67));
            assert_eq!(
                delivery.take_abandoned(),
                None,
                "live receiver retains its reply"
            );
            assert!(delivery.poll_observed(&mut cx).is_pending());
            match mode {
                1 => {
                    assert_eq!(receiver.try_take(), Ok(Some(61)));
                    assert!(delivery.poll_observed(&mut cx).is_ready());
                    assert_eq!(delivery.cancel(), None, "taken response cannot be revoked");
                    assert_eq!(receiver.try_take(), Err(Error::Consumed));
                }
                2 => {
                    drop(receiver);
                    assert!(delivery.poll_observed(&mut cx).is_ready());
                    assert_eq!(delivery.take_abandoned(), Some(61));
                    assert_eq!(delivery.take_abandoned(), None);
                    assert_eq!(delivery.cancel(), None);
                }
                3 => {
                    assert_eq!(delivery.cancel(), Some(61));
                    assert_eq!(receiver.try_take(), Err(Error::Canceled));
                    assert_eq!(delivery.cancel(), None);
                }
                _ => {
                    core::mem::forget(receiver);
                    assert_eq!(delivery.cancel(), Some(61));
                    assert_eq!(delivery.cancel(), None);
                }
            }
        }
        let (sender, mut receiver) = slot.pair().unwrap();
        let mut delivery = sender.track();
        assert_eq!(delivery.cancel(), None);
        assert_eq!(delivery.try_send(71), Err(71));
        assert_eq!(receiver.try_take(), Err(Error::Canceled));
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn tracked_receiver_observation_wakes_the_producer_outside_borrow() {
    extern crate std;
    use std::{sync::Arc, task::Wake};
    std::thread_local! { static SLOT:Slot<u32>=const {Slot::new()}; static WAKES:core::cell::Cell<u32>=const {core::cell::Cell::new(0)}; }
    struct Probe;
    impl Drop for Probe {
        fn drop(&mut self) {
            SLOT.with(|slot| assert!(slot.state.try_borrow_mut().is_ok()));
        }
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            SLOT.with(|slot| {
                let state = slot
                    .state
                    .try_borrow()
                    .expect("observation wake held state borrow");
                assert!(observed(&state.phase), "observation must precede wake");
                WAKES.with(|count| count.set(count.get() + 1));
            });
        }
    }
    SLOT.with(|slot| {
        for mode in 0..4 {
            WAKES.with(|count| count.set(0));
            slot.endpoints.set(2);
            *slot.state.borrow_mut() = State {
                phase: Phase::Pending,
                waker: None,
                observed_waker: None,
            };
            let mut delivery = Delivery {
                slot: SlotRef::Borrowed(slot),
            };
            let mut receiver = Receiver {
                slot: SlotRef::Borrowed(slot),
            };
            let waker = Waker::from(Arc::new(Probe));
            assert!(delivery
                .poll_observed(&mut Context::from_waker(&waker))
                .is_pending());
            drop(waker);
            if mode == 0 {
                drop(receiver);
            } else {
                assert_eq!(delivery.try_send(73), Ok(()));
                match mode {
                    1 => {
                        assert_eq!(receiver.try_take(), Ok(Some(73)));
                    }
                    2 => {
                        assert_eq!(
                            Pin::new(&mut receiver).poll(&mut Context::from_waker(Waker::noop())),
                            Poll::Ready(Ok(73))
                        );
                    }
                    _ => {
                        drop(receiver);
                        assert_eq!(delivery.take_abandoned(), Some(73));
                    }
                }
            }
            assert!(delivery
                .poll_observed(&mut Context::from_waker(Waker::noop()))
                .is_ready());
            WAKES.with(|count| {
                assert_eq!(count.get(), 1, "receiver observation must wake producer")
            });
        }
    });
}

#[test]
fn shared_reply_slots_wait_for_both_endpoints_without_allocation() {
    let measured = allocation_counter::measure(|| {
        let slots = [Slot::new(), Slot::new()];
        for _ in 0..3 {
            let (sender, mut receiver) = slots[0].try_pair().unwrap();
            let (other, other_receiver) = slots[1].try_pair().unwrap();
            assert!(matches!(slots[0].try_pair(), Err(Error::Busy)));
            sender.send(97).unwrap();
            assert_eq!(receiver.try_take(), Ok(Some(97)));
            assert!(
                matches!(slots[0].try_pair(), Err(Error::Busy)),
                "an accessible consumed receiver still prevents reuse"
            );
            drop(receiver);
            let (sender, receiver) = slots[0].try_pair().unwrap();
            let mut delivery = sender.track();
            delivery.try_send(101).unwrap();
            drop(receiver);
            assert!(matches!(slots[0].try_pair(), Err(Error::Busy)));
            assert_eq!(delivery.take_abandoned(), Some(101));
            assert!(
                matches!(slots[0].try_pair(), Err(Error::Busy)),
                "cleanup observer still prevents reuse"
            );
            drop(delivery);
            drop(other_receiver);
            assert_eq!(other.send(103), Err(103));
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn endpoint_release_survives_a_panicking_receiver_wake() {
    extern crate std;
    use std::{
        panic::{catch_unwind, AssertUnwindSafe},
        sync::Arc,
        task::Wake,
    };
    struct PanicWake;
    impl Wake for PanicWake {
        fn wake(self: Arc<Self>) {
            panic!("injected receiver wake failure");
        }
    }
    let slot = Slot::new();
    for cancel in [false, true] {
        let (sender, mut receiver) = slot.try_pair().unwrap();
        let waker = Waker::from(Arc::new(PanicWake));
        assert!(Pin::new(&mut receiver)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        let failed = catch_unwind(AssertUnwindSafe(|| {
            if cancel {
                drop(sender);
            } else {
                sender.send(107).unwrap();
            }
        }));
        assert!(failed.is_err());
        assert_eq!(
            receiver.try_take(),
            if cancel {
                Err(Error::Canceled)
            } else {
                Ok(Some(107))
            }
        );
        assert!(matches!(slot.try_pair(), Err(Error::Busy)));
        drop(receiver);
        let (sender, receiver) = slot.try_pair().unwrap();
        drop(sender);
        drop(receiver);
    }
}

use super::*;
#[kani::proof]
#[kani::unwind(8)]
fn scoped_reply_transfers_once_and_preserves_rejected_owner() {
    let value: u8 = kani::any();
    let close: bool = kani::any();
    let cancel: bool = kani::any();
    let abandon: bool = kani::any();
    let mut slot = Slot::new();
    {
        let (sender, mut receiver) = slot.pair().unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(Pin::new(&mut receiver).poll(&mut cx), Poll::Pending);
        // Exercise the already-registered fast path before every terminal
        // transition: it cannot consume or abandon the future reply owner.
        assert_eq!(Pin::new(&mut receiver).poll(&mut cx), Poll::Pending);
        assert_eq!(receiver.try_take(), Ok(None));
        if close {
            drop(receiver);
            let rejected = sender.send(value);
            kani::assert(
                rejected == Err(value),
                "closed receiver preserves rejected reply owner",
            );
        } else {
            if cancel {
                drop(sender);
                kani::assert(
                    Pin::new(&mut receiver).poll(&mut cx) == Poll::Ready(Err(Error::Canceled)),
                    "dropped producer reports cancellation",
                );
            } else if abandon {
                assert_eq!(sender.send(value), Ok(()));
                drop(receiver);
                kani::assert(
                    matches!(slot.pair(), Err(Error::Undelivered)),
                    "undelivered reply prevents premature slot reuse",
                );
                kani::assert(
                    slot.take_undelivered() == Some(value),
                    "canceled receiver preserves accepted reply owner",
                );
                kani::assert(
                    slot.take_undelivered().is_none(),
                    "undelivered reply is recovered once",
                );
                let (sender, mut receiver) = slot.pair().unwrap();
                assert_eq!(sender.send(!value), Ok(()));
                assert_eq!(receiver.try_take(), Ok(Some(!value)));
                return;
            } else {
                assert_eq!(sender.send(value), Ok(()));
                kani::assert(
                    Pin::new(&mut receiver).poll(&mut cx) == Poll::Ready(Ok(value)),
                    "accepted reply transfers its exact value",
                );
            }
            kani::assert(
                receiver.try_take() == Err(Error::Consumed),
                "reply completion is consumed once",
            );
        }
    }
    let (sender, mut receiver) = slot.pair().unwrap();
    assert_eq!(receiver.try_take(), Ok(None));
    assert_eq!(sender.send(!value), Ok(()));
    kani::assert(
        receiver.try_take() == Ok(Some(!value)),
        "new reply cannot observe recycled value",
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn tracked_reply_reclaims_only_unobserved_owner() {
    let value: u8 = kani::any();
    let close: bool = kani::any();
    let cancel: bool = kani::any();
    let consumed: bool = kani::any();
    let mut slot = Slot::new();
    {
        let (sender, mut receiver) = slot.pair().unwrap();
        let mut delivery = sender.track();
        if close {
            drop(receiver);
            kani::assert(
                delivery.is_observed(),
                "closed receiver notifies tracked producer",
            );
            kani::assert(
                delivery.try_send(value) == Err(value),
                "tracked closed receiver preserves rejected owner",
            );
            assert_eq!(delivery.cancel(), None);
        } else {
            assert_eq!(delivery.try_send(value), Ok(()));
            kani::assert(
                delivery.take_abandoned().is_none(),
                "cleanup cannot steal live reply",
            );
            kani::assert(
                delivery.try_send(!value) == Err(!value),
                "tracked reply rejects duplicate delivery",
            );
            if consumed {
                assert_eq!(receiver.try_take(), Ok(Some(value)));
                kani::assert(
                    delivery.cancel().is_none(),
                    "consumed reply cannot be reclaimed again",
                );
                assert_eq!(receiver.try_take(), Err(Error::Consumed));
            } else if cancel {
                kani::assert(
                    delivery.cancel() == Some(value),
                    "shutdown recovers unobserved reply owner",
                );
                assert_eq!(receiver.try_take(), Err(Error::Canceled));
            } else {
                drop(receiver);
                kani::assert(
                    delivery.take_abandoned() == Some(value),
                    "tracked abandoned reply preserves exact owner",
                );
                assert_eq!(delivery.take_abandoned(), None);
            }
            kani::assert(delivery.is_observed(), "reply terminal state is observable");
            kani::assert(
                delivery.cancel().is_none(),
                "tracked reply owner reclaimed once",
            );
        }
    }
    let (sender, mut receiver) = slot.pair().unwrap();
    assert_eq!(sender.send(!value), Ok(()));
    assert_eq!(receiver.try_take(), Ok(Some(!value)));
}

#[kani::proof]
#[kani::unwind(8)]
fn shared_reply_claim_preserves_accessible_endpoint_identity() {
    let slot = Slot::new();
    let value: u8 = kani::any();
    let abandon: bool = kani::any();
    let (sender, mut receiver) = slot.try_pair().unwrap();
    kani::assert(
        matches!(slot.try_pair(), Err(Error::Busy)),
        "shared reply refuses live endpoint reuse",
    );
    let mut delivery = sender.track();
    assert_eq!(delivery.try_send(value), Ok(()));
    if abandon {
        drop(receiver);
        kani::assert(
            matches!(slot.try_pair(), Err(Error::Busy)),
            "tracked owner blocks reuse after receiver closure",
        );
        assert_eq!(delivery.take_abandoned(), Some(value));
    } else {
        assert_eq!(receiver.try_take(), Ok(Some(value)));
        kani::assert(
            matches!(slot.try_pair(), Err(Error::Busy)),
            "consumed but accessible receiver blocks reuse",
        );
        drop(receiver);
    }
    kani::assert(
        matches!(slot.try_pair(), Err(Error::Busy)),
        "producer observer blocks reuse after recovery",
    );
    drop(delivery);
    let (sender, mut receiver) = slot.try_pair().unwrap();
    assert_eq!(sender.send(!value), Ok(()));
    kani::assert(
        receiver.try_take() == Ok(Some(!value)),
        "recycled shared reply observes only its new value",
    );
}

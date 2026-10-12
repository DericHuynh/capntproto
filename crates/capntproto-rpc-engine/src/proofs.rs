//! Direct production-code proofs. Loop bounds are checked, never assumed away.
use crate::{
    flow_control::{Error, Window},
    ids::{AnswerId, WireId},
    peer_table::{BorrowedSparse, InsertErrorKind, PeerTable, Slot},
    question::{Delivery, Question},
};
use core::time::Duration;

#[kani::proof]
#[kani::unwind(9)]
fn question_finish_once_and_delayed_reuse() {
    let no_return: bool = kani::any();
    let mut question = if no_return {
        Question::no_return()
    } else {
        Question::new()
    };
    let mut pending = !no_return;
    let mut released = false;
    let mut finish_needed = true;
    let mut finishes = 0;
    for _ in 0..8 {
        if kani::any() {
            let no_finish: bool = kani::any();
            let result = question.returned(no_finish);
            if pending {
                assert_eq!(
                    result,
                    Ok(if released {
                        Delivery::Discard
                    } else {
                        Delivery::Deliver
                    })
                );
                pending = false;
                finish_needed = !no_finish;
            } else {
                assert!(result.is_err());
            }
        } else {
            let result = question.release();
            assert_eq!(result.reclaim, !pending);
            assert_eq!(result.finish.is_some(), !released && finish_needed);
            if let Some(finish) = result.finish {
                assert_eq!(finish.release_result_caps, pending);
                finishes += 1;
            }
            released = true;
        }
        assert!(finishes <= 1);
        assert_eq!(question.is_awaiting_return(), pending);
    }
}

#[kani::proof]
fn fixed_credit_conservation_and_overflow() {
    let mut window = Window::fixed(kani::any());
    let a: u128 = kani::any();
    let b: u128 = kani::any();
    let first = window.sent(a, Duration::ZERO).unwrap();
    assert_eq!(window.in_flight(), a);
    assert!(window.is_ready());
    assert!(!window.needs_window());
    match window.sent(b, Duration::ZERO) {
        Err(error) => {
            assert_eq!(error, Error::CounterOverflow);
            assert!(b > u128::MAX - a);
            assert_eq!(window.in_flight(), a);
            window.ack(first, Duration::ZERO).unwrap();
        }
        Ok(second) => {
            assert!(b <= u128::MAX - a);
            assert_eq!(window.in_flight(), a + b);
            window.set_size(kani::any());
            assert_eq!(window.needs_window(), a != 0 && b != 0);
            let smaller = a.min(b);
            assert_eq!(
                window.is_ready(),
                smaller == 0 || smaller < window.size() as u128
            );
            if kani::any() {
                window.ack(first, Duration::ZERO).unwrap();
                assert_eq!(window.in_flight(), b);
                window.ack(second, Duration::ZERO).unwrap();
            } else {
                window.ack(second, Duration::ZERO).unwrap();
                assert_eq!(window.in_flight(), a);
                window.ack(first, Duration::ZERO).unwrap();
            }
        }
    }
    assert_eq!(window.in_flight(), 0);
    assert!(window.is_ready());
}

#[kani::proof]
fn adaptive_clock_rejection_preserves_credit() {
    let size: usize = kani::any();
    let bytes: u128 = kani::any();
    let t: u64 = kani::any();
    kani::assume(t > 0);
    let mut window = Window::adaptive(size);
    let receipt = window.sent(bytes, Duration::from_secs(t)).unwrap();
    assert_eq!(
        window
            .sent(kani::any(), Duration::from_secs(t - 1))
            .unwrap_err(),
        Error::ClockWentBackwards
    );
    assert_eq!(window.in_flight(), bytes);
    assert_eq!(window.size(), size);
    assert_eq!(
        window.ack(receipt, Duration::from_secs(t - 1)),
        Err(Error::ClockWentBackwards)
    );
    assert_eq!(window.in_flight(), bytes);
    assert_eq!(window.size(), size);
}

#[kani::proof]
#[kani::unwind(4)]
fn peer_full_wire_range_and_rejected_ownership() {
    let mut slots = [const { Slot::EMPTY }; 1];
    let mut table =
        PeerTable::<AnswerId, u8, _, 2>::try_new(BorrowedSparse::new(&mut slots)).unwrap();
    let a = AnswerId::from_wire(kani::any());
    let b = AnswerId::from_wire(kani::any());
    let owner: u8 = kani::any();
    table.try_insert_unique(a, owner).unwrap();
    let result = table.try_insert_unique(b, 17);
    if a == b {
        let error = result.unwrap_err();
        assert_eq!(error.kind, InsertErrorKind::Duplicate);
        assert_eq!(error.value, 17);
    } else if a.to_wire() >= 2 && b.to_wire() >= 2 {
        let error = result.unwrap_err();
        assert_eq!(error.kind, InsertErrorKind::Full);
        assert_eq!(error.value, 17);
        assert_eq!(table.get(&b), None);
    } else {
        assert!(result.is_ok());
        assert_eq!(table.remove(&b), Some(17));
    }
    assert_eq!(table.len(), 1);
    assert_eq!(table.get(&a), Some(&owner));
    assert_eq!(table.try_insert(a, 18), Ok(Some(owner)));
    assert_eq!(table.remove(&a), Some(18));
    assert_eq!(table.remove(&a), None);
    assert!(table.is_empty());
}

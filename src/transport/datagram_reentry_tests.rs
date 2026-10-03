use super::*;
use crate::{
    realtime::{Config, MonotonicClock},
    realtime_datagram::{Router, Sender},
};
use std::{cell::RefCell, rc::Rc, sync::Arc, task::Context};

thread_local! {
    static ON_WAKE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}
struct Reenter;
impl futures::task::ArcWake for Reenter {
    fn wake_by_ref(_: &Arc<Self>) {
        let callback = ON_WAKE.with(|slot| slot.borrow_mut().take());
        if let Some(callback) = callback {
            callback();
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn offer_commits_sequence_before_a_queue_waker_can_reenter() {
    let (port, mut driver) = datagram_pair();
    let outbound = port.sender();
    let (router, _router_driver) = Router::new(port);
    let (_receiver, control) = router
        .bind(
            Config::new("reenter", 0, 1, 1, 2, 1100, 1).unwrap(),
            Rc::new(MonotonicClock::new(tokio::time::Instant::now(), 0)),
        )
        .unwrap();
    let sender = Sender::connect(control, outbound).await.unwrap();
    let clone = sender.clone();
    let nested = Rc::new(RefCell::new(None));
    let observed = nested.clone();
    ON_WAKE.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            *observed.borrow_mut() = Some(clone.offer(0, u64::MAX, b"nested").unwrap());
        }));
    });
    let waker = futures::task::waker(Arc::new(Reenter));
    assert!(driver
        .outgoing
        .poll_recv(&mut Context::from_waker(&waker))
        .is_pending());
    let first = sender.offer(0, u64::MAX, &[42; 1100]).unwrap();
    assert_eq!(first.sequence(), 1);
    assert_eq!(nested.borrow().as_ref().unwrap().sequence(), 2);
    assert_eq!(sender.next_sequence_for_test(), 3);
    let sequences: Vec<_> = (0..3)
        .map(|_| {
            let packet = driver.outgoing.try_recv().unwrap();
            u64::from_be_bytes(packet[36..44].try_into().unwrap())
        })
        .collect();
    assert_eq!(sequences, [1, 2, 1]);
    assert!(driver.outgoing.try_recv().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn reentrant_close_is_sticky_and_last_sequence_is_not_reissued() {
    for limit in [1, 2] {
        for close_queue in [false, true] {
            let (port, driver) = datagram_pair();
            let driver = Rc::new(RefCell::new(driver));
            let outbound = port.sender();
            let (router, _router_driver) = Router::new(port);
            let (_receiver, control) = router
                .bind(
                    Config::new("close-reentry", 0, 1, 1, limit, 1100, 1).unwrap(),
                    Rc::new(MonotonicClock::new(tokio::time::Instant::now(), 0)),
                )
                .unwrap();
            let sender = Sender::connect(control, outbound).await.unwrap();
            let clone = sender.clone();
            let closing = driver.clone();
            ON_WAKE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    assert_eq!(clone.next_sequence_for_test(), 2);
                    if close_queue {
                        closing.borrow_mut().outgoing.close();
                    } else {
                        drop(clone.close());
                    }
                }));
            });
            let waker = futures::task::waker(Arc::new(Reenter));
            assert!(driver
                .borrow_mut()
                .outgoing
                .poll_recv(&mut Context::from_waker(&waker))
                .is_pending());
            let receipt = sender.offer(0, u64::MAX, &[42; 1100]).unwrap();
            assert_eq!(receipt.sequence(), 1);
            assert_eq!(
                driver.borrow().outgoing.len(),
                2,
                "reservation survives a close during send"
            );
            assert!(sender.offer(0, u64::MAX, b"after close").is_err());
            assert_eq!(
                receipt.resend().unwrap_err().kind(),
                io::ErrorKind::BrokenPipe
            );
            assert_eq!(sender.next_sequence_for_test(), 2);
            assert!(ON_WAKE.with(|slot| slot.borrow().is_none()));
        }
    }
}

#[test]
fn reserved_batches_are_invisible_until_sent_and_drop_releases_capacity() {
    let (port, mut driver) = datagram_pair();
    let outbound = port.sender();
    let packets = vec![vec![1], vec![2]];
    let reservation = outbound.reserve_batch(&packets).unwrap();
    assert_eq!(outbound.0.capacity(), DATAGRAM_QUEUE - 2);
    assert!(driver.outgoing.try_recv().is_err());
    drop(reservation);
    assert_eq!(outbound.0.capacity(), DATAGRAM_QUEUE);
    assert!(driver.outgoing.try_recv().is_err());
    let reservation = outbound.reserve_batch(&packets).unwrap();
    driver.outgoing.close();
    reservation.send();
    assert_eq!(driver.outgoing.try_recv().unwrap(), packets[0]);
    assert_eq!(driver.outgoing.try_recv().unwrap(), packets[1]);
    assert!(driver.outgoing.try_recv().is_err());
    assert_eq!(outbound.0.capacity(), DATAGRAM_QUEUE);
    assert!(outbound.reserve_batch(&packets).is_err());
}

fn rpc_result<T>(result: &capnp::Result<T>) -> u64 {
    match result {
        Ok(_) => 1,
        Err(e) => match e.kind {
            capnp::ErrorKind::Overloaded => 2,
            capnp::ErrorKind::Failed => 3,
            capnp::ErrorKind::Disconnected => 4,
            _ => panic!("unexpected RPC error: {e:?}"),
        },
    }
}
fn retry_result(result: io::Result<()>) -> u64 {
    match result {
        Ok(()) => 1,
        Err(e) => match e.kind() {
            io::ErrorKind::WouldBlock => 2,
            io::ErrorKind::BrokenPipe => 4,
            _ => panic!("unexpected queue error: {e:?}"),
        },
    }
}
fn drain_code(driver: &mut DatagramDriver) -> u64 {
    let mut encoded = 0;
    let mut power = 1;
    while let Ok(packet) = driver.outgoing.try_recv() {
        let sequence = if packet == b"pressure" {
            3
        } else {
            assert!(matches!(&packet[..4], b"RDS1" | b"RDS2"));
            u64::from_be_bytes(packet[36..44].try_into().unwrap())
        };
        assert!((1..=3).contains(&sequence));
        encoded += sequence * power;
        power *= 4;
    }
    encoded
}
#[derive(Default)]
struct Observed {
    woke: u64,
    next: u64,
    capacity: u64,
    nested_result: u64,
    nested_sequence: u64,
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_datagram_reentry_traces() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/DatagramReentry.tla";
    const CONFIG: &str = include_str!("../../verification/DatagramReentry.cfg");
    let paths = exploration::traces(MODEL, "datagram-reentry", CONFIG).unwrap();
    for path in &paths {
        let initial = &path[0];
        assert_eq!(initial["event"], 1);
        let (port, driver) = datagram_pair();
        let driver = Rc::new(RefCell::new(driver));
        let outbound = port.sender();
        let _reserved = outbound.0.try_reserve_many(DATAGRAM_QUEUE - 3).unwrap();
        let (router, _router_driver) = Router::new(port);
        let (_receiver, control) = router
            .bind(
                Config::new("reentry-model", 0, 1, 1, initial["limit"], 1100, 1).unwrap(),
                Rc::new(MonotonicClock::new(tokio::time::Instant::now(), 0)),
            )
            .unwrap();
        let sender = Sender::connect(control, outbound.clone()).await.unwrap();
        let mut receipt = None;
        for sequence in 1..initial["next"] {
            receipt = Some(Rc::new(sender.offer(0, u64::MAX, &[42]).unwrap()));
            assert_eq!(drain_code(&mut driver.borrow_mut()), sequence);
        }
        for _ in 0..initial["queued"] {
            outbound.try_send(b"pressure").unwrap();
        }
        if initial["closed"] == 1 {
            drop(sender.close());
        }
        if initial["connected"] == 0 {
            driver.borrow_mut().outgoing.close();
        }
        let observed = Rc::new(RefCell::new(Observed::default()));
        let mut outer_sequence = 0;
        for state in path {
            let before_next = sender.next_sequence_for_test();
            let before_queued = driver.borrow().outgoing.len() as u64;
            let result = match state["event"] {
                1 => 0,
                2 => {
                    let callback = state["callback"];
                    if callback > 0 && before_queued == 0 && !outbound.0.is_closed() {
                        let clone = sender.clone();
                        let outbound = outbound.clone();
                        let previous = receipt.clone();
                        let seen = observed.clone();
                        ON_WAKE.with(|slot| {
                            *slot.borrow_mut() = Some(Box::new(move || {
                                let mut report = seen.borrow_mut();
                                report.woke = 1;
                                report.next = clone.next_sequence_for_test();
                                report.capacity = outbound.0.capacity() as u64;
                                match callback {
                                    1 => {
                                        let nested = clone.offer(0, u64::MAX, &[42; 1100]);
                                        report.nested_result = rpc_result(&nested);
                                        report.nested_sequence =
                                            nested.ok().map_or(0, |r| r.sequence());
                                    }
                                    2 => drop(clone.close()),
                                    3 => {
                                        if let Some(previous) = previous {
                                            report.nested_result = retry_result(previous.resend());
                                        }
                                    }
                                    _ => unreachable!(),
                                }
                            }));
                        });
                        let waker = futures::task::waker(Arc::new(Reenter));
                        assert!(driver
                            .borrow_mut()
                            .outgoing
                            .poll_recv(&mut Context::from_waker(&waker))
                            .is_pending());
                    }
                    let offered = sender.offer(
                        0,
                        u64::MAX,
                        &vec![42; if state["fragments"] == 1 { 1 } else { 1100 }],
                    );
                    // A failed offer never published, so its callback can still
                    // be armed. Remove it before cleanup or any subsequent call.
                    ON_WAKE.with(|slot| {
                        slot.borrow_mut().take();
                    });
                    let result = rpc_result(&offered);
                    if let Ok(r) = offered {
                        outer_sequence = r.sequence();
                        receipt = Some(Rc::new(r));
                    }
                    result
                }
                3..=5 => {
                    if state["event"] == 4 {
                        drop(sender.close());
                    }
                    if state["event"] == 5 {
                        assert_eq!(
                            drain_code(&mut driver.borrow_mut()),
                            state["beforeQueue"],
                            "{path:?}"
                        );
                    }
                    retry_result(receipt.as_ref().unwrap().resend())
                }
                6 => {
                    assert_eq!(
                        drain_code(&mut driver.borrow_mut()),
                        state["beforeQueue"],
                        "{path:?}"
                    );
                    let offered = sender.offer(0, u64::MAX, &[42]);
                    let result = rpc_result(&offered);
                    if let Ok(r) = offered {
                        receipt = Some(Rc::new(r));
                    }
                    result
                }
                _ => panic!("unexpected event: {state:?}"),
            };
            assert_eq!(result, state["result"], "{path:?}");
            let seen = observed.borrow();
            for (name, actual) in [
                ("next", sender.next_sequence_for_test()),
                ("closed", u64::from(sender.is_closed_for_test())),
                ("connected", u64::from(!outbound.0.is_closed())),
                ("queued", driver.borrow().outgoing.len() as u64),
                ("receipt", receipt.as_ref().map_or(0, |r| r.sequence())),
                ("outerSequence", outer_sequence),
                ("nestedSequence", seen.nested_sequence),
                ("nestedResult", seen.nested_result),
                ("woke", seen.woke),
                ("seenNext", seen.next),
                ("seenCapacity", seen.capacity),
            ] {
                assert_eq!(actual, state[name], "{name}: {path:?}");
            }
            if state["event"] > 1 {
                assert_eq!(before_next, state["beforeNext"], "{path:?}");
                assert_eq!(before_queued, state["beforeQueued"], "{path:?}");
            }
            assert_eq!(
                outbound.0.capacity() as u64 + state["queued"],
                3,
                "leaked reservation: {path:?}"
            );
        }
        // Each graph edge is the end of a replayed prefix. Drain only there (or
        // on an explicit drain step), checking the exact order of wire IDs.
        assert_eq!(
            drain_code(&mut driver.borrow_mut()),
            path.last().unwrap()["queue"],
            "{path:?}"
        );
    }
    exploration::controls(
        MODEL,
        "datagram-reentry",
        CONFIG,
        &[
            ("late", "CommittedBeforeWake"),
            ("rewind", "AllocationAccounting"),
            ("reserve", "ReservationBeforeWake"),
            ("partial", "FailureAtomic"),
            ("burn", "FailureSequence"),
            ("close", "ExpectedOperation"),
            ("retry", "RetrySequence"),
            ("payload", "RetryPayload"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} datagram reentry edge-prefix replays", paths.len());
}

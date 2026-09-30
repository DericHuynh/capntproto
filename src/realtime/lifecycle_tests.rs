use super::*;
use std::cell::Cell;

#[derive(Default)]
struct TestClock(Cell<u64>);
impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.get()
    }
}
fn config() -> Config {
    Config::new("record-test", 0, 1, 1, 2, 1, 2).unwrap()
}
fn code(outcome: Outcome) -> u64 {
    match outcome {
        Outcome::Applied => 2,
        Outcome::Expired => 3,
        Outcome::Superseded => 4,
        Outcome::Busy => 5,
        Outcome::Canceled => 6,
        Outcome::Closed => 7,
    }
}
struct Receipt {
    sequence: u64,
    future: Option<Promise<Outcome, Error>>,
    observed: Option<Outcome>,
}
impl Receipt {
    fn poll(&mut self) {
        if let Some(future) = self.future.as_mut() {
            if let Some(outcome) = future.now_or_never() {
                self.observed = Some(outcome.unwrap());
                self.future = None;
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_realtime_receipt_lifecycle() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RealtimeReceiptLifecycle.tla";
    const CONFIG: &str = include_str!("../../verification/RealtimeReceiptLifecycle.cfg");
    let paths = exploration::traces(MODEL, "realtime-receipt-lifecycle", CONFIG).unwrap();
    for path in &paths {
        let clock = Rc::new(TestClock::default());
        let (receiver, _client) = Receiver::new(config(), clock.clone());
        let mut receipts: [Option<Receipt>; 2] = [None, None];
        for state in path {
            let sequence = state["item"];
            let result = match state["event"] {
                1 => {
                    let slot = state["slot"] as usize - 1;
                    let mut future = receiver.offer(sequence, 0, 2, &[state["data"] as u8]);
                    let observed = (&mut future).now_or_never();
                    let result = match &observed {
                        None => 1,
                        Some(Ok(outcome)) => code(*outcome),
                        Some(Err(_)) => 0,
                    };
                    if result > 0 && slot < 2 {
                        assert!(receipts[slot].is_none());
                        receipts[slot] = Some(Receipt {
                            sequence,
                            future: if observed.is_none() {
                                Some(future)
                            } else {
                                None
                            },
                            observed: observed.map(Result::unwrap),
                        });
                    }
                    // Slot 3 is a transient observer: it exercises receipt quota
                    // and admission ordering, then drops its local wait only.
                    result
                }
                2 => receiver.apply(sequence).map_or(0, code),
                3 => code(receiver.cancel(sequence).unwrap()),
                4 => {
                    receiver.close();
                    8
                }
                5 => {
                    drop(receipts[state["slot"] as usize - 1].take());
                    8
                }
                6 => {
                    clock.0.set(clock.0.get() + 1);
                    8
                }
                7 => {
                    receiver.expire().unwrap();
                    8
                }
                _ => panic!("invalid event: {state:?}"),
            };
            assert_eq!(result, state["result"], "{path:?}");
            for receipt in receipts.iter_mut().flatten() {
                receipt.poll();
                let status = state[if receipt.sequence == 1 { "s1" } else { "s2" }];
                assert_eq!(
                    receipt.observed.map(code),
                    if status >= 2 { Some(status) } else { None },
                    "{path:?}"
                );
                assert_eq!(receipt.future.is_some(), status == 1, "{path:?}");
            }
            assert_eq!(receipts[0].as_ref().map_or(0, |r| r.sequence), state["a"]);
            assert_eq!(receipts[1].as_ref().map_or(0, |r| r.sequence), state["b"]);
            assert_eq!(receiver.waiter_count() as u64, state["w"], "{path:?}");
            assert_eq!(u64::from(receiver.is_closed()), state["closed"]);
            assert_eq!(clock.now(), state["now"]);
            assert_eq!(
                receiver.get(0).map_or(0, |v| v.sequence()),
                state["visible"]
            );
            if let Some(value) = receiver.get(0) {
                assert_eq!(value.key(), 0);
                assert_eq!(value.not_after(), 2);
                assert_eq!(
                    value.bytes(),
                    &[state[if value.sequence() == 1 { "m1" } else { "m2" }] as u8]
                );
            }
            let actual = receiver.0 .0.state.borrow();
            assert_eq!(actual.high[0], state["high"]);
            let mut waiters = 0;
            for (seq, status, metadata) in
                [(1, state["s1"], state["m1"]), (2, state["s2"], state["m2"])]
            {
                let record = actual.records.get(&seq);
                let observed = record.map_or(0, |r| r.outcome().map_or(1, code));
                assert_eq!(observed, status, "{path:?}");
                assert_eq!(
                    actual.pending.get(&0) == Some(&seq),
                    status == 1,
                    "{path:?}"
                );
                if let Some(record) = record {
                    assert_eq!(record.metadata().is_some(), metadata != 0, "{path:?}");
                    if let Some(meta) = record.metadata() {
                        assert_eq!(meta.key, 0);
                        assert_eq!(meta.deadline, 2);
                        assert_eq!(
                            meta.digest.as_slice(),
                            ring::digest::digest(&ring::digest::SHA256, &[metadata as u8]).as_ref()
                        );
                    }
                    if let Some(pending) = record.pending() {
                        assert_eq!(pending.snapshot.sequence(), seq);
                        assert_eq!(pending.snapshot.bytes(), &[metadata as u8]);
                        waiters += pending.waiters.len();
                    }
                }
            }
            assert_eq!(waiters, actual.waiters);
        }
    }
    exploration::controls(
        MODEL,
        "realtime-receipt-lifecycle",
        CONFIG,
        &[
            ("conflict", "ExpectedOffer"),
            ("quota", "ExpectedOffer"),
            ("revive", "ExpectedOffer"),
            ("leak", "WaiterConservation"),
            ("drop", "DropPreservesWork"),
            ("overwrite", "TerminalStable"),
            ("duplicate", "AppliedOnce"),
        ],
        None,
    )
    .unwrap();
    eprintln!(
        "{} realtime receipt lifecycle edge-prefix replays",
        paths.len()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn finishing_releases_queued_payloads_and_waiter_exhaustion_is_inert() {
    for terminal in [
        Outcome::Canceled,
        Outcome::Expired,
        Outcome::Superseded,
        Outcome::Closed,
        Outcome::Applied,
    ] {
        let clock = Rc::new(TestClock::default());
        let (receiver, _client) = Receiver::new(config(), clock.clone());
        let first = receiver.offer(1, 0, 2, b"a");
        let duplicate = receiver.offer(1, 0, 2, b"a");
        let weak = {
            let state = receiver.0 .0.state.borrow();
            Rc::downgrade(&state.records[&1].pending().unwrap().snapshot)
        };
        match terminal {
            Outcome::Canceled => {
                receiver.cancel(1).unwrap();
            }
            Outcome::Expired => {
                clock.0.set(2);
                receiver.expire().unwrap();
            }
            Outcome::Superseded => {
                // Admission must reject before superseding when receipts are full.
                assert!(receiver.offer(2, 0, 2, b"b").await.is_err());
                assert_eq!(receiver.pending(), [1]);
                drop(duplicate); // release only this waiter's quota
                let replacement = receiver.offer(2, 0, 2, b"b");
                drop(replacement);
                assert_eq!(first.await.unwrap(), terminal);
                assert!(weak.upgrade().is_none());
                continue;
            }
            Outcome::Closed => receiver.close(),
            Outcome::Applied => {
                receiver.apply(1).unwrap();
            }
            Outcome::Busy => unreachable!(),
        }
        assert_eq!(first.await.unwrap(), terminal);
        assert_eq!(duplicate.await.unwrap(), terminal);
        assert_eq!(receiver.waiter_count(), 0);
        assert!(receiver.0 .0.state.borrow().records[&1].pending().is_none());
        assert_eq!(weak.upgrade().is_some(), terminal == Outcome::Applied);
    }
    let clock = Rc::new(TestClock::default());
    let (receiver, _client) = Receiver::new(config(), clock);
    let first = receiver.offer(1, 0, 2, b"a");
    receiver.0 .0.state.borrow_mut().next_waiter = u64::MAX;
    assert!(receiver.offer(2, 0, 2, b"b").await.is_err());
    assert!(receiver.offer(1, 0, 2, b"a").await.is_err());
    assert_eq!(receiver.pending(), [1]);
    assert_eq!(receiver.waiter_count(), 1);
    receiver.cancel(1).unwrap();
    assert_eq!(first.await.unwrap(), Outcome::Canceled);
    assert_eq!(
        receiver.offer(1, 0, 2, b"a").await.unwrap(),
        Outcome::Canceled
    );
}

thread_local! {
    static INSPECT_ON_WAKE: RefCell<Option<Receiver>> = const { RefCell::new(None) };
}
struct InspectWake(std::sync::atomic::AtomicUsize);
impl futures::task::ArcWake for InspectWake {
    fn wake_by_ref(this: &std::sync::Arc<Self>) {
        this.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        INSPECT_ON_WAKE.with(|slot| {
            if let Some(receiver) = slot.borrow().as_ref() {
                assert!(
                    receiver.0 .0.state.try_borrow_mut().is_ok(),
                    "receipt woke user code under the receiver borrow"
                );
            }
        });
    }
}
#[tokio::test(flavor = "current_thread")]
async fn receipt_notifications_allow_reentry_after_record_transition() {
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        task::Context,
    };
    for action in 0..4 {
        let (receiver, _client) = Receiver::new(config(), Rc::new(TestClock::default()));
        let wake = Arc::new(InspectWake(AtomicUsize::new(0)));
        let waker = futures::task::waker(wake.clone());
        let mut context = Context::from_waker(&waker);
        let mut first = receiver.offer(1, 0, 2, b"a");
        let mut duplicate = receiver.offer(1, 0, 2, b"a");
        INSPECT_ON_WAKE.with(|slot| {
            *slot.borrow_mut() = Some(receiver.clone());
        });
        assert!(Pin::new(&mut first).poll(&mut context).is_pending());
        assert!(Pin::new(&mut duplicate).poll(&mut context).is_pending());
        if action == 3 {
            drop(first);
            drop(duplicate);
            assert_eq!(receiver.waiter_count(), 0);
            assert_eq!(receiver.pending(), [1]);
            receiver.apply(1).unwrap();
        } else {
            let expected = match action {
                0 => receiver.apply(1).unwrap(),
                1 => receiver.cancel(1).unwrap(),
                _ => {
                    receiver.close();
                    Outcome::Closed
                }
            };
            assert_eq!(wake.0.load(Ordering::Relaxed), 2);
            assert_eq!(receiver.waiter_count(), 0);
            assert_eq!(first.await.unwrap(), expected);
            assert_eq!(duplicate.await.unwrap(), expected);
            receiver.close();
            assert_eq!(wake.0.load(Ordering::Relaxed), 2);
        }
        INSPECT_ON_WAKE.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

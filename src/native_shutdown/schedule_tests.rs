use super::*;
use capntproto_test_support::schedules::{self, block_on, spawn_local, yield_now, Trace};
use std::cell::Cell;

#[derive(Default)]
struct WakeFlag(std::sync::atomic::AtomicBool);
impl std::task::Wake for WakeFlag {
    fn wake(self: std::sync::Arc<Self>) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn replay_tlc_shutdown_waiter_lifecycle() {
    use capntproto_test_support::verification::exploration;
    use std::{
        future::Future,
        pin::Pin,
        sync::{atomic::Ordering, Arc},
        task::{Context, Waker},
    };
    const MODEL: &str = "verification/ShutdownWaiters.tla";
    const CONFIG: &str = include_str!("../../verification/ShutdownWaiters.cfg");
    let paths = exploration::traces(MODEL, "shutdown-waiters", CONFIG).unwrap();
    for path in &paths {
        let old = Control::new();
        let fresh = Control::new();
        type Wait<'a> = Pin<Box<dyn Future<Output = io::Result<Receipt>> + 'a>>;
        let mut waits: [Option<Wait<'_>>; 3] = std::array::from_fn(|_| None);
        let wakes: [Arc<WakeFlag>; 3] = std::array::from_fn(|_| Arc::new(WakeFlag::default()));
        for step in path {
            let event = step["event"];
            match event {
                1 | 4 | 7 => {
                    let id = ((event - 1) / 3) as usize;
                    waits[id] = Some(Box::pin(if id == 2 { fresh.wait() } else { old.wait() }));
                    wakes[id].0.store(false, Ordering::Relaxed);
                }
                2 | 5 | 8 => {
                    let id = ((event - 2) / 3) as usize;
                    wakes[id].0.store(false, Ordering::Relaxed);
                    let waker = Waker::from(wakes[id].clone());
                    let result = waits[id]
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .poll(&mut Context::from_waker(&waker));
                    assert_eq!(
                        result.is_ready(),
                        step[["a", "b", "c"][id]] == 3,
                        "{path:?}"
                    );
                    if let std::task::Poll::Ready(result) = result {
                        assert_eq!(
                            code(result),
                            step[["seenA", "seenB", "seenC"][id]],
                            "{path:?}"
                        );
                    }
                }
                3 => {
                    waits[0] = None;
                    wakes[0].0.store(false, Ordering::Relaxed);
                }
                10 => old.finish(Ok(Receipt { bytes: 37 })),
                11 => drop(DriverGuard(old.clone())),
                12 => old.finish(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "test deadline",
                ))),
                13 => fresh.finish(Ok(Receipt { bytes: 73 })),
                _ => panic!("unknown waiter event"),
            }
            for (id, name) in ["wa", "wb", "wc"].iter().enumerate() {
                assert_eq!(
                    u64::from(wakes[id].0.load(Ordering::Relaxed)),
                    step[*name],
                    "{path:?}"
                );
            }
            for (control, name) in [(&old, "old"), (&fresh, "fresh")] {
                let actual = control
                    .0
                    .state
                    .borrow()
                    .outcome
                    .clone()
                    .map(|outcome| {
                        code(
                            outcome.map_err(|Failure(kind, message)| io::Error::new(kind, message)),
                        )
                    })
                    .unwrap_or(0);
                assert_eq!(actual, step[name], "{path:?}");
            }
        }
    }
    let live =
        CONFIG.replace("SPECIFICATION Spec", "SPECIFICATION FairSpec") + "\nPROPERTY Progress\n";
    exploration::controls(
        MODEL,
        "shutdown-waiters",
        CONFIG,
        &[
            ("overwrite", "FirstOutcome"),
            ("wakeOne", "Broadcast"),
            ("cancelSibling", "CancellationIsolation"),
            ("crossGeneration", "FirstOutcome"),
        ],
        Some(&live),
    )
    .unwrap();
    eprintln!("{} native shutdown-waiter edge-prefix replays", paths.len());
}

fn code(outcome: io::Result<Receipt>) -> u64 {
    match outcome {
        Ok(receipt) => match receipt.bytes {
            37 => 1,
            73 => 4,
            bytes => panic!("unexpected receipt: {bytes}"),
        },
        Err(error) => match error.kind() {
            io::ErrorKind::ConnectionAborted => 2,
            io::ErrorKind::TimedOut => {
                assert_eq!(error.to_string(), "test deadline");
                3
            }
            kind => panic!("unexpected shutdown error: {kind:?}"),
        },
    }
}

fn cancel_and_replace(trace: Trace) {
    block_on(async {
        let old = Control::new();
        let new = Control::new();
        let winner = Rc::new(Cell::new(0));
        let (cancel, registration) = futures::future::AbortHandle::new_pair();
        let mut registration = Some(registration);
        let mut readers = Vec::new();
        for id in 0..3 {
            let old = old.clone();
            let winner = winner.clone();
            let trace = trace.clone();
            let registration = registration.take();
            readers.push(spawn_local(async move {
                let mut wait = Box::pin(old.wait());
                let observed = futures::future::poll_fn(|cx| {
                    let result = std::future::Future::poll(wait.as_mut(), cx);
                    trace.event(&format!("poll-{id}"), u64::from(result.is_ready()));
                    result
                });
                if let Some(registration) = registration {
                    match futures::future::Abortable::new(observed, registration).await {
                        Ok(value) => {
                            assert_eq!(code(value), winner.get());
                            trace.event("cancel-result", 1);
                        }
                        Err(_) => trace.event("cancel-result", 0),
                    }
                    drop(wait); // Release Notify registration before replacing it.
                    assert_eq!(code(old.wait().await), winner.get());
                    trace.event("replacement", winner.get());
                } else {
                    assert_eq!(code(observed.await), winner.get());
                    trace.event("survivor", id);
                }
            }));
        }
        let new_reader = {
            let new = new.clone();
            let trace = trace.clone();
            spawn_local(async move {
                assert_eq!(code(new.wait().await), 4);
                trace.event("new-observed", 4);
            })
        };
        let canceler = {
            let trace = trace.clone();
            spawn_local(async move {
                yield_now().await;
                trace.event("cancel", 0);
                cancel.abort();
            })
        };
        let complete = {
            let old = old.clone();
            let winner = winner.clone();
            let trace = trace.clone();
            spawn_local(async move {
                yield_now().await;
                if winner.get() == 0 {
                    winner.set(1);
                }
                trace.event("old-ack", 1);
                old.finish(Ok(Receipt { bytes: 37 }));
            })
        };
        let drop_driver = {
            let guard = DriverGuard(old.clone());
            let winner = winner.clone();
            let trace = trace.clone();
            spawn_local(async move {
                yield_now().await;
                if winner.get() == 0 {
                    winner.set(2);
                }
                trace.event("old-drop", 2);
                drop(guard);
            })
        };
        let new_complete = {
            let new = new.clone();
            let trace = trace.clone();
            spawn_local(async move {
                yield_now().await;
                trace.event("new-ack", 4);
                new.finish(Ok(Receipt { bytes: 73 }));
            })
        };
        canceler.await.unwrap();
        complete.await.unwrap();
        drop_driver.await.unwrap();
        new_complete.await.unwrap();
        for reader in readers {
            reader.await.unwrap();
        }
        new_reader.await.unwrap();
        // Old observers/late callbacks cannot revive or complete a replacement generation.
        drop(DriverGuard(old.clone()));
        old.finish(Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "test deadline",
        )));
        assert_eq!(code(old.wait().await), winner.get());
        assert_eq!(code(new.wait().await), 4);
    });
}

#[test]
fn shuttle_shutdown_cancellation_and_generation_isolation() {
    let batch = schedules::check("shutdown-cancellation", cancel_and_replace);
    if !batch.sampled {
        return;
    }
    let runs = batch.runs;
    let outcomes: std::collections::BTreeSet<_> = runs
        .iter()
        .flat_map(|run| &run.events)
        .filter_map(|(event, value)| (event == "cancel-result").then_some(*value))
        .collect();
    assert_eq!(outcomes, [0, 1].into_iter().collect());
    let pending_cancels = runs.iter().any(|run| {
        let cancel = run
            .events
            .iter()
            .position(|(event, _)| event == "cancel")
            .unwrap();
        run.events[..cancel].contains(&("poll-0".into(), 0))
            && run.events.contains(&("cancel-result".into(), 0))
    });
    assert!(
        pending_cancels,
        "cancellation never exercised a registered waiter"
    );
    assert!(
        runs.iter().any(|run| {
            let cancel = run
                .events
                .iter()
                .position(|(event, _)| event == "cancel")
                .unwrap();
            !run.events[..cancel]
                .iter()
                .any(|(event, _)| event == "poll-0")
                && run.events.contains(&("cancel-result".into(), 0))
        }),
        "cancellation never preceded the first waiter poll"
    );
}

fn competing_completion(trace: Trace) {
    block_on(async {
        let control = Control::new();
        let expected = Rc::new(Cell::new(0));
        let guard = DriverGuard(control.clone());
        let mut writers = Vec::new();
        for source in 1..=3 {
            let control = control.clone();
            let expected = expected.clone();
            let trace = trace.clone();
            // Construct the driver guard before spawning: cancellation before
            // its first poll must still complete the generation's waiters.
            let mut guard = (source == 2).then(|| DriverGuard(control.clone()));
            writers.push(spawn_local(async move {
                yield_now().await;
                if expected.get() == 0 {
                    expected.set(source);
                }
                trace.event("finish", source);
                match source {
                    1 => control.finish(Ok(Receipt { bytes: 37 })),
                    2 => drop(guard.take()),
                    3 => control.finish(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "test deadline",
                    ))),
                    _ => unreachable!(),
                }
            }));
        }
        let mut readers = Vec::new();
        for id in 0..3 {
            let control = control.clone();
            let expected = expected.clone();
            let trace = trace.clone();
            readers.push(spawn_local(async move {
                trace.event("wait", id);
                let actual = code(control.wait().await);
                assert_eq!(actual, expected.get());
                trace.event("observed", actual);
                actual
            }));
        }
        for writer in writers {
            writer.await.unwrap();
        }
        for reader in readers {
            assert_eq!(reader.await.unwrap(), expected.get());
        }
        drop(guard);
        assert_eq!(code(control.wait().await), expected.get());
        trace.event("late", expected.get());
    });
}

#[test]
fn shuttle_shutdown_first_completion_wakes_all_waiters() {
    let batch = schedules::check("shutdown-completion", competing_completion);
    if !batch.sampled {
        return;
    }
    let runs = batch.runs;
    let winners: std::collections::BTreeSet<_> = runs
        .iter()
        .flat_map(|run| run.events.iter())
        .filter_map(|(event, value)| (event == "late").then_some(*value))
        .collect();
    assert_eq!(winners, [1, 2, 3].into_iter().collect());
}

#[test]
#[ignore = "intentional lost-waker negative control; run through tests/schedules.rs"]
fn shuttle_detects_lost_wakeup() {
    schedules::check("lost-wakeup-control", |trace| {
        block_on(async {
            let control = Control::new();
            let (registered, registration) = futures::channel::oneshot::channel();
            let waiter = {
                let control = control.clone();
                let trace = trace.clone();
                spawn_local(async move {
                    let mut future = Box::pin(control.wait());
                    let mut registered = Some(registered);
                    let outcome = futures::future::poll_fn(|_| {
                        // Deliberately discard the executor's waker. Control still
                        // uses the real Notify and records the real terminal result.
                        let result = std::future::Future::poll(
                            future.as_mut(),
                            &mut std::task::Context::from_waker(std::task::Waker::noop()),
                        );
                        if result.is_pending() {
                            trace.event("registered-with-lost-waker", 1);
                            if let Some(registered) = registered.take() {
                                registered.send(()).unwrap();
                            }
                        }
                        result
                    })
                    .await;
                    assert_eq!(code(outcome), 1);
                })
            };
            registration.await.unwrap();
            control.finish(Ok(Receipt { bytes: 37 }));
            trace.event("finished-without-wakeup", 1);
            waiter.await.unwrap();
        });
    });
}

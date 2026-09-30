use super::test_executor::{Executor, Manual};
use super::*;
use reproto_test_support::schedules::{self, block_on, spawn_local, yield_now, Trace};
use std::{cell::Cell, future::Future, pin::Pin};

struct Dropped(Rc<Cell<u64>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct Released {
    count: Rc<Cell<u64>>,
    notify: Option<futures::channel::oneshot::Sender<()>>,
}
impl Drop for Released {
    fn drop(&mut self) {
        self.count.set(self.count.get() + 1);
        let _ = self.notify.take().unwrap().send(());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn tokio_owner_cancellation_releases_all_workers_with_observers_retained() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for registered in [false, true] {
                for explicit_stop in [false, true] {
                    let releases = Rc::new(Cell::new(0));
                    let (send, receive) = futures::channel::oneshot::channel::<()>();
                    let (dial_started, started) = futures::channel::oneshot::channel();
                    let (dial_released, released) = futures::channel::oneshot::channel();
                    let probe = Released {
                        count: releases.clone(),
                        notify: Some(dial_released),
                    };
                    let (_, old) = SessionTask::pending(
                        generation(1),
                        [1; 32],
                        [2; 32],
                        async move {
                            let _probe = probe;
                            let _ = dial_started.send(());
                            receive.await.unwrap();
                            Err(RouteFailure::new(FailureKind::Connect, "old dial failed"))
                        },
                        None,
                    );
                    let observer = old.observe();
                    let (worker_started, worker_ready) = futures::channel::oneshot::channel();
                    let (worker_released, worker_done) = futures::channel::oneshot::channel();
                    let probe = Released {
                        count: releases.clone(),
                        notify: Some(worker_released),
                    };
                    old.spawn(async move {
                        let _probe = probe;
                        let _ = worker_started.send(());
                        futures::future::pending::<()>().await;
                    });
                    if registered {
                        started.await.unwrap();
                        worker_ready.await.unwrap();
                    }
                    if explicit_stop {
                        old.stop();
                        old.stop();
                    }
                    drop(old);
                    assert_eq!(cause(&observer), 3);
                    assert_eq!(
                        releases.get(),
                        0,
                        "abort must not synchronously poll workers"
                    );
                    // Install another pending generation while the old canceled
                    // futures still retain their captures in the Tokio executor.
                    let (new_done, new_released) = futures::channel::oneshot::channel();
                    let (_, fresh) = SessionTask::pending(
                        generation(2),
                        [1; 32],
                        [2; 32],
                        async move {
                            new_done.send(()).unwrap();
                            Err(RouteFailure::new(FailureKind::Timeout, "new dial expired"))
                        },
                        None,
                    );
                    tokio::time::timeout(Duration::from_secs(2), async {
                        released.await.unwrap();
                        worker_done.await.unwrap();
                        new_released.await.unwrap();
                    })
                    .await
                    .unwrap();
                    assert_eq!(releases.get(), 2);
                    assert!(
                        send.send(()).is_err(),
                        "canceled dial still retained its receiver"
                    );
                    assert_eq!(cause(&fresh.observe()), 2);
                    observer
                        .0
                        .fail(RouteFailure::new(FailureKind::Connect, "old dial failed"));
                    assert_eq!(cause(&observer), 3);
                    assert_eq!(observer.generation().get(), 1);
                    drop(fresh);
                }
            }
        })
        .await;
}

#[test]
fn replay_tlc_route_task_ownership() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RouteTaskOwner.tla";
    const CONFIG: &str = include_str!("../../../verification/RouteTaskOwner.cfg");
    let paths = exploration::traces(MODEL, "route-task-owner", CONFIG).unwrap();
    for path in &paths {
        let executor = Manual::default();
        let releases = Rc::new(Cell::new(0));
        let (send, receive) = futures::channel::oneshot::channel::<()>();
        let mut send = Some(send);
        let probe = Dropped(releases.clone());
        let (_, old) = OwnedSession::pending_with_executor(
            generation(1),
            [1; 32],
            [2; 32],
            async move {
                let _probe = probe;
                receive.await.unwrap();
                Err(RouteFailure::new(FailureKind::Connect, "old dial failed"))
            },
            None,
            executor.clone(),
        );
        let probe = Dropped(releases.clone());
        old.spawn(async move {
            let _probe = probe;
            futures::future::pending::<()>().await;
        });
        let old_observer = old.observe();
        let mut old = Some(old);
        let mut fresh: Option<OwnedSession<Manual>> = None;
        for step in path {
            match step["event"] {
                1 => executor.poll(0),
                2 => executor.poll(1),
                3 => {
                    let accepted = send.take().unwrap().send(()).is_ok();
                    assert_eq!(accepted, step["dial"] < 2, "{path:?}");
                }
                4 => old.as_ref().unwrap().stop(),
                5 => drop(old.take()),
                6 => {
                    let (_, owner) = OwnedSession::pending_with_executor(
                        generation(2),
                        [1; 32],
                        [2; 32],
                        async { Err(RouteFailure::new(FailureKind::Timeout, "new dial expired")) },
                        None,
                        executor.clone(),
                    );
                    fresh = Some(owner);
                }
                7 => executor.poll(2),
                8 => old_observer
                    .0
                    .fail(RouteFailure::new(FailureKind::Connect, "old dial failed")),
                event => panic!("unknown ownership event {event}"),
            }
            assert_eq!(u64::from(old.is_some()), step["owner"], "{path:?}");
            assert_eq!(cause(&old_observer), step["outcome"], "{path:?}");
            let tasks = executor.0.borrow();
            assert_eq!(tasks[0].state, step["dial"], "{path:?}");
            assert_eq!(tasks[1].state, step["sibling"], "{path:?}");
            assert_eq!(
                u64::from(tasks[0].abort.is_aborted()),
                step["abortDial"],
                "{path:?}"
            );
            assert_eq!(
                u64::from(tasks[1].abort.is_aborted()),
                step["abortSibling"],
                "{path:?}"
            );
            assert_eq!(
                releases.get(),
                u64::from(step["dial"] >= 2) + u64::from(step["sibling"] == 3),
                "{path:?}"
            );
            if let Some(fresh) = &fresh {
                assert_eq!(fresh.observe().generation().get(), 2);
                assert_eq!(cause(&fresh.observe()), step["newOutcome"], "{path:?}");
                assert_eq!(
                    tasks[2].state,
                    if step["fresh"] == 2 { 2 } else { 0 },
                    "{path:?}"
                );
                assert!(
                    !tasks[2].abort.is_aborted(),
                    "old owner canceled replacement: {path:?}"
                );
            } else {
                assert_eq!(step["fresh"], 0, "{path:?}");
            }
        }
    }
    let live =
        CONFIG.replace("SPECIFICATION Spec", "SPECIFICATION FairSpec") + "\nPROPERTY Progress\n";
    exploration::controls(
        MODEL,
        "route-task-owner",
        CONFIG,
        &[
            ("retainOwner", "OwnerReleased"),
            ("missWorker", "AllWorkersCanceled"),
            ("overwrite", "FirstCause"),
            ("crossGeneration", "GenerationIsolation"),
            ("runCanceled", "NoPostCancelCompletion"),
        ],
        Some(&live),
    )
    .unwrap();
    eprintln!("{} native route-owner edge-prefix replays", paths.len());
}

struct Release {
    count: Rc<Cell<u64>>,
    trace: Trace,
    id: u64,
}
impl Drop for Release {
    fn drop(&mut self) {
        self.count.set(self.count.get() + 1);
        self.trace.event("release", self.id);
    }
}

fn cause(observer: &RouteObserver) -> u64 {
    match observer.termination() {
        None => 0,
        Some(Termination::Canceled) => 3,
        Some(Termination::Failed(error)) => match error.kind {
            FailureKind::Connect => {
                assert_eq!(error.message, "old dial failed");
                1
            }
            FailureKind::Timeout => {
                assert_eq!(error.message, "new dial expired");
                2
            }
            kind => panic!("unexpected failure: {kind:?}"),
        },
        other => panic!("unexpected cause: {other:?}"),
    }
}

fn first(expected: &Cell<u64>, value: u64) {
    if expected.get() == 0 {
        expected.set(value);
    }
}

fn pending_owner(
    executor: Executor,
    generation: u64,
    trace: Trace,
    expected: Rc<Cell<u64>>,
    releases: Rc<Cell<u64>>,
) -> (
    OwnedSession<Executor>,
    futures::channel::oneshot::Sender<()>,
) {
    let (send, mut receive) = futures::channel::oneshot::channel();
    let probe = Release {
        count: releases.clone(),
        trace: trace.clone(),
        id: generation * 10,
    };
    let dial_trace = trace.clone();
    let selection = async move {
        let _probe = probe;
        futures::future::poll_fn(|cx| {
            let result = Pin::new(&mut receive).poll(cx);
            dial_trace.event(
                &format!("poll-dial-{generation}"),
                u64::from(result.is_ready()),
            );
            result
        })
        .await
        .unwrap();
        first(&expected, generation);
        dial_trace.event("dial-result", generation);
        Err(if generation == 1 {
            RouteFailure::new(FailureKind::Connect, "old dial failed")
        } else {
            RouteFailure::new(FailureKind::Timeout, "new dial expired")
        })
    };
    let (_io, owner) = OwnedSession::pending_with_executor(
        super::generation(generation),
        [1; 32],
        [2; 32],
        selection,
        None,
        executor,
    );
    // A second owned worker represents another independently pending task.
    // It is deliberately not a simulated RPC writer or transport driver.
    let probe = Release {
        count: releases,
        trace,
        id: generation * 10 + 1,
    };
    owner.spawn(async move {
        let _probe = probe;
        futures::future::pending::<()>().await;
    });
    (owner, send)
}

fn owner_schedules(trace: Trace) {
    block_on(async {
        let executor = Executor::default();
        let old_expected = Rc::new(Cell::new(0));
        let new_expected = Rc::new(Cell::new(0));
        let old_releases = Rc::new(Cell::new(0));
        let new_releases = Rc::new(Cell::new(0));
        let (old, old_send) = pending_owner(
            executor.clone(),
            1,
            trace.clone(),
            old_expected.clone(),
            old_releases.clone(),
        );
        let old_observer = old.observe();
        let old = Rc::new(RefCell::new(Some(old)));
        let (fresh, new_send) = pending_owner(
            executor.clone(),
            2,
            trace.clone(),
            new_expected.clone(),
            new_releases.clone(),
        );
        let new_observer = fresh.observe();
        let mut actors = Vec::new();
        for (send, id) in [(old_send, 1), (new_send, 2)] {
            let trace = trace.clone();
            actors.push(spawn_local(async move {
                yield_now().await;
                trace.event(&format!("delivered-{id}"), u64::from(send.send(()).is_ok()));
            }));
        }
        {
            let old = old.clone();
            let expected = old_expected.clone();
            let trace = trace.clone();
            actors.push(spawn_local(async move {
                yield_now().await;
                if let Some(owner) = old.borrow().as_ref() {
                    first(&expected, 3);
                    trace.event("stop-old", expected.get());
                    owner.stop();
                    owner.stop(); // Repeated disconnect is harmless.
                }
            }));
        }
        {
            let old = old.clone();
            let expected = old_expected.clone();
            let trace = trace.clone();
            actors.push(spawn_local(async move {
                yield_now().await;
                first(&expected, 3);
                trace.event("drop-old", expected.get());
                drop(old.borrow_mut().take());
            }));
        }
        for (observer, expected) in [
            (old_observer.clone(), old_expected.clone()),
            (new_observer.clone(), new_expected.clone()),
        ] {
            let trace = trace.clone();
            actors.push(spawn_local(async move {
                for _ in 0..3 {
                    yield_now().await;
                    assert_eq!(cause(&observer), expected.get());
                    trace.event(
                        &format!("observe-{}", observer.generation()),
                        expected.get(),
                    );
                }
            }));
        }
        for actor in actors {
            actor.await.unwrap();
        }
        // Joining a worker is necessary: stop requests cancellation, and the
        // executor releases captured resources when it next polls that task.
        // The new pending dial must finish without cancellation from the old one.
        while cause(&new_observer) == 0 {
            yield_now().await;
        }
        assert_eq!(cause(&new_observer), 2);
        assert_eq!(new_releases.get(), 1);
        fresh.stop();
        drop(fresh);
        executor.join().await;
        assert!(old.borrow().is_none());
        assert_eq!(old_releases.get(), 2);
        assert_eq!(new_releases.get(), 2);
        assert_eq!(cause(&old_observer), old_expected.get());
        assert_eq!(cause(&new_observer), 2);
        trace.event("old-outcome", cause(&old_observer));
        trace.event("new-outcome", cause(&new_observer));
    });
}

#[test]
fn shuttle_route_owners_cancel_pending_tasks_and_isolate_generations() {
    let batch = schedules::check("route-owners", owner_schedules);
    if !batch.sampled {
        return;
    }
    for value in [1, 3] {
        assert!(batch
            .runs
            .iter()
            .any(|run| run.events.contains(&("old-outcome".into(), value))));
    }
    for delivered in [0, 1] {
        assert!(batch
            .runs
            .iter()
            .any(|run| run.events.contains(&("delivered-1".into(), delivered))));
    }
    assert!(
        batch.runs.iter().any(|run| {
            run.events.contains(&("old-outcome".into(), 3))
                && !run.events.iter().any(|(name, _)| name == "poll-dial-1")
        }),
        "no cancellation before first dial poll"
    );
    assert!(
        batch.runs.iter().any(|run| {
            run.events.contains(&("old-outcome".into(), 3))
                && run.events.contains(&("poll-dial-1".into(), 0))
        }),
        "no cancellation of a registered dial"
    );
}

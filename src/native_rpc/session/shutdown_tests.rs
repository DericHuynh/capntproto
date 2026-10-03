use super::*;
use crate::native_shutdown::Receipt;
use capntproto_test_support::schedules::{self, block_on, spawn_local, yield_now, Trace};
use futures::FutureExt;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn draining() -> (Rc<Lifecycle>, Control) {
    let lifecycle = Lifecycle::new(generation(1));
    lifecycle.activate(Control::new()).unwrap();
    let control = lifecycle.begin_shutdown(Duration::from_secs(1)).unwrap();
    (lifecycle, control)
}

#[test]
fn canceled_route_rejects_a_receipt_recorded_before_cancellation() {
    let (lifecycle, control) = draining();
    control.finish(Ok(Receipt { bytes: 41 }));
    lifecycle.finish(Termination::Canceled);
    let result = futures::executor::block_on(lifecycle.complete_shutdown(
        Promise::ok(()),
        Promise::ok(()).shared(),
        &control,
        futures::future::pending(),
    ));
    assert!(
        result.is_err(),
        "canceled route returned a successful shutdown receipt"
    );
    assert_eq!(
        RouteObserver(lifecycle).termination(),
        Some(Termination::Canceled)
    );
}

#[test]
fn terminal_transition_wakes_shutdown_blocked_before_receipt_wait() {
    for failed in [false, true] {
        let (lifecycle, control) = draining();
        control.finish(Ok(Receipt { bytes: 41 }));
        let mut wait = Box::pin(lifecycle.complete_shutdown(
            Promise::from_future(futures::future::pending()),
            Promise::ok(()).shared(),
            &control,
            futures::future::pending(),
        ));
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        let message = if failed {
            "first route failure"
        } else {
            "Native route canceled"
        };
        if failed {
            lifecycle.fail(RouteFailure::new(FailureKind::PeerClosed, message));
        } else {
            lifecycle.finish(Termination::Canceled);
        }
        assert!(
            wakes.0.load(Ordering::Relaxed) > 0,
            "blocked drain lost the terminal notification"
        );
        let Poll::Ready(Err(error)) = wait.as_mut().poll(&mut cx) else {
            panic!("terminal route still pending");
        };
        assert_eq!(error.extra, message);
        lifecycle.fail(RouteFailure::new(FailureKind::Writer, "late writer error"));
        assert_eq!(
            futures::executor::block_on(lifecycle.terminal_error()).extra,
            message
        );
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn shutdown_deadline_bounds_each_fence_and_completed_chain_wins_ties() {
    for first_poll in [false, true] {
        for pending_stage in 0..=3 {
            let (lifecycle, control) = draining();
            if pending_stage != 2 {
                control.finish(Ok(Receipt { bytes: 41 }));
            }
            let flush = if pending_stage == 0 {
                Promise::from_future(futures::future::pending())
            } else {
                Promise::ok(())
            };
            let output = if pending_stage == 1 {
                Promise::from_future(futures::future::pending())
            } else {
                Promise::ok(())
            };
            let mut wait = Box::pin(lifecycle.complete_shutdown(
                flush,
                output.shared(),
                &control,
                control.expired(),
            ));
            if first_poll && pending_stage != 3 {
                assert!(futures::poll!(&mut wait).is_pending());
            }
            // Exercise expiration before the initial poll and with a timer already
            // registered while blocked on each of the three fences.
            tokio::time::advance(Duration::from_secs(2)).await;
            let result = wait.await;
            if pending_stage == 3 {
                assert_eq!(result.unwrap().bytes, 41);
                assert_eq!(
                    RouteObserver(lifecycle.clone()).termination(),
                    Some(Termination::ShutdownAcknowledged)
                );
            } else {
                assert_eq!(result.unwrap_err().extra, "Native shutdown timed out");
                assert!(matches!(
                    RouteObserver(lifecycle.clone()).termination(),
                    Some(Termination::Failed(RouteFailure {
                        kind: FailureKind::Timeout,
                        ..
                    }))
                ));
            }
            let terminal = RouteObserver(lifecycle.clone()).termination();
            lifecycle.finish(Termination::Canceled);
            assert_eq!(RouteObserver(lifecycle).termination(), terminal);
        }
    }
}

type Wait = Pin<Box<dyn Future<Output = capnp::Result<Receipt>>>>;
struct Inputs {
    lifecycle: Rc<Lifecycle>,
    control: Control,
    signals: [Option<futures::channel::oneshot::Sender<()>>; 3],
    mode: u64,
}
impl Inputs {
    fn new(mode: u64) -> (Self, Wait) {
        let (lifecycle, control) = draining();
        let (flush_tx, flush_rx) = futures::channel::oneshot::channel();
        let (close_tx, close_rx) = futures::channel::oneshot::channel();
        let (expire_tx, expire_rx) = futures::channel::oneshot::channel();
        let flush = Promise::from_future(async move {
            flush_rx.await.unwrap();
            if mode == 1 {
                Err(capnp::Error::failed("flush fault".into()))
            } else {
                Ok(())
            }
        });
        let close = Promise::from_future(async move {
            close_rx.await.unwrap();
            if mode == 2 {
                Err(capnp::Error::failed("close fault".into()))
            } else {
                Ok(())
            }
        })
        .shared();
        let life = lifecycle.clone();
        let ctrl = control.clone();
        let wait = Box::pin(async move {
            life.complete_shutdown(flush, close, &ctrl, async {
                expire_rx.await.unwrap();
            })
            .await
        });
        (
            Self {
                lifecycle,
                control,
                signals: [Some(flush_tx), Some(close_tx), Some(expire_tx)],
                mode,
            },
            wait,
        )
    }
    fn signal(&mut self, index: usize) {
        let _ = self.signals[index].take().unwrap().send(());
    }
    fn receipt(&self) {
        self.control.finish(if self.mode == 3 {
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "peer fault"))
        } else {
            Ok(Receipt { bytes: 41 })
        });
    }
}
fn error_code(text: &str) -> u64 {
    for (index, marker) in [
        "flush fault",
        "close fault",
        "peer fault",
        "Native shutdown timed out",
        "Native route canceled",
        "prior route failure",
    ]
    .iter()
    .enumerate()
    {
        if text.contains(marker) {
            return index as u64 + 2;
        }
    }
    panic!("unexpected shutdown failure {text}");
}
fn result_code(result: capnp::Result<Receipt>) -> u64 {
    match result {
        Ok(receipt) => {
            assert_eq!(receipt.bytes, 41);
            1
        }
        Err(error) => {
            assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
            error_code(&error.extra)
        }
    }
}
fn cause(lifecycle: &Rc<Lifecycle>) -> u64 {
    match RouteObserver(lifecycle.clone()).termination() {
        None => 0,
        Some(Termination::ShutdownAcknowledged) => 1,
        Some(Termination::Canceled) => 6,
        Some(Termination::Failed(error)) => {
            let code = error_code(&error.message);
            assert_eq!(
                error.kind,
                match code {
                    2 | 3 => FailureKind::Writer,
                    4 => FailureKind::Transport,
                    5 => FailureKind::Timeout,
                    7 => FailureKind::PeerClosed,
                    _ => panic!("unexpected structured error: {error:?}"),
                }
            );
            code
        }
    }
}
fn control_outcome(control: &Control) -> u64 {
    match control.wait().now_or_never() {
        None => 0,
        Some(Ok(receipt)) => {
            assert_eq!(receipt.bytes, 41);
            1
        }
        Some(Err(error)) if error.to_string() == "peer fault" => 2,
        Some(Err(error)) => {
            assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
            3
        }
    }
}

#[test]
fn replay_tlc_shutdown_completion() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RouteShutdownCompletion.tla";
    const CONFIG: &str = include_str!("../../../verification/RouteShutdownCompletion.cfg");
    let mut total = 0;
    for mode in 0..=3 {
        let config = CONFIG.replace("Mode = 0", &format!("Mode = {mode}"));
        let report = format!("route-shutdown-completion/{mode}");
        let paths = exploration::traces(MODEL, &report, &config).unwrap();
        total += paths.len();
        for path in &paths {
            let (mut inputs, wait) = Inputs::new(mode);
            let mut wait = Some(wait);
            let mut result = 0;
            for step in path {
                match step["event"] {
                    1 => inputs.signal(0),
                    2 => inputs.signal(1),
                    3 => inputs.receipt(),
                    4 => inputs.signal(2),
                    5 => inputs.lifecycle.finish(Termination::Canceled),
                    6 => inputs.lifecycle.fail(RouteFailure::new(
                        FailureKind::PeerClosed,
                        "prior route failure",
                    )),
                    7 => {
                        let polled = wait
                            .as_mut()
                            .unwrap()
                            .as_mut()
                            .poll(&mut Context::from_waker(futures::task::noop_waker_ref()));
                        if let Poll::Ready(value) = polled {
                            result = result_code(value);
                            wait.take();
                        }
                    }
                    other => panic!("unexpected shutdown event {other}"),
                }
                assert_eq!(result, step["result"], "mode={mode} {path:?}");
                assert_eq!(
                    cause(&inputs.lifecycle),
                    step["cause"],
                    "mode={mode} {path:?}"
                );
                assert_eq!(
                    control_outcome(&inputs.control),
                    step["control"],
                    "mode={mode} {path:?}"
                );
                let expected = match step["cause"] {
                    0 => RouteStatus::Draining,
                    1 | 6 => RouteStatus::Stopped,
                    _ => RouteStatus::Failed,
                };
                assert_eq!(inputs.lifecycle.status(), expected, "{path:?}");
            }
        }
        let faults: &[(&str, &str)] = if mode == 0 {
            &[
                ("skipFlush", "AllFences"),
                ("skipClose", "AllFences"),
                ("skipReceipt", "AllFences"),
                ("ignoreTerminal", "OutcomeAgreement"),
                ("deadlineFirst", "CompletionPriority"),
                ("overwrite", "FirstCause"),
            ]
        } else {
            &[]
        };
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION FairSpec")
            + "\nPROPERTY Progress\n";
        exploration::controls(MODEL, &report, &config, faults, Some(&live)).unwrap();
    }
    eprintln!("{total} native shutdown-completion edge-prefix replays");
}

#[derive(Default)]
struct Expected {
    flush: bool,
    close: bool,
    receipt: bool,
    expired: bool,
    terminal: u64,
}
impl Expected {
    fn result(&self, mode: u64) -> u64 {
        if self.terminal != 0 {
            return self.terminal;
        }
        let chain = if !self.flush {
            0
        } else if mode == 1 {
            2
        } else if !self.close {
            0
        } else if mode == 2 {
            3
        } else if !self.receipt {
            0
        } else if mode == 3 {
            4
        } else {
            1
        };
        if chain != 0 {
            chain
        } else if self.expired {
            5
        } else {
            0
        }
    }
    fn finish(&mut self, value: u64) {
        if self.terminal == 0 {
            self.terminal = value;
        }
    }
}

async fn race_completion(trace: Trace, mode: u64, terminal_actors: bool) {
    trace.event("mode", mode);
    let (mut inputs, mut wait) = Inputs::new(mode);
    let expected = Rc::new(RefCell::new(Expected::default()));
    let reader = {
        let expected = expected.clone();
        let lifecycle = inputs.lifecycle.clone();
        let trace = trace.clone();
        spawn_local(async move {
            let code = futures::future::poll_fn(|cx| {
                let want = expected.borrow().result(mode);
                let result = wait.as_mut().poll(cx);
                assert_eq!(result.is_ready(), want != 0);
                trace.event("poll-shutdown", want);
                result.map(|result| {
                    let code = result_code(result);
                    assert_eq!(code, want);
                    expected.borrow_mut().finish(code);
                    assert_eq!(cause(&lifecycle), expected.borrow().terminal);
                    code
                })
            })
            .await;
            trace.event("shutdown-result", code);
        })
    };
    let mut actors = Vec::new();
    for event in 0..4 {
        let send = match event {
            0 | 1 => inputs.signals[event].take(),
            3 => inputs.signals[2].take(),
            _ => None,
        };
        let expected = expected.clone();
        let control = inputs.control.clone();
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            yield_now().await;
            match event {
                0 => expected.borrow_mut().flush = true,
                1 => expected.borrow_mut().close = true,
                2 => expected.borrow_mut().receipt = true,
                3 => expected.borrow_mut().expired = true,
                _ => unreachable!(),
            }
            if let Some(send) = send {
                let _ = send.send(());
            } else {
                control.finish(if mode == 3 {
                    Err(io::Error::new(io::ErrorKind::ConnectionReset, "peer fault"))
                } else {
                    Ok(Receipt { bytes: 41 })
                });
            }
            trace.event("ready", event as u64);
        }));
    }
    if terminal_actors {
        for value in [6, 7] {
            let lifecycle = inputs.lifecycle.clone();
            let expected = expected.clone();
            let trace = trace.clone();
            actors.push(spawn_local(async move {
                yield_now().await;
                expected.borrow_mut().finish(value);
                if value == 6 {
                    lifecycle.finish(Termination::Canceled);
                } else {
                    lifecycle.fail(RouteFailure::new(
                        FailureKind::PeerClosed,
                        "prior route failure",
                    ));
                }
                trace.event("terminal-contender", value);
                assert_eq!(cause(&lifecycle), expected.borrow().terminal);
            }));
        }
    }
    for actor in actors {
        actor.await.unwrap();
    }
    reader.await.unwrap();
    assert_eq!(cause(&inputs.lifecycle), expected.borrow().terminal);
    trace.event("terminal-cause", cause(&inputs.lifecycle));
}

async fn blocked_cancellation(trace: Trace, lose_waker: bool) {
    let (lifecycle, control) = draining();
    control.finish(Ok(Receipt { bytes: 41 }));
    let (registered, ready) = futures::channel::oneshot::channel();
    let reader = {
        let lifecycle = lifecycle.clone();
        let trace = trace.clone();
        spawn_local(async move {
            let mut wait = Box::pin(lifecycle.complete_shutdown(
                Promise::from_future(futures::future::pending()),
                Promise::ok(()).shared(),
                &control,
                futures::future::pending(),
            ));
            let mut registered = Some(registered);
            let result = futures::future::poll_fn(|cx| {
                let result = if lose_waker {
                    wait.as_mut()
                        .poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
                } else {
                    wait.as_mut().poll(cx)
                };
                if result.is_pending() {
                    if let Some(send) = registered.take() {
                        trace.event("blocked-with-receipt", 1);
                        send.send(()).unwrap();
                    }
                }
                result
            })
            .await;
            assert_eq!(result_code(result), 6);
            trace.event("blocked-cancel-observed", 6);
        })
    };
    let canceler = spawn_local(async move {
        ready.await.unwrap();
        yield_now().await;
        lifecycle.finish(Termination::Canceled);
        trace.event("terminal-canceled", 1);
    });
    canceler.await.unwrap();
    reader.await.unwrap(); // Missing terminal notification must deadlock here.
}

#[test]
fn shuttle_shutdown_fences_compete_with_deadline() {
    let batch = schedules::check("shutdown-fences", |trace| {
        block_on(async {
            for mode in 0..=3 {
                race_completion(trace.clone(), mode, false).await;
            }
        })
    });
    if batch.sampled {
        for mode in 0..=3 {
            for result in [mode + 1, 5] {
                assert_observed(&batch, mode, result);
            }
        }
    }
}

#[test]
fn shuttle_shutdown_terminal_causes_and_cancellation_wake_blocked_fences() {
    let batch = schedules::check("shutdown-terminal", |trace| {
        block_on(async {
            for mode in 0..=3 {
                race_completion(trace.clone(), mode, true).await;
            }
            blocked_cancellation(trace, false).await;
        })
    });
    if batch.sampled {
        for mode in 0..=3 {
            for result in [mode + 1, 5, 6, 7] {
                assert_observed(&batch, mode, result);
            }
        }
    }
}

#[test]
#[ignore = "negative control: the schedule gate requires a lost terminal wakeup to deadlock"]
fn shuttle_detects_lost_shutdown_terminal_wakeup() {
    schedules::check("shutdown-terminal-lost-wakeup", |trace| {
        block_on(blocked_cancellation(trace, true))
    });
}
fn assert_observed(batch: &schedules::Batch, mode: u64, result: u64) {
    assert!(
        batch.runs.iter().any(|run| {
            run.events
                .iter()
                .skip_while(|(name, value)| name != "mode" || *value != mode)
                .skip(1)
                .take_while(|(name, _)| name != "mode")
                .any(|(name, value)| name == "shutdown-result" && *value == result)
        }),
        "mode {mode} never produced {result}"
    );
}

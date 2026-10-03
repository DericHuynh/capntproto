//! Drive the real serializer, two-party output fence and route watcher. Only
//! the byte-stream readiness/failures and local executor are controlled here.
use super::{
    test_executor::{Executor, Manual},
    *,
};
use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork};
use futures::{Future, FutureExt};
use reproto_test_support::schedules::{self, block_on, spawn_local, yield_now, Trace};
use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

type Fence = futures::future::Shared<Promise<(), capnp::Error>>;
type Connection = Box<dyn capnp_rpc::Connection<Side>>;
// Each fixture envelope contains a root pointer, Message and Bootstrap structs
// (five words), preceded by one segment-table word.
const WIRE_BYTES: usize = 2 * 6 * 8;

#[derive(Default)]
struct Output {
    gates: [bool; 3],
    wakes: [Option<Waker>; 3],
    error_at: usize,
    bytes: Vec<u8>,
    phase: u64,
    flushed: bool,
    closed: bool,
    trace: Option<Trace>,
}
impl Output {
    fn ready(&mut self, stage: usize, cx: &Context<'_>) -> Poll<io::Result<()>> {
        if let Some(trace) = &self.trace {
            trace.event(&format!("io-poll-{stage}"), u64::from(self.gates[stage]));
        }
        if !self.gates[stage] {
            self.wakes[stage] = Some(cx.waker().clone());
            return Poll::Pending;
        }
        if self.error_at == stage + 1 {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                ["write fault", "flush fault", "close fault"][stage],
            )))
        } else {
            Poll::Ready(Ok(()))
        }
    }
    fn release(output: &Rc<RefCell<Self>>, stage: usize) {
        let wake = {
            let mut output = output.borrow_mut();
            output.gates[stage] = true;
            output.wakes[stage].take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
struct Writer(Rc<RefCell<Output>>);
impl futures::AsyncWrite for Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut out = self.0.borrow_mut();
        match out.ready(0, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                out.phase = 2;
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(())) => {
                // Short writes exercise the actual framing/serialization loop.
                let n = bytes.len().min(3);
                out.bytes.extend_from_slice(&bytes[..n]);
                if out.bytes.len() == WIRE_BYTES {
                    out.phase = 1;
                }
                assert!(
                    out.bytes.len() <= WIRE_BYTES,
                    "late message escaped the queue terminator"
                );
                Poll::Ready(Ok(n))
            }
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut out = self.0.borrow_mut();
        let result = out.ready(1, cx);
        if result.is_ready() {
            out.flushed = matches!(result, Poll::Ready(Ok(())));
            out.phase = 2;
        }
        result
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut out = self.0.borrow_mut();
        let result = out.ready(2, cx);
        if result.is_ready() {
            out.closed = true;
            out.phase = 3;
        }
        result
    }
}
struct Input;
impl futures::AsyncRead for Input {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        panic!("output fence must not poll input");
    }
}
struct Harness<E: LocalExecutor> {
    owner: Rc<OwnedSession<E>>,
    connection: Rc<RefCell<Option<Connection>>>,
    output: Rc<RefCell<Output>>,
    marker: Fence,
    closed: Fence,
    late: Promise<(), capnp::Error>,
}
impl<E: LocalExecutor> Harness<E> {
    fn new(executor: E, error_at: usize, trace: Option<Trace>) -> Self {
        let output = Rc::new(RefCell::new(Output {
            error_at,
            trace,
            ..Output::default()
        }));
        let mut network = capnp_rpc::twoparty::VatNetwork::new(
            Input,
            Writer(output.clone()),
            Side::Client,
            Default::default(),
        );
        let mut connection = network.connect(Side::Server).unwrap();
        for value in [11, 22] {
            let mut message = connection.new_outgoing_message(8);
            message
                .get_body()
                .unwrap()
                .init_as::<capnp_rpc::rpc_capnp::message::Builder>()
                .init_bootstrap()
                .set_question_id(value);
            drop(message.send());
        }
        let mut late = connection.new_outgoing_message(8);
        late.get_body()
            .unwrap()
            .init_as::<capnp_rpc::rpc_capnp::message::Builder>()
            .init_bootstrap()
            .set_question_id(99);
        let marker = connection.shutdown(Ok(())).shared();
        let late = late.send().0;
        let closed = network.output_closed();
        let owner = Rc::new(OwnedSession::with_executor(generation(1), None, executor));
        // Only the lifecycle policy is initialized here. This fixture does not
        // manufacture an AuthenticatedSession or bypass identity validation.
        owner.lifecycle.activate(Control::new()).unwrap();
        owner.watch_writer(network.drive_until_shutdown(), closed.clone());
        drop(network); // The route worker must be the sole execution-driver owner.
        Self {
            owner,
            connection: Rc::new(RefCell::new(Some(connection))),
            output,
            marker,
            closed,
            late,
        }
    }
}
fn outcome(observer: &RouteObserver) -> u64 {
    match observer.termination() {
        None => 0,
        Some(Termination::Failed(error)) => {
            assert_eq!(error.kind, FailureKind::Writer);
            assert!(
                ["write fault", "flush fault", "close fault"]
                    .iter()
                    .any(|text| error.message.contains(text)),
                "{error:?}"
            );
            1
        }
        Some(Termination::Canceled) => 2,
        other => panic!("unexpected route result: {other:?}"),
    }
}
fn fence_result(result: Poll<capnp::Result<()>>) -> u64 {
    match result {
        Poll::Pending => 0,
        Poll::Ready(Ok(())) => 1,
        Poll::Ready(Err(error)) => {
            if error.extra == "output driver canceled" {
                3
            } else {
                2
            }
        }
    }
}
fn poll(future: &Fence) -> u64 {
    // A Shared handle is consumed by its successful poll. A fresh clone can
    // observe the cached result without polling a completed handle again.
    fence_result(
        Pin::new(&mut future.clone())
            .poll(&mut Context::from_waker(futures::task::noop_waker_ref())),
    )
}
fn check_bytes(output: &Output) {
    assert_eq!(output.bytes.len(), WIRE_BYTES);
    let mut remaining = &output.bytes[..];
    for value in [11, 22] {
        let message = capnp::serialize::read_message(&mut remaining, Default::default()).unwrap();
        let envelope = message
            .get_root::<capnp_rpc::rpc_capnp::message::Reader>()
            .unwrap();
        let capnp_rpc::rpc_capnp::message::Bootstrap(bootstrap) = envelope.which().unwrap() else {
            panic!("expected Bootstrap envelope");
        };
        assert_eq!(bootstrap.unwrap().get_question_id(), value);
    }
    assert!(remaining.is_empty());
}

#[test]
fn output_fence_reports_before_disconnect_and_preserves_draining() {
    for error_at in 0..=3 {
        for draining in [false, true] {
            let executor = Manual::default();
            let mut h = Harness::new(executor.clone(), error_at, None);
            if draining {
                h.owner
                    .lifecycle
                    .begin_shutdown(Duration::from_secs(1))
                    .unwrap();
            }
            assert_eq!(poll(&h.marker), 0);
            assert_eq!(poll(&h.closed), 0);
            for stage in 0..3 {
                Output::release(&h.output, stage);
                executor.poll(0);
                if stage < 2 {
                    assert_eq!(poll(&h.closed), 0);
                }
            }
            assert_eq!(
                poll(&h.marker),
                if matches!(error_at, 1 | 2) { 2 } else { 1 }
            );
            assert_eq!(poll(&h.closed), if error_at == 0 { 1 } else { 2 });
            assert_eq!(
                executor.0.borrow()[0].state,
                1,
                "driver must still await disconnect"
            );
            assert_eq!(
                outcome(&h.owner.observe()),
                u64::from(error_at != 0 && !draining)
            );
            if error_at == 0 {
                check_bytes(&h.output.borrow());
            }
            h.connection.borrow_mut().take();
            executor.poll(0);
            assert_eq!(executor.0.borrow()[0].state, 2);
            assert_eq!(
                outcome(&h.owner.observe()),
                u64::from(error_at != 0 && !draining)
            );
            assert!(matches!(
                Pin::new(&mut h.late)
                    .poll(&mut Context::from_waker(futures::task::noop_waker_ref())),
                Poll::Ready(Err(_))
            ));
        }
    }
}

#[test]
fn receipt_cannot_bypass_real_output_close_or_route_cancellation() {
    for error_at in [0, 3] {
        for cancel in [false, true] {
            let executor = Manual::default();
            let h = Harness::new(executor.clone(), error_at, None);
            let control = h
                .owner
                .lifecycle
                .begin_shutdown(Duration::from_secs(1))
                .unwrap();
            // Treat receipt arrival as adversarial input: even an already
            // recorded receipt cannot bypass the real output close result.
            control.finish(Ok(crate::native_shutdown::Receipt {
                bytes: WIRE_BYTES as u64,
            }));
            let mut wait = Box::pin(h.owner.lifecycle.complete_shutdown(
                Promise::from_future(h.marker.clone()),
                h.closed.clone(),
                &control,
                futures::future::pending(),
            ));
            Output::release(&h.output, 0);
            Output::release(&h.output, 1);
            executor.poll(0);
            assert_eq!(poll(&h.marker), 1);
            assert_eq!(poll(&h.closed), 0);
            let mut cx = Context::from_waker(futures::task::noop_waker_ref());
            assert!(wait.as_mut().poll(&mut cx).is_pending());
            if cancel {
                h.owner.stop();
            } else {
                Output::release(&h.output, 2);
                executor.poll(0);
            }
            let Poll::Ready(result) = wait.as_mut().poll(&mut cx) else {
                panic!("shutdown did not resolve");
            };
            if cancel {
                assert_eq!(result.unwrap_err().extra, "Native route canceled");
                assert_eq!(h.owner.observe().termination(), Some(Termination::Canceled));
                executor.poll(0); // Release the canceled writer after observing cancellation.
            } else if error_at == 3 {
                assert!(result.unwrap_err().extra.contains("close fault"));
                assert!(matches!(
                    h.owner.observe().termination(),
                    Some(Termination::Failed(RouteFailure {
                        kind: FailureKind::Writer,
                        ..
                    }))
                ));
            } else {
                assert_eq!(result.unwrap().bytes, WIRE_BYTES as u64);
                assert_eq!(
                    h.owner.observe().termination(),
                    Some(Termination::ShutdownAcknowledged)
                );
            }
        }
    }
}

#[test]
fn replay_tlc_route_output_fences() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RouteOutputFence.tla";
    const CONFIG: &str = include_str!("../../../verification/RouteOutputFence.cfg");
    let mut total = 0;
    for error_at in 0..=3 {
        let config = CONFIG.replace("ErrorAt = 0", &format!("ErrorAt = {error_at}"));
        let report = format!("route-output-fence/{error_at}");
        let paths = exploration::traces(MODEL, &report, &config).unwrap();
        total += paths.len();
        for path in &paths {
            let executor = Manual::default();
            let h = Harness::new(executor.clone(), error_at, None);
            for step in path {
                match step["event"] {
                    1..=3 => Output::release(&h.output, (step["event"] - 1) as usize),
                    4 => executor.poll(0),
                    5 => {
                        h.owner
                            .lifecycle
                            .begin_shutdown(Duration::from_secs(1))
                            .unwrap();
                    }
                    6 => h.owner.stop(),
                    7 => {
                        h.connection.borrow_mut().take();
                    }
                    event => panic!("unknown output event {event}"),
                }
                let task = &executor.0.borrow()[0];
                let output = h.output.borrow();
                let phase = match task.state {
                    2 => 4,
                    3 => 5,
                    _ => output.phase,
                };
                assert_eq!(phase, step["phase"], "mode={error_at} {path:?}");
                assert_eq!(
                    u64::from(output.bytes.len() == WIRE_BYTES),
                    step["wrote"],
                    "{path:?}"
                );
                assert_eq!(u64::from(output.flushed), step["flushed"], "{path:?}");
                assert_eq!(u64::from(output.closed), step["closed"], "{path:?}");
                assert_eq!(poll(&h.marker), step["marker"], "mode={error_at} {path:?}");
                assert_eq!(poll(&h.closed), step["fence"], "mode={error_at} {path:?}");
                assert_eq!(
                    outcome(&h.owner.observe()),
                    step["cause"],
                    "mode={error_at} {path:?}"
                );
                assert_eq!(
                    h.owner.status(),
                    match step["cause"] {
                        0 if step["draining"] == 1 => RouteStatus::Draining,
                        0 => RouteStatus::Authenticated,
                        1 => RouteStatus::Failed,
                        2 => RouteStatus::Stopped,
                        _ => unreachable!(),
                    },
                    "{path:?}"
                );
                for (stage, name) in ["writeGate", "flushGate", "closeGate"].iter().enumerate() {
                    assert_eq!(u64::from(output.gates[stage]), step[*name], "{path:?}");
                }
                assert_eq!(
                    u64::from(h.connection.borrow().is_none()),
                    step["disconnected"],
                    "{path:?}"
                );
                assert_eq!(
                    u64::from(task.abort.is_aborted()),
                    step["stopped"],
                    "{path:?}"
                );
                if step["fence"] == 1 {
                    check_bytes(&output);
                }
            }
        }
        let faults = if error_at == 0 {
            vec![
                ("earlyFence", "OutputPublication"),
                ("skipFlush", "MarkerFence"),
                ("missCancel", "CanceledPoll"),
            ]
        } else if error_at == 1 {
            vec![
                ("overwrite", "FirstCause"),
                ("drainFailure", "DrainOwnsCause"),
            ]
        } else {
            vec![]
        };
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION FairSpec")
            + "\nPROPERTY Progress\n";
        exploration::controls(MODEL, &report, &config, &faults, Some(&live)).unwrap();
    }
    eprintln!("{total} native route-output edge-prefix replays");
}

async fn scheduled_output(trace: Trace, error_at: usize, stop: bool) {
    trace.event("mode", error_at as u64);
    let executor = Executor::default();
    let h = Harness::new(executor.clone(), error_at, Some(trace.clone()));
    let observer = h.owner.observe();
    let expected = Rc::new(std::cell::Cell::new(0));
    let draining = Rc::new(std::cell::Cell::new(false));
    let mut actors = Vec::new();
    for stage in 0..3 {
        let output = h.output.clone();
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            yield_now().await;
            trace.event("release-io", stage as u64);
            Output::release(&output, stage);
        }));
    }
    {
        let connection = h.connection.clone();
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            yield_now().await;
            connection.borrow_mut().take();
            trace.event("disconnect", 1);
        }));
    }
    {
        let owner = h.owner.clone();
        let draining = draining.clone();
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            yield_now().await;
            let began = owner
                .lifecycle
                .begin_shutdown(Duration::from_secs(1))
                .is_ok();
            draining.set(began);
            trace.event("begin-drain", u64::from(began));
        }));
    }
    if stop {
        let owner = h.owner.clone();
        let output = h.output.clone();
        let draining = draining.clone();
        let expected = expected.clone();
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            yield_now().await;
            // Closing the real output and observing its error happen within
            // one watcher poll. Draining reserves that cause for shutdown.
            expected.set(if output.borrow().closed && !draining.get() {
                1
            } else {
                2
            });
            trace.event(
                "stop-after-drained-error",
                u64::from(output.borrow().closed && draining.get()),
            );
            owner.stop();
            assert_eq!(outcome(&owner.observe()), expected.get());
            trace.event("stop-result", expected.get());
        }));
    }
    let (abort, registration) = futures::future::AbortHandle::new_pair();
    {
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            yield_now().await;
            abort.abort();
            trace.event("cancel-waiter", 1);
        }));
    }
    let mut registration = Some(registration);
    for id in 0..3 {
        let mut wait = h.closed.clone();
        let replacement = h.closed.clone();
        let output = h.output.clone();
        let registration = registration.take();
        let trace = trace.clone();
        actors.push(spawn_local(async move {
            let observed = futures::future::poll_fn(|cx| {
                let value = Pin::new(&mut wait).poll(cx);
                trace.event(&format!("fence-poll-{id}"), u64::from(value.is_ready()));
                value
            });
            let result = if let Some(registration) = registration {
                match futures::future::Abortable::new(observed, registration).await {
                    Ok(value) => {
                        trace.event("cancel-result", 1);
                        value
                    }
                    Err(_) => {
                        trace.event("cancel-result", 0);
                        drop(wait);
                        replacement.await
                    }
                }
            } else {
                observed.await
            };
            let value = fence_result(Poll::Ready(result));
            let output = output.borrow();
            if value == 1 {
                assert_eq!(error_at, 0);
                assert!(output.closed && output.flushed);
                check_bytes(&output);
            }
            if value == 2 {
                assert_ne!(error_at, 0);
                assert!(output.closed);
            }
            if value == 3 {
                assert!(stop);
                assert!(!output.closed);
            }
            trace.event(&format!("fence-observed-{id}"), value);
        }));
    }
    for actor in actors {
        actor.await.unwrap();
    }
    executor.join().await;
    assert!(h.late.await.is_err());
    let marker = h.marker.await;
    if !stop {
        assert!(marker.is_ok());
        assert_eq!(outcome(&observer), 0);
    } else {
        assert_eq!(outcome(&observer), expected.get());
    }
    trace.event("final-cause", outcome(&observer));
}

#[test]
fn shuttle_output_fences_cancel_replace_and_preserve_order() {
    let batch = schedules::check("route-output-success", |trace| {
        block_on(scheduled_output(trace, 0, false))
    });
    if batch.sampled {
        for value in [0, 1] {
            assert!(batch
                .runs
                .iter()
                .any(|run| run.events.contains(&("cancel-result".into(), value))));
        }
        assert!(
            batch.runs.iter().any(|run| {
                run.events.contains(&("cancel-result".into(), 0))
                    && !run.events.iter().any(|(name, _)| name == "fence-poll-0")
            }),
            "no cancellation before initial fence poll"
        );
        assert!(
            batch.runs.iter().any(|run| {
                run.events.contains(&("cancel-result".into(), 0))
                    && run.events.contains(&("fence-poll-0".into(), 0))
            }),
            "no cancellation of a registered fence waiter"
        );
    }
}

#[test]
fn shuttle_output_failures_race_drain_and_owner_stop() {
    let batch = schedules::check("route-output-errors", |trace| {
        block_on(async {
            for error_at in 1..=3 {
                scheduled_output(trace.clone(), error_at, true).await;
            }
        })
    });
    if batch.sampled {
        for mode in 1..=3 {
            for (name, value) in [
                ("stop-result", 1),
                ("stop-result", 2),
                ("fence-observed-1", 2),
                ("fence-observed-1", 3),
                ("stop-after-drained-error", 1),
            ] {
                assert!(
                    batch.runs.iter().any(|run| {
                        run.events
                            .iter()
                            .skip_while(|event| **event != ("mode".into(), mode))
                            .skip(1)
                            .take_while(|(name, _)| name != "mode")
                            .any(|event| *event == (name.into(), value))
                    }),
                    "mode {mode} did not exercise {name}={value}"
                );
            }
        }
    }
}

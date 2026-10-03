use capnp::{capability::Promise, message, Error};
use capnp_rpc::{FlowController, OutgoingMessage};
use futures::{channel::oneshot, executor::LocalPool, task::LocalSpawnExt, FutureExt};
use std::{cell::Cell, rc::Rc, time::Duration};

const KIB: usize = 1024;

#[path = "flow_control/socket_window.rs"]
mod socket_window;

struct Message {
    words: usize,
    sent: Rc<Cell<usize>>,
    body: message::Builder<message::HeapAllocator>,
}
impl OutgoingMessage for Message {
    fn get_body(&mut self) -> capnp::Result<capnp::any_pointer::Builder<'_>> {
        self.body.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.body.get_root_as_reader()
    }
    fn size_in_words(&self) -> usize {
        self.words
    }
    fn take(self: Box<Self>) -> message::Builder<message::HeapAllocator> {
        self.body
    }
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<message::Builder<message::HeapAllocator>>,
    ) {
        self.sent.set(self.sent.get() + 1);
        (Promise::ok(()), Rc::new(self.body))
    }
}

struct Harness {
    controller: Option<Box<dyn FlowController>>,
    pool: LocalPool,
    sent: Rc<Cell<usize>>,
    window: Rc<Cell<usize>>,
    time: Rc<Cell<Duration>>,
    acks: Vec<Option<oneshot::Sender<capnp::Result<()>>>>,
    credit: Vec<Option<Promise<(), Error>>>,
    outcomes: Vec<u64>,
    drain: Option<Promise<(), Error>>,
    drained: bool,
    done: Rc<Cell<bool>>,
    max_words: usize,
}
impl Harness {
    fn new(adaptive: bool, initial: usize) -> Self {
        let window = Rc::new(Cell::new(initial));
        let time = Rc::new(Cell::new(Duration::ZERO));
        let (controller, driver) = if adaptive {
            let time = time.clone();
            capnp_rpc::new_adaptive_flow_controller_with_clock(initial, move || time.get())
        } else {
            let window = window.clone();
            capnp_rpc::new_variable_window_flow_controller(move || window.get())
        };
        Self::with_controller(controller, driver, window, time)
    }

    fn with_controller(
        controller: Box<dyn FlowController>,
        driver: Promise<(), Error>,
        window: Rc<Cell<usize>>,
        time: Rc<Cell<Duration>>,
    ) -> Self {
        let pool = LocalPool::new();
        let done = Rc::new(Cell::new(false));
        let finished = done.clone();
        pool.spawner()
            .spawn_local(async move {
                driver.await.unwrap();
                finished.set(true);
            })
            .unwrap();
        Self {
            controller: Some(controller),
            pool,
            sent: Rc::new(Cell::new(0)),
            window,
            time,
            acks: vec![],
            credit: vec![],
            outcomes: vec![],
            drain: None,
            drained: false,
            done,
            max_words: 0,
        }
    }

    fn poll(&mut self) {
        self.pool.run_until_stalled();
        for (i, credit) in self.credit.iter_mut().enumerate() {
            if let Some(promise) = credit {
                if let Some(result) = promise.now_or_never() {
                    self.outcomes[i] = if result.is_ok() { 1 } else { 3 };
                    *credit = None;
                }
            }
        }
        if let Some(drain) = &mut self.drain {
            if let Some(result) = drain.now_or_never() {
                result.unwrap();
                self.drained = true;
                self.drain = None;
            }
        }
    }

    fn send(&mut self, words: usize) -> usize {
        self.max_words = self.max_words.max(words);
        let index = self.acks.len();
        let (tx, rx) = oneshot::channel();
        let ack = Promise::from_future(async move {
            rx.await
                .map_err(|_| Error::disconnected("ack canceled".into()))?
        });
        let message = Box::new(Message {
            words,
            sent: self.sent.clone(),
            body: message::Builder::new_default(),
        });
        let credit = self.controller.as_mut().unwrap().send(message, ack);
        assert_eq!(
            self.sent.get(),
            index + 1,
            "message must be sent synchronously"
        );
        self.acks.push(Some(tx));
        self.credit.push(Some(credit));
        self.outcomes.push(2);
        self.poll();
        index
    }

    fn ack(&mut self, index: usize, failed: bool) {
        self.acks[index]
            .take()
            .unwrap()
            .send(if failed {
                Err(Error::disconnected("original ack failure".into()))
            } else {
                Ok(())
            })
            .unwrap();
        self.poll();
    }

    fn wait(&mut self) {
        self.drain = Some(self.controller.as_mut().unwrap().wait_all_acked());
        self.poll();
    }

    fn drop_controller(&mut self) {
        self.controller.take();
        self.poll();
    }

    // Observe the estimated window through the public send API, with no test
    // access to estimator internals. Requires all earlier acknowledgements settled.
    // Message sizes are words, so the two probes straddle its 8-byte boundary.
    fn assert_window(&mut self, expected: usize) {
        assert!(self.acks.iter().all(Option::is_none));
        let first = self.send(self.max_words.max(1024 * 1024 * 1024 / 8));
        assert_eq!(self.outcomes[first], if expected == 0 { 2 } else { 1 });
        if expected > 0 {
            let below = self.send((expected - 1) / 8);
            assert_eq!(self.outcomes[below], 1, "below {expected} bytes");
            let edge = self.send(1);
            assert_eq!(self.outcomes[edge], 2, "at {expected} bytes");
        }
    }
}

#[test]
fn variable_window_changes_apply_to_existing_stream_and_wait_for_ack() {
    let mut h = Harness::new(false, 8);
    h.send(1);
    h.send(1);
    h.send(1);
    assert_eq!(h.outcomes, [1, 2, 2]);
    h.window.set(64);
    h.poll();
    assert_eq!(h.outcomes, [1, 2, 2], "changing the getter is not a wakeup");
    h.ack(2, false); // Out-of-order ack observes the larger shared window.
    assert_eq!(h.outcomes, [1, 1, 1]);
    h.window.set(0);
    let last = h.send(1);
    assert_eq!(h.outcomes[last], 2);
    h.ack(0, false);
    assert_eq!(h.outcomes[last], 2);
    h.ack(1, false);
    assert_eq!(
        h.outcomes[last], 1,
        "one largest message may remain at zero window"
    );
    h.wait();
    assert!(!h.drained);
    h.ack(last, false);
    assert!(h.drained);
}

#[test]
fn both_policies_preserve_acknowledgements_errors_and_drop_lifetimes() {
    for adaptive in [false, true] {
        let mut h = Harness::new(adaptive, 8);
        h.wait();
        assert!(h.drained);
        h.drop_controller();
        assert!(h.done.get());

        let mut h = Harness::new(adaptive, 8);
        h.send(1);
        h.send(1);
        h.send(1);
        h.credit[1].take(); // Canceling backpressure never cancels the call.
        h.ack(0, true);
        assert_eq!(h.outcomes[2], 3);
        let next = h.send(1); // Error still sends synchronously and owns the ack.
        assert_eq!(h.outcomes[next], 3);
        h.wait();
        h.drop_controller();
        assert!(!h.drained);
        assert!(!h.done.get());
        h.ack(2, false);
        h.ack(next, true);
        assert!(!h.drained);
        h.ack(1, false);
        assert!(h.drained);
        assert!(h.done.get());

        let mut h = Harness::new(adaptive, 8);
        h.send(1);
        h.send(1);
        h.drop_controller();
        assert_eq!(h.outcomes, [1, 1]);
        assert!(!h.done.get());
        h.ack(0, false);
        h.ack(1, false);
        assert!(h.done.get());
    }
}

#[test]
fn adaptive_handles_first_ack_zero_intervals_and_extreme_message_sizes() {
    let mut h = Harness::new(true, 256 * KIB);
    for _ in 0..5 {
        h.send(64 * KIB / 8);
    }
    assert_eq!(h.outcomes, [1, 1, 1, 1, 2]);
    for i in 0..5 {
        h.ack(i, false);
    } // Zero intervals cannot divide by zero or tune.
    h.assert_window(256 * KIB);

    let mut h = Harness::new(true, 0);
    h.send(1);
    assert_eq!(h.outcomes, [2]);
    h.ack(0, false);
    assert_eq!(h.outcomes, [1]);
    h.assert_window(0);

    for adaptive in [false, true] {
        let mut h = Harness::new(adaptive, usize::MAX);
        h.send(usize::MAX);
        h.send(usize::MAX);
        assert_eq!(h.outcomes, [1, 2], "word/byte accounting must not wrap");
        h.ack(1, false);
        h.ack(0, false);
        h.wait();
        assert!(h.drained);
    }
}

#[test]
fn two_party_policy_selection_survives_connection_handoff() {
    use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork};
    let mut net = capnp_rpc::twoparty::VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        futures::io::sink(),
        Side::Client,
        Default::default(),
    );
    let mut conn = net.connect(Side::Server).unwrap();
    net.set_window_size(0); // Previously silently ignored after connect().
    let (controller, driver) = conn.new_stream();
    let mut h = Harness::with_controller(
        controller,
        driver,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(Duration::ZERO)),
    );
    h.send(1);
    h.send(1);
    assert_eq!(h.outcomes, [1, 2]);

    let window = Rc::new(Cell::new(0));
    let source = window.clone();
    net.set_variable_window(move || source.get());
    let (controller, driver) = conn.new_stream();
    let mut variable = Harness::with_controller(
        controller,
        driver,
        window.clone(),
        Rc::new(Cell::new(Duration::ZERO)),
    );
    window.set(64);
    variable.send(1);
    variable.send(1);
    assert_eq!(variable.outcomes, [1, 1]);
    h.send(1);
    assert_eq!(h.outcomes[2], 2, "existing fixed stream keeps its policy");

    net.set_adaptive_window(8);
    let (controller, driver) = conn.new_stream();
    let mut adaptive = Harness::with_controller(
        controller,
        driver,
        Rc::new(Cell::new(8)),
        Rc::new(Cell::new(Duration::ZERO)),
    );
    adaptive.send(1);
    adaptive.send(1);
    assert_eq!(adaptive.outcomes, [1, 2]);
}

#[test]
fn adaptive_estimates_clamp_to_minimum_and_maximum() {
    for (initial, words, expected) in [
        (0, 1, 64 * KIB),
        (
            2 * 1024 * 1024 * KIB,
            2 * 1024 * 1024 * KIB / 8,
            1024 * 1024 * KIB,
        ),
    ] {
        let mut h = Harness::new(true, initial);
        h.send(words);
        h.send(words);
        h.time.set(Duration::from_micros(1));
        h.ack(0, false);
        h.time.set(Duration::from_micros(2));
        h.ack(1, false);
        h.assert_window(expected);
    }
}

#[test]
fn tlc_variable_window_model_replays_credit_failure_cancellation_and_drain() {
    use capntproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcVariableWindow.tla";
    const CFG: &str = include_str!("../verification/RpcVariableWindow.cfg");
    let live =
        CFG.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY DrainProgress\n";
    controls(
        MODEL,
        "variable-window",
        CFG,
        &[
            ("staleWindow", "VariableReadiness"),
            ("cancelAck", "AckOwnership"),
            ("dropAck", "AckOwnership"),
        ],
        Some(&live),
    )
    .unwrap();
    let mut coverage = std::collections::BTreeSet::new();
    for trace in traces(MODEL, "variable-window", CFG).unwrap() {
        let mut h = Harness::new(false, 8);
        for state in trace {
            match state["event"] {
                1 => {
                    h.send(state["sent"] as usize);
                }
                2 => h.ack(0, false),
                3 => h.ack(0, true),
                4 => h.ack(1, false),
                5 => h.ack(1, true),
                6 => {
                    h.window.set(state["window"] as usize * 8);
                    h.poll();
                }
                7 => {
                    h.credit[1].take();
                    h.outcomes[1] = 4;
                    h.poll();
                }
                8 => h.wait(),
                9 => h.drop_controller(),
                _ => panic!("{state:?}"),
            }
            assert_eq!(h.sent.get() as u64, state["sent"]);
            assert_eq!(*h.outcomes.first().unwrap_or(&0), state["p1"], "{state:?}");
            assert_eq!(*h.outcomes.get(1).unwrap_or(&0), state["p2"], "{state:?}");
            assert_eq!(u64::from(h.drained), state["drained"], "{state:?}");
            assert_eq!(
                h.done.get(),
                state["dropped"] == 1 && h.acks.iter().all(Option::is_none)
            );
            coverage.insert(state["event"]);
        }
    }
    assert_eq!(coverage, (1..=9).collect());
}

#[test]
fn tlc_adaptive_model_replays_timing_reordering_collar_and_startup_transitions() {
    use capntproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcAdaptiveWindow.tla";
    const CFG: &str = include_str!("../verification/RpcAdaptiveWindow.cfg");
    let mut saw_steady = false;
    let mut saw_growth = false;
    let mut saw_decay = false;
    for scenario in 0..3 {
        let config = CFG
            .replace("Scenario = 0", &format!("Scenario = {scenario}"))
            .replace(
                "MaxRounds = 2",
                if scenario == 1 {
                    "MaxRounds = 6"
                } else {
                    "MaxRounds = 2"
                },
            );
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
            + "\nPROPERTY Progress\n";
        let faults: &[(&str, &str)] = match scenario {
            0 => &[
                ("unboundedGrowth", "GrowthCollar"),
                ("skipDecay", "DecayCollar"),
                ("dropCredit", "ReadyCredit"),
            ],
            1 => &[
                ("shrinkLimited", "AppLimited"),
                ("neverExit", "StartupExit"),
                ("steadyGrowth", "GrowthCollar"),
            ],
            _ => &[],
        };
        let report = format!("adaptive-window-{scenario}");
        controls(MODEL, &report, &config, faults, Some(&live)).unwrap();
        for trace in traces(MODEL, &report, &config).unwrap() {
            let mut h = Harness::new(true, if scenario == 2 { 0 } else { 256 * KIB });
            let mut batch = 0;
            for state in &trace {
                h.time.set(Duration::from_micros(state["now"]));
                match state["event"] {
                    1 => {
                        batch = h.acks.len();
                        let bytes = if scenario == 1 && state["rounds"] < 5 {
                            8
                        } else {
                            512 * KIB
                        };
                        h.send(bytes / 8);
                        h.send(bytes / 8);
                    }
                    2 => h.ack(batch + state["whichAck"] as usize - 1, false),
                    _ => panic!("{state:?}"),
                }
                assert_eq!(
                    &h.outcomes[batch..],
                    &[state["p1"], state["p2"]],
                    "{state:?}"
                );
                saw_steady |= state["startup"] == 0;
                saw_growth |= state["estimated"] == 1 && state["window"] > state["oldWindow"];
                saw_decay |= state["estimated"] == 1 && state["window"] < state["oldWindow"];
            }
            let state = trace.last().unwrap();
            if state["pending"] == 0 {
                h.assert_window(state["window"] as usize);
            }
        }
    }
    assert!(saw_steady && saw_growth && saw_decay);
}

#[test]
fn timed_variable_and_adaptive_backpressure_matches_pinned_cpp() {
    use capntproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("flow-control");
    let logs = root().join("target/verification/flow-control-cpp");
    let mut compile = command("g++");
    compile
        .args([
            "-std=c++23",
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/flow-control.c++",
        ])
        .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
        .arg(build.join("c++/src/capnp/libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(&executable);
    run(&mut compile, &logs.join("compile.log"), 0).unwrap();

    let mut script = String::new();
    for kind in 0..2 {
        for initial in [0, 64 * KIB, 256 * KIB] {
            script += &format!("new {kind} {initial}\n");
            let mut sent = 0;
            let mut time = 0;
            for round in 0..20 {
                if kind == 0 {
                    script += &format!("window {}\n", [0, 8, 256 * KIB][round % 3]);
                }
                time += 1;
                script += &format!("time {time}\n");
                let start = sent;
                let count = if round < 6 { 2 } else { 4 };
                for i in 0..count {
                    let words = if round < 6 {
                        1
                    } else {
                        [64 * KIB, 512 * KIB, 8, 128 * KIB][i] / 8
                    };
                    script += &format!("send {words}\n");
                    sent += 1;
                }
                let mut order: Vec<_> = (start..sent).collect();
                if round % 2 == 1 {
                    order.reverse();
                }
                for (position, index) in order.into_iter().enumerate() {
                    time += [0, 1, 13, 20][(round + position) % 4];
                    script += &format!(
                        "time {time}\nack {index} {}\n",
                        u8::from(round == 18 && position == 0)
                    );
                }
            }
        }
    }
    let input = directory.path().join("operations.txt");
    std::fs::write(&input, &script).unwrap();
    let cpp = run(
        command(&executable).arg(&input),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    let mut h = Harness::new(false, 0);
    let mut rust = String::new();
    for line in script.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        let a: usize = parts[1].parse().unwrap();
        match parts[0] {
            "new" => h = Harness::new(a == 1, parts[2].parse().unwrap()),
            "time" => h.time.set(Duration::from_micros(a as u64)),
            "window" => h.window.set(a),
            "send" => {
                h.send(a);
            }
            "ack" => h.ack(a, parts[2] == "1"),
            _ => unreachable!(),
        }
        h.poll();
        rust += &format!("{}:", h.sent.get());
        for outcome in &h.outcomes {
            rust += &outcome.to_string();
        }
        rust.push('\n');
    }
    std::fs::write(logs.join("operations.txt"), &script).unwrap();
    std::fs::write(logs.join("rust.log"), &rust).unwrap();
    assert_eq!(cpp.lines().count(), script.lines().count());
    for (index, (actual, expected)) in rust.lines().zip(cpp.lines()).enumerate() {
        assert_eq!(
            actual,
            expected,
            "operation {index}: {}",
            script.lines().nth(index).unwrap()
        );
    }
    eprintln!(
        "{} timed controller observations match pinned C++",
        script.lines().count()
    );
}

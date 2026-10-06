#[allow(dead_code)]
mod support;
use capnp::{capability::Client, private::capability::ResultsHook, traits::HasTypeId, Error};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, return_},
    Connection, RpcSystem,
};
use capntproto_test_support::{
    cancellation_policy_capnp::{allowed, derived, policy},
    runtime_test_capnp::harness,
};
use futures::{channel::oneshot, FutureExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use support::{Endpoint, Hub};
#[derive(Default)]
struct State {
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    started: Cell<bool>,
    completed: Cell<bool>,
    running: Cell<bool>,
    alive: Cell<bool>,
    fail: bool,
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}
struct Value(Rc<State>);
impl Drop for Value {
    fn drop(&mut self) {
        self.0.alive.set(false);
    }
}
impl harness::Server for Value {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(42);
        Ok(())
    }
}
struct Service(Rc<State>);
impl Service {
    async fn work(
        self: Rc<Self>,
        early: bool,
        mut results: Box<dyn ResultsHook>,
    ) -> capnp::Result<()> {
        let _running = Running(self.0.clone());
        self.0.running.set(true);
        self.0.started.set(true);
        self.0.alive.set(true);
        results
            .get()?
            .init_as::<harness::bounce_results::Builder>()
            .set_cap(capnp_rpc::new_client(Value(self.0.clone())));
        let results = if early {
            drop(results);
            None
        } else {
            Some(results)
        };
        let gate = self.0.gate.borrow_mut().take().unwrap();
        gate.await
            .map_err(|_| Error::failed("gate canceled".into()))?;
        self.0.completed.set(true);
        drop(results);
        if self.0.fail {
            Err(Error::failed("application failure".into()))
        } else {
            Ok(())
        }
    }
}
impl policy::Server for Service {
    async fn pending(
        self: Rc<Self>,
        p: policy::PendingParams,
        r: policy::PendingResults,
    ) -> capnp::Result<()> {
        let early = p.get()?.get_early_drop();
        self.work(early, r.hook).await
    }
    async fn cancellable(
        self: Rc<Self>,
        p: policy::CancellableParams,
        r: policy::CancellableResults,
    ) -> capnp::Result<()> {
        let early = p.get()?.get_early_drop();
        self.work(early, r.hook).await
    }
}
impl allowed::Server for Service {
    async fn pending(
        self: Rc<Self>,
        p: allowed::PendingParams,
        r: allowed::PendingResults,
    ) -> capnp::Result<()> {
        let early = p.get()?.get_early_drop();
        self.work(early, r.hook).await
    }
}
impl derived::Server for Service {
    async fn pending_own(
        self: Rc<Self>,
        p: derived::PendingOwnParams,
        r: derived::PendingOwnResults,
    ) -> capnp::Result<()> {
        let early = p.get()?.get_early_drop();
        self.work(early, r.hook).await
    }
}
impl harness::Server for Service {
    async fn pending(
        self: Rc<Self>,
        p: harness::PendingParams,
        r: harness::PendingResults,
    ) -> capnp::Result<()> {
        let early = p.get()?.get_hold_context();
        self.work(early, r.hook).await
    }
}
async fn settle() {
    for _ in 0..48 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    _hub: Rc<RefCell<Hub>>,
    _network: support::Network,
    peer: Endpoint,
    driver: tokio::task::JoinHandle<capnp::Result<()>>,
    state: Rc<State>,
    gate: Option<oneshot::Sender<()>>,
    returns: u32,
    kind: u32,
    connected: bool,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.driver.abort();
    }
}
impl Fixture {
    async fn new(mode: u32, early: bool, fail: bool, membrane: bool) -> Self {
        Self::configured(mode, early, fail, membrane, false, false).await
    }

    async fn configured(
        mode: u32,
        early: bool,
        fail: bool,
        membrane: bool,
        no_pipeline: bool,
        ready: bool,
    ) -> Self {
        let (tx, rx) = oneshot::channel();
        let mut tx = Some(tx);
        if ready {
            tx.take().unwrap().send(()).unwrap();
        }
        let state = Rc::new(State {
            fail,
            ..Default::default()
        });
        *state.gate.borrow_mut() = Some(rx);
        let (cap, interface, method): (Client, u64, u16) = match mode {
            0 | 1 => {
                let c: policy::Client = capnp_rpc::new_client(Service(state.clone()));
                (c.client, policy::Client::TYPE_ID, mode as u16)
            }
            2 => {
                let c: allowed::Client = capnp_rpc::new_client(Service(state.clone()));
                (c.client, allowed::Client::TYPE_ID, 0)
            }
            3 | 4 => {
                let c: derived::Client = capnp_rpc::new_client(Service(state.clone()));
                (
                    c.client,
                    if mode == 3 {
                        policy::Client::TYPE_ID
                    } else {
                        derived::Client::TYPE_ID
                    },
                    0,
                )
            }
            5 => {
                let c: harness::Client = capnp_rpc::new_client(Service(state.clone()));
                (c.client, harness::Client::TYPE_ID, 3)
            }
            _ => panic!(),
        };
        let cap = if membrane {
            struct Permit;
            impl capnp_rpc::membrane::Policy for Permit {
                fn call(
                    &self,
                    _: capnp_rpc::membrane::Direction,
                    _: u64,
                    _: u16,
                    _: &Client,
                ) -> capnp::Result<Option<Client>> {
                    Ok(None)
                }
            }
            capnp_rpc::membrane::Membrane::new(Rc::new(Permit)).export(cap)
        } else {
            cap
        };
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 2);
        let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(cap));
        let mut peer = hub.borrow_mut().connect(2, 1);
        let driver = tokio::task::spawn_local(system);
        peer.send(|m| m.init_bootstrap().set_question_id(0));
        let input = peer.recv().await;
        let message::Return(r) = input
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let return_::Results(p) = r.unwrap().which().unwrap() else {
            panic!()
        };
        let cap_descriptor::SenderHosted(id) =
            p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
        else {
            panic!()
        };
        peer.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(1);
            c.set_interface_id(interface);
            c.set_method_id(method);
            c.set_no_promise_pipelining(no_pipeline);
            c.reborrow().init_target().set_imported_cap(id);
            c.init_params()
                .get_content()
                .init_as::<policy::pending_params::Builder>()
                .set_early_drop(early);
        });
        settle().await;
        assert!(state.started.get());
        Self {
            _hub: hub,
            _network: network,
            peer,
            driver,
            state,
            gate: tx,
            returns: 0,
            kind: 0,
            connected: true,
        }
    }
    fn drain(&mut self) {
        while let Some(input) = self.peer.receive_incoming_message().now_or_never() {
            let Some(input) = input.unwrap() else { break };
            match input
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(r) => {
                    let r = r.unwrap();
                    assert_eq!(r.get_answer_id(), 1);
                    self.returns += 1;
                    self.kind = match r.which().unwrap() {
                        return_::Results(_) => 1,
                        return_::Canceled(()) => 2,
                        return_::Exception(_) => 3,
                        _ => panic!(),
                    };
                }
                message::Release(_) | message::Abort(_) => (),
                _ => panic!("unexpected protocol message"),
            }
        }
    }
    async fn step(&mut self, action: &str) {
        match action {
            "finish" => self.peer.send(|m| {
                let mut f = m.init_finish();
                f.set_question_id(1);
                f.set_release_result_caps(true);
                f.set_require_early_cancellation_workaround(false);
            }),
            "complete" => {
                assert!(self.gate.take().unwrap().send(()).is_ok());
            }
            "disconnect" => {
                self.peer.send(|m| m.init_abort().set_reason("disconnect"));
                self.connected = false;
            }
            "use" => {
                self.peer.send(|m| {
                    let mut c = m.init_call();
                    c.set_question_id(2);
                    c.set_interface_id(harness::Client::TYPE_ID);
                    c.set_method_id(0);
                    let mut a = c.reborrow().init_target().init_promised_answer();
                    a.set_question_id(1);
                    a.init_transform(1).get(0).set_get_pointer_field(0);
                    c.init_params();
                });
                let input = self.peer.recv().await;
                let message::Return(r) = input
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!()
                };
                let r = r.unwrap();
                assert_eq!(r.get_answer_id(), 2);
                let return_::Results(p) = r.which().unwrap() else {
                    panic!()
                };
                assert_eq!(
                    p.unwrap()
                        .get_content()
                        .get_as::<harness::value::Reader>()
                        .unwrap()
                        .get_value(),
                    42
                );
            }
            "reuse" => {
                self.peer.send(|m| m.init_bootstrap().set_question_id(1));
                let input = self.peer.recv().await;
                let message::Return(r) = input
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!()
                };
                assert!(matches!(r.unwrap().which().unwrap(), return_::Results(_)));
            }
            _ => panic!(),
        }
        settle().await;
        self.drain();
    }
    fn check(&self, values: &[serde_json::Value]) {
        assert_eq!(
            self.state.completed.get(),
            values[2] == 1,
            "method completion"
        );
        assert_eq!(
            self.state.running.get(),
            values[3] == 1,
            "application lifetime"
        );
        assert_eq!(
            self.state.alive.get(),
            values[4] == 1,
            "result capability lifetime"
        );
        assert_eq!(
            self.returns as u64,
            values[5].as_u64().unwrap(),
            "wire Return count"
        );
        assert_eq!(
            self.kind as u64,
            values[6].as_u64().unwrap(),
            "wire Return kind"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn protected_calls_survive_finish_and_disconnect_with_early_results_release() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                for mode in 0..6 {
                    for membrane in [false, true] {
                        for disconnect in [false, true] {
                            let protected = mode == 0 || mode == 3;
                            let mut f = Fixture::new(mode, true, false, membrane).await;
                            f.step(if disconnect { "disconnect" } else { "finish" })
                                .await;
                            assert_eq!(f.state.running.get(), protected, "mode {mode}");
                            assert_eq!(f.state.alive.get(), protected, "mode {mode}");
                            if protected {
                                assert_eq!(f.returns, 0);
                                f.step("complete").await;
                                assert!(f.state.completed.get());
                            }
                            assert!(!f.state.alive.get());
                            assert!(!f.state.running.get());
                            assert_eq!(f.returns, u32::from(!disconnect));
                        }
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn immediate_rpc_poll_preserves_pending_protection_and_reports_ready_errors() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for fail in [false, true] {
                for early in [false, true] {
                    for ready in [false, true] {
                        let mut f = Fixture::configured(0, early, fail, false, true, ready).await;
                        if !ready {
                            f.step("finish").await;
                            assert!(f.state.running.get());
                            assert!(f.state.alive.get());
                            assert_eq!(f.returns, 0);
                            f.step("complete").await;
                        } else {
                            f.drain();
                            assert_eq!(f.kind, if fail { 3 } else { 1 });
                            f.step("finish").await;
                        }
                        assert!(f.state.completed.get());
                        assert!(!f.state.running.get());
                        assert!(!f.state.alive.get());
                        assert_eq!(f.returns, 1);
                    }
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn local_executor_owns_protected_calls_after_caller_drop() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for early in [false, true] {
                for constructor in 0..if cfg!(unix) { 5 } else { 4 } {
                    let (tx, rx) = oneshot::channel();
                    let state = Rc::new(State::default());
                    *state.gate.borrow_mut() = Some(rx);
                    let (executor, driver) = capnp_rpc::new_call_executor();
                    let driver = tokio::task::spawn_local(driver);
                    let mut servers =
                        capnp_rpc::CapabilityServerSet::<Service, policy::Client>::new();
                    let client: policy::Client = match constructor {
                        0 => capnp_rpc::new_client_with_executor(Service(state.clone()), executor),
                        1 => capnp_rpc::new_client_from_rc_with_executor(
                            Rc::new(Service(state.clone())),
                            executor,
                        ),
                        2 => servers.new_client_with_executor(Service(state.clone()), executor),
                        3 => servers.new_client_from_rc_with_executor(
                            Rc::new(Service(state.clone())),
                            executor,
                        ),
                        4 => {
                            #[cfg(unix)]
                            {
                                let client: policy::Client = capnp_rpc::new_fd_client_with_executor(
                                    Service(state.clone()),
                                    std::fs::File::open("/dev/null").unwrap().into(),
                                    executor,
                                );
                                assert!(client.client.get_fd().await.unwrap().is_some());
                                client
                            }
                            #[cfg(not(unix))]
                            {
                                unreachable!()
                            }
                        }
                        _ => unreachable!(),
                    };
                    if constructor == 2 || constructor == 3 {
                        let server = servers.get_local_server(&client).await.unwrap();
                        assert!(Rc::ptr_eq(&server.0, &state));
                    }
                    let mut request = client.pending_request();
                    request.get().set_early_drop(early);
                    let mut promise = Box::pin(request.send().promise);
                    assert!(
                        !state.started.get(),
                        "ordinary local dispatch remains deferred"
                    );
                    assert!(futures::poll!(&mut promise).is_pending());
                    settle().await;
                    assert!(state.started.get());
                    drop(promise);
                    drop(client);
                    settle().await;
                    assert!(state.running.get());
                    assert!(state.alive.get());
                    tx.send(()).unwrap();
                    settle().await;
                    assert!(state.completed.get());
                    assert!(!state.running.get());
                    assert!(!state.alive.get());
                    driver.abort();
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn missing_or_stopped_local_executor_fails_before_application_dispatch() {
    for stopped in [false, true] {
        let state = Rc::new(State::default());
        let client: policy::Client = if stopped {
            let (executor, driver) = capnp_rpc::new_call_executor();
            drop(driver);
            capnp_rpc::new_client_with_executor(Service(state.clone()), executor)
        } else {
            capnp_rpc::new_client(Service(state.clone()))
        };
        let error = client.pending_request().send().promise.await.err().unwrap();
        assert!(error.extra.contains("executor"));
        assert!(!state.started.get());
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_static_cancellation_traces() {
    let path =
        capntproto_test_support::verification::input("CAPNTPROTO_STATIC_CANCELLATION_TRACES")
            .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                let allow = case["allow"].as_bool().unwrap();
                for mode in if allow { vec![1, 2, 4, 5] } else { vec![0, 3] } {
                    for membrane in [false, true] {
                        let mut f = Fixture::new(
                            mode,
                            case["early"].as_bool().unwrap(),
                            case["fail"].as_bool().unwrap(),
                            membrane,
                        )
                        .await;
                        for step in case["steps"].as_array().unwrap() {
                            f.step(step["action"].as_str().unwrap()).await;
                            f.check(step["state"].as_array().unwrap());
                        }
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn executor_shutdown_releases_abandoned_protected_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for early in [false, true] {
                let (tx, rx) = oneshot::channel();
                let state = Rc::new(State::default());
                *state.gate.borrow_mut() = Some(rx);
                let (executor, driver) = capnp_rpc::new_call_executor();
                let driver = tokio::task::spawn_local(driver);
                let client: policy::Client =
                    capnp_rpc::new_client_with_executor(Service(state.clone()), executor);
                let mut request = client.pending_request();
                request.get().set_early_drop(early);
                let mut promise = Box::pin(request.send().promise);
                assert!(futures::poll!(&mut promise).is_pending());
                settle().await;
                drop(promise);
                drop(client);
                assert!(state.running.get());
                assert!(state.alive.get());
                driver.abort();
                assert!(driver.await.unwrap_err().is_cancelled());
                assert!(!state.running.get());
                assert!(!state.alive.get());
                assert!(!state.completed.get());
                assert!(tx.send(()).is_err());
            }
        })
        .await;
}

async fn executor_trace(steps: &[serde_json::Value], early: bool, fail: bool) {
    let (tx, rx) = oneshot::channel();
    let state = Rc::new(State {
        fail,
        ..Default::default()
    });
    *state.gate.borrow_mut() = Some(rx);
    let mut gate = Some(tx);
    let (executor, driver) = capnp_rpc::new_call_executor();
    let mut driver = Some(tokio::task::spawn_local(driver));
    let mut client: Option<policy::Client> = Some(capnp_rpc::new_client_with_executor(
        Service(state.clone()),
        executor.clone(),
    ));
    let mut request = client.as_ref().unwrap().pending_request();
    request.get().set_early_drop(early);
    let mut promise = Some(Box::pin(request.send().promise));
    let mut response = None;
    assert!(futures::poll!(promise.as_mut().unwrap()).is_pending());
    settle().await;
    assert!(state.running.get());
    assert!(state.alive.get());
    for step in steps {
        match step["action"].as_str().unwrap() {
            "drop" => {
                promise.take();
                response.take();
                client.take();
            }
            "stop" => {
                let task = driver.take().unwrap();
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            }
            "complete" => {
                gate.take().unwrap().send(()).unwrap();
            }
            "probe" => {
                let probe = Rc::new(State::default());
                let c: policy::Client =
                    capnp_rpc::new_client_with_executor(Service(probe.clone()), executor.clone());
                let e = c.pending_request().send().promise.await.err().unwrap();
                assert_eq!(e.kind, capnp::ErrorKind::Disconnected);
                assert!(!probe.started.get());
            }
            _ => panic!(),
        }
        for _ in 0..32 {
            if let Some(p) = promise.as_mut() {
                if let std::task::Poll::Ready(result) = futures::poll!(p) {
                    assert_eq!(result.is_err(), fail);
                    response = result.ok();
                    promise.take();
                }
            }
            tokio::task::yield_now().await;
        }
        let expected = step["state"].as_array().unwrap();
        assert_eq!(client.is_some(), expected[0] == 1, "caller ownership");
        assert_eq!(driver.is_some(), expected[1] == 1, "executor ownership");
        assert_eq!(state.completed.get(), expected[2] == 1, "method completion");
        assert_eq!(state.running.get(), expected[3] == 1, "method lifetime");
        assert_eq!(state.alive.get(), expected[4] == 1, "capability lifetime");
    }
    if let Some(driver) = driver {
        driver.abort();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_call_executor_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_CALL_EXECUTOR_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for early in [false, true] {
                    executor_trace(
                        case["steps"].as_array().unwrap(),
                        early,
                        case["fail"].as_bool().unwrap(),
                    )
                    .await;
                }
            }
        })
        .await;
}

#[allow(dead_code)]
mod support;
use capnp::{
    capability::{Client, FromClientHook, Promise, SelfCapability, ServerHooks},
    traits::HasTypeId,
    Error,
};
use capnp_rpc::{RevocableServer, RpcSystem};
use futures::channel::oneshot;
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

#[derive(Default)]
struct State {
    self_cap: SelfCapability,
    shorten: RefCell<Option<Promise<Client, Error>>>,
    hooks: Cell<u32>,
    streams: Cell<u32>,
    active: Cell<u32>,
    echoes: Cell<u32>,
    alive: Cell<bool>,
    gates: RefCell<VecDeque<oneshot::Receiver<()>>>,
    fail: bool,
}
struct Service(Rc<State>);
impl Drop for Service {
    fn drop(&mut self) {
        self.0.alive.set(false);
    }
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
    }
}
impl ServerHooks for Service {
    fn self_cap(&self) -> Option<&SelfCapability> {
        Some(&self.0.self_cap)
    }
    fn shorten_path(&self) -> Option<Promise<Client, Error>> {
        self.0.hooks.set(self.0.hooks.get() + 1);
        self.0.shorten.borrow_mut().take()
    }
}
impl harness::Server for Service {
    fn _capnp_server_hooks(&self) -> Option<&dyn ServerHooks> {
        Some(self)
    }
    async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
        self.0.streams.set(self.0.streams.get() + 1);
        self.0.active.set(self.0.active.get() + 1);
        let _running = Running(self.0.clone());
        let gate = self.0.gates.borrow_mut().pop_front().unwrap();
        gate.await
            .map_err(|_| Error::failed("gate dropped".into()))?;
        if self.0.fail {
            Err(Error::failed("stream failed".into()))
        } else {
            Ok(())
        }
    }
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.0.echoes.set(self.0.echoes.get() + 1);
        results.get().set_value(1);
        Ok(())
    }
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        results
            .get()
            .set_cap(self.0.self_cap.get::<harness::Client>().unwrap());
        Ok(())
    }
}
impl reproto_test_support::cancellation_policy_capnp::policy::Server for Service {}

struct Target(Rc<Cell<u32>>);
impl harness::Server for Target {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.0.set(self.0.get() + 1);
        results.get().set_value(9);
        Ok(())
    }
}
struct Drivers(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Drivers {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}
async fn settle() {
    for _ in 0..48 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn weak_self_capability_and_canonical_client_lifetimes() {
    let state = Rc::new(State::default());
    assert!(state.self_cap.get::<harness::Client>().is_none());
    state.alive.set(true);
    let server = Rc::new(Service(state.clone()));
    let a: harness::Client = capnp_rpc::new_client_from_rc(server.clone());
    let b: harness::Client = capnp_rpc::new_client_from_rc(server.clone());
    assert_eq!(state.hooks.get(), 1);
    assert_eq!(a.as_client_hook().get_ptr(), b.as_client_hook().get_ptr());
    let c: harness::Client = state.self_cap.get().unwrap();
    assert_eq!(a.as_client_hook().get_ptr(), c.as_client_hook().get_ptr());
    let returned = a
        .pending_request()
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_cap()
        .unwrap();
    assert_eq!(
        returned.as_client_hook().get_ptr(),
        a.as_client_hook().get_ptr()
    );
    drop((a, b, c, returned));
    assert!(state.self_cap.get::<harness::Client>().is_none());
    assert!(state.alive.get());
    let owner = RevocableServer::<harness::Client>::from_rc(server.clone());
    assert!(state.self_cap.get::<harness::Client>().is_some());
    owner.revoke();
    assert!(state.self_cap.get::<harness::Client>().is_none());
    let replacement: harness::Client = capnp_rpc::new_client_from_rc(server.clone());
    drop(owner);
    assert!(state.self_cap.get::<harness::Client>().is_some());
    drop((replacement, server));
    assert!(!state.alive.get());
    assert!(state.self_cap.get::<harness::Client>().is_none());
}

async fn trace(steps: &[serde_json::Value], fail: bool, reject: bool, wire: bool) {
    let state = Rc::new(State {
        fail,
        ..Default::default()
    });
    state.alive.set(true);
    let (resolve, resolution) = oneshot::channel();
    let mut resolve = Some(resolve);
    *state.shorten.borrow_mut() = Some(Promise::from_future(async move {
        resolution
            .await
            .map_err(|_| Error::failed("resolver dropped".into()))?
    }));
    let mut gates = vec![];
    for _ in 0..2 {
        let (tx, rx) = oneshot::channel();
        gates.push(Some(tx));
        state.gates.borrow_mut().push_back(rx);
    }
    let (executor, driver) = capnp_rpc::new_call_executor();
    let mut drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
    let owner =
        RevocableServer::<harness::Client>::new_with_executor(Service(state.clone()), executor);
    let local = owner.get_client();
    let client = if wire {
        let hub = Rc::new(RefCell::new(support::Hub::default()));
        let server = RpcSystem::new(
            Box::new(support::Hub::network(&hub, 1)),
            Some(local.client.clone()),
        );
        let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
        let client = caller.bootstrap(1);
        drivers.0.push(tokio::task::spawn_local(server));
        drivers.0.push(tokio::task::spawn_local(caller));
        client
    } else {
        local.clone()
    };
    let target_calls = Rc::new(Cell::new(0));
    let target: harness::Client = capnp_rpc::new_client(Target(target_calls.clone()));
    let mut streams = [
        Some(
            client
                .client
                .new_call::<capnp::any_pointer::Owned, capnp::any_pointer::Owned>(
                    harness::Client::TYPE_ID,
                    13,
                    None,
                )
                .send()
                .promise,
        ),
        None,
    ];
    assert!(futures::poll!(streams[0].as_mut().unwrap()).is_pending());
    settle().await;
    streams[1] = Some(
        client
            .client
            .new_call::<capnp::any_pointer::Owned, capnp::any_pointer::Owned>(
                harness::Client::TYPE_ID,
                13,
                None,
            )
            .send()
            .promise,
    );
    assert!(futures::poll!(streams[1].as_mut().unwrap()).is_pending());
    settle().await;
    assert_eq!(state.streams.get(), 1);
    let mut echo = None;
    let mut outcomes = [0u64; 3];
    let mut ready = Some(client.client.when_resolved());
    let mut resolved = 0u64;
    for step in steps {
        let action = step["action"].as_str().unwrap();
        match action {
            "shorten" => {
                resolve
                    .take()
                    .unwrap()
                    .send(if reject {
                        Err(Error::failed("shortening failed".into()))
                    } else {
                        Ok(target.client.clone())
                    })
                    .ok()
                    .unwrap();
            }
            "revoke" => {
                owner.revoke();
                assert!(!state.alive.get());
                assert_eq!(state.active.get(), 0);
            }
            "send" => {
                echo = Some(client.echo_request().send().promise);
            }
            "complete-a" => {
                gates[0].take().unwrap().send(()).unwrap();
            }
            "complete-b" => {
                gates[1].take().unwrap().send(()).unwrap();
            }
            "drop-a" => {
                streams[0].take();
            }
            "drop-b" => {
                streams[1].take();
            }
            "drop-c" => {
                echo.take();
            }
            _ => panic!("unknown action {action}"),
        }
        for _ in 0..64 {
            for (i, call) in streams.iter_mut().enumerate() {
                if let Some(p) = call {
                    if let std::task::Poll::Ready(r) = futures::poll!(p) {
                        outcomes[i] = if r.is_ok() { 3 } else { 5 };
                        call.take();
                    }
                }
            }
            if let Some(p) = &mut echo {
                if let std::task::Poll::Ready(r) = futures::poll!(p) {
                    outcomes[2] = match r {
                        Ok(r) => match r.get().unwrap().get_value() {
                            1 => 3,
                            9 => 7,
                            v => panic!("unexpected value {v}"),
                        },
                        Err(_) => 5,
                    };
                    echo.take();
                }
            }
            if let Some(p) = &mut ready {
                if let std::task::Poll::Ready(r) = futures::poll!(p) {
                    resolved = if r.is_ok() { 1 } else { 2 };
                    ready.take();
                }
            }
            tokio::task::yield_now().await;
        }
        let expected = step["state"].as_array().unwrap();
        let e: Vec<u64> = expected.iter().map(|v| v.as_u64().unwrap()).collect();
        assert_eq!(
            state.active.get(),
            u32::from(e[0] == 2 || e[1] == 2),
            "active: {step}, wire={wire}, reject={reject}, fail={fail}"
        );
        assert_eq!(
            state.streams.get(),
            1 + e[5] as u32,
            "old stream dispatch: {step}"
        );
        assert_eq!(state.alive.get(), e[4] == 0, "lifetime: {step}");
        assert_eq!(
            state.self_cap.get::<harness::Client>().is_some(),
            e[4] == 0,
            "weak self: {step}"
        );
        assert_eq!(
            state.echoes.get(),
            u32::from(e[2] == 3),
            "old echo dispatch: {step}"
        );
        assert_eq!(
            target_calls.get(),
            u32::from(e[2] == 7),
            "target dispatch: {step}"
        );
        for i in 0..3 {
            let expected = if [3, 5, 7].contains(&e[i]) { e[i] } else { 0 };
            assert_eq!(outcomes[i], expected, "outcome {i}: {step}, wire={wire}");
        }
        assert_eq!(resolved, e[6], "resolution: {step}, wire={wire}");
    }
}
#[tokio::test(flavor = "current_thread")]
async fn shortening_waits_for_old_streams_and_survives_later_revocation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let steps = serde_json::json!([
                {"action":"shorten","state":[2,1,0,1,0,0,0]},
                {"action":"send","state":[2,1,6,1,0,0,0]},
                {"action":"complete-a","state":[3,2,6,1,0,1,0]},
                {"action":"revoke","state":[3,5,7,1,1,1,1]}
            ]);
            for wire in [false, true] {
                trace(steps.as_array().unwrap(), false, false, wire).await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_server_hooks_traces() {
    let path = reproto_test_support::verification::input("REPROTO_SERVER_HOOKS_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                trace(
                    case["steps"].as_array().unwrap(),
                    case["fail"].as_bool().unwrap(),
                    case["reject"].as_bool().unwrap(),
                    case["wire"].as_bool().unwrap(),
                )
                .await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn shortening_future_is_canceled_with_its_local_owner() {
    struct Dropped(Rc<Cell<bool>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for revoke in [false, true] {
                let dropped = Rc::new(Cell::new(false));
                let guard = Dropped(dropped.clone());
                let state = Rc::new(State::default());
                state.alive.set(true);
                *state.shorten.borrow_mut() = Some(Promise::from_future(async move {
                    let _guard = guard;
                    futures::future::pending::<capnp::Result<Client>>().await
                }));
                let (executor, driver) = capnp_rpc::new_call_executor();
                let drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
                let owner = RevocableServer::<harness::Client>::new_with_executor(
                    Service(state.clone()),
                    executor,
                );
                settle().await;
                assert!(!dropped.get());
                if revoke {
                    owner.revoke();
                } else {
                    drop(owner);
                }
                assert!(
                    dropped.get(),
                    "resolution future must be destroyed synchronously"
                );
                assert!(!state.alive.get());
                drop(drivers);
            }
            // Non-revocable last-client drop also cancels the eager resolution task.
            let dropped = Rc::new(Cell::new(false));
            let guard = Dropped(dropped.clone());
            let state = Rc::new(State::default());
            *state.shorten.borrow_mut() = Some(Promise::from_future(async move {
                let _guard = guard;
                futures::future::pending::<capnp::Result<Client>>().await
            }));
            let (executor, driver) = capnp_rpc::new_call_executor();
            let _drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
            let client: harness::Client =
                capnp_rpc::new_client_with_executor(Service(state), executor);
            settle().await;
            drop(client);
            assert!(dropped.get());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn direct_self_resolution_rejects_without_losing_the_local_route() {
    let state = Rc::new(State::default());
    let (tx, rx) = oneshot::channel();
    *state.shorten.borrow_mut() = Some(Promise::from_future(async move {
        rx.await
            .map_err(|_| Error::failed("resolver dropped".into()))
    }));
    let client: harness::Client = capnp_rpc::new_client(Service(state.clone()));
    tx.send(state.self_cap.get::<Client>().unwrap())
        .ok()
        .unwrap();
    let error = client.client.when_resolved().await.unwrap_err();
    assert!(error.extra.contains("resolved to itself"));
    assert_eq!(
        client
            .echo_request()
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn canonical_aliases_share_the_streaming_queue() {
    let state = Rc::new(State::default());
    let (tx, rx) = oneshot::channel();
    state.gates.borrow_mut().push_back(rx);
    let server = Rc::new(Service(state.clone()));
    let a: harness::Client = capnp_rpc::new_client_from_rc(server.clone());
    let b: harness::Client = capnp_rpc::new_client_from_rc(server);
    let mut stream = a.stream_request().send();
    assert!(futures::poll!(&mut stream).is_pending());
    let mut echo = b.echo_request().send().promise;
    assert!(futures::poll!(&mut echo).is_pending());
    assert_eq!(state.echoes.get(), 0);
    tx.send(()).unwrap();
    stream.await.unwrap();
    assert_eq!(echo.await.unwrap().get().unwrap().get_value(), 1);
}

async fn self_trace(steps: &[serde_json::Value], revocable: bool) {
    let state = Rc::new(State::default());
    state.alive.set(true);
    let mut external = Some(Rc::new(Service(state.clone())));
    let mut owner = None;
    let mut root = if revocable {
        owner = Some(RevocableServer::<harness::Client>::from_rc(
            external.as_ref().unwrap().clone(),
        ));
        None
    } else {
        Some(capnp_rpc::new_client_from_rc::<harness::Client, _>(
            external.as_ref().unwrap().clone(),
        ))
    };
    let mut alias: Option<harness::Client> = None;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "self" => {
                alias = Some(state.self_cap.get().unwrap());
            }
            "construct" => {
                let new: harness::Client =
                    capnp_rpc::new_client_from_rc(external.as_ref().unwrap().clone());
                if revocable {
                    let other: reproto_test_support::cancellation_policy_capnp::policy::Client =
                        capnp_rpc::new_client_from_rc(external.as_ref().unwrap().clone());
                    assert!(other
                        .cancellable_request()
                        .send()
                        .promise
                        .await
                        .err()
                        .unwrap()
                        .extra
                        .contains("revocable client"));
                    assert!(new
                        .echo_request()
                        .send()
                        .promise
                        .await
                        .err()
                        .unwrap()
                        .extra
                        .contains("revocable client"));
                } else {
                    alias = Some(new);
                }
            }
            "drop-root" => {
                root.take();
                owner.take();
            }
            "drop-alias" => {
                alias.take();
            }
            "drop-external" => {
                external.take();
            }
            "revoke" => {
                owner.as_ref().unwrap().revoke();
            }
            a => panic!("{a}"),
        }
        let e = step["state"].as_array().unwrap();
        assert_eq!(state.alive.get(), e[7] == 1, "server lifetime: {step}");
        assert_eq!(
            state.self_cap.get::<harness::Client>().is_some(),
            e[8] == 1,
            "weak reference: {step}"
        );
        let cap = root
            .clone()
            .or_else(|| owner.as_ref().map(RevocableServer::get_client));
        if let (Some(cap), Some(alias)) = (&cap, &alias) {
            assert_eq!(
                cap.as_client_hook().get_ptr(),
                alias.as_client_hook().get_ptr(),
                "canonical identity"
            );
        }
        if let Some(cap) = cap.or_else(|| alias.clone()) {
            let result = cap.echo_request().send().promise.await;
            assert_eq!(result.is_ok(), e[3] == 0, "revocation authority: {step}");
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_self_capability_traces() {
    let path = reproto_test_support::verification::input("REPROTO_SELF_CAPABILITY_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for case in cases {
        self_trace(
            case["steps"].as_array().unwrap(),
            case["revocable"].as_bool().unwrap(),
        )
        .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn inherited_server_hooks_and_cross_interface_revocation_exclusivity() {
    use reproto_test_support::cancellation_policy_capnp::{derived, policy};
    struct Inherited(SelfCapability);
    impl ServerHooks for Inherited {
        fn self_cap(&self) -> Option<&SelfCapability> {
            Some(&self.0)
        }
    }
    impl policy::Server for Inherited {
        fn _capnp_server_hooks(&self) -> Option<&dyn ServerHooks> {
            Some(self)
        }
    }
    impl derived::Server for Inherited {}
    let server = Rc::new(Inherited(SelfCapability::default()));
    let client: derived::Client = capnp_rpc::new_client_from_rc(server.clone());
    let self_client = server.0.get::<derived::Client>().unwrap();
    assert_eq!(
        self_client.as_client_hook().get_ptr(),
        client.as_client_hook().get_ptr()
    );
    // A base interface view must not establish a new revocation owner while
    // the generated derived interface still holds the same server.
    let rejected = RevocableServer::<policy::Client>::from_rc(server.clone());
    assert!(rejected
        .get_client()
        .cancellable_request()
        .send()
        .promise
        .await
        .err()
        .unwrap()
        .extra
        .contains("revocable client"));
    drop((self_client, client));
    assert!(server.0.get::<derived::Client>().is_none());
    let owner = RevocableServer::<derived::Client>::from_rc(server.clone());
    let rejected: policy::Client = capnp_rpc::new_client_from_rc(server.clone());
    assert!(rejected
        .cancellable_request()
        .send()
        .promise
        .await
        .err()
        .unwrap()
        .extra
        .contains("revocable client"));
    owner.revoke();
    let replacement: policy::Client = capnp_rpc::new_client_from_rc(server.clone());
    assert_eq!(
        server
            .0
            .get::<policy::Client>()
            .unwrap()
            .as_client_hook()
            .get_ptr(),
        replacement.as_client_hook().get_ptr()
    );
}

async fn lookup_trace(steps: &[serde_json::Value], reject: bool) {
    let state = Rc::new(State::default());
    let target_state = Rc::new(State::default());
    let (tx, rx) = oneshot::channel();
    let mut tx = Some(tx);
    state.gates.borrow_mut().push_back(rx);
    let (resolve, resolution) = oneshot::channel();
    let mut resolve = Some(resolve);
    *state.shorten.borrow_mut() = Some(Promise::from_future(async move {
        resolution
            .await
            .map_err(|_| Error::failed("resolver dropped".into()))?
    }));
    let (executor, driver) = capnp_rpc::new_call_executor();
    let _drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
    let mut set = capnp_rpc::CapabilityServerSet::<Service, harness::Client>::new();
    let target = set.new_client(Service(target_state.clone()));
    let client = set.new_client_with_executor(Service(state.clone()), executor);
    let mut stream = Some(client.stream_request().send());
    assert!(futures::poll!(stream.as_mut().unwrap()).is_pending());
    let mut lookup = None;
    let mut found = None;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "lookup" => {
                lookup = Some(Box::pin(set.get_local_server(&client)));
            }
            "shorten" => {
                resolve
                    .take()
                    .unwrap()
                    .send(if reject {
                        Err(Error::failed("shortening failed".into()))
                    } else {
                        Ok(target.client.clone())
                    })
                    .ok()
                    .unwrap();
            }
            "complete" => {
                tx.take().unwrap().send(()).unwrap();
            }
            "drop-lookup" => {
                lookup.take();
                found.take();
            }
            _ => panic!(),
        }
        for _ in 0..48 {
            if let Some(p) = &mut stream {
                if let std::task::Poll::Ready(r) = futures::poll!(p) {
                    r.unwrap();
                    stream.take();
                }
            }
            if let Some(p) = &mut lookup {
                if let std::task::Poll::Ready(r) = futures::poll!(p) {
                    found = Some(r.unwrap());
                    lookup.take();
                }
            }
            tokio::task::yield_now().await;
        }
        let e = step["state"].as_array().unwrap();
        let identity = |server: &Rc<Service>| {
            if Rc::ptr_eq(&server.0, &state) {
                2
            } else {
                assert!(Rc::ptr_eq(&server.0, &target_state));
                3
            }
        };
        let expected = if e[2] == 2 {
            2
        } else if e[2] == 3 {
            3
        } else {
            0
        };
        assert_eq!(
            found.as_ref().map_or(0, identity),
            expected,
            "async lookup: {step}"
        );
        let expected = if e[0] == 1 {
            0
        } else if e[1] == 1 {
            3
        } else {
            2
        };
        assert_eq!(
            set.get_local_server_of_resolved(&client)
                .as_ref()
                .map_or(0, identity),
            expected,
            "sync lookup: {step}"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_shorten_lookup_traces() {
    let path = reproto_test_support::verification::input("REPROTO_SHORTEN_LOOKUP_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                lookup_trace(
                    case["steps"].as_array().unwrap(),
                    case["reject"].as_bool().unwrap(),
                )
                .await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn local_lookup_does_not_wait_for_shortening_or_switch_a_prior_lookup() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for before in [true, false] {
                let steps = if before {
                    serde_json::json!([
                        {"action":"lookup","state":[1,0,1,1]},
                        {"action":"shorten","state":[1,1,1,1]},
                        {"action":"complete","state":[0,1,2,1]}
                    ])
                } else {
                    serde_json::json!([
                        {"action":"complete","state":[0,0,0,0]},
                        {"action":"lookup","state":[0,0,2,1]},
                        {"action":"shorten","state":[0,1,2,1]}
                    ])
                };
                lookup_trace(steps.as_array().unwrap(), false).await;
            }
        })
        .await;
}

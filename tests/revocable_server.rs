#[allow(dead_code)]
mod support;
use capnp::{
    any_pointer,
    capability::{Client, Promise, Response},
    private::capability::ResultsHook,
    traits::HasTypeId,
    Error, ErrorKind,
};
use capnp_rpc::{RevocableServer, RpcSystem};
use capntproto_test_support::{cancellation_policy_capnp::policy, runtime_test_capnp::harness};
use futures::channel::oneshot;
use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
};
use support::Hub;
#[derive(Default)]
struct State {
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    running: Cell<bool>,
    completed: Cell<bool>,
    alive: Cell<bool>,
    server: Cell<bool>,
    fail: bool,
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}
struct Child(Rc<State>);
impl Drop for Child {
    fn drop(&mut self) {
        self.0.alive.set(false);
    }
}
impl harness::Server for Child {
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
impl Drop for Service {
    fn drop(&mut self) {
        self.0.server.set(false);
    }
}
impl Service {
    async fn work(
        self: Rc<Self>,
        early: bool,
        mut results: Box<dyn ResultsHook>,
    ) -> capnp::Result<()> {
        let _running = Running(self.0.clone());
        self.0.running.set(true);
        self.0.alive.set(true);
        results
            .get()?
            .init_as::<policy::pending_results::Builder>()
            .set_cap(capnp_rpc::new_client(Child(self.0.clone())));
        let results = if early {
            drop(results);
            None
        } else {
            Some(results)
        };
        let gate = self.0.gate.borrow_mut().take().expect("dispatched twice");
        gate.await
            .map_err(|_| Error::failed("gate dropped".into()))?;
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
        self.work(p.get()?.get_early_drop(), r.hook).await
    }
    async fn cancellable(
        self: Rc<Self>,
        p: policy::CancellableParams,
        r: policy::CancellableResults,
    ) -> capnp::Result<()> {
        self.work(p.get()?.get_early_drop(), r.hook).await
    }
}
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
async fn settle() {
    for _ in 0..48 {
        tokio::task::yield_now().await;
    }
}
type PendingResponse = Pin<Box<Promise<Response<any_pointer::Owned>, Error>>>;
struct Fixture {
    _hub: Rc<RefCell<Hub>>,
    drivers: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
    owner: Option<RevocableServer<policy::Client>>,
    client: policy::Client,
    #[cfg(unix)]
    host_client: policy::Client,
    #[cfg(unix)]
    retained_fd: Option<Rc<std::os::fd::OwnedFd>>,
    state: Rc<State>,
    gate: Option<oneshot::Sender<()>>,
    promise: Option<PendingResponse>,
    response: Option<Response<any_pointer::Owned>>,
    outcome: u32,
    revoked_reason: Option<&'static str>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.drivers {
            task.abort();
        }
    }
}
impl Fixture {
    async fn new(allow: bool, early: bool, fail: bool, wire: bool, membrane: bool) -> Self {
        let state = Rc::new(State {
            fail,
            ..Default::default()
        });
        state.server.set(true);
        let (tx, rx) = oneshot::channel();
        *state.gate.borrow_mut() = Some(rx);
        let mut drivers = vec![];
        let owner = if wire {
            RevocableServer::<policy::Client>::new(Service(state.clone()))
        } else {
            let (executor, driver) = capnp_rpc::new_call_executor();
            drivers.push(tokio::task::spawn_local(driver));
            #[cfg(unix)]
            {
                RevocableServer::new_with_fd_and_executor(
                    Service(state.clone()),
                    std::fs::File::open("/dev/zero").unwrap().into(),
                    executor,
                )
            }
            #[cfg(not(unix))]
            {
                RevocableServer::new_with_executor(Service(state.clone()), executor)
            }
        };
        assert!(!owner.is_in_use());
        #[cfg(unix)]
        let host_client = owner.get_client();
        #[cfg(unix)]
        let retained_fd = host_client.client.get_fd().await.unwrap();
        #[cfg(unix)]
        assert_eq!(retained_fd.is_some(), !wire);
        let hub = Rc::new(RefCell::new(Hub::default()));
        let client = if wire {
            let server = RpcSystem::new(
                Box::new(Hub::network(&hub, 1)),
                Some(owner.get_client().client),
            );
            let mut client = RpcSystem::new(Box::new(Hub::network(&hub, 2)), None);
            let cap: policy::Client = client.bootstrap(1);
            drivers.push(tokio::task::spawn_local(server));
            drivers.push(tokio::task::spawn_local(client));
            capnp::capability::get_resolved_cap(cap).await
        } else {
            owner.get_client()
        };
        assert!(owner.is_in_use());
        let client = if membrane {
            capnp_rpc::membrane::Membrane::new(Rc::new(Permit)).export(client)
        } else {
            client
        };
        let mut request: capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> =
            client
                .client
                .new_call(policy::Client::TYPE_ID, u16::from(allow), None);
        request
            .get()
            .init_as::<policy::pending_params::Builder>()
            .set_early_drop(early);
        let mut promise = Box::pin(request.send().promise);
        assert!(futures::poll!(&mut promise).is_pending());
        settle().await;
        assert!(state.running.get());
        assert!(state.alive.get());
        Self {
            _hub: hub,
            drivers,
            owner: Some(owner),
            client,
            #[cfg(unix)]
            host_client,
            #[cfg(unix)]
            retained_fd,
            state,
            gate: Some(tx),
            promise: Some(promise),
            response: None,
            outcome: 0,
            revoked_reason: None,
        }
    }
    fn check_error(&self, error: &Error) {
        let reason = self.revoked_reason.unwrap();
        assert!(error.extra.contains(reason), "{error:?}");
        if reason == "owner revoked" {
            assert_eq!(error.kind, ErrorKind::Overloaded);
            assert_eq!(error.detail(9), Some(&[1, 2][..]));
        }
    }
    async fn step(&mut self, action: &str) {
        match action {
            "drop" => {
                self.promise.take();
                self.response.take();
            }
            "complete" => {
                self.gate.take().unwrap().send(()).unwrap();
            }
            "revoke" => {
                self.revoked_reason.get_or_insert("owner revoked");
                let mut error = Error::from_kind(ErrorKind::Overloaded);
                error.extra = "owner revoked".into();
                error.set_detail(9, vec![1, 2]);
                self.owner.as_ref().unwrap().revoke_with_error(error);
                assert!(
                    !self.state.running.get(),
                    "revoke must destroy suspended methods synchronously"
                );
                assert!(
                    !self.state.server.get(),
                    "revoke must release server synchronously"
                );
            }
            "owner" => {
                self.revoked_reason.get_or_insert("capability was revoked");
                self.owner.take();
                assert!(!self.state.running.get());
                assert!(!self.state.server.get());
            }
            "retry" => {
                self.owner
                    .as_ref()
                    .unwrap()
                    .revoke_with_error(Error::failed("second reason".into()));
            }
            "probe" => {
                let error = self
                    .client
                    .pending_request()
                    .send()
                    .promise
                    .await
                    .err()
                    .unwrap();
                self.check_error(&error);
            }
            "use" => {
                let cap = self
                    .response
                    .as_ref()
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as::<policy::pending_results::Reader>()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                assert_eq!(
                    cap.echo_request()
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    42
                );
            }
            _ => panic!("unknown action {action}"),
        }
        for _ in 0..48 {
            if let Some(p) = self.promise.as_mut() {
                if let std::task::Poll::Ready(result) = futures::poll!(p) {
                    match result {
                        Ok(response) => {
                            self.response = Some(response);
                            self.outcome = 1;
                        }
                        Err(error) => {
                            if error.extra.contains("application failure") {
                                self.outcome = 3;
                            } else {
                                self.check_error(&error);
                                self.outcome = 2;
                            }
                        }
                    }
                    self.promise.take();
                }
            }
            tokio::task::yield_now().await;
        }
        #[cfg(unix)]
        if let Some(fd) = &self.retained_fd {
            assert_eq!(
                self.host_client.client.get_fd().await.unwrap().is_some(),
                self.state.server.get(),
                "local descriptor availability follows revocation"
            );
            use std::io::Read;
            let mut file = std::fs::File::from(fd.try_clone().unwrap());
            let mut byte = [1u8];
            file.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [0], "previously obtained descriptors stay usable");
        }
    }
    fn check(&self, state: &[serde_json::Value]) {
        assert_eq!(self.state.completed.get(), state[0] == 1, "completion");
        assert_eq!(self.state.running.get(), state[1] == 1, "running");
        assert_eq!(self.state.alive.get(), state[4] == 1, "result capability");
        assert_eq!(self.state.server.get(), state[5] == 1, "server ownership");
        assert_eq!(self.outcome as u64, state[6].as_u64().unwrap(), "outcome");
        assert_eq!(self.owner.is_some(), state[7] == 1, "revocation owner");
    }
}
#[tokio::test(flavor = "current_thread")]
async fn protected_methods_are_synchronously_revoked_even_after_caller_drop() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for membrane in [false, true] {
                    for early in [false, true] {
                        let mut f = Fixture::new(false, early, false, wire, membrane).await;
                        f.step("drop").await;
                        assert!(f.state.running.get());
                        assert!(f.state.alive.get());
                        f.step("revoke").await;
                        assert!(!f.state.running.get());
                        assert!(!f.state.alive.get());
                        f.step("retry").await;
                        f.step("probe").await;
                    }
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_revocable_server_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_REVOCABLE_SERVER_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for wire in [false, true] {
                    for membrane in [false, true] {
                        for early in [false, true] {
                            let mut f = Fixture::new(
                                case["allow"].as_bool().unwrap(),
                                early,
                                case["fail"].as_bool().unwrap(),
                                wire,
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
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn revocation_before_dispatch_and_duplicate_owner_rejection() {
    let state = Rc::new(State::default());
    state.server.set(true);
    let server = Rc::new(Service(state.clone()));
    let first = RevocableServer::<policy::Client>::from_rc(server.clone());
    let second = RevocableServer::<policy::Client>::from_rc(server.clone());
    assert!(second
        .get_client()
        .pending_request()
        .send()
        .promise
        .await
        .err()
        .unwrap()
        .extra
        .contains("revocable client"));
    let bypass: policy::Client = capnp_rpc::new_client_from_rc(server.clone());
    assert!(bypass
        .pending_request()
        .send()
        .promise
        .await
        .err()
        .unwrap()
        .extra
        .contains("revocable client"));
    let pending = first.get_client().pending_request().send();
    first.revoke();
    assert!(pending
        .promise
        .await
        .err()
        .unwrap()
        .extra
        .contains("revoked"));
    assert!(!state.running.get());
    assert!(state.server.get());
    assert!(!first.is_in_use());
    let replacement = RevocableServer::<policy::Client>::from_rc(server.clone());
    drop(server);
    assert!(state.server.get());
    drop(second);
    assert!(state.server.get());
    drop(replacement);
    assert!(!state.server.get());
}

#[tokio::test(flavor = "current_thread")]
async fn self_revocation_and_reentrant_destruction_do_not_borrow_panic() {
    struct SelfRevoking {
        owner: Rc<RefCell<Option<std::rc::Weak<RevocableServer<harness::Client>>>>>,
        dropped: Rc<Cell<bool>>,
    }
    impl Drop for SelfRevoking {
        fn drop(&mut self) {
            self.dropped.set(true);
            if let Some(owner) = self.owner.borrow().as_ref().and_then(|o| o.upgrade()) {
                owner.revoke();
            }
        }
    }
    impl harness::Server for SelfRevoking {
        async fn echo(
            self: Rc<Self>,
            _: harness::EchoParams,
            _: harness::EchoResults,
        ) -> capnp::Result<()> {
            self.owner
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .revoke();
            Ok(())
        }
    }
    let slot = Rc::new(RefCell::new(None));
    let dropped = Rc::new(Cell::new(false));
    let owner = Rc::new(RevocableServer::<harness::Client>::new(SelfRevoking {
        owner: slot.clone(),
        dropped: dropped.clone(),
    }));
    *slot.borrow_mut() = Some(Rc::downgrade(&owner));
    let error = owner
        .get_client()
        .echo_request()
        .send()
        .promise
        .await
        .err()
        .unwrap();
    assert!(error.extra.contains("revoked"));
    assert!(dropped.get());
    assert!(!owner.is_in_use());
}

#[tokio::test(flavor = "current_thread")]
async fn revocation_cancels_stream_and_queued_call_without_dispatch() {
    struct Streaming(Rc<Cell<u32>>, Rc<Cell<bool>>);
    impl Drop for Streaming {
        fn drop(&mut self) {
            self.1.set(true);
        }
    }
    impl harness::Server for Streaming {
        async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
            self.0.set(self.0.get() + 1);
            futures::future::pending().await
        }
        async fn echo(
            self: Rc<Self>,
            _: harness::EchoParams,
            _: harness::EchoResults,
        ) -> capnp::Result<()> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    let calls = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(false));
    let owner = RevocableServer::<harness::Client>::new(Streaming(calls.clone(), dropped.clone()));
    let client = owner.get_client();
    let mut stream = client.stream_request().send();
    assert!(futures::poll!(&mut stream).is_pending());
    let mut queued = client.echo_request().send().promise;
    assert!(futures::poll!(&mut queued).is_pending());
    owner.revoke();
    assert!(dropped.get());
    assert_eq!(calls.get(), 1);
    assert!(stream.await.unwrap_err().extra.contains("revoked"));
    assert!(queued.await.err().unwrap().extra.contains("revoked"));
    assert_eq!(calls.get(), 1);
}

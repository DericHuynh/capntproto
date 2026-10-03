#[allow(dead_code)]
mod support;
use capnp::{
    any_pointer,
    capability::{Client, Promise},
    dynamic_capability as dynamic,
    traits::HasTypeId,
    Error,
};
use capnp_rpc::{RevocableServer, RpcSystem};
use capntproto_test_support::cancellation_policy_capnp::policy;
use futures::channel::oneshot;
use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
};
use support::Hub;
#[derive(Default)]
struct State {
    running: Cell<bool>,
    completed: Cell<bool>,
    server: Cell<bool>,
    bstarted: Cell<bool>,
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    fail: bool,
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}
struct Service(Rc<State>);
impl Drop for Service {
    fn drop(&mut self) {
        self.0.server.set(false);
    }
}
impl Service {
    async fn work(self: Rc<Self>) -> capnp::Result<()> {
        self.0.running.set(true);
        let _running = Running(self.0.clone());
        let gate = self.0.gate.borrow_mut().take().unwrap();
        gate.await.map_err(|_| Error::failed("gate gone".into()))?;
        self.0.completed.set(true);
        if self.0.fail {
            Err(Error::failed("stream failed".into()))
        } else {
            Ok(())
        }
    }
}
impl policy::Server for Service {
    async fn stream(self: Rc<Self>, _: policy::StreamParams) -> capnp::Result<()> {
        self.work().await
    }
    async fn cancellable_stream(
        self: Rc<Self>,
        _: policy::CancellableStreamParams,
    ) -> capnp::Result<()> {
        self.work().await
    }
    async fn pending(
        self: Rc<Self>,
        _: policy::PendingParams,
        _: policy::PendingResults,
    ) -> capnp::Result<()> {
        assert!(!self.0.bstarted.replace(true));
        Ok(())
    }
}
struct DynamicService(Service, bool);
impl dynamic::Server for DynamicService {
    fn get_schema(&self) -> capnp::schema::InterfaceSchema {
        policy::Client::schema()
    }
    fn allow_cancellation(&self) -> bool {
        self.1
    }
    fn call(
        self: Rc<Self>,
        method: capnp::schema::Method,
        _: dynamic::CallContext,
    ) -> Promise<(), Error> {
        Promise::from_future(async move {
            match method.get_index() {
                2 | 3 => {
                    self.0 .0.running.set(true);
                    let _running = Running(self.0 .0.clone());
                    let gate = self.0 .0.gate.borrow_mut().take().unwrap();
                    gate.await.map_err(|_| Error::failed("gate gone".into()))?;
                    self.0 .0.completed.set(true);
                    if self.0 .0.fail {
                        return Err(Error::failed("stream failed".into()));
                    }
                }
                0 => {
                    assert!(!self.0 .0.bstarted.replace(true));
                }
                _ => return Err(Error::unimplemented("test method".into())),
            }
            Ok(())
        })
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
type Pending = Pin<Box<Promise<(), Error>>>;
fn request(client: &policy::Client, method: u16, reflected: bool) -> Pending {
    if reflected {
        let client = dynamic::Client::new(client.clone(), policy::Client::schema());
        let method = policy::Client::schema().get_methods().unwrap().get(method);
        Box::pin(
            client
                .new_request_for(method, None)
                .unwrap()
                .send_ignoring_result(),
        )
    } else {
        let r: capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> = client
            .client
            .new_call(policy::Client::TYPE_ID, method, None);
        Box::pin(Promise::from_future(async move {
            r.send().promise.await.map(|_| ())
        }))
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
async fn trace(
    steps: &[serde_json::Value],
    allow: bool,
    fail: bool,
    wire: bool,
    membrane: bool,
    reflected: bool,
) {
    let state = Rc::new(State {
        fail,
        ..Default::default()
    });
    state.server.set(true);
    let (tx, rx) = oneshot::channel();
    *state.gate.borrow_mut() = Some(rx);
    let mut gate = Some(tx);
    let mut drivers = Drivers(vec![]);
    let (executor, driver) = capnp_rpc::new_call_executor();
    drivers.0.push(tokio::task::spawn_local(driver));
    let (local, revoke): (Client, Box<dyn Fn()>) = if reflected {
        let owner = RevocableServer::<Client>::new_with_executor(
            DynamicService(Service(state.clone()), allow),
            executor,
        );
        (owner.get_client(), Box::new(move || owner.revoke()))
    } else {
        let owner = if wire {
            RevocableServer::<policy::Client>::new(Service(state.clone()))
        } else {
            RevocableServer::new_with_executor(Service(state.clone()), executor)
        };
        (owner.get_client().client, Box::new(move || owner.revoke()))
    };
    let hub = Rc::new(RefCell::new(Hub::default()));
    let client = if wire {
        let server = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(local));
        let mut client = RpcSystem::new(Box::new(Hub::network(&hub, 2)), None);
        let cap: policy::Client = client.bootstrap(1);
        drivers.0.push(tokio::task::spawn_local(server));
        drivers.0.push(tokio::task::spawn_local(client));
        capnp::capability::get_resolved_cap(cap).await
    } else {
        policy::Client { client: local }
    };
    let client = if membrane {
        capnp_rpc::membrane::Membrane::new(Rc::new(Permit)).export(client)
    } else {
        client
    };
    let mut calls = [
        Some(request(&client, if allow { 3 } else { 2 }, reflected)),
        None,
    ];
    let mut outcomes = [0u64; 2];
    assert!(futures::poll!(calls[0].as_mut().unwrap()).is_pending());
    for _ in 0..48 {
        tokio::task::yield_now().await;
    }
    assert!(state.running.get());
    for step in steps {
        match step["action"].as_str().unwrap() {
            "send" => {
                calls[1] = Some(request(&client, 0, reflected));
            }
            "drop-a" => {
                calls[0].take();
            }
            "drop-b" => {
                calls[1].take();
            }
            "complete" => {
                gate.take().unwrap().send(()).unwrap();
            }
            "revoke" => {
                revoke();
                assert!(!state.running.get());
                assert!(!state.server.get());
            }
            _ => panic!(),
        }
        for _ in 0..48 {
            for (i, call) in calls.iter_mut().enumerate() {
                if let Some(p) = call {
                    if let std::task::Poll::Ready(result) = futures::poll!(p) {
                        outcomes[i] = match result {
                            Ok(_) => 1,
                            Err(e) => {
                                if e.extra.contains("revoked") {
                                    2
                                } else {
                                    assert!(e.extra.contains("stream failed"), "{e}");
                                    3
                                }
                            }
                        };
                        call.take();
                    }
                }
            }
            tokio::task::yield_now().await;
        }
        let expected = step["state"].as_array().unwrap();
        assert_eq!(state.running.get(), expected[0] == 1, "running");
        assert_eq!(state.completed.get(), expected[1] == 1, "completion");
        assert_eq!(state.server.get(), expected[3] == 0, "server lifetime");
        assert_eq!(state.bstarted.get(), expected[6] == 1, "queued dispatch");
        assert_eq!(
            outcomes,
            [expected[7].as_u64().unwrap(), expected[8].as_u64().unwrap()],
            "outcomes"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn protected_stream_cancellation_releases_queue_but_revocation_stops_work() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let steps = serde_json::json!([
                {"action":"send","state":[1,0,1,0,1,1,0,0,0,0]},
                {"action":"drop-a","state":[1,0,0,0,1,1,1,0,1,0]},
                {"action":"revoke","state":[0,0,0,1,1,1,1,0,1,0]}
            ]);
            for wire in [false, true] {
                for membrane in [false, true] {
                    for reflected in [false, true] {
                        trace(
                            steps.as_array().unwrap(),
                            false,
                            false,
                            wire,
                            membrane,
                            reflected,
                        )
                        .await;
                    }
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_revocable_streaming_traces() {
    let path =
        capntproto_test_support::verification::input("CAPNTPROTO_REVOCABLE_STREAMING_TRACES")
            .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for wire in [false, true] {
                    for membrane in [false, true] {
                        for reflected in [false, true] {
                            trace(
                                case["steps"].as_array().unwrap(),
                                case["allow"].as_bool().unwrap(),
                                case["fail"].as_bool().unwrap(),
                                wire,
                                membrane,
                                reflected,
                            )
                            .await;
                        }
                    }
                }
            }
        })
        .await;
}

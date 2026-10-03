#[allow(dead_code)]
mod support;
use capnp::{capability::Promise, private::capability::ResultsHook, Error};
use capnp_rpc::RpcSystem;
use capntproto_test_support::{cancellation_policy_capnp::policy, runtime_test_capnp::harness};
use futures::{channel::oneshot, FutureExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
mod ignore_result {
    pub mod hooks;
    pub mod verification;
}

#[derive(Default)]
struct State {
    results: RefCell<Option<Box<dyn ResultsHook>>>,
    gate: RefCell<Option<oneshot::Receiver<bool>>>,
    running: Cell<bool>,
    completed: Cell<u64>,
    alive: Cell<bool>,
}
struct Value(Rc<State>);
impl harness::Server for Value {}
impl Drop for Value {
    fn drop(&mut self) {
        self.0.alive.set(false);
    }
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
        self.0.results.borrow_mut().take();
    }
}
struct Service(Rc<State>);
impl Service {
    async fn work(self: Rc<Self>, mut results: Box<dyn ResultsHook>) -> capnp::Result<()> {
        self.0.running.set(true);
        let _running = Running(self.0.clone());
        self.0.alive.set(true);
        results
            .get()?
            .init_as::<harness::bounce_results::Builder>()
            .set_cap(capnp_rpc::new_client(Value(self.0.clone())));
        *self.0.results.borrow_mut() = Some(results);
        let gate = self.0.gate.borrow_mut().take().unwrap();
        let ok = gate.await.map_err(|_| Error::failed("gate lost".into()))?;
        self.0.completed.set(if ok { 1 } else { 2 });
        if ok {
            Ok(())
        } else {
            Err(Error::failed("late failure".into()))
        }
    }
}
impl policy::Server for Service {
    async fn pending(
        self: Rc<Self>,
        _: policy::PendingParams,
        r: policy::PendingResults,
    ) -> capnp::Result<()> {
        self.work(r.hook).await
    }
    async fn cancellable(
        self: Rc<Self>,
        _: policy::CancellableParams,
        r: policy::CancellableResults,
    ) -> capnp::Result<()> {
        self.work(r.hook).await
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
struct Fixture {
    state: Rc<State>,
    gate: Option<oneshot::Sender<bool>>,
    promise: Option<Promise<(), Error>>,
    outcome: u64,
    _drivers: Drivers,
}
impl Fixture {
    async fn new(wire: bool, cancellable: bool, api: u32, wrapper: u32) -> Self {
        let (tx, rx) = oneshot::channel();
        let state = Rc::new(State::default());
        *state.gate.borrow_mut() = Some(rx);
        let (executor, driver) = capnp_rpc::new_call_executor();
        let mut drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
        let client: policy::Client =
            capnp_rpc::new_client_with_executor(Service(state.clone()), executor);
        let client: policy::Client = if wire {
            let hub = Rc::new(RefCell::new(support::Hub::default()));
            let server = RpcSystem::new(
                Box::new(support::Hub::network(&hub, 1)),
                Some(client.client),
            );
            let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
            let client = caller.bootstrap(1);
            drivers.0.push(tokio::task::spawn_local(server));
            drivers.0.push(tokio::task::spawn_local(caller));
            client
        } else {
            client
        };
        let client = match wrapper {
            0 => client,
            1 => capnp_rpc::new_future_client(async move { Ok(client) }),
            2 => {
                struct Permit;
                impl capnp_rpc::membrane::Policy for Permit {
                    fn call(
                        &self,
                        _: capnp_rpc::membrane::Direction,
                        _: u64,
                        _: u16,
                        _: &capnp::capability::Client,
                    ) -> capnp::Result<Option<capnp::capability::Client>> {
                        Ok(None)
                    }
                }
                policy::Client {
                    client: capnp_rpc::membrane::Membrane::new(Rc::new(Permit))
                        .export(client.client),
                }
            }
            _ => panic!(),
        };
        let name = if cancellable {
            "cancellable"
        } else {
            "pending"
        };
        let promise = match api {
            0 => {
                if cancellable {
                    client.cancellable_request().send_ignoring_result()
                } else {
                    client.pending_request().send_ignoring_result()
                }
            }
            1 => capnp::dynamic_capability::Client::from(client)
                .new_request(name, None)
                .unwrap()
                .send_ignoring_result(),
            2 => {
                let mut loader = capnp::schema_loader::SchemaLoader::default();
                loader
                    .load_compiled_type_and_dependencies::<policy::Owned>()
                    .unwrap();
                let schema = capnp::schema_loader::dynamic::ServiceSchema::new(
                    Rc::new(loader),
                    policy::Client::schema().get_proto().get_id(),
                )
                .unwrap();
                schema
                    .reflect(client)
                    .unwrap()
                    .new_request(name)
                    .unwrap()
                    .send_ignoring_result()
            }
            _ => panic!(),
        };
        let mut f = Self {
            state,
            gate: Some(tx),
            promise: Some(promise),
            outcome: 0,
            _drivers: drivers,
        };
        f.drive().await;
        assert!(f.state.running.get());
        assert!(f.state.alive.get());
        assert_eq!(f.outcome, 0);
        f
    }
    async fn drive(&mut self) {
        for _ in 0..48 {
            if let Some(promise) = &mut self.promise {
                if let Some(result) = promise.now_or_never() {
                    self.outcome = match result {
                        Ok(()) => 1,
                        Err(e) => {
                            assert!(e.extra.contains("late failure"), "{e}");
                            2
                        }
                    };
                    self.promise.take();
                }
            }
            tokio::task::yield_now().await;
        }
    }
    async fn step(&mut self, event: u64) {
        match event {
            3 => self
                .state
                .results
                .borrow_mut()
                .as_mut()
                .unwrap()
                .set_pipeline()
                .unwrap(),
            4 => {
                self.state.results.borrow_mut().take();
            }
            5 | 6 => {
                self.gate.take().unwrap().send(event == 5).unwrap();
            }
            7 => {
                self.promise.take();
            }
            _ => panic!(),
        }
        self.drive().await;
    }
    fn observation(&self) -> [u64; 6] {
        [
            u64::from(self.promise.is_some()),
            u64::from(self.state.running.get()),
            self.state.completed.get(),
            self.outcome,
            u64::from(self.state.alive.get()),
            u64::from(self.state.results.borrow().is_some()),
        ]
    }
    async fn close(mut self) {
        self.promise.take();
        self.drive().await;
        if self.state.running.get() {
            self.gate.take().unwrap().send(true).unwrap();
            self.drive().await;
        }
        assert!(!self.state.alive.get());
        assert!(!self.state.running.get());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn typed_and_dynamic_helpers_preserve_lifetime_through_forwarders() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for api in 0..3 {
                    for wrapper in 0..3 {
                        for cancellable in [false, true] {
                            for terminal in [5, 6, 7] {
                                let mut f = Fixture::new(wire, cancellable, api, wrapper).await;
                                f.step(3).await;
                                f.step(4).await;
                                assert_eq!(f.observation(), [1, 1, 0, 0, 1, 0]);
                                f.step(terminal).await;
                                if terminal == 7 {
                                    let retained = u64::from(!cancellable);
                                    assert_eq!(f.observation(), [0, retained, 0, 0, retained, 0]);
                                } else {
                                    assert_eq!(
                                        f.observation(),
                                        [0, 0, terminal - 4, terminal - 4, 0, 0]
                                    );
                                }
                                f.close().await;
                            }
                        }
                    }
                }
            }
        })
        .await;
}

#[allow(dead_code)]
mod support;
use capnp::{
    capability::{FromClientHook, IntoTypelessPipeline, Promise, Response},
    Error,
};
use capnp_rpc::{PipelineBuilder, RpcSystem};
use futures::{channel::oneshot, FutureExt};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod result_pipeline {
    pub mod integration;
    pub mod verification;
}

struct Echo(u32, Rc<Cell<u32>>);
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.1.set(self.1.get() + 1);
        results.get().set_value(self.0);
        Ok(())
    }
}
type ParentPromise = Promise<Response<harness::pending_results::Owned>, Error>;
type ChildPromise = Promise<Response<harness::value::Owned>, Error>;
struct State {
    results: RefCell<Option<harness::PendingResults>>,
    gate: RefCell<Option<oneshot::Receiver<bool>>>,
    running: Cell<bool>,
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
        self.0.results.borrow_mut().take();
    }
}
struct Controlled(Rc<State>);
impl harness::Server for Controlled {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        results: harness::PendingResults,
    ) -> capnp::Result<()> {
        self.0.running.set(true);
        let _running = Running(self.0.clone());
        *self.0.results.borrow_mut() = Some(results);
        let gate = self.0.gate.borrow_mut().take().unwrap();
        if gate.await.map_err(|_| Error::failed("gate lost".into()))? {
            Ok(())
        } else {
            Err(Error::failed("late failure".into()))
        }
    }
}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
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
    resolve: Option<oneshot::Sender<bool>>,
    target: harness::Client,
    parent: Option<ParentPromise>,
    pipeline: harness::pending_results::Pipeline,
    children: Vec<Option<ChildPromise>>,
    parent_result: u64,
    done: u64,
    failed: u64,
    calls: Rc<Cell<u32>>,
    resolving: Option<Promise<(), Error>>,
    observed: u64,
    _drivers: Drivers,
}
impl Fixture {
    async fn new(wire: bool) -> Self {
        Self::with_wrapper(wire, |client| client).await
    }
    async fn with_wrapper(
        wire: bool,
        wrap: impl FnOnce(harness::Client) -> harness::Client,
    ) -> Self {
        let (gate, rx) = oneshot::channel();
        let state = Rc::new(State {
            results: RefCell::new(None),
            gate: RefCell::new(Some(rx)),
            running: Cell::new(false),
        });
        let client = wrap(capnp_rpc::new_client(Controlled(state.clone())));
        let mut drivers = Drivers(Vec::new());
        let client = if wire {
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
        let calls = Rc::new(Cell::new(0));
        let (resolve, rx) = oneshot::channel();
        let count = calls.clone();
        let target = capnp_rpc::new_future_client(async move {
            if rx.await.map_err(|_| Error::failed("target lost".into()))? {
                Ok(capnp_rpc::new_client(Echo(73, count)))
            } else {
                Err(Error::failed("target rejected".into()))
            }
        });
        let call = client.pending_request().send();
        let mut f = Self {
            state,
            gate: Some(gate),
            resolve: Some(resolve),
            target,
            parent: Some(call.promise),
            pipeline: call.pipeline,
            children: Vec::new(),
            parent_result: 0,
            done: 0,
            failed: 0,
            calls,
            resolving: None,
            observed: 0,
            _drivers: drivers,
        };
        f.drive().await;
        assert!(f.state.results.borrow().is_some());
        f
    }
    async fn drive(&mut self) {
        for _ in 0..32 {
            if let Some(parent) = &mut self.parent {
                if let Some(result) = parent.now_or_never() {
                    self.parent_result = if result.is_ok() { 1 } else { 2 };
                    self.parent.take();
                }
            }
            for child in &mut self.children {
                if let Some(promise) = child {
                    if let Some(result) = promise.now_or_never() {
                        match result {
                            Ok(response) => {
                                assert_eq!(response.get().unwrap().get_value(), 73);
                                self.done += 1;
                            }
                            Err(_) => self.failed += 1,
                        }
                        child.take();
                    }
                }
            }
            if let Some(resolving) = &mut self.resolving {
                if let Some(result) = resolving.now_or_never() {
                    self.observed = if result.is_ok() { 1 } else { 2 };
                    self.resolving.take();
                }
            }
            tokio::task::yield_now().await;
        }
    }
    fn publish(&mut self) -> capnp::Result<()> {
        let mut builder = PipelineBuilder::<harness::pending_results::Owned>::new();
        builder.get().set_cap(self.target.clone());
        self.state
            .results
            .borrow_mut()
            .as_mut()
            .unwrap()
            .set_pipeline_from(builder.build())
    }
    fn duplicate(&mut self) {
        let mut builder = PipelineBuilder::<harness::pending_results::Owned>::new();
        builder
            .get()
            .set_cap(capnp_rpc::new_client(Echo(99, Rc::new(Cell::new(0)))));
        assert!(self
            .state
            .results
            .borrow_mut()
            .as_mut()
            .unwrap()
            .set_pipeline_from(builder.build())
            .is_err());
    }
    fn finish(&mut self, success: bool) {
        let mut results = self.state.results.borrow_mut().take().unwrap();
        if success {
            results.get().set_cap(self.target.clone());
        }
        drop(results);
        self.gate.take().unwrap().send(success).unwrap();
    }
    fn call(&mut self) {
        self.children
            .push(Some(self.pipeline.get_cap().echo_request().send().promise));
    }
    fn observe(&mut self) {
        self.resolving = Some(self.pipeline.get_cap().as_client_hook().when_resolved());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn independent_publication_routes_before_return_with_local_and_wire_error_semantics() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for success in [false, true] {
                    let mut f = Fixture::new(wire).await;
                    f.call();
                    f.drive().await;
                    assert_eq!(f.done, 0);
                    f.publish().unwrap();
                    f.duplicate();
                    f.resolve.take().unwrap().send(true).unwrap();
                    f.drive().await;
                    assert_eq!(f.done, 1);
                    assert_eq!(f.parent_result, 0);
                    f.finish(success);
                    f.drive().await;
                    assert_eq!(f.parent_result, if success { 1 } else { 2 });
                    f.call();
                    f.drive().await;
                    // The local published pipeline survives a late parent error.
                    // Across RPC, Return(exception) breaks the caller's unresolved
                    // result pipeline; already completed child calls stay complete.
                    assert_eq!(
                        (f.done, f.failed),
                        if wire && !success { (1, 1) } else { (2, 0) }
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn target_failure_and_parent_failure_before_publication_propagate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let mut f = Fixture::new(wire).await;
                f.publish().unwrap();
                f.call();
                f.resolve.take().unwrap().send(false).unwrap();
                f.drive().await;
                assert_eq!((f.done, f.failed, f.parent_result), (0, 1, 0));
                f.finish(true);
                f.drive().await;
                assert_eq!(f.parent_result, 1);
                let mut f = Fixture::new(wire).await;
                f.call();
                f.finish(false);
                f.drive().await;
                assert_eq!((f.done, f.failed, f.parent_result), (0, 1, 2));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn builders_preserve_nested_paths_groups_generics_and_capability_lifetimes() {
    use reproto_test_support::dynamic_test_capnp::{base, parcel};
    let calls = Rc::new(Cell::new(0));
    for words in [0, 1, 64] {
        let target: harness::Client = capnp_rpc::new_client(Echo(73, calls.clone()));
        let identity = target.as_client_hook().get_ptr();
        let mut builder =
            PipelineBuilder::<harness::cap_cycle::Owned>::with_first_segment_words(words);
        builder
            .get()
            .set_cap(capnp_rpc::new_client(Echo(99, calls.clone())));
        builder.get().init_next().set_cap(target.clone());
        let nested = builder.build().get_next();
        let rebased = nested.into_typeless_pipeline().into_hook();
        let piped: harness::cap_cycle::Pipeline = capnp::capability::FromTypelessPipeline::new(
            capnp::any_pointer::Pipeline::new(rebased),
        );
        assert_eq!(piped.get_cap().as_client_hook().get_ptr(), identity);
        assert_eq!(
            piped
                .get_cap()
                .echo_request()
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            73
        );
        let mut grouped = PipelineBuilder::<harness::grouped::Owned>::new();
        grouped.get().get_body().set_cap(target.clone());
        assert_eq!(
            grouped
                .build()
                .get_body()
                .get_cap()
                .as_client_hook()
                .get_ptr(),
            identity
        );
        let mut generic = PipelineBuilder::<parcel::Owned<harness::Owned>>::new();
        generic.get().set_value(target).unwrap();
        let projected = generic.build().into_typeless_pipeline();
        assert_eq!(projected.get_pointer_field(0).as_cap().get_ptr(), identity);
        // Unset capability fields yield a broken client, not a panic.
        assert!(PipelineBuilder::<harness::pending_results::Owned>::new()
            .build()
            .get_cap()
            .echo_request()
            .send()
            .promise
            .await
            .is_err());
    }
    struct Service;
    impl base::Server<capnp::text::Owned> for Service {}
    let service = Rc::new(Service);
    let weak = Rc::downgrade(&service);
    let cap: base::Client<capnp::text::Owned> = capnp_rpc::new_client_from_rc(service);
    let mut builder = PipelineBuilder::<parcel::Owned<base::Owned<capnp::text::Owned>>>::new();
    builder.get().set_value(cap).unwrap();
    let pipeline = builder.build();
    assert!(weak.upgrade().is_some());
    drop(pipeline);
    assert!(weak.upgrade().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn publication_does_not_keep_canceled_parent_alive() {
    let mut f = Fixture::new(false).await;
    f.publish().unwrap();
    f.resolve.take().unwrap().send(true).unwrap();
    let child = f.pipeline.get_cap();
    assert_eq!(
        child
            .echo_request()
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        73
    );
    f.parent.take();
    settle().await;
    assert!(!f.state.running.get());
    // Published capability ownership is independent of the canceled call.
    assert_eq!(
        child
            .echo_request()
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        73
    );
}

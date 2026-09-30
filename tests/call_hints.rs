#[allow(dead_code)]
mod support;
use capnp::{
    capability::{CallHints, Client},
    traits::{HasTypeId, ImbueMut},
};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, message_target},
    Connection, RpcSystem,
};
use futures::FutureExt;
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use support::{Endpoint, Hub};
struct Dummy(Option<Rc<Cell<bool>>>);
impl Drop for Dummy {
    fn drop(&mut self) {
        if let Some(alive) = &self.0 {
            alive.set(false);
        }
    }
}
impl harness::Server for Dummy {}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    _hub: Rc<RefCell<Hub>>,
    _network: support::Network,
    peer: Endpoint,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    remote: harness::Client,
    pipeline: Option<harness::bounce_results::Pipeline>,
    child: Option<harness::Client>,
    a: u32,
    b: Option<u32>,
    export: Option<u32>,
    alive: Rc<Cell<bool>>,
    finishes: Vec<u32>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(wrapper: u32) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 2);
        let mut system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
        let remote: harness::Client = system.bootstrap(2);
        let mut peer = hub.borrow_mut().connect(2, 1);
        let task = tokio::task::spawn_local(system);
        let input = peer.recv().await;
        let message::Bootstrap(b) = input
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let q = b.unwrap().get_question_id();
        peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(q);
            let mut p = r.init_results();
            let mut caps = vec![];
            let mut content = p.reborrow().get_content();
            content.imbue_mut(&mut caps);
            let dummy: harness::Client = capnp_rpc::new_client(Dummy(None));
            content.set_as_capability(dummy.client.hook);
            p.init_cap_table(1).get(0).set_sender_hosted(77);
        });
        let remote = capnp::capability::get_resolved_cap(remote).await;
        let remote = match wrapper {
            0 => remote,
            1 => capnp_rpc::new_future_client(async move { Ok(remote) }),
            2 => {
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
                capnp_rpc::membrane::Membrane::new(Rc::new(Permit)).export(remote)
            }
            3 => {
                capnp_rpc::auto_reconnect(move || Ok(remote.clone()))
                    .unwrap()
                    .0
            }
            _ => panic!(),
        };
        settle().await;
        let mut f = Self {
            _hub: hub,
            _network: network,
            peer,
            task,
            remote,
            pipeline: None,
            child: None,
            a: 0,
            b: None,
            export: None,
            alive: Rc::new(Cell::new(false)),
            finishes: vec![],
        };
        f.drain();
        f.finishes.clear();
        f
    }
    fn drain(&mut self) {
        loop {
            let Some(result) = self.peer.receive_incoming_message().now_or_never() else {
                break;
            };
            let Some(m) = result.unwrap() else { break };
            match m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Finish(f) => self.finishes.push(f.unwrap().get_question_id()),
                message::Release(_) => {}
                _ => panic!("unexpected outgoing RPC message"),
            }
        }
    }
    async fn next_call(&mut self) -> Box<dyn capnp_rpc::IncomingMessage> {
        loop {
            let m = self.peer.recv().await;
            let call = match m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Call(_) => true,
                message::Finish(f) => {
                    self.finishes.push(f.unwrap().get_question_id());
                    false
                }
                message::Release(_) => false,
                _ => panic!(),
            };
            if call {
                return m;
            }
        }
    }
    async fn send(&mut self, second: bool, params: bool) {
        let mut request = self.remote.bounce_request();
        if params {
            self.alive.set(true);
            request
                .get()
                .set_cap(capnp_rpc::new_client(Dummy(Some(self.alive.clone()))));
        }
        let pipeline = request.send_for_pipeline();
        // Local wrappers drive dispatch when the pipeline is used; this poll
        // requests resolution without issuing an application method call.
        let cap = pipeline.get_cap();
        let mut resolve = Box::pin(cap.client.when_resolved());
        let m = tokio::select! { m = self.next_call() => m, result = &mut resolve => { result.unwrap(); self.next_call().await } };
        let message::Call(c) = m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let c = c.unwrap();
        let id = c.get_question_id();
        assert_eq!(c.get_only_promise_pipeline(), id & (1 << 31) != 0);
        assert!(!c.get_no_promise_pipelining());
        assert!(matches!(
            c.get_send_results_to().which().unwrap(),
            capnp_rpc::rpc_capnp::call::send_results_to::Caller(())
        ));
        if second {
            self.b = Some(id);
            assert_ne!(id, self.a);
        } else {
            self.a = id;
            assert_ne!(id & (1 << 31), 0);
            if params {
                let cap_descriptor::SenderHosted(id) = c
                    .get_params()
                    .unwrap()
                    .get_cap_table()
                    .unwrap()
                    .get(0)
                    .which()
                    .unwrap()
                else {
                    panic!()
                };
                self.export = Some(id);
            }
            self.pipeline = Some(pipeline);
        }
    }
    async fn use_child(&mut self) {
        let request = self.child.as_ref().unwrap().echo_request().send();
        let mut response = Box::pin(request.promise);
        let m = tokio::select! { m = self.next_call() => m, _ = &mut response => panic!("reply arrived before wire call") };
        let message::Call(c) = m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let c = c.unwrap();
        let message_target::PromisedAnswer(p) = c.get_target().unwrap().which().unwrap() else {
            panic!()
        };
        assert_eq!(p.unwrap().get_question_id(), self.a);
        let q = c.get_question_id();
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(q);
            r.set_release_param_caps(false);
            r.init_results()
                .get_content()
                .init_as::<harness::value::Builder>()
                .set_value(42);
        });
        assert_eq!(response.await.unwrap().get().unwrap().get_value(), 42);
    }
    async fn legacy(&mut self) {
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(self.a);
            r.set_release_param_caps(false);
            r.set_canceled(());
        });
        settle().await;
    }
}
async fn replay(steps: &[serde_json::Value], params: bool, wrapper: u32) {
    let mut f = Fixture::new(wrapper).await;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "send" => f.send(false, params).await,
            "derive" => f.child = Some(f.pipeline.as_ref().unwrap().get_cap()),
            "dropRoot" => {
                f.pipeline.take();
            }
            "dropChild" => {
                f.child.take();
            }
            "use" => f.use_child().await,
            "legacy" => f.legacy().await,
            "second" => f.send(true, false).await,
            "release" => {
                if let Some(id) = f.export {
                    f.peer.send(|m| {
                        let mut r = m.init_release();
                        r.set_id(id);
                        r.set_reference_count(1);
                    });
                }
            }
            _ => panic!(),
        }
        settle().await;
        f.drain();
        let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
        assert_eq!(
            f.finishes.iter().filter(|id| **id == f.a).count(),
            state[8] as usize,
            "{step}"
        );
        if let Some(b) = f.b {
            assert_eq!(u32::from(b & (1 << 31) != 0), state[9], "{step}");
        }
        if params {
            assert_eq!(f.alive.get(), state[10] != 0, "{step}");
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_call_hint_traces() {
    let path = reproto_test_support::verification::input("REPROTO_CALL_HINT_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                replay(
                    case["steps"].as_array().unwrap(),
                    case["params"].as_bool().unwrap(),
                    0,
                )
                .await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn wrappers_preserve_pipeline_only_and_disabled_pipeline_hints() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                for wrapper in 0..4 {
                    let mut f = Fixture::new(wrapper).await;
                    f.send(false, false).await;
                    f.child = Some(f.pipeline.as_ref().unwrap().get_cap());
                    f.pipeline.take();
                    f.use_child().await;
                    f.child.take();
                    settle().await;
                    f.drain();
                    assert_eq!(f.finishes.iter().filter(|id| **id == f.a).count(), 1);
                    f.legacy().await;
                    f.send(true, false).await;
                    assert_eq!(f.b.unwrap() & (1 << 31), 0);
                }
                for wrapper in 0..4 {
                    let mut f = Fixture::new(wrapper).await;
                    let request: capnp::capability::Request<
                        harness::bounce_params::Owned,
                        harness::bounce_results::Owned,
                    > = f.remote.client.new_call_with_hints(
                        harness::Client::TYPE_ID,
                        1,
                        None,
                        CallHints {
                            no_promise_pipelining: true,
                            only_promise_pipeline: false,
                        },
                    );
                    let remote = request.send();
                    assert!(remote
                        .pipeline
                        .get_cap()
                        .client
                        .when_resolved()
                        .await
                        .is_err());
                    let mut response = Box::pin(remote.promise);
                    let m = tokio::select! {
                        m = f.next_call() => m,
                        result = &mut response => panic!("response before Call: {:?}", result.err()),
                    };
                    let message::Call(c) = m
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!()
                    };
                    let c = c.unwrap();
                    assert!(c.get_no_promise_pipelining());
                    let q = c.get_question_id();
                    f.peer.send(|m| {
                        let mut r = m.init_return();
                        r.set_answer_id(q);
                        r.init_results();
                    });
                    response.await.unwrap();
                    let request: capnp::capability::StreamingRequest<harness::echo_params::Owned> =
                        f.remote.client.new_streaming_call_with_hints(
                            harness::Client::TYPE_ID, 0, None,
                            CallHints { no_promise_pipelining: true, only_promise_pipeline: false },
                        );
                    let mut ready = Box::pin(request.send());
                    let readiness = futures::poll!(&mut ready);
                    let m = f.next_call().await;
                    let message::Call(call) = m.get_body().unwrap().get_as::<message::Reader>().unwrap().which().unwrap() else { panic!() };
                    let call = call.unwrap();
                    assert!(call.get_no_promise_pipelining());
                    assert!(!call.get_only_promise_pipeline());
                    f.peer.send(|m| {
                        let mut r = m.init_return(); r.set_answer_id(call.get_question_id()); r.init_results();
                    });
                    match readiness {
                        std::task::Poll::Ready(result) => result.unwrap(),
                        std::task::Poll::Pending => ready.await.unwrap(),
                    }

                }
            })
            .await
            .unwrap();
        })
        .await;
}

struct Gated {
    started: RefCell<Option<futures::channel::oneshot::Sender<()>>>,
    complete: RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
    held: Rc<RefCell<Option<harness::PendingResults>>>,
    hold: bool,
    fail: bool,
    alive: Rc<Cell<bool>>,
}
impl harness::Server for Gated {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        struct Value(Rc<Cell<bool>>);
        impl Drop for Value {
            fn drop(&mut self) {
                self.0.set(false);
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
        self.alive.set(true);
        results
            .get()
            .set_cap(capnp_rpc::new_client(Value(self.alive.clone())));
        let retained = if self.hold {
            *self.held.borrow_mut() = Some(results);
            None
        } else {
            Some(results)
        };
        let complete = self.complete.borrow_mut().take().unwrap();
        let _ = self.started.borrow_mut().take().unwrap().send(());
        complete
            .await
            .map_err(|_| capnp::Error::failed("gate dropped".into()))?;
        drop(retained);
        if self.fail {
            Err(capnp::Error::failed("deliberate failure".into()))
        } else {
            Ok(())
        }
    }
}
struct Incoming {
    peer: Endpoint,
    _hub: Rc<RefCell<Hub>>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    system: Rc<RefCell<RpcSystem<u8>>>,
    complete: Option<futures::channel::oneshot::Sender<()>>,
    held: Rc<RefCell<Option<harness::PendingResults>>>,
    alive: Rc<Cell<bool>>,
    returns: u32,
    child: u32,
    reused: bool,
    reuse_return: bool,
}
impl Drop for Incoming {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Incoming {
    async fn new(hold: bool, fail: bool, limited: bool) -> Self {
        use std::{future::Future, pin::Pin};
        let (start, started) = futures::channel::oneshot::channel();
        let (complete, completed) = futures::channel::oneshot::channel();
        let held = Rc::new(RefCell::new(None));
        let alive = Rc::new(Cell::new(false));
        let service: harness::Client = capnp_rpc::new_client(Gated {
            started: RefCell::new(Some(start)),
            complete: RefCell::new(Some(completed)),
            held: held.clone(),
            hold,
            fail,
            alive: alive.clone(),
        });
        let hub = Rc::new(RefCell::new(Hub::default()));
        let mut system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(service.client));
        if limited {
            system.set_flow_limit(1);
        }
        let system = Rc::new(RefCell::new(system));
        let driver = system.clone();
        let task = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
            Pin::new(&mut *driver.borrow_mut()).poll(cx)
        }));
        let mut peer = hub.borrow_mut().connect(2, 1);
        peer.send(|m| m.init_bootstrap().set_question_id(0));
        let m = peer.recv().await;
        let message::Return(r) = m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let capnp_rpc::rpc_capnp::return_::Results(p) = r.unwrap().which().unwrap() else {
            panic!()
        };
        let cap_descriptor::SenderHosted(export) =
            p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
        else {
            panic!()
        };
        peer.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(1);
            c.set_only_promise_pipeline(true);
            c.reborrow().init_target().set_imported_cap(export);
            c.set_interface_id(harness::Client::TYPE_ID);
            c.set_method_id(3);
            c.init_params()
                .get_content()
                .init_as::<harness::pending_params::Builder>();
        });
        started.await.unwrap();
        settle().await;
        Self {
            peer,
            _hub: hub,
            task,
            system,
            complete: Some(complete),
            held,
            alive,
            returns: 0,
            child: 0,
            reused: false,
            reuse_return: false,
        }
    }
    fn finish(&mut self) {
        self.peer.send(|m| {
            let mut f = m.init_finish();
            f.set_question_id(1);
            f.set_require_early_cancellation_workaround(false);
        });
    }
    fn drain(&mut self) {
        loop {
            let Some(m) = self.peer.receive_incoming_message().now_or_never() else {
                break;
            };
            let Some(m) = m.unwrap() else { break };
            match m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(r) => {
                    let r = r.unwrap();
                    if r.get_answer_id() == 1 {
                        if self.reused {
                            assert!(matches!(
                                r.which().unwrap(),
                                capnp_rpc::rpc_capnp::return_::Results(_)
                            ));
                            self.reuse_return = true;
                        } else {
                            self.returns += 1;
                        }
                    } else if r.get_answer_id() == 2 {
                        self.child = match r.which().unwrap() {
                            capnp_rpc::rpc_capnp::return_::Results(p) => {
                                assert_eq!(
                                    p.unwrap()
                                        .get_content()
                                        .get_as::<harness::value::Reader>()
                                        .unwrap()
                                        .get_value(),
                                    42
                                );
                                1
                            }
                            capnp_rpc::rpc_capnp::return_::Exception(_) => 2,
                            _ => panic!(),
                        };
                    } else {
                        panic!("unexpected Return");
                    }
                }
                message::Release(_) => {}
                _ => panic!("unexpected pipeline-only message"),
            }
        }
    }
    async fn replay(steps: &[serde_json::Value], hold: bool, fail: bool) {
        let mut f = Self::new(hold, fail, false).await;
        for step in steps {
            match step["action"].as_str().unwrap() {
                "complete" => {
                    let _ = f.complete.take().unwrap().send(());
                }
                "finish" => f.finish(),
                "release" => {
                    f.held.borrow_mut().take();
                }
                "use" => f.peer.send(|m| {
                    let mut c = m.init_call();
                    c.set_question_id(2);
                    c.set_interface_id(harness::Client::TYPE_ID);
                    c.set_method_id(0);
                    let mut p = c.reborrow().init_target().init_promised_answer();
                    p.set_question_id(1);
                    p.init_transform(1).get(0).set_get_pointer_field(0);
                    c.init_params()
                        .get_content()
                        .init_as::<harness::echo_params::Builder>();
                }),
                "reuse" => {
                    f.reused = true;
                    f.peer.send(|m| m.init_bootstrap().set_question_id(1));
                }
                _ => panic!(),
            }
            settle().await;
            f.drain();
            let s: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
            assert_eq!(f.alive.get(), s[6] != 0, "{step}");
            assert_eq!(f.returns, s[5], "{step}");
            assert_eq!(f.child, s[7], "{step}");
            assert_eq!(f.reuse_return, s[4] != 0, "{step}");
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_incoming_hint_traces() {
    let path = reproto_test_support::verification::input("REPROTO_INCOMING_HINT_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                Incoming::replay(
                    case["steps"].as_array().unwrap(),
                    case["hold"].as_bool().unwrap(),
                    case["fail"].as_bool().unwrap(),
                )
                .await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn pipeline_only_context_holds_flow_credit_until_finish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut f = Incoming::new(false, false, true).await;
            f.complete.take().unwrap().send(()).unwrap();
            settle().await;
            // No Return exists to release credit. Context retention until Finish is
            // observable because the next message cannot be read at this limit.
            f.peer.send(|m| m.init_bootstrap().set_question_id(2));
            settle().await;
            assert!(f.peer.receive_incoming_message().now_or_never().is_none());
            f.system.borrow_mut().set_flow_limit(usize::MAX);
            f.finish();
            let m = f.peer.recv().await;
            let message::Return(r) = m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(r.unwrap().get_answer_id(), 2);
            settle().await;
            f.drain();
            assert!(!f.alive.get());
            assert_eq!(f.returns, 0);
        })
        .await;
}

async fn queued_streaming_trace(steps: &[serde_json::Value], fail: bool, wrapper: u32) {
    let mut f = Fixture::new(0).await;
    let (tx, rx) = futures::channel::oneshot::channel();
    let mut resolver = Some(tx);
    let queued: harness::Client = capnp_rpc::new_future_client(async move { rx.await.unwrap() });
    let mut client = Some(match wrapper {
        0 => queued,
        1 => {
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
            capnp_rpc::membrane::Membrane::new(Rc::new(Permit)).export(queued)
        }
        2 => {
            capnp_rpc::auto_reconnect(move || Ok(queued.clone()))
                .unwrap()
                .0
        }
        _ => panic!(),
    });
    let mut promise = None;
    let mut ready = false;
    let mut question = None;
    let mut finished = false;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "send" => {
                let request: capnp::capability::StreamingRequest<harness::echo_params::Owned> =
                    client.as_ref().unwrap().client.new_streaming_call(
                        harness::Client::TYPE_ID,
                        0,
                        None,
                    );
                promise = Some(request.send());
            }
            "resolve" => {
                assert!(resolver.take().unwrap().send(Ok(f.remote.clone())).is_ok());
            }
            "drop" => {
                client.take();
            }
            "ack" => {
                f.peer.send(|m| {
                    let mut r = m.init_return();
                    r.set_answer_id(question.unwrap());
                    r.set_release_param_caps(false);
                    if fail {
                        let mut e = r.init_exception();
                        e.set_type(capnp_rpc::rpc_capnp::exception::Type::Failed);
                        e.set_reason("stream failed");
                    } else {
                        r.init_results();
                    }
                });
            }
            _ => panic!(),
        }
        for _ in 0..32 {
            if let Some(p) = promise.as_mut() {
                if let std::task::Poll::Ready(result) = futures::poll!(p) {
                    result.unwrap();
                    ready = true;
                    promise.take();
                }
            }
            tokio::task::yield_now().await;
        }
        while let Some(result) = f.peer.receive_incoming_message().now_or_never() {
            let Some(m) = result.unwrap() else { break };
            match m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Call(c) => {
                    assert!(question.is_none(), "stream was sent twice");
                    let c = c.unwrap();
                    assert_eq!(c.get_method_id(), 0);
                    question = Some(c.get_question_id());
                }
                message::Finish(fin) => {
                    assert_eq!(Some(fin.unwrap().get_question_id()), question);
                    finished = true;
                }
                message::Release(_) => (),
                _ => panic!("unexpected streaming traffic"),
            }
        }
        let state = step["state"].as_array().unwrap();
        assert_eq!(
            question.is_some(),
            state[0] == 1 && state[1] == 1,
            "send after resolution"
        );
        assert_eq!(
            ready,
            state[4] == 1,
            "wrapper {wrapper}, action {}: streaming readiness waits for credit, not acknowledgement", step["action"]
        );
        assert_eq!(
            finished,
            state[5] == 1,
            "acknowledgement owns the question after readiness"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_queued_streaming_traces() {
    let path = reproto_test_support::verification::input("REPROTO_QUEUED_STREAMING_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                for fail in [false, true] {
                    for wrapper in 0..3 {
                        queued_streaming_trace(&trace, fail, wrapper).await;
                    }
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn queued_streaming_preserves_credit_readiness_and_ack_lifetime() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let steps = serde_json::json!([
                    {"action":"send","state":[1,0,0,0,0,0]},
                    {"action":"drop","state":[1,0,1,0,0,0]},
                    {"action":"resolve","state":[1,1,1,0,1,0]},
                    {"action":"ack","state":[1,1,1,1,1,1]}
                ]);
                for wrapper in 0..3 {
                    queued_streaming_trace(steps.as_array().unwrap(), true, wrapper).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn generated_result_schemas_set_conservative_wire_hints() {
    tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for wrapper in 0..4 {
                let mut f = Fixture::new(wrapper).await;
                macro_rules! check {
                    ($method:ident, $id:expr, $hint:expr) => {{
                        let response = f.remote.$method().send();
                        let mut promise = Box::pin(response.promise);
                        let m = tokio::select! {
                            m = f.next_call() => m,
                            result = &mut promise => panic!("response before Call: {:?}", result.err()),
                        };
                        let message::Call(call) = m.get_body().unwrap().get_as::<message::Reader>().unwrap().which().unwrap() else { panic!() };
                        let call = call.unwrap();
                        assert_eq!(call.get_method_id(), $id);
                        assert_eq!(call.get_no_promise_pipelining(), $hint);
                        assert!(!call.get_only_promise_pipeline());
                        f.peer.send(|m| { let mut r = m.init_return(); r.set_answer_id(call.get_question_id()); r.init_results(); });
                        promise.await.unwrap();
                    }};
                }
                check!(echo_request, 0, true);
                check!(bounce_request, 1, false);
                check!(plain_cycle_request, 7, true);
                check!(cap_cycle_request, 8, false);
                check!(nested_lists_request, 9, false);
                check!(grouped_request, 10, false);
                check!(opaque_request, 11, false);
                // C++ conservatively treats unbound generic fields as AnyPointer.
                check!(generic_request, 12, false);
                let mut ready = Box::pin(f.remote.stream_request().send());
                let readiness = futures::poll!(&mut ready);
                let m = f.next_call().await;
                let message::Call(call) = m.get_body().unwrap().get_as::<message::Reader>().unwrap().which().unwrap() else { panic!() };
                let call = call.unwrap();
                assert_eq!(call.get_method_id(), 13);
                assert!(call.get_no_promise_pipelining());
                assert!(!call.get_only_promise_pipeline());
                f.peer.send(|m| { let mut r = m.init_return(); r.set_answer_id(call.get_question_id()); r.init_results(); });
                match readiness {
                    std::task::Poll::Ready(result) => result.unwrap(),
                    std::task::Poll::Pending => ready.await.unwrap(),
                }
            }
        }).await.unwrap();
    }).await;
}

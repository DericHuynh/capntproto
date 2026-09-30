#[allow(dead_code)]
mod support;
use capnp::{
    capability::{Client, Promise},
    dynamic_capability as dynamic, dynamic_value,
    traits::{Imbue, ImbueMut},
    Error,
};
use futures::channel::oneshot;
use reproto_test_support::{
    dynamic_test_capnp::{base, derived, envelope, other},
    runtime_test_capnp::harness,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
type Base = base::Client<harness::Owned>;
type Derived = derived::Client<harness::Owned>;
struct Echo;
struct OwnedEcho(Rc<Cell<bool>>);
impl Drop for OwnedEcho {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl harness::Server for OwnedEcho {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(42);
        Ok(())
    }
}
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(p.get()?.get_value() + 1);
        Ok(())
    }
}
#[derive(Default)]
struct State {
    calls: Cell<u32>,
    running: Cell<bool>,
    completed: Cell<bool>,
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    allow: bool,
    early: bool,
    fail: bool,
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}
struct DynamicServer(Rc<State>);
impl dynamic::Server for DynamicServer {
    fn get_schema(&self) -> capnp::schema::InterfaceSchema {
        Derived::schema()
    }
    fn allow_cancellation(&self) -> bool {
        self.0.allow
    }
    fn call(
        self: Rc<Self>,
        method: capnp::schema::Method,
        mut context: dynamic::CallContext,
    ) -> Promise<(), Error> {
        Promise::from_future(async move {
            self.0.calls.set(self.0.calls.get() + 1);
            let name = method.get_proto().get_name()?.to_str()?;
            match name {
                "transform" => {
                    let p = context.get_params()?;
                    let value = p.get_named("value")?.downcast::<u32>();
                    let cap = p.get_named("cap")?.downcast::<dynamic::Client>();
                    let mut r = context.get_results()?;
                    r.set_named("value", (value + 10).into())?;
                    r.set_named("cap", cap.into())?;
                }
                "pending" => {
                    let cap = context
                        .get_params()?
                        .get_named("cap")?
                        .downcast::<dynamic::Client>();
                    context.get_results()?.set_named("cap", cap.into())?;
                    context.release_params();
                    let retained = if self.0.early { None } else { Some(context) };
                    self.0.running.set(true);
                    let _running = Running(self.0.clone());
                    let gate = self.0.gate.borrow_mut().take().unwrap();
                    gate.await.map_err(|_| Error::failed("gate gone".into()))?;
                    self.0.completed.set(true);
                    drop(retained);
                    if self.0.fail {
                        return Err(Error::failed("dynamic method failed".into()));
                    }
                }
                "stream" => {
                    self.0.running.set(true);
                    let _running = Running(self.0.clone());
                    let gate = self.0.gate.borrow_mut().take().unwrap();
                    gate.await.map_err(|_| Error::failed("gate gone".into()))?;
                }
                _ => return Err(Error::unimplemented("test method".into())),
            }
            Ok(())
        })
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
async fn reflected_calls_preserve_generic_capabilities_inheritance_and_pipelines() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let state = Rc::new(State {
                    allow: true,
                    ..Default::default()
                });
                let server = capnp_rpc::new_dynamic_client(DynamicServer(state.clone()));
                let mut drivers = Drivers(vec![]);
                let client = if wire {
                    let hub = Rc::new(RefCell::new(support::Hub::default()));
                    let host = capnp_rpc::RpcSystem::new(
                        Box::new(support::Hub::network(&hub, 1)),
                        Some(server.as_client().unwrap().clone()),
                    );
                    let mut caller =
                        capnp_rpc::RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
                    let cap: Client = caller.bootstrap(1);
                    drivers.0.push(tokio::task::spawn_local(host));
                    drivers.0.push(tokio::task::spawn_local(caller));
                    dynamic::Client::new(cap, Derived::schema())
                } else {
                    server
                };
                assert!(Derived::schema().extends(Base::schema()).unwrap());
                assert!(client.upcast(other::Client::schema()).is_err());
                assert!(client
                    .new_request_for(
                        other::Client::schema().get_method_by_name("other").unwrap(),
                        None
                    )
                    .is_err());
                let upcast = client.upcast(Base::schema()).unwrap();
                assert!(upcast.new_request("own", None).is_err());
                let echo: harness::Client = capnp_rpc::new_client(Echo);
                let mut request = client.new_request("transform", None).unwrap();
                request
                    .get()
                    .unwrap()
                    .set_named("value", 32u32.into())
                    .unwrap();
                request
                    .get()
                    .unwrap()
                    .set_named("cap", dynamic::Client::from(echo.clone()).into())
                    .unwrap();
                let result = request.send();
                let dynamic::PipelineValue::Capability(pipeline) =
                    result.pipeline.get_by_name("cap").unwrap()
                else {
                    panic!()
                };
                let mut piped = pipeline.new_request("echo", None).unwrap();
                piped
                    .get()
                    .unwrap()
                    .set_named("value", 7u32.into())
                    .unwrap();
                assert_eq!(
                    piped
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_named("value")
                        .unwrap()
                        .downcast::<u32>(),
                    8
                );
                let response = result.promise.await.unwrap();
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_named("value")
                        .unwrap()
                        .downcast::<u32>(),
                    42
                );
                let returned = response
                    .get()
                    .unwrap()
                    .get_named("cap")
                    .unwrap()
                    .downcast::<dynamic::Client>();
                assert_eq!(
                    returned.get_schema().unwrap().get_proto().get_id(),
                    harness::Client::schema().get_proto().get_id()
                );
                let returned = capnp::capability::get_resolved_cap(
                    returned.cast::<harness::Client>().unwrap(),
                )
                .await;
                assert_eq!(returned.client.hook.get_ptr(), echo.client.hook.get_ptr());
                assert_eq!(state.calls.get(), 1);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn dynamic_struct_lists_groups_and_any_pointer_keep_real_capabilities() {
    let state = Rc::new(State {
        allow: true,
        ..Default::default()
    });
    let cap = capnp_rpc::new_dynamic_client(DynamicServer(state));
    let mut message = capnp::message::Builder::new_default();
    let mut table = capnp::private::layout::CapTable::new();
    {
        let mut root = message.init_root::<envelope::Builder>();
        root.imbue_mut(&mut table);
        let mut root =
            dynamic_value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
        root.set_named("cap", cap.clone().into()).unwrap();
        root.set_named("any", cap.clone().into()).unwrap();
        let mut group = root
            .reborrow()
            .get_named("body")
            .unwrap()
            .downcast::<capnp::dynamic_struct::Builder>();
        group.set_named("cap", cap.clone().into()).unwrap();
        let mut list = root
            .reborrow()
            .initn_named("caps", 2)
            .unwrap()
            .downcast::<capnp::dynamic_list::Builder>();
        list.set(0, cap.clone().into()).unwrap();
        list.set(1, cap.clone().into()).unwrap();
    }
    let mut root = message.get_root_as_reader::<envelope::Reader>().unwrap();
    root.imbue(&table);
    let root = dynamic_value::Reader::from(root).downcast::<capnp::dynamic_struct::Reader>();
    let held = root.get_named("cap").unwrap().downcast::<dynamic::Client>();
    let list = root
        .get_named("caps")
        .unwrap()
        .downcast::<capnp::dynamic_list::Reader>();
    for i in 0..2 {
        let got = list.get(i).unwrap().downcast::<dynamic::Client>();
        assert_eq!(
            got.as_client().unwrap().hook.get_ptr(),
            cap.as_client().unwrap().hook.get_ptr()
        );
        assert_eq!(
            got.get_schema().unwrap().get_proto().get_id(),
            Derived::schema().get_proto().get_id()
        );
    }
    drop((message, table, cap));
    assert!(held.new_request("transform", None).is_ok());
}

async fn call_trace(steps: &[serde_json::Value], allow: bool, early: bool, fail: bool, wire: bool) {
    let state = Rc::new(State {
        allow,
        early,
        fail,
        ..Default::default()
    });
    let alive = Rc::new(Cell::new(true));
    let cap: harness::Client = capnp_rpc::new_client(OwnedEcho(alive.clone()));
    let (tx, rx) = oneshot::channel();
    let mut tx = Some(tx);
    *state.gate.borrow_mut() = Some(rx);
    let (executor, driver) = capnp_rpc::new_call_executor();
    let server = capnp_rpc::new_dynamic_client_from_rc(
        Rc::new(DynamicServer(state.clone())),
        Some(executor),
    );
    let mut drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
    let client = if wire {
        let hub = Rc::new(RefCell::new(support::Hub::default()));
        let host = capnp_rpc::RpcSystem::new(
            Box::new(support::Hub::network(&hub, 1)),
            Some(server.as_client().unwrap().clone()),
        );
        let mut caller = capnp_rpc::RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
        let cap: Client = caller.bootstrap(1);
        drivers.0.push(tokio::task::spawn_local(host));
        drivers.0.push(tokio::task::spawn_local(caller));
        dynamic::Client::new(cap, Derived::schema())
    } else {
        server
    };
    let mut req = client.new_request("pending", None).unwrap();
    req.get()
        .unwrap()
        .set_named("cap", dynamic::Client::from(cap).into())
        .unwrap();
    let remote = req.send();
    drop(remote.pipeline);
    let mut promise = Some(remote.promise);
    assert!(futures::poll!(promise.as_mut().unwrap()).is_pending());
    settle().await;
    assert!(state.running.get());
    assert!(alive.get());
    let mut response: Option<dynamic::Response> = None;
    let mut held: Option<dynamic::Client> = None;
    let mut outcome = 0;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "drop-caller" => {
                promise.take();
                response.take();
            }
            "complete" => {
                tx.take().unwrap().send(()).unwrap();
            }
            "extract" => {
                held = Some(
                    response
                        .as_ref()
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_named("cap")
                        .unwrap()
                        .downcast::<dynamic::Client>(),
                );
            }
            "drop-held" => {
                held.take();
            }
            "use" => {
                let response = held
                    .as_ref()
                    .unwrap()
                    .new_request("echo", None)
                    .unwrap()
                    .send()
                    .promise
                    .await
                    .unwrap();
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_named("value")
                        .unwrap()
                        .downcast::<u32>(),
                    42
                );
            }
            _ => panic!(),
        }
        for _ in 0..64 {
            if let Some(p) = &mut promise {
                if let std::task::Poll::Ready(result) = futures::poll!(p) {
                    match result {
                        Ok(value) => {
                            response = Some(value);
                            outcome = 1;
                        }
                        Err(error) => {
                            assert!(error.extra.contains("dynamic method failed"));
                            outcome = 2;
                        }
                    }
                    promise.take();
                }
            }
            tokio::task::yield_now().await;
        }
        let e = step["state"].as_array().unwrap();
        assert_eq!(state.completed.get(), e[1] == 1, "completed: {step}");
        assert_eq!(
            state.running.get(),
            e[2] == 1,
            "running: {step}, allow={allow}, wire={wire}"
        );
        assert_eq!(
            alive.get(),
            e[3] == 1,
            "cap lifetime: {step}, early={early}, wire={wire}"
        );
        assert_eq!(held.is_some(), e[4] == 1);
        if e[0] == 1 && e[1] == 1 {
            assert_eq!(outcome, if fail { 2 } else { 1 });
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_dynamic_capability_traces() {
    let path = reproto_test_support::verification::input("REPROTO_DYNAMIC_CAPABILITY_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for wire in [false, true] {
                    call_trace(
                        case["steps"].as_array().unwrap(),
                        case["allow"].as_bool().unwrap(),
                        case["early"].as_bool().unwrap(),
                        case["fail"].as_bool().unwrap(),
                        wire,
                    )
                    .await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn dynamic_policy_ignores_schema_annotations_and_owned_reads_survive_response_drop() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let steps = serde_json::json!([
                    {"action":"complete","state":[1,1,0,1,0,0,0]},
                    {"action":"extract","state":[1,1,0,1,1,1,0]},
                    {"action":"drop-caller","state":[0,1,0,1,1,1,0]},
                    {"action":"use","state":[0,1,0,1,1,1,1]},
                    {"action":"drop-held","state":[0,1,0,0,0,1,1]}
                ]);
                call_trace(steps.as_array().unwrap(), false, true, false, wire).await;
                let steps = serde_json::json!([
                    {"action":"drop-caller","state":[0,0,1,1,0,0,0]},
                    {"action":"complete","state":[0,1,0,0,0,0,0]}
                ]);
                call_trace(steps.as_array().unwrap(), false, true, false, wire).await;
            }
        })
        .await;
}

struct Stored(Rc<Cell<bool>>);
impl Drop for Stored {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl base::Server<harness::Owned> for Stored {}
impl derived::Server<harness::Owned> for Stored {
    async fn own(
        self: Rc<Self>,
        _: derived::OwnParams<harness::Owned>,
        _: derived::OwnResults<harness::Owned>,
    ) -> capnp::Result<()> {
        Ok(())
    }
}
type Message = (
    capnp::message::Builder<capnp::message::HeapAllocator>,
    capnp::private::layout::CapTable,
);
fn store(cap: &dynamic::Client) -> Message {
    let mut message = capnp::message::Builder::new_default();
    let mut table = capnp::private::layout::CapTable::new();
    {
        let mut root = message.init_root::<envelope::Builder>();
        root.imbue_mut(&mut table);
        let mut root =
            dynamic_value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
        root.set_named("cap", cap.clone().into()).unwrap();
        root.set_named("any", cap.clone().into()).unwrap();
        root.reborrow()
            .get_named("body")
            .unwrap()
            .downcast::<capnp::dynamic_struct::Builder>()
            .set_named("cap", cap.clone().into())
            .unwrap();
        root.reborrow()
            .initn_named("caps", 1)
            .unwrap()
            .downcast::<capnp::dynamic_list::Builder>()
            .set(0, cap.clone().into())
            .unwrap();
    }
    (message, table)
}
fn read(message: &Message, location: u8) -> dynamic::Client {
    let mut root = message.0.get_root_as_reader::<envelope::Reader>().unwrap();
    root.imbue(&message.1);
    let root = dynamic_value::Reader::from(root).downcast::<capnp::dynamic_struct::Reader>();
    match location {
        0 => root.get_named("cap").unwrap().downcast(),
        1 => root
            .get_named("caps")
            .unwrap()
            .downcast::<capnp::dynamic_list::Reader>()
            .get(0)
            .unwrap()
            .downcast(),
        2 => root
            .get_named("body")
            .unwrap()
            .downcast::<capnp::dynamic_struct::Reader>()
            .get_named("cap")
            .unwrap()
            .downcast(),
        3 => root
            .get_named("any")
            .unwrap()
            .downcast::<capnp::any_pointer::Reader>()
            .get_as::<Derived>()
            .unwrap()
            .into(),
        _ => panic!(),
    }
}
async fn ownership_trace(steps: &[serde_json::Value], revocable: bool, location: u8) {
    let alive = Rc::new(Cell::new(true));
    let mut owner = if revocable {
        Some(capnp_rpc::RevocableServer::<Derived>::new(Stored(
            alive.clone(),
        )))
    } else {
        None
    };
    let mut root: Option<Derived> = Some(match &owner {
        Some(owner) => owner.get_client(),
        None => capnp_rpc::new_client(Stored(alive.clone())),
    });
    let identity = root.as_ref().unwrap().client.hook.get_ptr();
    let mut message = Some(store(&root.as_ref().unwrap().clone().into()));
    let mut value: Option<dynamic::Client> = None;
    let mut copy: Option<dynamic::Client> = None;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "read" => {
                value = Some(read(message.as_ref().unwrap(), location));
            }
            "clone" => {
                copy = value.clone();
            }
            "drop-root" => {
                root.take();
                owner.take();
            }
            "drop-message" => {
                message.take();
            }
            "drop-value" => {
                value.take();
            }
            "drop-copy" => {
                copy.take();
            }
            "revoke" => {
                owner.as_ref().unwrap().revoke();
            }
            _ => panic!(),
        }
        let e = step["state"].as_array().unwrap();
        assert_eq!(
            alive.get(),
            e[7] == 1,
            "ownership: {step}, revocable={revocable}, location={location}"
        );
        let probe = message.as_ref().map(|message| read(message, location));
        for cap in [value.as_ref(), copy.as_ref(), probe.as_ref()]
            .into_iter()
            .flatten()
        {
            assert_eq!(
                cap.as_client().unwrap().hook.get_ptr(),
                identity,
                "capability identity"
            );
            let result = cap
                .new_request("own", None)
                .unwrap()
                .send_ignoring_result()
                .await;
            assert_eq!(result.is_ok(), e[8] == 1, "authority: {step}");
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_dynamic_ownership_traces() {
    let path = reproto_test_support::verification::input("REPROTO_DYNAMIC_OWNERSHIP_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for case in cases {
        for location in 0..4 {
            ownership_trace(
                case["steps"].as_array().unwrap(),
                case["revocable"].as_bool().unwrap(),
                location,
            )
            .await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn dynamic_dispatch_checks_methods_and_preserves_server_identity() {
    let state = Rc::new(State {
        allow: true,
        ..Default::default()
    });
    let server = Rc::new(DynamicServer(state.clone()));
    let mut set = capnp_rpc::CapabilityServerSet::<DynamicServer, Client>::new();
    let raw = set.new_client_from_rc(server.clone());
    let client = capnp_rpc::new_dynamic_client_from_rc(server.clone(), None);
    assert_eq!(
        raw.hook.get_ptr(),
        client.as_client().unwrap().hook.get_ptr()
    );
    assert!(Rc::ptr_eq(
        &server,
        &set.get_local_server(&raw).await.unwrap()
    ));
    assert!(client
        .upcast(Base::schema())
        .unwrap()
        .cast::<Derived>()
        .is_err());
    assert!(client.cast::<other::Client>().is_err());
    assert!(client
        .new_request("transform", None)
        .unwrap()
        .send_streaming()
        .is_err());
    for (id, method) in [
        (other::Client::schema().get_proto().get_id(), 0),
        (Base::schema().get_proto().get_id(), u16::MAX),
    ] {
        let req: capnp::capability::Request<capnp::any_pointer::Owned, capnp::any_pointer::Owned> =
            raw.new_call(id, method, None);
        let error = req.send().promise.await.err().unwrap();
        assert_eq!(error.kind, capnp::ErrorKind::Unimplemented);
    }
    assert_eq!(state.calls.get(), 0);
    let wrong: harness::Client = capnp_rpc::new_client(Echo);
    let wrong = dynamic::Client::from(wrong);
    let mut msg = capnp::message::Builder::new_default();
    let mut table = capnp::private::layout::CapTable::new();
    let mut root = msg.init_root::<envelope::Builder>();
    root.imbue_mut(&mut table);
    let mut root = dynamic_value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
    assert!(root.set_named("cap", wrong.clone().into()).is_err());
    assert!(root
        .reborrow()
        .initn_named("caps", 1)
        .unwrap()
        .downcast::<capnp::dynamic_list::Builder>()
        .set(0, wrong.into())
        .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn reflected_stream_credit_keeps_work_and_orders_following_call() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let state = Rc::new(State {
                    allow: true,
                    ..Default::default()
                });
                let (tx, rx) = oneshot::channel();
                *state.gate.borrow_mut() = Some(rx);
                let server = capnp_rpc::new_dynamic_client(DynamicServer(state.clone()));
                let mut drivers = Drivers(vec![]);
                let client = if wire {
                    let hub = Rc::new(RefCell::new(support::Hub::default()));
                    let host = capnp_rpc::RpcSystem::new(
                        Box::new(support::Hub::network(&hub, 1)),
                        Some(server.as_client().unwrap().clone()),
                    );
                    let mut caller =
                        capnp_rpc::RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
                    let raw: Client = caller.bootstrap(1);
                    drivers.0.push(tokio::task::spawn_local(host));
                    drivers.0.push(tokio::task::spawn_local(caller));
                    dynamic::Client::new(raw, Derived::schema())
                } else {
                    server
                };
                let mut credit = client
                    .new_request("stream", None)
                    .unwrap()
                    .send_streaming()
                    .unwrap();
                // Direct local sends wait for completion; a network send returns
                // credit before acknowledgement and retains its own response waiter.
                let local_credit = if wire {
                    credit.await.unwrap();
                    None
                } else {
                    assert!(futures::poll!(&mut credit).is_pending());
                    Some(credit)
                };
                settle().await;
                assert!(state.running.get());
                let mut next = client
                    .new_request("transform", None)
                    .unwrap()
                    .send()
                    .promise;
                assert!(futures::poll!(&mut next).is_pending());
                settle().await;
                assert_eq!(state.calls.get(), 1);
                tx.send(()).unwrap();
                if let Some(credit) = local_credit {
                    credit.await.unwrap();
                }
                assert_eq!(
                    next.await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_named("value")
                        .unwrap()
                        .downcast::<u32>(),
                    10
                );
                assert_eq!(state.calls.get(), 2);
            }
        })
        .await;
}

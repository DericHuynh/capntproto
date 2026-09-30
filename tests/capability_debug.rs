#[allow(dead_code)]
mod support;

use capnp::{
    any_pointer,
    capability::{Client, FromClientHook, Promise},
    private::capability::{ClientHook, DebugInfo, ParamsHook, ResultsHook},
    Error,
};
use capnp_rpc::membrane::{Direction, Membrane, Policy};
use futures::{channel::oneshot, FutureExt};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Service;
impl harness::Server for Service {}
fn local() -> harness::Client {
    capnp_rpc::new_client(Service)
}
struct Boundary;
impl Policy for Boundary {
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        panic!("diagnostics must not invoke call policy")
    }
}

#[test]
fn typed_and_dynamic_clients_describe_current_hooks() {
    let client = local();
    let text = client.debug_info();
    assert!(text.starts_with("local:"));
    assert!(text.contains("Service"));
    assert_eq!(client.client.debug_info(), text);
    let dynamic: capnp::dynamic_capability::Client = client.into();
    assert_eq!(dynamic.debug_info(), text);
    assert_eq!(
        capnp::dynamic_capability::Client::null(None).debug_info(),
        "null"
    );
}

#[test]
fn diagnostics_do_not_drive_ready_promises_or_policy_revocation() {
    let polls = Rc::new(Cell::new(0));
    let count = polls.clone();
    let client: harness::Client = capnp_rpc::new_future_client(async move {
        count.set(count.get() + 1);
        Err(Error::failed("rejected target".into()))
    });
    for _ in 0..3 {
        assert_eq!(client.debug_info(), "promise");
    }
    assert_eq!(polls.get(), 0);
    assert!(client
        .client
        .when_resolved()
        .now_or_never()
        .unwrap()
        .is_err());
    assert_eq!(polls.get(), 1);
    assert!(client.debug_info().starts_with("resolved:broken:"));
    assert!(client.debug_info().contains("rejected target"));

    struct Revoking(Rc<Cell<u32>>, RefCell<Option<oneshot::Receiver<()>>>);
    impl Policy for Revoking {
        fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
            unreachable!()
        }
        fn on_revoked(&self) -> Option<Promise<(), Error>> {
            let count = self.0.clone();
            let signal = self.1.borrow_mut().take().unwrap();
            Some(Promise::from_future(async move {
                signal.await.unwrap();
                count.set(count.get() + 1);
                Err(Error::failed("policy revoked".into()))
            }))
        }
    }
    let count = Rc::new(Cell::new(0));
    let (tx, rx) = oneshot::channel();
    let membrane = Membrane::new(Rc::new(Revoking(count.clone(), RefCell::new(Some(rx)))));
    let wrapped = membrane.export(local());
    tx.send(()).unwrap();
    assert!(wrapped.debug_info().contains("Revoking:local:"));
    assert_eq!(count.get(), 0);
    assert!(membrane.when_revoked().now_or_never().unwrap().is_err());
    assert_eq!(count.get(), 1);
    assert!(wrapped.debug_info().contains("Revoking:broken:"));
}

#[test]
fn diagnostics_preserve_membrane_directions_identity_and_server_lifetime() {
    let server = Rc::new(Service);
    let weak = Rc::downgrade(&server);
    let client: harness::Client = capnp_rpc::new_client_from_rc(server);
    let identity = client.as_client_hook().get_ptr();
    let membrane = Membrane::new(Rc::new(Boundary));
    let wrapped = membrane.export(client.clone());
    let wrapped_id = wrapped.as_client_hook().get_ptr();
    let description = wrapped.debug_info();
    assert!(description.contains("Boundary:local:"));
    assert_eq!(wrapped.as_client_hook().get_ptr(), wrapped_id);
    let returned = membrane.import(wrapped);
    assert_eq!(returned.as_client_hook().get_ptr(), identity);
    assert_eq!(returned.debug_info(), client.debug_info());
    let reverse = membrane.import(client);
    assert!(reverse.debug_info().contains("Boundary:local:"));
    drop((returned, reverse, membrane));
    assert!(
        weak.upgrade().is_none(),
        "diagnostic text retained the server"
    );
    assert!(description.contains("Service"));
}

#[test]
fn revoked_server_reports_first_error_without_restoring_authority() {
    let owner = capnp_rpc::RevocableServer::<harness::Client>::new(Service);
    let client = owner.get_client();
    assert!(client.debug_info().starts_with("local:"));
    owner.revoke_with_error(Error::failed("first reason".into()));
    owner.revoke_with_error(Error::failed("second reason".into()));
    assert!(client.debug_info().starts_with("broken:"));
    assert!(client.debug_info().contains("first reason"));
    assert!(!client.debug_info().contains("second reason"));
    assert!(client
        .echo_request()
        .send()
        .promise
        .now_or_never()
        .unwrap()
        .is_err());
}

// Opaque hooks inherit a useful type name. Every unrelated hook operation is a
// tripwire: diagnostic traversal must not call get_resolved(), get_ptr(), etc.
struct Opaque;
impl ClientHook for Opaque {
    fn add_ref(&self) -> Box<dyn ClientHook> {
        panic!("unexpected clone")
    }
    fn new_call(
        &self,
        _: u64,
        _: u16,
        _: Option<capnp::MessageSize>,
    ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        unreachable!()
    }
    fn call(
        &self,
        _: u64,
        _: u16,
        _: Box<dyn ParamsHook>,
        _: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        unreachable!()
    }
    fn get_brand(&self) -> usize {
        unreachable!()
    }
    fn get_ptr(&self) -> usize {
        unreachable!()
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        unreachable!()
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        unreachable!()
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        unreachable!()
    }
}

#[test]
fn default_hook_description_is_pure() {
    let client = Client::new(Box::new(Opaque));
    assert_eq!(client.debug_info(), std::any::type_name::<Opaque>());
}

#[test]
fn diagnostics_can_be_used_inside_a_reconnect_callback() {
    let slot = Rc::new(RefCell::new(None::<harness::Client>));
    let observed = Rc::new(RefCell::new(String::new()));
    let weak = Rc::downgrade(&slot);
    let text = observed.clone();
    let (client, _) = capnp_rpc::lazy_auto_reconnect(move || {
        *text.borrow_mut() = weak
            .upgrade()
            .unwrap()
            .borrow()
            .as_ref()
            .unwrap()
            .debug_info();
        Ok(local())
    });
    *slot.borrow_mut() = Some(client.clone());
    assert_eq!(client.debug_info(), "reconnect:disconnected");
    drop(client.echo_request());
    assert_eq!(&*observed.borrow(), "reconnect:busy");
    assert!(client.debug_info().starts_with("reconnect:local:"));
}

#[test]
fn shortened_servers_show_the_target_only_after_resolution_is_driven() {
    use capnp::capability::ServerHooks;
    struct Shortening(RefCell<Option<Promise<Client, Error>>>);
    impl ServerHooks for Shortening {
        fn shorten_path(&self) -> Option<Promise<Client, Error>> {
            self.0.borrow_mut().take()
        }
    }
    impl harness::Server for Shortening {
        fn _capnp_server_hooks(&self) -> Option<&dyn ServerHooks> {
            Some(self)
        }
    }
    let (tx, rx) = oneshot::channel();
    let client: harness::Client = capnp_rpc::new_client(Shortening(RefCell::new(Some(
        Promise::from_future(async move { rx.await.unwrap() }),
    ))));
    let initial = client.debug_info();
    assert!(initial.starts_with("local:"));
    assert!(tx.send(Ok(local().client)).is_ok());
    assert_eq!(client.debug_info(), initial);
    client
        .client
        .when_resolved()
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(client.debug_info().starts_with("shortened:local:"));
}

#[tokio::test(flavor = "current_thread")]
async fn rpc_promise_pipeline_and_import_descriptions_do_not_send_messages() {
    use capnp_rpc::RpcSystem;
    use support::Hub;
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let (tx, rx) = oneshot::channel();
                let offered: harness::Client =
                    capnp_rpc::new_future_client(async move { Ok(rx.await.unwrap()) });
                let server = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(offered.client));
                let mut system = RpcSystem::new(Box::new(Hub::network(&hub, 0)), None);
                let remote: harness::Client = system.bootstrap(1);
                let status = hub.borrow_mut().connect(0, 1).status();
                let sent = status.borrow().sent;
                assert_eq!(remote.debug_info(), "rpcPromise:rpcPipeline");
                assert_eq!(status.borrow().sent, sent);
                let tasks = [
                    tokio::task::spawn_local(server),
                    tokio::task::spawn_local(system),
                ];
                for _ in 0..40 {
                    tokio::task::yield_now().await;
                }
                let sent = status.borrow().sent;
                assert!(remote.debug_info().contains("rpcPromise:rpcImport"));
                assert_eq!(status.borrow().sent, sent);
                assert!(tx.send(local()).is_ok());
                remote.client.when_resolved().await.unwrap();
                let import = capnp::capability::get_resolved_cap(remote.clone()).await;
                let sent = status.borrow().sent;
                assert_eq!(import.debug_info(), "rpcImport");
                assert!(remote.debug_info().starts_with("rpcPromise:"));
                assert_eq!(status.borrow().sent, sent);
                for task in tasks {
                    task.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[test]
fn cyclic_and_deep_diagnostics_are_bounded() {
    struct Cycle;
    impl ClientHook for Cycle {
        fn debug_info(&self, chain: &mut DebugInfo) {
            chain.push("cycle");
            chain.follow(self);
        }
        fn add_ref(&self) -> Box<dyn ClientHook> {
            unreachable!()
        }
        fn new_call(
            &self,
            _: u64,
            _: u16,
            _: Option<capnp::MessageSize>,
        ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
            unreachable!()
        }
        fn call(
            &self,
            _: u64,
            _: u16,
            _: Box<dyn ParamsHook>,
            _: Box<dyn ResultsHook>,
        ) -> Promise<(), Error> {
            unreachable!()
        }
        fn get_brand(&self) -> usize {
            unreachable!()
        }
        fn get_ptr(&self) -> usize {
            unreachable!()
        }
        fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
            unreachable!()
        }
        fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
            unreachable!()
        }
        fn when_resolved(&self) -> Promise<(), Error> {
            unreachable!()
        }
    }
    let text = Client::new(Box::new(Cycle)).debug_info();
    assert_eq!(text, "cycle:".repeat(64) + "truncated");
    let mut client = local();
    for _ in 0..100 {
        client = Membrane::new(Rc::new(Boundary)).export(client);
    }
    assert!(client.debug_info().ends_with(":truncated"));
}

#[test]
fn tlc_diagnostics_replay_without_changing_resolution_or_connection_state() {
    use reproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcDebugInfo.tla";
    const CONFIG: &str = include_str!("../verification/RpcDebugInfo.cfg");
    for path in traces(MODEL, "capability-debug", CONFIG).unwrap() {
        let (tx, rx) = oneshot::channel::<capnp::Result<harness::Client>>();
        let mut tx = Some(tx);
        let polls = Rc::new(Cell::new(0));
        let count = polls.clone();
        let promise: harness::Client = capnp_rpc::new_future_client(async move {
            count.set(count.get() + 1);
            rx.await.unwrap()
        });
        let membrane = Membrane::new(Rc::new(Boundary));
        let wrapped = membrane.export(promise.clone());
        let connections = Rc::new(Cell::new(0));
        let count = connections.clone();
        let (reconnect, control) = capnp_rpc::lazy_auto_reconnect(move || {
            count.set(count.get() + 1);
            Ok(local())
        });
        for state in path {
            match state["event"] {
                1 => {
                    assert!(tx.take().unwrap().send(Ok(local())).is_ok());
                }
                2 => {
                    assert!(tx
                        .take()
                        .unwrap()
                        .send(Err(Error::failed("rejected target".into())))
                        .is_ok());
                }
                3 => {
                    let result = promise.client.when_resolved().now_or_never().unwrap();
                    assert_eq!(result.is_ok(), state["offered"] == 1);
                }
                4 => membrane.revoke(Error::failed("boundary revoked".into())),
                5 => drop(reconnect.echo_request()),
                6 => control.reset(),
                7 => {
                    let text = wrapped.debug_info();
                    let prefix = format!("{}:", std::any::type_name::<Boundary>());
                    let text = text
                        .strip_prefix(&prefix)
                        .expect("membrane must remain visible");
                    let code = if text == "promise" {
                        10
                    } else if text.starts_with("resolved:local:") {
                        20
                    } else if text.starts_with("resolved:broken:")
                        && text.contains("rejected target")
                    {
                        21
                    } else if text.starts_with("broken:") && text.contains("boundary revoked") {
                        30
                    } else {
                        panic!("unexpected description: {text}, {state:?}")
                    };
                    assert_eq!(code, state["diag"]);
                    let text = reconnect.debug_info();
                    if state["connDiag"] == 0 {
                        assert_eq!(text, "reconnect:disconnected");
                    } else {
                        assert!(text.starts_with("reconnect:local:"));
                    }
                }
                _ => panic!("{state:?}"),
            }
            assert_eq!(polls.get(), state["driven"], "{state:?}");
            assert_eq!(connections.get(), state["connects"], "{state:?}");
        }
    }
    controls(
        MODEL,
        "capability-debug",
        CONFIG,
        &[
            ("drivePromise", "ResolutionRequiresDrive"),
            ("lazyConnect", "NoConnectOnInspect"),
            ("stripMembrane", "WrapperVisible"),
            ("hideError", "AccurateDescription"),
        ],
        None,
    )
    .unwrap();
}

fn normalized(client: &harness::Client) -> String {
    let description = client.debug_info();
    let mut text = description.as_str();
    let mut result = String::new();
    let policy = format!("{}:", std::any::type_name::<Boundary>());
    loop {
        if let Some(rest) = text.strip_prefix(&policy) {
            result += "boundary:";
            text = rest;
        } else if let Some(rest) = text.strip_prefix("resolved:") {
            result += "resolved:";
            text = rest;
        } else if text.starts_with("local:") {
            assert!(text.contains("Service"));
            return result + "local";
        } else if text.starts_with("broken:") {
            assert!(text.contains("reason"));
            return result + "broken";
        } else {
            assert_eq!(text, "promise");
            return result + text;
        }
    }
}

#[test]
fn wrapper_order_resolution_and_revocation_match_pinned_cpp() {
    use reproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("capability-debug");
    let logs = root().join("target/verification/capability-debug-cpp");
    let mut compile = command("g++");
    compile
        .args([
            "-std=c++23",
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/capability-debug.c++",
        ])
        .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
        .arg(build.join("c++/src/capnp/libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(&executable);
    run(&mut compile, &logs.join("compile.log"), 0).unwrap();
    let reference = run(&mut command(&executable), &logs.join("reference.log"), 0).unwrap();
    let mut observed = String::new();
    for depth in 0..=3 {
        for reject in [false, true] {
            let owner = capnp_rpc::RevocableServer::<harness::Client>::new(Service);
            let (tx, rx) = oneshot::channel();
            let promise: harness::Client =
                capnp_rpc::new_future_client(async move { rx.await.unwrap() });
            let mut wrapped = promise.clone();
            for _ in 0..depth {
                wrapped = Membrane::new(Rc::new(Boundary)).export(wrapped);
            }
            let mut observe = |phase| {
                observed += &format!(
                    "{depth} {} {phase} {}\n",
                    u8::from(reject),
                    normalized(&wrapped)
                );
            };
            observe(0);
            assert!(tx
                .send(if reject {
                    Err(Error::failed("reason".into()))
                } else {
                    Ok(owner.get_client())
                })
                .is_ok());
            observe(1);
            assert_eq!(
                promise
                    .client
                    .when_resolved()
                    .now_or_never()
                    .unwrap()
                    .is_err(),
                reject
            );
            observe(2);
            owner.revoke_with_error(Error::failed("reason".into()));
            observe(3);
        }
    }
    assert_eq!(observed, reference);
    eprintln!(
        "{} diagnostic observations match pinned C++",
        observed.lines().count()
    );
}

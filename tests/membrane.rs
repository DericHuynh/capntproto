#[allow(dead_code)]
mod support;
use capnp::capability::{Client, FromClientHook};
use capnp_rpc::{
    membrane::{Direction, Membrane, Policy},
    RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::channel::oneshot;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use support::Hub;
struct Echo(u32, Rc<Cell<u32>>);
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.1.set(self.1.get() + 1);
        r.get().set_value(self.0);
        Ok(())
    }
}
struct Root {
    crossings: Cell<u32>,
}
impl Policy for Root {
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }
    fn export_external(&self, cap: Client, _: &dyn Policy, _: &dyn Policy) -> Client {
        self.crossings.set(self.crossings.get() + 1);
        cap
    }
    fn import_internal(&self, cap: Client, _: &dyn Policy, _: &dyn Policy) -> Client {
        self.crossings.set(self.crossings.get() + 1);
        cap
    }
}
struct Redirect {
    signal: RefCell<Option<capnp::capability::Promise<(), capnp::Error>>>,
    root: Rc<Root>,
    redirect: harness::Client,
}
impl Policy for Redirect {
    fn on_revoked(&self) -> Option<capnp::capability::Promise<(), capnp::Error>> {
        self.signal.borrow_mut().take()
    }
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        Ok(Some(self.redirect.client.clone()))
    }
    fn root_policy(&self) -> Option<Rc<dyn Policy>> {
        Some(self.root.clone())
    }
    fn should_resolve_before_redirecting(&self) -> bool {
        true
    }
}
struct Fixture {
    client: harness::Client,
    membrane: Membrane,
    related: Membrane,
    original: harness::Client,
    resolver: Option<oneshot::Sender<harness::Client>>,
    revoker: Option<oneshot::Sender<()>>,
    pending: Option<tokio::task::JoinHandle<capnp::Result<u32>>>,
    result: u32,
    outside_calls: Rc<Cell<u32>>,
    redirect_calls: Rc<Cell<u32>>,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(task) = &self.pending {
            task.abort();
        }
    }
}
impl Fixture {
    fn new(policy_revocation: bool) -> Self {
        let (revoke_tx, revoke_rx) = oneshot::channel();
        let signal = policy_revocation.then(|| {
            capnp::capability::Promise::from_future(async move {
                let _ = revoke_rx.await;
                Err(capnp::Error::failed("membrane revoked".into()))
            })
        });
        let hub = Rc::new(RefCell::new(Hub::default()));
        let outside_calls = Rc::new(Cell::new(0));
        let service: harness::Client = capnp_rpc::new_client(Echo(1, outside_calls.clone()));
        let server = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(service.client));
        let mut caller = RpcSystem::new(Box::new(Hub::network(&hub, 0)), None);
        let original = caller.bootstrap(1);
        let mut tasks = vec![
            tokio::task::spawn_local(server),
            tokio::task::spawn_local(caller),
        ];
        let redirect_calls = Rc::new(Cell::new(0));
        let redirect: harness::Client = capnp_rpc::new_client(Echo(2, redirect_calls.clone()));
        let root = Rc::new(Root {
            crossings: Cell::new(0),
        });
        let policy = Rc::new(Redirect {
            signal: RefCell::new(signal),
            root: root.clone(),
            redirect: redirect.clone(),
        });
        let membrane = Membrane::new(policy);
        if policy_revocation {
            tasks.push(tokio::task::spawn_local(membrane.when_revoked()));
        }
        let related = Membrane::new(Rc::new(Redirect {
            signal: RefCell::new(None),
            root,
            redirect,
        }));
        let (tx, rx) = oneshot::channel();
        let promise: harness::Client =
            capnp_rpc::new_future_client(async move { Ok(rx.await.unwrap()) });
        let client = membrane.export(promise);
        Self {
            client,
            membrane,
            related,
            original,
            resolver: Some(tx),
            revoker: policy_revocation.then_some(revoke_tx),
            pending: None,
            result: 0,
            outside_calls,
            redirect_calls,
            tasks,
        }
    }
    async fn replay(&mut self, steps: &[serde_json::Value], reflect: bool) {
        for step in steps {
            match step["action"].as_str().unwrap() {
                "call" => {
                    let promise = self.client.echo_request().send().promise;
                    self.pending = Some(tokio::task::spawn_local(async move {
                        Ok(promise.await?.get()?.get_value())
                    }));
                }
                "resolve" => {
                    let cap = if reflect {
                        self.related.import(self.original.clone())
                    } else {
                        self.original.clone()
                    };
                    let delivered = self.resolver.take().unwrap().send(cap).is_ok();
                    assert!(
                        delivered || step["state"][2] == 1,
                        "only revocation may release the unresolved target"
                    );
                }
                "observe" => self.client.client.hook.when_resolved().await.unwrap(),
                "revoke" => {
                    if let Some(revoker) = self.revoker.take() {
                        let _ = revoker.send(());
                    } else {
                        self.membrane
                            .revoke(capnp::Error::failed("membrane revoked".into()));
                    }
                }
                other => panic!("{other}"),
            }
            for _ in 0..64 {
                tokio::task::yield_now().await;
            }
            if self.pending.as_ref().is_some_and(|p| p.is_finished()) {
                self.result = match self.pending.take().unwrap().await.unwrap() {
                    Ok(n) => n,
                    Err(e) => {
                        assert!(e.extra.contains("revoked"));
                        3
                    }
                };
            }
            let expected = step["state"][3].as_u64().unwrap() as u32;
            assert_eq!(self.result, expected, "{step}");
            assert_eq!(
                self.outside_calls.get(),
                u32::from(expected == 1),
                "outside authority at {step}"
            );
            assert_eq!(
                self.redirect_calls.get(),
                u32::from(expected == 2),
                "redirect authority at {step}"
            );
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_membrane_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_MEMBRANE_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for policy_revocation in [false, true] {
                    Fixture::new(policy_revocation)
                        .replay(
                            case["steps"].as_array().unwrap(),
                            case["reflect"].as_bool().unwrap(),
                        )
                        .await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn shared_and_related_policies_preserve_reverse_identity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = Rc::new(Root {
                crossings: Cell::new(0),
            });
            let cap: harness::Client = capnp_rpc::new_client(Echo(9, Rc::new(Cell::new(0))));
            let first = Membrane::new(root.clone());
            let second = Membrane::new(root.clone());
            let a = first.export(cap.clone());
            let b = second.export(cap.clone());
            assert_eq!(a.as_client_hook().get_ptr(), b.as_client_hook().get_ptr());
            assert_eq!(
                second.import(a).as_client_hook().get_ptr(),
                cap.as_client_hook().get_ptr()
            );
            assert_eq!(root.crossings.get(), 1);
            let child = Membrane::new(Rc::new(Redirect {
                signal: RefCell::new(None),
                root: root.clone(),
                redirect: cap.clone(),
            }));
            assert_eq!(
                child
                    .import(first.export(cap.clone()))
                    .as_client_hook()
                    .get_ptr(),
                cap.as_client_hook().get_ptr()
            );
            assert_eq!(root.crossings.get(), 2);
            first.revoke(capnp::Error::failed("shared revocation".into()));
            assert!(b.echo_request().send().promise.await.is_err());
        })
        .await;
}

struct Recorded {
    effects: Rc<Cell<u32>>,
    drops: Rc<Cell<u32>>,
}
impl Drop for Recorded {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
impl harness::Server for Recorded {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.effects.set(self.effects.get() + 1);
        r.get().set_value(1);
        Ok(())
    }
}
struct RevokedPolicy {
    signal: RefCell<Option<capnp::capability::Promise<(), capnp::Error>>>,
    reason: &'static str,
    allow_redirect: bool,
    calls: Rc<Cell<u32>>,
    broken: Rc<Cell<bool>>,
    redirect: harness::Client,
}
impl Policy for RevokedPolicy {
    fn on_revoked(&self) -> Option<capnp::capability::Promise<(), capnp::Error>> {
        self.signal.borrow_mut().take()
    }
    fn call(&self, _: Direction, _: u64, _: u16, target: &Client) -> capnp::Result<Option<Client>> {
        use futures::FutureExt;
        self.calls.set(self.calls.get() + 1);
        let broken = target
            .hook
            .when_more_resolved()
            .map(|promise| {
                let error = promise
                    .now_or_never()
                    .expect("local broken target must resolve immediately")
                    .err()
                    .expect("broken target must reject");
                assert_eq!(error.extra, self.reason);
                true
            })
            .unwrap_or(false);
        self.broken.set(broken);
        Ok((broken && self.allow_redirect).then(|| self.redirect.client.clone()))
    }
}
async fn revoked_policy_trace(steps: &[serde_json::Value], redirect: bool, mode: u32) {
    let reason = if mode == 2 {
        "on_revoked() completed successfully; expected an error"
    } else {
        "revoked by owner"
    };
    let (sender, receiver) = oneshot::channel();
    let mut sender = Some(sender);
    let signal = (mode != 0).then(|| {
        capnp::capability::Promise::from_future(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(capnp::Error::failed("test ended".into())))
        })
    });
    let effects = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let broken = Rc::new(Cell::new(false));
    let redirected = Rc::new(Cell::new(0));
    let alternate: harness::Client = capnp_rpc::new_client(Echo(2, redirected.clone()));
    let membrane = Membrane::new(Rc::new(RevokedPolicy {
        signal: RefCell::new(signal),
        reason,
        allow_redirect: redirect,
        calls: calls.clone(),
        broken: broken.clone(),
        redirect: alternate,
    }));
    let mut driver = (mode != 0).then(|| tokio::task::spawn_local(membrane.when_revoked()));
    let make = || {
        let cap: harness::Client = capnp_rpc::new_client(Recorded {
            effects: effects.clone(),
            drops: drops.clone(),
        });
        membrane.export(cap)
    };
    let mut cap = make();
    let mut result = 0;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "revoke" => {
                if mode == 0 {
                    membrane.revoke(capnp::Error::failed(reason.into()));
                } else {
                    let outcome = if mode == 2 {
                        Ok(())
                    } else {
                        Err(capnp::Error::failed(reason.into()))
                    };
                    sender.take().unwrap().send(outcome).unwrap();
                    assert_eq!(
                        driver.take().unwrap().await.unwrap().unwrap_err().extra,
                        reason
                    );
                }
            }
            "fresh" => cap = make(),
            "call" => {
                result = match cap.echo_request().send().promise.await {
                    Ok(response) => response.get().unwrap().get_value(),
                    Err(error) => {
                        assert_eq!(error.extra, reason);
                        3
                    }
                };
            }
            _ => panic!(),
        }
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
        assert_eq!(result, state[3], "{step}");
        assert_eq!(calls.get(), state[4], "policy at {step}");
        assert_eq!(broken.get(), state[5] != 0, "target at {step}");
        assert_eq!(
            effects.get(),
            u32::from(result == 1),
            "old authority at {step}"
        );
        assert_eq!(
            redirected.get(),
            u32::from(result == 2),
            "redirect authority at {step}"
        );
        assert_eq!(
            drops.get(),
            if state[0] != 0 { 1 + state[2] } else { 0 },
            "retained target at {step}"
        );
    }
    if let Some(driver) = driver {
        driver.abort();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_revoked_policy_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_REVOKED_POLICY_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for mode in 0..3 {
                    revoked_policy_trace(
                        case["steps"].as_array().unwrap(),
                        case["redirect"].as_bool().unwrap(),
                        mode,
                    )
                    .await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn policy_can_redirect_calls_after_revocation_without_retaining_target() {
    let steps = serde_json::json!([
        {"action":"revoke","state":[1,0,0,0,0,0,0]},
        {"action":"fresh","state":[1,0,1,0,0,0,0]},
        {"action":"call","state":[1,1,1,2,1,1,1]}
    ]);
    tokio::task::LocalSet::new()
        .run_until(revoked_policy_trace(steps.as_array().unwrap(), true, 0))
        .await;
}

struct Transform {
    custom: bool,
    imports: Cell<u32>,
    exports: Cell<u32>,
    inside: harness::Client,
    outside: harness::Client,
}
impl Policy for Transform {
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }
    fn import_external(&self, _: &Client) -> capnp::Result<Option<Client>> {
        self.imports.set(self.imports.get() + 1);
        Ok(self.custom.then(|| self.inside.client.clone()))
    }
    fn export_internal(&self, _: &Client) -> capnp::Result<Option<Client>> {
        self.exports.set(self.exports.get() + 1);
        Ok(self.custom.then(|| self.outside.client.clone()))
    }
}
async fn transform_trace(steps: &[serde_json::Value], custom: bool, reverse: bool) {
    let effects: [Rc<Cell<u32>>; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
    let original: harness::Client = capnp_rpc::new_client(Echo(1, effects[0].clone()));
    let outside: harness::Client = capnp_rpc::new_client(Echo(2, effects[1].clone()));
    let inside: harness::Client = capnp_rpc::new_client(Echo(3, effects[2].clone()));
    let policy = Rc::new(Transform {
        custom,
        imports: Cell::new(0),
        exports: Cell::new(0),
        inside,
        outside,
    });
    let membrane = Membrane::new(policy.clone());
    let cross = |cap: harness::Client, rev: bool| {
        if rev {
            membrane.import(cap)
        } else {
            membrane.export(cap)
        }
    };
    let mut cap = None;
    let mut result = 0;
    for step in steps {
        let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
        match step["action"].as_str().unwrap() {
            "cross" => {
                let c = cross(original.clone(), reverse);
                if state[2] == 0 {
                    let cached = cross(original.clone(), reverse);
                    assert_eq!(
                        c.as_client_hook().get_ptr(),
                        cached.as_client_hook().get_ptr()
                    );
                }
                cap = Some(c);
            }
            "return" => {
                let c = cross(cap.take().unwrap(), !reverse);
                if !custom && state[2] == 0 {
                    assert_eq!(
                        c.as_client_hook().get_ptr(),
                        original.as_client_hook().get_ptr()
                    );
                }
                cap = Some(c);
            }
            "revoke" => membrane.revoke(capnp::Error::failed("custom revoked".into())),
            "use" => {
                result = match cap.as_ref().unwrap().echo_request().send().promise.await {
                    Ok(r) => r.get().unwrap().get_value(),
                    Err(e) => {
                        assert_eq!(e.extra, "custom revoked");
                        9
                    }
                }
            }
            _ => panic!(),
        }
        assert_eq!(result, state[5], "{step}");
        let first = state[0];
        let second = if custom { state[1] } else { 0 };
        assert_eq!(
            policy.imports.get(),
            if reverse { first } else { second },
            "imports at {step}"
        );
        assert_eq!(
            policy.exports.get(),
            if reverse { second } else { first },
            "exports at {step}"
        );
        for (i, effect) in effects.iter().enumerate() {
            assert_eq!(
                effect.get(),
                u32::from(result == i as u32 + 1),
                "authority at {step}"
            );
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_membrane_transform_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_MEMBRANE_TRANSFORM_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                transform_trace(
                    case["steps"].as_array().unwrap(),
                    case["custom"].as_bool().unwrap(),
                    case["reverse"].as_bool().unwrap(),
                )
                .await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn custom_crossings_preserve_substitution_authority_after_revocation() {
    let steps = serde_json::json!([
        {"action":"cross","state":[1,0,0,0,0,0,0]},
        {"action":"return","state":[1,1,0,0,0,0,0]},
        {"action":"revoke","state":[1,1,1,0,0,0,0]},
        {"action":"use","state":[1,1,1,1,0,3,3]}
    ]);
    tokio::task::LocalSet::new()
        .run_until(transform_trace(steps.as_array().unwrap(), true, false))
        .await;
}

struct SignalOnly(RefCell<Option<capnp::capability::Promise<(), capnp::Error>>>);
impl Policy for SignalOnly {
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }
    fn on_revoked(&self) -> Option<capnp::capability::Promise<(), capnp::Error>> {
        self.0.borrow_mut().take()
    }
}
#[tokio::test(flavor = "current_thread")]
async fn pending_call_drives_policy_revocation_without_background_driver() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (tx, rx) = oneshot::channel::<()>();
            let policy = SignalOnly(RefCell::new(Some(capnp::capability::Promise::from_future(
                async move {
                    rx.await.unwrap();
                    Err(capnp::Error::failed("asynchronous revocation".into()))
                },
            ))));
            let membrane = Membrane::new(Rc::new(policy));
            let cap: harness::Client = capnp_rpc::new_future_client(futures::future::pending());
            let cap = membrane.export(cap);
            let promise = cap.echo_request().send().promise;
            let call = tokio::task::spawn_local(promise);
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            assert!(!call.is_finished());
            tx.send(()).unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), call)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.err().unwrap().extra, "asynchronous revocation");
        })
        .await;
}

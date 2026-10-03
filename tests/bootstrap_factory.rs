mod support;
use capnp::{capability::Client, traits::HasTypeId};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, return_},
    BootstrapFactory, RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::{Endpoint, Hub};

struct Grant(u32, Rc<RefCell<Vec<u32>>>);
impl harness::Server for Grant {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(self.0);
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        mut r: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = p.get()?.get_cap()?;
        let observed = value(&cap).await;
        self.1.borrow_mut().push(observed);
        r.get().set_cap(cap);
        Ok(())
    }
}
#[derive(Default)]
struct Factory {
    epoch: Cell<u32>,
    base: u32,
    delegated: Rc<RefCell<Vec<u32>>>,
    calls: RefCell<Vec<(u8, u32)>>,
}
impl BootstrapFactory<u8> for Factory {
    fn create_for(&self, peer: &u8) -> capnp::Result<Client> {
        let epoch = self.epoch.get();
        self.calls.borrow_mut().push((*peer, epoch));
        if *peer == 2 && epoch == 1 {
            return Err(capnp::Error::failed("bootstrap denied".into()));
        }
        let cap: harness::Client = capnp_rpc::new_client(Grant(
            self.base + u32::from(*peer) * 10 + epoch,
            self.delegated.clone(),
        ));
        Ok(cap.client)
    }
}
async fn value(cap: &harness::Client) -> u32 {
    cap.echo_request()
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_value()
}
fn call(peer: &mut Endpoint, question: u32, target: u32, pipeline: bool) {
    peer.send(|m| {
        let mut c = m.init_call();
        c.set_question_id(question);
        c.set_interface_id(harness::Client::TYPE_ID);
        c.set_method_id(0);
        if pipeline {
            c.reborrow()
                .init_target()
                .init_promised_answer()
                .set_question_id(target);
        } else {
            c.reborrow().init_target().set_imported_cap(target);
        }
        c.init_params()
            .get_content()
            .init_as::<harness::echo_params::Builder>();
    });
}
fn finish(peer: &mut Endpoint, question: u32) {
    peer.send(|m| {
        let mut f = m.init_finish();
        f.set_question_id(question);
        // Keep the export reference; the raw peer stores and later invokes it.
        f.set_release_result_caps(false);
        f.set_require_early_cancellation_workaround(false);
    });
}
async fn bootstrap(
    peer: &mut Endpoint,
    question: u32,
    expected: u32,
    obsolete: bool,
) -> Option<u32> {
    peer.send(|m| {
        let mut b = m.init_bootstrap();
        b.set_question_id(question);
        if obsolete {
            b.get_deprecated_object_id()
                .set_as::<capnp::data::Owned>(&[3][..])
                .unwrap();
        }
    });
    // Send the call before reading Bootstrap's Return, exercising its pipeline.
    call(peer, question + 1, question, true);
    let mut export = None;
    let mut seen = vec![];
    for _ in 0..2 {
        let msg = peer.recv().await;
        let message::Return(ret) = msg
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!("expected Return");
        };
        let ret = ret.unwrap();
        let id = ret.get_answer_id();
        seen.push(id);
        if expected == 1 {
            let return_::Exception(error) = ret.which().unwrap() else {
                panic!("expected Bootstrap/pipeline denial");
            };
            let reason = error
                .unwrap()
                .get_reason()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            assert!(
                reason.contains(if obsolete {
                    "named exports"
                } else {
                    "bootstrap denied"
                }),
                "{reason}"
            );
        } else {
            let return_::Results(payload) = ret.which().unwrap() else {
                panic!("expected results");
            };
            let payload = payload.unwrap();
            if id == question {
                let cap_descriptor::SenderHosted(id) =
                    payload.get_cap_table().unwrap().get(0).which().unwrap()
                else {
                    panic!("expected exported capability");
                };
                export = Some(id);
            } else {
                assert_eq!(
                    payload
                        .get_content()
                        .get_as::<harness::value::Reader>()
                        .unwrap()
                        .get_value(),
                    expected
                );
            }
        }
    }
    seen.sort();
    assert_eq!(seen, vec![question, question + 1]);
    finish(peer, question);
    finish(peer, question + 1);
    export
}
async fn use_export(peer: &mut Endpoint, export: u32, expected: u32) {
    call(peer, 100, export, false);
    let msg = peer.recv().await;
    let message::Return(ret) = msg
        .get_body()
        .unwrap()
        .get_as::<message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!();
    };
    let ret = ret.unwrap();
    assert_eq!(ret.get_answer_id(), 100);
    let return_::Results(payload) = ret.which().unwrap() else {
        panic!("previously issued capability was revoked by bootstrap policy");
    };
    assert_eq!(
        payload
            .unwrap()
            .get_content()
            .get_as::<harness::value::Reader>()
            .unwrap()
            .get_value(),
        expected
    );
    finish(peer, 100);
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_factory_uses_local_identity_and_current_policy() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let hub = Rc::new(RefCell::new(Hub::default()));
            let network = Hub::network(&hub, 1);
            let factory = Rc::new(Factory::default());
            let mut system =
                RpcSystem::new_with_bootstrap_factory(Box::new(network), 1, factory.clone());
            let first: harness::Client = system.bootstrap(1);
            assert_eq!(value(&first).await, 10);
            factory.epoch.set(1);
            let second: harness::Client = system.bootstrap(1);
            assert_eq!(value(&second).await, 11);
            assert_eq!(value(&first).await, 10);
            assert_eq!(*factory.calls.borrow(), vec![(1, 0), (1, 1)]);
            assert_eq!(hub.borrow().provision_count(), 0);
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_factory_separates_peers_and_denies_without_disconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let network = Hub::network(&hub, 1);
                let factory = Rc::new(Factory::default());
                let task = tokio::task::spawn_local(RpcSystem::new_with_bootstrap_factory(
                    Box::new(network),
                    1,
                    factory.clone(),
                ));
                let mut a = hub.borrow_mut().connect(2, 1);
                let mut b = hub.borrow_mut().connect(3, 1);
                let old = bootstrap(&mut a, 10, 20, false).await.unwrap();
                bootstrap(&mut b, 10, 30, false).await;
                factory.epoch.set(1);
                bootstrap(&mut a, 20, 1, false).await;
                bootstrap(&mut b, 20, 31, false).await;
                bootstrap(&mut a, 30, 1, true).await;
                use_export(&mut a, old, 20).await;
                assert_eq!(
                    *factory.calls.borrow(),
                    vec![(2, 0), (3, 0), (2, 1), (3, 1)]
                );
                task.abort();
                let _ = task.await;
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct Step {
    action: String,
    state: [u32; 12],
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_bootstrap_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_BOOTSTRAP_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<Step>> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(60), async {
                for trace in traces {
                    let hub = Rc::new(RefCell::new(Hub::default()));
                    let network = Hub::network(&hub, 1);
                    let factory = Rc::new(Factory::default());
                    let task = tokio::task::spawn_local(RpcSystem::new_with_bootstrap_factory(
                        Box::new(network),
                        1,
                        factory.clone(),
                    ));
                    let a = hub.borrow_mut().connect(2, 1);
                    let b = hub.borrow_mut().connect(3, 1);
                    let mut peers = [a, b];
                    let mut first = [None, None];
                    let mut calls = vec![];
                    for step in &trace {
                        let s = step.state;
                        match step.action.as_str() {
                            "policy" => factory.epoch.set(1),
                            "a" | "b" => {
                                let p = usize::from(step.action == "b");
                                let export = bootstrap(
                                    &mut peers[p],
                                    10 + 2 * s[1 + p],
                                    s[4 + 2 * p],
                                    false,
                                )
                                .await;
                                if s[1 + p] == 1 {
                                    first[p] = export;
                                }
                                calls.push((p as u8 + 2, s[0]));
                            }
                            "use_a" => use_export(&mut peers[0], first[0].unwrap(), s[3]).await,
                            "use_b" => use_export(&mut peers[1], first[1].unwrap(), s[5]).await,
                            "obsolete" => {
                                bootstrap(&mut peers[0], 50, 1, true).await;
                            }
                            other => panic!("unknown action {other}"),
                        }
                        assert_eq!(*factory.calls.borrow(), calls, "{step:?} in {trace:?}");
                        assert_eq!(factory.calls.borrow().len() as u32, s[1] + s[2]);
                        assert_eq!(factory.epoch.get(), s[0]);
                    }
                    task.abort();
                    let _ = task.await;
                }
            })
            .await
            .expect("bootstrap trace replay timed out");
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct IntroductionStep {
    action: String,
    state: [u32; 3],
}
async fn introduction_trace(trace: &[IntroductionStep]) {
    use std::{future::Future, pin::Pin};
    let hub = Rc::new(RefCell::new(Hub::default()));
    hub.borrow_mut().introductions = true;
    let a = Hub::network(&hub, 1);
    let b = Hub::network(&hub, 2);
    let c = Hub::network(&hub, 3);
    let bf = Rc::new(Factory {
        base: 200,
        ..Factory::default()
    });
    let cf = Rc::new(Factory {
        base: 300,
        ..Factory::default()
    });
    let bs = Rc::new(RefCell::new(RpcSystem::new_with_bootstrap_factory(
        Box::new(b),
        2,
        bf.clone(),
    )));
    let cs = Rc::new(RefCell::new(RpcSystem::new_with_bootstrap_factory(
        Box::new(c),
        3,
        cf.clone(),
    )));
    let bdriver = bs.clone();
    let cdriver = cs.clone();
    let bt = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
        Pin::new(&mut *bdriver.borrow_mut()).poll(cx)
    }));
    let ct = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
        Pin::new(&mut *cdriver.borrow_mut()).poll(cx)
    }));
    let mut system = RpcSystem::new(Box::new(a), None);
    let broker: harness::Client = system.bootstrap(2);
    let owner: harness::Client = system.bootstrap(3);
    let at = tokio::task::spawn_local(system);
    let owner = capnp::capability::get_resolved_cap(owner).await;
    assert_eq!(value(&owner).await, 310);
    assert_eq!(value(&broker).await, 210);
    assert_eq!(hub.borrow().introduction_count, 0);
    for step in trace {
        match step.action.as_str() {
            "introduce" => {
                let mut r = broker.bounce_request();
                r.get().set_cap(owner.clone());
                let cap = r
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                assert_eq!(value(&cap).await, step.state[0]);
                assert_eq!(*bf.delegated.borrow(), vec![310]);
                assert!(hub.borrow().introduction_count > 0);
            }
            "c_to_b" => {
                let cap: harness::Client = cs.borrow_mut().bootstrap(2);
                assert_eq!(value(&cap).await, step.state[1]);
            }
            "b_to_c" => {
                let cap: harness::Client = bs.borrow_mut().bootstrap(3);
                assert_eq!(value(&cap).await, step.state[2]);
            }
            other => panic!("unknown introduction action {other}"),
        }
        let mut b_calls = vec![(1, 0)];
        let mut c_calls = vec![(1, 0)];
        if step.state[1] != 0 {
            b_calls.push((3, 0));
        }
        if step.state[2] != 0 {
            c_calls.push((2, 0));
        }
        assert_eq!(*bf.calls.borrow(), b_calls, "{trace:?}");
        assert_eq!(*cf.calls.borrow(), c_calls, "{trace:?}");
    }
    for task in [at, bt, ct] {
        task.abort();
        let _ = task.await;
    }
}
#[tokio::test(flavor = "current_thread")]
async fn introduced_connections_keep_factory_and_delegated_capability_authority() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(
                Duration::from_secs(5),
                introduction_trace(&[
                    IntroductionStep {
                        action: "introduce".into(),
                        state: [310, 0, 0],
                    },
                    IntroductionStep {
                        action: "c_to_b".into(),
                        state: [310, 230, 0],
                    },
                    IntroductionStep {
                        action: "b_to_c".into(),
                        state: [310, 230, 320],
                    },
                ]),
            )
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_introduced_bootstrap_traces() {
    let path =
        capntproto_test_support::verification::input("CAPNTPROTO_INTRODUCED_BOOTSTRAP_TRACES")
            .expect("prepare verified trace corpus");
    let traces: Vec<Vec<IntroductionStep>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                for trace in traces {
                    introduction_trace(&trace).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

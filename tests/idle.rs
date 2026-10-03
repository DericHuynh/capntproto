#[allow(dead_code)]
mod support;
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, return_},
    RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::FutureExt;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, time::Duration};
use support::{ConnectionStatus, Endpoint, Hub};
struct Echo(Rc<RefCell<Option<harness::PendingResults>>>);
impl harness::Server for Echo {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        results: harness::PendingResults,
    ) -> capnp::Result<()> {
        *self.0.borrow_mut() = Some(results);
        futures::future::pending().await
    }
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(42);
        Ok(())
    }
}
async fn drain() {
    for _ in 0..40 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    hub: Rc<RefCell<Hub>>,
    system: Rc<RefCell<RpcSystem<u8>>>,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
    peer: Endpoint,
    status: Rc<RefCell<ConnectionStatus>>,
    cap: Option<harness::Client>,
    response: Option<capnp::capability::Response<harness::value::Owned>>,
    export: u32,
    held: Rc<RefCell<Option<harness::PendingResults>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let results = self.held.borrow_mut().take();
        drop(results);
        for t in &self.tasks {
            t.abort();
        }
    }
}
impl Fixture {
    async fn new(outgoing: bool) -> Self {
        Self::configured(outgoing, false).await
    }
    async fn configured(outgoing: bool, retained: bool) -> Self {
        let held = Rc::new(RefCell::new(None));
        let hub = Rc::new(RefCell::new(Hub::default()));
        let a = Hub::network(&hub, 1);
        let service: harness::Client = capnp_rpc::new_client(Echo(held.clone()));
        let system = Rc::new(RefCell::new(RpcSystem::new(
            Box::new(a),
            Some(service.client),
        )));
        let driver = system.clone();
        let mut tasks = vec![tokio::task::spawn_local(futures::future::poll_fn(
            move |cx| Pin::new(&mut *driver.borrow_mut()).poll(cx),
        ))];
        let (cap, response, export, peer) = if outgoing {
            let b = Hub::network(&hub, 2);
            let service: harness::Client = capnp_rpc::new_client(Echo(held.clone()));
            tasks.push(tokio::task::spawn_local(RpcSystem::new(
                Box::new(b),
                Some(service.client),
            )));
            let cap: harness::Client = system.borrow_mut().bootstrap(2);
            let cap = capnp::capability::get_resolved_cap(cap).await;
            let response = cap.echo_request().send().promise.await.unwrap();
            let peer = hub.borrow_mut().connect(2, 1);
            (Some(cap), Some(response), 0, peer)
        } else {
            let mut peer = hub.borrow_mut().connect(2, 1);
            peer.send(|m| m.init_bootstrap().set_question_id(0));
            let msg = peer.recv().await;
            let message::Return(ret) = msg
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            let return_::Results(payload) = ret.unwrap().which().unwrap() else {
                panic!()
            };
            let cap_descriptor::SenderHosted(export) = payload
                .unwrap()
                .get_cap_table()
                .unwrap()
                .get(0)
                .which()
                .unwrap()
            else {
                panic!()
            };
            (None, None, export, peer)
        };
        let mut peer = peer;
        if retained {
            use capnp::traits::HasTypeId;
            peer.send(|m| {
                let mut c = m.init_call();
                c.set_question_id(1);
                c.set_interface_id(harness::Client::TYPE_ID);
                c.set_method_id(3);
                c.reborrow().init_target().set_imported_cap(export);
                c.init_params()
                    .get_content()
                    .init_as::<harness::pending_params::Builder>();
            });
        }
        let status = hub.borrow_mut().connect(1, 2).status();
        drain().await;
        assert_eq!(status.borrow().notifications, vec![false]);
        Self {
            hub,
            system,
            tasks,
            peer,
            status,
            cap,
            response,
            export,
            held,
        }
    }
    async fn release_cap(&mut self, outgoing: bool) {
        if outgoing {
            self.cap.take();
        } else {
            self.peer.send(|m| {
                let mut r = m.init_release();
                r.set_id(self.export);
                r.set_reference_count(1);
            });
        }
        drain().await;
    }
    async fn release_question(&mut self, outgoing: bool) {
        if outgoing {
            self.response.take();
        } else {
            self.peer.send(|m| {
                let mut f = m.init_finish();
                f.set_question_id(0);
                f.set_release_result_caps(false);
            });
        }
        drain().await;
    }
    async fn probe(&mut self, outgoing: bool) {
        if outgoing {
            let cap: harness::Client = self.system.borrow_mut().bootstrap(2);
            drop(capnp::capability::get_resolved_cap(cap).await);
        } else {
            self.peer.send(|m| {
                m.init_join();
            });
            let msg = self.peer.recv().await;
            assert!(matches!(
                msg.get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap(),
                message::Unimplemented(_)
            ));
        }
        drain().await;
    }
}
#[derive(serde::Deserialize, Debug)]
struct Step {
    action: String,
    state: Vec<u64>,
}
#[derive(serde::Deserialize, Debug)]
struct Trace {
    outgoing: bool,
    retained: bool,
    end_mode: u64,
    steps: Vec<Step>,
}
async fn replay(trace: &Trace) {
    let mut f = Fixture::configured(trace.outgoing, trace.retained).await;
    let (mut held, mut finished, mut returned) = (trace.retained, false, false);
    let (mut cap, mut q, mut connected, mut probed, mut ever) = (true, true, true, false, false);
    for step in &trace.steps {
        match step.action.as_str() {
            "cap" => {
                cap = false;
                f.release_cap(trace.outgoing).await;
            }
            "question" => {
                q = false;
                f.release_question(trace.outgoing).await;
            }
            "held" => {
                held = false;
                let results = f.held.borrow_mut().take();
                drop(results);
                drain().await;
            }
            "finish-call" => {
                finished = true;
                f.peer.send(|m| {
                    let mut v = m.init_finish();
                    v.set_question_id(1);
                    v.set_require_early_cancellation_workaround(false);
                });
                drain().await;
            }
            "probe" => {
                probed = true;
                f.probe(trace.outgoing).await;
            }
            "eof" => {
                connected = false;
                match trace.end_mode {
                    0 => f.hub.borrow_mut().disconnect_pair(1, 2),
                    1 => {
                        f.status.borrow_mut().read_error =
                            Some(capnp::Error::failed("transport read failed".into()));
                        f.hub.borrow_mut().disconnect_pair(1, 2);
                    }
                    2 => {
                        let disconnector = f.system.borrow().get_disconnector();
                        disconnector.await.unwrap();
                    }
                    _ => panic!("bad mode"),
                }
                drain().await;
            }
            other => panic!("{other}"),
        }
        let call = trace.retained && (!finished || held);
        if connected && trace.retained && !call && !returned {
            let msg = f.peer.recv().await;
            let message::Return(ret) = msg
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            assert!(matches!(
                ret.unwrap().which().unwrap(),
                return_::Canceled(())
            ));
            returned = true;
        }
        ever |= connected && !cap && !q && !call;
        let status = f.status.borrow();
        let actual = vec![
            cap as u64,
            q as u64,
            connected as u64,
            probed as u64,
            status.idle as u64,
            !status.shutdowns.is_empty() as u64,
            status.aborts as u64,
            status.notifications.len() as u64,
            status.shutdowns.first().copied().unwrap_or(false) as u64,
            ever as u64,
            held as u64,
            finished as u64,
            call as u64,
        ];
        assert_eq!(actual, step.state, "{trace:?} action {}", step.action);
        assert!(status.shutdowns.len() <= 1);
    }
    drop(f);
    drain().await;
}
#[tokio::test(flavor = "current_thread")]
async fn capabilities_and_answers_prevent_idle_until_last_release() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for outgoing in [false, true] {
                for question_first in [false, true] {
                    let mut f = Fixture::new(outgoing).await;
                    if question_first {
                        f.release_question(outgoing).await;
                    } else {
                        f.release_cap(outgoing).await;
                    }
                    assert!(!f.status.borrow().idle);
                    if question_first {
                        f.release_cap(outgoing).await;
                    } else {
                        f.release_question(outgoing).await;
                    }
                    assert!(f.status.borrow().idle);
                    f.probe(outgoing).await;
                    assert_eq!(
                        f.status.borrow().notifications,
                        vec![false, true, false, true]
                    );
                    f.hub.borrow_mut().disconnect_pair(1, 2);
                    drain().await;
                    assert_eq!(f.status.borrow().shutdowns, vec![true]);
                    assert_eq!(f.status.borrow().aborts, 0);
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_idle_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_IDLE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for t in &traces {
                tokio::time::timeout(Duration::from_secs(5), replay(t))
                    .await
                    .unwrap_or_else(|_| panic!("{t:?}"));
            }
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct ShutdownTrace {
    steps: Vec<Step>,
}
async fn replay_shutdown(trace: &ShutdownTrace) {
    let mut f = Fixture::new(true).await;
    f.release_cap(true).await;
    f.release_question(true).await;
    assert!(f.status.borrow().idle);
    let gate = f.hub.borrow_mut().connect(1, 2).gate_shutdown();
    let mut gate = Some(gate);
    f.hub.borrow_mut().disconnect_pair(1, 2);
    drain().await;
    assert_eq!(f.status.borrow().shutdowns, vec![true]);
    f.hub.borrow_mut().forget_pair(1, 2);
    let mut disconnector = f.system.borrow().get_disconnector().boxed_local();
    let (mut flushed, mut closing, mut reconnected, mut done) = (false, false, false, false);
    for step in &trace.steps {
        match step.action.as_str() {
            "flush" => {
                gate.take().unwrap().send(()).unwrap();
                flushed = true;
            }
            "disconnect" => {
                closing = true;
            }
            "reconnect" => {
                let cap: harness::Client = f.system.borrow_mut().bootstrap(2);
                let response = cap.echo_request().send().promise.await.unwrap();
                assert_eq!(response.get().unwrap().get_value(), 42);
                f.cap = Some(cap);
                reconnected = true;
            }
            other => panic!("{other}"),
        }
        drain().await;
        if closing && !done {
            if let Some(result) = disconnector.as_mut().now_or_never() {
                result.unwrap();
                done = true;
            }
            drain().await;
            if !done {
                if let Some(result) = disconnector.as_mut().now_or_never() {
                    result.unwrap();
                    done = true;
                }
            }
        }
        assert_eq!(
            vec![
                flushed as u64,
                closing as u64,
                reconnected as u64,
                done as u64
            ],
            step.state,
            "{trace:?}"
        );
        assert_eq!(f.status.borrow().shutdowns, vec![true]);
        assert_eq!(f.status.borrow().aborts, 0);
        if closing {
            let cap: harness::Client = f.system.borrow_mut().bootstrap(2);
            assert!(cap.echo_request().send().promise.await.is_err());
        }
    }
    drop(gate);
    drop(f);
    drain().await;
}
#[tokio::test(flavor = "current_thread")]
async fn disconnector_waits_for_idle_flush_alongside_replacement_connection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let trace = ShutdownTrace {
                steps: vec![
                    Step {
                        action: "reconnect".into(),
                        state: vec![0, 0, 1, 0],
                    },
                    Step {
                        action: "disconnect".into(),
                        state: vec![0, 1, 1, 0],
                    },
                    Step {
                        action: "flush".into(),
                        state: vec![1, 1, 1, 1],
                    },
                ],
            };
            tokio::time::timeout(Duration::from_secs(5), replay_shutdown(&trace))
                .await
                .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_idle_shutdown_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_IDLE_SHUTDOWN_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<ShutdownTrace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for t in &traces {
                tokio::time::timeout(Duration::from_secs(5), replay_shutdown(t))
                    .await
                    .unwrap_or_else(|_| panic!("{t:?}"));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn idle_close_never_sends_abort_for_eof_read_error_or_explicit_disconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for outgoing in [false, true] {
                for end_mode in 0..3 {
                    let trace = Trace {
                        outgoing,
                        retained: false,
                        end_mode,
                        steps: vec![
                            Step {
                                action: "cap".into(),
                                state: vec![0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0],
                            },
                            Step {
                                action: "question".into(),
                                state: vec![0, 0, 1, 0, 1, 0, 0, 2, 0, 1, 0, 0, 0],
                            },
                            Step {
                                action: "eof".into(),
                                state: vec![
                                    0,
                                    0,
                                    0,
                                    0,
                                    1,
                                    1,
                                    0,
                                    2,
                                    (end_mode == 0) as u64,
                                    1,
                                    0,
                                    0,
                                    0,
                                ],
                            },
                        ],
                    };
                    tokio::time::timeout(Duration::from_secs(5), replay(&trace))
                        .await
                        .unwrap();
                }
            }
        })
        .await;
}

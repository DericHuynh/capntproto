#[allow(dead_code)]
mod support;
use capnp::{
    capability::{Request, ResponsePromise},
    traits::ImbueMut,
    Error, ErrorKind,
};
use capnp_rpc::rpc_capnp::{cap_descriptor, message};

use capntproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::{
    driver::{RunningSystem, System as RpcSystem},
    ConnectionStatus, Endpoint, Hub,
};

type EchoPromise = ResponsePromise<harness::value::Owned>;
type EchoRequest = Request<harness::echo_params::Owned, harness::value::Owned>;
struct Empty;
impl harness::Server for Empty {}
struct Destructor {
    remote: harness::Client,
    built: Option<EchoRequest>,
    pending: Option<EchoPromise>,
    result: Rc<RefCell<Option<EchoPromise>>>,
    destroyed: Rc<Cell<bool>>,
}
impl harness::Server for Destructor {}
impl Drop for Destructor {
    fn drop(&mut self) {
        self.destroyed.set(true);
        // This also releases a QuestionRef while the connection is being torn down.
        drop(self.pending.take());
        let request = self
            .built
            .take()
            .unwrap_or_else(|| self.remote.echo_request());
        *self.result.borrow_mut() = Some(request.send().promise);
    }
}
async fn drain() {
    for _ in 0..40 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    _hub: Rc<RefCell<Hub>>,
    _network: support::Network,
    system: RunningSystem<u8>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    peer: Endpoint,
    status: Rc<RefCell<ConnectionStatus>>,
    remote: harness::Client,
    cap: Option<harness::Client>,
    export: u32,
    question: Option<capnp::capability::RemotePromise<harness::bounce_results::Owned>>,
    destroyed: Rc<Cell<bool>>,
    result: Rc<RefCell<Option<EchoPromise>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(portable: bool, prebuilt: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 2);
        let mut system = RpcSystem::new(portable, Box::new(Hub::network(&hub, 1)), None);
        let remote: harness::Client = system.bootstrap(2);
        let mut peer = hub.borrow_mut().connect(2, 1);
        let status = hub.borrow_mut().connect(1, 2).status();
        let (system, driver) = system.start();
        let task = tokio::task::spawn_local(driver);
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
        peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(b.unwrap().get_question_id());
            let mut payload = r.init_results();
            let mut caps = vec![];
            let mut content = payload.reborrow().get_content();
            content.imbue_mut(&mut caps);
            content.set_as_capability(
                capnp_rpc::new_client::<harness::Client, _>(Empty)
                    .client
                    .hook,
            );
            payload.init_cap_table(1).get(0).set_sender_hosted(77);
        });
        let remote = capnp::capability::get_resolved_cap(remote).await;
        let pending = remote.echo_request().send().promise;
        let destroyed = Rc::new(Cell::new(false));
        let result = Rc::new(RefCell::new(None));
        let cap: harness::Client = capnp_rpc::new_client(Destructor {
            remote: remote.clone(),
            built: prebuilt.then(|| remote.echo_request()),
            pending: Some(pending),
            result: result.clone(),
            destroyed: destroyed.clone(),
        });
        let mut request = remote.bounce_request();
        request.get().set_cap(cap.clone());
        let question = Some(request.send());
        let export = loop {
            let input = peer.recv().await;
            if let message::Call(c) = input
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                let c = c.unwrap();
                if c.get_method_id() == 1 {
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
                    break id;
                }
            }
        };
        drain().await;
        Self {
            _hub: hub,
            _network: network,
            system,
            task,
            peer,
            status,
            remote,
            cap: Some(cap),
            export,
            question,
            destroyed,
            result,
        }
    }
    async fn abort(&mut self) {
        self.peer.send(|m| {
            let mut e = m.init_abort();
            e.set_reason("cleanup failure");
            e.set_type(capnp_rpc::rpc_capnp::exception::Type::Overloaded);
            e.set_trace("cleanup trace");
            let mut d = e.init_details(1);
            d.reborrow().get(0).set_detail_id(9);
            d.get(0).set_data(b"owned");
        });
        drain().await;
    }
}
fn check_error(e: Error) {
    assert_eq!(e.kind, ErrorKind::Disconnected);
    assert_eq!(e.remote_trace(), Some("cleanup trace"), "{e:?}");
    assert_eq!(e.detail(9), Some(&b"owned"[..]));
}
#[tokio::test(flavor = "current_thread")]
async fn disconnect_marks_broken_before_capability_destructors() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(5), async {
                    for prebuilt in [false, true] {
                        let mut f = Fixture::new(portable, prebuilt).await;
                        drop(f.cap.take());
                        let sent = f.status.borrow().sent;
                        f.abort().await;
                        assert!(f.destroyed.get());
                        assert_eq!(
                            f.status.borrow().sent,
                            sent + 1,
                            "only Abort may be sent during disconnect"
                        );
                        let p = f.result.borrow_mut().take().unwrap();
                        check_error(p.await.err().unwrap());
                        check_error(f.question.take().unwrap().promise.await.err().unwrap());
                    }
                })
                .await
                .unwrap();
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn requests_built_before_disconnect_fail_without_wire_traffic() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let mut f = Fixture::new(portable, false).await;
                    let ordinary = f.remote.echo_request();
                    let stream = f.remote.stream_request();
                    let pipeline = f.remote.bounce_request();
                    f.abort().await;
                    let sent = f.status.borrow().sent;
                    let pending = ordinary.send().promise;
                    assert_eq!(f.status.borrow().sent, sent, "stale ordinary request sent");
                    check_error(pending.await.err().unwrap());
                    check_error(stream.send().await.err().unwrap());
                    let cap = pipeline.send_for_pipeline().get_cap();
                    check_error(cap.echo_request().send().promise.await.err().unwrap());
                    assert_eq!(f.status.borrow().sent, sent, "stale streaming request sent");
                })
                .await
                .unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn disconnect_trace_encoder_can_reenter_rpc() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let mut f = Fixture::new(portable, false).await;
                    let remote = f.remote.clone();
                    let reentered = Rc::new(RefCell::new(None));
                    let output = reentered.clone();
                    f.system.set_trace_encoder(move |_| {
                        *output.borrow_mut() = Some(remote.echo_request().send().promise);
                        "encoded".into()
                    });
                    let sent = f.status.borrow().sent;
                    f.abort().await;
                    assert_eq!(f.status.borrow().sent, sent + 1);
                    let p = reentered.borrow_mut().take().unwrap();
                    check_error(p.await.err().unwrap());
                    f.system.clear_trace_encoder();
                })
                .await
                .unwrap();
            }
        })
        .await;
}

fn check_disconnect(error: Error, explicit: bool) {
    if explicit {
        assert_eq!(error.kind, ErrorKind::Disconnected);
        assert_eq!(error.extra, "client requested disconnect");
        assert_eq!(error.remote_trace(), None);
    } else {
        check_error(error);
    }
}

async fn replay(
    portable: bool,
    steps: &[serde_json::Value],
    prebuilt: bool,
    fail_body: bool,
    explicit: bool,
) {
    let mut f = Fixture::new(portable, prebuilt).await;
    let encoder_calls = Rc::new(Cell::new(0));
    let calls = encoder_calls.clone();
    let encoder_result = Rc::new(RefCell::new(None));
    let output = encoder_result.clone();
    let remote = f.remote.clone();
    f.system.set_trace_encoder(move |_| {
        calls.set(calls.get() + 1);
        *output.borrow_mut() = Some(remote.echo_request().send().promise);
        "cleanup encoder".into()
    });
    let baseline = {
        let s = f.status.borrow();
        (s.calls, s.finishes)
    };
    let mut request = Some(f.remote.echo_request());
    let mut request_result = None;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "drop-app" => drop(f.cap.take()),
            "release" => f.peer.send(|m| {
                let mut r = m.init_release();
                r.set_id(f.export);
                r.set_reference_count(1);
            }),
            "close" => {
                f.status.borrow_mut().fail_body = fail_body;
                if explicit {
                    let disconnect = f.system.get_disconnector();
                    disconnect.await.unwrap();
                } else {
                    f.abort().await;
                }
            }
            "request" => {
                request_result = Some(request.take().unwrap().send().promise);
            }
            "question" => drop(f.question.take()),
            "observe" => {
                let p = f.result.borrow_mut().take().unwrap();
                check_disconnect(p.await.err().unwrap(), explicit);
            }
            _ => panic!("unexpected cleanup action"),
        }
        drain().await;
        let state: Vec<u64> = step["state"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        assert_eq!(f.cap.is_some(), state[0] != 0);
        assert_eq!(f.destroyed.get(), state[3] != 0);
        assert_eq!(request.is_none(), state[5] != 0);
        assert_eq!(f.question.is_some(), state[6] != 0);
        {
            let s = f.status.borrow();
            assert_eq!(s.calls - baseline.0, state[7] as usize, "calls at {step:?}");
            assert_eq!(
                s.finishes - baseline.1,
                state[8] as usize,
                "finishes at {step:?}"
            );
            assert_eq!(s.aborts, state[9] as usize, "portable={portable}, prebuilt={prebuilt}, fail_body={fail_body}, explicit={explicit}, step={step}");
            assert_eq!(s.shutdowns.len(), state[11] as usize);
            assert_eq!(s.readers, state[12] as usize);
        }
        assert_eq!(encoder_calls.get(), state[2]);
        if state[2] != 0 {
            let encoded = encoder_result.borrow_mut().take();
            if let Some(p) = encoded {
                check_disconnect(p.await.err().unwrap(), explicit);
            }
            if let Some(p) = request_result.take() {
                check_disconnect(p.await.err().unwrap(), explicit);
            }
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_disconnect_cleanup_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_DISCONNECT_CLEANUP_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(60), async {
                    for trace in &traces {
                        for prebuilt in [false, true] {
                            replay(
                                portable,
                                trace["steps"].as_array().unwrap(),
                                prebuilt,
                                trace["fail_body"].as_bool().unwrap(),
                                trace["explicit"].as_bool().unwrap(),
                            )
                            .await;
                        }
                    }
                })
                .await
                .unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn abort_body_failure_still_releases_capabilities_and_shuts_down() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let mut f = Fixture::new(portable, true).await;
                    drop(f.cap.take());
                    f.status.borrow_mut().fail_body = true;
                    let sent = f.status.borrow().sent;
                    f.abort().await;
                    assert!(f.destroyed.get());
                    assert_eq!(f.status.borrow().sent, sent);
                    assert_eq!(f.status.borrow().shutdowns.len(), 1);
                    let result = f.result.borrow_mut().take().unwrap();
                    check_error(result.await.err().unwrap());
                    check_error(f.question.take().unwrap().promise.await.err().unwrap());
                })
                .await
                .unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_disconnect_cancels_outstanding_transport_read() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let f = Fixture::new(portable, false).await;
                    assert_eq!(f.status.borrow().readers, 1);
                    let disconnect = f.system.get_disconnector();
                    disconnect.await.unwrap();
                    drain().await;
                    assert_eq!(
                        f.status.borrow().readers,
                        0,
                        "read retained after disconnect"
                    );
                })
                .await
                .unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_terminal_publication_still_awaits_physical_shutdown() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                let mut f = Fixture::new(portable, true).await;
                drop(f.cap.take());
                let release = f._hub.borrow_mut().connect(1, 2).gate_shutdown();
                f.status.borrow_mut().fail_body = true;
                let mut disconnect = f.system.get_disconnector();
                assert!(futures::poll!(&mut disconnect).is_pending());
                drain().await;
                assert_eq!(f.status.borrow().shutdowns.len(), 1);
                assert!(
                    futures::poll!(&mut disconnect).is_pending(),
                    "portable={portable}: shutdown must finish before disconnect completes"
                );
                release.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(2), disconnect)
                    .await
                    .unwrap()
                    .unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn disconnect_releases_exports_before_waiting_for_physical_shutdown() {
    tokio::task::LocalSet::new().run_until(async {
        for portable in [false, true] {
            for fail_body in [false, true] {
                let mut f = Fixture::new(portable, true).await;
                drop(f.cap.take());
                let release = f._hub.borrow_mut().connect(1, 2).gate_shutdown();
                f.status.borrow_mut().fail_body = fail_body;
                let mut disconnect = f.system.get_disconnector();
                assert!(futures::poll!(&mut disconnect).is_pending());
                drain().await;
                assert_eq!(f.status.borrow().shutdowns.len(), 1);
                assert!(f.destroyed.get(), "portable={portable}, fail_body={fail_body}: exported contexts must retire before physical shutdown waits");
                assert_eq!(f.status.borrow().readers, 0, "pending read must stop before shutdown waits");
                assert_eq!(f.status.borrow().shutdowns, [false], "transport receives the disconnect reason");
                assert!(futures::poll!(&mut disconnect).is_pending());
                let response = f.result.borrow_mut().take().unwrap();
                let error = response.await.err().unwrap();
                check_disconnect(error, true);
                release.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(2), disconnect).await.unwrap().unwrap();
            }
        }
    }).await;
}

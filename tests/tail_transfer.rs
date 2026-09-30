mod support;
use capnp::traits::{HasTypeId, ImbueMut};
use capnp_rpc::{
    rpc_capnp::{call, cap_descriptor, message, message_target, promised_answer, return_},
    Connection, RpcSystem,
};
use futures::FutureExt;
use reproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, time::Duration};
use support::{Endpoint, Hub};
struct Tail;
impl harness::Server for Tail {
    async fn tail_cap(
        self: Rc<Self>,
        p: harness::TailCapParams,
        r: harness::TailCapResults,
    ) -> capnp::Result<()> {
        let request = p.get()?.get_cap()?.pending_request();
        r.hook.tail_call(request.hook).await
    }
}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}
fn finish(peer: &mut Endpoint, id: u32) {
    peer.send(|m| {
        let mut f = m.init_finish();
        f.set_question_id(id);
        f.set_require_early_cancellation_workaround(false);
    });
}
struct Fixture {
    peer: Endpoint,
    tail: u32,
    child: Option<u32>,
    tail_finishes: u32,
    child_returns: u32,
    child_ack: u32,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    system: Rc<RefCell<RpcSystem<u8>>>,
    disconnected: bool,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_hint(false).await
    }
    async fn with_hint(only_pipeline: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let service: harness::Client = capnp_rpc::new_client(Tail);
        let mut system = RpcSystem::new(Box::new(network), Some(service.client));
        if !only_pipeline {
            system.set_flow_limit(1);
        }
        let system = Rc::new(RefCell::new(system));
        let driver = system.clone();
        let task = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
            Pin::new(&mut *driver.borrow_mut()).poll(cx)
        }));
        let mut peer = hub.borrow_mut().connect(2, 1);
        assert_eq!(hub.borrow().provision_count(), 0);
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
        peer.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(1);
            c.set_interface_id(harness::Client::TYPE_ID);
            c.set_method_id(4);
            c.set_only_promise_pipeline(only_pipeline);
            c.reborrow().init_target().set_imported_cap(export);
            let mut payload = c.init_params();
            let mut caps = vec![];
            {
                let mut content = payload.reborrow().get_content();
                content.imbue_mut(&mut caps);
                let cap: harness::Client = capnp_rpc::new_client(Tail);
                content
                    .init_as::<harness::tail_cap_params::Builder>()
                    .set_cap(cap);
            }
            payload.init_cap_table(1).get(0).set_sender_hosted(77);
        });
        if only_pipeline {
            let question = loop {
                let msg = peer.recv().await;
                match msg
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                {
                    message::Call(c) => {
                        let c = c.unwrap();
                        assert!(
                            matches!(
                                c.get_send_results_to().which().unwrap(),
                                call::send_results_to::Caller(())
                            ),
                            "onlyPromisePipeline must not transfer results"
                        );
                        break c.get_question_id();
                    }
                    message::Release(_) => {}
                    _ => panic!("unexpected message for pipeline-only tail call"),
                }
            };
            assert_ne!(question & (1 << 31), 0);
            // A pipeline-only proxy forwards the hint and suppresses Return.
            // Even a late Return from an older downstream peer is ignored.
            peer.send(|m| {
                let mut r = m.init_return();
                r.set_answer_id(question);
                r.set_release_param_caps(false);
                r.set_canceled(());
            });
            finish(&mut peer, 1);
            loop {
                let msg = peer.recv().await;
                match msg
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                {
                    message::Finish(f) => {
                        assert_eq!(f.unwrap().get_question_id(), question);
                        break;
                    }
                    message::Release(_) => {}
                    _ => panic!("pipeline-only tail call must not send Return"),
                }
            }
            return Self {
                peer,
                tail: question,
                child: None,
                tail_finishes: 0,
                child_returns: 0,
                child_ack: 0,
                task,
                system,
                disconnected: false,
            };
        }
        let mut tail = None;
        let mut adopted = None;
        while tail.is_none() || adopted.is_none() {
            let msg = peer.recv().await;
            match msg
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Call(c) => {
                    let c = c.unwrap();
                    assert!(matches!(
                        c.get_send_results_to().which().unwrap(),
                        call::send_results_to::Yourself(())
                    ));
                    assert!(matches!(
                        c.get_target().unwrap().which().unwrap(),
                        message_target::ImportedCap(77)
                    ));
                    assert_eq!(c.get_method_id(), 3);
                    tail = Some(c.get_question_id());
                }
                message::Return(r) => {
                    let r = r.unwrap();
                    assert_eq!(r.get_answer_id(), 1);
                    let return_::TakeFromOtherQuestion(id) = r.which().unwrap() else {
                        panic!("expected optimized tail transfer")
                    };
                    adopted = Some(id);
                }
                message::Release(_) => {}
                _ => panic!("unexpected transfer message"),
            }
        }
        assert_eq!(tail, adopted);
        // Transfer must return incoming flow credit before the peer acknowledges.
        peer.send(|m| m.init_bootstrap().set_question_id(51));
        loop {
            let msg = peer.recv().await;
            match msg
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(r) => {
                    let r = r.unwrap();
                    assert_eq!(r.get_answer_id(), 51);
                    assert!(matches!(r.which().unwrap(), return_::Results(_)));
                    break;
                }
                message::Release(_) => {}
                _ => panic!("unexpected message before acknowledgement"),
            }
        }
        system.borrow_mut().set_flow_limit(usize::MAX);
        Self {
            peer,
            tail: tail.unwrap(),
            child: None,
            tail_finishes: 0,
            child_returns: 0,
            child_ack: 0,
            task,
            system,
            disconnected: false,
        }
    }
    fn ack(&mut self) {
        let id = self.tail;
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(id);
            r.set_results_sent_elsewhere(());
        });
    }
    fn pipeline(&mut self) {
        self.peer.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(2);
            c.set_interface_id(harness::Client::TYPE_ID);
            c.set_method_id(0);
            let mut t = c.reborrow().init_target().init_promised_answer();
            t.set_question_id(1);
            t.init_transform(1).get(0).set_get_pointer_field(0);
            c.init_params()
                .get_content()
                .init_as::<harness::echo_params::Builder>();
        });
    }
    fn reply_child(&mut self) {
        let id = self.child.unwrap();
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(id);
            r.set_results_sent_elsewhere(());
        });
        self.child_ack += 1;
    }
    fn drain(&mut self) {
        loop {
            let Some(input) = self.peer.receive_incoming_message().now_or_never() else {
                break;
            };
            let Some(input) = input.unwrap() else { break };
            match input
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Call(c) => {
                    let c = c.unwrap();
                    assert!(self.child.is_none());
                    assert!(matches!(
                        c.get_send_results_to().which().unwrap(),
                        call::send_results_to::Yourself(())
                    ));
                    let message_target::PromisedAnswer(t) =
                        c.get_target().unwrap().which().unwrap()
                    else {
                        panic!()
                    };
                    let t = t.unwrap();
                    assert_eq!(t.get_question_id(), self.tail);
                    assert_eq!(t.get_transform().unwrap().len(), 1);
                    assert!(matches!(
                        t.get_transform().unwrap().get(0).which().unwrap(),
                        promised_answer::op::GetPointerField(0)
                    ));
                    self.child = Some(c.get_question_id());
                }
                message::Return(r) => {
                    let r = r.unwrap();
                    assert_eq!(
                        r.get_answer_id(),
                        2,
                        "duplicate Return for transferred parent"
                    );
                    let return_::TakeFromOtherQuestion(question) = r.which().unwrap() else {
                        panic!("proxy child must transfer results back to the caller")
                    };
                    assert_eq!(Some(question), self.child);
                    self.child_returns += 1;
                }
                message::Finish(f) => {
                    if f.unwrap().get_question_id() == self.tail {
                        self.tail_finishes += 1;
                    }
                }
                message::Release(_) => {}
                message::Abort(_) => assert!(self.disconnected),
                _ => panic!("unexpected message"),
            }
        }
    }
    async fn cleanup(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}
#[derive(serde::Deserialize, Debug)]
struct Step {
    action: String,
    state: [u32; 8],
}
async fn replay(steps: &[Step]) {
    let mut f = Fixture::new().await;
    for s in steps {
        match s.action.as_str() {
            "ack" => f.ack(),
            "finish" => finish(&mut f.peer, 1),
            "pipeline" => f.pipeline(),
            "reply" => f.reply_child(),
            "disconnect" => {
                f.disconnected = true;
                let d = f.system.borrow().get_disconnector();
                d.await.unwrap();
            }
            "reuse" => {
                f.peer.send(|m| m.init_bootstrap().set_question_id(1));
                let msg = f.peer.recv().await;
                let message::Return(r) = msg
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!()
                };
                assert_eq!(r.unwrap().get_answer_id(), 1);
            }
            other => panic!("unknown action {other}"),
        }
        settle().await;
        f.drain();
        assert_eq!(
            f.tail_finishes,
            u32::from(s.state[0] != 0),
            "tail Finish at {s:?} in {steps:?}"
        );
        assert_eq!(
            f.child.is_some(),
            s.state[2] != 0,
            "pipeline at {s:?} in {steps:?}"
        );
        assert_eq!(
            f.child_returns, s.state[2],
            "exactly one transfer Return per proxy child"
        );
        assert_eq!(
            f.child_ack, s.state[3],
            "child acknowledgement at {s:?} in {steps:?}"
        );
    }
    f.cleanup().await;
}
#[tokio::test(flavor = "current_thread")]
async fn optimized_tail_transfer_preserves_pipeline_and_sends_one_return() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(
                Duration::from_secs(5),
                replay(&[
                    Step {
                        action: "pipeline".into(),
                        state: [0, 0, 1, 0, 1, 0, 1, 1],
                    },
                    Step {
                        action: "ack".into(),
                        state: [0, 1, 1, 0, 1, 0, 1, 1],
                    },
                    Step {
                        action: "finish".into(),
                        state: [1, 1, 1, 0, 1, 0, 1, 0],
                    },
                    Step {
                        action: "reply".into(),
                        state: [1, 1, 1, 1, 1, 0, 1, 0],
                    },
                    Step {
                        action: "reuse".into(),
                        state: [1, 1, 1, 1, 1, 1, 1, 0],
                    },
                ]),
            )
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_tail_transfer_traces() {
    let path = reproto_test_support::verification::input("REPROTO_TAIL_TRANSFER_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<Step>> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(60), async {
                for trace in traces {
                    replay(&trace).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

struct DelayedTail {
    prepared: RefCell<Option<futures::channel::oneshot::Sender<()>>>,
    gate: RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
}
impl harness::Server for DelayedTail {
    async fn tail_cap(
        self: Rc<Self>,
        p: harness::TailCapParams,
        r: harness::TailCapResults,
    ) -> capnp::Result<()> {
        let request = p.get()?.get_cap()?.pending_request();
        self.prepared.borrow_mut().take().unwrap().send(()).unwrap();
        let gate = self.gate.borrow_mut().take().unwrap();
        gate.await.unwrap();
        r.hook.tail_call(request.hook).await
    }
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut r: harness::PendingResults,
    ) -> capnp::Result<()> {
        let cap: harness::Client = capnp_rpc::new_client_from_rc(self);
        r.get().set_cap(cap);
        Ok(())
    }
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(9);
        Ok(())
    }
}
#[derive(serde::Deserialize, Debug)]
struct RoutingStep {
    action: String,
    state: [u32; 4],
}
async fn routing_trace(steps: &[RoutingStep]) {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let network = Hub::network(&hub, 1);
    let (prepared, ready) = futures::channel::oneshot::channel();
    let (gate, waiting) = futures::channel::oneshot::channel();
    let service: harness::Client = capnp_rpc::new_client(DelayedTail {
        prepared: RefCell::new(Some(prepared)),
        gate: RefCell::new(Some(waiting)),
    });
    let task = tokio::task::spawn_local(RpcSystem::new(Box::new(network), Some(service.client)));
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
    let return_::Results(p) = ret.unwrap().which().unwrap() else {
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
        c.set_interface_id(harness::Client::TYPE_ID);
        c.set_method_id(4);
        c.reborrow().init_target().set_imported_cap(export);
        let mut p = c.init_params();
        let mut caps = vec![];
        {
            let mut content = p.reborrow().get_content();
            content.imbue_mut(&mut caps);
            let dummy: harness::Client = capnp_rpc::new_client(Tail);
            content
                .init_as::<harness::tail_cap_params::Builder>()
                .set_cap(dummy);
        }
        p.init_cap_table(1).get(0).set_sender_promise(77);
    });
    ready.await.unwrap();
    let mut gate = Some(gate);
    let mut returned = None;
    let mut actual = [0; 4];
    for step in steps {
        match step.action.as_str() {
            "resolve" => {
                peer.send(|m| {
                    let mut r = m.init_resolve();
                    r.set_promise_id(77);
                    r.init_cap().set_receiver_hosted(export);
                });
                settle().await;
                actual[0] = 1;
            }
            "send" => {
                gate.take().unwrap().send(()).unwrap();
                returned = Some(loop {
                    let msg = peer.recv().await;
                    match msg
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    {
                        message::Return(r) => {
                            let r = r.unwrap();
                            assert_eq!(r.get_answer_id(), 1);
                            let return_::Results(p) = r.which().unwrap() else {
                                panic!("resolved local request was not forwarded")
                            };
                            let cap_descriptor::SenderHosted(id) =
                                p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
                            else {
                                panic!()
                            };
                            break id;
                        }
                        message::Release(_) => {}
                        _ => panic!("unexpected message after local resolution"),
                    }
                });
                actual[1] = 1;
                actual[3] = 1;
            }
            "use" => {
                peer.send(|m| {
                    let mut c = m.init_call();
                    c.set_question_id(2);
                    c.set_interface_id(harness::Client::TYPE_ID);
                    c.set_method_id(0);
                    c.reborrow()
                        .init_target()
                        .set_imported_cap(returned.unwrap());
                    c.init_params()
                        .get_content()
                        .init_as::<harness::echo_params::Builder>();
                });
                loop {
                    let msg = peer.recv().await;
                    match msg
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    {
                        message::Return(r) => {
                            let r = r.unwrap();
                            assert_eq!(r.get_answer_id(), 2);
                            let return_::Results(p) = r.which().unwrap() else {
                                panic!()
                            };
                            assert_eq!(
                                p.unwrap()
                                    .get_content()
                                    .get_as::<harness::value::Reader>()
                                    .unwrap()
                                    .get_value(),
                                9
                            );
                            break;
                        }
                        message::Release(_) => {}
                        _ => panic!(),
                    }
                }
                actual[2] = 1;
            }
            other => panic!("unknown routing action {other}"),
        }
        assert_eq!(actual, step.state, "{step:?} in {steps:?}");
    }
    task.abort();
    let _ = task.await;
}
#[tokio::test(flavor = "current_thread")]
async fn tail_request_resolved_locally_after_construction_uses_forwarding() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(
                Duration::from_secs(5),
                routing_trace(&[
                    RoutingStep {
                        action: "resolve".into(),
                        state: [1, 0, 0, 0],
                    },
                    RoutingStep {
                        action: "send".into(),
                        state: [1, 1, 0, 1],
                    },
                    RoutingStep {
                        action: "use".into(),
                        state: [1, 1, 1, 1],
                    },
                ]),
            )
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_tail_routing_traces() {
    let path = reproto_test_support::verification::input("REPROTO_TAIL_ROUTING_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<RoutingStep>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                for trace in traces {
                    routing_trace(&trace).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pipeline_only_hint_skips_tail_transfer() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                Fixture::with_hint(true).await.cleanup().await;
            })
            .await
            .unwrap();
        })
        .await;
}

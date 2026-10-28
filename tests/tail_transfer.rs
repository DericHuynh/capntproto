mod support;
use capnp::traits::{HasTypeId, ImbueMut};
use capnp_rpc::{
    rpc_capnp::{call, cap_descriptor, message, message_target, promised_answer, return_},
    Connection,
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::FutureExt;
use std::{cell::RefCell, rc::Rc, time::Duration};
use support::{Endpoint, Hub};
struct Tail;
impl harness::Server for Tail {
    fn tail_cap(
        self: Rc<Self>,
        context: harness::TailCapContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (p, r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let request = p.get()?.get_cap()?.pending_request();
            r.hook.tail_call(request.hook).await
        })
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

fn send_tail_call(peer: &mut Endpoint, export: u32, only_pipeline: bool) {
    send_tail_call_for(peer, export, harness::Client::TYPE_ID, 4, only_pipeline);
}
fn send_tail_call_for(
    peer: &mut Endpoint,
    export: u32,
    interface: u64,
    method: u16,
    only_pipeline: bool,
) {
    peer.send(|m| {
        let mut c = m.init_call();
        c.set_question_id(1);
        c.set_interface_id(interface);
        c.set_method_id(method);
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
}

async fn tail_peer(
    portable: bool,
    service: harness::Client,
) -> (
    Rc<RefCell<Hub>>,
    Endpoint,
    u32,
    tokio::task::JoinHandle<capnp::Result<()>>,
) {
    tail_peer_with_bootstrap(portable, service.client).await
}
async fn tail_peer_with_bootstrap(
    portable: bool,
    service: capnp::capability::Client,
) -> (
    Rc<RefCell<Hub>>,
    Endpoint,
    u32,
    tokio::task::JoinHandle<capnp::Result<()>>,
) {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let system =
        support::driver::System::new(portable, Box::new(Hub::network(&hub, 1)), Some(service));
    let task = tokio::task::spawn_local(system.run());
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
        panic!("expected bootstrap return")
    };
    let return_::Results(payload) = ret.unwrap().which().unwrap() else {
        panic!("expected bootstrap capability")
    };
    let cap_descriptor::SenderHosted(export) = payload
        .unwrap()
        .get_cap_table()
        .unwrap()
        .get(0)
        .which()
        .unwrap()
    else {
        panic!("expected bootstrap export")
    };
    (hub, peer, export, task)
}

struct Fixture {
    peer: Endpoint,
    tail: u32,
    child: Option<u32>,
    tail_finishes: u32,
    child_returns: u32,
    child_ack: u32,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    disconnect: Option<capnp::capability::Promise<(), capnp::Error>>,
    disconnected: bool,
}
impl Fixture {
    async fn with_driver(portable: bool, only_pipeline: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let service: harness::Client = capnp_rpc::new_client(Tail);
        let mut system =
            support::driver::System::new(portable, Box::new(network), Some(service.client));
        system.set_outgoing_call_limit(2);
        if !only_pipeline {
            system.set_flow_limit(1);
        }
        let disconnect = Some(system.disconnect());
        let task = tokio::task::spawn_local(system.run());
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
        send_tail_call(&mut peer, export, only_pipeline);
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
                disconnect,
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
        Self {
            peer,
            tail: tail.unwrap(),
            child: None,
            tail_finishes: 0,
            child_returns: 0,
            child_ack: 0,
            task,
            disconnect,
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
    for portable in [false, true] {
        replay_driver(steps, portable).await;
    }
}
async fn replay_driver(steps: &[Step], portable: bool) {
    let mut f = Fixture::with_driver(portable, false).await;
    for s in steps {
        match s.action.as_str() {
            "ack" => f.ack(),
            "finish" => finish(&mut f.peer, 1),
            "pipeline" => f.pipeline(),
            "reply" => f.reply_child(),
            "disconnect" => {
                f.disconnected = true;
                f.disconnect.take().unwrap().await.unwrap();
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
    let path = capntproto_test_support::verification::input("CAPNTPROTO_TAIL_TRANSFER_TRACES")
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
    fn tail_cap(
        self: Rc<Self>,
        context: harness::TailCapContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (p, r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let request = p.get()?.get_cap()?.pending_request();
            self.prepared.borrow_mut().take().unwrap().send(()).unwrap();
            let gate = self.gate.borrow_mut().take().unwrap();
            gate.await.unwrap();
            r.hook.tail_call(request.hook).await
        })
    }
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let cap: harness::Client = capnp_rpc::new_client_from_rc(self);
            r.get().set_cap(cap);
            Ok(())
        })
    }
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            r.get().set_value(9);
            Ok(())
        })
    }
}
impl capntproto_test_support::cancellation_policy_capnp::policy::Server for DelayedTail {
    fn pending(
        self: Rc<Self>,
        context: capntproto_test_support::cancellation_policy_capnp::policy::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let request = params
                .hook
                .get()?
                .get_as::<harness::tail_cap_params::Reader>()?
                .get_cap()?
                .pending_request();
            self.prepared.borrow_mut().take().unwrap().send(()).unwrap();
            self.gate.borrow_mut().take().unwrap().await.unwrap();
            results.hook.tail_call(request.hook).await
        })
    }
}
#[derive(serde::Deserialize, Debug)]
struct RoutingStep {
    action: String,
    state: [u32; 4],
}
async fn routing_trace(steps: &[RoutingStep]) {
    for portable in [false, true] {
        routing_trace_driver(steps, portable).await;
    }
}
async fn routing_trace_driver(steps: &[RoutingStep], portable: bool) {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let network = Hub::network(&hub, 1);
    let (prepared, ready) = futures::channel::oneshot::channel();
    let (gate, waiting) = futures::channel::oneshot::channel();
    let service: harness::Client = capnp_rpc::new_client(DelayedTail {
        prepared: RefCell::new(Some(prepared)),
        gate: RefCell::new(Some(waiting)),
    });
    let task = tokio::task::spawn_local(
        support::driver::System::new(portable, Box::new(network), Some(service.client)).run(),
    );
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
    let path = capntproto_test_support::verification::input("CAPNTPROTO_TAIL_ROUTING_TRACES")
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
                for portable in [false, true] {
                    Fixture::with_driver(portable, true).await.cleanup().await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pipeline_only_tail_forwarding_keeps_parent_pending() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                for portable in [false, true] {
                    for downstream_return in [false, true] {
                        let (hub, mut peer, export, task) =
                            tail_peer(portable, capnp_rpc::new_client(Tail)).await;
                        send_tail_call(&mut peer, export, true);
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
                                message::Call(call) => {
                                    let call = call.unwrap();
                                    assert!(call.get_only_promise_pipeline());
                                    assert!(matches!(
                                        call.get_send_results_to().which().unwrap(),
                                        call::send_results_to::Caller(())
                                    ));
                                    assert_ne!(call.get_question_id() & (1 << 31), 0);
                                    break call.get_question_id();
                                }
                                message::Release(_) => (),
                                _ => {
                                    panic!("expected pipeline-only child call; portable={portable}")
                                }
                            }
                        };
                        // An unexpected downstream Return must neither complete
                        // the parent nor release its pipeline before Finish.
                        if downstream_return {
                            peer.send(|m| {
                                let mut ret = m.init_return();
                                ret.set_answer_id(question);
                                ret.set_release_param_caps(false);
                                ret.set_canceled(());
                            });
                        }
                        settle().await;
                        let mut child_finished = false;
                        while let Some(msg) = peer.receive_incoming_message().now_or_never() {
                            let msg = msg.unwrap().unwrap();
                            match msg
                                .get_body()
                                .unwrap()
                                .get_as::<message::Reader>()
                                .unwrap()
                                .which()
                                .unwrap()
                            {
                                message::Release(_) => (),
                                _ => panic!(
                                "pipeline-only call completed before Finish; portable={portable}"
                            ),
                            }
                        }
                        finish(&mut peer, 1);
                        while !child_finished {
                            let msg = peer.recv().await;
                            match msg
                                .get_body()
                                .unwrap()
                                .get_as::<message::Reader>()
                                .unwrap()
                                .which()
                                .unwrap()
                            {
                                message::Finish(finish) => {
                                    assert_eq!(finish.unwrap().get_question_id(), question);
                                    child_finished = true;
                                }
                                message::Release(_) => (),
                                _ => panic!("pipeline-only tail sent Return; portable={portable}"),
                            }
                        }
                        settle().await;
                        while let Some(msg) = peer.receive_incoming_message().now_or_never() {
                            let msg = msg.unwrap().unwrap();
                            match msg
                                .get_body()
                                .unwrap()
                                .get_as::<message::Reader>()
                                .unwrap()
                                .which()
                                .unwrap()
                            {
                                message::Release(_) => (),
                                _ => panic!("unexpected tail cleanup; portable={portable}"),
                            }
                        }
                        assert_eq!(hub.borrow().provision_count(), 0);
                        task.abort();
                        let _ = task.await;
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}

struct InitializedTail {
    populated: bool,
}

#[tokio::test(flavor = "current_thread")]
async fn pipeline_only_tail_preserves_dependent_calls_after_parent_finish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(
                    Duration::from_secs(5),
                    dependent_pipeline_after_finish(portable),
                )
                .await
                .unwrap();
            }
        })
        .await;
}

async fn dependent_pipeline_after_finish(portable: bool) {
    let (_hub, mut peer, export, task) = tail_peer(portable, capnp_rpc::new_client(Tail)).await;
    send_tail_call(&mut peer, export, true);
    let parent = loop {
        let msg = peer.recv().await;
        match msg
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        {
            message::Call(call) => {
                let call = call.unwrap();
                assert!(call.get_only_promise_pipeline());
                break call.get_question_id();
            }
            message::Release(_) => (),
            _ => panic!("expected tail child; portable={portable}"),
        }
    };
    peer.send(|m| {
        let mut call = m.init_call();
        call.set_question_id(2);
        call.set_interface_id(harness::Client::TYPE_ID);
        call.set_method_id(0);
        let mut target = call.reborrow().init_target().init_promised_answer();
        target.set_question_id(1);
        target.init_transform(1).get(0).set_get_pointer_field(0);
        call.init_params()
            .get_content()
            .init_as::<harness::echo_params::Builder>()
            .set_value(9);
    });
    let (child, redirected) = loop {
        let msg = peer.recv().await;
        match msg
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        {
            message::Call(call) => {
                let call = call.unwrap();
                let message_target::PromisedAnswer(target) =
                    call.get_target().unwrap().which().unwrap()
                else {
                    panic!("promised target")
                };
                let target = target.unwrap();
                assert_eq!(target.get_question_id(), parent);
                assert!(!call.get_only_promise_pipeline());
                let redirected = matches!(
                    call.get_send_results_to().which().unwrap(),
                    call::send_results_to::Yourself(())
                );
                break (call.get_question_id(), redirected);
            }
            message::Release(_) => (),
            _ => panic!("parent retired before dependent call publication; portable={portable}"),
        }
    };
    finish(&mut peer, 1);
    peer.send(|m| {
        let mut ret = m.init_return();
        ret.set_answer_id(child);
        ret.set_release_param_caps(false);
        if redirected {
            ret.set_results_sent_elsewhere(());
        } else {
            ret.init_results()
                .get_content()
                .init_as::<harness::value::Builder>()
                .set_value(9);
        }
    });
    let mut parent_finished = false;
    let mut child_returned = false;
    while !parent_finished || !child_returned {
        let msg = peer.recv().await;
        match msg
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        {
            message::Return(ret) => {
                let ret = ret.unwrap();
                assert_eq!(
                    ret.get_answer_id(),
                    2,
                    "pipeline-only parent must not return"
                );
                match ret.which().unwrap() {
                    return_::Results(payload) => {
                        assert!(!redirected);
                        assert_eq!(
                            payload
                                .unwrap()
                                .get_content()
                                .get_as::<harness::value::Reader>()
                                .unwrap()
                                .get_value(),
                            9
                        );
                    }
                    return_::TakeFromOtherQuestion(id) => {
                        assert!(redirected);
                        assert_eq!(id, child);
                    }
                    _ => panic!("unexpected dependent return; portable={portable}"),
                }
                assert!(!child_returned);
                child_returned = true;
            }
            message::Finish(finish) => {
                let id = finish.unwrap().get_question_id();
                if id == parent {
                    assert!(!parent_finished);
                    parent_finished = true;
                } else {
                    assert_eq!(id, child);
                }
            }
            message::Release(_) => (),
            _ => panic!("unexpected dependent tail message; portable={portable}"),
        }
    }
    finish(&mut peer, 2);
    settle().await;
    task.abort();
    let _ = task.await;
}

struct CompletedPipeline(Rc<std::cell::Cell<bool>>);
struct PipelineValue(Rc<std::cell::Cell<bool>>);
impl Drop for PipelineValue {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl harness::Server for PipelineValue {
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        results.get().set_value(42);
        capnp::capability::Promise::ok(())
    }
}
impl harness::Server for CompletedPipeline {
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        self.0.set(true);
        results
            .get()
            .set_cap(capnp_rpc::new_client(PipelineValue(self.0.clone())));
        capnp::capability::Promise::ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn completed_pipeline_answer_remains_callable_without_return_until_finish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                tokio::time::timeout(Duration::from_secs(5), completed_pipeline_answer(portable))
                    .await
                    .unwrap();
            }
        })
        .await;
}

async fn completed_pipeline_answer(portable: bool) {
    let alive = Rc::new(std::cell::Cell::new(false));
    let (_hub, mut peer, export, task) = tail_peer(
        portable,
        capnp_rpc::new_client(CompletedPipeline(alive.clone())),
    )
    .await;
    peer.send(|m| {
        let mut call = m.init_call();
        call.set_question_id(1);
        call.set_interface_id(harness::Client::TYPE_ID);
        call.set_method_id(3);
        call.set_only_promise_pipeline(true);
        call.reborrow().init_target().set_imported_cap(export);
        call.init_params()
            .get_content()
            .init_as::<harness::pending_params::Builder>();
    });
    settle().await;
    assert!(alive.get());
    while let Some(msg) = peer.receive_incoming_message().now_or_never() {
        let msg = msg.unwrap().unwrap();
        assert!(
            matches!(
                msg.get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap(),
                message::Release(_)
            ),
            "completed pipeline-only answer must not send Return; portable={portable}"
        );
    }
    peer.send(|m| {
        let mut call = m.init_call();
        call.set_question_id(2);
        call.set_interface_id(harness::Client::TYPE_ID);
        call.set_method_id(0);
        let mut target = call.reborrow().init_target().init_promised_answer();
        target.set_question_id(1);
        target.init_transform(1).get(0).set_get_pointer_field(0);
        call.init_params()
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
            message::Return(ret) => {
                let ret = ret.unwrap();
                assert_eq!(ret.get_answer_id(), 2);
                let return_::Results(payload) = ret.which().unwrap() else {
                    panic!("dependent result")
                };
                assert_eq!(
                    payload
                        .unwrap()
                        .get_content()
                        .get_as::<harness::value::Reader>()
                        .unwrap()
                        .get_value(),
                    42
                );
                break;
            }
            message::Release(_) => (),
            _ => panic!("unexpected completed pipeline message; portable={portable}"),
        }
    }
    finish(&mut peer, 1);
    finish(&mut peer, 2);
    settle().await;
    assert!(
        !alive.get(),
        "Finish must release retained result capabilities; portable={portable}"
    );
    task.abort();
    let _ = task.await;
}

impl harness::Server for InitializedTail {
    fn tail_cap(
        self: Rc<Self>,
        context: harness::TailCapContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let cap = params.get()?.get_cap()?;
            let mut initialized = results.get();
            if self.populated {
                initialized.set_cap(cap.clone());
            }
            results.hook.tail_call(cap.pending_request().hook).await
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn initialized_tail_results_are_rejected_by_both_drivers() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                for portable in [false, true] {
                    for populated in [false, true] {
                        let (_hub, mut peer, export, task) = tail_peer(
                            portable,
                            capnp_rpc::new_client(InitializedTail { populated }),
                        )
                        .await;
                        send_tail_call(&mut peer, export, false);
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
                                message::Return(ret) => {
                                    let ret = ret.unwrap();
                                    assert_eq!(ret.get_answer_id(), 1);
                                    let return_::Exception(error) = ret.which().unwrap() else {
                                        panic!("expected initialization error; portable={portable}")
                                    };
                                    assert_eq!(
                                        error.unwrap().get_reason().unwrap().to_str().unwrap(),
                                        "tail_call after initializing results"
                                    );
                                    break;
                                }
                                message::Release(_) => (),
                                _ => panic!(
                                    "initialized results sent a tail call; portable={portable}"
                                ),
                            }
                        }
                        finish(&mut peer, 1);
                        settle().await;
                        task.abort();
                        let _ = task.await;
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn protected_tail_after_parent_finish_keeps_the_connection_usable() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (portable, finish_before_resume) in [false, true].into_iter().flat_map(|portable| [true, false].map(|early| (portable, early))) {
                tokio::time::timeout(Duration::from_secs(2), async {
                    let (prepared, ready) = futures::channel::oneshot::channel();
                    let (gate, waiting) = futures::channel::oneshot::channel();
                    let service: capntproto_test_support::cancellation_policy_capnp::policy::Client = capnp_rpc::new_client(DelayedTail {
                        prepared: RefCell::new(Some(prepared)),
                        gate: RefCell::new(Some(waiting)),
                    });
                    let (_hub, mut peer, export, task) = tail_peer_with_bootstrap(portable, service.client).await;
                    send_tail_call_for(&mut peer, export, capntproto_test_support::cancellation_policy_capnp::policy::Client::TYPE_ID, 0, false);
                    ready.await.unwrap();
                    if finish_before_resume {
                        finish(&mut peer, 1);
                        settle().await;
                        gate.send(()).unwrap();
                    } else {
                        gate.send(()).unwrap();
                        finish(&mut peer, 1);
                    }
                    let (mut child, mut parent) = (false, false);
                    while !child || !parent {
                        let input = peer.recv().await;
                        match input
                            .get_body()
                            .unwrap()
                            .get_as::<message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap()
                        {
                            message::Call(call) => {
                                let call = call.unwrap();
                                let redirected = matches!(call.get_send_results_to().which().unwrap(), call::send_results_to::Yourself(()));
                                let id = call.get_question_id();
                                assert!(!child);
                                child = true;
                                peer.send(|m| {
                                    let mut reply = m.init_return();
                                    reply.set_answer_id(id);
                                    reply.set_release_param_caps(false);
                                    if redirected { reply.set_results_sent_elsewhere(()); }
                                    else {
                                        let mut error = reply.init_exception();
                                        error.set_type(capnp_rpc::rpc_capnp::exception::Type::Failed);
                                        error.set_reason("completed protected child");
                                    }
                                });
                            }
                            message::Return(reply) => {
                                assert_eq!(reply.unwrap().get_answer_id(), 1);
                                parent = true;
                            }
                            message::Finish(_) | message::Release(_) => (),
                            message::Abort(_) => {
                                panic!("portable={portable}: late tail aborted the connection")
                            }
                            _ => panic!("unexpected tail message"),
                        }
                    }
                    peer.send(|m| m.init_bootstrap().set_question_id(2));
                    loop {
                        let input = peer.recv().await;
                        match input
                            .get_body()
                            .unwrap()
                            .get_as::<message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap()
                        {
                            message::Return(reply) => {
                                let reply = reply.unwrap();
                                assert_eq!(reply.get_answer_id(), 2);
                                let return_::Results(payload) = reply.which().unwrap() else {
                                    panic!("healthy service response")
                                };
                                assert_eq!(payload.unwrap().get_cap_table().unwrap().len(), 1);
                                break;
                            }
                            message::Finish(_) | message::Release(_) => (),
                            _ => panic!(
                                "portable={portable}: connection failed after protected tail"
                            ),
                        }
                    }
                    task.abort();
                })
                .await
                .unwrap_or_else(|_| panic!("portable={portable}: protected tail stalled"));
            }
        })
        .await;
}

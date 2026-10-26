#[allow(dead_code)]
mod support;
use capnp::{
    capability::ResponsePromise,
    traits::{HasTypeId, ImbueMut},
};
use capnp_rpc::rpc_capnp::{cap_descriptor, message, return_};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::{channel::oneshot, FutureExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::{Endpoint, Hub};
fn driver_cases() -> impl Iterator<Item = [bool; 3]> {
    (0u8..8).map(|mask| [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0])
}
const ID: u32 = 1 << 30;
const TOKEN: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 99];
struct Echo;
impl harness::Server for Echo {
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            r.get().set_value(73);
            Ok(())
        })
    }
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            r.get().set_cap(capnp_rpc::new_client(Echo));
            Ok(())
        })
    }
}
struct Tail(harness::Client);
struct Factory(RefCell<Option<capnp::capability::Client>>);
impl<VatId> capnp_rpc::BootstrapFactory<VatId> for Factory {
    fn create_for(&self, _: &VatId) -> capnp::Result<capnp::capability::Client> {
        Ok(self.0.borrow().as_ref().unwrap().clone())
    }
}
impl harness::Server for Tail {
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            r.tail_call(self.0.pending_request()).await
        })
    }
}
struct Gated(RefCell<Option<oneshot::Receiver<()>>>, Rc<Cell<u32>>);
impl harness::Server for Gated {
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            self.1.set(self.1.get() + 1);
            let gate = self.0.borrow_mut().take().unwrap();
            gate.await
                .map_err(|_| capnp::Error::failed("gate dropped".into()))?;
            r.get().set_cap(capnp_rpc::new_client(Echo));
            Ok(())
        })
    }
}
struct EarlyPipeline(RefCell<Option<oneshot::Receiver<()>>>, Rc<Cell<u32>>);
impl harness::Server for EarlyPipeline {
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            self.1.set(self.1.get() + 1);
            r.get().set_cap(capnp_rpc::new_client(Echo));
            r.hook.set_pipeline()?;
            let gate = self.0.borrow_mut().take().unwrap();
            gate.await
                .map_err(|_| capnp::Error::failed("gate dropped".into()))
        })
    }
}
struct RetainedResults(Rc<RefCell<Option<harness::PendingResults>>>);
impl harness::Server for RetainedResults {
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        results.get().set_cap(capnp_rpc::new_client(Echo));
        *self.0.borrow_mut() = Some(results);
        capnp::capability::Promise::ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn adopted_work_waits_for_retained_results_without_repolling_completed_execution() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in driver_cases() {
                for early in [false, true] {
                    tokio::time::timeout(Duration::from_secs(5), async {
                        let hub = Rc::new(RefCell::new(Hub::default()));
                        hub.borrow_mut().introductions = true;
                        enable(&hub);
                        let retained = Rc::new(RefCell::new(None));
                        let service: harness::Client =
                            capnp_rpc::new_client(RetainedResults(retained.clone()));
                        let callee = support::driver::System::new(
                            portable[2],
                            Box::new(Hub::network(&hub, 3)),
                            Some(service.client),
                        );
                        let factory = Rc::new(Factory(RefCell::new(None)));
                        let mut relay = support::driver::System::with_factory(
                            portable[1],
                            Box::new(Hub::network(&hub, 2)),
                            2,
                            factory.clone(),
                        );
                        let target: harness::Client = relay.bootstrap(3);
                        let ready = target.client.hook.when_resolved();
                        let service: harness::Client = capnp_rpc::new_client(Tail(target));
                        *factory.0.borrow_mut() = Some(service.client);
                        let callee = tokio::task::spawn_local(callee.run());
                        let relay = tokio::task::spawn_local(relay.run());
                        ready.await.unwrap();
                        let mut caller = support::driver::System::new(
                            portable[0],
                            Box::new(Hub::network(&hub, 1)),
                            None,
                        );
                        let target: harness::Client = caller.bootstrap(2);
                        let caller = tokio::task::spawn_local(caller.run());
                        let mut request = target.pending_request().send();
                        while retained.borrow().is_none() {
                            tokio::task::yield_now().await;
                        }
                        drain().await;
                        assert!((&mut request.promise).now_or_never().is_none());
                        if early {
                            // Park a child on the adopted answer before publishing its
                            // early pipeline. No additional network input may be needed
                            // to discover the newly callable target.
                            let mut child =
                                request.pipeline.get_cap().echo_request().send().promise;
                            drain().await;
                            assert!((&mut child).now_or_never().is_none());
                            retained
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .hook
                                .set_pipeline()
                                .unwrap();
                            assert_eq!(child.await.unwrap().get().unwrap().get_value(), 73);
                            assert!((&mut request.promise).now_or_never().is_none());
                        }
                        // Execution is complete, but the application still owns its
                        // result builder. Returning it must resume collection once.
                        drop(retained.borrow_mut().take());
                        let response = request.promise.await.unwrap();
                        let cap = response.get().unwrap().get_cap().unwrap();
                        assert_eq!(
                            cap.echo_request()
                                .send()
                                .promise
                                .await
                                .unwrap()
                                .get()
                                .unwrap()
                                .get_value(),
                            73
                        );
                        for task in [caller, relay, callee] {
                            assert!(
                                !task.is_finished(),
                                "returning retained results stopped RPC"
                            );
                            task.abort();
                        }
                    })
                    .await
                    .unwrap_or_else(|_| {
                        panic!("retained results stalled: portable={portable:?}, early={early}")
                    });
                }
            }
        })
        .await;
}

async fn drain() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}
async fn recv(peer: &mut Endpoint) -> Box<dyn capnp_rpc::IncomingMessage> {
    tokio::time::timeout(Duration::from_secs(2), peer.recv())
        .await
        .unwrap()
}
fn enable(hub: &Rc<RefCell<Hub>>) {
    hub.borrow_mut().answer_adoption = true;
}

#[tokio::test(flavor = "current_thread")]
async fn third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in driver_cases() {
                for budget in (1..=8).chain([64]) {
                    let stage = Cell::new("connect");
                    tokio::time::timeout(Duration::from_secs(5), async {
                        let hub = Rc::new(RefCell::new(Hub::default()));
                        hub.borrow_mut().introductions = true;
                        enable(&hub);
                        let mut options = support::driver::options();
                        options.poll_budget = std::num::NonZeroUsize::new(budget).unwrap();
                        let (gate, blocked) = oneshot::channel();
                        let calls = Rc::new(Cell::new(0));
                        let service: harness::Client = capnp_rpc::new_client(Gated(
                            RefCell::new(Some(blocked)),
                            calls.clone(),
                        ));
                        let callee = tokio::task::spawn_local(
                            support::driver::System::with_options(
                                portable[2],
                                Box::new(Hub::network(&hub, 3)),
                                Some(service.client),
                                options,
                            )
                            .run(),
                        );
                        let factory = Rc::new(Factory(RefCell::new(None)));
                        let mut relay = support::driver::System::with_factory_options(
                            portable[1],
                            Box::new(Hub::network(&hub, 2)),
                            2,
                            factory.clone(),
                            options,
                        );
                        let remote: harness::Client = relay.bootstrap(3);
                        let ready = remote.client.hook.when_resolved();
                        let tail: harness::Client = capnp_rpc::new_client(Tail(remote));
                        *factory.0.borrow_mut() = Some(tail.client);
                        let relay = tokio::task::spawn_local(relay.run());
                        stage.set("resolve host");
                        ready.await.unwrap();
                        let mut caller = support::driver::System::with_options(
                            portable[0],
                            Box::new(Hub::network(&hub, 1)),
                            None,
                            options,
                        );
                        let remote: harness::Client = caller.bootstrap(2);
                        let caller = tokio::task::spawn_local(caller.run());
                        let request = remote.pending_request().send();
                        let early = request.pipeline.get_cap().echo_request().send().promise;
                        stage.set("dispatch parent");
                        while calls.get() == 0 {
                            tokio::task::yield_now().await;
                        }
                        // Let the worker certify idle while the pipelined target
                        // is unresolved. Opening the gate must reactivate dispatch
                        // without additional IO or manual polling of the driver.
                        drain().await;
                        gate.send(()).unwrap();
                        stage.set("parent reply");
                        let response = request.promise.await.unwrap();
                        stage.set("pipelined child reply");
                        assert_eq!(early.await.unwrap().get().unwrap().get_value(), 73);
                        assert_eq!(calls.get(), 1);
                        drop(response);
                        caller.abort();
                        relay.abort();
                        callee.abort();
                    })
                    .await
                    .unwrap_or_else(|_| {
                        panic!(
                        "transfer stalled: portable={portable:?}, poll budget={budget}, stage={}",
                        stage.get()
                    )
                    });
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn automatic_third_party_tail_returns_capability_and_survives_relay_drop() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (portable, allow) in (0u8..8).flat_map(|mask| {
                [false, true]
                    .map(move |allow| ([mask & 1 != 0, mask & 2 != 0, mask & 4 != 0], allow))
            }) {
                let hub = Rc::new(RefCell::new(Hub::default()));
                hub.borrow_mut().introductions = true;
                let a = Hub::network(&hub, 1);
                let b = Hub::network(&hub, 2);
                let c = Hub::network(&hub, 3);
                enable(&hub);
                hub.borrow_mut()
                    .connect(1, 2)
                    .status()
                    .borrow_mut()
                    .third_party_answers = Some(allow);
                let (gate, rx) = oneshot::channel();
                let calls = Rc::new(Cell::new(0));
                let service: harness::Client =
                    capnp_rpc::new_client(Gated(RefCell::new(Some(rx)), calls.clone()));
                let callee = tokio::task::spawn_local(
                    support::driver::System::new(portable[2], Box::new(c), Some(service.client))
                        .run(),
                );
                let factory = Rc::new(Factory(RefCell::new(None)));
                let mut relay = support::driver::System::with_factory(
                    portable[1],
                    Box::new(b),
                    2,
                    factory.clone(),
                );
                let remote: harness::Client = relay.bootstrap(3);
                let ready = remote.client.hook.when_resolved();
                let service: harness::Client = capnp_rpc::new_client(Tail(remote));
                *factory.0.borrow_mut() = Some(service.client);
                let relay = tokio::task::spawn_local(relay.run());
                ready.await.unwrap();
                let mut caller = support::driver::System::new(portable[0], Box::new(a), None);
                let remote: harness::Client = caller.bootstrap(2);
                let caller = tokio::task::spawn_local(caller.run());
                let request = remote.pending_request().send();
                let early = request.pipeline.get_cap().echo_request().send().promise;
                drain().await;
                assert_eq!(calls.get(), 1, "portable={portable:?}, allow={allow}");
                gate.send(()).unwrap();
                let response = tokio::time::timeout(Duration::from_secs(2), request.promise)
                    .await
                    .unwrap()
                    .unwrap_or_else(|e| panic!("root {portable:?}/{allow}: {e}"));
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), early)
                        .await
                        .unwrap_or_else(|_| panic!("early pipeline stalled {portable:?}/{allow}"))
                        .unwrap_or_else(|e| panic!("response {portable:?}/{allow}: {e}"))
                        .get()
                        .unwrap()
                        .get_value(),
                    73
                );
                let cap = response.get().unwrap().get_cap().unwrap();
                drop(response);
                if allow {
                    relay.abort();
                    drain().await;
                }
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), cap.echo_request().send().promise)
                        .await
                        .unwrap_or_else(|_| panic!(
                            "result after relay drop stalled {portable:?}/{allow}"
                        ))
                        .unwrap_or_else(|e| panic!(
                            "result after relay drop {portable:?}/{allow}: {e}"
                        ))
                        .get()
                        .unwrap()
                        .get_value(),
                    73
                );
                assert_eq!(
                    hub.borrow_mut()
                        .connect(3, 1)
                        .status()
                        .borrow()
                        .adoptions_sent,
                    if allow {
                        if portable[1] {
                            1
                        } else {
                            2
                        }
                    } else {
                        0
                    }
                );
                caller.abort();
                relay.abort();
                callee.abort();
            }
        })
        .await;
}

fn bootstrap(peer: &mut Endpoint, q: u32) {
    peer.send(|m| {
        let mut r = m.init_return();
        r.set_answer_id(q);
        let mut p = r.init_results();
        let mut caps = vec![];
        {
            let mut c = p.reborrow().get_content();
            c.imbue_mut(&mut caps);
            let cap: harness::Client = capnp_rpc::new_client(Echo);
            c.set_as_capability(cap.client.hook);
        }
        p.init_cap_table(1).get(0).set_sender_hosted(77);
    });
}
struct Caller {
    hub: Rc<RefCell<Hub>>,
    relay: Endpoint,
    callee: Endpoint,
    question: u32,
    pending: Option<ResponsePromise<harness::pending_results::Owned>>,
    pipeline: Option<harness::pending_results::Pipeline>,
    calls: Vec<(bool, u32)>,
    response: Option<capnp::capability::Response<harness::pending_results::Owned>>,
    outcome: u32,
    old_finishes: usize,
    direct_finishes: usize,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    _networks: Vec<support::Network>,
}
impl Drop for Caller {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Caller {
    async fn new() -> Self {
        Self::with_permission(true).await
    }
    async fn with_permission(allow: bool) -> Self {
        Self::setup(allow, false).await
    }
    async fn with_pipeline() -> Self {
        Self::setup(true, true).await
    }
    async fn setup(allow: bool, keep_pipeline: bool) -> Self {
        Self::with_service(allow, keep_pipeline, capnp_rpc::new_client(Echo)).await
    }
    async fn with_service(allow: bool, keep_pipeline: bool, service: harness::Client) -> Self {
        Self::with_driver(false, allow, keep_pipeline, service).await
    }
    async fn with_driver(
        portable: bool,
        allow: bool,
        keep_pipeline: bool,
        service: harness::Client,
    ) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let networks = vec![Hub::network(&hub, 2), Hub::network(&hub, 3)];
        enable(&hub);
        let mut system =
            support::driver::System::new(portable, Box::new(network), Some(service.client));
        let cap: harness::Client = system.bootstrap(2);
        let task = tokio::task::spawn_local(system.run());
        let mut relay = hub.borrow_mut().connect(2, 1);
        let callee = hub.borrow_mut().connect(3, 1);
        let m = recv(&mut relay).await;
        let message::Bootstrap(b) = m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        bootstrap(&mut relay, b.unwrap().get_question_id());
        let cap = capnp::capability::get_resolved_cap(cap).await;
        drain().await;
        while capnp_rpc::Connection::receive_incoming_message(&mut relay)
            .now_or_never()
            .is_some()
        {}
        hub.borrow_mut()
            .connect(1, 2)
            .status()
            .borrow_mut()
            .third_party_answers = Some(allow);
        let request = cap.pending_request().send();
        let pipeline = keep_pipeline.then_some(request.pipeline);
        let pending = request.promise;
        let m = recv(&mut relay).await;
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
        assert_eq!(c.get_allow_third_party_tail_call(), allow);
        let question = c.get_question_id();
        Self {
            hub,
            relay,
            callee,
            question,
            pending: Some(pending),
            pipeline,
            calls: vec![],
            response: None,
            outcome: 0,
            old_finishes: 0,
            direct_finishes: 0,
            task,
            _networks: networks,
        }
    }
    fn redirect(&mut self) {
        let mut token = vec![3];
        token.extend(TOKEN);
        let q = self.question;
        self.relay.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(q);
            r.init_await_from_third_party()
                .set_as::<capnp::data::Owned>(&token[..])
                .unwrap();
        });
    }
    fn adopt(&mut self) {
        self.callee.send(|m| {
            let mut a = m.init_third_party_answer();
            a.set_answer_id(ID);
            a.get_completion()
                .set_as::<capnp::data::Owned>(&TOKEN[..])
                .unwrap();
        });
    }
    fn complete(&mut self, broken: bool) {
        self.callee.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(ID);
            if broken {
                r.init_exception().set_reason("callee failed");
                return;
            }
            let mut p = r.init_results();
            let mut caps = vec![];
            {
                let mut content = p.reborrow().get_content();
                content.imbue_mut(&mut caps);
                content
                    .init_as::<harness::pending_results::Builder>()
                    .set_cap(capnp_rpc::new_client(Echo));
            }
            p.init_cap_table(1).get(0).set_sender_hosted(99);
        });
    }
    async fn observe(&mut self) {
        drain().await;
        if let Some(pending) = self.pending.as_mut() {
            if let Some(result) = pending.now_or_never() {
                self.pending.take();
                match result {
                    Ok(r) => {
                        assert!(r.get().unwrap().has_cap());
                        self.response = Some(r);
                        self.outcome = 1;
                    }
                    Err(_) => self.outcome = 2,
                }
            }
        }
        drain().await;
        for (direct, peer, count, parent) in [
            (
                false,
                &mut self.relay,
                &mut self.old_finishes,
                self.question,
            ),
            (true, &mut self.callee, &mut self.direct_finishes, ID),
        ] {
            while let Some(m) = capnp_rpc::Connection::receive_incoming_message(peer).now_or_never()
            {
                let Some(m) = m.unwrap() else {
                    break;
                };
                match m
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                {
                    message::Finish(f) => {
                        if f.unwrap().get_question_id() == parent {
                            *count += 1;
                        }
                    }
                    message::Call(c) => {
                        let c = c.unwrap();
                        assert_eq!(c.get_method_id(), 0);
                        let target = match c.get_target().unwrap().which().unwrap() {
                            capnp_rpc::rpc_capnp::message_target::PromisedAnswer(p) => {
                                let p = p.unwrap();
                                let ops = p.get_transform().unwrap();
                                assert_eq!(ops.len(), 1);
                                assert!(matches!(
                                    ops.get(0).which().unwrap(),
                                    capnp_rpc::rpc_capnp::promised_answer::op::GetPointerField(0)
                                ));
                                p.get_question_id()
                            }
                            capnp_rpc::rpc_capnp::message_target::ImportedCap(id) => id,
                        };
                        self.calls.push((direct, target));
                        let id = c.get_question_id();
                        peer.send(|m| {
                            let mut r = m.init_return();
                            r.set_answer_id(id);
                            r.init_results()
                                .get_content()
                                .init_as::<harness::value::Builder>()
                                .set_value(73);
                        });
                    }
                    message::Release(_) => (),
                    message::Disembargo(barrier) => {
                        let barrier = barrier.unwrap();
                        let capnp_rpc::rpc_capnp::disembargo::context::SenderLoopback(id) =
                            barrier.get_context().which().unwrap()
                        else {
                            panic!("unexpected embargo context");
                        };
                        peer.send(|m| {
                            let mut reply = m.init_disembargo();
                            reply.set_target(barrier.get_target().unwrap()).unwrap();
                            reply.init_context().set_receiver_loopback(id);
                        });
                    }
                    _ => panic!("unexpected outgoing message"),
                }
            }
        }
    }
}
async fn echo_on(c: &mut Caller, cap: &harness::Client) {
    let promise = cap.echo_request().send().promise;
    c.observe().await;
    assert_eq!(promise.await.unwrap().get().unwrap().get_value(), 73);
    c.observe().await;
}

fn expire_setup(c: &Caller, peer: u8) {
    let status = c.hub.borrow_mut().connect(1, peer).status();
    let timer = status
        .borrow_mut()
        .answer_deadlines
        .pop()
        .expect("setup timer");
    timer.send(()).expect("setup still pending");
}

#[tokio::test(flavor = "current_thread")]
async fn missing_adoption_times_out_retires_token_and_rejects_late_answer() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::new().await;
            c.redirect();
            c.observe().await;
            expire_setup(&c, 2);
            c.observe().await;
            assert_eq!(c.outcome, 2);
            assert_eq!(c.old_finishes, 1);
            assert_eq!(c.hub.borrow().provision_count(), 0);
            c.adopt();
            c.observe().await;
            assert_eq!(c.direct_finishes, 1);
            assert_eq!(c.hub.borrow().waiter_count(), 0);
            assert_eq!(c.callee.status().borrow().aborts, 0);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn missing_redirect_reclaims_early_answer_without_aborting_peer() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for early_return in [false, true] {
                let mut c = Caller::new().await;
                c.adopt();
                if early_return {
                    c.complete(false);
                }
                c.observe().await;
                expire_setup(&c, 3);
                c.observe().await;
                assert_eq!(c.outcome, 0);
                assert_eq!(c.direct_finishes, 1);
                assert_eq!(c.hub.borrow().waiter_count(), 0);
                c.redirect();
                c.observe().await;
                expire_setup(&c, 2);
                c.observe().await;
                assert_eq!(c.outcome, 2);
                assert_eq!(c.direct_finishes, 1);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_adoption_cancels_setup_timers_before_method_completion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for notice_first in [false, true] {
                let mut c = Caller::new().await;
                if notice_first {
                    c.adopt();
                } else {
                    c.redirect();
                }
                c.observe().await;
                if notice_first {
                    c.redirect();
                } else {
                    c.adopt();
                }
                c.observe().await;
                assert_eq!(c.outcome, 0);
                for peer in [2, 3] {
                    let status = c.hub.borrow_mut().connect(1, peer).status();
                    assert!(status
                        .borrow()
                        .answer_deadlines
                        .iter()
                        .all(|timer| timer.is_canceled()));
                }
                c.complete(false);
                c.observe().await;
                assert_eq!(c.outcome, 1);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_caller_cancels_setup_timer_and_registration() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::new().await;
            c.redirect();
            c.observe().await;
            drop(c.pending.take());
            c.observe().await;
            let status = c.hub.borrow_mut().connect(1, 2).status();
            assert!(status
                .borrow()
                .answer_deadlines
                .iter()
                .all(|timer| timer.is_canceled()));
            assert_eq!(c.hub.borrow().provision_count(), 0);
            assert_eq!(c.old_finishes, 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn expired_unreturned_answers_cannot_exhaust_unbounded_question_ids() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let c = Caller::new().await;
            let mut peer = c.callee.clone();
            for offset in 0..4096 {
                peer.send(|m| {
                    let mut answer = m.init_third_party_answer();
                    answer.set_answer_id(ID + offset);
                    answer
                        .get_completion()
                        .set_as::<capnp::data::Owned>(&offset.to_le_bytes()[..])
                        .unwrap();
                });
                drain().await;
                expire_setup(&c, 3);
                drain().await;
                assert_eq!(c.hub.borrow().waiter_count(), 0);
            }
            peer.send(|m| {
                let mut answer = m.init_third_party_answer();
                answer.set_answer_id(ID + 4096);
                answer
                    .get_completion()
                    .set_as::<capnp::data::Owned>(&TOKEN[..])
                    .unwrap();
            });
            drain().await;
            let status = c.hub.borrow_mut().connect(1, 3).status();
            assert_eq!(status.borrow().aborts, 1);
            assert_eq!(c.hub.borrow().waiter_count(), 0);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_answer_setup_deadlines() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../verification/RpcAnswerSetup.cfg");
    let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
        + "\nPROPERTY SetupSettles\n";
    exploration::controls(
        "verification/RpcAnswerSetup.tla",
        "answer-setup",
        config,
        &[
            ("unauthenticated", "Authentication"),
            ("resurrect", "NoResurrection"),
            ("lateTimer", "NoResurrection"),
            ("leak", "Reclaimed"),
        ],
        Some(&live),
    )
    .unwrap();
    let traces =
        exploration::traces("verification/RpcAnswerSetup.tla", "answer-setup", config).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                let mut c = Caller::new().await;
                for state in &trace {
                    match state["event"] {
                        1 => c.redirect(),
                        2 => c.adopt(),
                        3 => c.complete(false),
                        4 => expire_setup(&c, 2),
                        5 => expire_setup(&c, 3),
                        6 => {
                            drop(c.pending.take());
                            drop(c.response.take());
                        }
                        _ => panic!("unknown action"),
                    }
                    c.observe().await;
                    assert_eq!(u64::from(c.outcome), state["outcome"], "{trace:?}");
                    assert_eq!(c.direct_finishes as u64, state["finished"], "{trace:?}");
                    let waiting =
                        state["noticed"] == 1 && state["linked"] == 0 && state["finished"] == 0;
                    assert_eq!(
                        c.hub.borrow().waiter_count(),
                        usize::from(waiting),
                        "{trace:?}"
                    );
                    if state["linked"] == 1 {
                        for peer in [2, 3] {
                            let status = c.hub.borrow_mut().connect(1, peer).status();
                            assert!(status
                                .borrow()
                                .answer_deadlines
                                .iter()
                                .all(|timer| timer.is_canceled()));
                        }
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn caller_pipelines_migrate_only_after_authenticated_adoption_before_return() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for first_adopt in [false, true] {
                for prebuilt in [false, true] {
                    let mut c = Caller::with_pipeline().await;
                    let unused = c.pipeline.as_ref().unwrap().get_cap();
                    let built = prebuilt.then(|| unused.echo_request());
                    if first_adopt {
                        c.adopt();
                    } else {
                        c.redirect();
                    }
                    c.observe().await;
                    let probe = c.pipeline.as_ref().unwrap().get_cap();
                    echo_on(&mut c, &probe).await;
                    assert_eq!(c.calls, [(false, c.question)]);
                    drop(probe);
                    if first_adopt {
                        c.redirect();
                    } else {
                        c.adopt();
                    }
                    c.observe().await;
                    assert_eq!(c.outcome, 0, "adoption must not resolve the response");
                    let promise = built
                        .unwrap_or_else(|| unused.echo_request())
                        .send()
                        .promise;
                    c.observe().await;
                    assert_eq!(promise.await.unwrap().get().unwrap().get_value(), 73);
                    assert_eq!(c.calls.last(), Some(&(true, ID)));
                    let fresh = c.pipeline.as_ref().unwrap().get_cap();
                    echo_on(&mut c, &fresh).await;
                    assert_eq!(c.calls.last(), Some(&(true, ID)));
                    assert_eq!(c.outcome, 0);
                    c.complete(false);
                    c.observe().await;
                    assert_eq!(c.outcome, 1);
                    echo_on(&mut c, &unused).await;
                    assert_eq!(c.calls.last(), Some(&(true, 99)));
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn previously_used_pipeline_references_keep_their_ordered_relay_route() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::with_pipeline().await;
            let used = c.pipeline.as_ref().unwrap().get_cap();
            echo_on(&mut c, &used).await;
            c.adopt();
            c.redirect();
            c.observe().await;
            echo_on(&mut c, &used).await;
            assert_eq!(c.calls, [(false, c.question), (false, c.question)]);
            let fresh = c.pipeline.as_ref().unwrap().get_cap();
            echo_on(&mut c, &fresh).await;
            assert_eq!(c.calls.last(), Some(&(true, ID)));
            c.complete(false);
            c.observe().await;
            echo_on(&mut c, &used).await;
            assert_eq!(c.calls.last(), Some(&(false, c.question)));
            c.pending.take();
            c.response.take();
            c.pipeline.take();
            drop(fresh);
            c.observe().await;
            assert_eq!(c.old_finishes, 0, "used reference lost its relay answer");
            drop(used);
            c.observe().await;
            assert_eq!(c.old_finishes, 1);
            assert_eq!(c.direct_finishes, 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn migrated_capability_owns_adopted_answer_after_original_request_drop() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for disconnect in [false, true] {
                let mut c = Caller::with_pipeline().await;
                let cap = c.pipeline.as_ref().unwrap().get_cap();
                c.redirect();
                c.adopt();
                c.observe().await;
                if disconnect {
                    c.hub.borrow_mut().disconnect_pair(1, 2);
                    c.observe().await;
                }
                c.pending.take();
                c.pipeline.take();
                c.observe().await;
                assert_eq!(c.old_finishes, usize::from(!disconnect));
                assert_eq!(c.direct_finishes, 0);
                echo_on(&mut c, &cap).await;
                assert_eq!(c.calls.last(), Some(&(true, ID)));
                drop(cap);
                c.observe().await;
                assert_eq!(c.direct_finishes, 1);
                c.complete(false);
                c.observe().await;
                assert_eq!(
                    c.direct_finishes, 1,
                    "late Return resurrected an adopted answer"
                );
                assert_eq!(c.hub.borrow().provision_count(), 0);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn early_return_and_callee_disconnect_do_not_resurrect_pipeline_authority() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for broken in [false, true] {
                let mut c = Caller::with_pipeline().await;
                let cap = c.pipeline.as_ref().unwrap().get_cap();
                c.adopt();
                c.complete(broken);
                c.observe().await;
                assert_eq!(c.outcome, 0);
                assert!(c.calls.is_empty());
                c.redirect();
                c.observe().await;
                assert_eq!(c.outcome, if broken { 2 } else { 1 });
                if broken {
                    assert!(cap.echo_request().send().promise.await.is_err());
                    assert!(c.calls.is_empty());
                } else {
                    echo_on(&mut c, &cap).await;
                    assert_eq!(c.calls.last(), Some(&(true, 99)));
                }
            }
            let mut c = Caller::with_pipeline().await;
            let cap = c.pipeline.as_ref().unwrap().get_cap();
            c.adopt();
            c.redirect();
            c.observe().await;
            c.hub.borrow_mut().disconnect_pair(1, 3);
            c.observe().await;
            assert_eq!(c.outcome, 2);
            assert!(cap.echo_request().send().promise.await.is_err());
            assert!(c.calls.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn caller_drop_before_authorization_respects_retained_pipeline_ownership() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for first_adopt in [false, true] {
                for keep_cap in [false, true] {
                    for first_notice in [false, true] {
                        let mut c = Caller::with_pipeline().await;
                        let mut cap = Some(c.pipeline.as_ref().unwrap().get_cap());
                        if first_notice {
                            if first_adopt {
                                c.adopt();
                            } else {
                                c.redirect();
                            }
                            c.observe().await;
                        }
                        c.pending.take();
                        c.pipeline.take();
                        if !keep_cap {
                            cap.take();
                        }
                        c.observe().await;
                        assert_eq!(c.old_finishes, usize::from(!keep_cap));
                        if !first_notice {
                            if first_adopt {
                                c.adopt();
                            } else {
                                c.redirect();
                            }
                            c.observe().await;
                        }
                        if first_adopt {
                            c.redirect();
                        } else {
                            c.adopt();
                        }
                        c.observe().await;
                        assert_eq!(c.outcome, 0);
                        assert_eq!(c.old_finishes, 1);
                        assert_eq!(c.direct_finishes, usize::from(!keep_cap));
                        if let Some(cap) = &cap {
                            echo_on(&mut c, cap).await;
                            assert_eq!(c.calls.last(), Some(&(true, ID)));
                        }
                        cap.take();
                        c.observe().await;
                        assert_eq!(c.direct_finishes, 1);
                        assert_eq!(c.hub.borrow().provision_count(), 0);
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn exported_pipeline_reference_stays_on_its_ordered_route() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::with_pipeline().await;
            let exported = c.pipeline.as_ref().unwrap().get_cap();
            let carrier = c.pipeline.as_ref().unwrap().get_cap();
            let mut request = carrier.echo_request();
            // Deliberately use pointer-bearing params on our mock method to export
            // the saved reference without ever sending a call on that reference.
            request
                .hook
                .get()
                .init_as::<harness::bounce_params::Builder>()
                .set_cap(exported.clone());
            let promise = request.send().promise;
            c.observe().await;
            promise.await.unwrap();
            c.observe().await;
            drop(carrier);
            c.calls.clear();
            c.adopt();
            c.redirect();
            c.observe().await;
            echo_on(&mut c, &exported).await;
            assert_eq!(c.calls, [(false, c.question)]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn adopted_pipeline_with_missing_result_capability_rejects_without_panicking() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::with_pipeline().await;
            let cap = c.pipeline.as_ref().unwrap().get_cap();
            c.pending.take();
            c.adopt();
            c.callee.send(|m| {
                let mut r = m.init_return();
                r.set_answer_id(ID);
                r.init_results()
                    .get_content()
                    .init_as::<harness::pending_results::Builder>();
            });
            c.observe().await;
            c.redirect();
            c.observe().await;
            assert!(cap.echo_request().send().promise.await.is_err());
            let fresh = c.pipeline.as_ref().unwrap().get_cap();
            assert!(fresh.echo_request().send().promise.await.is_err());
            assert!(c.calls.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn self_adopted_public_pipeline_calls_before_return_and_after_relay_disconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(3), async {
                let (gate, rx) = oneshot::channel();
                let entered = Rc::new(Cell::new(0));
                let service: harness::Client =
                    capnp_rpc::new_client(EarlyPipeline(RefCell::new(Some(rx)), entered.clone()));
                let mut c = Caller::with_service(true, true, service).await;
                let cap = c.pipeline.as_ref().unwrap().get_cap();
                c.observe().await;
                c.relay.send(|m| m.init_bootstrap().set_question_id(41));
                let m = recv(&mut c.relay).await;
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
                let return_::Results(p) = r.unwrap().which().unwrap() else {
                    panic!()
                };
                let cap_descriptor::SenderHosted(export) =
                    p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
                else {
                    panic!()
                };
                let mut contact = vec![1, 1];
                contact.extend(TOKEN);
                c.relay.send(|m| {
                    let mut call = m.init_call();
                    call.set_question_id(42);
                    call.set_interface_id(harness::Client::TYPE_ID);
                    call.set_method_id(3);
                    call.reborrow().init_target().set_imported_cap(export);
                    call.init_send_results_to()
                        .init_third_party()
                        .set_as::<capnp::data::Owned>(&contact[..])
                        .unwrap();
                });
                let mut token = vec![1];
                token.extend(TOKEN);
                let q = c.question;
                c.relay.send(|m| {
                    let mut r = m.init_return();
                    r.set_answer_id(q);
                    r.init_await_from_third_party()
                        .set_as::<capnp::data::Owned>(&token[..])
                        .unwrap();
                });
                cap.client.hook.when_more_resolved().unwrap().await.unwrap();
                assert_eq!(entered.get(), 1);
                assert_eq!(
                    cap.echo_request()
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    73
                );
                assert!(c.pending.as_mut().unwrap().now_or_never().is_none());
                c.hub.borrow_mut().disconnect_pair(1, 2);
                drain().await;
                assert_eq!(
                    cap.echo_request()
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    73
                );
                gate.send(()).unwrap();
                c.pending.take().unwrap().await.unwrap();
                assert_eq!(c.hub.borrow().local_accept_count, 1);
                assert_eq!(c.callee.status().borrow().adoptions_sent, 0);
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn answer_adoption_waits_for_authorization_and_retains_result_until_drop() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for order in [[0, 1, 2], [1, 0, 2], [1, 2, 0]] {
                for broken in [false, true] {
                    let mut c = Caller::new().await;
                    for (i, action) in order.into_iter().enumerate() {
                        match action {
                            0 => c.redirect(),
                            1 => c.adopt(),
                            _ => c.complete(broken),
                        };
                        c.observe().await;
                        if i < 2 {
                            assert_eq!(c.outcome, 0);
                        }
                    }
                    assert_eq!(c.outcome, if broken { 2 } else { 1 });
                    if !broken {
                        assert_eq!(c.direct_finishes, 0);
                    }
                    c.response.take();
                    c.observe().await;
                    assert_eq!(c.direct_finishes, 1);
                    assert_eq!(c.old_finishes, 1);
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn portable_answer_handoff_authenticates_reordered_results_and_reclaims_cancellation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                for order in [[0, 1, 2], [1, 0, 2], [1, 2, 0]] {
                    for broken in [false, true] {
                        let mut c =
                            Caller::with_driver(true, true, false, capnp_rpc::new_client(Echo))
                                .await;
                        for (index, action) in order.into_iter().enumerate() {
                            match action {
                                0 => c.redirect(),
                                1 => c.adopt(),
                                _ => c.complete(broken),
                            }
                            c.observe().await;
                            if index < 2 {
                                assert_eq!(c.outcome, 0);
                            }
                        }
                        assert_eq!(c.outcome, if broken { 2 } else { 1 });
                        if !broken {
                            assert_eq!(c.direct_finishes, 0);
                        }
                        c.response.take();
                        c.observe().await;
                        assert_eq!(c.direct_finishes, 1);
                        assert_eq!(c.old_finishes, 1);
                        assert_eq!(c.hub.borrow().waiter_count(), 0);
                        assert_eq!(c.hub.borrow().provision_count(), 0);
                    }
                }
                for cancel in [false, true] {
                    let mut c =
                        Caller::with_driver(true, true, false, capnp_rpc::new_client(Echo)).await;
                    c.redirect();
                    c.observe().await;
                    if cancel {
                        c.pending.take();
                    } else {
                        expire_setup(&c, 2);
                    }
                    c.observe().await;
                    assert_eq!(c.hub.borrow().provision_count(), 0);
                    assert_eq!(c.old_finishes, 1);
                    c.adopt();
                    c.observe().await;
                    assert_eq!(c.direct_finishes, 1);
                    assert_eq!(c.hub.borrow().waiter_count(), 0);
                    assert_eq!(c.callee.status().borrow().aborts, 0);
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn adoption_exposes_unused_pipeline_before_return_and_preserves_used_route() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                for portable in [false, true] {
                    for previously_used in [false, true] {
                        let mut c =
                            Caller::with_driver(portable, true, true, capnp_rpc::new_client(Echo))
                                .await;
                        if previously_used {
                            let call = c
                                .pipeline
                                .as_ref()
                                .unwrap()
                                .get_cap()
                                .echo_request()
                                .send()
                                .promise;
                            c.observe().await;
                            assert_eq!(c.calls.len(), 1, "old pipeline call was not published");
                            assert_eq!(call.await.unwrap().get().unwrap().get_value(), 73);
                            assert_eq!(c.calls.last(), Some(&(false, c.question)));
                        }
                        c.adopt();
                        c.redirect();
                        c.observe().await;
                        let call = c
                            .pipeline
                            .as_ref()
                            .unwrap()
                            .get_cap()
                            .echo_request()
                            .send()
                            .promise;
                        c.observe().await;
                        assert_eq!(
                            c.calls.len(),
                            if previously_used { 2 } else { 1 },
                            "post-adoption pipeline call was not published; used={previously_used}"
                        );
                        assert_eq!(call.await.unwrap().get().unwrap().get_value(), 73);
                        assert_eq!(
                            c.calls.last(),
                            Some(&(
                                !previously_used,
                                if previously_used { c.question } else { ID }
                            ))
                        );
                        assert_eq!(c.outcome, 0);
                        for peer in [2, 3] {
                            let status = c.hub.borrow_mut().connect(1, peer).status();
                            assert!(status
                                .borrow()
                                .answer_deadlines
                                .iter()
                                .all(|timer| timer.is_canceled()));
                        }
                        c.complete(false);
                        c.observe().await;
                        assert_eq!(c.outcome, 1);
                        c.pipeline.take();
                        c.response.take();
                        c.observe().await;
                        assert_eq!(c.direct_finishes, 1);
                        assert_eq!(c.old_finishes, 1);
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_and_disconnected_adoptions_settle_without_losing_authority_checks() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for order in [[0, 1, 2], [1, 0, 2], [1, 2, 0]] {
                for cancel_at in 0..3 {
                    let mut c = Caller::new().await;
                    for (i, action) in order.into_iter().enumerate() {
                        if i == cancel_at {
                            c.pending.take();
                            c.response.take();
                        }
                        match action {
                            0 => c.redirect(),
                            1 => c.adopt(),
                            _ => c.complete(false),
                        };
                        c.observe().await;
                    }
                    assert_eq!(c.outcome, 0);
                    assert_eq!(c.direct_finishes, 1);
                    assert_eq!(c.old_finishes, 1);
                }
            }
            // A callee may disappear before the introducer's redirect arrives.
            let mut c = Caller::new().await;
            c.adopt();
            c.observe().await;
            c.hub.borrow_mut().disconnect_pair(3, 1);
            c.observe().await;
            assert_eq!(c.outcome, 0);
            c.redirect();
            c.observe().await;
            assert_eq!(c.outcome, 2);
            // Once authorized, the direct result does not depend on the relay link.
            let mut c = Caller::new().await;
            c.redirect();
            c.observe().await;
            c.hub.borrow_mut().disconnect_pair(2, 1);
            c.observe().await;
            c.adopt();
            c.complete(false);
            c.observe().await;
            assert_eq!(c.outcome, 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn third_party_tail_returns_over_authenticated_native() {
    use capntproto::{
        native_rpc::{Handle, Network},
        transport::{self, Identity},
    };
    async fn pair(a: &Identity, b: &Identity, ah: &Handle, bh: &Handle) {
        let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = right.local_addr().unwrap();
        let (a, b) = tokio::join!(
            transport::connect_authenticated(
                left,
                addr,
                a,
                b.public_key(),
                Some([17; 32]),
                b"answer-adoption"
            ),
            transport::accept_authenticated(
                right,
                b,
                a.public_key(),
                Some([17; 32]),
                b"answer-adoption"
            )
        );
        ah.attach(a.unwrap()).unwrap();
        bh.attach(b.unwrap()).unwrap();
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in driver_cases() {
                let stage = Cell::new("connect");
                tokio::time::timeout(Duration::from_secs(10), async {
                    let ids = [
                        Identity::generate(),
                        Identity::generate(),
                        Identity::generate(),
                    ];
                    let (an, ah) = Network::new(ids[0].public_key());
                    let (bn, bh) = Network::new(ids[1].public_key());
                    let (cn, ch) = Network::new(ids[2].public_key());
                    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
                        pair(&ids[a], &ids[b], [&ah, &bh, &ch][a], [&ah, &bh, &ch][b]).await;
                    }
                    let (gate, rx) = oneshot::channel();
                    let count = Rc::new(Cell::new(0));
                    let host: harness::Client =
                        capnp_rpc::new_client(EarlyPipeline(RefCell::new(Some(rx)), count.clone()));
                    let c =
                        support::driver::System::new(portable[2], Box::new(cn), Some(host.client));
                    let factory = Rc::new(Factory(RefCell::new(None)));
                    let mut b = support::driver::System::with_factory(
                        portable[1],
                        Box::new(bn),
                        ids[1].public_key(),
                        factory.clone(),
                    );
                    let host: harness::Client = b.bootstrap(ids[2].public_key());
                    let ready = host.client.hook.when_resolved();
                    let relay: harness::Client = capnp_rpc::new_client(Tail(host));
                    *factory.0.borrow_mut() = Some(relay.client);
                    let mut a = support::driver::System::new(portable[0], Box::new(an), None);
                    let relay: harness::Client = a.bootstrap(ids[1].public_key());
                    let tasks = [a, b, c].map(|system| tokio::task::spawn_local(system.run()));
                    stage.set("resolve host");
                    ready.await.unwrap();
                    stage.set("dispatch parent");
                    let mut request = relay.pending_request().send();
                    let early = request.pipeline.get_cap();
                    while count.get() == 0 {
                        tokio::task::yield_now().await;
                    }
                    stage.set("resolve early pipeline");
                    early
                        .client
                        .hook
                        .when_more_resolved()
                        .unwrap()
                        .await
                        .unwrap();
                    assert!((&mut request.promise).now_or_never().is_none());
                    stage.set("early child reply");
                    assert_eq!(
                        early
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
                    // Previously used portable pipelines deliberately retain their
                    // ordered relay route until a distributed Join fence is added.
                    if portable == [false; 3] {
                        bh.disconnect(ids[0].public_key());
                        bh.disconnect(ids[2].public_key());
                    }
                    assert_eq!(
                        early
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
                    assert!((&mut request.promise).now_or_never().is_none());
                    stage.set("parent reply");
                    gate.send(()).unwrap();
                    let cap = request
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_cap()
                        .unwrap();
                    assert!(ah.stats().published > 0, "no answer rendezvous registered");
                    if portable != [false; 3] {
                        bh.disconnect(ids[0].public_key());
                        bh.disconnect(ids[2].public_key());
                    }
                    stage.set("direct child reply");
                    assert_eq!(
                        cap.echo_request()
                            .send()
                            .promise
                            .await
                            .unwrap()
                            .get()
                            .unwrap()
                            .get_value(),
                        73
                    );
                    for task in tasks {
                        task.abort();
                    }
                })
                .await
                .unwrap_or_else(|_| panic!("portable={portable:?}: stalled at {}", stage.get()));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn self_introduced_answer_adopts_locally_without_allocating_a_wire_question() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (portable, early) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut c =
                    Caller::with_driver(portable, true, false, capnp_rpc::new_client(Echo)).await;
                c.observe().await;
                c.relay.send(|m| m.init_bootstrap().set_question_id(41));
                let m = recv(&mut c.relay).await;
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
                let return_::Results(p) = r.unwrap().which().unwrap() else {
                    panic!()
                };
                let cap_descriptor::SenderHosted(export) =
                    p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
                else {
                    panic!()
                };
                let mut await_token = vec![1];
                await_token.extend(TOKEN);
                let q = c.question;
                if early {
                    c.relay.send(|m| {
                        let mut r = m.init_return();
                        r.set_answer_id(q);
                        r.init_await_from_third_party()
                            .set_as::<capnp::data::Owned>(&await_token[..])
                            .unwrap();
                    });
                }
                let mut contact = vec![1, 1];
                contact.extend(TOKEN);
                c.relay.send(|m| {
                    let mut r = m.init_call();
                    r.set_question_id(42);
                    r.set_interface_id(harness::Client::TYPE_ID);
                    r.set_method_id(3);
                    r.reborrow().init_target().set_imported_cap(export);
                    r.get_send_results_to()
                        .init_third_party()
                        .set_as::<capnp::data::Owned>(&contact[..])
                        .unwrap();
                });
                let m = recv(&mut c.relay).await;
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
                let r = r.unwrap();
                assert_eq!(r.get_answer_id(), 42);
                assert!(matches!(
                    r.which().unwrap(),
                    return_::ResultsSentElsewhere(())
                ));
                if !early {
                    c.relay.send(|m| {
                        let mut r = m.init_return();
                        r.set_answer_id(q);
                        r.init_await_from_third_party()
                            .set_as::<capnp::data::Owned>(&await_token[..])
                            .unwrap();
                    });
                }
                c.observe().await;
                assert_eq!(c.outcome, 1);
                let cap = c.response.take().unwrap().get().unwrap().get_cap().unwrap();
                assert_eq!(
                    cap.echo_request()
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    73
                );
                assert_eq!(c.hub.borrow().local_accept_count, 1);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_answer_adoption_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ANSWER_ADOPTION_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, case) in cases.iter().enumerate() {
                let mut c = Caller::new().await;
                for step in case["steps"].as_array().unwrap() {
                    match step["action"].as_str().unwrap() {
                        "redirect" => c.redirect(),
                        "adopt" => c.adopt(),
                        "return" => c.complete(case["broken"].as_bool().unwrap()),
                        "drop" => {
                            c.pending.take();
                            c.response.take();
                        }
                        "disconnect" => c.hub.borrow_mut().disconnect_pair(3, 1),
                        _ => panic!(),
                    }
                    c.observe().await;
                    let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
                    assert_eq!(c.outcome, state[5], "trace {index}: {step}");
                    assert_eq!(
                        c.old_finishes as u32, state[6],
                        "old Finish, trace {index}: {step}"
                    );
                    assert_eq!(
                        c.direct_finishes as u32, state[7],
                        "direct Finish, trace {index}: {step}"
                    );
                    assert_eq!(
                        u32::from(c.response.is_some()),
                        state[8],
                        "capability lifetime, trace {index}: {step}"
                    );
                }
            }
        })
        .await;
}

#[derive(Default)]
struct Lifetime {
    entered: Cell<u32>,
    ended: Cell<u32>,
    completed: Cell<u32>,
}
struct Scope(Rc<Lifetime>);
impl Drop for Scope {
    fn drop(&mut self) {
        self.0.ended.set(self.0.ended.get() + 1);
    }
}
struct OwnedGate {
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    life: Rc<Lifetime>,
}
impl harness::Server for OwnedGate {
    fn pending(
        self: Rc<Self>,
        context: harness::PendingContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            self.life.entered.set(self.life.entered.get() + 1);
            let _scope = Scope(self.life.clone());
            let gate = self.gate.borrow_mut().take().unwrap();
            gate.await
                .map_err(|_| capnp::Error::failed("gate gone".into()))?;
            self.life.completed.set(self.life.completed.get() + 1);
            results.get().set_cap(capnp_rpc::new_client(Echo));
            Ok(())
        })
    }
}
struct Callee {
    hub: Rc<RefCell<Hub>>,
    relay: Endpoint,
    caller: Endpoint,
    gate: Option<oneshot::Sender<()>>,
    life: Rc<Lifetime>,
    returned: u32,
    ack: u32,
    child_returned: u32,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    _networks: Vec<support::Network>,
}
impl Drop for Callee {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn finish(peer: &mut Endpoint, id: u32) {
    peer.send(|m| {
        let mut f = m.init_finish();
        f.set_question_id(id);
        f.set_require_early_cancellation_workaround(false);
    });
}
impl Callee {
    async fn new() -> Self {
        Self::with_driver(false).await
    }
    async fn with_driver(portable: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        enable(&hub);
        let network = Hub::network(&hub, 3);
        let networks = vec![Hub::network(&hub, 1), Hub::network(&hub, 2)];
        let life = Rc::new(Lifetime::default());
        let (gate, rx) = oneshot::channel();
        let server: harness::Client = capnp_rpc::new_client(OwnedGate {
            gate: RefCell::new(Some(rx)),
            life: life.clone(),
        });
        let task = tokio::task::spawn_local(
            support::driver::System::new(portable, Box::new(network), Some(server.client)).run(),
        );
        let mut relay = hub.borrow_mut().connect(2, 3);
        relay.send(|m| m.init_bootstrap().set_question_id(0));
        let m = recv(&mut relay).await;
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
        let return_::Results(p) = r.unwrap().which().unwrap() else {
            panic!()
        };
        let cap_descriptor::SenderHosted(export) =
            p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
        else {
            panic!()
        };
        relay.send(|m| {
            let mut f = m.init_finish();
            f.set_question_id(0);
            f.set_release_result_caps(false);
        });
        let mut contact = vec![1, 3];
        contact.extend(TOKEN);
        relay.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(1);
            c.set_interface_id(harness::Client::TYPE_ID);
            c.set_method_id(3);
            c.reborrow().init_target().set_imported_cap(export);
            c.get_send_results_to()
                .init_third_party()
                .set_as::<capnp::data::Owned>(&contact[..])
                .unwrap();
        });
        drain().await;
        assert_eq!(life.entered.get(), 1);
        let mut caller = hub.borrow_mut().connect(1, 3);
        let m = recv(&mut caller).await;
        let message::ThirdPartyAnswer(a) = m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(a.unwrap().get_answer_id(), ID);
        Self {
            hub,
            relay,
            caller,
            gate: Some(gate),
            life,
            returned: 0,
            ack: 0,
            child_returned: 0,
            task,
            _networks: networks,
        }
    }
    async fn observe(&mut self) {
        drain().await;
        for (peer, direct) in [(&mut self.relay, false), (&mut self.caller, true)] {
            while let Some(m) = capnp_rpc::Connection::receive_incoming_message(peer).now_or_never()
            {
                let Some(m) = m.unwrap() else {
                    break;
                };
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
                        if direct && r.get_answer_id() == 7 {
                            assert_eq!(self.child_returned, 0);
                            self.child_returned = match r.which().unwrap() {
                                return_::Results(p) => {
                                    assert_eq!(
                                        p.unwrap()
                                            .get_content()
                                            .get_as::<harness::value::Reader>()
                                            .unwrap()
                                            .get_value(),
                                        73
                                    );
                                    1
                                }
                                return_::Canceled(()) => 2,
                                _ => panic!(),
                            };
                            continue;
                        }
                        if direct {
                            assert_eq!(r.get_answer_id(), ID);
                            assert_eq!(self.returned, 0);
                            assert!(!r.get_no_finish_needed());
                            self.returned = match r.which().unwrap() {
                                return_::Results(p) => {
                                    assert!(p
                                        .unwrap()
                                        .get_content()
                                        .get_as::<harness::pending_results::Reader>()
                                        .unwrap()
                                        .has_cap());
                                    1
                                }
                                return_::Canceled(()) => 2,
                                _ => panic!(),
                            };
                        } else {
                            assert_eq!(r.get_answer_id(), 1);
                            assert_eq!(self.ack, 0);
                            assert!(matches!(
                                r.which().unwrap(),
                                return_::ResultsSentElsewhere(()) | return_::Canceled(())
                            ));
                            self.ack = 1;
                        }
                    }
                    message::Release(_) => (),
                    _ => panic!("unexpected callee message"),
                }
            }
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn adopted_call_owns_execution_until_both_answers_finish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (portable, first) in [(false, false), (false, true), (true, false), (true, true)] {
                for complete in [false, true] {
                    let mut c = Callee::with_driver(portable).await;
                    if first {
                        finish(&mut c.caller, ID);
                    } else {
                        finish(&mut c.relay, 1);
                    }
                    c.observe().await;
                    assert_eq!(c.life.ended.get(), 0);
                    if complete {
                        c.gate.take().unwrap().send(()).unwrap();
                        c.observe().await;
                        assert_eq!(c.life.completed.get(), 1);
                    }
                    if first {
                        finish(&mut c.relay, 1);
                    } else {
                        finish(&mut c.caller, ID);
                    }
                    c.observe().await;
                    assert_eq!(c.life.ended.get(), 1);
                    assert_eq!(c.ack, 1);
                    assert_eq!(c.returned, if complete && !first { 1 } else { 2 });
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_adopted_call_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ADOPTED_CALL_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, case) in cases.iter().enumerate() {
                let mut c = Callee::new().await;
                for step in case["steps"].as_array().unwrap() {
                    match step["action"].as_str().unwrap() {
                        "finishOld" => finish(&mut c.relay, 1),
                        "finishDirect" => finish(&mut c.caller, ID),
                        "finishChild" => finish(&mut c.caller, 7),
                        "pipeline" => c.caller.send(|m| {
                            let mut request = m.init_call();
                            request.set_question_id(7);
                            request.set_interface_id(harness::Client::TYPE_ID);
                            request.set_method_id(0);
                            let mut target = request.init_target().init_promised_answer();
                            target.set_question_id(ID);
                            target.init_transform(1).get(0).set_get_pointer_field(0);
                        }),
                        "ready" => {
                            let _ = c.gate.take().unwrap().send(());
                        }
                        "disconnectOld" => c.hub.borrow_mut().disconnect_pair(2, 3),
                        "disconnectDirect" => c.hub.borrow_mut().disconnect_pair(1, 3),
                        _ => panic!(),
                    }
                    c.observe().await;
                    let s: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
                    assert_eq!(
                        c.life.ended.get(),
                        u32::from(s[5] != 0),
                        "context lifetime, trace {index}: {step}"
                    );
                    assert_eq!(
                        c.life.completed.get(),
                        u32::from(s[5] == 1),
                        "execution, trace {index}: {step}"
                    );
                    assert_eq!(c.ack, s[6], "ack, trace {index}: {step}");
                    assert_eq!(c.returned, s[7], "Return, trace {index}: {step}");
                    assert_eq!(
                        c.child_returned, s[10],
                        "child Return, trace {index}: {step}"
                    );
                }
            }
        })
        .await;
}

async fn expect_abort(peer: &mut Endpoint) {
    loop {
        let m = recv(peer).await;
        match m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        {
            message::Abort(_) => return,
            message::Finish(_) | message::Release(_) => (),
            _ => panic!("expected Abort"),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn system_drop_releases_unmatched_answer_waiters() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for authorized in [false, true] {
                let mut c = Caller::new().await;
                if authorized {
                    c.redirect();
                } else {
                    c.adopt();
                }
                c.observe().await;
                assert_eq!(c.hub.borrow().waiter_count(), usize::from(!authorized));
                assert_eq!(c.hub.borrow().provision_count(), usize::from(authorized));
                let hub = c.hub.clone();
                drop(c);
                drain().await;
                assert_eq!(hub.borrow().waiter_count(), 0);
                assert_eq!(hub.borrow().provision_count(), 0);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn answer_cannot_claim_a_capability_provision() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::new().await;
            c.observe().await;
            c.relay.send(|m| m.init_bootstrap().set_question_id(41));
            let m = recv(&mut c.relay).await;
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
            let return_::Results(p) = r.unwrap().which().unwrap() else {
                panic!()
            };
            let cap_descriptor::SenderHosted(export) =
                p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
            else {
                panic!()
            };
            let mut recipient = vec![3];
            recipient.extend(TOKEN);
            c.relay.send(|m| {
                let mut p = m.init_provide();
                p.set_question_id(42);
                p.reborrow().init_target().set_imported_cap(export);
                p.get_recipient()
                    .set_as::<capnp::data::Owned>(&recipient[..])
                    .unwrap();
            });
            drain().await;
            assert_eq!(c.hub.borrow().provision_count(), 1);
            c.adopt();
            expect_abort(&mut c.callee).await;
            assert_eq!(c.hub.borrow().provision_count(), 1);
            finish(&mut c.relay, 42);
            drain().await;
            assert_eq!(c.hub.borrow().provision_count(), 0);
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn answer_namespaces_duplicate_claims_and_missing_permission_are_rejected() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for bad in [0, ID - 1, 1 << 31, u32::MAX] {
                let mut c = Caller::new().await;
                c.callee.send(|m| {
                    let mut a = m.init_third_party_answer();
                    a.set_answer_id(bad);
                    a.get_completion()
                        .set_as::<capnp::data::Owned>(&TOKEN[..])
                        .unwrap();
                });
                expect_abort(&mut c.callee).await;
            }
            for kind in 0..5 {
                let mut c = Caller::new().await;
                c.hub
                    .borrow_mut()
                    .connect(1, 3)
                    .status()
                    .borrow_mut()
                    .two_party_join = true;
                c.callee.send(|m| match kind {
                    0 => m.init_bootstrap().set_question_id(ID),
                    1 => m.init_call().set_question_id(ID),
                    2 => m.init_provide().set_question_id(ID),
                    3 => m.init_accept().set_question_id(ID),
                    _ => m.init_join().set_question_id(ID),
                });
                expect_abort(&mut c.callee).await;
            }
            {
                let mut c = Caller::new().await;
                c.redirect();
                c.adopt();
                c.observe().await;
                c.callee.send(|m| {
                    let mut r = m.init_return();
                    r.set_answer_id(ID);
                    r.set_no_finish_needed(true);
                    r.init_exception().set_reason("invalid Finish suppression");
                });
                expect_abort(&mut c.callee).await;
                c.observe().await;
                assert_eq!(c.outcome, 2);
            }
            for same_id in [false, true] {
                let mut c = Caller::new().await;
                c.redirect();
                c.adopt();
                c.observe().await;
                c.callee.send(|m| {
                    let mut a = m.init_third_party_answer();
                    a.set_answer_id(if same_id { ID } else { ID + 1 });
                    a.get_completion()
                        .set_as::<capnp::data::Owned>(&TOKEN[..])
                        .unwrap();
                });
                expect_abort(&mut c.callee).await;
                c.observe().await;
                assert_eq!(c.outcome, 2);
            }
            for canceled in [false, true] {
                let mut c = Caller::with_permission(false).await;
                c.observe().await;
                if canceled {
                    c.pending.take();
                    c.observe().await;
                }
                c.redirect();
                expect_abort(&mut c.relay).await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn answer_tokens_are_bound_to_peer_and_cannot_be_consumed_by_accept() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Caller::new().await;
            let _network = Hub::network(&c.hub, 4);
            let mut imposter = c.hub.borrow_mut().connect(4, 1);
            c.redirect();
            imposter.send(|m| {
                let mut a = m.init_third_party_answer();
                a.set_answer_id(ID);
                a.get_completion()
                    .set_as::<capnp::data::Owned>(&TOKEN[..])
                    .unwrap();
            });
            imposter.send(|m| {
                let mut r = m.init_return();
                r.set_answer_id(ID);
                r.init_exception().set_reason("forged failure");
            });
            c.observe().await;
            assert_eq!(c.outcome, 0);
            c.callee.send(|m| {
                let mut a = m.init_accept();
                a.set_question_id(13);
                a.get_provision()
                    .set_as::<capnp::data::Owned>(&TOKEN[..])
                    .unwrap();
            });
            let m = recv(&mut c.callee).await;
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
            let r = r.unwrap();
            assert_eq!(r.get_answer_id(), 13);
            assert!(matches!(r.which().unwrap(), return_::Exception(_)));
            finish(&mut c.callee, 13);
            c.observe().await;
            assert_eq!(c.outcome, 0);
            c.adopt();
            c.complete(false);
            c.observe().await;
            assert_eq!(c.outcome, 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn direct_pipeline_remains_live_after_both_parent_answers_are_finished() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in [false, true] {
                let mut c = Callee::with_driver(portable).await;
                c.caller.send(|m| {
                    let mut request = m.init_call();
                    request.set_question_id(7);
                    request.set_interface_id(harness::Client::TYPE_ID);
                    request.set_method_id(0);
                    let mut target = request.init_target().init_promised_answer();
                    target.set_question_id(ID);
                    target.init_transform(1).get(0).set_get_pointer_field(0);
                });
                drain().await;
                finish(&mut c.relay, 1);
                finish(&mut c.caller, ID);
                drain().await;
                assert_eq!(c.life.ended.get(), 0, "child pipeline lost its producer");
                c.gate.take().unwrap().send(()).unwrap();
                let mut child = false;
                let mut parent = false;
                while !child || !parent {
                    let m = recv(&mut c.caller).await;
                    let message::Return(r) = m
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        continue;
                    };
                    let r = r.unwrap();
                    if r.get_answer_id() == 7 {
                        let return_::Results(p) = r.which().unwrap() else {
                            panic!()
                        };
                        assert_eq!(
                            p.unwrap()
                                .get_content()
                                .get_as::<harness::value::Reader>()
                                .unwrap()
                                .get_value(),
                            73
                        );
                        child = true;
                    } else {
                        assert_eq!(r.get_answer_id(), ID);
                        assert!(matches!(r.which().unwrap(), return_::Canceled(())));
                        parent = true;
                    }
                }
                assert_eq!(c.life.completed.get(), 1);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_caller_pipeline_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_CALLER_PIPELINE_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, case) in cases.iter().enumerate() {
                let used = case["used"].as_bool().unwrap();
                let broken = case["broken"].as_bool().unwrap();
                let mut c = Caller::with_pipeline().await;
                let mut cap = Some(c.pipeline.as_ref().unwrap().get_cap());
                if used {
                    echo_on(&mut c, cap.as_ref().unwrap()).await;
                    c.calls.clear();
                }
                let (mut r, mut a, mut v, mut d, mut k) = (false, false, false, false, false);
                let (mut o, mut connected) = (true, true);
                let (mut called, mut ready, mut final_call) = (false, false, false);
                let mut route = 0;
                for step in case["steps"].as_array().unwrap() {
                    match step["action"].as_str().unwrap() {
                        "redirect" => {
                            r = true;
                            c.redirect();
                        }
                        "adopt" => {
                            a = true;
                            c.adopt();
                        }
                        "return" => {
                            v = true;
                            c.complete(broken);
                        }
                        "dropRoot" => {
                            d = true;
                            c.pending.take();
                            c.response.take();
                            c.pipeline.take();
                        }
                        "dropCap" => {
                            k = true;
                            cap.take();
                        }
                        "disconnectOld" => {
                            o = false;
                            c.hub.borrow_mut().disconnect_pair(1, 2);
                        }
                        "disconnectDirect" => {
                            connected = false;
                            c.hub.borrow_mut().disconnect_pair(1, 3);
                        }
                        "call" => {
                            called = true;
                            ready = r && a && !used && connected && !(v && broken);
                            final_call = v;
                            let promise = cap.as_ref().unwrap().echo_request().send().promise;
                            c.observe().await;
                            let result = tokio::time::timeout(Duration::from_secs(2), promise)
                                .await
                                .unwrap();
                            route = match result {
                                Ok(response) => {
                                    assert_eq!(response.get().unwrap().get_value(), 73);
                                    match c.calls.as_slice() {
                                        [(false, q)] if *q == c.question => 1,
                                        [(true, q)] if *q == ID => 2,
                                        [(true, 99)] => 3,
                                        calls => panic!(
                                            "trace {index}: unexpected call targets {calls:?}"
                                        ),
                                    }
                                }
                                Err(_) => {
                                    assert!(c.calls.is_empty());
                                    4
                                }
                            };
                        }
                        other => panic!("unknown action {other}"),
                    }
                    c.observe().await;
                    let actual = vec![
                        r as u32,
                        a as u32,
                        v as u32,
                        d as u32,
                        k as u32,
                        o as u32,
                        connected as u32,
                        c.outcome,
                        c.old_finishes as u32,
                        c.direct_finishes as u32,
                        called as u32,
                        route,
                        ready as u32,
                        final_call as u32,
                    ];
                    let expected: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
                    assert_eq!(
                        actual, expected,
                        "trace {index}, used={used}, broken={broken}: {step}"
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn portable_callee_transfer_survives_relay_disconnect_before_completion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut c = Callee::with_driver(true).await;
            c.hub.borrow_mut().disconnect_pair(2, 3);
            drain().await;
            assert_eq!(
                c.life.ended.get(),
                0,
                "relay disconnect canceled recipient work"
            );
            c.gate.take().unwrap().send(()).unwrap();
            let message = recv(&mut c.caller).await;
            let message::Return(returned) = message
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("direct Return")
            };
            let returned = returned.unwrap();
            assert_eq!(returned.get_answer_id(), ID);
            assert!(matches!(returned.which().unwrap(), return_::Results(_)));
            assert_eq!(c.life.completed.get(), 1);
            assert_eq!(c.life.ended.get(), 1);
            finish(&mut c.caller, ID);
            drain().await;
            assert!(!c.task.is_finished(), "relay failure stopped other peers");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn portable_tail_pipeline_can_be_projected_after_results_sent_elsewhere() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                enable(&hub);
                hub.borrow_mut().introductions = true;
                let _caller_network = Hub::network(&hub, 1);
                let relay_network = Hub::network(&hub, 2);
                let callee_network = Hub::network(&hub, 3);
                let service: harness::Client = capnp_rpc::new_client(Echo);
                let callee = tokio::task::spawn_local(
                    support::driver::System::new(
                        true,
                        Box::new(callee_network),
                        Some(service.client),
                    )
                    .run(),
                );
                let factory = Rc::new(Factory(RefCell::new(None)));
                let mut relay = support::driver::System::with_factory(
                    true,
                    Box::new(relay_network),
                    2,
                    factory.clone(),
                );
                let target: harness::Client = relay.bootstrap(3);
                let ready = target.client.hook.when_resolved();
                let service: harness::Client = capnp_rpc::new_client(Tail(target));
                *factory.0.borrow_mut() = Some(service.client);
                let relay = tokio::task::spawn_local(relay.run());
                ready.await.unwrap();
                let mut caller = hub.borrow_mut().connect(1, 2);
                caller.send(|m| m.init_bootstrap().set_question_id(0));
                let returned = recv(&mut caller).await;
                let message::Return(r) = returned
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("bootstrap Return")
                };
                let return_::Results(payload) = r.unwrap().which().unwrap() else {
                    panic!("bootstrap result")
                };
                let cap_descriptor::SenderHosted(export) = payload
                    .unwrap()
                    .get_cap_table()
                    .unwrap()
                    .get(0)
                    .which()
                    .unwrap()
                else {
                    panic!("bootstrap capability")
                };
                caller.send(|m| {
                    let mut call = m.init_call();
                    call.set_question_id(1);
                    call.set_interface_id(harness::Client::TYPE_ID);
                    call.set_method_id(3);
                    call.set_allow_third_party_tail_call(true);
                    call.init_target().set_imported_cap(export);
                });
                let returned = recv(&mut caller).await;
                let message::Return(r) = returned
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("handoff Return")
                };
                assert!(matches!(
                    r.unwrap().which().unwrap(),
                    return_::AwaitFromThirdParty(_)
                ));
                let mut direct = hub.borrow_mut().connect(1, 3);
                let adopted = recv(&mut direct).await;
                let message::ThirdPartyAnswer(adopted) = adopted
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("adopted answer")
                };
                let adopted_id = adopted.unwrap().get_answer_id();
                let result = recv(&mut direct).await;
                assert!(matches!(
                    result
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap(),
                    message::Return(_)
                ));
                drain().await; // The callee's acknowledgement has reached the relay.
                caller.send(|m| {
                    let mut call = m.init_call();
                    call.set_question_id(7);
                    call.set_interface_id(harness::Client::TYPE_ID);
                    call.set_method_id(0);
                    let mut target = call.init_target().init_promised_answer();
                    target.set_question_id(1);
                    target.init_transform(1).get(0).set_get_pointer_field(0);
                });
                let returned = recv(&mut caller).await;
                let message::Return(r) = returned
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("late pipeline Return")
                };
                let r = r.unwrap();
                assert_eq!(r.get_answer_id(), 7);
                let return_::Results(payload) = r.which().unwrap() else {
                    panic!("late pipeline result")
                };
                assert_eq!(
                    payload
                        .unwrap()
                        .get_content()
                        .get_as::<harness::value::Reader>()
                        .unwrap()
                        .get_value(),
                    73
                );
                for id in [7, 1, 0] {
                    finish(&mut caller, id);
                }
                finish(&mut direct, adopted_id);
                drain().await;
                assert!(hub
                    .borrow_mut()
                    .connect(2, 1)
                    .status()
                    .borrow()
                    .shutdowns
                    .is_empty());
                relay.abort();
                callee.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tail_finish_before_child_completion_reclaims_both_interests() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for portable in driver_cases() {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let hub = Rc::new(RefCell::new(Hub::default()));
                    enable(&hub);
                    hub.borrow_mut().introductions = true;
                    let networks = [
                        Hub::network(&hub, 1),
                        Hub::network(&hub, 2),
                        Hub::network(&hub, 3),
                    ];
                    let [caller_network, relay_network, callee_network] = networks;
                    let life = Rc::new(Lifetime::default());
                    let (_gate, receiver) = oneshot::channel();
                    let service: harness::Client = capnp_rpc::new_client(OwnedGate {
                        gate: RefCell::new(Some(receiver)),
                        life: life.clone(),
                    });
                    let callee = tokio::task::spawn_local(
                        support::driver::System::new(
                            portable[2],
                            Box::new(callee_network),
                            Some(service.client),
                        )
                        .run(),
                    );
                    let factory = Rc::new(Factory(RefCell::new(None)));
                    let mut relay = support::driver::System::with_factory(
                        portable[1],
                        Box::new(relay_network),
                        2,
                        factory.clone(),
                    );
                    let target: harness::Client = relay.bootstrap(3);
                    let ready = target.client.hook.when_resolved();
                    let service: harness::Client = capnp_rpc::new_client(Tail(target));
                    *factory.0.borrow_mut() = Some(service.client);
                    let relay = tokio::task::spawn_local(relay.run());
                    ready.await.unwrap();
                    let mut caller =
                        support::driver::System::new(portable[0], Box::new(caller_network), None);
                    let target: harness::Client = caller.bootstrap(2);
                    let caller = tokio::task::spawn_local(caller.run());
                    let request = target.pending_request().send();
                    drain().await;
                    assert_eq!(life.entered.get(), 1);
                    assert_eq!(
                        hub.borrow_mut()
                            .connect(3, 1)
                            .status()
                            .borrow()
                            .adoptions_sent,
                        1
                    );
                    drop(request);
                    drain().await;
                    assert_eq!(
                        life.ended.get(),
                        1,
                        "both canceled answers retained application work; portable={portable:?}"
                    );
                    assert_eq!(life.completed.get(), 0);
                    let error = target
                        .echo_request()
                        .send()
                        .promise
                        .await
                        .err()
                        .expect("unimplemented probe method");
                    assert_eq!(
                        error.kind,
                        capnp::ErrorKind::Unimplemented,
                        "cancellation broke the relay connection"
                    );
                    for (a, b) in [(1, 2), (2, 1), (2, 3), (3, 2), (1, 3), (3, 1)] {
                        let status = hub.borrow_mut().connect(a, b).status();
                        assert!(
                            status.borrow().shutdowns.is_empty(),
                            "unexpected disconnect {a}->{b}"
                        );
                    }
                    caller.abort();
                    relay.abort();
                    callee.abort();
                })
                .await
                .unwrap_or_else(|_| panic!("cancellation stalled: portable={portable:?}"));
            }
        })
        .await;
}

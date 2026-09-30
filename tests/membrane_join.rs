#[allow(dead_code)]
mod support;
use capnp::{capability::Promise, traits::ImbueMut, Error};
use capnp_rpc::{
    membrane::{Direction, Membrane, Policy},
    rpc_capnp::message,
    rpc_twoparty_capnp::{join_key_part, join_result},
    RpcSystem,
};
use futures::FutureExt;
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::{Endpoint, Hub};

struct Gate {
    allowed: bool,
    checks: Cell<u64>,
    directions: RefCell<Vec<Direction>>,
    callback: RefCell<Option<Box<dyn FnOnce()>>>,
    signal: RefCell<Option<Promise<(), Error>>>,
    crossing: RefCell<Option<Box<dyn FnOnce()>>>,
}
impl Gate {
    fn new(allowed: bool) -> Rc<Self> {
        Rc::new(Self {
            allowed,
            checks: Cell::new(0),
            directions: RefCell::new(Vec::new()),
            callback: RefCell::new(None),
            signal: RefCell::new(None),
            crossing: RefCell::new(None),
        })
    }
}
impl Policy for Gate {
    fn export_internal(
        &self,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        let callback = self.crossing.borrow_mut().take();
        if let Some(callback) = callback {
            callback();
        }
        Ok(None)
    }
    fn import_external(
        &self,
        cap: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        self.export_internal(cap)
    }
    fn allow_join(
        &self,
        direction: Direction,
        targets: &[capnp::capability::Client],
    ) -> capnp::Result<bool> {
        assert_eq!(targets.len(), 2);
        self.checks.set(self.checks.get() + 1);
        self.directions.borrow_mut().push(direction);
        let callback = self.callback.borrow_mut().take();
        if let Some(callback) = callback {
            callback();
        }
        Ok(self.allowed)
    }
    fn call(
        &self,
        _: Direction,
        _: u64,
        _: u16,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        Err(Error::failed("joined capability still protected".into()))
    }
    fn on_revoked(&self) -> Option<Promise<(), Error>> {
        self.signal.borrow_mut().take()
    }
}
struct Echo;
impl harness::Server for Echo {}
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
fn bootstrap(peer: &mut Endpoint, question: u32, export: u32) {
    peer.send(|m| {
        let mut r = m.init_return();
        r.set_answer_id(question);
        let mut p = r.init_results();
        let mut caps = Vec::new();
        let mut value = p.reborrow().get_content();
        value.imbue_mut(&mut caps);
        value.set_as_capability(
            capnp_rpc::new_client::<harness::Client, _>(Echo)
                .client
                .hook,
        );
        p.init_cap_table(1).get(0).set_sender_hosted(export);
    });
}
struct Fixture {
    hub: Rc<RefCell<Hub>>,
    peer: Endpoint,
    _network: support::Network,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    joiner: capnp_rpc::Joiner<u8>,
    policy: Rc<Gate>,
    membrane: Membrane,
    inputs: Vec<harness::Client>,
    pending: Option<Promise<Option<harness::Client>, Error>>,
    result: Option<harness::Client>,
    outcome: u64,
    error: Option<Error>,
    questions: Vec<(u32, u32, u16)>,
    finishes: u64,
    releases: u64,
    brand: usize,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(allowed: bool, mixed: bool, reverse: bool) -> Self {
        Self::with_policy(Gate::new(allowed), mixed, reverse).await
    }
    async fn with_policy(policy: Rc<Gate>, mixed: bool, reverse: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let other = Hub::network(&hub, 2);
        let mut system = RpcSystem::new(Box::new(network), None);
        let joiner = system.get_joiner();
        let raw = [
            system.bootstrap::<harness::Client>(2),
            system.bootstrap::<harness::Client>(2),
        ];
        let task = tokio::task::spawn_local(system);
        let mut peer = hub.borrow_mut().connect(2, 1);
        hub.borrow_mut()
            .connect(1, 2)
            .status()
            .borrow_mut()
            .two_party_join = true;
        for i in 0..2 {
            let m = recv(&mut peer).await;
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
            bootstrap(&mut peer, b.unwrap().get_question_id(), 77 + i);
        }
        let mut raw = raw.into_iter();
        let a = capnp::capability::get_resolved_cap(raw.next().unwrap()).await;
        let b = capnp::capability::get_resolved_cap(raw.next().unwrap()).await;
        drain().await;
        while capnp_rpc::Connection::receive_incoming_message(&mut peer)
            .now_or_never()
            .is_some()
        {}
        let membrane = Membrane::new(policy.clone());
        let first = if reverse {
            membrane.import(a)
        } else {
            membrane.export(a)
        };
        let second = if mixed {
            let other = Membrane::new(Gate::new(policy.allowed));
            if reverse {
                other.import(b)
            } else {
                other.export(b)
            }
        } else if reverse {
            membrane.import(b)
        } else {
            membrane.export(b)
        };
        let brand = first.client.hook.get_brand();
        Self {
            hub,
            peer,
            _network: other,
            task,
            joiner,
            policy,
            membrane,
            inputs: vec![first, second],
            pending: None,
            result: None,
            outcome: 0,
            error: None,
            questions: Vec::new(),
            finishes: 0,
            releases: 0,
            brand,
        }
    }
    fn start(&mut self) {
        self.pending = Some(self.joiner.join(self.inputs.clone()));
    }
    fn revoke(&self) {
        self.membrane
            .revoke(Error::failed("join boundary revoked".into()));
    }
    fn reply(&mut self, part: u16, equal: bool) {
        let &(id, join_id, _) = self.questions.iter().find(|(_, _, p)| *p == part).unwrap();
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(id);
            let mut p = r.init_results();
            {
                let mut result = p.reborrow().get_content().init_as::<join_result::Builder>();
                result.set_join_id(join_id);
                result.set_succeeded(equal);
                if equal && part == 0 {
                    let mut table = vec![];
                    let mut value = result.get_cap();
                    value.imbue_mut(&mut table);
                    value.set_as_capability(
                        capnp_rpc::new_client::<harness::Client, _>(Echo)
                            .client
                            .hook,
                    );
                }
            }
            if equal && part == 0 {
                p.init_cap_table(1).get(0).set_sender_hosted(99);
            }
        });
    }
    async fn observe(&mut self) {
        // Poll before and after executor work: starting the public Join is lazy.
        for _ in 0..2 {
            if let Some(pending) = self.pending.as_mut() {
                if let Some(result) = pending.now_or_never() {
                    self.pending.take();
                    match result {
                        Ok(Some(cap)) => {
                            self.outcome = 1;
                            self.result = Some(cap);
                        }
                        Ok(None) => self.outcome = 2,
                        Err(e) => {
                            self.outcome = 3;
                            self.error = Some(e);
                        }
                    }
                }
            }
            drain().await;
        }
        while let Some(m) =
            capnp_rpc::Connection::receive_incoming_message(&mut self.peer).now_or_never()
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
                message::Join(j) => {
                    let j = j.unwrap();
                    let key = j.get_key_part().get_as::<join_key_part::Reader>().unwrap();
                    assert_eq!(key.get_part_count(), 2);
                    self.questions.push((
                        j.get_question_id(),
                        key.get_join_id(),
                        key.get_part_num(),
                    ));
                }
                message::Finish(f) => {
                    assert!(self
                        .questions
                        .iter()
                        .any(|(id, _, _)| *id == f.as_ref().unwrap().get_question_id()));
                    self.finishes += 1;
                }
                message::Release(r) => {
                    if r.unwrap().get_id() == 99 {
                        self.releases += 1;
                    }
                }
                _ => panic!("unexpected caller message"),
            }
        }
    }
    async fn protected(&self) -> bool {
        let cap = self.result.as_ref().unwrap();
        let result =
            tokio::time::timeout(Duration::from_secs(2), cap.echo_request().send().promise)
                .await
                .unwrap();
        cap.client.hook.get_brand() == self.brand
            && result
                .err()
                .is_some_and(|e| e.extra == "joined capability still protected")
    }
}

#[tokio::test(flavor = "current_thread")]
async fn membrane_join_delegates_equal_and_unequal_batches_and_keeps_policy() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for reverse in [false, true] {
                for equal in [false, true] {
                    for order in [[0, 1], [1, 0]] {
                        let mut f = Fixture::new(true, false, reverse).await;
                        f.start();
                        f.observe().await;
                        assert_eq!(f.questions.len(), 2);
                        assert_eq!(f.policy.checks.get(), 1);
                        assert_eq!(
                            *f.policy.directions.borrow(),
                            [if reverse {
                                Direction::Outbound
                            } else {
                                Direction::Inbound
                            }]
                        );
                        f.reply(order[0], equal);
                        f.observe().await;
                        assert_eq!(f.outcome, 0);
                        f.reply(order[1], equal);
                        f.observe().await;
                        assert_eq!(f.outcome, if equal { 1 } else { 2 });
                        assert_eq!(f.finishes, 2);
                        if equal {
                            assert_eq!(f.releases, 0);
                            assert!(f.protected().await);
                            f.result.take();
                            f.observe().await;
                            assert_eq!(f.releases, 1);
                        }
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn denied_mixed_and_reentrant_revoked_joins_send_no_downstream_parts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for allowed in [false, true] {
                for mixed in [false, true] {
                    for reverse_inputs in [false, true] {
                        if allowed && !mixed {
                            continue;
                        }
                        let mut f = Fixture::new(allowed, mixed, false).await;
                        if reverse_inputs {
                            f.inputs.reverse();
                        }
                        f.start();
                        f.observe().await;
                        assert_eq!(f.outcome, 3);
                        assert!(f.questions.is_empty());
                        assert_eq!(f.policy.checks.get(), if mixed { 0 } else { 1 });
                        assert_eq!(
                            f.error.as_ref().unwrap().kind,
                            capnp::ErrorKind::Unimplemented
                        );
                    }
                }
            }
            let mut f = Fixture::new(true, false, false).await;
            let membrane = f.membrane.clone();
            *f.policy.callback.borrow_mut() = Some(Box::new(move || {
                membrane.revoke(Error::failed("policy withdrew Join".into()))
            }));
            f.start();
            f.observe().await;
            assert_eq!(f.outcome, 3);
            assert!(f.questions.is_empty());
            assert_eq!(f.error.as_ref().unwrap().extra, "policy withdrew Join");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_and_revocation_release_join_questions_and_late_results() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for reply_first in [false, true] {
                for revoke in [false, true] {
                    let mut f = Fixture::new(true, false, false).await;
                    f.start();
                    f.observe().await;
                    if reply_first {
                        f.reply(0, true);
                        f.observe().await;
                    }
                    if revoke {
                        f.revoke();
                    } else {
                        f.pending.take();
                    }
                    f.observe().await;
                    assert_eq!(f.finishes, 2);
                    assert_eq!(f.outcome, if revoke { 3 } else { 0 });
                    assert!(f.result.is_none());
                    if !reply_first {
                        f.reply(0, true);
                    }
                    f.reply(1, true);
                    f.observe().await;
                    assert_eq!(f.finishes, 2);
                    assert!(f.result.is_none());
                    assert_eq!(f.releases, u64::from(reply_first));
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn policy_signal_and_revocation_during_completion_prevent_capability_escape() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (tx, rx) = futures::channel::oneshot::channel::<()>();
            let policy = Gate::new(true);
            *policy.signal.borrow_mut() = Some(Promise::from_future(async move {
                rx.await.unwrap();
                Err(Error::failed("policy signal revoked Join".into()))
            }));
            let mut f = Fixture::with_policy(policy, false, false).await;
            f.start();
            f.observe().await;
            assert_eq!(f.questions.len(), 2);
            tx.send(()).unwrap();
            f.observe().await;
            assert_eq!(f.outcome, 3);
            assert_eq!(f.finishes, 2);
            assert_eq!(
                f.error.as_ref().unwrap().extra,
                "policy signal revoked Join"
            );

            for during_crossing in [false, true] {
                for reverse in [false, true] {
                    let mut f = Fixture::new(true, false, reverse).await;
                    f.start();
                    f.observe().await;
                    f.reply(0, true);
                    f.reply(1, true);
                    // Make both replies ready without polling the public Join again.
                    drain().await;
                    if during_crossing {
                        let membrane = f.membrane.clone();
                        *f.policy.crossing.borrow_mut() = Some(Box::new(move || {
                            membrane.revoke(Error::failed("result crossing revoked Join".into()))
                        }));
                    } else {
                        f.revoke();
                    }
                    f.observe().await;
                    assert_eq!(f.outcome, 3);
                    assert_eq!(f.finishes, 2);
                    assert!(f.result.is_none());
                    assert_eq!(f.releases, 1);
                }
            }
        })
        .await;
}

struct Related(Rc<Gate>, Rc<dyn Policy>);
impl Policy for Related {
    fn root_policy(&self) -> Option<Rc<dyn Policy>> {
        Some(self.1.clone())
    }
    fn allow_join(&self, d: Direction, t: &[capnp::capability::Client]) -> capnp::Result<bool> {
        self.0.allow_join(d, t)
    }
    fn call(
        &self,
        d: Direction,
        i: u64,
        m: u16,
        t: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        self.0.call(d, i, m, t)
    }
}
#[tokio::test(flavor = "current_thread")]
async fn nested_policies_authorize_each_boundary_and_shared_roots_do_not_merge_authority() {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
    let joiner = system.get_joiner();
    for inner_allowed in [false, true] {
        let inner_policy = Gate::new(inner_allowed);
        let outer_policy = Gate::new(true);
        let inner = Membrane::new(inner_policy.clone());
        let outer = Membrane::new(outer_policy.clone());
        let caps = (0..2)
            .map(|_| outer.export(inner.export(capnp_rpc::new_client::<harness::Client, _>(Echo))))
            .collect();
        let result = joiner.join(caps).await;
        if inner_allowed {
            assert!(result.unwrap().is_none());
        } else {
            assert_eq!(result.err().unwrap().kind, capnp::ErrorKind::Unimplemented);
        }
        assert_eq!(inner_policy.checks.get(), 1);
        assert_eq!(outer_policy.checks.get(), 1);
    }
    let root = Gate::new(true);
    let left = Gate::new(true);
    let right = Gate::new(true);
    let a = Membrane::new(Rc::new(Related(left.clone(), root.clone())));
    let b = Membrane::new(Rc::new(Related(right.clone(), root)));
    let x = a.export(capnp_rpc::new_client::<harness::Client, _>(Echo));
    let y = b.export(capnp_rpc::new_client::<harness::Client, _>(Echo));
    for inputs in [vec![x.clone(), y.clone()], vec![y.clone(), x.clone()]] {
        assert_eq!(
            joiner.join(inputs).await.err().unwrap().kind,
            capnp::ErrorKind::Unimplemented
        );
    }
    assert_eq!(left.checks.get(), 0);
    assert_eq!(right.checks.get(), 0);
    let y = a.import(capnp_rpc::new_client::<harness::Client, _>(Echo));
    assert_eq!(
        joiner.join(vec![x, y]).await.err().unwrap().kind,
        capnp::ErrorKind::Unimplemented
    );
    assert_eq!(left.checks.get(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn join_delegation_has_a_bounded_nesting_budget() {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
    for depth in [64, 65] {
        let mut caps: Vec<harness::Client> = (0..2).map(|_| capnp_rpc::new_client(Echo)).collect();
        for _ in 0..depth {
            let membrane = Membrane::new(Gate::new(true));
            caps = caps.into_iter().map(|cap| membrane.export(cap)).collect();
        }
        let result = system.get_joiner().join(caps).await;
        if depth == 64 {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.err().unwrap().extra.contains("Join delegation"));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn delegated_join_retains_downstream_answers_until_all_upstream_finishes() {
    struct Factory(RefCell<Vec<harness::Client>>);
    impl capnp_rpc::BootstrapFactory<u8> for Factory {
        fn create_for(&self, _: &u8) -> capnp::Result<capnp::capability::Client> {
            Ok(self.0.borrow_mut().remove(0).client)
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut f = Fixture::new(true, false, false).await;
            let network = Hub::network(&f.hub, 3);
            let _peer_network = Hub::network(&f.hub, 4);
            let task = tokio::task::spawn_local(RpcSystem::new_with_bootstrap_factory(
                Box::new(network),
                3,
                Rc::new(Factory(RefCell::new(f.inputs.clone()))),
            ));
            let mut peer = f.hub.borrow_mut().connect(4, 3);
            f.hub
                .borrow_mut()
                .connect(3, 4)
                .status()
                .borrow_mut()
                .two_party_join = true;
            for q in 0..2 {
                peer.send(|m| m.init_bootstrap().set_question_id(q));
                let m = recv(&mut peer).await;
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
                let capnp_rpc::rpc_capnp::cap_descriptor::SenderHosted(export) =
                    p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
                else {
                    panic!()
                };
                peer.send(|m| {
                    let mut r = m.init_join();
                    r.set_question_id(10 + q);
                    r.reborrow().init_target().set_imported_cap(export);
                    let mut k = r.get_key_part().init_as::<join_key_part::Builder>();
                    k.set_join_id(17);
                    k.set_part_count(2);
                    k.set_part_num(q as u16);
                });
            }
            f.observe().await;
            assert_eq!(f.questions.len(), 2);
            f.reply(0, true);
            f.reply(1, true);
            f.observe().await;
            for _ in 0..2 {
                let m = recv(&mut peer).await;
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
                assert!(p
                    .unwrap()
                    .get_content()
                    .get_as::<join_result::Reader>()
                    .unwrap()
                    .get_succeeded());
            }
            assert_eq!(f.finishes, 0);
            for (id, expected) in [(10, 0), (11, 2)] {
                peer.send(|m| {
                    let mut r = m.init_finish();
                    r.set_question_id(id);
                    r.set_release_result_caps(true);
                    r.set_require_early_cancellation_workaround(false);
                });
                f.observe().await;
                assert_eq!(f.finishes, expected);
            }
            assert_eq!(f.releases, 1);
            task.abort();
        })
        .await;
}

#[derive(serde::Deserialize)]
struct Trace {
    allowed: bool,
    mixed: bool,
    equal: bool,
    reverse: bool,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u64>,
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_membrane_join_traces() {
    let path = reproto_test_support::verification::input("REPROTO_MEMBRANE_JOIN_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, trace) in traces.into_iter().enumerate() {
                let mut f = Fixture::new(trace.allowed, trace.mixed, trace.reverse).await;
                let (mut started, mut r0, mut r1, mut revoked, mut dropped, mut used) =
                    (false, false, false, false, false, false);
                let mut connected = true;
                let mut protected = true;
                for step in trace.steps {
                    match step.action.as_str() {
                        "start" => {
                            started = true;
                            f.start();
                        }
                        "reply0" => {
                            r0 = true;
                            f.reply(0, trace.equal);
                        }
                        "reply1" => {
                            r1 = true;
                            f.reply(1, trace.equal);
                        }
                        "revoke" => {
                            revoked = true;
                            f.revoke();
                        }
                        "drop" => {
                            dropped = true;
                            f.pending.take();
                            f.result.take();
                        }
                        "disconnect" => {
                            connected = false;
                            f.hub.borrow_mut().disconnect_pair(1, 2);
                        }
                        "use" => {
                            used = true;
                            protected &= f.protected().await;
                        }
                        other => panic!("unknown action {other}"),
                    }
                    f.observe().await;
                    if let Some(cap) = &f.result {
                        protected &= cap.client.hook.get_brand() == f.brand;
                    }
                    if f.policy.checks.get() != 0 {
                        assert_eq!(
                            *f.policy.directions.borrow(),
                            [if trace.reverse {
                                Direction::Outbound
                            } else {
                                Direction::Inbound
                            }]
                        );
                    }
                    let actual = vec![
                        started as u64,
                        (f.questions.len() == 2) as u64,
                        r0 as u64,
                        r1 as u64,
                        revoked as u64,
                        dropped as u64,
                        connected as u64,
                        f.outcome,
                        f.finishes,
                        f.policy.checks.get(),
                        used as u64,
                        protected as u64,
                    ];
                    assert_eq!(actual, step.state, "trace {index}, action {}", step.action);
                    if dropped {
                        assert!(f.result.is_none());
                    }
                }
            }
        })
        .await;
}

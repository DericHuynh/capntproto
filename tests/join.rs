#[allow(dead_code)]
mod support;
use capnp::traits::HasTypeId;
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, return_},
    rpc_twoparty_capnp::{join_key_part, join_result},
    RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::{channel::oneshot, FutureExt, TryFutureExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::{Endpoint, Hub};
struct Echo(u32, Rc<Cell<usize>>);
impl Drop for Echo {
    fn drop(&mut self) {
        self.1.set(self.1.get() + 1);
    }
}
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(self.0);
        Ok(())
    }
}
fn client(n: u32) -> harness::Client {
    capnp_rpc::new_client(Echo(n, Rc::new(Cell::new(0))))
}
struct Factory(RefCell<Vec<harness::Client>>);
impl<VatId> capnp_rpc::BootstrapFactory<VatId> for Factory {
    fn create_for(&self, _: &VatId) -> capnp::Result<capnp::capability::Client> {
        Ok(self.0.borrow_mut().remove(0).client)
    }
}
async fn drain() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}
async fn receive(peer: &mut Endpoint) -> Box<dyn capnp_rpc::IncomingMessage> {
    tokio::time::timeout(Duration::from_secs(2), peer.recv())
        .await
        .unwrap()
}
fn join_part(peer: &mut Endpoint, q: u32, j: u32, n: u16, part: u16, target: u32) {
    peer.send(|m| {
        let mut r = m.init_join();
        r.set_question_id(q);
        r.reborrow().init_target().set_imported_cap(target);
        let mut k = r.get_key_part().init_as::<join_key_part::Builder>();
        k.set_join_id(j);
        k.set_part_count(n);
        k.set_part_num(part);
    });
}
fn finish(peer: &mut Endpoint, q: u32, release: bool) {
    peer.send(|m| {
        let mut f = m.init_finish();
        f.set_question_id(q);
        f.set_release_result_caps(release);
        f.set_require_early_cancellation_workaround(false);
    });
}
struct Fixture {
    hub: Rc<RefCell<Hub>>,
    peer: Endpoint,
    exports: [u32; 2],
    gate: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(equal: bool, gated: bool) -> Self {
        let a = client(73);
        let b = if equal { a.clone() } else { client(91) };
        let (tx, rx) = oneshot::channel();
        let gate = rx
            .map_err(|_| capnp::Error::failed("gate gone".into()))
            .boxed_local()
            .shared();
        let caps = if gated {
            vec![a, b]
                .into_iter()
                .map(|cap| {
                    let gate = gate.clone();
                    capnp_rpc::new_future_client(async move {
                        gate.await?;
                        Ok(cap)
                    })
                })
                .collect()
        } else {
            vec![a, b]
        };
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let task = tokio::task::spawn_local(RpcSystem::new_with_bootstrap_factory(
            Box::new(network),
            1,
            Rc::new(Factory(RefCell::new(caps))),
        ));
        let mut peer = hub.borrow_mut().connect(2, 1);
        hub.borrow_mut()
            .connect(1, 2)
            .status()
            .borrow_mut()
            .two_party_join = true;
        let mut exports = [0; 2];
        for (i, export) in exports.iter_mut().enumerate() {
            peer.send(|m| m.init_bootstrap().set_question_id(i as u32));
            let reply = receive(&mut peer).await;
            let message::Return(r) = reply
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
            *export = match p.unwrap().get_cap_table().unwrap().get(0).which().unwrap() {
                cap_descriptor::SenderHosted(id) | cap_descriptor::SenderPromise(id) => id,
                _ => panic!(),
            };
            finish(&mut peer, i as u32, false);
        }
        drain().await;
        Self {
            hub,
            peer,
            exports,
            gate: Some(tx),
            task,
        }
    }
    async fn returns(&mut self) -> Vec<(u32, String, bool, Option<u32>)> {
        drain().await;
        let mut returns = Vec::new();
        loop {
            let Some(msg) =
                capnp_rpc::Connection::receive_incoming_message(&mut self.peer).now_or_never()
            else {
                break;
            };
            let Some(msg) = msg.unwrap() else { break };
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
                    assert!(!r.get_no_finish_needed());
                    match r.which().unwrap() {
                        return_::Results(p) => {
                            let p = p.unwrap();
                            let result = p.get_content().get_as::<join_result::Reader>().unwrap();
                            assert_eq!(result.get_join_id(), 17);
                            let cap = if result.has_cap() {
                                assert_eq!(p.get_cap_table().unwrap().len(), 1);
                                match p.get_cap_table().unwrap().get(0).which().unwrap() {
                                    cap_descriptor::SenderHosted(id)
                                    | cap_descriptor::SenderPromise(id) => Some(id),
                                    _ => panic!(),
                                }
                            } else {
                                None
                            };
                            returns.push((
                                r.get_answer_id(),
                                "results".into(),
                                result.get_succeeded(),
                                cap,
                            ));
                        }
                        return_::Canceled(()) => {
                            returns.push((r.get_answer_id(), "canceled".into(), false, None))
                        }
                        return_::Exception(_) => {
                            returns.push((r.get_answer_id(), "exception".into(), false, None))
                        }
                        _ => panic!(),
                    }
                }
                message::Resolve(_) => (),
                _ => panic!("unexpected wire message"),
            }
        }
        returns.sort_by_key(|r| r.0);
        returns
    }
}
#[tokio::test(flavor = "current_thread")]
async fn incoming_join_collects_parts_resolves_promises_and_returns_one_capability() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for equal in [false, true] {
                for order in [[0, 1], [1, 0]] {
                    let mut f = Fixture::new(equal, true).await;
                    join_part(
                        &mut f.peer,
                        10 + order[0],
                        17,
                        2,
                        order[0] as u16,
                        f.exports[order[0] as usize],
                    );
                    assert!(f.returns().await.is_empty());
                    join_part(
                        &mut f.peer,
                        10 + order[1],
                        17,
                        2,
                        order[1] as u16,
                        f.exports[order[1] as usize],
                    );
                    assert!(f.returns().await.is_empty());
                    f.gate.take().unwrap().send(()).unwrap();
                    let results = f.returns().await;
                    assert_eq!(results.len(), 2);
                    assert!(results.iter().all(|r| r.1 == "results" && r.2 == equal));
                    assert_eq!(
                        results.iter().filter(|r| r.3.is_some()).count(),
                        usize::from(equal)
                    );
                    if let Some(export) = results[0].3 {
                        f.peer.send(|m| {
                            let mut c = m.init_call();
                            c.set_question_id(12);
                            c.set_interface_id(harness::Client::TYPE_ID);
                            c.set_method_id(0);
                            c.init_target().set_imported_cap(export);
                        });
                        // Use a real typed method, then release only the Join results.
                        let message = receive(&mut f.peer).await;
                        let message::Return(ret) = message
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
                        assert_eq!(
                            payload
                                .unwrap()
                                .get_content()
                                .get_as::<harness::value::Reader>()
                                .unwrap()
                                .get_value(),
                            73
                        );
                        finish(&mut f.peer, 12, true);
                    }
                    for q in [10, 11] {
                        finish(&mut f.peer, q, true);
                    }
                    drain().await;
                    // Join ID and question IDs may be reused only after their Finishes.
                    for i in 0..2 {
                        join_part(&mut f.peer, 10 + i, 17, 2, i as u16, f.exports[i as usize]);
                    }
                    assert_eq!(f.returns().await.len(), 2);
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn local_join_preserves_identity_and_propagates_failure() {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
    let joiner = system.get_joiner();
    let a = client(73);
    let b = client(91);
    assert!(joiner.join(vec![a.clone(), b]).await.unwrap().is_none());
    let joined = joiner
        .join(vec![a.clone(), a.clone()])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(joined.client.hook.get_ptr(), a.client.hook.get_ptr());
    assert_eq!(
        joined
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
    assert!(joiner.join::<harness::Client>(vec![]).await.is_err());
    let broken = capnp_rpc::new_future_client::<harness::Client>(async {
        Err(capnp::Error::failed("broken join input".into()))
    });
    assert!(joiner.join(vec![a, broken]).await.is_err());
    drop(system);
    assert!(joiner.join(vec![joined]).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn a_join_handle_does_not_retain_the_system_bootstrap() {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let dropped = Rc::new(Cell::new(0));
    let cap: harness::Client = capnp_rpc::new_client(Echo(73, dropped.clone()));
    let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(cap.client));
    let joiner = system.get_joiner();
    drop(system);
    assert_eq!(dropped.get(), 1);
    assert!(joiner.join(vec![client(73)]).await.is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn outgoing_join_uses_standard_results_and_keeps_connection_after_unequal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let hub = Rc::new(RefCell::new(Hub::default()));
            let a = Hub::network(&hub, 1);
            let b = Hub::network(&hub, 2);
            let server = tokio::task::spawn_local(RpcSystem::new_with_bootstrap_factory(
                Box::new(b),
                2,
                Rc::new(Factory(RefCell::new(vec![client(73), client(91)]))),
            ));
            let mut system = RpcSystem::new(Box::new(a), None);
            let first: harness::Client = system.bootstrap(2);
            let second: harness::Client = system.bootstrap(2);
            let joiner = system.get_joiner();
            for (a, b) in [(1, 2), (2, 1)] {
                hub.borrow_mut()
                    .connect(a, b)
                    .status()
                    .borrow_mut()
                    .two_party_join = true;
            }
            let driver = tokio::task::spawn_local(system);
            assert!(tokio::time::timeout(
                Duration::from_secs(2),
                joiner.join(vec![first.clone(), second.clone()])
            )
            .await
            .unwrap()
            .unwrap()
            .is_none());
            assert_eq!(
                first
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
            assert_eq!(
                second
                    .echo_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                91
            );
            driver.abort();
            server.abort();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_join_releases_waiters_and_allows_id_reuse() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for complete in [false, true] {
                let mut f = Fixture::new(true, true).await;
                join_part(&mut f.peer, 10, 17, 2, 0, f.exports[0]);
                if complete {
                    join_part(&mut f.peer, 11, 17, 2, 1, f.exports[1]);
                }
                assert!(f.returns().await.is_empty());
                finish(&mut f.peer, 10, true);
                let replies = f.returns().await;
                assert_eq!(replies[0].1, "canceled");
                if complete {
                    assert_eq!(replies[1].1, "exception");
                    finish(&mut f.peer, 11, true);
                }
                drain().await;
                f.gate.take().unwrap().send(()).unwrap();
                assert!(f.returns().await.is_empty());
                for i in 0..2 {
                    join_part(&mut f.peer, 10 + i, 17, 2, i as u16, f.exports[i as usize]);
                }
                assert!(f.returns().await.iter().all(|r| r.1 == "results" && r.2));
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn malformed_join_parts_abort_without_publishing_partial_success() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for bad in 0..5 {
                let mut f = Fixture::new(true, true).await;
                join_part(&mut f.peer, 10, 17, 2, 0, f.exports[0]);
                assert!(f.returns().await.is_empty());
                let (q, count, part) = match bad {
                    0 => (11, 0, 0),
                    1 => (11, 2, 2),
                    2 => (11, 2, 0),
                    3 => (11, 3, 1),
                    _ => (10, 2, 1),
                };
                join_part(&mut f.peer, q, 17, count, part, f.exports[1]);
                let reply = receive(&mut f.peer).await;
                assert!(matches!(
                    reply
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap(),
                    message::Abort(_)
                ));
                drain().await;
                assert_eq!(f.hub.borrow_mut().connect(1, 2).status().borrow().aborts, 1);
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn legacy_peer_rejects_join_without_breaking_existing_capabilities() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let hub = Rc::new(RefCell::new(Hub::default()));
            let a = Hub::network(&hub, 1);
            let b = Hub::network(&hub, 2);
            let server = tokio::task::spawn_local(RpcSystem::new_with_bootstrap_factory(
                Box::new(b),
                2,
                Rc::new(Factory(RefCell::new(vec![client(73), client(91)]))),
            ));
            let mut system = RpcSystem::new(Box::new(a), None);
            let first: harness::Client = system.bootstrap(2);
            let second: harness::Client = system.bootstrap(2);
            let joiner = system.get_joiner();
            hub.borrow_mut()
                .connect(1, 2)
                .status()
                .borrow_mut()
                .two_party_join = true;
            let driver = tokio::task::spawn_local(system);
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                joiner.join(vec![first.clone(), second.clone()]),
            )
            .await
            .unwrap();
            assert_eq!(result.err().unwrap().kind, capnp::ErrorKind::Unimplemented);
            assert_eq!(
                first
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
            assert_eq!(
                second
                    .echo_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                91
            );
            assert_eq!(hub.borrow_mut().connect(1, 2).status().borrow().aborts, 0);
            driver.abort();
            server.abort();
        })
        .await;
}

fn bootstrap_result(peer: &mut Endpoint, q: u32, export: u32) {
    use capnp::traits::ImbueMut;
    peer.send(|m| {
        let mut r = m.init_return();
        r.set_answer_id(q);
        let mut p = r.init_results();
        let mut table = vec![];
        {
            let mut content = p.reborrow().get_content();
            content.imbue_mut(&mut table);
            content.set_as_capability(client(0).client.hook);
        }
        p.init_cap_table(1).get(0).set_sender_hosted(export);
    });
}
fn join_result(peer: &mut Endpoint, q: u32, id: u32, success: bool, export: Option<u32>) {
    use capnp::traits::ImbueMut;
    peer.send(|m| {
        let mut ret = m.init_return();
        ret.set_answer_id(q);
        let mut p = ret.init_results();
        let mut table = vec![];
        {
            let mut content = p.reborrow().get_content();
            content.imbue_mut(&mut table);
            let mut r = content.init_as::<join_result::Builder>();
            r.set_join_id(id);
            r.set_succeeded(success);
            if export.is_some() {
                r.get_cap().set_as_capability(client(0).client.hook);
            }
        }
        if let Some(export) = export {
            p.init_cap_table(1).get(0).set_sender_hosted(export);
        }
    });
}
struct Relay {
    f: Fixture,
    host: Endpoint,
    _network: support::Network,
}
impl Relay {
    async fn new() -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let remote = Hub::network(&hub, 3);
        let factory = Rc::new(Factory(RefCell::new(vec![])));
        let mut system =
            RpcSystem::new_with_bootstrap_factory(Box::new(network), 1, factory.clone());
        let a: harness::Client = system.bootstrap(3);
        let b: harness::Client = system.bootstrap(3);
        *factory.0.borrow_mut() = vec![a, b];
        for (a, b) in [(1, 3), (3, 1)] {
            hub.borrow_mut()
                .connect(a, b)
                .status()
                .borrow_mut()
                .two_party_join = true;
        }
        let mut host = hub.borrow_mut().connect(3, 1);
        let task = tokio::task::spawn_local(system);
        for export in [77, 88] {
            let request = receive(&mut host).await;
            let message::Bootstrap(request) = request
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            bootstrap_result(&mut host, request.unwrap().get_question_id(), export);
        }
        let mut peer = hub.borrow_mut().connect(2, 1);
        hub.borrow_mut()
            .connect(1, 2)
            .status()
            .borrow_mut()
            .two_party_join = true;
        let mut exports = [0; 2];
        for (i, export) in exports.iter_mut().enumerate() {
            peer.send(|m| m.init_bootstrap().set_question_id(i as u32));
            let reply = receive(&mut peer).await;
            let message::Return(r) = reply
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
            *export = match p.unwrap().get_cap_table().unwrap().get(0).which().unwrap() {
                cap_descriptor::SenderHosted(id) => id,
                _ => panic!(),
            };
            finish(&mut peer, i as u32, false);
        }
        let mut this = Self {
            f: Fixture {
                hub,
                peer,
                exports,
                gate: None,
                task,
            },
            host,
            _network: remote,
        };
        this.events().await;
        this
    }
    async fn events(&mut self) -> Vec<(String, u32, u32, u16)> {
        drain().await;
        let mut events = Vec::new();
        loop {
            let Some(m) =
                capnp_rpc::Connection::receive_incoming_message(&mut self.host).now_or_never()
            else {
                break;
            };
            let m = m.unwrap().unwrap();
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
                    let k = j.get_key_part().get_as::<join_key_part::Reader>().unwrap();
                    assert_eq!(k.get_part_count(), 2);
                    events.push((
                        "join".into(),
                        j.get_question_id(),
                        k.get_join_id(),
                        k.get_part_num(),
                    ));
                }
                message::Finish(f) => {
                    events.push(("finish".into(), f.unwrap().get_question_id(), 0, 0))
                }
                message::Release(_) => (),
                _ => panic!("unexpected downstream message"),
            }
        }
        events
    }
}
#[tokio::test(flavor = "current_thread")]
async fn forwarded_join_retains_downstream_results_until_upstream_finish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for success in [false, true] {
                let mut r = Relay::new().await;
                join_part(&mut r.f.peer, 10, 17, 2, 0, r.f.exports[0]);
                assert!(r.events().await.is_empty());
                join_part(&mut r.f.peer, 11, 17, 2, 1, r.f.exports[1]);
                let events = r.events().await;
                assert_eq!(events.len(), 2);
                assert!(events.iter().all(|e| e.0 == "join"));
                let j = events[0].2;
                assert_eq!(events[1].2, j);
                join_result(&mut r.host, events[1].1, j, success, None);
                assert!(r.f.returns().await.is_empty());
                assert!(r.events().await.is_empty());
                join_result(&mut r.host, events[0].1, j, success, success.then_some(77));
                let results = r.f.returns().await;
                assert_eq!(results.len(), 2);
                assert!(results.iter().all(|v| v.2 == success));
                assert_eq!(
                    results.iter().filter(|v| v.3.is_some()).count(),
                    usize::from(success)
                );
                assert!(r.events().await.is_empty());
                finish(&mut r.f.peer, 10, true);
                assert!(r.events().await.is_empty());
                finish(&mut r.f.peer, 11, true);
                let finished = r.events().await;
                assert_eq!(finished.len(), 2);
                assert!(finished.iter().all(|e| e.0 == "finish"));
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn forwarded_join_cancellation_finishes_every_downstream_question() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut r = Relay::new().await;
            for i in 0..2 {
                join_part(
                    &mut r.f.peer,
                    10 + i,
                    17,
                    2,
                    i as u16,
                    r.f.exports[i as usize],
                );
            }
            let events = r.events().await;
            assert_eq!(events.len(), 2);
            finish(&mut r.f.peer, 10, true);
            let replies = r.f.returns().await;
            assert_eq!(replies[0].1, "canceled");
            assert_eq!(replies[1].1, "exception");
            let finished = r.events().await;
            assert_eq!(finished.len(), 2);
            assert!(finished.iter().all(|e| e.0 == "finish"));
            for event in &events {
                r.host.send(|m| {
                    let mut r = m.init_return();
                    r.set_answer_id(event.1);
                    r.set_canceled(());
                });
            }
            assert!(r.f.returns().await.is_empty());
            finish(&mut r.f.peer, 11, true);
            assert!(r.events().await.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_join_results_fail_and_release_every_downstream_question() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for bad in 0..5 {
                let mut r = Relay::new().await;
                for i in 0..2 {
                    join_part(
                        &mut r.f.peer,
                        10 + i,
                        17,
                        2,
                        i as u16,
                        r.f.exports[i as usize],
                    );
                }
                let requests = r.events().await;
                assert_eq!(requests.len(), 2);
                let j = requests[0].2;
                let (id, a, b, ca, cb) = match bad {
                    0 => (j.wrapping_add(1), true, true, Some(77), None),
                    1 => (j, true, false, Some(77), None),
                    2 => (j, true, true, None, None),
                    3 => (j, true, true, Some(77), Some(88)),
                    _ => (j, false, false, Some(77), None),
                };
                join_result(&mut r.host, requests[0].1, id, a, ca);
                join_result(&mut r.host, requests[1].1, j, b, cb);
                let replies = r.f.returns().await;
                assert_eq!(replies.len(), 2);
                assert!(replies.iter().all(|v| v.1 == "exception" && v.3.is_none()));
                let mut finished = r
                    .events()
                    .await
                    .into_iter()
                    .map(|e| {
                        assert_eq!(e.0, "finish");
                        e.1
                    })
                    .collect::<Vec<_>>();
                finished.sort_unstable();
                let mut questions = requests.iter().map(|e| e.1).collect::<Vec<_>>();
                questions.sort_unstable();
                assert_eq!(finished, questions);
                for q in [10, 11] {
                    finish(&mut r.f.peer, q, true);
                }
                assert!(r.events().await.is_empty());
                // A new join succeeds on the same route after malformed results.
                for i in 0..2 {
                    join_part(
                        &mut r.f.peer,
                        10 + i,
                        17,
                        2,
                        i as u16,
                        r.f.exports[i as usize],
                    );
                }
                let requests = r.events().await;
                assert_eq!(requests.len(), 2);
                for q in &requests {
                    join_result(&mut r.host, q.1, q.2, false, None);
                }
                assert!(r.f.returns().await.iter().all(|v| v.1 == "results" && !v.2));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn join_over_fragmented_twoparty_stream_preserves_capability_calls() {
    use capnp_rpc::rpc_twoparty_capnp::Side;
    use futures::AsyncReadExt;
    use tokio_util::compat::TokioAsyncReadCompatExt;
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(3), async {
                let (a, b) = tokio::io::duplex(17);
                let (read, write) = a.compat().split();
                let server = RpcSystem::new_with_bootstrap_factory(
                    Box::new(capnp_rpc::twoparty::VatNetwork::new(
                        read,
                        write,
                        Side::Server,
                        Default::default(),
                    )),
                    Side::Server,
                    Rc::new(Factory(RefCell::new(vec![client(73), client(91)]))),
                );
                let (read, write) = b.compat().split();
                let mut system = RpcSystem::new(
                    Box::new(capnp_rpc::twoparty::VatNetwork::new(
                        read,
                        write,
                        Side::Client,
                        Default::default(),
                    )),
                    None,
                );
                let a: harness::Client = system.bootstrap(Side::Server);
                let b: harness::Client = system.bootstrap(Side::Server);
                let joiner = system.get_joiner();
                let disconnect = system.get_disconnector();
                let server = tokio::task::spawn_local(server);
                let task = tokio::task::spawn_local(system);
                assert!(joiner
                    .join(vec![a.clone(), b.clone()])
                    .await
                    .unwrap()
                    .is_none());
                let joined = joiner.join(vec![a.clone(), a]).await.unwrap().unwrap();
                assert_eq!(
                    joined
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
                assert_eq!(
                    b.echo_request()
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    91
                );
                disconnect.await.unwrap();
                task.abort();
                server.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn join_rejects_no_finish_needed_before_publishing_results() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut r = Relay::new().await;
            for i in 0..2 {
                join_part(
                    &mut r.f.peer,
                    10 + i,
                    17,
                    2,
                    i as u16,
                    r.f.exports[i as usize],
                );
            }
            let requests = r.events().await;
            assert_eq!(requests.len(), 2);
            r.host.send(|m| {
                let mut ret = m.init_return();
                ret.set_answer_id(requests[0].1);
                ret.set_no_finish_needed(true);
                let mut result = ret
                    .init_results()
                    .get_content()
                    .init_as::<join_result::Builder>();
                result.set_join_id(requests[0].2);
                result.set_succeeded(false);
            });
            let replies = r.f.returns().await;
            assert_eq!(replies.len(), 2);
            assert!(replies.iter().all(|r| r.1 == "exception" && r.3.is_none()));
            let abort = receive(&mut r.host).await;
            assert!(matches!(
                abort
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap(),
                message::Abort(_)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn join_crosses_independent_twoparty_systems() {
    use capnp_rpc::rpc_twoparty_capnp::Side;
    use futures::AsyncReadExt;
    use tokio_util::compat::TokioAsyncReadCompatExt;
    fn network(io: tokio::io::DuplexStream, side: Side) -> Box<dyn capnp_rpc::VatNetwork<Side>> {
        let (read, write) = io.compat().split();
        Box::new(capnp_rpc::twoparty::VatNetwork::new(
            read,
            write,
            side,
            Default::default(),
        ))
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(3), async {
                let (a, b) = tokio::io::duplex(17);
                let host = RpcSystem::new_with_bootstrap_factory(
                    network(a, Side::Server),
                    Side::Server,
                    Rc::new(Factory(RefCell::new(vec![client(73), client(91)]))),
                );
                let mut outbound = RpcSystem::new(network(b, Side::Client), None);
                let first: harness::Client = outbound.bootstrap(Side::Server);
                let second: harness::Client = outbound.bootstrap(Side::Server);
                let (a, b) = tokio::io::duplex(17);
                let inbound = RpcSystem::new_with_bootstrap_factory(
                    network(a, Side::Server),
                    Side::Server,
                    Rc::new(Factory(RefCell::new(vec![first, second]))),
                );
                let mut caller = RpcSystem::new(network(b, Side::Client), None);
                let a: harness::Client = caller.bootstrap(Side::Server);
                let b: harness::Client = caller.bootstrap(Side::Server);
                let joiner = caller.get_joiner();
                let disconnect = caller.get_disconnector();
                let tasks = [host, outbound, inbound, caller].map(tokio::task::spawn_local);
                assert!(joiner
                    .join(vec![a.clone(), b.clone()])
                    .await
                    .unwrap()
                    .is_none());
                for (cap, n) in [(a, 73), (b, 91)] {
                    assert_eq!(
                        cap.echo_request()
                            .send()
                            .promise
                            .await
                            .unwrap()
                            .get()
                            .unwrap()
                            .get_value(),
                        n
                    );
                }
                disconnect.await.unwrap();
                for task in tasks {
                    task.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn join_preserves_opaque_membrane_authority() {
    use capnp_rpc::membrane::{Direction, Membrane, Policy};
    struct Deny;
    impl Policy for Deny {
        fn call(
            &self,
            _: Direction,
            _: u64,
            _: u16,
            _: &capnp::capability::Client,
        ) -> capnp::Result<Option<capnp::capability::Client>> {
            Err(capnp::Error::failed("membrane denied".into()))
        }
    }
    let hub = Rc::new(RefCell::new(Hub::default()));
    let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
    let joiner = system.get_joiner();
    let membrane = Membrane::new(Rc::new(Deny));
    let a: harness::Client = membrane.export(client(73));
    let b: harness::Client = membrane.export(client(91));
    let joined = joiner
        .join(vec![a.clone(), a.clone()])
        .await
        .unwrap()
        .unwrap();
    assert!(joined
        .echo_request()
        .send()
        .promise
        .await
        .err()
        .unwrap()
        .extra
        .contains("membrane denied"));
    assert_eq!(
        joiner.join(vec![a, b]).await.err().unwrap().kind,
        capnp::ErrorKind::Unimplemented
    );
}

#[tokio::test(flavor = "current_thread")]
async fn join_accepts_one_and_three_parts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for count in [1, 3] {
                let mut f = Fixture::new(true, false).await;
                for part in (0..count).rev() {
                    join_part(
                        &mut f.peer,
                        10 + u32::from(part),
                        17,
                        count,
                        part,
                        f.exports[0],
                    );
                    if part != 0 {
                        assert!(f.returns().await.is_empty());
                    }
                }
                let replies = f.returns().await;
                assert_eq!(replies.len(), usize::from(count));
                assert!(replies.iter().all(|r| r.1 == "results" && r.2));
                assert_eq!(replies.iter().filter(|r| r.3.is_some()).count(), 1);
                for part in 0..count {
                    finish(&mut f.peer, 10 + u32::from(part), true);
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_two_party_join_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_TWO_PARTY_JOIN_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (case_index, case) in cases.iter().enumerate() {
                let remote = case["remote"].as_bool().unwrap();
                let equal = case["equal"].as_bool().unwrap();
                let mut relay = if remote {
                    Some(Relay::new().await)
                } else {
                    None
                };
                let mut local = if remote {
                    None
                } else {
                    Some(Fixture::new(equal, true).await)
                };
                let mut observed: [Option<(String, bool, Option<u32>)>; 2] = [None, None];
                let mut requests: Vec<(String, u32, u32, u16)> = vec![];
                let mut finished = std::collections::HashSet::new();
                for step in case["steps"].as_array().unwrap() {
                    let s: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
                    let action = step["action"].as_str().unwrap();
                    let f = if let Some(r) = relay.as_mut() {
                        &mut r.f
                    } else {
                        local.as_mut().unwrap()
                    };
                    match action {
                        "receive0" | "receive1" => {
                            let i = usize::from(action == "receive1");
                            join_part(&mut f.peer, 10 + i as u32, 17, 2, i as u16, f.exports[i]);
                        }
                        "finish0" | "finish1" => {
                            finish(&mut f.peer, 10 + u32::from(action == "finish1"), true)
                        }
                        "resolve" => {
                            f.gate.take().unwrap().send(()).unwrap();
                        }
                        "disconnect" => f.hub.borrow_mut().disconnect_pair(2, 1),
                        "reply0" | "reply1" => {
                            let i = u16::from(action == "reply1");
                            let request = requests.iter().find(|q| q.3 == i).unwrap();
                            join_result(
                                &mut relay.as_mut().unwrap().host,
                                request.1,
                                request.2,
                                equal,
                                (equal && i == 0).then_some(77),
                            );
                        }
                        "reuse" => {
                            observed = [None, None];
                            requests.clear();
                            finished.clear();
                        }
                        other => panic!("unknown action {other}"),
                    }
                    let f = if let Some(r) = relay.as_mut() {
                        &mut r.f
                    } else {
                        local.as_mut().unwrap()
                    };
                    for (q, kind, success, cap) in f.returns().await {
                        assert!((10..12).contains(&q));
                        let i = (q - 10) as usize;
                        assert!(
                            observed[i].is_none(),
                            "duplicate Return in trace {case_index}: {step}"
                        );
                        observed[i] = Some((kind, success, cap));
                    }
                    for i in 0..2 {
                        let code = observed[i]
                            .as_ref()
                            .map_or(0, |(kind, success, _)| match kind.as_str() {
                                "results" => {
                                    if *success {
                                        1
                                    } else {
                                        2
                                    }
                                }
                                "canceled" => 3,
                                "exception" => 4,
                                _ => panic!(),
                            });
                        assert_eq!(code, s[6 + i], "trace {case_index}: {step}");
                    }
                    assert_eq!(
                        observed
                            .iter()
                            .flatten()
                            .filter(|(_, _, cap)| cap.is_some())
                            .count() as u32,
                        s[8]
                    );
                    if let Some(r) = relay.as_mut() {
                        for event in r.events().await {
                            match event.0.as_str() {
                                "join" => {
                                    assert!(!requests.iter().any(|q| q.3 == event.3));
                                    requests.push(event);
                                }
                                "finish" => {
                                    if requests.iter().any(|q| q.1 == event.1) {
                                        assert!(finished.insert(event.1));
                                    }
                                }
                                _ => panic!(),
                            }
                        }
                        assert_eq!(
                            requests.len(),
                            if s[4] == 1 { 2 } else { 0 },
                            "collect before forward: trace {case_index}: {step}"
                        );
                        assert_eq!(
                            finished.len(),
                            if s[4] == 1 && s[9] == 0 { 2 } else { 0 },
                            "relay retention: trace {case_index}: {step}"
                        );
                    }
                }
            }
        })
        .await;
}

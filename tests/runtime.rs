mod support;
use capnp::{capability::FromClientHook, traits::HasTypeId};
use capnp_rpc::{
    rpc_capnp::{message, return_},
    RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc, time::Duration};
use support::{Endpoint, Hub};

struct Echo(u32, Rc<RefCell<Vec<u32>>>);
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        let v = p.get()?.get_value();
        self.1.borrow_mut().push(v);
        r.get().set_value(self.0 + v);
        Ok(())
    }
    async fn tail(
        self: Rc<Self>,
        p: harness::TailParams,
        r: harness::TailResults,
    ) -> capnp::Result<()> {
        let p = p.get()?;
        let mut request = p.get_cap()?.echo_request();
        request.get().set_value(p.get_value());
        r.hook.tail_call(request.hook).await
    }
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        mut r: harness::BounceResults,
    ) -> capnp::Result<()> {
        r.get().set_cap(p.get()?.get_cap()?);
        Ok(())
    }
}
async fn echo(c: &harness::Client, value: u32) -> u32 {
    let mut r = c.echo_request();
    r.get().set_value(value);
    r.send().promise.await.unwrap().get().unwrap().get_value()
}
async fn bootstrap(raw: &mut Endpoint, question: u32) -> u32 {
    raw.send(|m| m.init_bootstrap().set_question_id(question));
    let msg = raw.recv().await;
    let message::Return(ret) = msg
        .get_body()
        .unwrap()
        .get_as::<message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!("expected Return")
    };
    let return_::Results(results) = ret.unwrap().which().unwrap() else {
        panic!("expected results")
    };
    let capnp_rpc::rpc_capnp::cap_descriptor::SenderHosted(id) = results
        .unwrap()
        .get_cap_table()
        .unwrap()
        .get(0)
        .which()
        .unwrap()
    else {
        panic!("expected export")
    };
    id
}
fn provide(raw: &mut Endpoint, export: u32) {
    raw.send(|m| {
        let mut p = m.init_provide();
        p.set_question_id(10);
        p.reborrow().init_target().set_imported_cap(export);
        p.get_recipient()
            .set_as::<capnp::data::Owned>(&[3, 77][..])
            .unwrap();
    });
}
fn accept(raw: &mut Endpoint, embargo: bool) {
    raw.send(|m| {
        let mut a = m.init_accept();
        a.set_question_id(20);
        a.reborrow()
            .get_provision()
            .set_as::<capnp::data::Owned>(&[77][..])
            .unwrap();
        if embargo {
            a.set_embargo(b"order");
        }
    });
}
fn call(raw: &mut Endpoint, question: u32, target: u32, promised: bool, value: u32) {
    raw.send(|m| {
        let mut c = m.init_call();
        c.set_question_id(question);
        c.set_interface_id(harness::Client::TYPE_ID);
        c.set_method_id(0);
        let mut t = c.reborrow().init_target();
        if promised {
            t.init_promised_answer().set_question_id(target);
        } else {
            t.set_imported_cap(target);
        }
        c.init_params()
            .get_content()
            .init_as::<harness::echo_params::Builder>()
            .set_value(value);
    });
}
#[tokio::test(flavor = "current_thread")]
async fn multiple_connections_bootstrap_forward_caps_and_disconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let log = Rc::new(RefCell::new(vec![]));
                let a = Hub::network(&hub, 1);
                let b = Hub::network(&hub, 2);
                let c = Hub::network(&hub, 3);
                let bc: harness::Client = capnp_rpc::new_client(Echo(100, log.clone()));
                let cc: harness::Client = capnp_rpc::new_client(Echo(200, log.clone()));
                let bt = tokio::task::spawn_local(RpcSystem::new(Box::new(b), Some(bc.client)));
                let ct = tokio::task::spawn_local(RpcSystem::new(Box::new(c), Some(cc.client)));
                let mut system = RpcSystem::new(Box::new(a), None);
                let b: harness::Client = system.bootstrap(2);
                let c: harness::Client = system.bootstrap(3);
                let b2: harness::Client = system.bootstrap(2);
                let disconnect = system.get_disconnector();
                let at = tokio::task::spawn_local(system);
                assert_eq!(echo(&b, 1).await, 101);
                assert_eq!(echo(&c, 2).await, 202);
                assert_eq!(echo(&b2, 3).await, 103);
                // Export a capability from C across B and recover it on the original vat.
                let mut bounce = b.bounce_request();
                bounce.get().set_cap(c.clone());
                let returned = bounce
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                assert_eq!(echo(&returned, 4).await, 204);
                let resolved_original = capnp::capability::get_resolved_cap(c).await;
                let resolved_returned = capnp::capability::get_resolved_cap(returned).await;
                assert_eq!(
                    resolved_original.into_client_hook().get_ptr(),
                    resolved_returned.into_client_hook().get_ptr()
                );
                disconnect.await.unwrap();
                assert!(b.echo_request().send().promise.await.is_err());
                for t in [at, bt, ct] {
                    t.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn wire_provide_accept_embargo_orders_pipeline_and_finish_unregisters() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let network = Hub::network(&hub, 1);
                let log = Rc::new(RefCell::new(vec![]));
                let service: harness::Client = capnp_rpc::new_client(Echo(0, log.clone()));
                let task = tokio::task::spawn_local(RpcSystem::new(
                    Box::new(network),
                    Some(service.client),
                ));
                let mut broker = hub.borrow_mut().connect(2, 1);
                let mut recipient = hub.borrow_mut().connect(3, 1);
                let export = bootstrap(&mut broker, 0).await;
                // Accept and pipelined call arrive before Provide on a different connection.
                accept(&mut recipient, true);
                call(&mut recipient, 21, 20, true, 2);
                provide(&mut broker, export);
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), recipient.recv())
                        .await
                        .is_err()
                );
                assert!(log.borrow().is_empty());
                call(&mut broker, 11, export, false, 1);
                broker.send(|m| {
                    let mut d = m.init_disembargo();
                    d.reborrow()
                        .init_target()
                        .init_promised_answer()
                        .set_question_id(10);
                    d.init_context().set_accept(b"order");
                });
                for _ in 0..2 {
                    let msg = recipient.recv().await;
                    assert!(matches!(
                        msg.get_body()
                            .unwrap()
                            .get_as::<message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap(),
                        message::Return(_)
                    ));
                }
                assert_eq!(*log.borrow(), vec![1, 2]);
                assert_eq!(hub.borrow().provision_count(), 1);
                broker.send(|m| {
                    m.init_finish().set_question_id(10);
                });
                broker.recv().await; // Return for the old-route call.
                                     // A same-connection bootstrap is a processing barrier after Finish.
                bootstrap(&mut broker, 30).await;
                assert_eq!(hub.borrow().provision_count(), 0);
                task.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn automatic_wire_handoff_shortens_proxy_and_preserves_identity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                hub.borrow_mut().introductions = true;
                let a = Hub::network(&hub, 1);
                let b = Hub::network(&hub, 2);
                let c = Hub::network(&hub, 3);
                let log = Rc::new(RefCell::new(vec![]));
                let mut owners = capnp_rpc::CapabilityServerSet::new();
                let owner: harness::Client = owners.new_client(Echo(300, log.clone()));
                let ct = tokio::task::spawn_local(RpcSystem::new(
                    Box::new(c),
                    Some(owner.clone().client),
                ));
                let echoer: harness::Client = capnp_rpc::new_client(Echo(100, log));
                let bt = tokio::task::spawn_local(RpcSystem::new(Box::new(b), Some(echoer.client)));
                let mut system = RpcSystem::new(Box::new(a), None);
                let remote: harness::Client = system.bootstrap(3);
                let broker: harness::Client = system.bootstrap(2);
                let at = tokio::task::spawn_local(system);
                let remote = capnp::capability::get_resolved_cap(remote).await;
                let mut request = broker.bounce_request();
                request.get().set_cap(remote.clone());
                // B bounces the deferred introduction back over its vine.
                // Neither an Accept nor another introduction is necessary.
                let returned = request
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                assert_eq!(echo(&returned, 9).await, 309);
                let returned = capnp::capability::get_resolved_cap(returned).await;
                assert_eq!(
                    remote.into_client_hook().get_ptr(),
                    returned.into_client_hook().get_ptr()
                );
                assert_eq!(hub.borrow().introduction_count, 1);
                assert_eq!(hub.borrow().accept_count, 0);
                // The old broker route can disappear after acceptance.
                bt.abort();
                assert_eq!(echo(&owner, 1).await, 301);
                for t in [at, ct] {
                    t.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn promised_capability_resolves_to_third_party_after_earlier_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                hub.borrow_mut().introductions = true;
                let a = Hub::network(&hub, 1);
                let b = Hub::network(&hub, 2);
                let c = Hub::network(&hub, 3);
                let log = Rc::new(RefCell::new(vec![]));
                let owner: harness::Client = capnp_rpc::new_client(Echo(0, log.clone()));
                let ct = tokio::task::spawn_local(RpcSystem::new(Box::new(c), Some(owner.client)));
                let (sender, receiver) = futures::channel::oneshot::channel::<harness::Client>();
                let deferred: harness::Client = capnp_rpc::new_future_client(async {
                    receiver
                        .await
                        .map_err(|_| capnp::Error::failed("canceled".into()))
                });
                let mut broker = RpcSystem::new(Box::new(b), Some(deferred.client));
                let remote: harness::Client = broker.bootstrap(3);
                let bt = tokio::task::spawn_local(broker);
                let mut recipient = RpcSystem::new(Box::new(a), None);
                let promise: harness::Client = recipient.bootstrap(2);
                let at = tokio::task::spawn_local(recipient);
                let mut first = promise.echo_request();
                first.get().set_value(1);
                let first = first.send();
                for _ in 0..5 {
                    tokio::task::yield_now().await;
                }
                let remote = capnp::capability::get_resolved_cap(remote).await;
                assert!(sender.send(remote).is_ok());
                assert_eq!(first.promise.await.unwrap().get().unwrap().get_value(), 1);
                let direct = capnp::capability::get_resolved_cap(promise).await;
                assert_eq!(echo(&direct, 2).await, 2);
                assert_eq!(*log.borrow(), vec![1, 2]);
                assert!(hub.borrow().introduction_count >= 1);
                for t in [at, bt, ct] {
                    t.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tail_calls_return_values_and_capability_params_survive_redirection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let a = Hub::network(&hub, 1);
                let b = Hub::network(&hub, 2);
                let log = Rc::new(RefCell::new(vec![]));
                let local: harness::Client = capnp_rpc::new_client(Echo(100, log.clone()));
                let server: harness::Client = capnp_rpc::new_client(Echo(0, log));
                // Local tail-call path.
                let mut request = server.tail_request();
                request.get().set_cap(local.clone());
                request.get().set_value(1);
                assert_eq!(
                    request
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    101
                );
                let bt = tokio::task::spawn_local(RpcSystem::new(Box::new(b), Some(server.client)));
                let mut system = RpcSystem::new(Box::new(a), None);
                let remote: harness::Client = system.bootstrap(2);
                let at = tokio::task::spawn_local(system);
                // Build before bootstrap resolution, send afterwards with embedded capability.
                let mut request = remote.tail_request();
                request.get().set_cap(local);
                request.get().set_value(2);
                let _settled = capnp::capability::get_resolved_cap(remote).await;
                assert_eq!(
                    request
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    102
                );
                at.abort();
                bt.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn finish_rejects_pending_accept_and_forged_recipient_cannot_claim() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let network = Hub::network(&hub, 1);
                let log = Rc::new(RefCell::new(vec![]));
                let service: harness::Client = capnp_rpc::new_client(Echo(0, log.clone()));
                let task = tokio::task::spawn_local(RpcSystem::new(
                    Box::new(network),
                    Some(service.client),
                ));
                let mut broker = hub.borrow_mut().connect(2, 1);
                let mut recipient = hub.borrow_mut().connect(3, 1);
                let mut attacker = hub.borrow_mut().connect(4, 1);
                let export = bootstrap(&mut broker, 0).await;
                provide(&mut broker, export);
                accept(&mut recipient, true);
                accept(&mut attacker, false); // Correct token, wrong authenticated endpoint.
                call(&mut attacker, 21, 20, true, 666);
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), attacker.recv())
                        .await
                        .is_err()
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), recipient.recv())
                        .await
                        .is_err()
                );
                broker.send(|m| m.init_finish().set_question_id(10));
                let response = recipient.recv().await;
                let message::Return(ret) = response
                    .get_body()
                    .unwrap()
                    .get_as::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("expected Return")
                };
                assert!(matches!(
                    ret.unwrap().which().unwrap(),
                    return_::Exception(_)
                ));
                assert!(log.borrow().is_empty());
                assert_eq!(hub.borrow().provision_count(), 0);
                task.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_return_aborts_only_its_connection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let hub = Rc::new(RefCell::new(Hub::default()));
                let network = Hub::network(&hub, 1);
                let service: harness::Client =
                    capnp_rpc::new_client(Echo(0, Rc::new(RefCell::new(vec![]))));
                let task = tokio::task::spawn_local(RpcSystem::new(
                    Box::new(network),
                    Some(service.client),
                ));
                let mut bad = hub.borrow_mut().connect(2, 1);
                // High IDs are reserved for pipeline-only calls and tolerate late Returns.
                bad.send(|m| m.init_return().set_answer_id(0x7fff_ffff));
                let response = bad.recv().await;
                assert!(matches!(
                    response
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap(),
                    message::Abort(_)
                ));
                let mut good = hub.borrow_mut().connect(3, 1);
                bootstrap(&mut good, 0).await;
                task.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

struct LoggingPolicy(Rc<RefCell<Vec<(capnp_rpc::membrane::Direction, u16)>>>);
impl capnp_rpc::membrane::Policy for LoggingPolicy {
    fn call(
        &self,
        d: capnp_rpc::membrane::Direction,
        _: u64,
        m: u16,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        self.0.borrow_mut().push((d, m));
        Ok(None)
    }
}
#[tokio::test(flavor = "current_thread")]
async fn membrane_wraps_callbacks_pipelines_identity_and_revokes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                use capnp_rpc::membrane::{Direction, Membrane};
                let calls = Rc::new(RefCell::new(vec![]));
                let membrane = Membrane::new(Rc::new(LoggingPolicy(calls.clone())));
                let log = Rc::new(RefCell::new(vec![]));
                let inside: harness::Client = capnp_rpc::new_client(Echo(100, log.clone()));
                let outside: harness::Client = capnp_rpc::new_client(Echo(200, log));
                let exported = membrane.export(inside.clone());
                let again = membrane.export(inside.clone());
                assert_eq!(
                    exported.as_client_hook().get_ptr(),
                    again.as_client_hook().get_ptr()
                );
                assert_eq!(
                    membrane.import(exported.clone()).as_client_hook().get_ptr(),
                    inside.as_client_hook().get_ptr()
                );
                let mut request = exported.tail_request();
                request.get().set_cap(outside.clone());
                request.get().set_value(1);
                assert_eq!(
                    request
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    201
                );
                assert!(calls.borrow().contains(&(Direction::Inbound, 2)));
                assert!(calls.borrow().contains(&(Direction::Outbound, 0)));
                let mut request = exported.bounce_request();
                request.get().set_cap(outside.clone());
                let response = request.send();
                let pipeline = response.pipeline.get_cap();
                assert_eq!(echo(&pipeline, 2).await, 202);
                let returned = response
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                assert_eq!(
                    returned.as_client_hook().get_ptr(),
                    outside.as_client_hook().get_ptr()
                );
                let unresolved: harness::Client =
                    capnp_rpc::new_future_client(futures::future::pending());
                let pending = membrane.export(unresolved).echo_request().send();
                tokio::task::yield_now().await;
                membrane.revoke(capnp::Error::failed("policy revoked".into()));
                assert!(pending.promise.await.is_err());
                assert!(exported.echo_request().send().promise.await.is_err());
                // Returning the outside capability across the same boundary unwraps it;
                // revocation must not revoke authority the caller already held.
                assert_eq!(echo(&outside, 3).await, 203);
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize)]
struct WireTrace {
    actions: Vec<String>,
    state: [u8; 8],
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_wire_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_TLC_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<WireTrace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                for trace in traces {
                    let hub = Rc::new(RefCell::new(Hub::default()));
                    let network = Hub::network(&hub, 1);
                    let log = Rc::new(RefCell::new(vec![]));
                    let service: harness::Client = capnp_rpc::new_client(Echo(0, log.clone()));
                    let task = tokio::task::spawn_local(RpcSystem::new(
                        Box::new(network),
                        Some(service.client),
                    ));
                    let mut broker = hub.borrow_mut().connect(2, 1);
                    let mut recipient = hub.borrow_mut().connect(3, 1);
                    let export = bootstrap(&mut broker, 0).await;
                    for action in &trace.actions {
                        match action.as_str() {
                            "provide" => provide(&mut broker, export),
                            "accept" => accept(&mut recipient, true),
                            "release" => broker.send(|m| {
                                let mut d = m.init_disembargo();
                                d.reborrow()
                                    .init_target()
                                    .init_promised_answer()
                                    .set_question_id(10);
                                d.init_context().set_accept(b"order");
                            }),
                            "old" => call(&mut broker, 11, export, false, 1),
                            "new" => call(&mut recipient, 21, 20, true, 2),
                            "finish" => broker.send(|m| m.init_finish().set_question_id(10)),
                            other => panic!("unexpected TLC action {other}"),
                        }
                        // Drain this deterministic schedule before the next modeled input.
                        for _ in 0..20 {
                            tokio::task::yield_now().await;
                        }
                    }
                    let [phase, requested, _, accepted, failed, old, queued, delivered] =
                        trace.state;
                    let mut expected = vec![];
                    if old != 0 {
                        expected.push(1);
                    }
                    if delivered != 0 {
                        expected.push(2);
                    }
                    assert_eq!(*log.borrow(), expected, "trace {:?}", trace.actions);
                    assert_eq!(
                        hub.borrow().provision_count(),
                        usize::from(phase == 1),
                        "trace {:?}",
                        trace.actions
                    );
                    if accepted != 0 || failed != 0 {
                        let count = 1 + usize::from(queued != 0);
                        for _ in 0..count {
                            let msg = recipient.recv().await;
                            let message::Return(ret) = msg
                                .get_body()
                                .unwrap()
                                .get_as::<message::Reader>()
                                .unwrap()
                                .which()
                                .unwrap()
                            else {
                                panic!("expected Return: {:?}", trace.actions)
                            };
                            assert_eq!(
                                matches!(ret.unwrap().which().unwrap(), return_::Exception(_)),
                                failed != 0,
                                "trace {:?}",
                                trace.actions
                            );
                        }
                    } else if requested != 0 {
                        assert!(
                            tokio::time::timeout(Duration::from_millis(1), recipient.recv())
                                .await
                                .is_err(),
                            "premature Return: {:?}",
                            trace.actions
                        );
                    }
                    task.abort();
                    let _ = task.await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

struct PersistentEntry;
impl capnp_rpc::persistent_capnp::persistent::Server<capnp::data::Owned, capnp::data::Owned>
    for PersistentEntry
{
    async fn save(
        self: Rc<Self>,
        p: capnp_rpc::persistent_capnp::persistent::SaveParams<
            capnp::data::Owned,
            capnp::data::Owned,
        >,
        mut r: capnp_rpc::persistent_capnp::persistent::SaveResults<
            capnp::data::Owned,
            capnp::data::Owned,
        >,
    ) -> capnp::Result<()> {
        if p.get()?.get_seal_for()? != b"authorized-owner" {
            return Err(capnp::Error::failed("owner cannot save this object".into()));
        }
        r.get().set_sturdy_ref(&b"realm-owned-reference"[..])?;
        Ok(())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn standard_persistent_interface_transports_owner_and_sturdy_ref() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                type Persistent = capnp_rpc::persistent_capnp::persistent::Client<
                    capnp::data::Owned,
                    capnp::data::Owned,
                >;
                assert_eq!(Persistent::TYPE_ID, 0xc8cb212fcd9f5691);
                let hub = Rc::new(RefCell::new(Hub::default()));
                let a = Hub::network(&hub, 1);
                let b = Hub::network(&hub, 2);
                let service: Persistent = capnp_rpc::new_client(PersistentEntry);
                let bt =
                    tokio::task::spawn_local(RpcSystem::new(Box::new(b), Some(service.client)));
                let mut system = RpcSystem::new(Box::new(a), None);
                let client: Persistent = system.bootstrap(2);
                let at = tokio::task::spawn_local(system);
                assert!(client.save_request().send().promise.await.is_err());
                let mut save = client.save_request();
                save.get().set_seal_for(&b"authorized-owner"[..]).unwrap();
                assert_eq!(
                    save.send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_sturdy_ref()
                        .unwrap(),
                    b"realm-owned-reference"
                );
                at.abort();
                bt.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

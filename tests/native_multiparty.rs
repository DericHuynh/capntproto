use capnp_rpc::{Connection, VatNetwork};
use reproto::{
    native_rpc::{
        Connector, DirectoryConnector, Handle, Network, Options, RetryPolicy, RetryingConnector,
        RouteStatus,
    },
    transport::{self, Identity},
};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
type VatId = [u8; 32];
struct Service {
    stored: Rc<RefCell<Option<harness::Client>>>,
}
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(p.get()?.get_value());
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        mut r: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = p.get()?.get_cap()?;
        *self.stored.borrow_mut() = Some(cap.clone());
        r.get().set_cap(cap);
        Ok(())
    }
}
async fn pair(a: &Identity, b: &Identity, ah: &Handle, bh: &Handle) -> capnp::Result<()> {
    let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = right.local_addr().unwrap();
    let (a_session, b_session) = tokio::join!(
        transport::connect_authenticated(
            left,
            addr,
            a,
            b.public_key(),
            Some([7; 32]),
            b"native-handoff-test"
        ),
        transport::accept_authenticated(
            right,
            b,
            a.public_key(),
            Some([7; 32]),
            b"native-handoff-test"
        )
    );
    ah.attach(a_session.unwrap())?;
    bh.attach(b_session.unwrap())?;
    Ok(())
}
struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}
async fn echo(client: &harness::Client, value: u32) -> capnp::Result<()> {
    let mut req = client.echo_request();
    req.get().set_value(value);
    assert_eq!(req.send().promise.await?.get()?.get_value(), value);
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn native_three_party_handoff_over_native_udp() {
    handoff(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn native_handoff_dials_missing_native_route() {
    handoff(true).await;
}
async fn handoff(dial: bool) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let ids = Rc::new([
                    Identity::generate(),
                    Identity::generate(),
                    Identity::generate(),
                ]);
                let (an, ah) = Network::new(ids[0].public_key());
                let (bn, bh) = Network::new(ids[1].public_key());
                let dials = Rc::new(Cell::new(0));
                let (cn, ch) = if dial {
                    Network::with_connector(
                        ids[2].public_key(),
                        Rc::new(HandoffConnector {
                            ids: ids.clone(),
                            host: ah.clone(),
                            dials: dials.clone(),
                        }),
                    )
                } else {
                    Network::new(ids[2].public_key())
                };
                pair(&ids[0], &ids[1], &ah, &bh).await?;
                if !dial {
                    pair(&ids[0], &ids[2], &ah, &ch).await?;
                }
                pair(&ids[1], &ids[2], &bh, &ch).await?;
                let slots: Vec<_> = (0..3).map(|_| Rc::new(RefCell::new(None))).collect();
                let boot: Vec<harness::Client> = slots
                    .iter()
                    .map(|s| capnp_rpc::new_client(Service { stored: s.clone() }))
                    .collect();
                let a = capnp_rpc::RpcSystem::new(Box::new(an), Some(boot[0].client.clone()));
                let mut b = capnp_rpc::RpcSystem::new(Box::new(bn), Some(boot[1].client.clone()));
                let c = capnp_rpc::RpcSystem::new(Box::new(cn), Some(boot[2].client.clone()));
                let owner: harness::Client = b.bootstrap(ids[0].public_key());
                let recipient: harness::Client = b.bootstrap(ids[2].public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(a),
                    tokio::task::spawn_local(b),
                    tokio::task::spawn_local(c),
                ]);
                echo(&owner, 1).await?;
                let mut req = recipient.bounce_request();
                req.get().set_cap(owner);
                let returned = req.send().promise.await?.get()?.get_cap()?;
                echo(&returned, 2).await?;
                let direct = slots[2].borrow().as_ref().unwrap().clone();
                echo(&direct, 3).await?;
                assert!(
                    ah.stats().published > 0,
                    "host never received native Provide"
                );
                // The recipient's retained capability now uses its authenticated
                // host connection, and remains usable after the introducer goes away.
                bh.disconnect(ids[0].public_key());
                bh.disconnect(ids[2].public_key());
                echo(&direct, 4).await?;
                assert_eq!(dials.get(), usize::from(dial));
                assert_eq!(
                    ch.route_status(ids[0].public_key()),
                    Some(RouteStatus::Authenticated)
                );
                for slot in slots {
                    slot.borrow_mut().take();
                }
                Ok::<(), capnp::Error>(())
            })
            .await
            .unwrap()
            .unwrap();
        })
        .await;
}
fn send(
    route: &mut dyn Connection<VatId>,
    fill: impl FnOnce(capnp_rpc::rpc_capnp::message::Builder<'_>),
) {
    let mut msg = route.new_outgoing_message(64);
    fill(msg.get_body().unwrap().init_as());
    drop(msg.send());
}
async fn wait_stats(handle: &Handle, p: impl Fn(reproto::native_rpc::Stats) -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !p(handle.stats()) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
}
struct Fixture {
    ids: [Identity; 3],
    networks: Vec<Network>,
    handles: Vec<Handle>,
    _tasks: Tasks,
    intro: Box<dyn Connection<VatId>>,
    recipient: Box<dyn Connection<VatId>>,
    provision: capnp::message::Builder<capnp::message::HeapAllocator>,
    completion: capnp::message::Builder<capnp::message::HeapAllocator>,
    cap: u32,
}
impl Fixture {
    async fn new() -> capnp::Result<Self> {
        let ids = [
            Identity::generate(),
            Identity::generate(),
            Identity::generate(),
        ];
        let (mut networks, handles): (Vec<_>, Vec<_>) =
            ids.iter().map(|i| Network::new(i.public_key())).unzip();
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            pair(&ids[a], &ids[b], &handles[a], &handles[b]).await?;
        }
        let host = networks.remove(0);
        let bootstrap: harness::Client = capnp_rpc::new_client(Service {
            stored: Rc::new(RefCell::new(None)),
        });
        let tasks = Tasks(vec![tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
            Box::new(host),
            Some(bootstrap.client),
        ))]);
        let mut intro = networks[0].connect(ids[0].public_key()).unwrap();
        let recipient = networks[1].connect(ids[0].public_key()).unwrap();
        send(&mut *intro, |m| m.init_bootstrap().set_question_id(0));
        let response = intro.receive_incoming_message().await?.unwrap();
        let msg = response
            .get_body()?
            .get_as::<capnp_rpc::rpc_capnp::message::Reader<'_>>()?;
        let cap = match msg.which()? {
            capnp_rpc::rpc_capnp::message::Return(r) => match (match r?.which()? {
                capnp_rpc::rpc_capnp::return_::Results(p) => p?,
                _ => panic!(),
            })
            .get_cap_table()?
            .get(0)
            .which()?
            {
                capnp_rpc::rpc_capnp::cap_descriptor::SenderHosted(id) => id,
                _ => panic!(),
            },
            _ => panic!(),
        };
        let mut contact = capnp::message::Builder::new_default();
        let mut provision = capnp::message::Builder::new_default();
        assert!(intro.introduce_to(
            ids[2].public_key(),
            contact.get_root()?,
            provision.get_root()?
        )?);
        let mut completion = capnp::message::Builder::new_default();
        networks[1]
            .connect(ids[1].public_key())
            .unwrap()
            .connect_to_introduced(contact.get_root_as_reader()?, completion.get_root()?)?;
        Ok(Self {
            ids,
            networks,
            handles,
            _tasks: tasks,
            intro,
            recipient,
            provision,
            completion,
            cap,
        })
    }
    fn publish(&mut self) {
        let token = self
            .provision
            .get_root_as_reader::<capnp::any_pointer::Reader>()
            .unwrap();
        let cap = self.cap;
        send(&mut *self.intro, |m| {
            let mut p = m.init_provide();
            p.set_question_id(1);
            p.reborrow().init_target().set_imported_cap(cap);
            p.get_recipient()
                .set_as::<capnp::any_pointer::Owned>(token)
                .unwrap();
        });
    }
    fn finish(&mut self) {
        send(&mut *self.intro, |m| {
            let mut f = m.init_finish();
            f.set_question_id(1);
            f.set_release_result_caps(true);
        });
    }
}
#[tokio::test(flavor = "current_thread")]
async fn rendezvous_binds_both_peers_and_cancels_waiters() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut f = Fixture::new().await.unwrap();
            let completion = f.completion.get_root_as_reader().unwrap();
            let wait = f.recipient.complete_third_party(completion);
            assert_eq!(f.handles[0].stats().waiters, 0); // This is the recipient-local registry.
            drop(wait);
            // The host authenticates each request on its own connection. Send an
            // Accept from the wrong peer: it must not claim the recipient's value.
            let bytes = f
                .completion
                .get_root_as_reader::<capnp::data::Reader>()
                .unwrap()
                .to_vec();
            send(&mut *f.intro, |m| {
                let mut a = m.init_accept();
                a.set_question_id(2);
                a.get_provision()
                    .set_as::<capnp::data::Owned>(&bytes[..])
                    .unwrap();
            });
            wait_stats(&f.handles[0], |s| s.waiters == 1).await;
            f.publish();
            wait_stats(&f.handles[0], |s| s.provisions == 1).await;
            assert_eq!(f.handles[0].stats().waiters, 1);
            send(&mut *f.intro, |m| {
                let mut finish = m.init_finish();
                finish.set_question_id(2);
                finish.set_release_result_caps(true);
            });
            wait_stats(&f.handles[0], |s| s.waiters == 0).await;
            send(&mut *f.recipient, |m| {
                let mut a = m.init_accept();
                a.set_question_id(3);
                a.get_provision()
                    .set_as::<capnp::data::Owned>(&bytes[..])
                    .unwrap();
            });
            let accepted = f
                .recipient
                .receive_incoming_message()
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(
                accepted
                    .get_body()
                    .unwrap()
                    .get_as::<capnp_rpc::rpc_capnp::message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap(),
                capnp_rpc::rpc_capnp::message::Return(_)
            ));
            f.finish();
            wait_stats(&f.handles[0], |s| s.retired == 1).await;
            assert_eq!(f.handles[0].stats().provisions, 0);
            // Unknown remote vats must fail, never be confused with the local vat.
            assert!(f.networks[0]
                .connect([0xff; 32])
                .unwrap()
                .receive_incoming_message()
                .await
                .is_err());
            assert!(f.networks[0].connect(f.ids[1].public_key()).is_none());
            Ok::<(), capnp::Error>(())
        })
        .await
        .unwrap();
}

#[derive(serde::Deserialize)]
struct Trace {
    kind: String,
    #[serde(default)]
    server: bool,
    #[serde(default)]
    wrong_origin: bool,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<usize>,
}
async fn returned_cap(
    route: &mut dyn Connection<VatId>,
    question: u32,
    ok: bool,
) -> capnp::Result<Option<u32>> {
    let message = route
        .receive_incoming_message()
        .await?
        .expect("connection remains open");
    let msg = message
        .get_body()?
        .get_as::<capnp_rpc::rpc_capnp::message::Reader>()?;
    let capnp_rpc::rpc_capnp::message::Return(r) = msg.which()? else {
        panic!("expected return")
    };
    let r = r?;
    assert_eq!(r.get_answer_id(), question);
    match r.which()? {
        capnp_rpc::rpc_capnp::return_::Results(p) if ok => {
            let capnp_rpc::rpc_capnp::cap_descriptor::SenderHosted(id) =
                p?.get_cap_table()?.get(0).which()?
            else {
                panic!("expected hosted capability")
            };
            Ok(Some(id))
        }
        capnp_rpc::rpc_capnp::return_::Exception(_) if !ok => Ok(None),
        _ => panic!("incorrect accept result"),
    }
}
async fn replay_rendezvous(trace: &Trace) -> capnp::Result<()> {
    let mut f = Fixture::new().await?;
    let bytes = f
        .completion
        .get_root_as_reader::<capnp::data::Reader>()?
        .to_vec();
    let mut old_good = 0;
    let mut cap = None;
    for step in &trace.steps {
        match step.action.as_str() {
            "provide" => f.publish(),
            "good" | "bad" => {
                let bad = step.action == "bad";
                let mut token = bytes.clone();
                if bad && trace.wrong_origin {
                    token[..32].copy_from_slice(&f.ids[2].public_key());
                }
                let route = if bad && !trace.wrong_origin {
                    &mut f.intro
                } else {
                    &mut f.recipient
                };
                send(&mut **route, |m| {
                    let mut a = m.init_accept();
                    a.set_question_id(if bad { 12 } else { 11 });
                    a.get_provision()
                        .set_as::<capnp::data::Owned>(&token[..])
                        .unwrap();
                });
            }
            "cancel-good" | "cancel-bad" => {
                let bad = step.action == "cancel-bad";
                let route = if bad && !trace.wrong_origin {
                    &mut f.intro
                } else {
                    &mut f.recipient
                };
                send(&mut **route, |m| {
                    let mut a = m.init_finish();
                    a.set_question_id(if bad { 12 } else { 11 });
                    a.set_release_result_caps(true);
                });
                // Consume cancellation returns so subsequent reads see the
                // specific Accept/Call result they are checking.
                let msg = route.receive_incoming_message().await?.unwrap();
                let msg = msg
                    .get_body()?
                    .get_as::<capnp_rpc::rpc_capnp::message::Reader>()?;
                let capnp_rpc::rpc_capnp::message::Return(r) = msg.which()? else {
                    panic!()
                };
                assert_eq!(r?.get_answer_id(), if bad { 12 } else { 11 });
            }
            "finish" => f.finish(),
            _ => panic!("unknown rendezvous action"),
        }
        let state = &step.state;
        if state[1] != old_good && (state[1] == 2 || state[1] == 4) {
            cap = returned_cap(&mut *f.recipient, 11, state[1] == 2).await?;
        }
        old_good = state[1];
        wait_stats(&f.handles[0], |s| {
            s.provisions == usize::from(state[0] == 1)
                && s.retired == usize::from(state[0] == 2)
                && s.waiters == usize::from(state[1] == 1) + usize::from(state[2] == 1)
        })
        .await;
    }
    if let Some(cap) = cap {
        use capnp::traits::HasTypeId;
        send(&mut *f.recipient, |m| {
            let mut call = m.init_call();
            call.set_question_id(20);
            call.reborrow().init_target().set_imported_cap(cap);
            call.set_interface_id(harness::Client::TYPE_ID);
            call.set_method_id(0);
            call.init_params()
                .get_content()
                .init_as::<harness::echo_params::Builder>()
                .set_value(77);
        });
        let message = f.recipient.receive_incoming_message().await?.unwrap();
        let msg = message
            .get_body()?
            .get_as::<capnp_rpc::rpc_capnp::message::Reader>()?;
        let capnp_rpc::rpc_capnp::message::Return(r) = msg.which()? else {
            panic!()
        };
        let r = r?;
        assert_eq!(r.get_answer_id(), 20);
        let capnp_rpc::rpc_capnp::return_::Results(p) = r.which()? else {
            panic!()
        };
        assert_eq!(
            p?.get_content()
                .get_as::<harness::value::Reader>()?
                .get_value(),
            77
        );
    }
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_native_multiparty_traces() {
    let path = reproto_test_support::verification::input("REPROTO_NATIVE_MULTIPARTY_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                if trace.kind == "gate" {
                    use reproto::semantics::{NativeStreamGate, NativeStreamState, StreamRole};
                    let mut gate = NativeStreamGate::new(if trace.server {
                        StreamRole::Responder
                    } else {
                        StreamRole::Initiator
                    });
                    let mut published = false;
                    assert!(!gate.ready());
                    assert!(!gate.sent());
                    assert!(!gate.receive(b'R'));
                    for step in trace.steps {
                        match step.action.as_str() {
                            "authenticate" => gate.authenticate(),
                            "open" => assert!(if trace.server {
                                gate.receive(b'R')
                            } else {
                                gate.sent()
                            }),
                            "bad" => assert!(!gate.receive(b'X')),
                            "publish" => {
                                assert!(gate.ready());
                                published = true;
                            }
                            _ => panic!(),
                        }
                        let (a, o, f) = match gate.state() {
                            NativeStreamState::Handshaking(_) => (false, false, false),
                            NativeStreamState::SendPreface | NativeStreamState::ReceivePreface => {
                                (true, false, false)
                            }
                            NativeStreamState::Ready => (true, true, false),
                            NativeStreamState::Failed => (true, false, true),
                        };
                        assert_eq!(
                            [
                                usize::from(a),
                                usize::from(o),
                                usize::from(f),
                                usize::from(published)
                            ],
                            step.state[..4]
                        );
                    }
                } else {
                    tokio::time::timeout(Duration::from_secs(5), replay_rendezvous(&trace))
                        .await
                        .unwrap()
                        .unwrap();
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_native_identity_never_produces_an_authenticated_session() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Identity::generate();
            let b = Identity::generate();
            let wrong = Identity::generate();
            let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let addr = right.local_addr().unwrap();
            let (client, server) = tokio::join!(
                tokio::time::timeout(
                    Duration::from_millis(150),
                    transport::connect_authenticated(
                        left,
                        addr,
                        &a,
                        b.public_key(),
                        None,
                        b"rejection"
                    )
                ),
                tokio::time::timeout(
                    Duration::from_millis(150),
                    transport::accept_authenticated(
                        right,
                        &b,
                        wrong.public_key(),
                        None,
                        b"rejection"
                    )
                )
            );
            assert!(!matches!(client, Ok(Ok(_))));
            assert!(!matches!(server, Ok(Ok(_))));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn network_accept_cancellation_and_session_identity_checks() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Identity::generate();
            let b = Identity::generate();
            let (mut an, ah) = Network::new(a.public_key());
            let (_bn, bh) = Network::new(b.public_key());
            drop(an.accept());
            pair(&a, &b, &ah, &bh).await.unwrap();
            let accepted = an.accept().await.unwrap();
            assert_eq!(accepted.get_peer_vat_id(), b.public_key());
            assert_eq!(
                accepted.connection_id(),
                an.connect(b.public_key()).unwrap().connection_id()
            );
            let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let addr = right.local_addr().unwrap();
            let (aa, bb) = tokio::join!(
                transport::connect_authenticated(left, addr, &a, b.public_key(), None, b"identity"),
                transport::accept_authenticated(right, &b, a.public_key(), None, b"identity")
            );
            let (_wrong, handle) = Network::new([0; 32]);
            assert!(handle.attach(aa.unwrap()).is_err());
            assert!(
                bh.attach(bb.unwrap()).is_err(),
                "duplicate live route accepted"
            );
            let mut token = capnp::message::Builder::new_default();
            token
                .set_root::<capnp::data::Owned>(&[0u8; 64][..])
                .unwrap();
            let waiter = an
                .connect(b.public_key())
                .unwrap()
                .complete_third_party(token.get_root_as_reader().unwrap());
            assert_eq!(ah.stats().waiters, 1);
            ah.disconnect(b.public_key());
            assert!(an
                .connect(b.public_key())
                .unwrap()
                .receive_incoming_message()
                .await
                .is_err());
            drop(an);
            assert!(waiter.await.is_err());
            assert_eq!(ah.stats().waiters, 0);
        })
        .await;
}

// The fixture's directory provisions a dedicated host listener when requested.
// Production connectors provide their own listener/discovery policy.
struct HandoffConnector {
    ids: Rc<[Identity; 3]>,
    host: Handle,
    dials: Rc<Cell<usize>>,
}
impl Connector for HandoffConnector {
    fn connect(
        &self,
        peer: VatId,
    ) -> capnp::capability::Promise<transport::AuthenticatedSession, capnp::Error> {
        assert_eq!(peer, self.ids[0].public_key());
        self.dials.set(self.dials.get() + 1);
        let ids = self.ids.clone();
        let host = self.host.clone();
        capnp::capability::Promise::from_future(async move {
            let (client, server) = sessions(&ids[2], &ids[0]).await;
            host.attach(server)?;
            Ok(client)
        })
    }
}
async fn sessions(
    a: &Identity,
    b: &Identity,
) -> (
    transport::AuthenticatedSession,
    transport::AuthenticatedSession,
) {
    let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = right.local_addr().unwrap();
    let (aa, bb) = tokio::join!(
        transport::connect_authenticated(left, addr, a, b.public_key(), None, b"route-lifecycle"),
        transport::accept_authenticated(right, b, a.public_key(), None, b"route-lifecycle")
    );
    (aa.unwrap(), bb.unwrap())
}
type SessionResult = capnp::Result<transport::AuthenticatedSession>;
#[derive(Default)]
struct ControlledConnector {
    attempts: Cell<usize>,
    requests: RefCell<std::collections::VecDeque<futures::channel::oneshot::Sender<SessionResult>>>,
}
impl Connector for ControlledConnector {
    fn connect(
        &self,
        _: VatId,
    ) -> capnp::capability::Promise<transport::AuthenticatedSession, capnp::Error> {
        self.attempts.set(self.attempts.get() + 1);
        let (tx, rx) = futures::channel::oneshot::channel();
        self.requests.borrow_mut().push_back(tx);
        capnp::capability::Promise::from_future(async move {
            rx.await
                .map_err(|_| capnp::Error::disconnected("cancelled dial".into()))?
        })
    }
}
async fn settled() {
    // Poll both the framing task and deferred connector without relying on time.
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}
async fn route_status(h: &Handle, peer: VatId, status: Option<RouteStatus>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while h.route_status(peer) != status {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_dial_cannot_replace_a_new_route() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Identity::generate();
            let b = Identity::generate();
            let dialer = Rc::new(ControlledConnector::default());
            let (mut network, handle) = Network::with_connector(a.public_key(), dialer.clone());
            let old = network.connect(b.public_key()).unwrap();
            settled().await;
            let old_reply = dialer.requests.borrow_mut().pop_front().unwrap();
            handle.disconnect(b.public_key());
            let mut fresh = network.connect(b.public_key()).unwrap();
            assert_ne!(old.connection_id(), fresh.connection_id());
            settled().await;
            assert!(old_reply.is_canceled());
            let (client, server) = sessions(&a, &b).await;
            let (mut remote, remote_handle) = Network::new(b.public_key());
            remote_handle.attach(server).unwrap();
            assert!(dialer
                .requests
                .borrow_mut()
                .pop_front()
                .unwrap()
                .send(Ok(client))
                .is_ok());
            route_status(&handle, b.public_key(), Some(RouteStatus::Authenticated)).await;
            drop(old);
            assert_eq!(
                network.connect(b.public_key()).unwrap().connection_id(),
                fresh.connection_id()
            );
            let mut peer = remote.accept().await.unwrap();
            send(&mut *fresh, |m| m.init_bootstrap().set_question_id(99));
            assert!(peer.receive_incoming_message().await.unwrap().is_some());
            assert_eq!(dialer.attempts.get(), 2);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_generation_recovers_for_new_connections_without_replaying_old_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for incoming in [false, true] {
                let a = Identity::generate();
                let b = Identity::generate();
                let dialer = Rc::new(ControlledConnector::default());
                let (mut network, handle) = Network::with_options(
                    a.public_key(),
                    Some(dialer.clone()),
                    Options {
                        recover_failed_routes: true,
                        ..Options::default()
                    },
                )
                .unwrap();
                let mut old = network.connect(b.public_key()).unwrap();
                let observer = handle.observe_route(b.public_key()).unwrap();
                send(&mut *old, |m| m.init_bootstrap().set_question_id(1));
                settled().await;
                assert!(dialer
                    .requests
                    .borrow_mut()
                    .pop_front()
                    .unwrap()
                    .send(Err(capnp::Error::disconnected("unreachable".into())))
                    .is_ok());
                route_status(&handle, b.public_key(), Some(RouteStatus::Failed)).await;
                let (client, server) = sessions(&a, &b).await;
                let (mut remote, remote_handle) = Network::new(b.public_key());
                remote_handle.attach(server).unwrap();
                let mut fresh = if incoming {
                    handle.attach(client).unwrap();
                    network.accept().await.unwrap()
                } else {
                    let fresh = network.connect(b.public_key()).unwrap();
                    assert_eq!(
                        network.connect(b.public_key()).unwrap().connection_id(),
                        fresh.connection_id()
                    );
                    settled().await;
                    assert!(dialer
                        .requests
                        .borrow_mut()
                        .pop_front()
                        .unwrap()
                        .send(Ok(client))
                        .is_ok());
                    fresh
                };
                route_status(&handle, b.public_key(), Some(RouteStatus::Authenticated)).await;
                let generation = handle.observe_route(b.public_key()).unwrap().generation();
                assert!(generation > observer.generation());
                assert_eq!(observer.status(), RouteStatus::Failed);
                assert_ne!(old.connection_id(), fresh.connection_id());
                assert!(matches!(
                    old.receive_incoming_message().await,
                    Ok(None) | Err(_)
                ));
                drop(old);
                assert_eq!(
                    handle.observe_route(b.public_key()).unwrap().generation(),
                    generation
                );
                let mut peer = remote.accept().await.unwrap();
                send(&mut *fresh, |m| m.init_bootstrap().set_question_id(2));
                let received = peer.receive_incoming_message().await.unwrap().unwrap();
                let capnp_rpc::rpc_capnp::message::Bootstrap(b) = received
                    .get_body()
                    .unwrap()
                    .get_as::<capnp_rpc::rpc_capnp::message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("expected bootstrap")
                };
                assert_eq!(
                    b.unwrap().get_question_id(),
                    2,
                    "old queued calls must not be replayed"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn retrying_connector_shares_pending_route_and_sends_queued_rpc_once() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Identity::generate();
            let b = Identity::generate();
            let dialer = Rc::new(ControlledConnector::default());
            let connector = Rc::new(
                RetryingConnector::new(
                    dialer.clone(),
                    RetryPolicy {
                        initial_backoff: Duration::from_millis(1),
                        ..RetryPolicy::default()
                    },
                )
                .unwrap(),
            );
            let (mut network, handle) = Network::with_connector(a.public_key(), connector);
            let mut route = network.connect(b.public_key()).unwrap();
            assert_eq!(
                network.connect(b.public_key()).unwrap().connection_id(),
                route.connection_id()
            );
            send(&mut *route, |m| m.init_bootstrap().set_question_id(99));
            settled().await;
            assert!(dialer
                .requests
                .borrow_mut()
                .pop_front()
                .unwrap()
                .send(Err(capnp::Error::disconnected("temporary failure".into())))
                .is_ok());
            tokio::time::timeout(Duration::from_secs(1), async {
                while dialer.attempts.get() != 2 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let (client, server) = sessions(&a, &b).await;
            let (mut remote, remote_handle) = Network::new(b.public_key());
            remote_handle.attach(server).unwrap();
            assert!(dialer
                .requests
                .borrow_mut()
                .pop_front()
                .unwrap()
                .send(Ok(client))
                .is_ok());
            route_status(&handle, b.public_key(), Some(RouteStatus::Authenticated)).await;
            let mut peer = remote.accept().await.unwrap();
            assert!(peer.receive_incoming_message().await.unwrap().is_some());
            assert!(tokio::time::timeout(
                Duration::from_millis(10),
                peer.receive_incoming_message()
            )
            .await
            .is_err());
            assert_eq!(dialer.attempts.get(), 2);
        })
        .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn network_deadline_bounds_entire_retry_sequence() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for in_backoff in [false, true] {
                let dialer = Rc::new(ControlledConnector::default());
                let connector = Rc::new(
                    RetryingConnector::new(dialer.clone(), RetryPolicy::default()).unwrap(),
                );
                let (mut network, handle) = Network::with_options(
                    [1; 32],
                    Some(connector),
                    Options {
                        connect_timeout: Duration::from_millis(50),
                        ..Options::default()
                    },
                )
                .unwrap();
                let _route = network.connect([2; 32]).unwrap();
                settled().await;
                if in_backoff {
                    assert!(dialer
                        .requests
                        .borrow_mut()
                        .pop_front()
                        .unwrap()
                        .send(Err(capnp::Error::disconnected("retry".into())))
                        .is_ok());
                    settled().await;
                }
                tokio::time::advance(Duration::from_secs(3)).await;
                settled().await;
                assert_eq!(handle.route_status([2; 32]), Some(RouteStatus::Failed));
                assert_eq!(dialer.attempts.get(), 1);
                assert!(dialer
                    .requests
                    .borrow()
                    .iter()
                    .all(|request| request.is_canceled()));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_failed_route_recovery() {
    use reproto_test_support::verification::exploration;
    let config = include_str!("../verification/RpcRouteRecovery.cfg");
    let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
        + "\nPROPERTY SetupSettles\n";
    exploration::controls(
        "verification/RpcRouteRecovery.tla",
        "route-recovery",
        config,
        &[
            ("reuseGeneration", "DistinctGeneration"),
            ("replaceCause", "FirstCause"),
            ("earlyPublish", "Authenticated"),
            ("dropCurrent", "FreshSurvives"),
        ],
        Some(&live),
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/RpcRouteRecovery.tla",
        "route-recovery",
        config,
    )
    .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                let a = Identity::generate();
                let b = Identity::generate();
                let dialer = Rc::new(ControlledConnector::default());
                let (network, handle) = Network::with_options(
                    a.public_key(),
                    Some(dialer.clone()),
                    Options {
                        recover_failed_routes: true,
                        connect_timeout: Duration::from_secs(30),
                        ..Options::default()
                    },
                )
                .unwrap();
                let mut network = Some(network);
                let mut current = None;
                let mut old = None;
                let mut old_observer = None;
                let mut remote = None;
                let mut remote_route = None;
                for state in &trace {
                    match state["event"] {
                        1 => {
                            current = network.as_mut().unwrap().connect(b.public_key());
                        }
                        2 => {
                            assert!(dialer
                                .requests
                                .borrow_mut()
                                .pop_back()
                                .unwrap()
                                .send(Err(capnp::Error::disconnected("unreachable".into())))
                                .is_ok());
                            route_status(&handle, b.public_key(), Some(RouteStatus::Failed)).await;
                        }
                        3 => {
                            tokio::time::pause();
                            tokio::time::advance(Duration::from_secs(31)).await;
                            settled().await;
                            tokio::time::resume();
                            route_status(&handle, b.public_key(), Some(RouteStatus::Failed)).await;
                        }
                        4 | 5 => {
                            old_observer = handle.observe_route(b.public_key());
                            old = current.take();
                            if state["event"] == 4 {
                                current = network.as_mut().unwrap().connect(b.public_key());
                            } else {
                                let (client, server) = sessions(&a, &b).await;
                                let (mut peer, h) = Network::new(b.public_key());
                                h.attach(server).unwrap();
                                remote_route = Some(peer.accept().await.unwrap());
                                remote = Some(peer);
                                handle.attach(client).unwrap();
                                current = Some(network.as_mut().unwrap().accept().await.unwrap());
                            }
                        }
                        6 => {
                            let (client, server) = sessions(&a, &b).await;
                            let (mut peer, h) = Network::new(b.public_key());
                            h.attach(server).unwrap();
                            remote_route = Some(peer.accept().await.unwrap());
                            remote = Some(peer);
                            assert!(dialer
                                .requests
                                .borrow_mut()
                                .pop_back()
                                .unwrap()
                                .send(Ok(client))
                                .is_ok());
                            route_status(&handle, b.public_key(), Some(RouteStatus::Authenticated))
                                .await;
                        }
                        7 => {
                            drop(old.take());
                        }
                        8 => {
                            assert_eq!(
                                network
                                    .as_mut()
                                    .unwrap()
                                    .connect(b.public_key())
                                    .unwrap()
                                    .connection_id(),
                                current.as_ref().unwrap().connection_id()
                            );
                        }
                        9 => {
                            drop(network.take());
                        }
                        _ => panic!("unknown action"),
                    }
                    settled().await;
                    let expected = match state["phase"] {
                        1 => Some(RouteStatus::Connecting),
                        2 => Some(RouteStatus::Failed),
                        3 => Some(RouteStatus::Authenticated),
                        4 => None,
                        _ => panic!(),
                    };
                    assert_eq!(handle.route_status(b.public_key()), expected, "{trace:?}");
                    if let Some(observer) = handle.observe_route(b.public_key()) {
                        assert_eq!(
                            observer.generation().get(),
                            state["generation"],
                            "{trace:?}"
                        );
                    }
                    if let Some(observer) = &old_observer {
                        assert_eq!(observer.status(), RouteStatus::Failed);
                        let Some(reproto::native_rpc::Termination::Failed(cause)) =
                            observer.termination()
                        else {
                            panic!()
                        };
                        assert_eq!(
                            cause.kind,
                            if state["oldCause"] == 1 {
                                reproto::native_rpc::FailureKind::Connect
                            } else {
                                reproto::native_rpc::FailureKind::Timeout
                            },
                            "{trace:?}"
                        );
                    }
                }
                drop((current, old, remote_route, remote));
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn deferred_native_route_rejects_wrong_session_identities() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Identity::generate();
            let b = Identity::generate();
            let wrong = Identity::generate();
            for wrong_local in [false, true] {
                let dialer = Rc::new(ControlledConnector::default());
                let local = if wrong_local {
                    wrong.public_key()
                } else {
                    a.public_key()
                };
                let peer = if wrong_local {
                    b.public_key()
                } else {
                    wrong.public_key()
                };
                let (mut network, handle) = Network::with_connector(local, dialer.clone());
                let mut route = network.connect(peer).unwrap();
                send(&mut *route, |m| m.init_bootstrap().set_question_id(42));
                settled().await;
                let (client, server) = sessions(&a, &b).await;
                let (mut remote, remote_handle) = Network::new(b.public_key());
                remote_handle.attach(server).unwrap();
                let mut remote = remote.accept().await.unwrap();
                let receive = remote.receive_incoming_message();
                assert!(dialer
                    .requests
                    .borrow_mut()
                    .pop_front()
                    .unwrap()
                    .send(Ok(client))
                    .is_ok());
                route_status(&handle, peer, Some(RouteStatus::Failed)).await;
                assert!(matches!(
                    route.receive_incoming_message().await,
                    Ok(None) | Err(_)
                ));
                assert!(!matches!(
                    tokio::time::timeout(Duration::from_millis(20), receive).await,
                    Ok(Ok(Some(_)))
                ));
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn deferred_native_routes_are_bounded_and_cancelled_on_shutdown() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dialer = Rc::new(ControlledConnector::default());
            let (mut network, handle) = Network::with_connector([255; 32], dialer.clone());
            let routes: Vec<_> = (0..64u8)
                .map(|id| network.connect([id; 32]).unwrap())
                .collect();
            let mut rejected = network.connect([64; 32]).unwrap();
            assert!(rejected.receive_incoming_message().await.is_err());
            settled().await;
            assert_eq!(dialer.attempts.get(), 64);
            drop(network);
            settled().await;
            assert!(dialer.requests.borrow().iter().all(|s| s.is_canceled()));
            assert_eq!(handle.route_status([0; 32]), None);
            drop(routes);
        })
        .await;
}

#[derive(serde::Deserialize)]
struct RouteTrace {
    steps: Vec<Step>,
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_native_route_traces() {
    let path = reproto_test_support::verification::input("REPROTO_NATIVE_ROUTE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<RouteTrace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                tokio::time::timeout(Duration::from_secs(5), replay_route(trace))
                    .await
                    .unwrap();
            }
        })
        .await;
}
async fn replay_route(trace: RouteTrace) {
    let a = Identity::generate();
    let b = Identity::generate();
    let dialer = Rc::new(ControlledConnector::default());
    let (network, handle) = Network::with_connector(a.public_key(), dialer.clone());
    let mut network = Some(network);
    let mut routes: Vec<Box<dyn Connection<VatId>>> = Vec::new();
    let mut pending = None;
    let mut delivered = false;
    let mut valid = false;
    let (mut remote, remote_handle) = Network::new(b.public_key());
    let mut incoming = None;
    let mut _peer = None;
    for step in trace.steps {
        match step.action.as_str() {
            "request" => {
                let mut route = network.as_mut().unwrap().connect(b.public_key()).unwrap();
                send(&mut *route, |m| m.init_bootstrap().set_question_id(12));
                routes.push(route);
                let (client, server) = sessions(&a, &b).await;
                pending = Some(client);
                remote_handle.attach(server).unwrap();
                let mut peer = remote.accept().await.unwrap();
                let mut receive = peer.receive_incoming_message();
                // A fully authenticated socket exists but has not been returned
                // by the connector. Queued RPC frames must remain local.
                assert!(tokio::time::timeout(Duration::from_millis(1), &mut receive)
                    .await
                    .is_err());
                _peer = Some(peer);
                incoming = Some(receive);
            }
            "share" => {
                let route = network.as_mut().unwrap().connect(b.public_key()).unwrap();
                assert_eq!(route.connection_id(), routes[0].connection_id());
                routes.push(route);
            }
            "authenticate" => {
                assert!(dialer
                    .requests
                    .borrow_mut()
                    .pop_front()
                    .unwrap()
                    .send(Ok(pending.take().unwrap()))
                    .is_ok());
                valid = true;
                route_status(&handle, b.public_key(), Some(RouteStatus::Authenticated)).await;
            }
            "reject" => {
                let wrong = Identity::generate();
                let (client, _server) = sessions(&a, &wrong).await;
                assert!(dialer
                    .requests
                    .borrow_mut()
                    .pop_front()
                    .unwrap()
                    .send(Ok(client))
                    .is_ok());
                pending.take();
                route_status(&handle, b.public_key(), Some(RouteStatus::Failed)).await;
            }
            "bad-key" => {
                // Invalid material is rejected before a connector can own a
                // transport configuration or publish an authenticated session.
                assert!(Identity::from_keypair([0; 32], [0; 32]).is_err());
                assert!(dialer
                    .requests
                    .borrow_mut()
                    .pop_front()
                    .unwrap()
                    .send(Err(capnp::Error::failed("inconsistent identity".into())))
                    .is_ok());
                pending.take();
                route_status(&handle, b.public_key(), Some(RouteStatus::Failed)).await;
            }
            "fail" => {
                assert!(dialer
                    .requests
                    .borrow_mut()
                    .pop_front()
                    .unwrap()
                    .send(Err(capnp::Error::disconnected("dial failed".into())))
                    .is_ok());
                pending.take();
                route_status(&handle, b.public_key(), Some(RouteStatus::Failed)).await;
            }
            "release" => {
                routes.pop().unwrap();
            }
            "disconnect" => {
                handle.disconnect(b.public_key());
                routes.clear();
            }
            "close" => {
                network.take();
                routes.clear();
            }
            "deliver" => {
                let message = incoming.take().unwrap().await.unwrap().unwrap();
                let body = message
                    .get_body()
                    .unwrap()
                    .get_as::<capnp_rpc::rpc_capnp::message::Reader>()
                    .unwrap();
                let capnp_rpc::rpc_capnp::message::Bootstrap(bootstrap) = body.which().unwrap()
                else {
                    panic!()
                };
                assert_eq!(bootstrap.unwrap().get_question_id(), 12);
                delivered = true;
            }
            _ => panic!("unknown action"),
        }
        settled().await;
        let phase = match handle.route_status(b.public_key()) {
            None => 0,
            Some(RouteStatus::Connecting) => 1,
            Some(RouteStatus::Authenticated) => 2,
            Some(RouteStatus::Failed) => 3,
            Some(RouteStatus::Draining) => panic!("unexpected shutdown"),
            Some(RouteStatus::Stopped) => panic!("stopped route left in registry"),
        };
        assert_eq!(
            [
                phase,
                routes.len(),
                dialer.attempts.get(),
                usize::from(network.is_none()),
                usize::from(valid),
                usize::from(delivered)
            ],
            step.state[..6],
            "{}",
            step.action
        );
        if phase == 0 {
            assert!(dialer.requests.borrow().iter().all(|s| s.is_canceled()));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn directory_connector_pins_and_authorizes_endpoints() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let a = Rc::new(Identity::generate());
                let b = Identity::generate();
                let listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let mut directory =
                    DirectoryConnector::new(a.clone(), "127.0.0.1:0".parse().unwrap());
                assert!(directory
                    .insert(a.public_key(), address, None, b"directory")
                    .is_err());
                assert!(directory
                    .insert(
                        b.public_key(),
                        "0.0.0.0:10".parse().unwrap(),
                        None,
                        b"directory"
                    )
                    .is_err());
                directory
                    .insert(b.public_key(), address, Some([17; 32]), b"directory")
                    .unwrap();
                assert!(directory.connect([0; 32]).await.is_err());
                let (client, server) = tokio::join!(
                    directory.connect(b.public_key()),
                    transport::accept_authenticated(
                        listener,
                        &b,
                        a.public_key(),
                        Some([17; 32]),
                        b"directory"
                    )
                );
                let (mut local, lh) = Network::new(a.public_key());
                let (mut remote, rh) = Network::new(b.public_key());
                lh.attach(client.unwrap()).unwrap();
                rh.attach(server.unwrap()).unwrap();
                let mut route = local.accept().await.unwrap();
                let mut peer = remote.accept().await.unwrap();
                send(&mut *route, |m| m.init_bootstrap().set_question_id(3));
                assert!(peer.receive_incoming_message().await.unwrap().is_some());
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn deferred_route_timeout_cancels_connector() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dialer = Rc::new(ControlledConnector::default());
            let (mut network, handle) = Network::with_connector([1; 32], dialer.clone());
            let mut route = network.connect([2; 32]).unwrap();
            settled().await;
            tokio::time::timeout(Duration::from_secs(12), async {
                while handle.route_status([2; 32]) != Some(RouteStatus::Failed) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(dialer.requests.borrow().front().unwrap().is_canceled());
            assert!(matches!(
                route.receive_incoming_message().await,
                Ok(None) | Err(_)
            ));
        })
        .await;
}

#[test]
fn imported_identity_must_match_its_native_private_key() {
    let a = Identity::from_private_key([7; 32]).unwrap();
    let b = Identity::from_private_key([9; 32]).unwrap();
    assert_eq!(
        Identity::from_keypair([7; 32], b.public_key()).unwrap_err(),
        transport::IdentityError::KeyMismatch
    );
    let imported = Identity::from_keypair([7; 32], a.public_key()).unwrap();
    assert!(transport::config(&imported, b.public_key(), None, b"key-pair").is_ok());
}

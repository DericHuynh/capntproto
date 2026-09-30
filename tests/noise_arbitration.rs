use capnp::{capability::Promise, Error};
use capnp_rpc::VatNetwork;
use reproto::{
    noise_arbitration::Limits,
    noise_listener::{self, Listener},
    noise_provisioning::{Provisioner, ProvisioningConnector},
    noise_provisioning_capnp::provisioner,
    noise_rpc::{Connector, Handle, Network, RouteStatus},
    transport::{AuthenticatedSession, Identity},
};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    rc::Rc,
    time::Duration,
};

fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
struct Service;
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
        r.get().set_cap(p.get()?.get_cap()?);
        Ok(())
    }
}
async fn echo(cap: &harness::Client, value: u32) {
    let mut request = cap.echo_request();
    request.get().set_value(value);
    assert_eq!(
        request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        value
    );
}
struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}
#[derive(Default)]
struct Gate {
    open: Cell<bool>,
    changed: tokio::sync::Notify,
}
impl Gate {
    fn release(&self) {
        self.open.set(true);
        self.changed.notify_waiters();
    }
    async fn wait(&self) {
        while !self.open.get() {
            let wait = self.changed.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if !self.open.get() {
                wait.await;
            }
        }
    }
}
struct Queued {
    session: RefCell<Option<AuthenticatedSession>>,
    gate: Rc<Gate>,
    calls: Cell<usize>,
}
impl Connector for Queued {
    fn connect(&self, _: [u8; 32]) -> Promise<AuthenticatedSession, Error> {
        self.calls.set(self.calls.get() + 1);
        let session = self.session.borrow_mut().take();
        let gate = self.gate.clone();
        Promise::from_future(async move {
            gate.wait().await;
            session.ok_or_else(|| Error::disconnected("test dial failed".into()))
        })
    }
}
async fn pair(
    listener: &Listener,
    client: &Identity,
) -> (AuthenticatedSession, AuthenticatedSession) {
    let reservation = listener
        .reserve(client.public_key(), Some([7; 32]), b"arbitration")
        .unwrap();
    let socket = tokio::net::UdpSocket::bind(local()).await.unwrap();
    let (a, b) = tokio::join!(
        noise_listener::connect(
            socket,
            reservation.target(),
            client,
            Some([7; 32]),
            b"arbitration"
        ),
        reservation.accept()
    );
    (a.unwrap(), b.unwrap())
}
async fn listener(id: Rc<Identity>) -> Listener {
    Listener::bind(local(), id, noise_listener::Limits::default())
        .await
        .unwrap()
}
fn network(id: [u8; 32], connector: Option<Rc<dyn Connector>>) -> (Network, Handle) {
    Network::with_arbitration(
        id,
        connector,
        Limits {
            timeout: Duration::from_secs(2),
            ..Limits::default()
        },
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn crossed_dials_share_one_rpc_connection_in_both_arrival_orders() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(12), async {
                for order in 0..3 {
                    let mut ids = [Rc::new(Identity::generate()), Rc::new(Identity::generate())];
                    ids.sort_by_key(|id| id.public_key());
                    let al = listener(ids[0].clone()).await;
                    let bl = listener(ids[1].clone()).await;
                    let ((ao, bi), (bo, ai)) = tokio::join!(pair(&bl, &ids[0]), pair(&al, &ids[1]));
                    let ac = Rc::new(Queued {
                        session: RefCell::new(Some(ao)),
                        gate: Rc::default(),
                        calls: Cell::new(0),
                    });
                    let bc = Rc::new(Queued {
                        session: RefCell::new(Some(bo)),
                        gate: Rc::default(),
                        calls: Cell::new(0),
                    });
                    let (mut an, ah) = network(ids[0].public_key(), Some(ac.clone()));
                    let (mut bn, bh) = network(ids[1].public_key(), Some(bc.clone()));
                    let ar = an.connect(ids[1].public_key()).unwrap();
                    let br = bn.connect(ids[0].public_key()).unwrap();
                    let aid = ar.connection_id();
                    let bid = br.connection_id();
                    until(|| ac.calls.get() == 1 && bc.calls.get() == 1).await;
                    assert_eq!(
                        ah.route_status(ids[1].public_key()),
                        Some(RouteStatus::Connecting)
                    );
                    assert_eq!(
                        bh.route_status(ids[0].public_key()),
                        Some(RouteStatus::Connecting)
                    );
                    ah.attach(ai).unwrap();
                    bh.attach(bi).unwrap();
                    if order != 1 {
                        ac.gate.release();
                    }
                    if order != 0 {
                        bc.gate.release();
                    }
                    let ab: harness::Client = capnp_rpc::new_client(Service);
                    let bb: harness::Client = capnp_rpc::new_client(Service);
                    let mut a = capnp_rpc::RpcSystem::new(Box::new(an), Some(ab.client));
                    let mut b = capnp_rpc::RpcSystem::new(Box::new(bn), Some(bb.client));
                    let to_b: harness::Client = a.bootstrap(ids[1].public_key());
                    let to_a: harness::Client = b.bootstrap(ids[0].public_key());
                    let _tasks = Tasks(vec![
                        tokio::task::spawn_local(a),
                        tokio::task::spawn_local(b),
                    ]);
                    // Calls are queued before arbitration, preserving both RPC tables.
                    tokio::join!(echo(&to_b, 11), echo(&to_a, 12));
                    assert_eq!(
                        ah.selected_session(ids[1].public_key()),
                        bh.selected_session(ids[0].public_key())
                    );
                    assert!(ah.selected_session(ids[1].public_key()).is_some());
                    let ap = ah.take_datagrams(ids[1].public_key()).unwrap();
                    let mut bp = bh.take_datagrams(ids[0].public_key()).unwrap();
                    assert!(ah.take_datagrams(ids[1].public_key()).is_none());
                    ap.sender().try_send(b"selected datagram").unwrap();
                    assert_eq!(bp.recv().await.unwrap(), b"selected datagram");
                    assert_eq!(ar.connection_id(), aid);
                    assert_eq!(br.connection_id(), bid);
                    ac.gate.release();
                    bc.gate.release();
                    until(|| al.stats().authenticated + bl.stats().authenticated == 1).await;
                    assert_eq!(ac.calls.get(), 1);
                    assert_eq!(bc.calls.get(), 1);
                    let mut bounce = to_a.bounce_request();
                    bounce.get().set_cap(to_b.clone());
                    let returned = bounce
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_cap()
                        .unwrap();
                    echo(&returned, 13).await;
                    // A later candidate cannot replace established capability tables.
                    let (extra, incoming) = pair(&al, &ids[1]).await;
                    assert!(ah.attach(incoming).is_err());
                    drop(extra);
                    echo(&to_b, 14).await;
                    ah.disconnect(ids[1].public_key());
                    bh.disconnect(ids[0].public_key());
                    assert!(to_a.echo_request().send().promise.await.is_err());
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn incoming_candidate_recovers_a_failed_outgoing_dial_and_reconnects_fresh() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let a = Rc::new(Identity::generate());
                let b = Rc::new(Identity::generate());
                let al = listener(a.clone()).await;
                let gate = Rc::new(Gate::default());
                gate.release();
                let failed = Rc::new(Queued {
                    session: RefCell::new(None),
                    gate,
                    calls: Cell::new(0),
                });
                let (mut an, ah) = network(a.public_key(), Some(failed));
                let (mut bn, bh) = network(b.public_key(), None);
                let mut old = None;
                for _ in 0..2 {
                    let ar = an.connect(b.public_key()).unwrap();
                    let (outgoing, incoming) = pair(&al, &b).await;
                    ah.attach(incoming).unwrap();
                    bh.attach(outgoing).unwrap();
                    let br = bn.connect(a.public_key()).unwrap();
                    until(|| {
                        ah.selected_session(b.public_key()).is_some()
                            && bh.selected_session(a.public_key()).is_some()
                    })
                    .await;
                    let selected = ah.selected_session(b.public_key()).unwrap();
                    assert_eq!(Some(selected), bh.selected_session(a.public_key()));
                    assert_ne!(Some(selected), old);
                    old = Some(selected);
                    assert_eq!(
                        an.connect(b.public_key()).unwrap().connection_id(),
                        ar.connection_id()
                    );
                    ah.disconnect(b.public_key());
                    bh.disconnect(a.public_key());
                    drop(ar);
                    drop(br);
                    until(|| al.stats().authenticated == 0).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn missing_profile_and_candidate_quota_fail_without_rpc_publication() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Rc::new(Identity::generate());
            let b = Rc::new(Identity::generate());
            let al = listener(a.clone()).await;
            let (_an, ah) = Network::with_arbitration(
                a.public_key(),
                None,
                Limits {
                    candidates: 1,
                    timeout: Duration::from_millis(150),
                },
            )
            .unwrap();
            let (outgoing, incoming) = pair(&al, &b).await;
            ah.attach(incoming).unwrap();
            let (extra, incoming) = pair(&al, &b).await;
            assert!(ah.attach(incoming).is_err());
            drop(extra);
            // A legacy native RPC peer does not speak the arbitration preface.
            let (bn, bh) = Network::new(b.public_key());
            bh.attach(outgoing).unwrap();
            let service: harness::Client = capnp_rpc::new_client(Service);
            let _tasks = Tasks(vec![tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                Box::new(bn),
                Some(service.client),
            ))]);
            until(|| ah.route_status(b.public_key()) == Some(RouteStatus::Failed)).await;
            assert!(ah.selected_session(b.public_key()).is_none());
            until(|| al.stats().authenticated == 0).await;
            assert!(Network::with_arbitration(
                a.public_key(),
                None,
                Limits {
                    candidates: 0,
                    ..Limits::default()
                }
            )
            .is_err());
            assert!(Network::with_arbitration(
                a.public_key(),
                None,
                Limits {
                    timeout: Duration::from_secs(11),
                    ..Limits::default()
                }
            )
            .is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn crossed_remote_provisioning_dials_do_not_deadlock_on_lease_readiness() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(6), async {
                let a = Rc::new(Identity::generate());
                let b = Rc::new(Identity::generate());
                let al = listener(a.clone()).await;
                let bl = listener(b.clone()).await;
                let ac = Rc::new(ProvisioningConnector::new(a.clone(), local()));
                let bc = Rc::new(ProvisioningConnector::new(b.clone(), local()));
                let (mut an, ah) = network(a.public_key(), Some(ac.clone()));
                let (mut bn, bh) = network(b.public_key(), Some(bc.clone()));
                let ag = Provisioner::new(
                    al.clone(),
                    ah.clone(),
                    b.public_key(),
                    al.local_addr().unwrap(),
                    b"cross",
                )
                .unwrap();
                let bg = Provisioner::new(
                    bl.clone(),
                    bh.clone(),
                    a.public_key(),
                    bl.local_addr().unwrap(),
                    b"cross",
                )
                .unwrap();
                // In-memory RPC control channels exercise delivery/lease semantics.
                // The native candidates below use actual Noise IKpsk2 UDP sessions.
                let (x, y) = tokio::io::duplex(4096);
                let at = reproto::rpc::serve(y, ag.client().client);
                let (ap, acontrol): (provisioner::Client, _) = reproto::rpc::client(x);
                let (x, y) = tokio::io::duplex(4096);
                let bt = reproto::rpc::serve(y, bg.client().client);
                let (bp, bcontrol): (provisioner::Client, _) = reproto::rpc::client(x);
                let controls = Tasks(vec![at, acontrol, bt, bcontrol]);
                ac.insert(b.public_key(), bp, bl.local_addr().unwrap(), b"cross")
                    .unwrap();
                bc.insert(a.public_key(), ap, al.local_addr().unwrap(), b"cross")
                    .unwrap();
                let ar = an.connect(b.public_key()).unwrap();
                let br = bn.connect(a.public_key()).unwrap();
                assert_eq!(
                    ah.route_status(b.public_key()),
                    Some(RouteStatus::Connecting)
                );
                assert_eq!(
                    bh.route_status(a.public_key()),
                    Some(RouteStatus::Connecting)
                );
                let ab: harness::Client = capnp_rpc::new_client(Service);
                let bb: harness::Client = capnp_rpc::new_client(Service);
                let mut an = capnp_rpc::RpcSystem::new(Box::new(an), Some(ab.client));
                let mut bn = capnp_rpc::RpcSystem::new(Box::new(bn), Some(bb.client));
                let to_b = an.bootstrap(b.public_key());
                let to_a = bn.bootstrap(a.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(an),
                    tokio::task::spawn_local(bn),
                ]);
                tokio::join!(echo(&to_b, 21), echo(&to_a, 22));
                assert_eq!(al.stats().issued, 1);
                assert_eq!(bl.stats().issued, 1);
                assert_eq!(
                    ah.selected_session(b.public_key()),
                    bh.selected_session(a.public_key())
                );
                until(|| al.stats().authenticated + bl.stats().authenticated == 1).await;
                ag.close();
                bg.close();
                drop(controls);
                tokio::join!(echo(&to_b, 23), echo(&to_a, 24));
                drop(ar);
                drop(br);
            })
            .await
            .unwrap();
        })
        .await;
}

struct PendingDial(Rc<Cell<usize>>, Rc<Cell<usize>>);
struct Live(Rc<Cell<usize>>);
impl Drop for Live {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
impl Connector for PendingDial {
    fn connect(&self, _: [u8; 32]) -> Promise<AuthenticatedSession, Error> {
        self.0.set(self.0.get() + 1);
        self.1.set(self.1.get() + 1);
        let guard = Live(self.0.clone());
        Promise::from_future(async move {
            let _guard = guard;
            futures::future::pending().await
        })
    }
}
#[tokio::test(flavor = "current_thread")]
async fn pending_generation_cancellation_timeout_and_route_quota_are_bounded() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let live = Rc::new(Cell::new(0));
            let calls = Rc::new(Cell::new(0));
            let (mut network, handle) = network(
                [9; 32],
                Some(Rc::new(PendingDial(live.clone(), calls.clone()))),
            );
            let peer = [8; 32];
            let a = network.connect(peer).unwrap();
            let b = network.connect(peer).unwrap();
            assert_eq!(a.connection_id(), b.connection_id());
            until(|| live.get() == 1).await;
            drop(a);
            assert_eq!(live.get(), 1);
            drop(b);
            until(|| live.get() == 0).await;
            let old = network.connect(peer).unwrap();
            until(|| live.get() == 1).await;
            handle.disconnect(peer);
            until(|| live.get() == 0).await;
            let fresh = network.connect(peer).unwrap();
            until(|| live.get() == 1).await;
            assert_ne!(old.connection_id(), fresh.connection_id());
            drop(old);
            tokio::task::yield_now().await;
            assert_eq!(live.get(), 1);
            drop(network);
            until(|| live.get() == 0).await;
            drop(fresh);
            let (mut network, handle) = Network::with_arbitration(
                [9; 32],
                Some(Rc::new(PendingDial(live.clone(), calls.clone()))),
                Limits {
                    timeout: Duration::from_millis(50),
                    ..Limits::default()
                },
            )
            .unwrap();
            let mut route = network.connect(peer).unwrap();
            until(|| handle.route_status(peer) == Some(RouteStatus::Failed)).await;
            assert_eq!(live.get(), 0);
            assert!(matches!(
                route.receive_incoming_message().await,
                Ok(None) | Err(_)
            ));
            drop(network);
            drop(route);
            let (mut network, _) = Network::with_arbitration(
                [9; 32],
                Some(Rc::new(PendingDial(live.clone(), calls.clone()))),
                Limits::default(),
            )
            .unwrap();
            let mut routes = Vec::new();
            for i in 0..64 {
                let mut id = [0; 32];
                id[0] = i;
                routes.push(network.connect(id).unwrap());
            }
            until(|| live.get() == 64).await;
            let mut extra = network.connect([8; 32]).unwrap();
            assert!(extra.receive_incoming_message().await.is_err());
            assert_eq!(live.get(), 64);
            drop(network);
            until(|| live.get() == 0).await;
            drop(routes);
            let previous = calls.get();
            let (mut network, handle) = Network::with_arbitration(
                [9; 32],
                Some(Rc::new(PendingDial(live.clone(), calls.clone()))),
                Limits {
                    timeout: Duration::from_millis(5),
                    ..Limits::default()
                },
            )
            .unwrap();
            let route = network.connect(peer).unwrap();
            // Deliberately stall this executor before its first task poll.
            // Allocation time, not the first poll, starts the route deadline.
            std::thread::sleep(Duration::from_millis(15));
            until(|| handle.route_status(peer) == Some(RouteStatus::Failed)).await;
            assert_eq!(calls.get(), previous);
            assert_eq!(live.get(), 0);
            drop(route);
        })
        .await;
}

use capnp_rpc::VatNetwork;
use reproto::{
    noise_arbitration,
    noise_listener::{self, Listener},
    noise_rpc::{Connector, Handle, Network, RouteStatus, Termination},
    transport::{self, AuthenticatedSession, Identity},
};
use reproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc, time::Duration};

async fn socket() -> tokio::net::UdpSocket {
    tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap()
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
async fn pair(
    listener: &Listener,
    client: &Identity,
) -> (AuthenticatedSession, AuthenticatedSession) {
    let reservation = listener
        .reserve(client.public_key(), Some([7; 32]), b"shutdown")
        .unwrap();
    let (a, b) = tokio::join!(
        noise_listener::connect(
            socket().await,
            reservation.target(),
            client,
            Some([7; 32]),
            b"shutdown"
        ),
        reservation.accept()
    );
    (a.unwrap(), b.unwrap())
}
async fn listener(id: Rc<Identity>) -> Listener {
    Listener::bind("127.0.0.1:0".parse().unwrap(), id, Default::default())
        .await
        .unwrap()
}
fn network(
    id: [u8; 32],
    arbitrated: bool,
    connector: Option<Rc<dyn Connector>>,
) -> (Network, Handle) {
    if arbitrated {
        Network::with_arbitration(id, connector, noise_arbitration::Limits::default()).unwrap()
    } else if let Some(connector) = connector {
        Network::with_connector(id, connector)
    } else {
        Network::new(id)
    }
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
}
struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}
async fn echo(c: &harness::Client) {
    let mut r = c.echo_request();
    r.get().set_value(42);
    assert_eq!(
        r.send().promise.await.unwrap().get().unwrap().get_value(),
        42
    );
}
struct Once(RefCell<Option<AuthenticatedSession>>);
impl Connector for Once {
    fn connect(
        &self,
        _: [u8; 32],
    ) -> capnp::capability::Promise<AuthenticatedSession, capnp::Error> {
        match self.0.borrow_mut().take() {
            Some(session) => capnp::capability::Promise::ok(session),
            None => {
                capnp::capability::Promise::err(capnp::Error::disconnected("no session".into()))
            }
        }
    }
}
// The RPC reader classifies envelopes before returning them. Keep the large
// payload/backpressure fixture inside a valid Call envelope.
fn data_message(
    connection: &mut dyn capnp_rpc::Connection<[u8; 32]>,
    value: u8,
    bytes: u32,
) -> Box<dyn capnp_rpc::OutgoingMessage> {
    let mut message = connection.new_outgoing_message(4096);
    let mut call = message
        .get_body()
        .unwrap()
        .init_as::<capnp_rpc::rpc_capnp::message::Builder>()
        .init_call();
    call.set_question_id(u32::from(value));
    call.reborrow().init_target().set_imported_cap(0);
    call.init_params()
        .init_content()
        .initn_as::<capnp::data::Builder>(bytes)
        .fill(value);
    message
}
#[tokio::test(flavor = "current_thread")]
async fn dedicated_and_shared_sessions_acknowledge_empty_drains() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Rc::new(Identity::generate());
            let b = Rc::new(Identity::generate());
            let sa = socket().await;
            let sb = socket().await;
            let address = sb.local_addr().unwrap();
            let (ca, cb) = tokio::join!(
                transport::connect_authenticated(sa, address, &a, b.public_key(), None, b"close"),
                transport::accept_authenticated(sb, &b, a.public_key(), None, b"close")
            );
            let (ca, cb) = (ca.unwrap(), cb.unwrap());
            assert_eq!(ca.shutdown(Duration::from_secs(2)).await.unwrap().bytes, 0);
            drop(cb);
            let bl = listener(b.clone()).await;
            for server in [false, true] {
                let (ca, cb) = pair(&bl, &a).await;
                let (first, other) = if server { (cb, ca) } else { (ca, cb) };
                assert_eq!(
                    first.shutdown(Duration::from_secs(2)).await.unwrap().bytes,
                    0
                );
                drop(other);
                until(|| bl.stats().authenticated == 0).await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn rpc_shutdown_drains_direct_connector_and_arbitrated_routes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for arbitrated in [false, true] {
                for outgoing in [false, true] {
                    let a = Rc::new(Identity::generate());
                    let b = Rc::new(Identity::generate());
                    let bl = listener(b.clone()).await;
                    let (mut ca, cb) = pair(&bl, &a).await;
                    let datagrams = ca.take_datagrams().unwrap();
                    let sender = datagrams.sender();
                    let mut ca = Some(ca);
                    let connector = outgoing
                        .then(|| Rc::new(Once(RefCell::new(ca.take()))) as Rc<dyn Connector>);
                    let (an, ah) = network(a.public_key(), arbitrated, connector);
                    let (bn, bh) = network(b.public_key(), arbitrated, None);
                    if let Some(ca) = ca {
                        ah.attach(ca).unwrap();
                    }
                    bh.attach(cb).unwrap();
                    let service: harness::Client = capnp_rpc::new_client(Service);
                    let mut a_rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
                    let b_rpc = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
                    let client: harness::Client = a_rpc.bootstrap(b.public_key());
                    let _tasks = Tasks(vec![
                        tokio::task::spawn_local(a_rpc),
                        tokio::task::spawn_local(b_rpc),
                    ]);
                    echo(&client).await;
                    let observer = ah.observe_route(b.public_key()).unwrap();
                    let receipt = ah
                        .shutdown(b.public_key(), Duration::from_secs(2))
                        .await
                        .unwrap();
                    assert_eq!(
                        observer.termination(),
                        Some(Termination::ShutdownAcknowledged)
                    );
                    assert!(receipt.bytes > 0);
                    assert_eq!(ah.route_status(b.public_key()), None);
                    assert!(sender.try_send(b"closed").is_err());
                    assert!(client.echo_request().send().promise.await.is_err());
                    until(|| bl.stats().authenticated == 0).await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn listener_admission_stops_before_all_routes_are_drained() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let a = Rc::new(Identity::generate());
            let b = Rc::new(Identity::generate());
            let bl = listener(b.clone()).await;
            let (ca, cb) = pair(&bl, &a).await;
            let pending = bl.reserve(a.public_key(), None, b"pending").unwrap();
            bl.stop_accepting();
            bl.stop_accepting();
            assert!(!bl.stats().accepting);
            assert!(!bl.stats().closed);
            assert_eq!(bl.stats().pending, 0);
            assert_eq!(bl.stats().authenticated, 1);
            assert!(bl.reserve(a.public_key(), None, b"new").is_err());
            assert!(pending.accept().await.is_err());
            let (an, ah) = network(a.public_key(), true, None);
            let (bn, bh) = network(b.public_key(), true, None);
            ah.attach(ca).unwrap();
            bh.attach(cb).unwrap();
            let service: harness::Client = capnp_rpc::new_client(Service);
            let mut a_rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
            let b_rpc = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
            let client: harness::Client = a_rpc.bootstrap(b.public_key());
            let _tasks = Tasks(vec![
                tokio::task::spawn_local(a_rpc),
                tokio::task::spawn_local(b_rpc),
            ]);
            echo(&client).await;
            let receipts = bh.shutdown_all(Duration::from_secs(2)).await.unwrap();
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0].0, a.public_key());
            assert!(receipts[0].1.is_ok());
            bl.close();
            assert!(bl.stats().closed);
            assert_eq!(bl.stats().authenticated, 0);
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn canceled_shutdown_cannot_remove_replacement_route() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for resume in [false, true] {
                let a = Rc::new(Identity::generate());
                let b = Rc::new(Identity::generate());
                let bl = listener(b.clone()).await;
                let (ca, cb) = pair(&bl, &a).await;
                let (mut an, ah) = Network::new(a.public_key());
                let (mut bn, bh) = Network::new(b.public_key());
                ah.attach(ca).unwrap();
                bh.attach(cb).unwrap();
                let old = an.connect(b.public_key()).unwrap();
                let observer = ah.observe_route(b.public_key()).unwrap();
                let _other = bn.accept().await.unwrap();
                assert!(ah.shutdown(b.public_key(), Duration::ZERO).await.is_err());
                assert_eq!(
                    ah.route_status(b.public_key()),
                    Some(RouteStatus::Authenticated)
                );
                let wait = ah.shutdown(b.public_key(), Duration::from_secs(2));
                assert_eq!(ah.route_status(b.public_key()), Some(RouteStatus::Draining));
                assert!(ah
                    .shutdown(b.public_key(), Duration::from_secs(2))
                    .await
                    .is_err());
                ah.disconnect(b.public_key());
                bh.disconnect(a.public_key());
                let (ca, cb) = pair(&bl, &a).await;
                ah.attach(ca).unwrap();
                bh.attach(cb).unwrap();
                let mut fresh = an.connect(b.public_key()).unwrap();
                if resume {
                    assert_eq!(wait.await.unwrap_err().extra, "Noise route canceled");
                } else {
                    drop(wait);
                }
                assert_eq!(observer.termination(), Some(Termination::Canceled));
                assert_ne!(old.connection_id(), fresh.connection_id());
                assert_eq!(
                    ah.route_status(b.public_key()),
                    Some(RouteStatus::Authenticated)
                );
                // No RPC consumer: >64KiB exceeds the peer input bridge, so receipt must
                // time out instead of acknowledging undelivered bytes.
                for index in 0..20 {
                    let _ = data_message(&mut *fresh, index, 16384).send();
                }
                assert!(ah
                    .shutdown(b.public_key(), Duration::from_millis(80))
                    .await
                    .is_err());
                assert_eq!(ah.route_status(b.public_key()), None);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn queued_bytes_and_crossed_shutdowns_receive_real_peer_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for arbitrated in [false, true] {
                let a = Rc::new(Identity::generate());
                let b = Rc::new(Identity::generate());
                let bl = listener(b.clone()).await;
                let (ca, cb) = pair(&bl, &a).await;
                let (mut an, ah) = network(a.public_key(), arbitrated, None);
                let (mut bn, bh) = network(b.public_key(), arbitrated, None);
                ah.attach(ca).unwrap();
                bh.attach(cb).unwrap();
                let mut ac = an.connect(b.public_key()).unwrap();
                let mut bc = bn.accept().await.unwrap();
                until(|| {
                    ah.route_status(b.public_key()) == Some(RouteStatus::Authenticated)
                        && bh.route_status(a.public_key()) == Some(RouteStatus::Authenticated)
                })
                .await;
                for index in 0..24 {
                    let _ = data_message(&mut *ac, index, 16384).send();
                }
                let prebuilt = data_message(&mut *ac, 99, 8);
                let shutdown = ah.shutdown(b.public_key(), Duration::from_secs(2));
                let late_send = prebuilt.send().0;
                tokio::pin!(shutdown);
                assert!(futures::poll!(&mut shutdown).is_pending());
                let received = async {
                    for index in 0..24 {
                        let message = bc.receive_incoming_message().await.unwrap().unwrap();
                        let envelope = message
                            .get_body()
                            .unwrap()
                            .get_as::<capnp_rpc::rpc_capnp::message::Reader>()
                            .unwrap();
                        let capnp_rpc::rpc_capnp::message::Call(call) = envelope.which().unwrap()
                        else {
                            panic!("expected queued Call envelope");
                        };
                        let call = call.unwrap();
                        assert_eq!(call.get_question_id(), u32::from(index));
                        let bytes = call
                            .get_params()
                            .unwrap()
                            .get_content()
                            .get_as::<capnp::data::Reader>()
                            .unwrap();
                        assert_eq!(bytes, vec![index; 16384]);
                    }
                };
                let (receipt, ()) = tokio::join!(&mut shutdown, received);
                assert!(receipt.unwrap().bytes >= 24 * 16384);
                assert!(late_send.await.is_err());
            }
            // Start both fences before either writer runs; each peer must wait for
            // the reciprocal receipt even when separate control streams reorder.
            for arbitrated in [false, true] {
                for _ in 0..4 {
                    let a = Rc::new(Identity::generate());
                    let b = Rc::new(Identity::generate());
                    let bl = listener(b.clone()).await;
                    let (ca, cb) = pair(&bl, &a).await;
                    let (mut an, ah) = network(a.public_key(), arbitrated, None);
                    let (mut bn, bh) = network(b.public_key(), arbitrated, None);
                    ah.attach(ca).unwrap();
                    bh.attach(cb).unwrap();
                    let _ac = an.connect(b.public_key()).unwrap();
                    let _bc = bn.accept().await.unwrap();
                    until(|| {
                        ah.route_status(b.public_key()) == Some(RouteStatus::Authenticated)
                            && bh.route_status(a.public_key()) == Some(RouteStatus::Authenticated)
                    })
                    .await;
                    let aw = ah.shutdown(b.public_key(), Duration::from_secs(2));
                    let bw = bh.shutdown(a.public_key(), Duration::from_secs(2));
                    let (ar, br) = tokio::join!(aw, bw);
                    assert!(ar.is_ok(), "{ar:?}");
                    assert!(br.is_ok(), "{br:?}");
                    assert_eq!(ah.route_status(b.public_key()), None);
                    assert_eq!(bh.route_status(a.public_key()), None);
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_all_reports_and_releases_pending_routes_without_receipts() {
    struct Pending;
    impl Connector for Pending {
        fn connect(
            &self,
            _: [u8; 32],
        ) -> capnp::capability::Promise<AuthenticatedSession, capnp::Error> {
            capnp::capability::Promise::from_future(std::future::pending())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for arbitrated in [false, true] {
                let a = Identity::generate();
                let b = Identity::generate();
                let (mut network, handle) =
                    network(a.public_key(), arbitrated, Some(Rc::new(Pending)));
                let mut connection = network.connect(b.public_key()).unwrap();
                assert!(handle.shutdown_all(Duration::ZERO).await.is_err());
                assert_eq!(
                    handle.route_status(b.public_key()),
                    Some(RouteStatus::Connecting)
                );
                let results = handle.shutdown_all(Duration::from_secs(1)).await.unwrap();
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].0, b.public_key());
                assert!(results[0].1.is_err());
                assert_eq!(handle.route_status(b.public_key()), None);
                assert!(connection
                    .receive_incoming_message()
                    .await
                    .unwrap()
                    .is_none());
                let fresh = network.connect(b.public_key()).unwrap();
                assert_ne!(connection.connection_id(), fresh.connection_id());
                assert_eq!(handle.route_status(b.public_key()), None);
            }
        })
        .await;
}

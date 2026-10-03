use capntproto::{
    native_listener::{self, Limits, Listener},
    native_rpc::{Connector, DirectoryConnector, Network},
    transport::Identity,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc, time::Duration};
struct Service(Rc<RefCell<Option<harness::Client>>>);
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
        *self.0.borrow_mut() = Some(cap.clone());
        r.get().set_cap(cap);
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
async fn socket() -> tokio::net::UdpSocket {
    tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap()
}
async fn listener(id: Rc<Identity>, limits: Limits) -> Listener {
    Listener::bind("127.0.0.1:0".parse().unwrap(), id, limits)
        .await
        .unwrap()
}
async fn echo(client: &harness::Client, value: u32) {
    let mut req = client.echo_request();
    req.get().set_value(value);
    assert_eq!(
        req.send().promise.await.unwrap().get().unwrap().get_value(),
        value
    );
}
async fn until(mut p: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !p() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn concurrent_shared_socket_rpc_and_datagrams() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let host = Rc::new(Identity::generate());
                let a = Identity::generate();
                let b = Identity::generate();
                let listener = listener(host.clone(), Limits::default()).await;
                let ar = listener
                    .reserve(a.public_key(), Some([7; 32]), b"a")
                    .unwrap();
                let br = listener.reserve(b.public_key(), None, b"b").unwrap();
                let (at, bt) = (ar.target(), br.target());
                assert_eq!(at.address, bt.address);
                assert_ne!(at.connection_id, bt.connection_id);
                let (aa, ab, ba, bb) = tokio::join!(
                    native_listener::connect(socket().await, at, &a, Some([7; 32]), b"a"),
                    ar.accept(),
                    native_listener::connect(socket().await, bt, &b, None, b"b"),
                    br.accept()
                );
                let (mut aa, mut ab, ba, bb) = (aa.unwrap(), ab.unwrap(), ba.unwrap(), bb.unwrap());
                assert_eq!(listener.stats().authenticated, 2);
                let ap = aa.take_datagrams().unwrap();
                let mut hp = ab.take_datagrams().unwrap();
                let (hn, hh) = Network::new(host.public_key());
                let (an, ah) = Network::new(a.public_key());
                let (bn, bh) = Network::new(b.public_key());
                hh.attach(ab).unwrap();
                hh.attach(bb).unwrap();
                ah.attach(aa).unwrap();
                bh.attach(ba).unwrap();
                let service: harness::Client = capnp_rpc::new_client(Service(Rc::default()));
                let h = capnp_rpc::RpcSystem::new(Box::new(hn), Some(service.client));
                let mut a = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let ac = a.bootstrap(host.public_key());
                let mut b = capnp_rpc::RpcSystem::new(Box::new(bn), None);
                let bc = b.bootstrap(host.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(h),
                    tokio::task::spawn_local(a),
                    tokio::task::spawn_local(b),
                ]);
                tokio::join!(echo(&ac, 41), echo(&bc, 42));
                ap.sender().try_send(b"shared datagram").unwrap();
                assert_eq!(hp.recv().await.unwrap(), b"shared datagram");
                hh.disconnect(at.host); // Disconnecting an unrelated/local vat is harmless.
                echo(&bc, 43).await;
                listener.close();
                assert!(listener.stats().closed);
                assert_eq!(listener.stats().authenticated, 0);
                assert!(hp.recv().await.is_none());
                // Listener close tears down local state; peer detection still
                // follows RPC/transport failure or its idle timeout.
                ah.disconnect(host.public_key());
                bh.disconnect(host.public_key());
                assert!(ac.echo_request().send().promise.await.is_err());
                assert!(bc.echo_request().send().promise.await.is_err());
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn expiry_cancellation_and_directory_reprovisioning() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let host = Rc::new(Identity::generate());
            let client = Rc::new(Identity::generate());
            let listener = listener(
                host.clone(),
                Limits {
                    routes: 1,
                    queue: 2,
                    reservation_timeout: Duration::from_millis(500),
                },
            )
            .await;
            let old = listener.reserve(client.public_key(), None, b"x").unwrap();
            assert!(listener.reserve(client.public_key(), None, b"x").is_err());
            tokio::time::sleep(Duration::from_millis(550)).await;
            assert_eq!(listener.stats().pending, 0);
            let fresh = listener.reserve(client.public_key(), None, b"x").unwrap();
            assert_ne!(old.target().connection_id, fresh.target().connection_id);
            drop(old);
            assert_eq!(listener.stats().pending, 1);
            drop(fresh);
            assert_eq!(listener.stats().pending, 0);
            let slot = listener.reserve(client.public_key(), None, b"x").unwrap();
            let task = tokio::task::spawn_local(slot.accept());
            tokio::task::yield_now().await;
            task.abort();
            let _ = task.await;
            until(|| listener.stats().pending == 0).await;
            let directory = DirectoryConnector::new(client.clone(), "127.0.0.1:0".parse().unwrap());
            for _ in 0..2 {
                let reservation = listener
                    .reserve(client.public_key(), Some([8; 32]), b"directory")
                    .unwrap();
                directory
                    .insert_reserved(reservation.target(), Some([8; 32]), b"directory")
                    .unwrap();
                let first = directory.connect(host.public_key());
                assert!(directory.connect(host.public_key()).await.is_err());
                let (client_session, server_session) = tokio::join!(first, reservation.accept());
                let (client_session, server_session) =
                    (client_session.unwrap(), server_session.unwrap());
                // Authenticated sessions are no longer subject to reservation expiry.
                tokio::time::sleep(Duration::from_millis(550)).await;
                assert_eq!(listener.stats().authenticated, 1);
                drop(client_session);
                drop(server_session);
                until(|| listener.stats().authenticated == 0).await;
            }
            let slot = listener.reserve(client.public_key(), None, b"x").unwrap();
            drop(listener);
            assert!(slot.accept().await.is_err());
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn wrong_keys_psks_contexts_and_cross_reservation_replay_never_publish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for mismatch in 0..4 {
                let host = Rc::new(Identity::generate());
                let client = Identity::generate();
                let other = Identity::generate();
                let listener = listener(
                    host,
                    Limits {
                        reservation_timeout: Duration::from_millis(120),
                        ..Limits::default()
                    },
                )
                .await;
                let reservation = listener
                    .reserve(client.public_key(), Some([7; 32]), b"context")
                    .unwrap();
                let target = reservation.target();
                let input = socket().await;
                let proxy = socket().await;
                let mut dial_target = target;
                // Rewrite a captured initial's destination to another reservation.
                // The Native prologue still names the original reservation ID.
                let relay = if mismatch == 3 {
                    let old = listener
                        .reserve(client.public_key(), Some([7; 32]), b"context")
                        .unwrap();
                    dial_target = old.target();
                    dial_target.address = proxy.local_addr().unwrap();
                    let to = target.address;
                    let id = target.connection_id;
                    Some(tokio::task::spawn_local(async move {
                        let mut bytes = vec![0; 2048];
                        let (n, _) = proxy.recv_from(&mut bytes).await.unwrap();
                        assert_eq!(bytes[5], 16);
                        bytes[6..22].copy_from_slice(&id);
                        proxy.send_to(&bytes[..n], to).await.unwrap();
                        drop(old);
                    }))
                } else {
                    None
                };
                let dial = native_listener::connect(
                    input,
                    dial_target,
                    if mismatch == 0 { &other } else { &client },
                    Some(if mismatch == 1 { [9; 32] } else { [7; 32] }),
                    if mismatch == 2 { b"wrong" } else { b"context" },
                );
                let (dial, accepted) = tokio::join!(
                    tokio::time::timeout(Duration::from_millis(250), dial),
                    reservation.accept()
                );
                assert!(!matches!(dial, Ok(Ok(_))));
                assert!(accepted.is_err());
                assert_eq!(listener.stats().authenticated, 0);
                if let Some(relay) = relay {
                    relay.await.unwrap();
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn native_three_party_handoff_uses_reserved_shared_listener() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let host = Rc::new(Identity::generate());
                let intro = Rc::new(Identity::generate());
                let recipient = Rc::new(Identity::generate());
                let host_listener = listener(host.clone(), Limits::default()).await;
                let recipient_listener = listener(recipient.clone(), Limits::default()).await;
                let ih = host_listener
                    .reserve(intro.public_key(), Some([1; 32]), b"ih")
                    .unwrap();
                let rh = host_listener
                    .reserve(recipient.public_key(), Some([2; 32]), b"rh")
                    .unwrap();
                let ir = recipient_listener
                    .reserve(intro.public_key(), Some([3; 32]), b"ir")
                    .unwrap();
                let directory = Rc::new(DirectoryConnector::new(
                    recipient.clone(),
                    "127.0.0.1:0".parse().unwrap(),
                ));
                directory
                    .insert_reserved(rh.target(), Some([2; 32]), b"rh")
                    .unwrap();
                let (hn, hh) = Network::new(host.public_key());
                let (inetwork, ihandle) = Network::new(intro.public_key());
                let (rn, rhnd) = Network::with_connector(recipient.public_key(), directory);
                let (a, b) = tokio::join!(
                    native_listener::connect(
                        socket().await,
                        ih.target(),
                        &intro,
                        Some([1; 32]),
                        b"ih"
                    ),
                    ih.accept()
                );
                ihandle.attach(a.unwrap()).unwrap();
                hh.attach(b.unwrap()).unwrap();
                let (a, b) = tokio::join!(
                    native_listener::connect(
                        socket().await,
                        ir.target(),
                        &intro,
                        Some([3; 32]),
                        b"ir"
                    ),
                    ir.accept()
                );
                ihandle.attach(a.unwrap()).unwrap();
                rhnd.attach(b.unwrap()).unwrap();
                let retained = Rc::new(RefCell::new(None));
                let hboot: harness::Client = capnp_rpc::new_client(Service(Rc::default()));
                let rboot: harness::Client = capnp_rpc::new_client(Service(retained.clone()));
                let host_rpc = capnp_rpc::RpcSystem::new(Box::new(hn), Some(hboot.client));
                let mut intro_rpc = capnp_rpc::RpcSystem::new(Box::new(inetwork), None);
                let recipient_rpc = capnp_rpc::RpcSystem::new(Box::new(rn), Some(rboot.client));
                let owner: harness::Client = intro_rpc.bootstrap(host.public_key());
                let recipient_cap: harness::Client = intro_rpc.bootstrap(recipient.public_key());
                let install =
                    tokio::task::spawn_local(async move { hh.attach(rh.accept().await.unwrap()) });
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(host_rpc),
                    tokio::task::spawn_local(intro_rpc),
                    tokio::task::spawn_local(recipient_rpc),
                    install,
                ]);
                echo(&owner, 1).await;
                let mut request = recipient_cap.bounce_request();
                request.get().set_cap(owner);
                let returned = request
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                echo(&returned, 2).await;
                let direct = retained.borrow().as_ref().unwrap().clone();
                echo(&direct, 3).await;
                assert_eq!(host_listener.stats().authenticated, 2);
                ihandle.disconnect(host.public_key());
                ihandle.disconnect(recipient.public_key());
                echo(&direct, 4).await;
                retained.borrow_mut().take();
            })
            .await
            .unwrap();
        })
        .await;
}

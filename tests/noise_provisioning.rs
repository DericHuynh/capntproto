use capnp::capability::FromClientHook;
use reproto::{
    noise_listener::{Limits, Listener},
    noise_provisioning::{relay, Provisioner, ProvisioningConnector},
    noise_provisioning_capnp::provisioner,
    noise_rpc::{Connector, Handle, Network, RouteStatus},
    transport::{self, Identity},
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
struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
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
async fn listener(identity: Rc<Identity>) -> Listener {
    Listener::bind(local(), identity, Limits::default())
        .await
        .unwrap()
}
async fn pair(a: &Identity, b: &Identity, ah: &Handle, bh: &Handle) {
    let left = tokio::net::UdpSocket::bind(local()).await.unwrap();
    let right = tokio::net::UdpSocket::bind(local()).await.unwrap();
    let address = right.local_addr().unwrap();
    let (a, b) = tokio::join!(
        transport::connect_authenticated(left, address, a, b.public_key(), None, b"control"),
        transport::accept_authenticated(right, b, a.public_key(), None, b"control")
    );
    ah.attach(a.unwrap()).unwrap();
    bh.attach(b.unwrap()).unwrap();
}
#[derive(Default)]
struct Service {
    stored: RefCell<Option<harness::Client>>,
    provider: RefCell<Option<provisioner::Client>>,
}
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(params.get()?.get_value());
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        mut results: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = params.get()?.get_cap()?;
        *self.stored.borrow_mut() = Some(cap.clone());
        results.get().set_cap(cap);
        Ok(())
    }
    async fn opaque(
        self: Rc<Self>,
        _: harness::OpaqueParams,
        mut results: harness::OpaqueResults,
    ) -> capnp::Result<()> {
        results.get().init_value().set_as_capability(
            self.provider
                .borrow()
                .as_ref()
                .unwrap()
                .clone()
                .into_client_hook(),
        );
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
async fn provider(cap: &harness::Client) -> provisioner::Client {
    cap.opaque_request()
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_value()
        .get_as_capability()
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn native_handoff_provisions_through_introducer_and_survives_its_disconnect() {
    native_handoff(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn native_handoff_with_arbitrated_routes_and_provisioning_relay() {
    native_handoff(true).await;
}
async fn native_handoff(arbitration: bool) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let host = Rc::new(Identity::generate());
                let intro = Identity::generate();
                let recipient = Rc::new(Identity::generate());
                let listener = listener(host.clone()).await;
                let connector = Rc::new(ProvisioningConnector::new(recipient.clone(), local()));
                let make = |id, connector: Option<Rc<dyn Connector>>| {
                    if arbitration {
                        Network::with_arbitration(
                            id,
                            connector,
                            reproto::noise_arbitration::Limits::default(),
                        )
                        .unwrap()
                    } else if let Some(connector) = connector {
                        Network::with_connector(id, connector)
                    } else {
                        Network::new(id)
                    }
                };
                let (hn, hh) = make(host.public_key(), None);
                let (inetwork, ih) = make(intro.public_key(), None);
                let (rn, rh) = make(recipient.public_key(), Some(connector.clone()));
                pair(&intro, &host, &ih, &hh).await;
                pair(&intro, &recipient, &ih, &rh).await;
                let grant = Provisioner::new(
                    listener.clone(),
                    hh.clone(),
                    recipient.public_key(),
                    listener.local_addr().unwrap(),
                    b"introduced",
                )
                .unwrap();
                let hs = Rc::new(Service::default());
                *hs.provider.borrow_mut() = Some(grant.client());
                let is = Rc::new(Service::default());
                let rs = Rc::new(Service::default());
                let hb: harness::Client = capnp_rpc::new_client_from_rc(hs);
                let ib: harness::Client = capnp_rpc::new_client_from_rc(is.clone());
                let rb: harness::Client = capnp_rpc::new_client_from_rc(rs.clone());
                let h = capnp_rpc::RpcSystem::new(Box::new(hn), Some(hb.client));
                let mut i = capnp_rpc::RpcSystem::new(Box::new(inetwork), Some(ib.client));
                let mut r = capnp_rpc::RpcSystem::new(Box::new(rn), Some(rb.client));
                let owner: harness::Client = i.bootstrap(host.public_key());
                let recipient_cap: harness::Client = i.bootstrap(recipient.public_key());
                let introducer_cap: harness::Client = r.bootstrap(intro.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(h),
                    tokio::task::spawn_local(i),
                    tokio::task::spawn_local(r),
                ]);
                // Both the provisioner and its returned lease stay on the existing
                // H-I-R control hops. No R-H reservation exists at delegation time.
                *is.provider.borrow_mut() = Some(relay(provider(&owner).await));
                connector
                    .insert(
                        host.public_key(),
                        provider(&introducer_cap).await,
                        listener.local_addr().unwrap(),
                        b"introduced",
                    )
                    .unwrap();
                assert_eq!(listener.stats().issued, 0);
                assert!(rh.route_status(host.public_key()).is_none());
                let mut request = recipient_cap.bounce_request();
                request.get().set_cap(owner.clone());
                let bounced = request
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                echo(&bounced, 11).await;
                let direct = rs.stored.borrow().as_ref().unwrap().clone();
                echo(&direct, 12).await;
                assert_eq!(listener.stats().issued, 1);
                assert_eq!(listener.stats().authenticated, 1);
                assert_eq!(
                    rh.route_status(host.public_key()),
                    Some(RouteStatus::Authenticated)
                );
                grant.close(); // Revoking provisioning cannot revoke installed RPC.
                ih.disconnect(host.public_key());
                ih.disconnect(recipient.public_key());
                connector.remove(host.public_key());
                echo(&direct, 13).await;
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_lease_cancellation_expiry_drop_and_revocation_release_reservations() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let host = Rc::new(Identity::generate());
            let recipient = Identity::generate();
            let listener = Listener::bind(
                local(),
                host.clone(),
                Limits {
                    reservation_timeout: Duration::from_millis(150),
                    ..Limits::default()
                },
            )
            .await
            .unwrap();
            let (_network, handle) = Network::new(host.public_key());
            let grant = Provisioner::new(
                listener.clone(),
                handle,
                recipient.public_key(),
                listener.local_addr().unwrap(),
                b"lease",
            )
            .unwrap();
            // This in-memory RPC fixture checks capability release, not secrecy.
            let (a, b) = tokio::io::duplex(4096);
            let server = reproto::rpc::serve(b, grant.client().client);
            let (client, task): (provisioner::Client, _) = reproto::rpc::client(a);
            let _tasks = Tasks(vec![server, task]);
            let mut previous = None;
            for mode in 0..5 {
                let response = client.reserve_request().send().promise.await.unwrap();
                let lease = response.get().unwrap().get_lease().unwrap();
                let ticket = response.get().unwrap().get_ticket().unwrap();
                let id = ticket.get_connection_id().unwrap().to_vec();
                let secret = zeroize::Zeroizing::new(ticket.get_psk().unwrap().to_vec());
                if let Some((last_id, last_secret)) = previous.take() {
                    assert!(id != last_id);
                    assert!(secret != last_secret);
                }
                previous = Some((id, secret));
                assert_eq!(listener.stats().pending, 1);
                assert!(client.reserve_request().send().promise.await.is_err());
                drop(response);
                let ready = lease.ready_request().send().promise;
                match mode {
                    0 => {
                        lease.cancel_request().send().promise.await.unwrap();
                        lease.cancel_request().send().promise.await.unwrap();
                        assert!(ready.await.is_err());
                    }
                    1 => assert!(ready.await.is_err()), // Reservation deadline.
                    2 => {
                        drop(ready);
                        drop(lease);
                    }
                    3 => {
                        // Canceling just a waiter preserves the lease's authority.
                        drop(ready);
                        tokio::task::yield_now().await;
                        assert_eq!(listener.stats().pending, 1);
                        lease.cancel_request().send().promise.await.unwrap();
                    }
                    4 => {
                        grant.close();
                        assert!(ready.await.is_err());
                        assert!(client.reserve_request().send().promise.await.is_err());
                    }
                    _ => unreachable!(),
                }
                until(|| listener.stats().pending == 0).await;
            }
            assert_eq!(listener.stats().issued, 5);
            assert_eq!(listener.stats().authenticated, 0);
        })
        .await;
}

struct DelayedProvider {
    inner: provisioner::Client,
    issued: Rc<Cell<bool>>,
}
impl provisioner::Server for DelayedProvider {
    async fn reserve(
        self: Rc<Self>,
        _: provisioner::ReserveParams,
        mut results: provisioner::ReserveResults,
    ) -> capnp::Result<()> {
        let response = self.inner.reserve_request().send().promise.await?;
        results.get().set_ticket(response.get()?.get_ticket()?)?;
        results.get().set_lease(response.get()?.get_lease()?);
        self.issued.set(true);
        futures::future::pending().await
    }
}
#[tokio::test(flavor = "current_thread")]
async fn canceled_control_call_and_provider_owner_drop_abort_pending_accepts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let host = Rc::new(Identity::generate());
            let recipient = Rc::new(Identity::generate());
            let listener = listener(host.clone()).await;
            let (_network, handle) = Network::new(host.public_key());
            let grant = Provisioner::new(
                listener.clone(),
                handle,
                recipient.public_key(),
                listener.local_addr().unwrap(),
                b"cancel",
            )
            .unwrap();
            let issued = Rc::new(Cell::new(false));
            let delayed: provisioner::Client = capnp_rpc::new_client(DelayedProvider {
                inner: grant.client(),
                issued: issued.clone(),
            });
            let (a, b) = tokio::io::duplex(4096);
            let server = reproto::rpc::serve(b, delayed.client);
            let (client, task): (provisioner::Client, _) = reproto::rpc::client(a);
            let tasks = Tasks(vec![server, task]);
            let connector = ProvisioningConnector::new(recipient, local());
            connector
                .insert(
                    host.public_key(),
                    client,
                    listener.local_addr().unwrap(),
                    b"cancel",
                )
                .unwrap();
            let dial = tokio::task::spawn_local(connector.connect(host.public_key()));
            until(|| issued.get()).await;
            assert_eq!(listener.stats().pending, 1);
            dial.abort();
            assert!(matches!(dial.await, Err(error) if error.is_cancelled()));
            // Cleanup must arrive through RPC cancellation, ahead of the 10s lease deadline.
            until(|| listener.stats().pending == 0).await;
            drop(connector);
            drop(tasks);
            tokio::task::yield_now().await;
            let client = grant.client();
            let response = client.reserve_request().send().promise.await.unwrap();
            let lease = response.get().unwrap().get_lease().unwrap();
            drop(response);
            drop(client);
            assert_eq!(listener.stats().pending, 1);
            drop(grant);
            until(|| listener.stats().pending == 0).await;
            assert!(lease.ready_request().send().promise.await.is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn connector_reprovisions_after_disconnect_and_rejects_wrong_bindings() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let host = Rc::new(Identity::generate());
                let recipient = Rc::new(Identity::generate());
                let listener = listener(host.clone()).await;
                let (network, handle) = Network::new(host.public_key());
                let service: harness::Client = capnp_rpc::new_client(Service::default());
                let _tasks = Tasks(vec![tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                    Box::new(network),
                    Some(service.client),
                ))]);
                let grant = Provisioner::new(
                    listener.clone(),
                    handle.clone(),
                    recipient.public_key(),
                    listener.local_addr().unwrap(),
                    b"retry",
                )
                .unwrap();
                let connector = ProvisioningConnector::new(recipient.clone(), local());
                assert!(connector.connect(host.public_key()).await.is_err());
                for mode in 0..3 {
                    let (peer, address, context) = match mode {
                        0 => ([9; 32], listener.local_addr().unwrap(), b"retry".as_slice()),
                        1 => (
                            host.public_key(),
                            "127.0.0.1:1".parse().unwrap(),
                            b"retry".as_slice(),
                        ),
                        _ => (
                            host.public_key(),
                            listener.local_addr().unwrap(),
                            b"bad".as_slice(),
                        ),
                    };
                    connector
                        .insert(peer, grant.client(), address, context)
                        .unwrap();
                    assert!(connector.connect(peer).await.is_err());
                    until(|| listener.stats().pending == 0).await;
                }
                let wrong = ProvisioningConnector::new(Rc::new(Identity::generate()), local());
                wrong
                    .insert(
                        host.public_key(),
                        grant.client(),
                        listener.local_addr().unwrap(),
                        b"retry",
                    )
                    .unwrap();
                assert!(wrong.connect(host.public_key()).await.is_err());
                until(|| listener.stats().pending == 0).await;
                connector
                    .insert(
                        host.public_key(),
                        grant.client(),
                        listener.local_addr().unwrap(),
                        b"retry",
                    )
                    .unwrap();
                for _ in 0..2 {
                    let session = connector.connect(host.public_key()).await.unwrap();
                    assert_eq!(session.peer(), host.public_key());
                    assert_eq!(
                        handle.route_status(recipient.public_key()),
                        Some(RouteStatus::Authenticated)
                    );
                    assert!(connector.connect(host.public_key()).await.is_err());
                    handle.disconnect(recipient.public_key());
                    drop(session);
                    until(|| listener.stats().authenticated == 0).await;
                }
                assert_eq!(listener.stats().issued, 6);
                grant.close();
                assert!(connector.connect(host.public_key()).await.is_err());
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_providers_cannot_publish_two_routes_for_one_peer() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let host = Rc::new(Identity::generate());
                let recipient = Rc::new(Identity::generate());
                let listener = listener(host.clone()).await;
                let (network, handle) = Network::new(host.public_key());
                let service: harness::Client = capnp_rpc::new_client(Service::default());
                let _tasks = Tasks(vec![tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                    Box::new(network),
                    Some(service.client),
                ))]);
                let a = Provisioner::new(
                    listener.clone(),
                    handle.clone(),
                    recipient.public_key(),
                    listener.local_addr().unwrap(),
                    b"race",
                )
                .unwrap();
                let b = Provisioner::new(
                    listener.clone(),
                    handle.clone(),
                    recipient.public_key(),
                    listener.local_addr().unwrap(),
                    b"race",
                )
                .unwrap();
                let ac = ProvisioningConnector::new(recipient.clone(), local());
                let bc = ProvisioningConnector::new(recipient.clone(), local());
                ac.insert(
                    host.public_key(),
                    a.client(),
                    listener.local_addr().unwrap(),
                    b"race",
                )
                .unwrap();
                bc.insert(
                    host.public_key(),
                    b.client(),
                    listener.local_addr().unwrap(),
                    b"race",
                )
                .unwrap();
                let (x, y) =
                    tokio::join!(ac.connect(host.public_key()), bc.connect(host.public_key()));
                assert_ne!(x.is_ok(), y.is_ok());
                assert_eq!(
                    handle.route_status(recipient.public_key()),
                    Some(RouteStatus::Authenticated)
                );
                until(|| listener.stats().pending == 0 && listener.stats().authenticated == 1)
                    .await;
                handle.disconnect(recipient.public_key());
            })
            .await
            .unwrap();
        })
        .await;
}

use capnp::{capability::Promise, Error};
use capnp_rpc::{Connection, IncomingMessage, OutgoingMessage, RpcSystem, VatNetwork};
use futures::TryFutureExt;
use reproto::{
    native_rpc::{DirectoryConnector, Handle, Network},
    transport::{self, Identity},
};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
type Vat = [u8; 32];

// Disable ordinary introductions so the independent proxy paths survive until
// Join. All wire messages, shares, proofs and direct routing use production code.
struct Proxies(Network, bool);
struct Link(Box<dyn Connection<Vat>>, bool);
impl Connection<Vat> for Link {
    fn get_peer_vat_id(&self) -> Vat {
        self.0.get_peer_vat_id()
    }
    fn connection_id(&self) -> usize {
        self.0.connection_id()
    }
    fn new_outgoing_message(&mut self, n: u32) -> Box<dyn OutgoingMessage> {
        self.0.new_outgoing_message(n)
    }
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        self.0.receive_incoming_message()
    }
    fn shutdown(&mut self, r: capnp::Result<()>) -> Promise<(), Error> {
        self.0.shutdown(r)
    }
    fn set_idle(&mut self, idle: bool) {
        self.0.set_idle(idle);
    }
    fn supports_third_party(&self) -> bool {
        true
    }
    fn supports_multiparty_join(&self) -> bool {
        self.1
    }
    fn complete_third_party(
        &mut self,
        p: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<capnp_rpc::third_party::ThirdPartyExchange>, Error> {
        self.0.complete_third_party(p)
    }
}
impl VatNetwork<Vat> for Proxies {
    fn join_network(&self) -> Option<Rc<dyn capnp_rpc::multiparty::JoinNetwork<Vat>>> {
        self.0.join_network()
    }
    fn connect(&mut self, id: Vat) -> Option<Box<dyn Connection<Vat>>> {
        self.0
            .connect(id)
            .map(|c| Box::new(Link(c, self.1)) as Box<dyn Connection<Vat>>)
    }
    fn accept(&mut self) -> Promise<Box<dyn Connection<Vat>>, Error> {
        let supports_join = self.1;
        Promise::from_future(
            self.0
                .accept()
                .map_ok(move |c| Box::new(Link(c, supports_join)) as Box<dyn Connection<Vat>>),
        )
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        self.0.drive_until_shutdown()
    }
}
struct Echo(u32);
struct JoinBoundary {
    allowed: bool,
    joins: Cell<u32>,
    calls: Cell<u32>,
    blocked: Cell<bool>,
}
impl capnp_rpc::membrane::Policy for JoinBoundary {
    fn allow_join(
        &self,
        direction: capnp_rpc::membrane::Direction,
        targets: &[capnp::capability::Client],
    ) -> capnp::Result<bool> {
        assert_eq!(direction, capnp_rpc::membrane::Direction::Inbound);
        assert_eq!(targets.len(), 2);
        self.joins.set(self.joins.get() + 1);
        Ok(self.allowed)
    }
    fn call(
        &self,
        _: capnp_rpc::membrane::Direction,
        _: u64,
        _: u16,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        self.calls.set(self.calls.get() + 1);
        if self.blocked.get() {
            Err(Error::failed("joined Native capability blocked".into()))
        } else {
            Ok(None)
        }
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
struct Relay(RefCell<Option<harness::Client>>);
impl capnp_rpc::BootstrapFactory<Vat> for Relay {
    fn create_for(&self, _: &Vat) -> capnp::Result<capnp::capability::Client> {
        Ok(self.0.borrow().as_ref().unwrap().client.clone())
    }
}
struct Host {
    other: Vat,
    first: harness::Client,
    second: harness::Client,
}
impl capnp_rpc::BootstrapFactory<Vat> for Host {
    fn create_for(&self, peer: &Vat) -> capnp::Result<capnp::capability::Client> {
        Ok(if *peer == self.other {
            self.second.client.clone()
        } else {
            self.first.client.clone()
        })
    }
}
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
            Some([27; 32]),
            b"multiparty-join"
        ),
        transport::accept_authenticated(
            right,
            b,
            a.public_key(),
            Some([27; 32]),
            b"multiparty-join"
        )
    );
    ah.attach(a.unwrap()).unwrap();
    bh.attach(b.unwrap()).unwrap();
}
async fn drain() {
    for _ in 0..128 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn independent_paths_join_over_native_and_keep_the_direct_capability() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                for (parts, scenario) in [
                    (2, 0),
                    (3, 0),
                    (2, 1),
                    (2, 2),
                    (2, 3),
                    (2, 4),
                    (2, 5),
                    (2, 6),
                    (2, 7),
                ] {
                    let ids: Vec<_> = (0..5).map(|_| Rc::new(Identity::generate())).collect();
                    let direct = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                    let mut directory =
                        DirectoryConnector::new(ids[0].clone(), "127.0.0.1:0".parse().unwrap());
                    directory
                        .insert(
                            ids[3].public_key(),
                            direct.local_addr().unwrap(),
                            Some([27; 32]),
                            b"multiparty-join",
                        )
                        .unwrap();
                    let (a, ah) = Network::with_connector(ids[0].public_key(), Rc::new(directory));
                    let (b, bh) = Network::new(ids[1].public_key());
                    let (c, ch) = Network::new(ids[2].public_key());
                    let (d, dh) = Network::new(ids[3].public_key());
                    let (e, eh) = Network::new(ids[4].public_key());
                    let handles = [ah, bh, ch, dh, eh];
                    let second_host = if scenario == 2 { 4 } else { 3 };
                    for (i, j) in [(0, 1), (0, 2), (1, 3), (2, second_host)] {
                        pair(&ids[i], &ids[j], &handles[i], &handles[j]).await;
                    }
                    let host_id = ids[3].clone();
                    let caller_id = ids[0].public_key();
                    let dh = handles[3].clone();
                    let listener = tokio::task::spawn_local(async move {
                        if scenario == 3 {
                            futures::future::pending::<()>().await;
                        }
                        let session = transport::accept_authenticated(
                            direct,
                            &host_id,
                            caller_id,
                            Some([27; 32]),
                            b"multiparty-join",
                        )
                        .await
                        .unwrap();
                        dh.attach(session).unwrap();
                    });
                    let first: harness::Client = capnp_rpc::new_client(Echo(73));
                    let second = if scenario == 1 {
                        capnp_rpc::new_client(Echo(91))
                    } else {
                        first.clone()
                    };
                    let d = RpcSystem::new_with_bootstrap_factory(
                        Box::new(Proxies(d, true)),
                        ids[3].public_key(),
                        Rc::new(Host {
                            other: ids[2].public_key(),
                            first,
                            second,
                        }),
                    );
                    let other: harness::Client = capnp_rpc::new_client(Echo(91));
                    let e = RpcSystem::new(Box::new(Proxies(e, true)), Some(other.client));
                    let bf = Rc::new(Relay(RefCell::new(None)));
                    let cf = Rc::new(Relay(RefCell::new(None)));
                    let mut b = RpcSystem::new_with_bootstrap_factory(
                        Box::new(Proxies(b, scenario != 4)),
                        ids[1].public_key(),
                        bf.clone(),
                    );
                    let mut c = RpcSystem::new_with_bootstrap_factory(
                        Box::new(Proxies(c, true)),
                        ids[2].public_key(),
                        cf.clone(),
                    );
                    *bf.0.borrow_mut() = Some(b.bootstrap(ids[3].public_key()));
                    *cf.0.borrow_mut() = Some(c.bootstrap(ids[second_host].public_key()));
                    let mut a = RpcSystem::new(Box::new(Proxies(a, true)), None);
                    let joiner = a.get_joiner();
                    let x: harness::Client = a.bootstrap(ids[1].public_key());
                    let y: harness::Client = a.bootstrap(ids[2].public_key());
                    let tasks = [a, b, c, d, e].map(tokio::task::spawn_local);
                    let retained = x.clone();
                    let mut inputs = if parts == 3 {
                        vec![x.clone(), y, x]
                    } else {
                        vec![x, y]
                    };
                    let mut boundaries = Vec::new();
                    if scenario >= 5 {
                        for _ in 0..if scenario == 7 { 2 } else { 1 } {
                            let policy = Rc::new(JoinBoundary {
                                allowed: scenario != 6,
                                joins: Cell::new(0),
                                calls: Cell::new(0),
                                blocked: Cell::new(false),
                            });
                            let membrane = capnp_rpc::membrane::Membrane::new(policy.clone());
                            inputs = inputs.into_iter().map(|cap| membrane.export(cap)).collect();
                            boundaries.push((membrane, policy));
                        }
                    }
                    let original_brand = inputs[0].client.hook.get_brand();
                    let joined = if scenario == 3 {
                        let pending = tokio::task::spawn_local(joiner.join(inputs));
                        while handles[3].join_stats().contributed != 2 {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                        pending.abort();
                        None
                    } else {
                        let result = joiner.join(inputs).await;
                        if scenario == 4 || scenario == 6 {
                            assert_eq!(result.err().unwrap().kind, capnp::ErrorKind::Unimplemented);
                            assert_eq!(
                                retained
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
                            None
                        } else {
                            result.unwrap()
                        }
                    };
                    assert_eq!(joined.is_some(), matches!(scenario, 0 | 5 | 7));
                    for (_, policy) in &boundaries {
                        assert_eq!(policy.joins.get(), 1);
                    }
                    if scenario == 6 {
                        assert_eq!(handles[3].join_stats().contributed, 0);
                    }
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while handles[3].join_stats().parts != 0
                            || handles[4].join_stats().parts != 0
                        {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    })
                    .await
                    .unwrap();
                    if let Some(joined) = joined {
                        if !boundaries.is_empty() {
                            assert_eq!(joined.client.hook.get_brand(), original_brand);
                        }
                        assert_eq!(handles[3].join_stats().accepted, 1);
                        assert_eq!(handles[3].join_stats().contributed, parts);
                        tasks[1].abort();
                        tasks[2].abort();
                        handles[0].disconnect(ids[1].public_key());
                        handles[0].disconnect(ids[2].public_key());
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
                        for (_, policy) in &boundaries {
                            assert_eq!(policy.calls.get(), 1);
                        }
                        if let Some((membrane, policy)) = boundaries.last() {
                            policy.blocked.set(true);
                            assert_eq!(
                                joined
                                    .echo_request()
                                    .send()
                                    .promise
                                    .await
                                    .err()
                                    .unwrap()
                                    .extra,
                                "joined Native capability blocked"
                            );
                            membrane.revoke(Error::failed("Native Join revoked".into()));
                            assert!(joiner.join(vec![joined.clone(), joined]).await.is_err());
                        }
                    } else {
                        assert_eq!(handles[3].join_stats().accepted, 0);
                    }
                    drain().await;
                    assert_eq!(handles[3].join_stats().parts, 0);
                    for task in tasks {
                        task.abort();
                    }
                    listener.abort();
                    drain().await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}

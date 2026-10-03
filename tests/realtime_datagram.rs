use capntproto::{
    native_rpc::Network,
    realtime::{Clock, Config, Outcome},
    realtime_datagram::{Router, Sender, Status, MAX_PAYLOAD_BYTES, MAX_SNAPSHOT_BYTES},
    transport::{self, AuthenticatedSession, Identity, MAX_DATAGRAM_BYTES},
};
use std::{cell::Cell, rc::Rc, time::Duration};
#[derive(Default)]
struct TestClock(Cell<u64>);
impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.get()
    }
}
fn config() -> Config {
    Config::new("datagram-test", 0, 2, 2, 16, MAX_PAYLOAD_BYTES as u32, 2).unwrap()
}
async fn sessions(a: &Identity, b: &Identity) -> (AuthenticatedSession, AuthenticatedSession) {
    let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (aa, bb) = tokio::join!(
        transport::connect_authenticated(
            left,
            right.local_addr().unwrap(),
            a,
            b.public_key(),
            Some([4; 32]),
            b"datagram-test"
        ),
        transport::accept_authenticated(right, b, a.public_key(), Some([4; 32]), b"datagram-test")
    );
    (aa.unwrap(), bb.unwrap())
}
struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
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
async fn large_fragmented_snapshots_over_native_with_reliable_capability_control() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                let (a, b) = (Identity::generate(), Identity::generate());
                let (mut aa, mut bb) = sessions(&a, &b).await;
                let outbound = aa.take_datagrams().unwrap();
                let (router, driver) = Router::new(bb.take_datagrams().unwrap());
                let config =
                    Config::new("datagram-test", 0, 2, 2, 16, MAX_SNAPSHOT_BYTES as u32, 2)
                        .unwrap();
                let (r, service) = router.bind(config, Rc::new(TestClock::default())).unwrap();
                let (an, ah) = Network::new(a.public_key());
                let (bn, bh) = Network::new(b.public_key());
                ah.attach(aa).unwrap();
                bh.attach(bb).unwrap();
                let server = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
                let mut client = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let control = client.bootstrap(b.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(driver),
                    tokio::task::spawn_local(async move {
                        let _ = server.await;
                    }),
                    tokio::task::spawn_local(async move {
                        let _ = client.await;
                    }),
                ]);
                let sender = Sender::connect(control, outbound.sender()).await.unwrap();
                assert!(sender
                    .offer(0, 10, &vec![0; MAX_SNAPSHOT_BYTES + 1])
                    .is_err());
                for (index, size) in [MAX_PAYLOAD_BYTES + 1, MAX_SNAPSHOT_BYTES]
                    .into_iter()
                    .enumerate()
                {
                    let bytes: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
                    let receipt = sender.offer(0, 10, &bytes).unwrap();
                    assert_eq!(receipt.sequence(), index as u64 + 1);
                    // The lane is deliberately unreliable. An explicit immutable
                    // retry recovers lost fragments; status does not imply delivery.
                    while receipt.status().await.unwrap().status == Status::Unknown {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        if let Err(e) = receipt.resend() {
                            assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
                        }
                    }
                    assert_eq!(receipt.status().await.unwrap().status, Status::Pending);
                    assert_eq!(r.apply(receipt.sequence()).unwrap(), Outcome::Applied);
                    assert_eq!(r.get(0).unwrap().bytes(), &bytes);
                    assert_eq!(receipt.cancel().await.unwrap(), Outcome::Applied);
                }
                assert_eq!(sender.cancel(3).await.unwrap(), Outcome::Canceled);
                let canceled = loop {
                    match sender.offer(1, 10, &vec![5; MAX_SNAPSHOT_BYTES]) {
                        Ok(receipt) => break receipt,
                        Err(e) if e.kind == capnp::ErrorKind::Overloaded => {
                            tokio::task::yield_now().await
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                assert_eq!(
                    canceled.status().await.unwrap().status,
                    Status::Terminal(Outcome::Canceled)
                );
                sender.close().await.unwrap();
                assert!(r.is_closed());
                assert!(canceled.resend().is_err());
                assert_eq!(r.get(0).unwrap().sequence(), 2);
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn native_datagrams_with_rpc_receipts_cancellation_deadlines_and_revocation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let (a, b) = (Identity::generate(), Identity::generate());
                let (mut aa, mut bb) = sessions(&a, &b).await;
                let aport = aa.take_datagrams().unwrap();
                assert!(aa.take_datagrams().is_none());
                let outbound = aport.sender();
                assert_eq!(
                    outbound
                        .try_send(&vec![0; MAX_DATAGRAM_BYTES + 1])
                        .unwrap_err()
                        .kind(),
                    std::io::ErrorKind::InvalidInput
                );
                let bport = bb.take_datagrams().unwrap();
                let reverse = bport.sender();
                let (router, driver) = Router::new(bport);
                let clock = Rc::new(TestClock::default());
                let (r, service) = router.bind(config(), clock.clone()).unwrap();
                let (an, ah) = Network::new(a.public_key());
                let (bn, bh) = Network::new(b.public_key());
                ah.attach(aa).unwrap();
                bh.attach(bb).unwrap();
                let server = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
                let mut client = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let control = client.bootstrap(b.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(driver),
                    tokio::task::spawn_local(async move {
                        let _ = server.await;
                    }),
                    tokio::task::spawn_local(async move {
                        let _ = client.await;
                    }),
                ]);
                let sender = Sender::connect(control, outbound.clone()).await.unwrap();
                assert_eq!(sender.status(1).await.unwrap().status, Status::Unknown);
                let first = sender.offer(0, 10, &vec![7; MAX_PAYLOAD_BYTES]).unwrap();
                until(|| r.pending() == [1]).await;
                assert_eq!(r.waiter_count(), 0);
                assert_eq!(first.status().await.unwrap().status, Status::Pending);
                assert_eq!(r.apply(1).unwrap(), Outcome::Applied);
                first.resend().unwrap();
                assert_eq!(
                    first.status().await.unwrap().status,
                    Status::Terminal(Outcome::Applied)
                );
                assert_eq!(first.cancel().await.unwrap(), Outcome::Applied);
                assert_eq!(r.get(0).unwrap().bytes().len(), MAX_PAYLOAD_BYTES);
                // Reliable cancel can arrive before any data and must survive it.
                assert_eq!(sender.cancel(2).await.unwrap(), Outcome::Canceled);
                let canceled = sender.offer(0, 10, b"cannot revive").unwrap();
                canceled.resend().unwrap();
                assert_eq!(
                    canceled.status().await.unwrap().status,
                    Status::Terminal(Outcome::Canceled)
                );
                let third = sender.offer(0, 10, b"old").unwrap();
                until(|| r.pending() == [3]).await;
                let fourth = sender.offer(0, 10, b"new").unwrap();
                until(|| r.pending() == [4]).await;
                assert_eq!(
                    third.status().await.unwrap().status,
                    Status::Terminal(Outcome::Superseded)
                );
                third.resend().unwrap();
                clock.0.set(10);
                assert_eq!(r.apply(4).unwrap(), Outcome::Expired);
                assert_eq!(
                    fourth.status().await.unwrap().status,
                    Status::Terminal(Outcome::Expired)
                );
                // Datagram pressure on an unread reverse lane must not block status RPCs.
                for _ in 0..256 {
                    loop {
                        match reverse.try_send(b"unread") {
                            Ok(()) => break,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                tokio::task::yield_now().await
                            }
                            Err(e) => panic!("{e}"),
                        }
                    }
                }
                let fifth = sender.offer(1, 20, b"close").unwrap();
                until(|| r.pending() == [5]).await;
                let close = sender.close();
                assert!(sender.offer(0, 20, b"sealed").is_err());
                close.await.unwrap();
                assert_eq!(
                    fifth.status().await.unwrap().status,
                    Status::Terminal(Outcome::Closed)
                );
                assert!(r.is_closed());
                assert!(first.resend().is_err());
                assert_eq!(r.get(0).unwrap().sequence(), 1);
                assert!(sender.status(0).await.is_err());
                // Route teardown closes the queue even while application handles survive.
                ah.disconnect(b.public_key());
                until(|| {
                    outbound
                        .try_send(b"x")
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
                })
                .await;
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn tokens_are_session_local_and_router_drop_revokes_pending_work() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let (a, b) = (Identity::generate(), Identity::generate());
                let (mut aa, mut bb) = sessions(&a, &b).await;
                let (mut cc, mut dd) = sessions(&a, &b).await;
                let aport = aa.take_datagrams().unwrap();
                let cport = cc.take_datagrams().unwrap();
                let (router, driver) = Router::new(bb.take_datagrams().unwrap());
                let (other, other_driver) = Router::new(dd.take_datagrams().unwrap());
                let (r, control) = router
                    .bind(config(), Rc::new(TestClock::default()))
                    .unwrap();
                let (other_r, other_control) =
                    other.bind(config(), Rc::new(TestClock::default())).unwrap();
                let task = tokio::task::spawn_local(driver);
                let other_task = tokio::task::spawn_local(other_driver);
                let _tasks = Tasks(vec![task, other_task]);
                // Same pinned identities, different sessions. No capability authority
                // crosses the session boundary merely because Native peers match.
                let wrong = Sender::connect(control.clone(), cport.sender())
                    .await
                    .unwrap();
                wrong.offer(0, 10, b"wrong lane").unwrap();
                // Exercise a valid grant on that other session as well; deterministic
                // ingress traces separately check rejection of wrong tokens.
                let marker = Sender::connect(other_control, cport.sender())
                    .await
                    .unwrap();
                marker.offer(0, 10, b"marker").unwrap();
                until(|| other_r.pending() == [1]).await;
                assert!(r.pending().is_empty());
                let sender = Sender::connect(control, aport.sender()).await.unwrap();
                let receipt = sender.offer(0, 10, b"right lane").unwrap();
                until(|| r.pending() == [1]).await;
                _tasks.0[0].abort();
                until(|| r.is_closed()).await;
                assert_eq!(
                    receipt.status().await.unwrap().status,
                    Status::Terminal(Outcome::Closed)
                );
                assert!(router
                    .bind(config(), Rc::new(TestClock::default()))
                    .is_err());
                // Never polling a driver must still close any grants it owns.
                let (unpolled, unpolled_driver) = Router::new(aport);
                let (unused, _cap) = unpolled
                    .bind(config(), Rc::new(TestClock::default()))
                    .unwrap();
                drop(unpolled_driver);
                assert!(unused.is_closed());
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn checked_configuration_preserves_datagram_import_bounds() {
    use capntproto::realtime_capnp::{config, datagram_snapshots};
    struct Advertisement {
        domain: Vec<u8>,
        sequence: u64,
        payload: u32,
    }
    impl datagram_snapshots::Server for Advertisement {
        async fn describe(
            self: Rc<Self>,
            _: datagram_snapshots::DescribeParams,
            mut result: datagram_snapshots::DescribeResults,
        ) -> capnp::Result<()> {
            let mut result = result.get();
            result.set_token(&[1; 32]);
            let mut c: config::Builder<'_> = result.init_config();
            c.set_clock_domain(capnp::text::Reader(&self.domain));
            c.set_clock_skew(0);
            c.set_keys(1);
            c.set_capacity(1);
            c.set_max_sequence(self.sequence);
            c.set_max_payload_bytes(self.payload);
            c.set_max_waiters(1);
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let (a, b) = (Identity::generate(), Identity::generate());
            let (mut aa, mut bb) = sessions(&a, &b).await;
            let outbound = aa.take_datagrams().unwrap();
            let (router, _driver) = Router::new(bb.take_datagrams().unwrap());
            // A generic realtime configuration may legitimately exceed the datagram
            // limit. Both local binding and peer import must enforce the adapter bound.
            let too_large =
                Config::new("ticks", 0, 1, 1, 1, MAX_SNAPSHOT_BYTES as u32 + 1, 1).unwrap();
            assert!(router
                .bind(too_large, Rc::new(TestClock::default()))
                .is_err());
            for advertisement in [
                Advertisement {
                    domain: b"ticks".to_vec(),
                    sequence: 1,
                    payload: MAX_SNAPSHOT_BYTES as u32 + 1,
                },
                Advertisement {
                    domain: vec![0xff],
                    sequence: 1,
                    payload: 1,
                },
                Advertisement {
                    domain: vec![b'x'; 129],
                    sequence: 1,
                    payload: 1,
                },
                Advertisement {
                    domain: b"ticks".to_vec(),
                    sequence: 0,
                    payload: 1,
                },
            ] {
                let control = capnp_rpc::new_client(advertisement);
                assert!(Sender::connect(control, outbound.sender()).await.is_err());
            }
            let exact = Config::new("ticks", 0, 1, 1, 1, MAX_SNAPSHOT_BYTES as u32, 1).unwrap();
            let (receiver, control) = router.bind(exact, Rc::new(TestClock::default())).unwrap();
            let sender = Sender::connect(control, outbound.sender()).await.unwrap();
            let receipt = sender
                .offer(0, 10, &vec![b'x'; MAX_SNAPSHOT_BYTES])
                .unwrap();
            assert_eq!(receipt.sequence(), 1);
            // Queue admission is intentionally distinct from receiver execution.
            assert!(receiver.pending().is_empty());
            assert!(sender.offer(0, 10, b"exhausted").is_err());
        })
        .await;
}

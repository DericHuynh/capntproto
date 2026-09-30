use reproto::{
    realtime::{Clock, Config, Outcome, Receiver, Sender},
    realtime_capnp::snapshots,
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
    Config::new("test-ticks/v1", 0, 2, 1, 16, 64, 16).unwrap()
}
fn setup() -> (Rc<TestClock>, Receiver, snapshots::Client) {
    let clock = Rc::new(TestClock::default());
    let (receiver, client) = Receiver::new(config(), clock.clone());
    (clock, receiver, client)
}
#[tokio::test(flavor = "current_thread")]
async fn replacement_expiration_busy_and_duplicates() {
    let (clock, r, _client) = setup();
    let first = r.offer(1, 0, 10, b"one");
    let duplicate = r.offer(1, 0, 10, b"one");
    assert_eq!(r.waiter_count(), 2);
    assert!(r.offer(1, 0, 10, b"changed").await.is_err());
    assert_eq!(r.offer(2, 1, 10, b"busy").await.unwrap(), Outcome::Busy);
    clock.0.set(2);
    assert_eq!(
        r.offer(3, 0, 2, b"expired replacement").await.unwrap(),
        Outcome::Expired
    );
    assert_eq!(first.await.unwrap(), Outcome::Superseded);
    assert_eq!(duplicate.await.unwrap(), Outcome::Superseded);
    assert!(r.pending().is_empty());
    assert!(r.get(0).is_none());
    // An older unseen update cannot revive a key after a newer expired offer.
    assert_eq!(
        r.offer(2, 0, 10, b"different key").await.unwrap_err().kind,
        capnp::ErrorKind::Failed
    );
    assert_eq!(
        r.offer(1, 0, 10, b"one").await.unwrap(),
        Outcome::Superseded
    );
    let fresh = r.offer(4, 0, 5, b"fresh");
    clock.0.set(5);
    assert_eq!(r.apply(4).unwrap(), Outcome::Expired);
    assert_eq!(fresh.await.unwrap(), Outcome::Expired);
    let fresh = r.offer(5, 0, 8, b"visible");
    assert_eq!(r.apply(5).unwrap(), Outcome::Applied);
    assert_eq!(fresh.await.unwrap(), Outcome::Applied);
    assert_eq!(r.get(0).unwrap().bytes(), b"visible");
    assert_eq!(
        r.offer(5, 0, 8, b"visible").await.unwrap(),
        Outcome::Applied
    );
    assert_eq!(r.cancel(5).unwrap(), Outcome::Applied);
}
#[tokio::test(flavor = "current_thread")]
async fn cancellation_close_and_receipt_drop_preserve_outcomes() {
    let (_clock, r, _client) = setup();
    assert_eq!(r.cancel(2).unwrap(), Outcome::Canceled);
    assert_eq!(
        r.offer(2, 0, 10, b"cannot revive").await.unwrap(),
        Outcome::Canceled
    );
    let receipt = r.offer(1, 0, 10, b"still pending");
    drop(receipt);
    assert_eq!(r.waiter_count(), 0);
    assert_eq!(r.pending(), [1]);
    assert_eq!(r.apply(1).unwrap(), Outcome::Applied);
    let receipt = r.offer(3, 0, 10, b"closed");
    r.close();
    assert_eq!(receipt.await.unwrap(), Outcome::Closed);
    assert_eq!(r.apply(3).unwrap(), Outcome::Closed);
    assert_eq!(r.offer(4, 0, 10, b"late").await.unwrap(), Outcome::Closed);
    assert_eq!(
        r.offer(1, 0, 10, b"still pending").await.unwrap(),
        Outcome::Applied
    );
    assert_eq!(r.get(0).unwrap().bytes(), b"still pending");
}
#[tokio::test(flavor = "current_thread")]
async fn resource_limits_and_clock_fail_closed() {
    let clock = Rc::new(TestClock(Cell::new(10)));
    let c = Config::new("test-ticks/v1", 1, 2, 1, 3, 64, 1).unwrap();
    let (r, _client) = Receiver::new(c, clock.clone());
    assert_eq!(
        r.offer(1, 0, 11, b"margin").await.unwrap(),
        Outcome::Expired
    );
    let p = r.offer(2, 0, 20, b"pending");
    assert!(r.offer(2, 0, 20, b"pending").await.is_err());
    assert!(r.offer(3, 0, 20, b"over quota").await.is_err());
    assert_eq!(r.pending(), [2]);
    assert!(r.offer(4, 0, 20, b"outside namespace").await.is_err());
    assert!(r.offer(3, 2, 20, b"outside keys").await.is_err());
    clock.0.set(9);
    assert!(r.apply(2).is_err());
    assert!(r.is_closed());
    assert_eq!(p.await.unwrap(), Outcome::Closed);
    let clock = Rc::new(TestClock(Cell::new(u64::MAX - 1)));
    let c = Config::new("test-ticks/v1", 3, 2, 1, 16, 64, 16).unwrap();
    let (r, _client) = Receiver::new(c, clock);
    assert_eq!(
        r.offer(1, 0, u64::MAX, b"overflow").await.unwrap(),
        Outcome::Expired
    );
}
struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}
async fn until(mut p: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !p() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn rpc_timeout_is_unknown_and_does_not_cancel_snapshot() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (_clock, r, service) = setup();
            let (a, b) = tokio::io::duplex(4096);
            let server = reproto::rpc::serve(b, service.client);
            let (client, driver): (snapshots::Client, _) = reproto::rpc::client(a);
            let _tasks = Tasks(vec![server, driver]);
            let sender = Sender::connect(client).await.unwrap();
            let first = sender.offer(0, 10, b"first").unwrap();
            assert_eq!(
                first
                    .wait_until(tokio::time::Instant::now() + Duration::from_millis(5))
                    .await
                    .unwrap(),
                None
            );
            assert_eq!(r.pending(), [1]);
            assert_eq!(r.waiter_count(), 1);
            let second = sender.clone().offer(0, 10, b"second").unwrap();
            assert_eq!(second.sequence(), 2);
            assert_eq!(first.outcome().await.unwrap(), Outcome::Superseded);
            until(|| r.pending() == [2]).await;
            assert_eq!(r.apply(2).unwrap(), Outcome::Applied);
            assert_eq!(second.outcome().await.unwrap(), Outcome::Applied);
            let third = sender.offer(1, 10, b"drop waiter").unwrap();
            until(|| r.pending() == [3] && r.waiter_count() == 1).await;
            drop(third);
            until(|| r.waiter_count() == 0).await;
            assert_eq!(r.pending(), [3]);
            assert_eq!(sender.cancel(3).await.unwrap(), Outcome::Canceled);
            let pending = sender.offer(0, 10, b"close pending").unwrap();
            until(|| r.pending() == [4]).await;
            let close = sender.close();
            assert!(sender.offer(0, 10, b"after close").is_err());
            close.await.unwrap();
            assert_eq!(pending.outcome().await.unwrap(), Outcome::Closed);
            assert!(r.is_closed());
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn receiver_owner_drop_closes_pending_receipts() {
    let (_clock, r, client) = setup();
    let sender = Sender::connect(client).await.unwrap();
    let receipt = sender.offer(0, 10, b"pending").unwrap();
    // Local RPC requests must be polled before they enter the receiver.
    assert_eq!(
        receipt
            .wait_until(tokio::time::Instant::now() + Duration::from_millis(1))
            .await
            .unwrap(),
        None
    );
    let clone = r.clone();
    drop(r);
    assert!(!clone.is_closed());
    drop(clone);
    assert_eq!(receipt.outcome().await.unwrap(), Outcome::Closed);
}
#[tokio::test(flavor = "current_thread")]
async fn authenticated_noise_realtime_worker() {
    use reproto::{
        noise_rpc::Network,
        transport::{self, Identity},
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let a = Identity::generate();
                let b = Identity::generate();
                let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let address = right.local_addr().unwrap();
                let (aa, bb) = tokio::join!(
                    transport::connect_authenticated(
                        left,
                        address,
                        &a,
                        b.public_key(),
                        None,
                        b"realtime"
                    ),
                    transport::accept_authenticated(right, &b, a.public_key(), None, b"realtime")
                );
                let (an, ah) = Network::new(a.public_key());
                let (bn, bh) = Network::new(b.public_key());
                ah.attach(aa.unwrap()).unwrap();
                bh.attach(bb.unwrap()).unwrap();
                let (_clock, r, service) = setup();
                let server = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
                let mut client = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let stream = client.bootstrap(b.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(server),
                    tokio::task::spawn_local(client),
                    tokio::task::spawn_local(r.clone().run(Duration::from_millis(2))),
                ]);
                let sender = Sender::connect(stream).await.unwrap();
                let receipt = sender.offer(1, 10, b"latest snapshot").unwrap();
                assert_eq!(receipt.outcome().await.unwrap(), Outcome::Applied);
                assert_eq!(r.get(1).unwrap().bytes(), b"latest snapshot");
                sender.close().await.unwrap();
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize)]
struct Trace {
    keys: u32,
    capacity: u32,
    skew: u64,
    longer: bool,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    item: u64,
    state: Vec<usize>,
}
fn code(outcome: Outcome) -> usize {
    match outcome {
        Outcome::Applied => 2,
        Outcome::Expired => 3,
        Outcome::Superseded => 4,
        Outcome::Busy => 5,
        Outcome::Canceled => 6,
        Outcome::Closed => 7,
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_realtime_traces() {
    use futures::FutureExt;
    let path = reproto_test_support::verification::input("REPROTO_REALTIME_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    for trace in traces {
        let clock = Rc::new(TestClock(Cell::new(10)));
        let c = Config::new(
            "test-ticks/v1",
            trace.skew,
            trace.keys,
            trace.capacity,
            16,
            64,
            64,
        )
        .unwrap();
        let (r, _service) = Receiver::new(c, clock.clone());
        let mut receipts: Vec<(
            u64,
            Option<capnp::capability::Promise<Outcome, capnp::Error>>,
        )> = Vec::new();
        let mut counts = [0usize; 2];
        let mut time = 0;
        for step in trace.steps {
            match step.action.as_str() {
                "offer" => {
                    let key = if trace.keys == 1 {
                        0
                    } else {
                        (step.item - 1) as u32
                    };
                    let deadline = if trace.longer {
                        if step.item == 1 {
                            3
                        } else {
                            1
                        }
                    } else {
                        2
                    };
                    receipts.push((
                        step.item,
                        Some(r.offer(step.item, key, 10 + deadline, &[step.item as u8])),
                    ));
                }
                "apply" => {
                    let outcome = r.apply(step.item).unwrap();
                    if outcome == Outcome::Applied {
                        counts[step.item as usize - 1] += 1;
                    }
                }
                "expire" => r.expire().unwrap(),
                "cancel" => {
                    r.cancel(step.item).unwrap();
                }
                "close" => r.close(),
                "tick" => {
                    time += 1;
                    clock.0.set(10 + time);
                }
                _ => panic!(),
            }
            let pending = r.pending();
            let status = |seq| {
                r.status(seq)
                    .map(code)
                    .unwrap_or(usize::from(pending.contains(&seq)))
            };
            assert_eq!(
                [
                    status(1),
                    status(2),
                    r.get(0).map_or(0, |s| s.sequence() as usize),
                    r.get(1).map_or(0, |s| s.sequence() as usize),
                    counts[0],
                    counts[1],
                    pending.len(),
                    usize::from(r.is_closed()),
                    time as usize
                ],
                step.state[..],
                "{} {}",
                step.action,
                step.item
            );
            for (seq, promise) in &mut receipts {
                if let Some(outcome) = r.status(*seq) {
                    if let Some(promise) = promise.take() {
                        assert_eq!(
                            promise
                                .now_or_never()
                                .expect("terminal receipt not delivered")
                                .unwrap(),
                            outcome
                        );
                    }
                }
            }
            assert_eq!(
                r.waiter_count(),
                receipts.iter().filter(|(_, p)| p.is_some()).count()
            );
        }
    }
}

struct DelayedReceipt {
    inner: snapshots::Client,
    gate: std::cell::RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
}
impl snapshots::Server for DelayedReceipt {
    async fn describe(
        self: Rc<Self>,
        _: snapshots::DescribeParams,
        mut out: snapshots::DescribeResults,
    ) -> capnp::Result<()> {
        let response = self.inner.describe_request().send().promise.await?;
        out.get().set_config(response.get()?.get_config()?)?;
        Ok(())
    }
    async fn offer(
        self: Rc<Self>,
        p: snapshots::OfferParams,
        mut out: snapshots::OfferResults,
    ) -> capnp::Result<()> {
        let mut request = self.inner.offer_request();
        request.set(p.get()?)?;
        drop(p);
        let response = request.send().promise.await?;
        let gate = self.gate.borrow_mut().take().unwrap();
        gate.await
            .map_err(|_| capnp::Error::failed("receipt gate canceled".into()))?;
        out.get().set_outcome(response.get()?.get_outcome()?);
        Ok(())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn timeout_after_application_still_returns_the_authoritative_receipt() {
    let (_clock, r, inner) = setup();
    let (tx, rx) = futures::channel::oneshot::channel();
    let delayed = capnp_rpc::new_client(DelayedReceipt {
        inner,
        gate: std::cell::RefCell::new(Some(rx)),
    });
    let sender = Sender::connect(delayed).await.unwrap();
    let receipt = sender.offer(0, 10, b"already visible").unwrap();
    let (result, ()) = tokio::join!(
        receipt.wait_until(tokio::time::Instant::now() + Duration::from_millis(10)),
        async {
            until(|| r.pending() == [1]).await;
            assert_eq!(r.apply(1).unwrap(), Outcome::Applied);
        }
    );
    assert_eq!(result.unwrap(), None);
    assert_eq!(r.get(0).unwrap().bytes(), b"already visible");
    tx.send(()).unwrap();
    assert_eq!(receipt.outcome().await.unwrap(), Outcome::Applied);
}
#[tokio::test(flavor = "current_thread")]
async fn stream_capabilities_isolate_identical_sequence_numbers() {
    let (_a, ra, ca) = setup();
    let (_b, rb, cb) = setup();
    let a = Sender::connect(ca).await.unwrap();
    let b = Sender::connect(cb).await.unwrap();
    let ar = a.offer(0, 10, b"a").unwrap();
    let br = b.offer(0, 10, b"b").unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(2);
    let (aw, bw) = tokio::join!(ar.wait_until(deadline), br.wait_until(deadline));
    assert_eq!(aw.unwrap(), None);
    assert_eq!(bw.unwrap(), None);
    assert_eq!(a.cancel(1).await.unwrap(), Outcome::Canceled);
    assert_eq!(ar.outcome().await.unwrap(), Outcome::Canceled);
    assert_eq!(rb.pending(), [1]);
    assert!(ra.get(0).is_none());
    assert_eq!(rb.apply(1).unwrap(), Outcome::Applied);
    assert_eq!(br.outcome().await.unwrap(), Outcome::Applied);
    assert_eq!(rb.get(0).unwrap().bytes(), b"b");
}

#[tokio::test(flavor = "current_thread")]
async fn retained_snapshots_keep_coordinates_and_bytes_after_replacement_and_close() {
    let (_clock, receiver, client) = setup();
    let first = receiver.offer(1, 0, 10, b"first");
    receiver.apply(1).unwrap();
    assert_eq!(first.await.unwrap(), Outcome::Applied);
    let retained = receiver.get(0).unwrap();
    let copied = retained.as_ref().clone();
    let second = receiver.offer(2, 0, 20, b"second");
    receiver.apply(2).unwrap();
    assert_eq!(second.await.unwrap(), Outcome::Applied);
    let newer = receiver.get(0).unwrap();
    // A longer history than the four-step receipt model: late observation of
    // an earlier application must not republish it over the newer snapshot.
    assert_eq!(receiver.apply(1).unwrap(), Outcome::Applied);
    assert_eq!(receiver.cancel(1).unwrap(), Outcome::Applied);
    assert_eq!(
        receiver.offer(1, 0, 10, b"first").await.unwrap(),
        Outcome::Applied
    );
    assert!(receiver.offer(1, 0, 10, b"altered").await.is_err());
    assert!(Rc::ptr_eq(&newer, &receiver.get(0).unwrap()));
    receiver.close();
    drop(receiver);
    drop(client);
    let owned = Rc::try_unwrap(retained).unwrap();
    for snapshot in [&owned, &copied] {
        assert_eq!(snapshot.sequence(), 1);
        assert_eq!(snapshot.key(), 0);
        assert_eq!(snapshot.not_after(), 10);
        assert_eq!(snapshot.bytes(), b"first");
    }
    assert_eq!(newer.sequence(), 2);
    assert_eq!(newer.not_after(), 20);
    assert_eq!(newer.bytes(), b"second");
}

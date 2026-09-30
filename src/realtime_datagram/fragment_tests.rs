use super::*;

#[derive(Default)]
struct TestClock(Cell<u64>);
impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.get()
    }
}
fn config() -> Config {
    Config::new("fragments", 0, 2, 1, 16, MAX_SNAPSHOT_BYTES as u32, 2).unwrap()
}
struct Fixture {
    router: Router,
    guard: Option<DriverGuard>,
    clock: Rc<TestClock>,
    receiver: Receiver,
    control: wire::Client,
    grant: Rc<Grant>,
}
impl Fixture {
    fn new(config: Config) -> Self {
        let state = Rc::new(RefCell::new(State {
            closed: false,
            issued: BTreeSet::new(),
            grants: BTreeMap::new(),
        }));
        let router = Router(state.clone());
        let clock = Rc::new(TestClock::default());
        let (receiver, control) = router.bind(config, clock.clone()).unwrap();
        let grant = state
            .borrow()
            .grants
            .values()
            .next()
            .unwrap()
            .upgrade()
            .unwrap();
        Self {
            router,
            guard: Some(DriverGuard(state)),
            clock,
            receiver,
            control,
            grant,
        }
    }
    fn packets(&self, sequence: u64, bytes: &[u8]) -> Vec<Vec<u8>> {
        packets(&self.grant.token, sequence, 0, 10, bytes)
    }
    fn deliver(&self, packet: &[u8]) {
        Router::dispatch(&self.router.0, packet);
    }
    fn all(&self, packets: &[Vec<u8>]) {
        for packet in packets {
            self.deliver(packet);
        }
    }
    fn buffered(&self) -> usize {
        self.grant.fragments.borrow().partial.len()
    }
    async fn status(&self, sequence: u64) -> Status {
        let mut request = self.control.status_request();
        request.get().set_sequence(sequence);
        let response = request.send().promise.await.unwrap();
        match response
            .get()
            .unwrap()
            .get_status()
            .unwrap()
            .which()
            .unwrap()
        {
            datagram_status::Unknown(()) => Status::Unknown,
            datagram_status::Pending(()) => Status::Pending,
            datagram_status::Terminal(outcome) => Status::Terminal(outcome.unwrap()),
        }
    }
    async fn cancel(&self, sequence: u64) -> Outcome {
        let mut request = self.control.cancel_request();
        request.get().set_sequence(sequence);
        request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_outcome()
            .unwrap()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_snapshots_reorder_duplicate_and_publish_only_complete_values() {
    for size in [
        MAX_PAYLOAD_BYTES + 1,
        2 * CHUNK,
        2 * CHUNK + 1,
        MAX_SNAPSHOT_BYTES,
    ] {
        let f = Fixture::new(config());
        let bytes: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let packets = f.packets(1, &bytes);
        assert_eq!(packets.len(), size.div_ceil(CHUNK));
        assert!(packets.iter().all(|p| p.len() <= MAX_DATAGRAM_BYTES));
        for p in packets[1..].iter().rev() {
            f.deliver(p);
            f.deliver(p);
            assert!(f.receiver.pending().is_empty());
            assert!(f.receiver.get(0).is_none());
            assert_eq!(f.status(1).await, Status::Unknown);
        }
        f.deliver(&packets[0]);
        assert_eq!(f.buffered(), 0);
        assert_eq!(f.status(1).await, Status::Pending);
        assert_eq!(f.receiver.waiter_count(), 0);
        assert_eq!(f.receiver.apply(1).unwrap(), Outcome::Applied);
        let value = f.receiver.get(0).unwrap();
        assert_eq!(value.bytes(), &bytes);
        f.all(&packets);
        assert_eq!(f.cancel(1).await, Outcome::Applied);
        assert!(Rc::ptr_eq(&value, &f.receiver.get(0).unwrap()));
        assert_eq!(f.status(1).await, Status::Terminal(Outcome::Applied));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn fragment_validation_commitments_and_hash_failure_preserve_authority() {
    let f = Fixture::new(config());
    let bytes = vec![37; CHUNK + 100];
    let good = f.packets(1, &bytes);
    let first = &good[0];
    for n in 0..first.len() {
        f.deliver(&first[..n]);
    }
    assert_eq!(f.buffered(), 0);
    assert!(f.grant.fragments.borrow().metadata.is_empty());
    let mut malformed = Vec::new();
    for (offset, replacement) in [
        (36, 0u64.to_be_bytes().to_vec()),
        (36, 17u64.to_be_bytes().to_vec()),
        (44, 2u32.to_be_bytes().to_vec()),
        (56, u32::MAX.to_be_bytes().to_vec()),
        (56, (MAX_PAYLOAD_BYTES as u32).to_be_bytes().to_vec()),
        (60, 1u32.to_be_bytes().to_vec()),
        (60, u32::MAX.to_be_bytes().to_vec()),
    ] {
        let mut p = first.clone();
        p[offset..offset + replacement.len()].copy_from_slice(&replacement);
        malformed.push(p);
    }
    for offset in [0, 4] {
        let mut p = first.clone();
        p[offset] ^= 1;
        malformed.push(p);
    }
    malformed.push(vec![0; MAX_DATAGRAM_BYTES + 1]);
    f.all(&malformed);
    assert_eq!(f.buffered(), 0);
    assert!(f.grant.fragments.borrow().metadata.is_empty());

    f.deliver(first);
    // A conflicting duplicate must not overwrite the first chunk.
    let mut conflict = first.clone();
    conflict[HEADER] ^= 1;
    f.deliver(&conflict);
    // Key, deadline, length and whole-value hash are immutable after first arrival.
    for offset in [47, 55, 59, 64] {
        let mut p = good[1].clone();
        p[offset] ^= 1;
        f.deliver(&p);
        assert!(f.receiver.pending().is_empty());
    }
    // Changing the encoding cannot replace the committed large value either.
    f.deliver(&super::super::packet(
        &f.grant.token,
        1,
        0,
        10,
        b"replacement",
    ));
    f.deliver(&good[1]);
    f.receiver.apply(1).unwrap();
    assert_eq!(f.receiver.get(0).unwrap().bytes(), &bytes);

    let second = f.packets(2, &bytes);
    let mut corrupt = second[0].clone();
    corrupt[HEADER] ^= 1;
    f.deliver(&corrupt);
    f.deliver(&second[1]);
    assert_eq!(f.buffered(), 0);
    assert_eq!(f.status(2).await, Status::Unknown);
    // Failed integrity releases storage, but cannot change the committed digest.
    f.all(&f.packets(2, &vec![42; bytes.len()]));
    assert_eq!(f.status(2).await, Status::Unknown);
    f.all(&second);
    f.receiver.apply(2).unwrap();
    assert_eq!(f.receiver.get(0).unwrap().bytes(), &bytes);
}

#[tokio::test(flavor = "current_thread")]
async fn bounded_partial_storage_cancel_expiry_and_close_reclaim_buffers() {
    let mut f = Fixture::new(config());
    let payload = vec![9; CHUNK + 100];
    let first = f.packets(1, &payload);
    let second = f.packets(2, &payload);
    f.deliver(&first[0]);
    f.deliver(&second[0]);
    assert_eq!(f.buffered(), 1);
    assert_eq!(f.status(1).await, Status::Unknown);
    assert_eq!(f.status(2).await, Status::Unknown);
    assert_eq!(f.cancel(1).await, Outcome::Canceled);
    assert_eq!(f.buffered(), 0);
    f.all(&first);
    assert_eq!(f.status(1).await, Status::Terminal(Outcome::Canceled));
    f.all(&second);
    assert_eq!(f.status(2).await, Status::Pending);
    // A newer partial snapshot cannot supersede complete, applicable work.
    let third = f.packets(3, &payload);
    f.deliver(&third[0]);
    assert_eq!(f.status(2).await, Status::Pending);
    f.clock.0.set(10);
    f.router.expire();
    assert_eq!(f.buffered(), 0);
    assert_eq!(f.status(2).await, Status::Terminal(Outcome::Expired));
    f.all(&third);
    assert_eq!(f.status(3).await, Status::Unknown);
    let fourth = packets(&f.grant.token, 4, 0, 20, &payload);
    f.deliver(&fourth[0]);
    assert_eq!(f.buffered(), 1);
    drop(f.guard.take());
    assert_eq!(f.buffered(), 0);
    assert!(f.receiver.is_closed());
    f.all(&fourth);
    assert_eq!(f.status(4).await, Status::Unknown);

    for method in 0..3 {
        let f = Fixture::new(config());
        f.deliver(&f.packets(1, &payload)[0]);
        match method {
            0 => {
                f.control.close_request().send().promise.await.unwrap();
            }
            1 => {
                f.receiver.close();
                f.router.expire();
            }
            _ => {
                f.clock.0.set(5);
                f.router.expire();
                f.clock.0.set(4);
                f.router.expire();
            }
        }
        assert!(f.receiver.is_closed());
        assert_eq!(f.buffered(), 0);
    }
    let c = Config::new("fragments", 0, 16, 16, 16, MAX_SNAPSHOT_BYTES as u32, 2).unwrap();
    let f = Fixture::new(c);
    for s in 1..=16 {
        f.deliver(&f.packets(s, &payload)[0]);
    }
    assert_eq!(f.buffered(), MAX_REASSEMBLIES);

    let f = Fixture::new(config());
    f.deliver(&f.packets(1, &payload)[0]);
    let weak = Rc::downgrade(&f.grant);
    drop(f.grant);
    drop(f.control);
    assert!(
        weak.upgrade().is_none(),
        "control release retained a partial payload"
    );
    assert!(f.receiver.is_closed());
    assert!(f.router.0.borrow().grants.is_empty());

    let c = Config::new(
        "fragments",
        u64::MAX,
        2,
        1,
        16,
        MAX_SNAPSHOT_BYTES as u32,
        2,
    )
    .unwrap();
    let f = Fixture::new(c);
    f.clock.0.set(1);
    f.all(&packets(&f.grant.token, 1, 0, u64::MAX, &payload));
    assert_eq!(
        f.buffered(),
        0,
        "overflowing deadline comparison admitted fragments"
    );
    assert_eq!(f.status(1).await, Status::Unknown);
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_and_compact_payloads_share_receiver_ordering() {
    let f = Fixture::new(config());
    let payload = vec![5; CHUNK + 100];
    let old = f.packets(1, &payload);
    f.deliver(&old[0]);
    f.all(&f.packets(2, b"new small snapshot"));
    assert_eq!(f.status(2).await, Status::Pending);
    f.deliver(&old[1]);
    assert_eq!(f.status(1).await, Status::Terminal(Outcome::Superseded));
    f.receiver.apply(2).unwrap();
    assert_eq!(f.receiver.get(0).unwrap().bytes(), b"new small snapshot");
    let mut other = packets(&f.grant.token, 3, 1, 10, &payload);
    f.all(&f.packets(4, &payload));
    f.all(&other);
    assert_eq!(f.status(3).await, Status::Terminal(Outcome::Busy));
    f.receiver.apply(4).unwrap();
    // Terminal Busy cannot be replayed into an effect once capacity frees.
    f.all(&other);
    assert_eq!(f.status(3).await, Status::Terminal(Outcome::Busy));
    // A receiver-bound violation is rejected before allocating a commitment.
    let c = Config::new("fragments", 0, 2, 1, 16, MAX_PAYLOAD_BYTES as u32, 2).unwrap();
    let small = Fixture::new(c);
    other = small.packets(1, &payload);
    small.all(&other);
    assert!(small.grant.fragments.borrow().metadata.is_empty());
}

#[derive(serde::Deserialize)]
struct Trace {
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u64>,
}
fn status_code(receiver: &Receiver) -> u64 {
    match receiver.status(1) {
        Some(Outcome::Applied) => 2,
        Some(Outcome::Canceled) => 3,
        Some(Outcome::Expired) => 4,
        Some(Outcome::Closed) => 5,
        Some(other) => panic!("unmodeled outcome {other:?}"),
        None if receiver.pending().contains(&1) => 1,
        None => 0,
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_fragment_traces() {
    let path = reproto_test_support::verification::input("REPROTO_FRAGMENT_TRACES")
        .expect("run this test through its verification driver");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, trace) in traces.into_iter().enumerate() {
                let mut f = Fixture::new(config());
                let bytes = vec![37; CHUNK + 100];
                let packets = f.packets(1, &bytes);
                let competing = f.packets(2, &bytes);
                let mut corrupt = packets[0].clone();
                corrupt[HEADER] ^= 1;
                let mut wrong = packets[0].clone();
                wrong[4] ^= 1;
                let mut conflict = packets[0].clone();
                conflict[64] ^= 1;
                let original = Metadata {
                    key: 0,
                    deadline: 10,
                    total: bytes.len(),
                    digest: digest(&bytes),
                };
                let (a, b) = tokio::io::duplex(4096);
                let server = crate::rpc::serve(b, f.control.clone().client);
                let (control, client) = crate::rpc::client(a);
                f.control = control;
                let mut verified = false;
                let mut applied = 0;
                let mut unauthorized = false;
                for step in trace.steps {
                    let old = f.receiver.get(0);
                    match step.action.as_str() {
                        "first" => f.deliver(&packets[0]),
                        "second" => f.deliver(&packets[1]),
                        "corrupt" => f.deliver(&corrupt),
                        "wrong_token" => {
                            let before = (
                                f.buffered(),
                                f.grant.fragments.borrow().metadata.len(),
                                status_code(&f.receiver),
                            );
                            f.deliver(&wrong);
                            unauthorized |= before
                                != (
                                    f.buffered(),
                                    f.grant.fragments.borrow().metadata.len(),
                                    status_code(&f.receiver),
                                );
                        }
                        "conflict" => f.deliver(&conflict),
                        "malformed" => f.deliver(&packets[0][..HEADER]),
                        "cancel" => {
                            f.cancel(1).await;
                        }
                        "apply" => {
                            assert_eq!(f.receiver.apply(1).unwrap(), Outcome::Applied);
                        }
                        "close" => {
                            f.control.close_request().send().promise.await.unwrap();
                        }
                        "tick" => {
                            f.clock.0.set(10);
                            f.router.expire();
                        }
                        "compete" => f.deliver(&competing[0]),
                        "cancel_other" => {
                            f.cancel(2).await;
                        }
                        "stop" => {
                            drop(f.guard.take());
                        }
                        other => panic!("unknown action {other}"),
                    }
                    let status = status_code(&f.receiver);
                    let observation = f.status(1).await;
                    assert_eq!(
                        observation,
                        match status {
                            0 => Status::Unknown,
                            1 => Status::Pending,
                            2 => Status::Terminal(Outcome::Applied),
                            3 => Status::Terminal(Outcome::Canceled),
                            4 => Status::Terminal(Outcome::Expired),
                            5 => Status::Terminal(Outcome::Closed),
                            _ => unreachable!(),
                        }
                    );
                    verified |= status == 1;
                    if let Some(value) = f.receiver.get(0) {
                        assert_eq!(
                            value.bytes(),
                            &bytes,
                            "corrupt/partial publication in trace {index}"
                        );
                        if old.is_none_or(|old| !Rc::ptr_eq(&old, &value)) {
                            applied += 1;
                        }
                    }
                    let fragments = f.grant.fragments.borrow();
                    let main = fragments.partial.get(&1);
                    let actual = vec![
                        fragments
                            .metadata
                            .get(&1)
                            .map_or(0, |meta| if *meta == original { 1 } else { 2 }),
                        main.map_or(0, |a| a.received),
                        main.is_some_and(|a| a.received & 1 != 0 && a.bytes[0] != bytes[0]) as u64,
                        status,
                        verified as u64,
                        applied,
                        fragments.contains(2) as u64,
                        fragments.partial.contains_key(&2) as u64,
                        (f.receiver.status(2) == Some(Outcome::Canceled)) as u64,
                        f.receiver.is_closed() as u64,
                        (f.clock.0.get() == 10) as u64,
                        unauthorized as u64,
                    ];
                    assert_eq!(actual, step.state, "trace {index}, action {}", step.action);
                }
                client.abort();
                server.abort();
            }
        })
        .await;
}

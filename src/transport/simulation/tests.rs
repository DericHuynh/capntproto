use super::*;
use crate::{
    noise_shutdown::Control,
    transport::{
        self, scheduling, shutdown::ShutdownDriver, Identity, PacketSocket, SessionDrivers,
    },
};
use futures::{future::LocalBoxFuture, FutureExt};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::oneshot,
};

const ADDRESSES: [&str; 2] = ["127.0.0.1:1234", "127.0.0.1:4321"];

#[derive(Default)]
struct Ready(AtomicBool);
impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct Task {
    future: Option<LocalBoxFuture<'static, io::Result<()>>>,
    ready: Arc<Ready>,
    result: Option<Result<(), io::ErrorKind>>,
}
impl Task {
    fn new(future: impl Future<Output = io::Result<()>> + 'static) -> Self {
        Self {
            future: Some(Box::pin(future)),
            ready: Arc::new(Ready(AtomicBool::new(true))),
            result: None,
        }
    }
    fn poll(&mut self) {
        if !self.ready.0.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Some(future) = &mut self.future {
            let waker = Waker::from(self.ready.clone());
            if let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(&waker)) {
                self.result = Some(result.map_err(|e| e.kind()));
                self.future = None;
            }
        }
    }
}
struct Peer {
    id: usize,
    app: DuplexStream,
    driver: Task,
    control: Control,
    ready: oneshot::Receiver<()>,
    authenticated: bool,
    received: Vec<u8>,
    mobility: transport::Mobility,
}

mod listener;
mod migration;
mod nat;
struct Fixture {
    network: Network,
    peers: [Peer; 2],
}
impl Fixture {
    fn new(network: Network, psk: bool) -> Self {
        Self::with_mobility(network, psk, false)
    }
    fn with_mobility(network: Network, psk: bool, mobility: bool) -> Self {
        let identities = [
            Identity::from_private_key([7; 32]).unwrap(),
            Identity::from_private_key([9; 32]).unwrap(),
        ];
        let addresses = ADDRESSES.map(|address| address.parse().unwrap());
        let peers = std::array::from_fn(|index| {
            let mut config = transport::config(
                &identities[index],
                identities[1 - index].public_key(),
                psk.then_some([42; 32]),
                b"runtime packet simulation",
            )
            .unwrap();
            config.set_initial_max_stream_data_bidi_local(42);
            config.set_initial_max_stream_data_bidi_remote(42);
            let cid = [index as u8 + 1; 16];
            let conn = if index == 0 {
                quiche::connect(
                    None,
                    &quiche::ConnectionId::from_ref(&cid),
                    addresses[index],
                    addresses[1 - index],
                    &mut config,
                )
            } else {
                quiche::accept(
                    &quiche::ConnectionId::from_ref(&cid),
                    None,
                    addresses[index],
                    addresses[1 - index],
                    &mut config,
                )
            }
            .unwrap();
            let socket = network.bind(addresses[index]);
            let id = socket.id;
            let control = Control::new();
            let (app, io) = tokio::io::duplex(11);
            let (ready, wait) = oneshot::channel();
            let (control_mobility, driver_mobility) = transport::mobility::pair();
            let driver = Task::new(transport::drive(
                PacketSocket::Dedicated(super::super::socket::DatagramSocket::Simulated(socket)),
                Box::new(conn),
                io,
                SessionDrivers {
                    established: Some(ready),
                    datagrams: None,
                    shutdown: Some(ShutdownDriver::new(control.clone(), index == 1)),
                    mobility: mobility.then_some(driver_mobility),
                    scheduling: scheduling::pair().1,
                },
            ));
            Peer {
                id,
                app,
                driver,
                control,
                ready: wait,
                authenticated: false,
                received: Vec::new(),
                mobility: control_mobility,
            }
        });
        Self { network, peers }
    }
    fn poll(&mut self) {
        for peer in &mut self.peers {
            peer.driver.poll();
            if !peer.authenticated && peer.ready.try_recv().is_ok() {
                peer.authenticated = true;
            }
        }
    }
    async fn tick(&mut self) {
        self.poll();
        while let Some(packet) = self.network.take(false) {
            self.network.deliver(packet);
        }
        tokio::time::advance(Duration::from_millis(1)).await;
        self.poll();
    }
    async fn establish(&mut self) {
        for _ in 0..2000 {
            self.tick().await;
            if self.peers.iter().all(|peer| peer.authenticated) {
                return;
            }
        }
        panic!(
            "simulated authentication did not finish: {:?}",
            self.peers.each_ref().map(|p| p.driver.result)
        );
    }
    fn read(&mut self) {
        for peer in &mut self.peers {
            let mut buf = [0; 7];
            if let Some(result) = peer.app.read(&mut buf).now_or_never() {
                let n = result.unwrap();
                peer.received.extend_from_slice(&buf[..n]);
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn socket_readiness_cancellation_and_address_reuse_match_udp_boundaries() {
    let network = Network::default();
    let a = network.bind(ADDRESSES[0].parse().unwrap());
    let b = network.bind(ADDRESSES[1].parse().unwrap());
    let address = b.local_addr().unwrap();
    network.sending(a.id, Send::Blocked);
    assert!(a.send_to(b"canceled", address).now_or_never().is_none());
    network.sending(a.id, Send::Ready);
    assert!(network.take(false).is_none());
    a.send_to(b"delivered", address).await.unwrap();
    network.deliver(network.take(false).unwrap());
    let mut buf = [0; 3];
    assert_eq!(b.recv_from(&mut buf).await.unwrap().0, 3);
    assert_eq!(&buf, b"del");
    assert!(b.recv_from(&mut buf).now_or_never().is_none());
    let wake = Arc::new(Ready::default());
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    let mut receive = Box::pin(a.recv_from(&mut buf));
    assert!(receive.as_mut().poll(&mut cx).is_pending());
    network.fail_receive(0, io::ErrorKind::ConnectionReset);
    assert!(wake.0.swap(false, Ordering::SeqCst));
    assert_eq!(
        receive.await.unwrap_err().kind(),
        io::ErrorKind::ConnectionReset
    );
    network.close(b.id);
    let replacement = network.bind(address);
    drop(b);
    assert!(replacement.check_open().is_ok());
}

#[tokio::test(start_paused = true)]
async fn real_drivers_preserve_bidirectional_bytes_and_receipts_through_packet_faults() {
    for psk in [false, true] {
        packet_schedule(psk, &[1, 2, 3, 4], &mut Vec::new()).await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    received: [Vec<u8>; 2],
    receipts: [u64; 2],
}

async fn packet_schedule(psk: bool, choices: &[u8], log: &mut Vec<(usize, usize, u8)>) -> Outcome {
    assert!(!choices.is_empty() && choices.len() <= 6 && choices.iter().all(|c| *c <= 4));
    let mut fixture = Fixture::new(Network::default(), psk);
    // Lose the initiator's first flight; recovery must run through the actual
    // adapter timer rather than directly calling Connection::on_timeout.
    let mut lost = false;
    for _ in 0..20 {
        fixture.poll();
        tokio::time::advance(Duration::from_millis(1)).await;
        if fixture.network.take(false).is_some() {
            lost = true;
            break;
        }
    }
    assert!(lost, "initial-flight loss did not occur");
    fixture.establish().await;
    let inputs = [
        b"client request sequence: abcdefghijklmnopqrstuvwxyz".to_vec(),
        b"server response sequence: ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_vec(),
    ];
    let mut sent = [0; 2];
    let mut ordinal = 0;
    let mut delayed = VecDeque::new();
    for turn in 0..4000 {
        for (i, peer) in fixture.peers.iter_mut().enumerate() {
            if sent[i] < inputs[i].len() {
                if let Some(result) = peer.app.write(&inputs[i][sent[i]..]).now_or_never() {
                    sent[i] += result.unwrap();
                }
            }
        }
        fixture.poll();
        while let Some(mut packet) = fixture.network.take(turn % 2 == 0) {
            let action = choices.get(ordinal).copied().unwrap_or(0);
            log.push((turn, packet.bytes.len(), action));
            ordinal += 1;
            match action {
                1 => (), // lost data/ACK
                2 => {
                    fixture.network.deliver(packet.clone());
                    fixture.network.deliver(packet);
                }
                3 => {
                    *packet.bytes.last_mut().unwrap() ^= 1;
                    fixture.network.deliver(packet);
                }
                4 => delayed.push_back((turn + 20, packet)),
                _ => fixture.network.deliver(packet),
            }
        }
        while delayed.front().is_some_and(|(until, _)| turn >= *until) {
            fixture.network.deliver(delayed.pop_front().unwrap().1);
        }
        fixture.read();
        tokio::time::advance(Duration::from_millis(1)).await;
        if fixture.peers[0].received == inputs[1] && fixture.peers[1].received == inputs[0] {
            break;
        }
    }
    assert!(
        ordinal >= choices.len(),
        "fault schedule did not reach its last action"
    );
    assert_eq!(fixture.peers[0].received, inputs[1]);
    assert_eq!(fixture.peers[1].received, inputs[0]);
    for peer in &mut fixture.peers {
        peer.control.begin(Duration::from_secs(1)).unwrap();
        peer.app.shutdown().await.unwrap();
    }
    for _ in 0..2000 {
        fixture.tick().await;
        fixture.read();
        if fixture
            .peers
            .iter()
            .all(|peer| peer.driver.result.is_some())
        {
            break;
        }
    }
    let mut receipts = [0; 2];
    for (i, peer) in fixture.peers.iter().enumerate() {
        assert_eq!(peer.driver.result, Some(Ok(())));
        receipts[i] = peer.control.wait().now_or_never().unwrap().unwrap().bytes;
        assert_eq!(receipts[i], inputs[i].len() as u64);
    }
    Outcome {
        received: fixture.peers.each_ref().map(|peer| peer.received.clone()),
        receipts,
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    format: u32,
    seed: u64,
    cases: Vec<(bool, Vec<u8>, Outcome)>,
}

#[test]
fn generated_runtime_packet_schedules() {
    use proptest::test_runner::{Config, RngAlgorithm, RngSeed, TestCaseError, TestRunner};
    use reproto_test_support::verification as v;
    const SEED: u64 = 0x5250_534F_434B_4554;
    let output = std::env::var_os("REPROTO_RUNTIME_SIM_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let base = v::root().join("target/verification/runtime-socket");
            std::fs::create_dir_all(&base).unwrap();
            tempfile::Builder::new()
                .prefix("run-")
                .tempdir_in(base)
                .unwrap()
                .keep()
        });
    std::fs::create_dir_all(&output).unwrap();
    eprintln!("runtime packet artifacts: {}", output.display());
    // Read before clearing the previous success record: input and output may
    // deliberately be the same saved run. A failed run must not leave a stale
    // success artifact that appears to qualify the new inputs.
    let replay =
        std::env::var_os("REPROTO_RUNTIME_SIM_REPLAY").map(|path| std::fs::read(path).unwrap());
    match std::fs::remove_file(output.join("runs.json")) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => panic!("cannot clear previous simulation outcome: {error}"),
    }
    let saved = RefCell::new(Saved {
        format: 1,
        seed: SEED,
        cases: Vec::new(),
    });
    let attempt = std::cell::Cell::new(0);
    let run = |psk, choices: Vec<u8>| {
        let mut log = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(packet_schedule(psk, &choices, &mut log))
        }));
        let file = output.join(format!("case-{}.json", attempt.get()));
        attempt.set(attempt.get() + 1);
        std::fs::write(
            file,
            serde_json::to_vec_pretty(&serde_json::json!({
                "psk":psk,"choices":choices,"packet_events":log,"ok":result.is_ok()
            }))
            .unwrap(),
        )
        .unwrap();
        match result {
            Ok(outcome) => {
                saved
                    .borrow_mut()
                    .cases
                    .push((psk, choices, outcome.clone()));
                Ok(outcome)
            }
            Err(_) => Err(TestCaseError::fail(
                "runtime packet scenario failed; see retained packet events",
            )),
        }
    };
    if let Some(replay) = replay {
        let expected: Saved = serde_json::from_slice(&replay).unwrap();
        assert_eq!(expected.format, 1);
        assert_eq!(expected.seed, SEED);
        assert!(!expected.cases.is_empty() && expected.cases.len() <= 32);
        for (psk, choices, outcome) in expected.cases {
            assert_eq!(
                run(psk, choices).unwrap(),
                outcome,
                "runtime semantic replay diverged"
            );
        }
    } else {
        let mut runner = TestRunner::new(Config {
            cases: 32,
            rng_algorithm: RngAlgorithm::ChaCha,
            rng_seed: RngSeed::Fixed(SEED),
            max_shrink_iters: 256,
            max_shrink_time: 30_000,
            failure_persistence: None,
            ..Config::default()
        });
        let strategy = (
            proptest::bool::ANY,
            proptest::collection::vec(0u8..5, 1..=6),
        );
        runner
            .run(&strategy, |(psk, choices)| run(psk, choices).map(|_| ()))
            .unwrap();
        assert_eq!(saved.borrow().cases.len(), 32);
    }
    std::fs::write(
        output.join("runs.json"),
        serde_json::to_vec_pretty(&saved.into_inner()).unwrap(),
    )
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn blocked_packet_send_is_canceled_by_shutdown_close_error_and_drop() {
    for mode in 0..4 {
        let mut fixture = Fixture::new(Network::default(), false);
        let id = fixture.peers[0].id;
        fixture.network.sending(id, Send::Blocked);
        for _ in 0..10 {
            fixture.poll();
            tokio::time::advance(Duration::from_millis(1)).await;
        }
        assert!(!fixture.network.0.borrow().endpoints[id]
            .send_wait
            .is_empty());
        let expected = match mode {
            0 => {
                fixture.peers[0]
                    .control
                    .begin(Duration::from_millis(10))
                    .unwrap();
                fixture.poll();
                tokio::time::advance(Duration::from_millis(10)).await;
                io::ErrorKind::TimedOut
            }
            1 => {
                fixture.network.close(id);
                io::ErrorKind::BrokenPipe
            }
            2 => {
                fixture
                    .network
                    .sending(id, Send::Fail(io::ErrorKind::ConnectionReset));
                io::ErrorKind::ConnectionReset
            }
            _ => {
                fixture.peers[0].driver.future = None;
                io::ErrorKind::ConnectionAborted
            }
        };
        fixture.poll();
        assert_eq!(
            fixture.peers[0]
                .control
                .wait()
                .now_or_never()
                .unwrap()
                .unwrap_err()
                .kind(),
            expected
        );
        if mode != 3 {
            assert_eq!(fixture.peers[0].driver.result, Some(Err(expected)));
        }
        fixture.network.sending(id, Send::Ready);
        fixture.poll();
        assert!(fixture.network.take(false).is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn receive_failure_wakes_driver_without_publishing_authentication() {
    let mut fixture = Fixture::new(Network::default(), false);
    fixture.poll();
    let peer = &fixture.peers[1];
    assert!(fixture.network.0.borrow().endpoints[peer.id]
        .receive_wait
        .is_some());
    fixture
        .network
        .fail_receive(peer.id, io::ErrorKind::ConnectionReset);
    fixture.poll();
    let peer = &fixture.peers[1];
    assert_eq!(
        peer.driver.result,
        Some(Err(io::ErrorKind::ConnectionReset))
    );
    assert!(!peer.authenticated);
    assert_eq!(
        peer.control
            .wait()
            .now_or_never()
            .unwrap()
            .unwrap_err()
            .kind(),
        io::ErrorKind::ConnectionReset
    );
}

#[tokio::test(start_paused = true)]
async fn partition_and_stale_packets_cannot_deliver_into_a_replacement_generation() {
    for psk in [false, true] {
        let network = Network::default();
        let mut old = Fixture::new(network.clone(), psk);
        old.establish().await;
        old.peers[0].app.write_all(b"stale").await.unwrap();
        let mut stale = Vec::new();
        for _ in 0..30 {
            old.poll();
            while let Some(packet) = network.take(false) {
                stale.push(packet);
            }
            tokio::time::advance(Duration::from_millis(1)).await;
        }
        assert!(!stale.is_empty());
        let old_ids = old.peers.each_ref().map(|p| p.id);
        let old_control = old.peers[0].control.clone();
        drop(old);
        let mut replacement = Fixture::new(network.clone(), psk);
        replacement.establish().await;
        for id in old_ids {
            network.close(id);
        }
        for packet in stale {
            network.deliver(packet);
        }
        for _ in 0..30 {
            replacement.tick().await;
            replacement.read();
        }
        assert!(replacement
            .peers
            .iter()
            .all(|p| p.received.is_empty() && p.driver.result.is_none()));
        old_control.finish(Ok(crate::noise_shutdown::Receipt { bytes: 5 }));
        assert_eq!(
            old_control.wait().await.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert!(replacement.peers[0].control.wait().now_or_never().is_none());
        replacement.peers[0].app.write_all(b"fresh").await.unwrap();
        // A finite partition drops all traffic; it must not synthesize delivery.
        for _ in 0..100 {
            replacement.poll();
            while network.take(false).is_some() {}
            replacement.read();
            tokio::time::advance(Duration::from_millis(1)).await;
        }
        assert!(replacement.peers[1].received.is_empty());
        for _ in 0..2000 {
            replacement.tick().await;
            replacement.read();
            if replacement.peers[1].received == b"fresh" {
                break;
            }
        }
        assert_eq!(replacement.peers[1].received, b"fresh");
        assert!(replacement.peers[0].received.is_empty());
    }
}

#[test]
fn replay_tlc_socket_driver_terminal_races() {
    use reproto_test_support::verification::exploration;
    let config = include_str!("../../../verification/NoiseSocketDriver.cfg");
    exploration::controls(
        "verification/NoiseSocketDriver.tla",
        "noise-socket-driver",
        config,
        &[
            ("missClose", "TerminalCause"),
            ("lateDeadline", "TerminalCause"),
            ("overwrite", "TerminalCause"),
            ("leakSend", "NoLateEmission"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NoiseSocketDriver.tla",
        "noise-socket-driver",
        config,
    )
    .unwrap();
    for trace in traces {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                let mut fixture = Fixture::new(Network::default(), false);
                let id = fixture.peers[0].id;
                fixture.network.sending(id, Send::Blocked);
                for _ in 0..10 {
                    fixture.poll();
                    tokio::time::advance(Duration::from_millis(1)).await;
                }
                assert!(!fixture.network.0.borrow().endpoints[id]
                    .send_wait
                    .is_empty());
                fixture.peers[0]
                    .control
                    .begin(Duration::from_millis(100))
                    .unwrap();
                fixture.poll();
                for state in trace {
                    match state["event"] {
                        1 => fixture.network.close(id),
                        2 => tokio::time::advance(Duration::from_millis(100)).await,
                        3 => fixture
                            .network
                            .sending(id, Send::Fail(io::ErrorKind::ConnectionReset)),
                        4 if state["failed"] == 0 => fixture.network.sending(id, Send::Ready),
                        4 => (),
                        5 => fixture.peers[0].driver.future = None,
                        6 => fixture.peers[0].driver.poll(),
                        other => panic!("unknown socket event {other}"),
                    }
                    let result = fixture.peers[0]
                        .control
                        .wait()
                        .now_or_never()
                        .map(|result| result.unwrap_err().kind());
                    let expected = match state["result"] {
                        0 => None,
                        1 => Some(io::ErrorKind::BrokenPipe),
                        2 => Some(io::ErrorKind::TimedOut),
                        3 => Some(io::ErrorKind::ConnectionReset),
                        4 => Some(io::ErrorKind::ConnectionAborted),
                        other => panic!("unknown terminal result {other}"),
                    };
                    assert_eq!(result, expected, "{state:?}");
                    assert_eq!(
                        !fixture.network.0.borrow().outgoing.is_empty(),
                        state["emitted"] == 1,
                        "{state:?}"
                    );
                }
            });
    }
}

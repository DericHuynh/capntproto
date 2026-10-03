use super::*;
use crate::transport::socket::DatagramSocket;
use tokio::time::Instant;

const CANDIDATE: &str = "127.0.0.1:2345";

async fn connected(psk: bool) -> Fixture {
    let mut fixture = Fixture::with_mobility(Network::default(), psk, true);
    fixture.establish().await;
    // Exchange the actual NEW_CONNECTION_ID frames before asking quiche to
    // probe. No validation state or spare-CID table is synthesized by the test.
    for _ in 0..30 {
        fixture.tick().await;
    }
    fixture
}

#[tokio::test(start_paused = true)]
async fn candidate_backpressure_preserves_active_rpc_and_migration_deadline() {
    let mut fixture = connected(false).await;
    let socket = fixture.network.bind(CANDIDATE.parse().unwrap());
    let id = socket.id;
    fixture.network.sending(id, Send::Blocked);
    let control = fixture.peers[0].mobility.clone();
    let mut migration = Box::pin(control.migrate_socket(
        DatagramSocket::Simulated(socket),
        ADDRESSES[1].parse().unwrap(),
        Duration::from_millis(200),
    ));
    assert!(migration.as_mut().now_or_never().is_none());
    for _ in 0..20 {
        fixture.tick().await;
    }
    fixture.peers[0].app.write_all(b"live").await.unwrap();
    for _ in 0..100 {
        fixture.tick().await;
        fixture.read();
    }
    assert_eq!(
        fixture.peers[1].received, b"live",
        "candidate send stalled the established RPC path"
    );
    for _ in 0..100 {
        fixture.tick().await;
    }
    assert_eq!(
        migration
            .as_mut()
            .now_or_never()
            .expect("candidate send hid the deadline")
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert!(!fixture.network.0.borrow().endpoints[id].open);
    assert!(fixture.peers.iter().all(|p| p.driver.result.is_none()));
}

// Application IO is deliberately exercised across the real stream bridge and
// encrypted packet engine, including after candidate retirement/commitment.
async fn exchange(fixture: &mut Fixture, byte: u8) {
    let before = fixture.peers.each_ref().map(|p| p.received.len());
    fixture.peers[0].app.write_all(&[byte]).await.unwrap();
    fixture.peers[1].app.write_all(&[byte + 1]).await.unwrap();
    for _ in 0..1000 {
        fixture.tick().await;
        fixture.read();
        if fixture
            .peers
            .iter()
            .zip(before)
            .all(|(p, n)| p.received.len() == n + 1)
        {
            break;
        }
    }
    assert_eq!(&fixture.peers[0].received[before[0]..], &[byte + 1]);
    assert_eq!(&fixture.peers[1].received[before[1]..], &[byte]);
    assert!(fixture.peers.iter().all(|p| p.driver.result.is_none()));
}

#[test]
fn replay_tlc_candidate_io_through_encrypted_driver() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../../../../verification/NativeCandidateIo.cfg");
    exploration::controls(
        "verification/NativeCandidateIo.tla",
        "native-candidate-io",
        config,
        &[
            ("stall", "ActiveProgress"),
            ("early", "Authenticated"),
            ("blockedDeadline", "Completion"),
            ("resurrect", "Authenticated"),
            ("retireActive", "StableCommit"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NativeCandidateIo.tla",
        "native-candidate-io",
        config,
    )
    .unwrap();
    // Both configured Native patterns must obey the same path lifecycle.
    for psk in [false, true] {
        for trace in &traces {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(async {
                    let mut fixture = connected(psk).await;
                    let address = CANDIDATE.parse().unwrap();
                    let socket = fixture.network.bind(address);
                    let id = socket.id;
                    fixture.network.sending(id, Send::Blocked);
                    let control = fixture.peers[0].mobility.clone();
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let mut migration = Some(Box::pin(control.migrate_socket(
                        DatagramSocket::Simulated(socket),
                        ADDRESSES[1].parse().unwrap(),
                        Duration::from_secs(5),
                    )));
                    assert!(migration
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .now_or_never()
                        .is_none());
                    for _ in 0..20 {
                        fixture.tick().await;
                    }
                    assert!(!fixture.network.0.borrow().endpoints[id]
                        .send_wait
                        .is_empty());
                    let mut held = Vec::new();
                    let mut result = 0;
                    for state in trace {
                        match state["event"] {
                            1 => exchange(&mut fixture, 1).await,
                            2 => {
                                fixture.network.sending(id, Send::Ready);
                                // Real PTO retransmission emits the dropped probe.
                                // Hold every encrypted reply to the candidate.
                                for _ in 0..2000 {
                                    fixture.poll();
                                    while let Some(packet) = fixture.network.take(false) {
                                        if packet.to == address {
                                            held.push(packet);
                                        } else {
                                            fixture.network.deliver(packet);
                                        }
                                    }
                                    tokio::time::advance(Duration::from_millis(1)).await;
                                    if !held.is_empty() {
                                        break;
                                    }
                                }
                                assert!(!held.is_empty(), "probe never received a response");
                            }
                            3 => {
                                for packet in &held {
                                    let mut corrupt = packet.clone();
                                    *corrupt.bytes.last_mut().unwrap() ^= 0x80;
                                    fixture.network.deliver(corrupt);
                                }
                                // Do not release a later valid response while
                                // checking that corrupted authentication fails.
                                for _ in 0..20 {
                                    fixture.poll();
                                    while let Some(packet) = fixture.network.take(false) {
                                        if packet.to == address {
                                            held.push(packet);
                                        } else {
                                            fixture.network.deliver(packet);
                                        }
                                    }
                                    tokio::time::advance(Duration::from_millis(1)).await;
                                }
                            }
                            4 | 8 => {
                                for packet in &held {
                                    fixture.network.deliver(packet.clone());
                                    fixture.network.deliver(packet.clone());
                                }
                                for _ in 0..30 {
                                    fixture.tick().await;
                                }
                            }
                            5 => {
                                // A valid response is queued but not polled
                                // until the deadline has elapsed.
                                for packet in &held {
                                    fixture.network.deliver(packet.clone());
                                }
                                tokio::time::advance(
                                    deadline.saturating_duration_since(Instant::now()),
                                )
                                .await;
                                for _ in 0..30 {
                                    fixture.tick().await;
                                }
                            }
                            6 | 9 => {
                                for packet in &held {
                                    fixture.network.deliver(packet.clone());
                                }
                                migration = None;
                                if result == 0 {
                                    result = 3;
                                }
                                for _ in 0..30 {
                                    fixture.tick().await;
                                }
                            }
                            7 => {
                                fixture
                                    .network
                                    .fail_receive(id, io::ErrorKind::ConnectionReset);
                                for _ in 0..30 {
                                    fixture.tick().await;
                                }
                            }
                            other => panic!("unknown candidate event {other}"),
                        }
                        if result == 0 {
                            if let Some(completion) =
                                migration.as_mut().unwrap().as_mut().now_or_never()
                            {
                                result = match completion {
                                    Ok(path) => {
                                        assert_eq!(path.local, address);
                                        assert_eq!(path.peer, ADDRESSES[1].parse().unwrap());
                                        1
                                    }
                                    Err(e) if e.kind() == io::ErrorKind::TimedOut => 2,
                                    Err(e) if e.kind() == io::ErrorKind::ConnectionReset => 4,
                                    other => panic!("unexpected migration outcome {other:?}"),
                                };
                            }
                        }
                        assert_eq!(
                            result, state["result"],
                            "psk={psk}, trace={trace:?}, state={state:?}"
                        );
                        let delivered = state["delivered"] == 1;
                        assert_eq!(
                            fixture.peers[0].received.as_slice(),
                            if delivered { &[2][..] } else { &[] }
                        );
                        assert_eq!(
                            fixture.peers[1].received.as_slice(),
                            if delivered { &[1][..] } else { &[] }
                        );
                        let network = fixture.network.0.borrow();
                        assert_eq!(network.endpoints[id].open, state["phase"] != 3, "{state:?}");
                        assert_eq!(
                            network.endpoints[fixture.peers[0].id].open,
                            state["phase"] != 2,
                            "{state:?}"
                        );
                        drop(network);
                        assert!(fixture.peers.iter().all(|p| p.driver.result.is_none()));
                    }
                    // Each terminal trace must retain a usable authenticated
                    // stream on the path selected by the actual runtime.
                    if result != 0 {
                        exchange(&mut fixture, 3).await;
                    }
                });
        }
    }
}

#[tokio::test(start_paused = true)]
async fn candidate_send_failure_allows_retry_and_shutdown_on_new_path() {
    for psk in [false, true] {
        let mut fixture = connected(psk).await;
        let old = fixture.peers[0].id;
        let candidate = fixture.network.bind(CANDIDATE.parse().unwrap());
        let failed = candidate.id;
        fixture
            .network
            .sending(failed, Send::Fail(io::ErrorKind::PermissionDenied));
        let control = fixture.peers[0].mobility.clone();
        let mut attempt = Box::pin(control.migrate_socket(
            DatagramSocket::Simulated(candidate),
            ADDRESSES[1].parse().unwrap(),
            Duration::from_secs(2),
        ));
        assert!(attempt.as_mut().now_or_never().is_none());
        for _ in 0..30 {
            fixture.tick().await;
        }
        assert_eq!(
            attempt.as_mut().now_or_never().unwrap().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!fixture.network.0.borrow().endpoints[failed].open);
        exchange(&mut fixture, 5).await;

        // Retry uses another concrete path; quiche may still emit old probes.
        let address = "127.0.0.1:3456".parse().unwrap();
        let candidate = fixture.network.bind(address);
        let active = candidate.id;
        let mut retry = Box::pin(control.migrate_socket(
            DatagramSocket::Simulated(candidate),
            ADDRESSES[1].parse().unwrap(),
            Duration::from_secs(2),
        ));
        assert!(retry.as_mut().now_or_never().is_none());
        let mut path = None;
        for _ in 0..2000 {
            fixture.tick().await;
            if let Some(result) = retry.as_mut().now_or_never() {
                path = Some(result.unwrap());
                break;
            }
        }
        assert_eq!(path.unwrap().local, address);
        assert!(!fixture.network.0.borrow().endpoints[old].open);
        assert!(fixture.network.0.borrow().endpoints[active].open);
        exchange(&mut fixture, 7).await;
        for peer in &mut fixture.peers {
            peer.control.begin(Duration::from_secs(1)).unwrap();
            peer.app.shutdown().await.unwrap();
        }
        for _ in 0..2000 {
            fixture.tick().await;
            fixture.read();
            if fixture.peers.iter().all(|p| p.driver.result.is_some()) {
                break;
            }
        }
        for peer in &fixture.peers {
            assert_eq!(peer.driver.result, Some(Ok(())));
            assert_eq!(
                peer.control.wait().now_or_never().unwrap().unwrap().bytes,
                2
            );
        }
        assert!(!fixture.network.0.borrow().endpoints[active].open);
    }
}

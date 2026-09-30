use super::*;
use crate::transport::socket::DatagramSocket;

const STUN: &str = "127.0.0.1:3478";

// Build a wire response independently of the production STUN parser. The ID
// comes from an actually emitted request; no transaction state is injected.
pub(super) fn response(request: &[u8], address: SocketAddr) -> Vec<u8> {
    assert_eq!(request.len(), 20);
    assert_eq!(&request[..8], &[0, 1, 0, 0, 0x21, 0x12, 0xa4, 0x42]);
    let mut bytes = request.to_vec();
    bytes[0] = 1;
    match address.ip() {
        std::net::IpAddr::V4(ip) => {
            bytes[2..4].copy_from_slice(&12u16.to_be_bytes());
            bytes.extend_from_slice(&[0, 0x20, 0, 8, 0, 1]);
            bytes.extend_from_slice(&(address.port() ^ 0x2112).to_be_bytes());
            bytes.extend(
                ip.octets()
                    .into_iter()
                    .zip([0x21, 0x12, 0xa4, 0x42])
                    .map(|(a, b)| a ^ b),
            );
        }
        std::net::IpAddr::V6(ip) => {
            bytes[2..4].copy_from_slice(&24u16.to_be_bytes());
            bytes.extend_from_slice(&[0, 0x20, 0, 20, 0, 2]);
            bytes.extend_from_slice(&(address.port() ^ 0x2112).to_be_bytes());
            bytes.extend(
                ip.octets()
                    .into_iter()
                    .zip(request[4..20].iter())
                    .map(|(a, b)| a ^ b),
            );
        }
    }
    bytes
}

#[tokio::test(start_paused = true)]
async fn blocked_discovery_send_cannot_hide_its_deadline() {
    let network = Network::default();
    let raw = network.bind(ADDRESSES[0].parse().unwrap());
    let id = raw.id;
    network.sending(id, Send::Blocked);
    let socket = DatagramSocket::Simulated(raw);
    let mut discovery = Box::pin(crate::nat::discover_on(
        &socket,
        STUN.parse().unwrap(),
        Duration::from_millis(50),
    ));
    assert!(discovery.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(discovery.as_mut().now_or_never().is_none());
    assert!(!network.0.borrow().endpoints[id].send_wait.is_empty());
    tokio::time::advance(Duration::from_millis(50)).await;
    assert_eq!(
        discovery
            .as_mut()
            .now_or_never()
            .expect("blocked send hid discovery deadline")
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    network.sending(id, Send::Ready);
    assert!(
        network.take(false).is_none(),
        "expired discovery emitted a late probe"
    );
}

#[test]
fn replay_tlc_discovery_io_deadlines_and_replies() {
    use reproto_test_support::verification::exploration;
    let config = include_str!("../../../../verification/NatDiscoveryIo.cfg");
    exploration::controls(
        "verification/NatDiscoveryIo.tla",
        "nat-discovery-io",
        config,
        &[
            ("blockedDeadline", "Completion"),
            ("foreign", "Completion"),
            ("lateDeadline", "Completion"),
            ("lateSend", "NoLateSend"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NatDiscoveryIo.tla",
        "nat-discovery-io",
        config,
    )
    .unwrap();
    for address in ["192.0.2.42:43123", "[2001:db8::42]:43123"] {
        let address: SocketAddr = address.parse().unwrap();
        for trace in &traces {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(async {
                    let network = Network::default();
                    let raw = network.bind(ADDRESSES[0].parse().unwrap());
                    let id = raw.id;
                    network.sending(id, Send::Blocked);
                    let socket = DatagramSocket::Simulated(raw);
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
                    let mut discovery = Some(Box::pin(crate::nat::discover_on(
                        &socket,
                        STUN.parse().unwrap(),
                        Duration::from_secs(2),
                    )));
                    assert!(discovery
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .now_or_never()
                        .is_none());
                    tokio::time::advance(Duration::from_millis(1)).await;
                    assert!(discovery
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .now_or_never()
                        .is_none());
                    let mut sent: Vec<Packet> = Vec::new();
                    let mut result = 0;
                    for state in trace {
                        let reply = || Packet {
                            from: STUN.parse().unwrap(),
                            to: ADDRESSES[0].parse().unwrap(),
                            bytes: response(&sent[0].bytes, address),
                        };
                        match state["event"] {
                            1 => network.sending(id, Send::Ready),
                            2 => network.deliver(reply()),
                            3 => {
                                let mut foreign = reply();
                                foreign.from = "127.0.0.1:3479".parse().unwrap();
                                network.deliver(foreign);
                                let mut stale = reply();
                                stale.bytes[8] ^= 1;
                                network.deliver(stale);
                                let mut malformed = reply();
                                malformed.bytes.truncate(25);
                                network.deliver(malformed);
                                let mut oversized = reply();
                                oversized.bytes.resize(1026, 0);
                                network.deliver(oversized);
                            }
                            4 | 8 => {
                                if state["event"] == 8 {
                                    network.deliver(reply());
                                }
                                tokio::time::advance(
                                    deadline.saturating_duration_since(tokio::time::Instant::now()),
                                )
                                .await;
                            }
                            5 => network.sending(id, Send::Fail(io::ErrorKind::ConnectionReset)),
                            6 => {
                                discovery = None;
                                result = 4;
                            }
                            7 => {
                                network.sending(id, Send::Ready);
                                if !sent.is_empty() {
                                    network.deliver(reply());
                                }
                                tokio::time::advance(Duration::from_secs(1)).await;
                            }
                            9 => tokio::time::advance(Duration::from_millis(500)).await,
                            other => panic!("unknown discovery event {other}"),
                        }
                        if let Some(future) = &mut discovery {
                            if let Some(completion) = future.as_mut().now_or_never() {
                                result = match completion {
                                    Ok(observed) => {
                                        assert_eq!(observed, address);
                                        1
                                    }
                                    Err(e) if e.kind() == io::ErrorKind::TimedOut => 2,
                                    Err(e) if e.kind() == io::ErrorKind::ConnectionReset => 3,
                                    other => panic!("unexpected discovery completion {other:?}"),
                                };
                                discovery = None;
                            }
                        }
                        while let Some(packet) = network.take(false) {
                            assert_eq!(packet.from, socket.local_addr().unwrap());
                            assert_eq!(packet.to, STUN.parse().unwrap());
                            if let Some(first) = sent.first() {
                                assert_eq!(packet.bytes, first.bytes, "retry changed transaction");
                            }
                            sent.push(packet);
                        }
                        assert_eq!(result, state["result"], "{state:?}, trace={trace:?}");
                        assert_eq!(sent.len() as u64, state["emitted"], "{state:?}");
                    }
                });
        }
    }
}

#[tokio::test(start_paused = true)]
async fn reply_can_complete_discovery_while_a_retransmission_is_blocked() {
    let network = Network::default();
    let raw = network.bind(ADDRESSES[0].parse().unwrap());
    let id = raw.id;
    let socket = DatagramSocket::Simulated(raw);
    let mut discovery = Box::pin(crate::nat::discover_on(
        &socket,
        STUN.parse().unwrap(),
        Duration::from_secs(2),
    ));
    assert!(discovery.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(discovery.as_mut().now_or_never().is_none());
    let request = network.take(false).unwrap();
    network.sending(id, Send::Blocked);
    tokio::time::advance(Duration::from_millis(500)).await;
    assert!(discovery.as_mut().now_or_never().is_none());
    let address = "192.0.2.43:1234".parse().unwrap();
    network.deliver(Packet {
        from: request.to,
        to: request.from,
        bytes: response(&request.bytes, address),
    });
    assert_eq!(discovery.as_mut().now_or_never().unwrap().unwrap(), address);
    network.sending(id, Send::Ready);
    assert!(network.take(false).is_none());
}

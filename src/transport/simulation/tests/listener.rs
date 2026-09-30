//! The real shared receive loop, reservations and spawned transport drivers.
use super::*;
use crate::{
    noise_listener::{self, Limits, Listener, Reservation},
    transport::{socket::DatagramSocket, AuthenticatedSession},
};

mod mapping;

async fn tick(network: &Network) {
    tokio::task::yield_now().await;
    while let Some(packet) = network.take(false) {
        network.deliver(packet);
    }
    tokio::time::advance(Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
}

struct Route {
    target: noise_listener::Target,
    client: AuthenticatedSession,
    server: Option<AuthenticatedSession>,
    received: Vec<u8>,
}
struct Harness {
    network: Network,
    socket: usize,
    listener: Option<Listener>,
    routes: Vec<Route>,
    pending: Option<Reservation>,
    deadline: tokio::time::Instant,
}
impl Harness {
    async fn new(psk: bool) -> Self {
        let network = Network::default();
        let socket = network.bind(ADDRESSES[1].parse().unwrap());
        let id = socket.id;
        let host = Rc::new(Identity::from_private_key([11; 32]).unwrap());
        let listener = Listener::start(
            Rc::new(DatagramSocket::Simulated(socket)),
            host,
            Limits {
                routes: 3,
                queue: 2,
                reservation_timeout: Duration::from_secs(2),
            },
        );
        let mut routes = Vec::new();
        for (index, address) in [ADDRESSES[0], "127.0.0.1:2345"].into_iter().enumerate() {
            let identity = Identity::from_private_key([index as u8 + 7; 32]).unwrap();
            let key = psk.then_some([index as u8 + 21; 32]);
            let reservation = listener
                .reserve(identity.public_key(), key, b"shared simulation")
                .unwrap();
            let target = reservation.target();
            let socket = DatagramSocket::Simulated(network.bind(address.parse().unwrap()));
            let client = tokio::task::spawn_local(async move {
                noise_listener::connect_socket(socket, target, &identity, key, b"shared simulation")
                    .await
            });
            let server = tokio::task::spawn_local(reservation.accept());
            for _ in 0..2000 {
                tick(&network).await;
                if client.is_finished() && server.is_finished() {
                    break;
                }
            }
            assert!(
                client.is_finished() && server.is_finished(),
                "shared authentication stalled"
            );
            routes.push(Route {
                target,
                client: client.await.unwrap().unwrap(),
                server: Some(server.await.unwrap().unwrap()),
                received: Vec::new(),
            });
        }
        assert_eq!(listener.stats().authenticated, 2);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let pending = listener.reserve([12; 32], None, b"pending").unwrap();
        Self {
            network,
            socket: id,
            listener: Some(listener),
            routes,
            pending: Some(pending),
            deadline,
        }
    }
    async fn settle(&mut self, turns: usize) {
        for _ in 0..turns {
            tick(&self.network).await;
            for route in &mut self.routes {
                let mut bytes = [0; 16];
                if let Some(result) = route
                    .client
                    .io
                    .as_mut()
                    .unwrap()
                    .read(&mut bytes)
                    .now_or_never()
                {
                    let n = result.unwrap();
                    route.received.extend_from_slice(&bytes[..n]);
                }
            }
        }
    }
    async fn block(&mut self) {
        self.network.sending(self.socket, Send::Blocked);
        for (i, route) in self.routes.iter_mut().enumerate() {
            route
                .server
                .as_mut()
                .unwrap()
                .io
                .as_mut()
                .unwrap()
                .write_all(&[i as u8 + 1])
                .await
                .unwrap();
        }
        self.settle(30).await;
        assert_eq!(
            self.network.0.borrow().endpoints[self.socket]
                .send_wait
                .len(),
            2
        );
        assert!(self.routes.iter().all(|r| r.received.is_empty()));
    }
}

#[tokio::test(start_paused = true)]
async fn shared_demultiplexing_never_treats_routing_ids_as_authority() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for psk in [false, true] {
                let mut h = Harness::new(psk).await;
                let target = h.pending.as_ref().unwrap().target();
                // Saturate an unaccepted route's two-packet queue. Oversized,
                // unknown-CID and STUN packets share the same receive boundary.
                for i in 0..128 {
                    let mut bytes = vec![0x40];
                    bytes.extend_from_slice(if i % 2 == 0 {
                        &target.connection_id
                    } else {
                        &[99; 16]
                    });
                    bytes.extend_from_slice(b"no authentication");
                    if i % 4 == 0 {
                        bytes.resize(noise_listener::MAX_PACKET_BYTES + 2, 0);
                    }
                    if i % 4 == 1 {
                        bytes = vec![0; 20];
                        bytes[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
                    }
                    h.network.deliver(Packet {
                        from: "127.0.0.1:9999".parse().unwrap(),
                        to: target.address,
                        bytes,
                    });
                }
                h.settle(30).await;
                assert_eq!(h.listener.as_ref().unwrap().stats().pending, 1);
                assert_eq!(h.listener.as_ref().unwrap().stats().authenticated, 2);

                h.routes[0]
                    .client
                    .io
                    .as_mut()
                    .unwrap()
                    .write_all(&[77])
                    .await
                    .unwrap();
                let mut held = Vec::new();
                for _ in 0..30 {
                    tokio::task::yield_now().await;
                    while let Some(packet) = h.network.take(false) {
                        if packet.from == ADDRESSES[0].parse::<SocketAddr>().unwrap() {
                            // Short headers use a 16-byte destination CID. Altering
                            // only that routing field cannot transfer authority.
                            assert_eq!(packet.bytes[0] & 0x80, 0);
                            let mut forged = packet.clone();
                            forged.bytes[1..17].copy_from_slice(&h.routes[1].target.connection_id);
                            h.network.deliver(forged);
                            held.push(packet);
                        } else {
                            h.network.deliver(packet);
                        }
                    }
                    tokio::time::advance(Duration::from_millis(1)).await;
                }
                assert!(!held.is_empty());
                let mut bytes = [0; 16];
                for route in &mut h.routes {
                    let server = route.server.as_mut().unwrap();
                    assert!(!server.driver.is_finished());
                    assert!(server
                        .io
                        .as_mut()
                        .unwrap()
                        .read(&mut bytes)
                        .now_or_never()
                        .is_none());
                }
                for packet in held {
                    h.network.deliver(packet.clone());
                    h.network.deliver(packet);
                }
                h.settle(100).await;
                let server = h.routes[0].server.as_mut().unwrap();
                let n = server
                    .io
                    .as_mut()
                    .unwrap()
                    .read(&mut bytes)
                    .now_or_never()
                    .unwrap()
                    .unwrap();
                assert_eq!(&bytes[..n], &[77]);
                assert!(server
                    .io
                    .as_mut()
                    .unwrap()
                    .read(&mut bytes)
                    .now_or_never()
                    .is_none());
                assert!(h.routes[1]
                    .server
                    .as_mut()
                    .unwrap()
                    .io
                    .as_mut()
                    .unwrap()
                    .read(&mut bytes)
                    .now_or_never()
                    .is_none());
                h.routes[1]
                    .server
                    .as_mut()
                    .unwrap()
                    .io
                    .as_mut()
                    .unwrap()
                    .write_all(&[88])
                    .await
                    .unwrap();
                h.settle(100).await;
                assert_eq!(h.routes[1].received, [88]);
                assert!(h.routes[0].received.is_empty());
            }
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn shared_socket_wakes_all_blocked_sessions_and_closes_them() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for psk in [false, true] {
                let mut h = Harness::new(psk).await;
                h.block().await;
                h.network.sending(h.socket, Send::Ready);
                h.settle(100).await;
                assert_eq!(h.routes[0].received, [1]);
                assert_eq!(h.routes[1].received, [2]);
                h.listener.as_ref().unwrap().close();
                h.settle(10).await;
                for route in &h.routes {
                    assert_eq!(
                        route
                            .server
                            .as_ref()
                            .unwrap()
                            .shutdown
                            .wait()
                            .now_or_never()
                            .unwrap()
                            .unwrap_err()
                            .kind(),
                        io::ErrorKind::BrokenPipe
                    );
                }
            }
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn failed_authentication_and_retired_ids_cannot_poison_replacement_reservations() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for psk in [false, true] {
                let mut h = Harness::new(psk).await;
                tokio::time::advance(
                    h.deadline
                        .saturating_duration_since(tokio::time::Instant::now()),
                )
                .await;
                h.settle(1).await;
                let stale = h.pending.take().unwrap();
                let listener = h.listener.as_ref().unwrap().clone();
                let identity = Rc::new(Identity::from_private_key([14; 32]).unwrap());
                let expected_peer = if psk {
                    identity.public_key()
                } else {
                    Identity::from_private_key([15; 32]).unwrap().public_key()
                };
                let reservation = listener
                    .reserve(expected_peer, psk.then_some([61; 32]), b"auth")
                    .unwrap();
                let target = reservation.target();
                drop(stale);
                assert_eq!(listener.stats().pending, 1);
                let address = "127.0.0.1:3456".parse().unwrap();
                let socket = DatagramSocket::Simulated(h.network.bind(address));
                let caller = identity.clone();
                let client = tokio::task::spawn_local(async move {
                    noise_listener::connect_socket(
                        socket,
                        target,
                        &caller,
                        psk.then_some([62; 32]),
                        b"auth",
                    )
                    .await
                });
                let server = tokio::task::spawn_local(reservation.accept());
                let mut stale_packets = Vec::new();
                for _ in 0..2200 {
                    tokio::task::yield_now().await;
                    while let Some(packet) = h.network.take(false) {
                        if packet.from == address {
                            stale_packets.push(packet.clone());
                        }
                        h.network.deliver(packet);
                    }
                    tokio::time::advance(Duration::from_millis(1)).await;
                    if server.is_finished() {
                        break;
                    }
                }
                assert!(server.is_finished(), "reservation did not reject or expire");
                assert!(
                    server.await.unwrap().is_err(),
                    "unauthorized Noise peer published"
                );
                assert!(!stale_packets.is_empty());
                client.abort();
                // Even an unexpectedly completed client must release its session.
                drop(client.await);
                h.settle(10).await;
                assert_eq!(listener.stats().authenticated, 2);
                assert_eq!(listener.stats().pending, 0);

                let key = psk.then_some([61; 32]);
                let fresh = listener
                    .reserve(identity.public_key(), key, b"auth")
                    .unwrap();
                let new_target = fresh.target();
                assert_ne!(target.connection_id, new_target.connection_id);
                for packet in stale_packets {
                    h.network.deliver(packet);
                }
                let socket = DatagramSocket::Simulated(h.network.bind(address));
                let client = tokio::task::spawn_local(async move {
                    noise_listener::connect_socket(socket, new_target, &identity, key, b"auth")
                        .await
                });
                let server = tokio::task::spawn_local(fresh.accept());
                for _ in 0..2000 {
                    h.settle(1).await;
                    if client.is_finished() && server.is_finished() {
                        break;
                    }
                }
                assert!(client.is_finished() && server.is_finished());
                let mut client = client.await.unwrap().unwrap();
                let mut server = server.await.unwrap().unwrap();
                assert_eq!(listener.stats().authenticated, 3);
                client
                    .io
                    .as_mut()
                    .unwrap()
                    .write_all(b"fresh")
                    .await
                    .unwrap();
                h.settle(30).await;
                let mut bytes = [0; 16];
                let n = server
                    .io
                    .as_mut()
                    .unwrap()
                    .read(&mut bytes)
                    .now_or_never()
                    .unwrap()
                    .unwrap();
                assert_eq!(&bytes[..n], b"fresh");
                assert!(h
                    .routes
                    .iter()
                    .all(|r| !r.server.as_ref().unwrap().driver.is_finished()
                        && r.received.is_empty()));
            }
        })
        .await;
}

#[test]
fn shared_send_readiness_wakes_each_task_without_emitting_canceled_sends() {
    for mode in 0..3 {
        let network = Network::default();
        let socket = Rc::new(network.bind(ADDRESSES[0].parse().unwrap()));
        network.sending(socket.id, Send::Blocked);
        let mut tasks: [Task; 3] = std::array::from_fn(|i| {
            let socket = socket.clone();
            Task::new(async move {
                socket
                    .send_to(&[i as u8], ADDRESSES[1].parse().unwrap())
                    .await?;
                Ok(())
            })
        });
        for task in &mut tasks {
            task.poll();
            assert!(!task.ready.0.load(Ordering::SeqCst));
        }
        tasks[0].future = None;
        match mode {
            0 => network.sending(socket.id, Send::Ready),
            1 => network.sending(socket.id, Send::Fail(io::ErrorKind::PermissionDenied)),
            _ => network.close(socket.id),
        }
        for task in &mut tasks[1..] {
            assert!(
                task.ready.0.load(Ordering::SeqCst),
                "lost shared socket wakeup"
            );
            task.poll();
            assert_eq!(
                task.result,
                Some(match mode {
                    0 => Ok(()),
                    1 => Err(io::ErrorKind::PermissionDenied),
                    _ => Err(io::ErrorKind::BrokenPipe),
                })
            );
        }
        if mode == 0 {
            assert_eq!(network.take(false).unwrap().bytes, [1]);
            assert_eq!(network.take(false).unwrap().bytes, [2]);
        }
        assert!(network.take(false).is_none());
    }
}

#[test]
fn replay_tlc_shared_listener_io_lifecycle() {
    use reproto_test_support::verification::exploration;
    let config = include_str!("../../../../verification/NoiseListenerIo.cfg");
    exploration::controls(
        "verification/NoiseListenerIo.tla",
        "noise-listener-io",
        config,
        &[
            ("lostWake", "Progress"),
            ("sibling", "Isolation"),
            ("admission", "Admission"),
            ("expiry", "Expiry"),
            ("close", "Cleanup"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NoiseListenerIo.tla",
        "noise-listener-io",
        config,
    )
    .unwrap();
    for psk in [false, true] {
        for trace in &traces {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(tokio::task::LocalSet::new().run_until(async {
                    let mut h = Harness::new(psk).await;
                    h.block().await;
                    for state in trace {
                        match state["event"] {
                            1 => h.network.sending(h.socket, Send::Ready),
                            2 => {
                                h.routes[0].server.take();
                            }
                            3 => h.listener.as_ref().unwrap().stop_accepting(),
                            4 => {
                                tokio::time::advance(
                                    h.deadline
                                        .saturating_duration_since(tokio::time::Instant::now()),
                                )
                                .await
                            }
                            5 => h
                                .network
                                .fail_receive(h.socket, io::ErrorKind::ConnectionRefused),
                            6 => h.listener.as_ref().unwrap().close(),
                            7 => h
                                .network
                                .fail_receive(h.socket, io::ErrorKind::PermissionDenied),
                            8 => {
                                h.listener.take();
                            }
                            other => panic!("unknown listener event {other}"),
                        }
                        h.settle(30).await;
                        for (index, key) in ["aliveA", "aliveB"].into_iter().enumerate() {
                            let live = h.routes[index]
                                .server
                                .as_ref()
                                .is_some_and(|s| !s.driver.is_finished());
                            assert_eq!(
                                live,
                                state[key] == 1,
                                "psk={psk}, state={state:?}, trace={trace:?}"
                            );
                        }
                        assert_eq!(
                            h.routes[0].received.as_slice(),
                            if state["deliveredA"] == 1 {
                                &[1][..]
                            } else {
                                &[]
                            }
                        );
                        assert_eq!(
                            h.routes[1].received.as_slice(),
                            if state["deliveredB"] == 1 {
                                &[2][..]
                            } else {
                                &[]
                            }
                        );
                        if let Some(listener) = &h.listener {
                            let stats = listener.stats();
                            assert_eq!(stats.closed, state["closed"] == 1);
                            assert_eq!(
                                stats.accepting,
                                state["closed"] == 0 && state["draining"] == 0
                            );
                            assert_eq!(stats.pending as u64, state["pending"]);
                            assert_eq!(
                                stats.authenticated as u64,
                                state["aliveA"] + state["aliveB"]
                            );
                            if !stats.accepting {
                                assert_eq!(
                                    listener
                                        .reserve([13; 32], None, b"late")
                                        .err()
                                        .unwrap()
                                        .kind(),
                                    io::ErrorKind::BrokenPipe
                                );
                            }
                        }
                    }
                    // A retained reservation cannot publish after retirement.
                    if trace.last().unwrap()["pending"] == 0 {
                        assert!(h.pending.take().unwrap().accept().await.is_err());
                    }
                }));
        }
    }
}

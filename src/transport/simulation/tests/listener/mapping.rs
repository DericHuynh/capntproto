//! Compose actual STUN packets, listener IO, mapping observers and capability
//! publication. Reported addresses are hints; no NAT reachability is assumed.
use super::super::nat::response;
use super::*;
use crate::{
    nat::{MappingOptions, MappingStatus},
    native_discovery::{
        self, Advertisement, AdvertisementOptions, AdvertisementStatus, AdvertisementStop,
        Directory, MappedService,
    },
};

const STUN: &str = "127.0.0.1:3478";

#[tokio::test(start_paused = true)]
async fn mapping_send_backpressure_retries_but_hard_failure_withdraws() {
    use crate::nat::{Mapping, MappingState};
    use crate::transport::socket::DatagramSocket;

    let network = Network::default();
    let socket = network.bind("127.0.0.1:54321".parse().unwrap());
    let id = socket.id;
    let socket = DatagramSocket::Simulated(socket);
    let options = MappingOptions::default();
    let state = MappingState::new(STUN.parse().unwrap(), options).unwrap();
    let mapping = Mapping::new(state.clone());
    network.sending(id, Send::Blocked);
    state.tick(&socket, tokio::time::Instant::now());
    assert_eq!(mapping.status(), MappingStatus::Discovering);
    assert!(network.take(false).is_none());

    network.sending(id, Send::Ready);
    tokio::time::advance(Duration::from_millis(500)).await;
    state.tick(&socket, tokio::time::Instant::now());
    let first = network.take(false).unwrap();
    assert_eq!(first.to, STUN.parse().unwrap());

    network.sending(id, Send::Fail(io::ErrorKind::NetworkUnreachable));
    tokio::time::advance(Duration::from_secs(1)).await;
    state.tick(&socket, tokio::time::Instant::now());
    assert_eq!(mapping.status(), MappingStatus::Unavailable);
    assert!(network.take(false).is_none());

    // A hard error retires the transaction; recovery starts a fresh one only
    // after the refresh interval, rather than spinning on the failed socket.
    network.sending(id, Send::Ready);
    state.tick(&socket, tokio::time::Instant::now());
    assert!(network.take(false).is_none());
    tokio::time::advance(options.interval).await;
    state.tick(&socket, tokio::time::Instant::now());
    let next = network.take(false).unwrap();
    assert_ne!(&first.bytes[8..20], &next.bytes[8..20]);
    state.receive(
        &response(&next.bytes, observed(1)),
        STUN.parse().unwrap(),
        tokio::time::Instant::now(),
    );
    assert_eq!(mapping.address(), Some(observed(1)));
}

fn observed(number: u64) -> SocketAddr {
    SocketAddr::from(([192, 0, 2, 42], 4000 + number as u16))
}

async fn advance_with_traffic(h: &mut Harness, duration: Duration) {
    let mut remaining = duration;
    while !remaining.is_zero() {
        let step = remaining.min(Duration::from_secs(2));
        tokio::time::advance(step).await;
        remaining -= step;
        let lengths: Vec<_> = h.routes.iter().map(|r| r.received.len()).collect();
        for route in &mut h.routes {
            route
                .server
                .as_mut()
                .unwrap()
                .io
                .as_mut()
                .unwrap()
                .write_all(&[42])
                .await
                .unwrap();
        }
        h.settle(30).await;
        for (route, length) in h.routes.iter().zip(lengths) {
            assert_eq!(&route.received[length..], &[42]);
            assert!(!route.server.as_ref().unwrap().driver.is_finished());
        }
    }
}

fn drain_requests(server: &Socket) -> Vec<Vec<u8>> {
    let mut requests = Vec::new();
    let mut bytes = [0; 128];
    while let Some(result) = server.recv_from(&mut bytes).now_or_never() {
        let (n, from) = result.unwrap();
        assert_eq!(
            from,
            ADDRESSES[1].parse().unwrap(),
            "probe left a different socket"
        );
        assert_eq!(n, 20);
        requests.push(bytes[..n].to_vec());
    }
    requests
}

#[tokio::test(start_paused = true)]
async fn blocked_mapping_expiry_and_replacement_preserve_shared_sessions() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for psk in [false, true] {
                let mut h = Harness::new(psk).await;
                let server = h.network.bind(STUN.parse().unwrap());
                let listener = h.listener.as_ref().unwrap().clone();
                let options = MappingOptions {
                    timeout: Duration::from_millis(250),
                    ..Default::default()
                };
                let mapping = listener
                    .maintain_mapping(server.local_addr().unwrap(), options)
                    .unwrap();
                h.settle(150).await;
                let first = drain_requests(&server).pop().unwrap();
                h.block().await;
                h.settle(400).await;
                assert_eq!(mapping.status(), MappingStatus::Unavailable);
                assert!(h
                    .routes
                    .iter()
                    .all(|r| !r.server.as_ref().unwrap().driver.is_finished()));
                mapping.close();
                let replacement = listener
                    .maintain_mapping(server.local_addr().unwrap(), options)
                    .unwrap();
                h.network.sending(h.socket, Send::Ready);
                h.settle(150).await;
                let current = drain_requests(&server).pop().unwrap();
                assert_ne!(first[8..], current[8..]);
                server
                    .send_to(
                        &response(&first, observed(1)),
                        listener.local_addr().unwrap(),
                    )
                    .await
                    .unwrap();
                h.settle(5).await;
                assert_eq!(
                    replacement.address(),
                    None,
                    "retired mapping response accepted"
                );
                drop(mapping);
                server
                    .send_to(
                        &response(&current, observed(2)),
                        listener.local_addr().unwrap(),
                    )
                    .await
                    .unwrap();
                h.settle(5).await;
                assert_eq!(replacement.address(), Some(observed(2)));
                assert_eq!(h.routes[0].received, [1]);
                assert_eq!(h.routes[1].received, [2]);
                listener.stop_accepting();
                assert_eq!(replacement.address(), Some(observed(2)));
                listener.close();
                h.settle(5).await;
                assert_eq!(replacement.status(), MappingStatus::Stopped);
                assert!(h
                    .routes
                    .iter()
                    .all(|r| r.server.as_ref().unwrap().driver.is_finished()));
            }
        })
        .await;
}

#[test]
fn replay_tlc_mapping_packets_and_discovery_publications() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../../../../../verification/NativeMappingRefresh.cfg");
    let traces = exploration::traces(
        "verification/NativeMappingRefresh.tla",
        "native-mapping-socket",
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
                    let server = h.network.bind(STUN.parse().unwrap());
                    let listener = h.listener.as_ref().unwrap().clone();
                    let mapping = listener
                        .maintain_mapping(server.local_addr().unwrap(), MappingOptions::default())
                        .unwrap();
                    let (_network, handle) = crate::native_rpc::Network::new(listener.identity());
                    let service = MappedService::new(listener.clone(), handle, &mapping).unwrap();
                    let directory = Directory::default();
                    let recipient = h.routes[0].client.local;
                    let reader = directory.client(recipient);
                    let mut ad: Option<Advertisement> = None;
                    let mut first = Vec::new();
                    let mut current = Vec::new();
                    let mut prior = None;
                    let mut prior_address = None;
                    for state in trace {
                        match state["event"] {
                            1 => {
                                if state["request"] == 1 {
                                    h.settle(150).await;
                                } else {
                                    drain_requests(&server);
                                    advance_with_traffic(&mut h, Duration::from_secs(16)).await;
                                }
                                let requests = drain_requests(&server);
                                assert!(!requests.is_empty(), "listener did not emit the probe");
                                current = requests[0].clone();
                                assert!(
                                    requests.iter().all(|r| *r == current),
                                    "retry changed transaction"
                                );
                                if first.is_empty() {
                                    first = current.clone();
                                } else {
                                    assert_ne!(first[8..], current[8..]);
                                }
                            }
                            2 => {
                                server
                                    .send_to(
                                        &response(&current, observed(state["request"])),
                                        listener.local_addr().unwrap(),
                                    )
                                    .await
                                    .unwrap();
                            }
                            3 => {
                                advance_with_traffic(&mut h, Duration::from_secs(3)).await;
                                drain_requests(&server);
                            }
                            4 => {
                                server
                                    .send_to(
                                        &response(&first, observed(1)),
                                        listener.local_addr().unwrap(),
                                    )
                                    .await
                                    .unwrap();
                            }
                            5 => h.network.deliver(Packet {
                                from: "127.0.0.1:3479".parse().unwrap(),
                                to: listener.local_addr().unwrap(),
                                bytes: response(&current, observed(state["request"])),
                            }),
                            6 => mapping.close(),
                            other => panic!("unknown mapping event {other}"),
                        }
                        h.settle(5).await;
                        let address = (state["observed"] != 0).then(|| observed(state["observed"]));
                        assert_eq!(
                            mapping.address(),
                            address,
                            "psk={psk}, state={state:?}, trace={trace:?}"
                        );
                        assert_eq!(
                            mapping.status() == MappingStatus::Stopped,
                            state["closed"] == 1
                        );
                        if ad.is_none() && address.is_some() {
                            ad = Some(
                                service
                                    .advertise(
                                        &directory,
                                        "service",
                                        recipient,
                                        b"mapped simulation",
                                        AdvertisementOptions {
                                            lifetime: Duration::from_secs(60),
                                            ..Default::default()
                                        },
                                    )
                                    .unwrap(),
                            );
                        }
                        if let Some(ad) = &ad {
                            match ad.status() {
                                AdvertisementStatus::Published {
                                    address: published, ..
                                } => assert_eq!(Some(published), address),
                                AdvertisementStatus::Suspended { .. } => {
                                    assert!(address.is_none());
                                    assert_eq!(state["closed"], 0);
                                }
                                AdvertisementStatus::Stopped(reason) => {
                                    assert_eq!(reason, AdvertisementStop::Mapping);
                                    assert_eq!(state["closed"], 1);
                                }
                            }
                        }
                        let resolved =
                            native_discovery::resolve(&reader, recipient, "service").await;
                        if let Some(address) = address {
                            let resolved = resolved.unwrap();
                            assert_eq!(resolved.binding().address, address);
                            assert_eq!(resolved.binding().host, listener.identity());
                            assert_eq!(resolved.binding().recipient, recipient);
                            assert_eq!(resolved.generation(), ad.as_ref().unwrap().generation());
                            if prior_address != Some(address) {
                                if let Some(old) = prior.take() {
                                    let old: crate::native_provisioning_capnp::provisioner::Client =
                                        old;
                                    assert!(
                                        old.reserve_request().send().promise.await.is_err(),
                                        "stale provider retained authority"
                                    );
                                }
                            }
                            prior = Some(resolved.binding().provider.clone());
                        } else {
                            assert!(resolved.is_err(), "withdrawn mapping remained discoverable");
                            if let Some(old) = prior.take() {
                                assert!(old.reserve_request().send().promise.await.is_err());
                            }
                        }
                        prior_address = address;
                        assert_eq!(listener.stats().authenticated, 2);
                    }
                    assert!(native_discovery::resolve(
                        &directory.client([99; 32]),
                        [99; 32],
                        "service"
                    )
                    .await
                    .is_err());
                    advance_with_traffic(&mut h, Duration::from_millis(1)).await;
                }));
        }
    }
}

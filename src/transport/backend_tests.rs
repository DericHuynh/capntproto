use super::*;
use std::time::Duration;
#[derive(Clone, Copy, Debug)]
enum Backend {
    Tcp,
    Quiche,
}
async fn pair(
    a: Backend,
    b: Backend,
    version: QuicVersion,
) -> (AuthenticatedSession, AuthenticatedSession) {
    let a_id = Identity::generate();
    let b_id = Identity::generate();
    let context = b"backend tests";
    let secret = Some([23; 32]);
    if matches!(a, Backend::Tcp) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (a, b) = tokio::join!(
            tcp::connect(
                listener.local_addr().unwrap(),
                &a_id,
                b_id.public_key(),
                secret,
                context,
                Duration::from_secs(5)
            ),
            async {
                tcp::accept(
                    listener.accept().await.unwrap().0,
                    &b_id,
                    a_id.public_key(),
                    secret,
                    context,
                    Duration::from_secs(5),
                )
                .await
            }
        );
        return (a.unwrap(), b.unwrap());
    }
    let left = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let right = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = right.local_addr().unwrap();
    let client = async {
        match a {
            Backend::Quiche => {
                connect_for_version(
                    left,
                    address,
                    &a_id,
                    b_id.public_key(),
                    secret,
                    context,
                    version,
                )
                .await
            }
            Backend::Tcp => unreachable!(),
        }
    };
    let server = async {
        match b {
            Backend::Quiche => {
                accept_authenticated(right, &b_id, a_id.public_key(), secret, context).await
            }
            Backend::Tcp => unreachable!(),
        }
    };
    let (a, b) = tokio::join!(client, server);
    (a.unwrap(), b.unwrap())
}
#[tokio::test(flavor = "current_thread")]
async fn authenticated_backends_exchange_rpc_datagrams_and_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (client, server, version) in [
                (Backend::Tcp, Backend::Tcp, QuicVersion::V1),
                (Backend::Quiche, Backend::Quiche, QuicVersion::V1),
                (Backend::Quiche, Backend::Quiche, QuicVersion::V2),
            ] {
                eprintln!("backend {client:?} -> {server:?} {version:?}");
                tokio::time::timeout(Duration::from_secs(15), async {
                    let (mut a, mut b) = pair(client, server, version).await;
                    assert_eq!(a.peer, b.local);
                    assert_eq!(b.peer, a.local);
                    let bytes = vec![42; 150_000];
                    let mut received = vec![0; bytes.len()];
                    let (sent, read) = tokio::join!(
                        a.io.as_mut().unwrap().write_all(&bytes),
                        b.io.as_mut().unwrap().read_exact(&mut received)
                    );
                    sent.unwrap();
                    read.unwrap();
                    assert_eq!(received, bytes);
                    if !matches!(client, Backend::Tcp) {
                        let ad = a.take_datagrams().unwrap();
                        let mut bd = b.take_datagrams().unwrap();
                        ad.sender().try_send(b"snapshot").unwrap();
                        assert_eq!(bd.recv().await.unwrap(), b"snapshot");
                    } else {
                        assert!(a.take_datagrams().is_none());
                    }
                    let receipt = a.shutdown(Duration::from_secs(3)).await.unwrap();
                    assert_eq!(receipt.bytes, bytes.len() as u64);
                })
                .await
                .unwrap();
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn backends_crossed_shutdown_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for backend in [Backend::Tcp, Backend::Quiche] {
                let (a, b) = pair(backend, backend, QuicVersion::V1).await;
                let (a, b) = tokio::join!(
                    a.shutdown(Duration::from_secs(3)),
                    b.shutdown(Duration::from_secs(3))
                );
                assert_eq!(a.unwrap().bytes, 0);
                assert_eq!(b.unwrap().bytes, 0);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_directory_honors_the_configured_local_bind() {
    use crate::native_rpc::{Backend as RouteBackend, Connector, DirectoryConnector};
    use std::rc::Rc;
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let identity = Rc::new(Identity::generate());
    let peer = Identity::generate().public_key();
    let directory = DirectoryConnector::new(identity, occupied.local_addr().unwrap());
    directory
        .insert_with_backend(
            peer,
            destination.local_addr().unwrap(),
            None,
            b"bind test",
            RouteBackend::Tcp,
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = directory.connect(peer) => assert!(result.is_err()),
            _ = destination.accept() => panic!("TCP route ignored its occupied local bind"),
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn quiche_dials_single_use_reservations_in_both_versions() {
    use crate::{
        native_listener::{Limits, Listener},
        native_rpc::{Connector, DirectoryConnector},
    };
    use std::rc::Rc;
    tokio::task::LocalSet::new()
        .run_until(async {
            for version in [QuicVersion::V1, QuicVersion::V2] {
                let a = Rc::new(Identity::generate());
                let b = Rc::new(Identity::generate());
                let listener =
                    Listener::bind("127.0.0.1:0".parse().unwrap(), b.clone(), Limits::default())
                        .await
                        .unwrap();
                let reservation = listener
                    .reserve(a.public_key(), Some([17; 32]), b"reserved")
                    .unwrap();
                let directory = DirectoryConnector::new(a.clone(), "127.0.0.1:0".parse().unwrap());
                directory
                    .insert_reserved_for_version(
                        reservation.target(),
                        Some([17; 32]),
                        b"reserved",
                        version,
                    )
                    .unwrap();
                let (client, server) =
                    tokio::join!(directory.connect(b.public_key()), reservation.accept());
                let (mut client, mut server) = (client.unwrap(), server.unwrap());
                client
                    .io
                    .as_mut()
                    .unwrap()
                    .write_all(b"authorized")
                    .await
                    .unwrap();
                let mut bytes = [0; 10];
                server
                    .io
                    .as_mut()
                    .unwrap()
                    .read_exact(&mut bytes)
                    .await
                    .unwrap();
                assert_eq!(&bytes, b"authorized");
                let scheduling = client.scheduling();
                scheduling
                    .configure(Schedule {
                        packet_burst: 1,
                        datagrams: Some(DatagramPacing {
                            interval: Duration::from_millis(1),
                            burst: 1,
                        }),
                    })
                    .unwrap();
                assert!(client.mobility().is_supported());
                server.mobility().rotate_connection_id().await.unwrap();
                let next = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let local = next.local_addr().unwrap();
                let path = client
                    .mobility()
                    .migrate(next, listener.local_addr().unwrap(), Duration::from_secs(3))
                    .await
                    .unwrap();
                assert_eq!(path.local, local);
                let outbound = client.take_datagrams().unwrap();
                let mut inbound = server.take_datagrams().unwrap();
                outbound.sender().try_send(b"after migration").unwrap();
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(3), inbound.recv())
                        .await
                        .unwrap()
                        .unwrap(),
                    b"after migration"
                );
                assert_eq!(scheduling.stats().unwrap().datagram_attempts, 1);
                assert!(directory.connect(b.public_key()).await.is_err());
                client.shutdown(Duration::from_secs(2)).await.unwrap();
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn native_network_runs_capability_rpc_on_every_backend() {
    use crate::native_rpc::Network;
    use capntproto_test_support::runtime_test_capnp::harness;
    use std::rc::Rc;
    struct Echo;
    impl harness::Server for Echo {
        async fn echo(
            self: Rc<Self>,
            p: harness::EchoParams,
            mut r: harness::EchoResults,
        ) -> capnp::Result<()> {
            r.get().set_value(p.get()?.get_value());
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for backend in [Backend::Tcp, Backend::Quiche] {
                let (a, b) = pair(backend, backend, QuicVersion::V2).await;
                let peer = b.local;
                let (an, ah) = Network::new(a.local);
                let (bn, bh) = Network::new(b.local);
                ah.attach(a).unwrap();
                bh.attach(b).unwrap();
                let service: harness::Client = capnp_rpc::new_client(Echo);
                let server = tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                    Box::new(bn),
                    Some(service.client),
                ));
                let mut rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let client: harness::Client = rpc.bootstrap(peer);
                let runner = tokio::task::spawn_local(rpc);
                let mut req = client.echo_request();
                req.get().set_value(731);
                let response = tokio::time::timeout(Duration::from_secs(3), req.send().promise)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(response.get().unwrap().get_value(), 731);
                server.abort();
                runner.abort();
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn mutual_tls_rejects_wrong_peer_secret_and_context_before_server_admission() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for tcp in [true, false] {
                for fault in 0..3 {
                    let a = Identity::generate();
                    let b = Identity::generate();
                    let peer = if fault == 0 {
                        Identity::generate().public_key()
                    } else {
                        a.public_key()
                    };
                    let secret = if fault == 1 {
                        Some([2; 32])
                    } else {
                        Some([1; 32])
                    };
                    let context: &[u8] = if fault == 2 { b"wrong" } else { b"right" };
                    if tcp {
                        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                        let (_client, server) = tokio::join!(
                            super::tcp::connect(
                                listener.local_addr().unwrap(),
                                &a,
                                b.public_key(),
                                Some([1; 32]),
                                b"right",
                                Duration::from_secs(2)
                            ),
                            async {
                                super::tcp::accept(
                                    listener.accept().await.unwrap().0,
                                    &b,
                                    peer,
                                    secret,
                                    context,
                                    Duration::from_secs(2),
                                )
                                .await
                            }
                        );
                        assert!(server.is_err());
                    } else {
                        let left = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                        let right = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                        let addr = right.local_addr().unwrap();
                        let (_client, server) = tokio::join!(
                            connect_for_version(
                                left,
                                addr,
                                &a,
                                b.public_key(),
                                Some([1; 32]),
                                b"right",
                                QuicVersion::V2
                            ),
                            accept_authenticated(right, &b, peer, secret, context)
                        );
                        assert!(server.is_err());
                    }
                }
            }
        })
        .await;
}

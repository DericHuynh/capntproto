use super::common::*;
use capnp_rpc::rpc_twoparty_capnp::Side;
use capntproto_transport::{driven::DriverExt, monoio, tokio as tokio_adapter};
use std::io;

#[::tokio::test(flavor = "current_thread")]
async fn explicit_tokio_driver_preserves_both_rpc_drivers_and_full_duplex() {
    for client_portable in [false, true] {
        for server_portable in [false, true] {
            local(async {
                let pki = Pki::new();
                let listener = ::tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .unwrap();
                let addr = listener.local_addr().unwrap();
                let serving = async {
                    let (socket, _) = listener.accept().await.unwrap();
                    let (mut stream, transport) =
                        tokio_adapter::tls::server_driven(socket, pki.server(false)).unwrap();
                    transport
                        .run_until(async {
                            stream.ready().await.unwrap();
                            capntproto_transport::rpc::driver(
                                stream,
                                Some(bootstrap()),
                                Side::Server,
                                protocol(server_portable),
                            )
                            .unwrap()
                            .await
                            .unwrap();
                        })
                        .await
                        .unwrap();
                };
                let calling = async {
                    let socket = ::tokio::net::TcpStream::connect(addr).await.unwrap();
                    let (mut stream, transport) = tokio_adapter::tls::client_driven(
                        socket,
                        pki.client(false),
                        "localhost".try_into().unwrap(),
                    )
                    .unwrap();
                    transport
                        .run_until(async {
                            stream.ready().await.unwrap();
                            exercise_borrowed(
                                capntproto_transport::rpc::driver(
                                    stream,
                                    None,
                                    Side::Client,
                                    protocol(client_portable),
                                )
                                .unwrap(),
                            )
                            .await;
                        })
                        .await
                        .unwrap();
                };
                futures::join!(serving, calling);
            })
            .await;
        }
    }
}

async fn exercise_monoio() {
    for client_portable in [false, true] {
        for server_portable in [false, true] {
            let pki = Pki::new();
            let listener = monoio::runtime::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let serving = async {
                let (socket, _) = listener.accept().await.unwrap();
                let (mut stream, transport) =
                    monoio::tls::server(socket, pki.server(false)).unwrap();
                transport
                    .run_until(async {
                        stream.ready().await.unwrap();
                        capntproto_transport::rpc::driver(
                            stream,
                            Some(bootstrap()),
                            Side::Server,
                            protocol(server_portable),
                        )
                        .unwrap()
                        .await
                        .unwrap();
                    })
                    .await
                    .unwrap();
            };
            let calling = async {
                let socket = monoio::tls::tcp_connect(addr).await.unwrap();
                let (mut stream, transport) =
                    monoio::tls::client(socket, pki.client(false), "localhost".try_into().unwrap())
                        .unwrap();
                transport
                    .run_until(async {
                        stream.ready().await.unwrap();
                        exercise_borrowed(
                            capntproto_transport::rpc::driver(
                                stream,
                                None,
                                Side::Client,
                                protocol(client_portable),
                            )
                            .unwrap(),
                        )
                        .await;
                    })
                    .await
                    .unwrap();
            };
            monoio::runtime::time::timeout(TIMEOUT, async {
                futures::join!(serving, calling);
            })
            .await
            .expect("monoio RPC timed out");
        }
    }
}

#[test]
fn monoio_epoll_drives_real_tls_rpc_without_a_tokio_runtime() {
    monoio::runtime::RuntimeBuilder::<monoio::runtime::LegacyDriver>::new()
        .enable_timer()
        .build()
        .unwrap()
        .block_on(exercise_monoio());
}

#[cfg(target_os = "linux")]
#[test]
fn monoio_uring_drives_real_tls_rpc_without_a_tokio_runtime() {
    match monoio::runtime::RuntimeBuilder::<monoio::runtime::IoUringDriver>::new()
        .enable_timer()
        .build()
    {
        Ok(mut runtime) => runtime.block_on(exercise_monoio()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("io_uring unavailable on this host: {error}; explicit epoll test still runs");
        }
        Err(error) => panic!("io_uring runtime initialization failed: {error}"),
    }
}

#[cfg(feature = "webtransport")]
async fn webtransport_pair<S, C>(
    server: capntproto_transport::webtransport::driven::Server,
    server_driver: S,
    client: capntproto_transport::webtransport::driven::Client,
    client_driver: C,
    client_portable: bool,
    server_portable: bool,
) where
    S: capntproto_transport::driven::Driver<Error = io::Error>,
    C: capntproto_transport::driven::Driver<Error = io::Error>,
{
    let address = server.local_addr().unwrap();
    let url = format!("https://127.0.0.1:{}/rpc", address.port());
    let serving = server_driver.run_until(async move {
        let request = server.accept().await.unwrap();
        assert_eq!(request.path(), "/rpc");
        let mut session = request.accept().await.unwrap();
        let stream = session.accept_bi().await.unwrap();
        drop(session);
        drop(server);
        capntproto_transport::rpc::driver(
            stream,
            Some(bootstrap()),
            Side::Server,
            protocol(server_portable),
        )
        .unwrap()
        .await
        .unwrap();
    });
    let calling = client_driver.run_until(async move {
        let mut session = client.connect(address, &url).await.unwrap();
        let stream = session.open_bi().await.unwrap();
        drop(session);
        drop(client);
        exercise_borrowed(
            capntproto_transport::rpc::driver(
                stream,
                None,
                Side::Client,
                protocol(client_portable),
            )
            .unwrap(),
        )
        .await;
    });
    let (server, client) = futures::join!(serving, calling);
    server.unwrap();
    client.unwrap();
}

#[cfg(feature = "webtransport")]
#[::tokio::test(flavor = "current_thread")]
async fn externally_driven_tokio_webtransport_runs_rpc_without_protocol_tasks() {
    for cp in [false, true] {
        for sp in [false, true] {
            local(async {
                let pki = Pki::new();
                let id = pki.identity("127.0.0.1", false, false);
                let (server, sd) = tokio_adapter::webtransport::server_driven(
                    "127.0.0.1:0".parse().unwrap(),
                    id.certificates,
                    id.private_key,
                    std::sync::Arc::new(capntproto::rpc::SystemClock),
                )
                .unwrap();
                let roots = capntproto_transport::webtransport::driven::tls::RootCertStore {
                    roots: pki.roots().roots,
                };
                let (client, cd) = tokio_adapter::webtransport::client_driven(
                    roots,
                    std::sync::Arc::new(capntproto::rpc::SystemClock),
                )
                .unwrap();
                webtransport_pair(server, sd, client, cd, cp, sp).await;
            })
            .await;
        }
    }
}

#[cfg(feature = "webtransport")]
async fn exercise_monoio_webtransport() {
    for cp in [false, true] {
        for sp in [false, true] {
            let pki = Pki::new();
            let id = pki.identity("127.0.0.1", false, false);
            let (server, sd) = monoio::webtransport::server_driven(
                "127.0.0.1:0".parse().unwrap(),
                id.certificates,
                id.private_key,
                std::sync::Arc::new(capntproto::rpc::SystemClock),
            )
            .unwrap();
            let roots = capntproto_transport::webtransport::driven::tls::RootCertStore {
                roots: pki.roots().roots,
            };
            let (client, cd) = monoio::webtransport::client_driven(
                roots,
                std::sync::Arc::new(capntproto::rpc::SystemClock),
            )
            .unwrap();
            monoio::runtime::time::timeout(
                TIMEOUT,
                webtransport_pair(server, sd, client, cd, cp, sp),
            )
            .await
            .expect("monoio WebTransport timed out");
        }
    }
}

#[cfg(feature = "webtransport")]
#[test]
fn monoio_epoll_webtransport_runs_without_tokio() {
    monoio::runtime::RuntimeBuilder::<monoio::runtime::LegacyDriver>::new()
        .enable_timer()
        .build()
        .unwrap()
        .block_on(exercise_monoio_webtransport());
}

#[cfg(all(feature = "webtransport", target_os = "linux"))]
#[test]
fn externally_driven_monoio_webtransport_runs_rpc_without_tokio() {
    match monoio::runtime::RuntimeBuilder::<monoio::runtime::IoUringDriver>::new()
        .enable_timer()
        .build()
    {
        Ok(mut runtime) => runtime.block_on(exercise_monoio_webtransport()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("io_uring unavailable: {error}; the separate epoll test still runs");
        }
        Err(error) => panic!("io_uring initialization failed: {error}"),
    }
}

#[::tokio::test(flavor = "current_thread")]
async fn owned_tls_interoperates_with_buffered_peer_key_updates_and_tiny_io() {
    use ::tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use capntproto_transport::rustls;
    use futures::{AsyncReadExt, AsyncWriteExt};
    local(async {
        let pki = Pki::new();
        let (client, server) = ::tokio::io::duplex(37);
        let (reader, writer) = ::tokio::io::split(client);
        let (mut stream, driver) = rustls::client(
            tokio_adapter::Reader(reader),
            tokio_adapter::Writer(writer),
            pki.client(false),
            "localhost".try_into().unwrap(),
        )
        .unwrap();
        let serving = async {
            let mut server = tokio_rustls::TlsAcceptor::from(pki.reference_server())
                .accept(server)
                .await
                .unwrap();
            for value in 0..4u8 {
                server.get_mut().1.refresh_traffic_keys().unwrap();
                server.write_all(&vec![value; 19_001]).await.unwrap();
                server.flush().await.unwrap();
                let mut received = [0; 13];
                server.read_exact(&mut received).await.unwrap();
                assert_eq!(received, [value; 13]);
            }
            assert_eq!(server.read(&mut [0; 1]).await.unwrap(), 0);
            server.shutdown().await.unwrap();
        };
        let calling = driver.run_until(async {
            stream.ready().await.unwrap();
            for value in 0..4u8 {
                let mut received = vec![0; 19_001];
                stream.read_exact(&mut received).await.unwrap();
                assert!(received.iter().all(|b| *b == value));
                stream.write_all(&[value; 13]).await.unwrap();
                stream.flush().await.unwrap();
            }
            stream.close().await.unwrap();
            assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
        });
        let ((), result) = futures::join!(serving, calling);
        result.unwrap();
    })
    .await;
}

#[::tokio::test(flavor = "current_thread")]
async fn owned_tls_detects_truncation_and_driver_drop_wakes_pending_read() {
    use ::tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use capntproto_transport::rustls;
    use futures::{AsyncReadExt, AsyncWriteExt};
    local(async {
        let pki=Pki::new();let (client,server)=::tokio::io::duplex(127);
        let (reader,writer)=::tokio::io::split(server);
        let (mut stream,driver)=rustls::server(tokio_adapter::Reader(reader),tokio_adapter::Writer(writer),pki.server(false)).unwrap();
        let (done, finished)=futures::channel::oneshot::channel();
        let calling=async {
            let mut client=tokio_rustls::TlsConnector::from(pki.reference_client()).connect("localhost".try_into().unwrap(),client).await.unwrap();
            client.write_all(b"authenticated").await.unwrap();client.flush().await.unwrap();
            client.get_mut().0.shutdown().await.unwrap();
            // Drain post-handshake tickets so the bounded peer can finish its handshake.
            let _=client.read_to_end(&mut Vec::new()).await;
            let _=finished.await;
        };
        let serving=driver.run_until(async {
            stream.ready().await.unwrap();let mut bytes=[0;13];stream.read_exact(&mut bytes).await.unwrap();assert_eq!(&bytes,b"authenticated");
            assert_eq!(stream.read(&mut [0;1]).await.unwrap_err().kind(),io::ErrorKind::UnexpectedEof);
        });
        let serving=async {let result=serving.await;let _=done.send(());result};
        let ((),result)=futures::join!(calling,serving);
        // A fatal ciphertext error is reflected in both the stream and driver.
        assert!(matches!(result,Err(capntproto_transport::driven::Error::Transport(ref e)) if e.kind()==io::ErrorKind::UnexpectedEof), "{result:?}");

        let (reader,writer)=::tokio::io::split(::tokio::io::duplex(127).0);
        let (mut stream,driver)=rustls::server(tokio_adapter::Reader(reader),tokio_adapter::Writer(writer),pki.server(false)).unwrap();
        drop(driver);
        assert_eq!(stream.ready().await.unwrap_err().kind(),io::ErrorKind::ConnectionAborted);
        assert_eq!(stream.write(&[1]).await.unwrap_err().kind(),io::ErrorKind::ConnectionAborted);
    }).await;
}

#[test]
fn raw_record_layer_validates_windows_and_runs_without_an_executor() {
    use capntproto_transport::rustls::{Progress, RecordLayer};
    fn pump(source: &mut RecordLayer, destination: &mut RecordLayer) {
        let bytes = source.output();
        if !bytes.is_empty() {
            let n = bytes.len().min(31);
            destination.input().unwrap()[..n].copy_from_slice(&bytes[..n]);
            destination.commit_input(n).unwrap();
            source.consume_output(n).unwrap();
        }
        if source.output().is_empty() {
            source.flushed().unwrap();
        }
    }
    let pki = Pki::new();
    let mut client =
        RecordLayer::client(pki.client(false), "localhost".try_into().unwrap()).unwrap();
    let mut server = RecordLayer::server(pki.server(false)).unwrap();
    let capacity = client.input().unwrap().len();
    assert_eq!(
        client.commit_input(capacity + 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        client.consume_output(1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(matches!(client.handshake().unwrap(), Progress::Output));
    assert_eq!(
        client.flushed().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    let mut ready = false;
    for _ in 0..20_000 {
        let cr = client.handshake().unwrap();
        let sr = server.handshake().unwrap();
        pump(&mut client, &mut server);
        pump(&mut server, &mut client);
        if matches!(cr, Progress::Ready(_))
            && matches!(sr, Progress::Ready(_))
            && client.output().is_empty()
            && server.output().is_empty()
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "raw handshake must make bounded progress");
    assert_eq!(
        client.common().alpn_protocol(),
        Some(capntproto_transport::ALPN)
    );
    // Queue separate application writes before submitting ciphertext. This
    // must batch without exposing unbounded buffering or changing record order.
    for byte in 0..32u8 {
        assert_eq!(client.write(&[byte]).unwrap(), Progress::Ready(1));
    }
    let mut received_prefix = Vec::new();
    for _ in 0..2_000 {
        pump(&mut client, &mut server);
        let mut bytes = [0; 32];
        if let Progress::Ready(n) = server.read(&mut bytes).unwrap() {
            received_prefix.extend_from_slice(&bytes[..n]);
        }
        if received_prefix.len() == 32 {
            break;
        }
    }
    assert_eq!(received_prefix, (0..32).collect::<Vec<_>>());
    let payload = vec![0x5a; 50_003];
    let mut written = 0;
    let mut received = Vec::new();
    for _ in 0..20_000 {
        if written < payload.len() {
            let slices = [io::IoSlice::new(&[]), io::IoSlice::new(&payload[written..])];
            if let Progress::Ready(n) = client.write_vectored(&slices).unwrap() {
                written += n;
            }
        }
        pump(&mut client, &mut server);
        pump(&mut server, &mut client);
        let mut bytes = [0; 73];
        if let Progress::Ready(n) = server.read(&mut bytes).unwrap() {
            received.extend_from_slice(&bytes[..n]);
        }
        if received.len() == payload.len() {
            break;
        }
    }
    assert_eq!(written, payload.len());
    assert_eq!(received, payload);
    for _ in 0..2_000 {
        let _ = client.shutdown().unwrap();
        pump(&mut client, &mut server);
        if matches!(server.read(&mut [0; 1]).unwrap(), Progress::Ready(0)) {
            return;
        }
    }
    panic!("close_notify was not observed");
}

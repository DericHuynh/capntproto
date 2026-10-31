use super::common::*;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::TwoPartyClient};
use capntproto::rpc::{webtransport, SystemClock};
use std::sync::Arc;

#[tokio::test(flavor = "current_thread")]
async fn webtransport_rpc_callbacks_pipelines_concurrency_and_large_frames() {
    for client_portable in [false, true] {
        for server_portable in [false, true] {
            local(async {
                let pki = Pki::new();
                let identity = pki.identity("127.0.0.1", false, false);
                let server = webtransport::Server::bind(
                    "127.0.0.1:0".parse().unwrap(),
                    identity.certificates,
                    identity.private_key,
                    Arc::new(SystemClock),
                )
                .unwrap();
                let roots = webtransport::tls::RootCertStore {
                    roots: pki.roots().roots,
                };
                let client = webtransport::Client::new(roots, Arc::new(SystemClock)).unwrap();
                let url = format!(
                    "https://127.0.0.1:{}/rpc",
                    server.local_addr().unwrap().port()
                );
                let serving = async move {
                    let request = server.accept().await.unwrap();
                    assert_eq!(request.path(), "/rpc");
                    assert!(request.origin().is_none());
                    let mut session = request.accept().await.unwrap();
                    let stream = session.accept_bi().await.unwrap();
                    drop(session);
                    drop(server);
                    let rpc = capntproto_transport::rpc::driver(
                        stream,
                        Some(bootstrap()),
                        Side::Server,
                        protocol(server_portable),
                    )
                    .unwrap();
                    rpc.await.unwrap();
                };
                let calling = async move {
                    let mut session = client.connect(&url).await.unwrap();
                    let stream = session.open_bi().await.unwrap();
                    drop(session);
                    drop(client);
                    let rpc: TwoPartyClient<'static> = capntproto_transport::rpc::driver(
                        stream,
                        None,
                        Side::Client,
                        protocol(client_portable),
                    )
                    .unwrap();
                    exercise_borrowed(rpc).await;
                };
                futures::join!(serving, calling);
            })
            .await;
        }
    }
}

struct Clock(Option<u64>);
impl capntproto::rpc::TlsClock for Clock {
    fn now(&self) -> Option<u64> {
        self.0
    }
}
#[tokio::test(flavor = "current_thread")]
async fn webtransport_uses_caller_clock_and_rejects_untrusted_servers() {
    for (now, trusted) in [
        (None, true),
        (Some(0), true),
        (Some(u64::MAX), true),
        (Some(1_800_000_000), false),
    ] {
        local(async {
            let pki = Pki::new();
            let id = pki.identity("127.0.0.1", false, false);
            let server = webtransport::Server::bind(
                "127.0.0.1:0".parse().unwrap(),
                id.certificates,
                id.private_key,
                Arc::new(SystemClock),
            )
            .unwrap();
            let roots = if trusted {
                pki.roots()
            } else {
                Pki::new().roots()
            };
            let client = webtransport::Client::new(
                webtransport::tls::RootCertStore { roots: roots.roots },
                Arc::new(Clock(now)),
            )
            .unwrap();
            let url = format!(
                "https://127.0.0.1:{}/rpc",
                server.local_addr().unwrap().port()
            );
            let serving = async {
                let _ = server.accept().await;
            };
            let calling = async {
                assert!(client.connect(&url).await.is_err());
            };
            futures::join!(serving, calling);
        })
        .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn listener_rejects_paths_times_out_idle_setup_and_preserves_active_rpc_on_cancel() {
    local(async {
        let pki = Pki::new();
        let id = pki.identity("127.0.0.1", false, false);
        let endpoint = webtransport::Server::bind(
            "127.0.0.1:0".parse().unwrap(),
            id.certificates,
            id.private_key,
            Arc::new(SystemClock),
        )
        .unwrap();
        let url = format!(
            "https://127.0.0.1:{}",
            endpoint.local_addr().unwrap().port()
        );
        let client = webtransport::Client::new(
            webtransport::tls::RootCertStore {
                roots: pki.roots().roots,
            },
            Arc::new(SystemClock),
        )
        .unwrap();
        let (server, driver) =
            capnp_rpc::twoparty::TwoPartyServer::with_driver(bootstrap(), protocol(true), |_| {});
        let server = std::rc::Rc::new(server);
        let driving = tokio::task::spawn_local(driver);
        let (rejected, mut errors) = tokio::sync::mpsc::unbounded_channel();
        let listener = {
            let server = server.clone();
            tokio::task::spawn_local(async move {
                webtransport::listen(
                    &server,
                    &endpoint,
                    Default::default(),
                    std::time::Duration::from_secs(2),
                    2.try_into().unwrap(),
                    |request| request.path() == "/rpc",
                    |err| {
                        rejected.send(err.kind()).unwrap();
                    },
                )
                .await
            })
        };
        assert!(client.connect(&format!("{url}/forbidden")).await.is_err());
        assert_eq!(
            errors.recv().await.unwrap(),
            std::io::ErrorKind::PermissionDenied
        );
        let _idle = client.connect(&format!("{url}/rpc")).await.unwrap();
        // A valid HTTP3 session that never opens its RPC stream must release its
        // bounded admission slot; it must not stall subsequent clients.
        assert_eq!(errors.recv().await.unwrap(), std::io::ErrorKind::TimedOut);
        let mut session = client.connect(&format!("{url}/rpc")).await.unwrap();
        let stream = session.open_bi().await.unwrap();
        let mut rpc = webtransport::driver(stream, None, Side::Client, protocol(true)).unwrap();
        let cap = rpc.bootstrap();
        let close = rpc.get_disconnector();
        let task = tokio::task::spawn_local(rpc);
        assert_eq!(echo(&cap, 81).await.unwrap(), 81);
        listener.abort();
        assert!(listener.await.unwrap_err().is_cancelled());
        assert_eq!(echo(&cap, 82).await.unwrap(), 82);
        close.await.unwrap();
        task.await.unwrap().unwrap();
        server.drain().await.unwrap();
        drop(server);
        driving.await.unwrap().unwrap();
    })
    .await;
}

// Keep an independent high-level HTTP/3 implementation as the interoperability
// peer. Self-interoperability alone would hide shared protocol mistakes.
struct ReferenceStream {
    read: tokio_util::compat::Compat<wtransport::RecvStream>,
    write: tokio_util::compat::Compat<wtransport::SendStream>,
    connection: wtransport::Connection,
}
impl ReferenceStream {
    fn new(
        connection: wtransport::Connection,
        send: wtransport::SendStream,
        recv: wtransport::RecvStream,
    ) -> Self {
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
        Self {
            read: recv.compat(),
            write: send.compat_write(),
            connection,
        }
    }
}
impl futures::AsyncRead for ReferenceStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.read).poll_read(cx, bytes)
    }
}
impl futures::AsyncWrite for ReferenceStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.write).poll_write(cx, bytes)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.write).poll_flush(cx)
    }
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.write).poll_close(cx)
    }
}
impl Drop for ReferenceStream {
    fn drop(&mut self) {
        self.connection
            .close(wtransport::VarInt::from_u32(0), b"RPC stream dropped");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn independent_webtransport_peer_interoperates_in_both_roles_and_rpc_drivers() {
    use capntproto_transport::tokio::webtransport;
    for portable in [false, true] {
        local(async {
            let pki = Pki::new();
            let mut tls = (*pki.reference_client()).clone();
            tls.alpn_protocols = vec![b"h3".to_vec()];
            let reference = wtransport::Endpoint::client(
                wtransport::ClientConfig::builder()
                    .with_bind_default()
                    .with_custom_tls(tls)
                    .build(),
            )
            .unwrap();
            let identity = pki.identity("127.0.0.1", false, false);
            let server = webtransport::Server::bind(
                "127.0.0.1:0".parse().unwrap(),
                identity.certificates,
                identity.private_key,
                Arc::new(SystemClock),
            )
            .unwrap();
            let url = format!(
                "https://127.0.0.1:{}/rpc",
                server.local_addr().unwrap().port()
            );
            let serving = async {
                let mut session = server.accept().await.unwrap().accept().await.unwrap();
                let stream = session.accept_bi().await.unwrap();
                capntproto_transport::rpc::driver(
                    stream,
                    Some(bootstrap()),
                    Side::Server,
                    protocol(portable),
                )
                .unwrap()
                .await
                .unwrap();
            };
            let calling = async {
                let connection = reference.connect(&url).await.unwrap();
                let (send, recv) = connection.open_bi().await.unwrap().await.unwrap();
                exercise_borrowed(
                    capntproto_transport::rpc::driver(
                        ReferenceStream::new(connection, send, recv),
                        None,
                        Side::Client,
                        protocol(portable),
                    )
                    .unwrap(),
                )
                .await;
            };
            futures::join!(serving, calling);
        })
        .await;
        local(async {
            let pki = Pki::new();
            let identity = pki.identity("127.0.0.1", false, false);
            let mut tls = webtransport::tls::ServerConfig::builder_with_provider(Arc::new(
                webtransport::tls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[&webtransport::tls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(identity.certificates, identity.private_key)
            .unwrap();
            tls.alpn_protocols = vec![b"h3".to_vec()];
            let reference = wtransport::Endpoint::server(
                wtransport::ServerConfig::builder()
                    .with_bind_address("127.0.0.1:0".parse().unwrap())
                    .with_custom_tls(tls)
                    .build(),
            )
            .unwrap();
            let client = webtransport::Client::new(
                webtransport::tls::RootCertStore {
                    roots: pki.roots().roots,
                },
                Arc::new(SystemClock),
            )
            .unwrap();
            let url = format!(
                "https://127.0.0.1:{}/rpc",
                reference.local_addr().unwrap().port()
            );
            let serving = async {
                let request = reference.accept().await.await.unwrap();
                assert_eq!(request.path(), "/rpc");
                let connection = request.accept().await.unwrap();
                let (send, recv) = connection.accept_bi().await.unwrap();
                capntproto_transport::rpc::driver(
                    ReferenceStream::new(connection, send, recv),
                    Some(bootstrap()),
                    Side::Server,
                    protocol(portable),
                )
                .unwrap()
                .await
                .unwrap();
            };
            let calling = async {
                let mut session = client.connect(&url).await.unwrap();
                let stream = session.open_bi().await.unwrap();
                exercise_borrowed(
                    capntproto_transport::rpc::driver(
                        stream,
                        None,
                        Side::Client,
                        protocol(portable),
                    )
                    .unwrap(),
                )
                .await;
            };
            futures::join!(serving, calling);
        })
        .await;
    }
}

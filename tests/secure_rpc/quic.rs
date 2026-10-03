use super::common::*;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::TwoPartyServer};
use capntproto::rpc::quic::{self, Endpoint, Version};
use std::{io, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn server(pki: &Pki, mutual: bool) -> Endpoint {
    Endpoint::server(
        quic::server_config(
            pki.identity("localhost", false, false),
            mutual.then(|| pki.roots_der()),
        )
        .unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap()
}
fn client(pki: &Pki, mutual: bool, version: Version) -> Endpoint {
    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(
        quic::client_config_for_version(
            pki.roots_der(),
            mutual.then(|| pki.identity("client.local", true, false)),
            version,
        )
        .unwrap(),
    );
    endpoint
}
async fn roundtrip(mutual: bool, version: Version) {
    local(async {
        let pki = Pki::new();
        let server = server(&pki, mutual);
        let address = server.local_addr().unwrap();
        let task = tokio::task::spawn_local(async move {
            let stream = quic::accept(server.accept().await.unwrap(), TIMEOUT)
                .await
                .unwrap();
            assert_eq!(stream.version(), version.wire_id());
            assert_eq!(!stream.peer_certificates().is_empty(), mutual);
            quic::client(stream, Some(bootstrap()), Side::Server, Default::default()).await
        });
        let endpoint = client(&pki, mutual, version);
        let stream = quic::connect(&endpoint, address, "localhost", TIMEOUT)
            .await
            .unwrap();
        assert_eq!(stream.version(), version.wire_id());
        assert!(!stream.peer_certificates().is_empty());
        exercise(quic::client(stream, None, Side::Client, Default::default())).await;
        task.await.unwrap().unwrap();
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_v1_rpc_capabilities_pipeline_large_messages_and_shutdown() {
    roundtrip(false, Version::V1).await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_v1_mtls_authenticates_both_peers() {
    roundtrip(true, Version::V1).await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_v2_rpc_and_mtls() {
    roundtrip(false, Version::V2).await;
    roundtrip(true, Version::V2).await;
}

async fn rejected(
    server: quic::ServerConfig,
    client: quic::ClientConfig,
    name: &str,
    server_must_reject: bool,
) {
    let server = Endpoint::server(server, "127.0.0.1:0".parse().unwrap()).unwrap();
    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client);
    let (client_result, server_result) = tokio::join!(
        quic::connect(&endpoint, server.local_addr().unwrap(), name, TIMEOUT),
        async { quic::accept(server.accept().await.unwrap(), TIMEOUT).await },
    );
    let result = if server_must_reject {
        server_result
    } else {
        client_result
    };
    let Err(error) = result else {
        panic!("invalid certificate/ALPN was accepted")
    };
    assert_ne!(
        error.kind(),
        io::ErrorKind::TimedOut,
        "expected validation failure: {error}"
    );
}
#[tokio::test(flavor = "current_thread")]
async fn quic_rejects_untrusted_wrong_name_expired_and_wrong_usage_servers() {
    local(async {
        let pki = Pki::new();
        for version in [Version::V1, Version::V2] {
            for fault in 0..4 {
                let roots = if fault == 0 {
                    Pki::new().roots_der()
                } else {
                    pki.roots_der()
                };
                let server =
                    quic::server_config(pki.identity("localhost", fault == 3, fault == 2), None)
                        .unwrap();
                let client = quic::client_config_for_version(roots, None, version).unwrap();
                rejected(
                    server,
                    client,
                    if fault == 1 {
                        "other.local"
                    } else {
                        "localhost"
                    },
                    false,
                )
                .await;
            }
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_mtls_rejects_missing_untrusted_expired_and_wrong_usage_clients() {
    local(async {
        let pki = Pki::new();
        for version in [Version::V1, Version::V2] {
            for fault in 0..4 {
                let identity = match fault {
                    0 => None,
                    1 => Some(Pki::new().identity("client.local", true, false)),
                    2 => Some(pki.identity("client.local", true, true)),
                    _ => Some(pki.identity("client.local", false, false)),
                };
                rejected(
                    quic::server_config(
                        pki.identity("localhost", false, false),
                        Some(pki.roots_der()),
                    )
                    .unwrap(),
                    quic::client_config_for_version(pki.roots_der(), identity, version).unwrap(),
                    "localhost",
                    true,
                )
                .await;
            }
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_rejects_mismatched_rpc_alpn() {
    local(async {
        let pki = Pki::new();
        for version in [Version::V1, Version::V2] {
            for protocols in [&[][..], &[&b"h3"[..]][..]] {
                let mut client =
                    quic::client_config_for_version(pki.roots_der(), None, version).unwrap();
                client.application_protocols(protocols).unwrap();
                rejected(
                    quic::server_config(pki.identity("localhost", false, false), None).unwrap(),
                    client,
                    "localhost",
                    false,
                )
                .await;
            }
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn setup_deadline_closes_authenticated_peer_without_rpc_stream() {
    local(async {
        let pki = Pki::new();
        let server = server(&pki, false);
        let client = client(&pki, false, Version::V2);
        let (connection, accepted) = tokio::join!(
            quic::connect(&client, server.local_addr().unwrap(), "localhost", TIMEOUT),
            async {
                quic::accept(server.accept().await.unwrap(), Duration::from_millis(100)).await
            },
        );
        assert_eq!(accepted.err().unwrap().kind(), io::ErrorKind::TimedOut);
        let mut connection = connection.unwrap();
        assert!(tokio::time::timeout(TIMEOUT, connection.shutdown())
            .await
            .unwrap()
            .is_err());
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn listener_accepts_while_peer_stalls_and_preserves_established_rpc_after_cancel() {
    local(async {
        let pki = Pki::new();
        let endpoint = server(&pki, true);
        let client = client(&pki, true, Version::V2);
        let (server, driver) = TwoPartyServer::new(bootstrap());
        let server_task = tokio::task::spawn_local(driver);
        let mut listening = Box::pin(quic::listen(&server, &endpoint, Default::default(), Default::default(), |_, e| panic!("unexpected rejection: {e}")));
        let clients = async {
            let stalled = quic::connect(&client, endpoint.local_addr().unwrap(), "localhost", TIMEOUT).await.unwrap();
            let stream = quic::connect(&client, endpoint.local_addr().unwrap(), "localhost", TIMEOUT).await.unwrap();
            let mut driver = quic::client(stream, None, Side::Client, Default::default());
            let cap = driver.bootstrap::<capntproto_test_support::structured::runtime_test_capnp::harness::Client>();
            let close = driver.get_disconnector();
            let task = tokio::task::spawn_local(driver);
            assert_eq!(echo(&cap, 81).await.unwrap(), 81);
            (stalled, cap, close, task)
        };
        let (mut stalled, cap, close, task) = tokio::select! { r = clients => r, r = &mut listening => panic!("listener exited: {r:?}") };
        drop(listening);
        assert!(tokio::time::timeout(TIMEOUT, stalled.shutdown()).await.unwrap().is_err());
        assert_eq!(echo(&cap, 82).await.unwrap(), 82);
        close.await.unwrap(); task.await.unwrap().unwrap();
        server.drain().await.unwrap(); drop(server); server_task.await.unwrap().unwrap();
    }).await;
}
#[tokio::test(flavor = "current_thread")]
async fn acknowledged_half_close_preserves_large_payload_and_reverse_direction() {
    local(async {
        let pki = Pki::new();
        let server = server(&pki, false);
        let address = server.local_addr().unwrap();
        let task = tokio::task::spawn_local(async move {
            let mut stream = quic::accept(server.accept().await.unwrap(), TIMEOUT)
                .await
                .unwrap();
            let mut payload = Vec::new();
            stream.read_to_end(&mut payload).await.unwrap();
            assert_eq!(payload, vec![0x37; 100_000]);
            stream.write_all(b"received").await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let endpoint = client(&pki, false, Version::V2);
        let mut stream = quic::connect(&endpoint, address, "localhost", TIMEOUT)
            .await
            .unwrap();
        stream.write_all(&vec![0x37; 100_000]).await.unwrap();
        stream.shutdown().await.unwrap();
        stream.shutdown().await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        assert_eq!(response, b"received");
        task.await.unwrap();
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn drop_cancels_session_and_releases_routes() {
    local(async {
        let pki = Pki::new();
        let server = server(&pki, false);
        let endpoint = client(&pki, false, Version::V1);
        let (client, server_stream) = tokio::join!(
            async {
                let mut stream = quic::connect(
                    &endpoint,
                    server.local_addr().unwrap(),
                    "localhost",
                    TIMEOUT,
                )
                .await
                .unwrap();
                stream.write_all(b"x").await.unwrap();
                stream
            },
            async {
                quic::accept(server.accept().await.unwrap(), TIMEOUT)
                    .await
                    .unwrap()
            },
        );
        drop(client);
        let mut server_stream = server_stream;
        assert!(tokio::time::timeout(TIMEOUT, server_stream.shutdown())
            .await
            .unwrap()
            .is_err());
        drop(server_stream);
        tokio::time::timeout(TIMEOUT, endpoint.wait_idle())
            .await
            .unwrap();
        tokio::time::timeout(TIMEOUT, server.wait_idle())
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn quic_validates_ip_subject_alternative_names() {
    local(async {
        let pki = Pki::new();
        for name in ["127.0.0.1", "::1"] {
            let endpoint = Endpoint::server(
                quic::server_config(pki.identity(name, false, false), None).unwrap(),
                "127.0.0.1:0".parse().unwrap(),
            )
            .unwrap();
            let client = client(&pki, false, Version::V2);
            let (sent, accepted) = tokio::join!(
                async {
                    let mut stream =
                        quic::connect(&client, endpoint.local_addr().unwrap(), name, TIMEOUT)
                            .await
                            .unwrap();
                    stream.write_all(b"i").await.unwrap();
                    stream
                },
                async {
                    let mut stream = quic::accept(endpoint.accept().await.unwrap(), TIMEOUT)
                        .await
                        .unwrap();
                    assert_eq!(stream.read_u8().await.unwrap(), b'i');
                    stream
                },
            );
            drop(sent);
            drop(accepted);
            rejected(
                quic::server_config(pki.identity("localhost", false, false), None).unwrap(),
                quic::client_config(pki.roots_der(), None).unwrap(),
                name,
                false,
            )
            .await;
        }
    })
    .await;
}

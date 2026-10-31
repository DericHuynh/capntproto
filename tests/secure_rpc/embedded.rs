use super::common::*;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::TwoPartyClient};
use capntproto::rpc::tcp::{embedded, tls};
use std::num::NonZeroUsize;
use tokio::net::TcpListener;

struct Clock(Option<u64>);
impl capntproto::rpc::TlsClock for Clock {
    fn now(&self) -> Option<u64> {
        self.0
    }
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_tcp_rpc_callbacks_pipelines_concurrency_and_large_frames() {
    for client_portable in [false, true] {
        for server_portable in [false, true] {
            local(async {
                let pki = Pki::new();
                let roots = pki.roots_der();
                let clock = Clock(Some(1_800_000_000));
                let name = if client_portable {
                    "127.0.0.1"
                } else {
                    "localhost"
                };
                let config = embedded::ClientConfig::new(name, roots[0].as_ref(), &clock).unwrap();
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                let serving = async {
                    let (io, _) = listener.accept().await.unwrap();
                    let server = tls::server_config(
                        pki.identity(name, false, false),
                        None,
                        std::sync::Arc::new(capntproto::rpc::SystemClock),
                    )
                    .unwrap();
                    let io = tls::accept(io, server, TIMEOUT).await.unwrap();
                    TwoPartyClient::from_network_with_driver(
                        tls::network(io, Side::Server, Default::default()),
                        Some(bootstrap()),
                        protocol(server_portable),
                    )
                    .unwrap()
                    .await
                    .unwrap();
                };
                let calling = async {
                    let mut incoming = [0; 18 * 1024];
                    let mut outgoing = [0; 18 * 1024];
                    let mut certificate = [0; 2048];
                    let connection = embedded::connect(
                        addr,
                        &config,
                        rand::rngs::OsRng,
                        embedded::Buffers {
                            read: &mut incoming,
                            write: &mut outgoing,
                            certificate: &mut certificate,
                        },
                        TIMEOUT,
                    )
                    .await
                    .unwrap();
                    embedded::with_futures_io(
                        connection,
                        NonZeroUsize::new(8192).unwrap(),
                        async |io| {
                            let driver = TwoPartyClient::new_borrowed_with_driver(
                                io,
                                None,
                                Side::Client,
                                Default::default(),
                                protocol(client_portable),
                            )
                            .unwrap();
                            exercise_borrowed(driver).await;
                            Ok::<_, std::io::Error>(())
                        },
                    )
                    .await
                    .unwrap();
                };
                futures::join!(serving, calling);
            })
            .await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_tcp_rejects_missing_time_wrong_name_trust_usage_and_expiry() {
    for fault in 0..10 {
        local(async {
            let pki = Pki::new();
            let untrusted = Pki::new();
            let roots = if fault == 3 {
                untrusted.roots_der()
            } else {
                pki.roots_der()
            };
            let clock = Clock(match fault {
                0 => None,
                1 => Some(0),
                2 => Some(u64::MAX),
                _ => Some(1_800_000_000),
            });
            let name = if fault == 9 {
                "127.0.0.2"
            } else if fault == 4 {
                "other.local"
            } else {
                "localhost"
            };
            let config = embedded::ClientConfig::new(name, roots[0].as_ref(), &clock).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let mut identity = pki.identity("localhost", fault == 5, fault == 6);
            if fault == 7 {
                // Keep valid DER and the matching subject key, but corrupt
                // the issuer's signature on the leaf certificate.
                let mut leaf = identity.certificates[0].as_ref().to_vec();
                *leaf.last_mut().unwrap() ^= 1;
                identity.certificates[0] = leaf.into();
            }
            let server_config = tls::server_config(
                identity,
                None,
                std::sync::Arc::new(capntproto::rpc::SystemClock),
            )
            .unwrap();
            let serving = async {
                let (io, _) = listener.accept().await.unwrap();
                let _ = tls::accept(io, server_config, TIMEOUT).await;
            };
            let calling = async {
                let mut read = [0; 18 * 1024];
                let mut write = [0; 18 * 1024];
                let mut certificate = [0; 2048];
                let certificate = if fault == 8 {
                    &mut certificate[..1]
                } else {
                    &mut certificate[..]
                };
                let error = embedded::connect(
                    addr,
                    &config,
                    rand::rngs::OsRng,
                    embedded::Buffers {
                        read: &mut read,
                        write: &mut write,
                        certificate,
                    },
                    TIMEOUT,
                )
                .await
                .err()
                .unwrap();
                assert_ne!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut,
                    "validation failure: {error}"
                );
            };
            futures::join!(serving, calling);
        })
        .await;
    }
}

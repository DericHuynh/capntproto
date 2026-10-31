//! Independent loopback server runtime; four client/server RPC driver pairs.
use capntproto::rpc::{tcp::tls, SystemClock};
use capntproto_configuration_example::{
    self as example, Driver, Pki, Runtime, Transport, DEADLINE,
};
use std::{
    net::SocketAddr,
    sync::{mpsc, Arc},
};
use tokio_util::compat::TokioAsyncReadCompatExt;
fn backend(
    pki: &Pki,
    transport: Transport,
    driver: Driver,
    mtls: bool,
) -> (
    SocketAddr,
    std::thread::JoinHandle<Result<(), String>>,
    tokio::sync::oneshot::Sender<()>,
) {
    let identity = example::copy_identity(&pki.server);
    let root = pki.root.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    let (done, finished) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tokio::task::LocalSet::new().run_until(async {
                tokio::time::timeout(DEADLINE, async {
                    match transport {
                        Transport::Tcp | Transport::Tls => {
                            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                            tx.send(listener.local_addr()?)?;
                            let (io, _) = listener.accept().await?;
                            match transport {
                                Transport::Tcp => {
                                    example::server_stream(io.compat(), driver).await?
                                }
                                _ => {
                                    let config = tls::server_config(
                                        identity,
                                        if mtls {
                                            Some(example::roots(&root)?)
                                        } else {
                                            None
                                        },
                                        Arc::new(SystemClock),
                                    )?;
                                    let io = tls::accept(io, config, DEADLINE).await?;
                                    example::server_stream(io.compat(), driver).await?;
                                }
                            }
                        }
                        #[cfg(feature = "webtransport")]
                        Transport::Webtransport => {
                            let server = capntproto::rpc::webtransport::Server::bind(
                                "127.0.0.1:0".parse()?,
                                identity.certificates,
                                identity.private_key,
                                Arc::new(SystemClock),
                            )?;
                            tx.send(server.local_addr()?)?;
                            let request = server.accept().await?;
                            assert_eq!(request.path(), "/rpc");
                            let mut session = request.accept().await?;
                            example::server_stream(session.accept_bi().await?, driver).await?;
                        }
                    }
                    // Keep QUIC's reactor alive until the client observes its FIN acknowledgements.
                    let _ = finished.await;
                    Ok::<_, Box<dyn std::error::Error>>(())
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
            }))
    });
    (
        rx.recv_timeout(DEADLINE).expect("backend readiness"),
        thread,
        done,
    )
}
fn client(
    runtime: Runtime,
    address: SocketAddr,
    transport: Transport,
    driver: Driver,
    pki: &Pki,
    mtls: bool,
    value: &str,
) -> example::Result<String> {
    let identity = mtls.then(|| example::copy_identity(&pki.client));
    match runtime {
        Runtime::Tokio => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(tokio::task::LocalSet::new().run_until(async {
                tokio::time::timeout(
                    DEADLINE,
                    example::tokio_client(
                        address, transport, driver, &pki.root, identity, "Hello, ", value,
                    ),
                )
                .await?
            })),
        #[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
        Runtime::Monoio => capntproto::rpc::monoio::runtime::RuntimeBuilder::<
            capntproto::rpc::monoio::runtime::LegacyDriver,
        >::new()
        .enable_timer()
        .build()?
        .block_on(async {
            capntproto::rpc::monoio::runtime::time::timeout(
                DEADLINE,
                example::monoio_client(
                    address, transport, driver, &pki.root, identity, "Hello, ", value,
                ),
            )
            .await?
        }),
    }
}
fn matrix(runtime: Runtime, transport: Transport, mtls: bool) {
    let pki = Pki::generate().unwrap();
    for server in [Driver::Legacy, Driver::Portable] {
        for caller in [Driver::Legacy, Driver::Portable] {
            let (address, serving, done) = backend(&pki, transport, server, mtls);
            let value = "hello 🦀".repeat(4096);
            let result = client(runtime, address, transport, caller, &pki, mtls, &value);
            let _ = done.send(());
            let served = serving.join().unwrap();
            assert_eq!(
                result.unwrap_or_else(|e| panic!(
                    "{runtime:?}/{transport:?} {caller:?}->{server:?}: {e}"
                )),
                format!("Hello, {value}")
            );
            served.unwrap();
        }
    }
}
#[test]
fn tokio_tcp() {
    matrix(Runtime::Tokio, Transport::Tcp, false);
}
#[test]
fn tokio_tls() {
    matrix(Runtime::Tokio, Transport::Tls, false);
}
#[test]
fn tokio_mutual_tls() {
    matrix(Runtime::Tokio, Transport::Tls, true);
}
#[cfg(feature = "webtransport")]
#[test]
fn tokio_webtransport() {
    matrix(Runtime::Tokio, Transport::Webtransport, false);
}
#[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
#[test]
fn monoio_tls() {
    matrix(Runtime::Monoio, Transport::Tls, false);
}
#[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
#[test]
fn monoio_mutual_tls() {
    matrix(Runtime::Monoio, Transport::Tls, true);
}
#[cfg(all(
    feature = "monoio",
    feature = "webtransport",
    any(target_os = "linux", target_os = "macos")
))]
#[test]
fn monoio_webtransport() {
    matrix(Runtime::Monoio, Transport::Webtransport, false);
}
#[test]
fn mutual_tls_rejects_missing_client_identity() {
    for driver in [Driver::Legacy, Driver::Portable] {
        let pki = Pki::generate().unwrap();
        let (address, serving, done) = backend(&pki, Transport::Tls, driver, true);
        assert!(client(
            Runtime::Tokio,
            address,
            Transport::Tls,
            driver,
            &pki,
            false,
            "rejected"
        )
        .is_err());
        let _ = done.send(());
        assert!(serving.join().unwrap().is_err());
    }
}
#[test]
fn development_credentials_are_private_and_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    Pki::generate().unwrap().write(directory.path()).unwrap();
    let certificate = std::fs::read(directory.path().join("server.pem")).unwrap();
    let key = std::fs::read(directory.path().join("server-key.pem")).unwrap();
    assert!(Pki::generate().unwrap().write(directory.path()).is_err());
    assert_eq!(
        std::fs::read(directory.path().join("server.pem")).unwrap(),
        certificate
    );
    assert_eq!(
        std::fs::read(directory.path().join("server-key.pem")).unwrap(),
        key
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.path().join("server-key.pem"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

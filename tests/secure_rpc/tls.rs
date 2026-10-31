use super::common::*;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::TwoPartyServer};
use capntproto::rpc::{security::rustls, tcp::tls};
use std::{cell::RefCell, io, rc::Rc, sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};

async fn roundtrip(mutual: bool) {
    local(async {
        let pki = Pki::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = pki.server(mutual);
        let task = tokio::task::spawn_local(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let stream = tls::accept(socket, config, TIMEOUT).await.unwrap();
            assert_eq!(
                stream.get_ref().1.protocol_version(),
                Some(rustls::ProtocolVersion::TLSv1_3)
            );
            assert_eq!(stream.get_ref().1.peer_certificates().is_some(), mutual);
            tls::driver(stream, Some(bootstrap()), Side::Server, Default::default()).await
        });
        let stream = tls::connect(
            address,
            "localhost".try_into().unwrap(),
            pki.client(mutual),
            TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(
            stream.get_ref().1.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
        assert_eq!(
            stream.get_ref().1.alpn_protocol(),
            Some(capntproto::rpc::ALPN)
        );
        assert_eq!(stream.get_ref().1.peer_certificates().unwrap().len(), 1);
        assert!(stream.get_ref().0.nodelay().unwrap());
        exercise(tls::driver(stream, None, Side::Client, Default::default())).await;
        task.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tls_rpc_capabilities_pipeline_large_messages_and_shutdown() {
    roundtrip(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn mtls_rpc_authenticates_both_peers() {
    roundtrip(true).await;
}

async fn rejected(
    server: Arc<rustls::ServerConfig>,
    client: Arc<rustls::ClientConfig>,
    name: &'static str,
    server_must_reject: bool,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (client, server) = tokio::join!(
        tls::connect(address, name.try_into().unwrap(), client, TIMEOUT),
        async {
            let (socket, _) = listener.accept().await.unwrap();
            tls::accept(socket, server, TIMEOUT).await
        },
    );
    let result = if server_must_reject { server } else { client };
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
async fn rejects_untrusted_wrong_name_expired_and_wrong_usage_servers() {
    let pki = Pki::new();
    rejected(
        pki.server(false),
        Pki::new().client(false),
        "localhost",
        false,
    )
    .await;
    rejected(pki.server(false), pki.client(false), "other.local", false).await;
    for (expired, client_usage) in [(true, false), (false, true)] {
        let server = tls::server_config(
            pki.identity("localhost", client_usage, expired),
            None,
            std::sync::Arc::new(capntproto::rpc::SystemClock),
        )
        .unwrap();
        rejected(server, pki.client(false), "localhost", false).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn mtls_rejects_missing_untrusted_expired_and_wrong_usage_clients() {
    let pki = Pki::new();
    rejected(pki.server(true), pki.client(false), "localhost", true).await;
    let untrusted = tls::client_config(
        pki.roots(),
        Some(Pki::new().identity("client.local", true, false)),
        std::sync::Arc::new(capntproto::rpc::SystemClock),
    )
    .unwrap();
    rejected(pki.server(true), untrusted, "localhost", true).await;
    for (expired, client_usage) in [(true, true), (false, false)] {
        let client = tls::client_config(
            pki.roots(),
            Some(pki.identity("client.local", client_usage, expired)),
            std::sync::Arc::new(capntproto::rpc::SystemClock),
        )
        .unwrap();
        rejected(pki.server(true), client, "localhost", true).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_missing_or_mismatched_rpc_alpn() {
    let pki = Pki::new();
    for protocols in [vec![], vec![b"h2".to_vec()]] {
        let mut client = pki.client(false).as_ref().clone();
        client.alpn_protocols = protocols;
        rejected(pki.server(false), Arc::new(client), "localhost", false).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn listener_isolates_invalid_and_stalled_handshakes_and_cancellation() {
    local(async {
        let pki = Pki::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (server, driver) = TwoPartyServer::new(bootstrap());
        let server_task = tokio::task::spawn_local(driver);
        let rejected = Rc::new(RefCell::new(Vec::new()));
        let errors = rejected.clone();
        let mut listening = Box::pin(tls::listen(
            &server,
            &listener,
            pki.server(true),
            Default::default(),
            capntproto::rpc::AcceptOptions::default(),
            move |_, error| errors.borrow_mut().push(error.kind()),
        ));
        // The first peer never sends ClientHello; a serial acceptor would block.
        let mut stalled = TcpStream::connect(address).await.unwrap();
        let client = async {
            let mut bad = tls::connect(
                address,
                "localhost".try_into().unwrap(),
                pki.client(false),
                TIMEOUT,
            )
            .await;
            if let Ok(stream) = &mut bad {
                let _ = stream.read_u8().await;
            }
            let stream = tls::connect(
                address,
                "localhost".try_into().unwrap(),
                pki.client(true),
                TIMEOUT,
            )
            .await
            .unwrap();
            let mut driver = tls::driver(stream, None, Side::Client, Default::default());
            let cap = driver
                .bootstrap::<capntproto_test_support::structured::runtime_test_capnp::harness::Client>(
                );
            let close = driver.get_disconnector();
            let task = tokio::task::spawn_local(driver);
            assert_eq!(echo(&cap, 81).await.unwrap(), 81);
            (cap, close, task)
        };
        let (cap, close, task) = tokio::select! {
            result = client => result,
            result = &mut listening => panic!("listener exited: {result:?}"),
        };
        assert!(!rejected.borrow().is_empty());
        drop(listening);
        assert_eq!(
            stalled.read_u8().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(echo(&cap, 82).await.unwrap(), 82);
        close.await.unwrap();
        task.await.unwrap().unwrap();
        server.drain().await.unwrap();
        drop(server);
        server_task.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn handshake_deadlines_release_stalled_sockets() {
    let pki = Pki::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (socket, _) = listener.accept().await.unwrap();
    let error = tls::accept(socket, pki.server(false), Duration::from_millis(50))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        client.read_u8().await.unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test(flavor = "current_thread")]
async fn portable_tls_wakes_only_one_of_32_parked_calls() {
    use capnp_rpc::portable::RpcSystem;
    local(async {
        let pki = Pki::new();
        let calls = SparseCalls::new();
        let bootstrap = calls.bootstrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = pki.server(false);
        let serving = tokio::task::spawn_local(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let stream = tls::accept(socket, config, TIMEOUT).await.unwrap();
            RpcSystem::new(
                Box::new(tls::network(stream, Side::Server, Default::default())),
                Some(bootstrap),
                portable_options(),
            )
            .unwrap()
            .run()
            .await
        });
        let stream = tls::connect(
            address,
            "localhost".try_into().unwrap(),
            pki.client(false),
            TIMEOUT,
        )
        .await
        .unwrap();
        let mut system = RpcSystem::new(
            Box::new(tls::network(stream, Side::Client, Default::default())),
            None,
            portable_options(),
        )
        .unwrap();
        let cap = system.bootstrap(Side::Server);
        let close = system.get_disconnector();
        let driving = tokio::task::spawn_local(system.run());
        calls.exercise(cap).await;
        close.await.unwrap();
        driving.await.unwrap().unwrap();
        serving.await.unwrap().unwrap();
    })
    .await;
}

struct FixedClock(Option<u64>);
impl capntproto::rpc::TlsClock for FixedClock {
    fn now(&self) -> Option<u64> {
        self.0
    }
}

#[tokio::test(flavor = "current_thread")]
async fn caller_clock_controls_server_and_client_certificate_validity() {
    for now in [None, Some(0), Some(u64::MAX)] {
        let pki = Pki::new();
        let client = tls::client_config(pki.roots(), None, Arc::new(FixedClock(now))).unwrap();
        rejected(pki.server(false), client, "localhost", false).await;
        let server = tls::server_config(
            pki.identity("localhost", false, false),
            Some(pki.roots()),
            Arc::new(FixedClock(now)),
        )
        .unwrap();
        rejected(server, pki.client(true), "localhost", true).await;
    }
}

use capnp_rpc::rpc_twoparty_capnp::Side;
use reproto::rpc::Connection;
use reproto_test_support::runtime_test_capnp::harness;
use std::{future::Future, rc::Rc, time::Duration};
use tokio::io::AsyncReadExt;

const TIMEOUT: Duration = Duration::from_secs(3);

struct Echo;
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(params.get()?.get_value());
        Ok(())
    }
}

async fn local(test: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async { tokio::time::timeout(TIMEOUT, test).await.unwrap() })
        .await;
}

async fn echo(cap: &harness::Client) -> capnp::Result<u32> {
    let mut request = cap.echo_request();
    request.get().set_value(42);
    Ok(request.send().promise.await?.get()?.get_value())
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_owned_connection_bootstraps_both_sides_and_shuts_down() {
    local(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let service: harness::Client = capnp_rpc::new_client(Echo);
        let (client, server) = tokio::join!(
            reproto::rpc::tcp::connect(
                listener.local_addr().unwrap(),
                Some(service.client.clone()),
                Default::default()
            ),
            listener.accept()
        );
        let a = Connection::spawn(client.unwrap());
        let b = Connection::spawn(reproto::rpc::tcp::client(
            server.unwrap().0,
            Some(service.client),
            Side::Server,
            Default::default(),
        ));
        let cap_a = a.bootstrap();
        let cap_b = b.bootstrap();
        assert_eq!(echo(&cap_a).await.unwrap(), 42);
        assert_eq!(echo(&cap_b).await.unwrap(), 42);
        let done = a.on_disconnect();
        let other_done = b.on_disconnect();
        a.shutdown(TIMEOUT).await.unwrap();
        done.clone().await.unwrap();
        done.await.unwrap();
        other_done.await.unwrap();
        assert!(echo(&cap_a).await.is_err());
        assert!(echo(&cap_b).await.is_err());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn drop_and_canceled_unpolled_shutdown_close_io_despite_retained_clients() {
    local(async {
        for cancel_shutdown in [false, true] {
            let (io, mut peer) = tokio::io::duplex(1);
            let owner = Connection::new(io, None, Side::Client);
            let cap: harness::Client = owner.bootstrap();
            let done = owner.on_disconnect();
            if cancel_shutdown {
                drop(owner.shutdown(TIMEOUT));
            } else {
                drop(owner);
            }
            assert_eq!(done.await.unwrap_err().kind, capnp::ErrorKind::Disconnected);
            assert!(echo(&cap).await.is_err());
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).await.unwrap();
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_deadline_cleans_up_a_blocked_writer() {
    local(async {
        let (io, mut peer) = tokio::io::duplex(1);
        let owner = Connection::new(io, None, Side::Client);
        let cap: harness::Client = owner.bootstrap();
        let _call = cap.echo_request().send();
        let done = owner.on_disconnect();
        let error = owner.shutdown(Duration::from_millis(10)).await.unwrap_err();
        assert!(error.extra.contains("timed out"), "{error}");
        assert!(done.await.is_err());
        assert!(echo(&cap).await.is_err());
        peer.read_to_end(&mut Vec::new()).await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn canceling_active_shutdown_aborts_the_driver() {
    local(async {
        let (io, mut peer) = tokio::io::duplex(1);
        let owner = Connection::new(io, None, Side::Client);
        let done = owner.on_disconnect();
        {
            let shutdown = owner.shutdown(TIMEOUT);
            tokio::pin!(shutdown);
            assert!(futures::poll!(&mut shutdown).is_pending());
        }
        assert!(done.await.is_err());
        peer.read_to_end(&mut Vec::new()).await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn zero_shutdown_timeout_reports_error_and_releases_io() {
    local(async {
        let (io, mut peer) = tokio::io::duplex(1);
        let owner = Connection::new(io, None, Side::Client);
        let done = owner.on_disconnect();
        assert_eq!(
            owner.shutdown(Duration::ZERO).await.unwrap_err().kind,
            capnp::ErrorKind::Failed
        );
        assert!(done.await.is_err());
        peer.read_to_end(&mut Vec::new()).await.unwrap();
    })
    .await;
}

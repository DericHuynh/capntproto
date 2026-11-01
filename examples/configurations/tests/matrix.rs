//! Exercise the supported TLS configurations over real TCP, with both peers driven concurrently.
use capntproto::rpc::{tcp::tls, SystemClock};
use capntproto_configuration_example::{self as example, Pki, DEADLINE};
use std::sync::Arc;
use tokio_util::compat::TokioAsyncReadCompatExt;
#[tokio::test(flavor = "current_thread")]
async fn server_authentication_and_mutual_tls_run_the_actual_service() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for mutual in [false, true] {
                let pki = Pki::generate().unwrap();
                let config = tls::server_config(
                    example::copy_identity(&pki.server),
                    mutual.then(|| example::roots(&pki.root).unwrap()),
                    Arc::new(SystemClock),
                )
                .unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let serve = async {
                    let (socket, _) = listener.accept().await?;
                    let stream = tls::accept(socket, config, DEADLINE).await?;
                    example::server_stream(stream.compat()).await
                };
                let call = example::client(
                    address,
                    &pki.root,
                    mutual.then(|| example::copy_identity(&pki.client)),
                    "Hello, ",
                    "world",
                );
                let (server, client) =
                    tokio::time::timeout(DEADLINE, async { futures::join!(serve, call) })
                        .await
                        .unwrap();
                server.unwrap();
                assert_eq!(client.unwrap(), "Hello, world");
            }
        })
        .await;
}

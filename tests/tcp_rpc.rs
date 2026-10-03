use capnp_rpc::twoparty::TwoPartyServer;
use futures::FutureExt;
use reproto::rpc::tcp;
use reproto_test_support::runtime_test_capnp::harness;
use std::{rc::Rc, time::Duration};

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
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        mut results: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = params.get()?.get_cap()?;
        assert_eq!(echo(&cap, 19).await?, 19);
        results.get().set_cap(cap);
        Ok(())
    }
}
async fn echo(cap: &harness::Client, value: u32) -> capnp::Result<u32> {
    let mut call = cap.echo_request();
    call.get().set_value(value);
    Ok(call.send().promise.await?.get()?.get_value())
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_listener_preserves_capabilities_drain_and_existing_connections_after_cancel() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let bootstrap: harness::Client = capnp_rpc::new_client(Echo);
                let (server, driver) = TwoPartyServer::new(bootstrap.client);
                let server_task = tokio::task::spawn_local(driver);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let mut listening = Box::pin(tcp::listen(&server, &listener, Default::default()));
                let mut driver =
                    tcp::connect(listener.local_addr().unwrap(), None, Default::default())
                        .await
                        .unwrap();
                let cap: harness::Client = driver.bootstrap();
                let disconnected = driver.on_disconnect();
                let close = driver.get_disconnector();
                let client_task = tokio::task::spawn_local(driver);
                tokio::select! {
                    result = echo(&cap, 17) => assert_eq!(result.unwrap(), 17),
                    result = &mut listening => panic!("listener ended: {result:?}"),
                }
                let mut drain = server.drain();
                assert!((&mut drain).now_or_never().is_none());
                drop(listening);
                assert_eq!(echo(&cap, 18).await.unwrap(), 18);
                let callback: harness::Client = capnp_rpc::new_client(Echo);
                let mut bounce = cap.bounce_request();
                bounce.get().set_cap(callback);
                let result = bounce.send().promise.await.unwrap();
                let returned = result.get().unwrap().get_cap().unwrap();
                assert_eq!(echo(&returned, 20).await.unwrap(), 20);
                close.await.unwrap();
                client_task.await.unwrap().unwrap();
                disconnected.await.unwrap();
                drain.await.unwrap();
                assert!(echo(&cap, 21).await.is_err());
                drop(server);
                server_task.await.unwrap().unwrap();
            })
            .await
            .expect("TCP facade timed out");
        })
        .await;
}

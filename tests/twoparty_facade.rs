use capnp::{capability::Promise, Error, ErrorKind};
use capnp_rpc::{
    rpc_twoparty_capnp::Side,
    twoparty::{TwoPartyClient, TwoPartyServer},
};
use futures::{AsyncRead, AsyncWrite, FutureExt};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::Cell,
    future::Future,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio_util::compat::TokioAsyncReadCompatExt;

#[path = "twoparty_facade/verification.rs"]
mod verification;

struct Echo;
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = params.get()?.get_value();
        if value == u32::MAX {
            return Err(Error::failed("intentional failure".into()));
        }
        results.get().set_value(value);
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        mut results: harness::BounceResults,
    ) -> capnp::Result<()> {
        // Actually exercise authority passed in the request, then return it.
        let cap = params.get()?.get_cap()?;
        assert_eq!(echo(&cap, 73).await?, 73);
        results.get().set_cap(cap);
        Ok(())
    }
    async fn generic(
        self: Rc<Self>,
        _: harness::GenericParams,
        mut results: harness::GenericResults,
    ) -> capnp::Result<()> {
        results.get().set_value(&vec![0x73; 100_000][..])?;
        Ok(())
    }
}
fn bootstrap() -> capnp::capability::Client {
    capnp_rpc::new_client::<harness::Client, _>(Echo).client
}
async fn echo(client: &harness::Client, value: u32) -> capnp::Result<u32> {
    let mut request = client.echo_request();
    request.get().set_value(value);
    Ok(request.send().promise.await?.get()?.get_value())
}
async fn settle() {
    for _ in 0..24 {
        tokio::task::yield_now().await;
    }
}
async fn local(task: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(20), task)
                .await
                .expect("facade test timed out");
        })
        .await;
}

struct Tracked<T> {
    io: T,
    drops: Rc<Cell<u64>>,
}
impl<T> Drop for Tracked<T> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for Tracked<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for Tracked<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_close(cx)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn owned_drain_tracks_new_connections_and_disconnect_observers_do_not_retain_io() {
    local(async {
        let (server, driver) = TwoPartyServer::new(bootstrap());
        let server_task = tokio::task::spawn_local(driver);
        let drops = Rc::new(Cell::new(0));
        let mut clients = Vec::new();
        let mut caps = Vec::new();
        let mut observers = Vec::new();
        let initially_empty = server.drain();
        let mut draining = None;
        for i in 0..2 {
            let (a, b) = tokio::io::duplex(128);
            server
                .accept(Tracked {
                    io: a.compat(),
                    drops: drops.clone(),
                })
                .unwrap();
            if i == 0 {
                draining = Some(server.drain());
            }
            let mut client = TwoPartyClient::new(b.compat());
            let cap: harness::Client = client.bootstrap();
            observers.push(client.on_disconnect());
            clients.push(tokio::task::spawn_local(client));
            assert_eq!(echo(&cap, 73 + i).await.unwrap(), 73 + i);
            caps.push(cap);
            assert!(draining.as_mut().unwrap().now_or_never().is_none());
        }
        initially_empty.await.unwrap();
        let mut draining = draining.unwrap();
        let another_drain = server.drain();
        clients.remove(0).abort();
        settle().await;
        assert_eq!(drops.get(), 1);
        assert!((&mut draining).now_or_never().is_none());
        assert!(observers[0].clone().await.is_err());
        assert!(observers[0].clone().await.is_err());
        assert_eq!(echo(&caps[1], 88).await.unwrap(), 88);
        assert_eq!(
            echo(&caps[0], 88).await.unwrap_err().kind,
            ErrorKind::Disconnected
        );
        clients.pop().unwrap().abort();
        draining.await.unwrap();
        another_drain.await.unwrap();
        assert_eq!(drops.get(), 2);
        server.drain().await.unwrap();
        drop(server);
        server_task.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_streams_support_capabilities_large_partial_io_and_reverse_bootstrap() {
    local(async {
        let (a, b) = tokio::io::duplex(37);
        let drops = Rc::new(Cell::new(0));
        let mut a = Tracked {
            io: a.compat(),
            drops: drops.clone(),
        };
        let mut b = Tracked {
            io: b.compat(),
            drops: drops.clone(),
        };
        let (server, driver) = TwoPartyServer::new(bootstrap());
        let server_task = tokio::task::spawn_local(driver);
        let mut hosted = server.accept_borrowed(&mut a);
        let reverse: harness::Client = hosted.bootstrap();
        let mut client = TwoPartyClient::new_borrowed(
            &mut b,
            Some(bootstrap()),
            Side::Client,
            Default::default(),
        );
        let remote: harness::Client = client.bootstrap();
        let disconnected = client.on_disconnect();
        let queue = client.outgoing_queue();
        let disconnect = client.get_disconnector();
        let work = async {
            assert_eq!(echo(&remote, 73).await.unwrap(), 73);
            assert_eq!(echo(&reverse, 91).await.unwrap(), 91);
            let mut request = remote.bounce_request();
            request.get().set_cap(capnp_rpc::new_client(Echo));
            let returned = request
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_cap()
                .unwrap();
            assert_eq!(echo(&returned, 123).await.unwrap(), 123);
            let response = remote.generic_request().send().promise.await.unwrap();
            assert_eq!(
                response.get().unwrap().get_value().unwrap(),
                vec![0x73; 100_000]
            );
            // Borrowed accepts never delay the owned connection drain.
            server.drain().await.unwrap();
            disconnect.await.unwrap();
        };
        let client_done = Box::pin(async { futures::join!(work, &mut client) });
        match futures::future::select(client_done, &mut hosted).await {
            futures::future::Either::Left((((), result), _)) => result.unwrap(),
            _ => panic!("server ended before client shutdown"),
        }
        disconnected.await.unwrap();
        drop(client);
        assert_eq!(drops.get(), 0);
        // The owner closes its stream once its borrowed driver is finished.
        // Otherwise the peer's shutdown reply can block in this 37-byte pipe.
        drop(b);
        hosted.await.unwrap();
        assert_eq!(queue.snapshot().message_count, 0);
        assert_eq!(
            echo(&remote, 1).await.unwrap_err().kind,
            ErrorKind::Disconnected
        );
        drop(server);
        server_task.await.unwrap().unwrap();
        drop(a);
        assert_eq!(drops.get(), 2);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_releases_borrow_without_closing_stream_and_cleans_unpolled_owned_connections()
{
    local(async {
        use futures::{AsyncReadExt, AsyncWriteExt};
        let drops = Rc::new(Cell::new(0));
        let (a, b) = tokio::io::duplex(128);
        let mut a = Tracked {
            io: a.compat(),
            drops: drops.clone(),
        };
        let mut b = b.compat();
        let (server, driver) = TwoPartyServer::new(bootstrap());
        let borrowed = server.accept_borrowed(&mut a);
        let observer = borrowed.on_disconnect();
        drop(borrowed);
        assert_eq!(observer.await.unwrap_err().kind, ErrorKind::Disconnected);
        a.write_all(b"reusable").await.unwrap();
        let mut bytes = [0; 8];
        b.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"reusable");
        assert_eq!(drops.get(), 0);
        server.accept(a).unwrap();
        let drain = server.drain();
        drop(driver); // even before its first poll
        assert_eq!(drops.get(), 1);
        assert_eq!(drain.await.unwrap_err().kind, ErrorKind::Disconnected);
        let (c, _) = tokio::io::duplex(128);
        assert!(server
            .accept(Tracked {
                io: c.compat(),
                drops: drops.clone()
            })
            .is_err());
        assert_eq!(drops.get(), 2);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn socket_listener_cancellation_keeps_accepted_capabilities_alive() {
    local(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let incoming = futures::stream::try_unfold(listener, |listener| async {
            let (stream, _) = listener.accept().await?;
            Ok::<_, io::Error>(Some((stream.compat(), listener)))
        });
        let (server, driver) = TwoPartyServer::new(bootstrap());
        let task = tokio::task::spawn_local(driver);
        let listen = Box::pin(server.listen(incoming));
        let client_io = tokio::net::TcpStream::connect(address).await.unwrap();
        let (cap, client_task) = reproto::rpc::client::<harness::Client>(client_io);
        let call = echo(&cap, 73);
        futures::pin_mut!(call);
        match futures::future::select(call, listen).await {
            futures::future::Either::Left((result, _)) => assert_eq!(result.unwrap(), 73),
            _ => panic!("listener ended"),
        }
        // The returned listener future was dropped by the match. Its accepted
        // stream remains owned by the server driver.
        assert_eq!(echo(&cap, 91).await.unwrap(), 91);
        client_task.abort();
        server.drain().await.unwrap();
        task.abort();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn listener_errors_are_reported_without_canceling_accepted_connections() {
    local(async {
        let (a, b) = tokio::io::duplex(128);
        let (mut server, driver) = TwoPartyServer::new(bootstrap());
        server.set_trace_encoder(|_| "facade diagnostic".into());
        let task = tokio::task::spawn_local(driver);
        let incoming =
            futures::stream::iter([Ok(a.compat()), Err(io::Error::other("listener failed"))]);
        assert!(server
            .listen(incoming)
            .await
            .unwrap_err()
            .to_string()
            .contains("listener failed"));
        let mut client = TwoPartyClient::new(b.compat());
        let cap: harness::Client = client.bootstrap();
        let queue = client.outgoing_queue();
        assert!(client.get_current_queue_count() > 0);
        assert!(client.get_current_queue_size() > 0);
        let _ = client.get_outgoing_message_wait_time();
        let client_task = tokio::task::spawn_local(client);
        assert_eq!(echo(&cap, 73).await.unwrap(), 73);
        let error = echo(&cap, u32::MAX).await.unwrap_err();
        assert_eq!(error.remote_trace(), Some("facade diagnostic"));
        assert_eq!(echo(&cap, 91).await.unwrap(), 91);
        client_task.abort();
        server.drain().await.unwrap();
        assert_eq!(queue.snapshot().message_count, 0);
        task.abort();
    })
    .await;
}

struct BrokenIo {
    closes: Rc<Cell<u64>>,
    fail_read: bool,
}
impl AsyncRead for BrokenIo {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.fail_read {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid input",
            )))
        } else {
            Poll::Pending
        }
    }
}
impl AsyncWrite for BrokenIo {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::other("failed output")))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.closes.set(self.closes.get() + 1);
        Poll::Ready(Ok(()))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn failed_owned_connection_is_reported_and_other_connections_remain_callable() {
    local(async {
        let errors = Rc::new(Cell::new(0));
        let seen = errors.clone();
        let (server, driver) = TwoPartyServer::with_error_handler(bootstrap(), move |error| {
            assert!(error.to_string().contains("failed output"));
            seen.set(seen.get() + 1);
        });
        let task = tokio::task::spawn_local(driver);
        let closes = Rc::new(Cell::new(0));
        server
            .accept(BrokenIo {
                closes: closes.clone(),
                fail_read: true,
            })
            .unwrap();
        let (a, b) = tokio::io::duplex(128);
        server.accept(a.compat()).unwrap();
        let (cap, client_task) = reproto::rpc::client::<harness::Client>(b);
        assert_eq!(echo(&cap, 73).await.unwrap(), 73);
        settle().await;
        assert_eq!(errors.get(), 1);
        assert_eq!(closes.get(), 1);
        assert_eq!(echo(&cap, 91).await.unwrap(), 91);
        client_task.abort();
        server.drain().await.unwrap();
        task.abort();
        // The same failure path must also finish through the scoped IO pump.
        let mut io = BrokenIo {
            closes: closes.clone(),
            fail_read: false,
        };
        let mut client =
            TwoPartyClient::new_borrowed(&mut io, None, Side::Client, Default::default());
        let remote: harness::Client = client.bootstrap();
        let call = remote.echo_request().send().promise;
        let observer = client.on_disconnect();
        assert!((&mut client)
            .await
            .unwrap_err()
            .to_string()
            .contains("failed output"));
        assert!(observer
            .await
            .unwrap_err()
            .to_string()
            .contains("failed output"));
        assert_eq!(closes.get(), 2);
        assert_eq!(call.await.err().unwrap().kind, ErrorKind::Disconnected);
        let late: harness::Client = client.bootstrap();
        assert_eq!(
            echo(&late, 73).await.unwrap_err().kind,
            ErrorKind::Disconnected
        );
        client.get_disconnector().await.unwrap();
        client.clear_trace_encoder();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_cancellation_after_pending_io_breaks_caps_without_destroying_stream() {
    local(async {
        let (a, b) = tokio::io::duplex(1);
        let drops = Rc::new(Cell::new(0));
        let mut a = Tracked {
            io: a.compat(),
            drops: drops.clone(),
        };
        let mut client =
            TwoPartyClient::new_borrowed(&mut a, None, Side::Client, Default::default());
        let remote: harness::Client = client.bootstrap();
        let call = remote.echo_request().send().promise;
        let observer = client.on_disconnect();
        assert!((&mut client).now_or_never().is_none());
        // Bootstrap has a partial write and the reader is waiting for the peer.
        drop(client);
        assert_eq!(observer.await.unwrap_err().kind, ErrorKind::Disconnected);
        assert_eq!(call.await.err().unwrap().kind, ErrorKind::Disconnected);
        assert_eq!(drops.get(), 0);
        drop(a);
        assert_eq!(drops.get(), 1);
        drop(b);
    })
    .await;
}

#![cfg(any(target_os = "linux", target_os = "macos"))]
use capnp::{
    capability::{Client, FromClientHook},
    ErrorKind,
};
use capnp_rpc::rpc_twoparty_capnp::Side;
use futures::FutureExt;
use reproto::unix_rpc::{self, Options, TwoPartyServer};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::Cell,
    future::Future,
    io::Write,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::FileExt,
    },
    rc::Rc,
    time::Duration,
};
use tokio::net::UnixStream;

#[cfg(target_os = "linux")]
#[path = "unix_facade/verification.rs"]
mod verification;

fn descriptor(value: u8) -> OwnedFd {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&[value]).unwrap();
    file.into()
}
fn value(fd: &Option<Rc<OwnedFd>>) -> u64 {
    fd.as_ref()
        .map(|fd| {
            let file = std::fs::File::from(fd.try_clone().unwrap());
            let mut byte = [0];
            file.read_exact_at(&mut byte, 0).unwrap();
            u64::from(byte[0])
        })
        .unwrap_or(0)
}
struct Echo {
    number: u32,
    calls: Rc<Cell<u64>>,
}
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        results.get().set_value(self.number);
        Ok(())
    }
}
fn fd_cap(number: u8, calls: &Rc<Cell<u64>>) -> harness::Client {
    capnp_rpc::new_fd_client(
        Echo {
            number: number.into(),
            calls: calls.clone(),
        },
        descriptor(number),
    )
}
struct Service {
    received: Rc<Cell<u64>>,
    calls: Rc<Cell<u64>>,
}
impl harness::Server for Service {
    async fn fd_caps(
        self: Rc<Self>,
        params: harness::FdCapsParams,
        mut results: harness::FdCapsResults,
    ) -> capnp::Result<()> {
        let caps = params.get()?.get_caps()?;
        let mut received = 0;
        for i in 0..caps.len() {
            let cap = caps.get(i)?;
            let fd = cap.client.get_fd().await?;
            received += value(&fd) * 10u64.pow(i);
            // Descriptor truncation must not destroy the capability itself.
            assert_eq!(
                cap.echo_request().send().promise.await?.get()?.get_value(),
                i + 1
            );
        }
        self.received.set(received);
        let mut caps = results.get().init_caps(2);
        caps.set(0, fd_cap(3, &self.calls).into_client_hook());
        caps.set(1, fd_cap(4, &self.calls).into_client_hook());
        Ok(())
    }
}
fn service(received: &Rc<Cell<u64>>, calls: &Rc<Cell<u64>>) -> Client {
    capnp_rpc::new_client::<harness::Client, _>(Service {
        received: received.clone(),
        calls: calls.clone(),
    })
    .client
}
type Received = ([harness::Client; 2], [Option<Rc<OwnedFd>>; 2]);
async fn exchange(remote: &harness::Client, calls: &Rc<Cell<u64>>) -> Received {
    let mut request = remote.fd_caps_request();
    let mut root = request.get();
    let mut caps = root.reborrow().init_caps(2);
    caps.set(0, fd_cap(1, calls).into_client_hook());
    caps.set(1, fd_cap(2, calls).into_client_hook());
    let result = request.send().promise.await.unwrap();
    let returned = result.get().unwrap().get_caps().unwrap();
    let caps = [returned.get(0).unwrap(), returned.get(1).unwrap()];
    let fds = [
        caps[0].client.get_fd().await.unwrap(),
        caps[1].client.get_fd().await.unwrap(),
    ];
    for (i, cap) in caps.iter().enumerate() {
        assert_eq!(
            cap.echo_request()
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            i as u32 + 3
        );
    }
    (caps, fds)
}
async fn local(task: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(30), task)
                .await
                .expect("Unix facade test timed out");
        })
        .await;
}
fn socket_alive(fd: i32) -> bool {
    // SAFETY: F_GETFD only inspects the integer descriptor, including EBADF.
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[tokio::test(flavor = "current_thread")]
async fn listener_preserves_capabilities_fd_limits_and_connections_after_cancel() {
    local(async {
        let path = tempfile::tempdir().unwrap();
        let listener = tokio::net::UnixListener::bind(path.path().join("rpc")).unwrap();
        let received = Rc::new(Cell::new(0));
        let calls = Rc::new(Cell::new(0));
        let (server, driver) = TwoPartyServer::new(
            service(&received, &calls),
            Options {
                max_fds: 1,
                ..Default::default()
            },
        );
        let driver = tokio::task::spawn_local(driver);
        let socket = UnixStream::connect(path.path().join("rpc")).await.unwrap();
        let mut client = unix_rpc::client(socket, None, Side::Client, Options::default());
        let remote: harness::Client = client.bootstrap();
        let metrics = client.outgoing_queue();
        assert_eq!(metrics.snapshot().message_count, 1);
        let client_task = tokio::task::spawn_local(client);
        let listener_task = Box::pin(server.listen(&listener));
        let ((caps, fds), remaining) =
            match futures::future::select(Box::pin(exchange(&remote, &calls)), listener_task).await
            {
                futures::future::Either::Left(result) => result,
                _ => panic!("listener terminated"),
            };
        drop(remaining);
        assert_eq!(received.get(), 1);
        assert_eq!([value(&fds[0]), value(&fds[1])], [3, 4]);
        assert!(server.drain().now_or_never().is_none());
        let _ = exchange(&remote, &calls).await;
        assert_eq!(received.get(), 1); // receive limit resets per message
        client_task.abort();
        let _ = client_task.await;
        server.drain().await.unwrap();
        assert_eq!(metrics.snapshot().message_count, 0);
        assert_eq!(value(&fds[0]), 3); // escaped OS authority remains usable
        assert_eq!(
            caps[0]
                .echo_request()
                .send()
                .promise
                .await
                .err()
                .unwrap()
                .kind,
            ErrorKind::Disconnected
        );
        drop(server);
        driver.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_cancellation_preserves_original_socket_and_closes_queued_descriptors() {
    local(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let a_fd = a.as_raw_fd();
        let mut client =
            unix_rpc::client_borrowed(&mut a, None, Side::Client, Default::default()).unwrap();
        let remote: harness::Client = client.bootstrap();
        let metrics = client.outgoing_queue();
        let closed = client.on_disconnect();
        let mut call = remote.fd_caps_request();
        let calls = Rc::new(Cell::new(0));
        let fd = descriptor(1);
        let raw = fd.as_raw_fd();
        let cap: harness::Client = capnp_rpc::new_fd_client(Echo { number: 1, calls }, fd);
        call.get().init_caps(1).set(0, cap.into_client_hook());
        let call = call.send().promise;
        assert!(socket_alive(raw));
        // The queued outgoing message owns its own descriptor until canceled.
        assert!(metrics.snapshot().message_count >= 2);
        drop(client);
        assert!(closed.await.is_err());
        assert_eq!(call.await.err().unwrap().kind, ErrorKind::Disconnected);
        assert_eq!(metrics.snapshot().message_count, 0);
        assert!(!socket_alive(raw));
        assert!(socket_alive(a_fd));
        a.write_all(b"reusable").await.unwrap();
        let mut bytes = [0; 8];
        b.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"reusable");
        // Cancellation after a pending read must also preserve the caller socket.
        let (server, driver) = TwoPartyServer::new(
            service(&Rc::new(Cell::new(0)), &Rc::new(Cell::new(0))),
            Default::default(),
        );
        let mut accepted = server.accept_borrowed(&mut a).unwrap();
        assert!((&mut accepted).now_or_never().is_none());
        drop(accepted);
        a.write_all(b"still-open").await.unwrap();
        let mut bytes = [0; 10];
        b.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"still-open");
        drop(driver);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_peers_exchange_descriptor_capabilities_and_reverse_bootstrap() {
    local(async {
        let calls = Rc::new(Cell::new(0));
        let seen = Rc::new(Cell::new(0));
        let (server, driver) = TwoPartyServer::new(service(&seen, &calls), Default::default());
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let mut accepted = server.accept_borrowed(&mut a).unwrap();
        let reverse: harness::Client = accepted.bootstrap();
        let mut client = unix_rpc::client_borrowed(
            &mut b,
            Some(fd_cap(9, &calls).client),
            Side::Client,
            Default::default(),
        )
        .unwrap();
        let remote: harness::Client = client.bootstrap();
        let disconnected = accepted.on_disconnect();
        let operations = Box::pin(async {
            let (_, fds) = exchange(&remote, &calls).await;
            assert_eq!(seen.get(), 21);
            assert_eq!([value(&fds[0]), value(&fds[1])], [3, 4]);
            assert_eq!(value(&reverse.client.get_fd().await.unwrap()), 9);
            assert_eq!(
                reverse
                    .echo_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                9
            );
            server.drain().await.unwrap(); // neither borrowed connection counts
        });
        let drivers = Box::pin(futures::future::join(&mut accepted, &mut client));
        match futures::future::select(operations, drivers).await {
            futures::future::Either::Left(((), remaining)) => drop(remaining),
            _ => panic!("borrowed drivers stopped before operations completed"),
        }
        drop(accepted);
        drop(client);
        assert!(disconnected.await.is_err());
        assert!(socket_alive(a.as_raw_fd()));
        assert!(socket_alive(b.as_raw_fd()));
        drop(driver);
    })
    .await;
}

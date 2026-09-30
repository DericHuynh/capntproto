use super::*;
use capnp_rpc::{twoparty::TwoPartyNetwork, VatNetwork as _};
use std::io::Read;

#[tokio::test(flavor = "current_thread")]
async fn borrowed_partial_frame_cancellation_releases_ancillary_fds_without_shutdown() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut socket, mut peer) = UnixStream::pair().unwrap();
    let (passed, mut witness) = std::os::unix::net::UnixStream::pair().unwrap();
    witness.set_nonblocking(true).unwrap();
    let fd = Rc::new(OwnedFd::from(passed));
    write_message(&peer, &[0; 5], &[fd]).await.unwrap();
    socket.readable().await.unwrap();
    let raw = socket.as_raw_fd();
    let mut driver = client_borrowed(&mut socket, None, Side::Server, Default::default()).unwrap();
    let consumed = Box::pin(async {
        loop {
            let mut unread: libc::c_int = 0;
            // SAFETY: live caller-owned socket and a writable integer result.
            assert_eq!(unsafe { libc::ioctl(raw, libc::FIONREAD, &mut unread) }, 0);
            if unread == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        futures::future::select(consumed, &mut driver),
    )
    .await
    .unwrap();
    assert!(
        matches!(result, futures::future::Either::Left(_)),
        "partial frame unexpectedly completed"
    );
    assert_eq!(
        Read::read(&mut witness, &mut [0]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(driver);
    assert_eq!(
        Read::read(&mut witness, &mut [0]).unwrap(),
        0,
        "consumed ancillary fd leaked"
    );
    socket.write_all(b"open").await.unwrap();
    let mut bytes = [0; 4];
    peer.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"open");
}

#[tokio::test(flavor = "current_thread")]
async fn unix_queue_metrics_exclude_active_io_and_cancellation_releases_queued_fds() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let raw = socket.as_raw_fd();
    let mut network = VatNetwork::new(socket, Side::Client, Default::default());
    let metrics = network.outgoing_queue();
    let closed = network.output_closed();
    let mut connection = network.connect(Side::Server).unwrap();
    let mut receipts = Vec::new();
    let fd = tests::descriptor(42);
    let raw_fd = fd.as_raw_fd();
    for fds in [vec![], vec![fd]] {
        let mut message = connection.new_outgoing_message(0);
        message
            .get_body()
            .unwrap()
            .initn_as::<capnp::data::Builder>(1024 * 1024)
            .fill(19);
        message.set_fds(fds);
        receipts.push(message.send().0);
    }
    let all_bytes = metrics.snapshot().bytes;
    assert_eq!(metrics.snapshot().message_count, 2);
    assert!(all_bytes >= 2 * 1024 * 1024);
    network.inner.socket.writable().await.unwrap();
    let mut driver = network.drive_until_shutdown();
    assert!((&mut driver).now_or_never().is_none());
    assert_eq!(metrics.snapshot().message_count, 1);
    assert_eq!(metrics.snapshot().bytes, all_bytes / 2);
    drop(network);
    drop(driver);
    for receipt in receipts {
        assert!(receipt.await.is_err());
    }
    assert!(closed.await.is_err());
    assert_eq!(metrics.snapshot(), Default::default());
    // SAFETY: F_GETFD only inspects descriptor liveness, including EBADF.
    assert_eq!(unsafe { libc::fcntl(raw_fd, libc::F_GETFD) }, -1);
    drop(connection);
    assert_eq!(
        unsafe { libc::fcntl(raw, libc::F_GETFD) },
        -1,
        "diagnostics retained the socket"
    );
}

use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork};
use std::{
    cell::Cell,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::AsyncReadExt;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[tokio::test(flavor = "current_thread")]
async fn output_fence_closes_writer_without_waiting_for_input_or_handles() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (io, mut peer) = tokio::io::duplex(32);
            let (read, write) = tokio::io::split(io);
            let mut network = capnp_rpc::twoparty::VatNetwork::new(
                read.compat(),
                write.compat_write(),
                Side::Client,
                Default::default(),
            );
            let mut connection = network.connect(Side::Server).unwrap();
            let mut message = connection.new_outgoing_message(0);
            message
                .get_body()
                .unwrap()
                .set_as::<capnp::text::Owned>("queued output exceeds the duplex capacity")
                .unwrap();
            let (_sent, _message) = message.send();
            let drained = connection.shutdown(Ok(()));
            let closed = network.output_closed();
            let driver = tokio::task::spawn_local(network.drive_until_shutdown());
            let bytes = tokio::time::timeout(Duration::from_secs(1), async {
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).await.unwrap();
                drained.await.unwrap();
                closed.await.unwrap();
                bytes
            })
            .await
            .unwrap();
            let reader =
                capnp::serialize::read_message(&mut &bytes[..], Default::default()).unwrap();
            assert_eq!(
                reader.get_root::<capnp::text::Reader>().unwrap(),
                "queued output exceeds the duplex capacity"
            );
            assert!(
                !driver.is_finished(),
                "input and connection handles are still alive"
            );
            driver.abort();
        })
        .await;
}
struct BrokenWriter(Rc<Cell<bool>>);
impl futures::AsyncWrite for BrokenWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.0.set(true);
        Poll::Ready(Ok(()))
    }
}
#[tokio::test(flavor = "current_thread")]
async fn failed_output_still_closes_and_reports_error_before_input_disconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (read, _peer) = tokio::io::duplex(32);
            let was_closed = Rc::new(Cell::new(false));
            let mut network = capnp_rpc::twoparty::VatNetwork::new(
                read.compat(),
                BrokenWriter(was_closed.clone()),
                Side::Client,
                Default::default(),
            );
            let mut connection = network.connect(Side::Server).unwrap();
            let mut message = connection.new_outgoing_message(0);
            message
                .get_body()
                .unwrap()
                .set_as::<capnp::text::Owned>("valid message")
                .unwrap();
            let (_sent, _message) = message.send();
            let closed = network.output_closed();
            let driver = tokio::task::spawn_local(network.drive_until_shutdown());
            assert!(tokio::time::timeout(Duration::from_secs(1), closed)
                .await
                .unwrap()
                .is_err());
            assert!(was_closed.get());
            assert!(!driver.is_finished());
            driver.abort();
        })
        .await;
}

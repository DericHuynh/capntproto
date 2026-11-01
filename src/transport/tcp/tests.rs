use super::{bridge, framing};
use crate::native_shutdown::{Control, Frame, FRAME_BYTES};
use futures::FutureExt;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

async fn read_receipt(peer: &mut DuplexStream) -> Frame {
    assert_eq!(peer.read_u8().await.unwrap(), 1);
    assert_eq!(peer.read_u32().await.unwrap() as usize, FRAME_BYTES);
    let mut bytes = [0; FRAME_BYTES];
    peer.read_exact(&mut bytes).await.unwrap();
    Frame::decode(bytes).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn crossed_shutdown_waits_for_the_local_writer_before_acknowledging_the_peer() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (stream, mut peer) = tokio::io::duplex(1024);
        let (mut application, io) = crate::rpc::local_io::pair(1024);
        let control = Control::new();
        control.begin(Duration::from_secs(10)).unwrap();
        let request = Frame {
            kind: 1,
            nonce: [7; 16],
            bytes: 0,
        };
        framing::write(&mut peer, 1, &request.encode())
            .await
            .unwrap();

        let driver = bridge(stream, io, control.clone());
        tokio::pin!(driver);
        // Poll every bridge lane while shutdown is requested but the RPC
        // writer remains open. An early kind-2 acknowledgement would let the
        // peer close without acknowledging our not-yet-published local fence.
        // Three polls cover receiving, queuing and writing, even when select
        // visits a consumer before its producer.
        for _ in 0..3 {
            assert!(driver.as_mut().now_or_never().is_none());
        }
        assert!(peer.read_u8().now_or_never().is_none());
        application.shutdown().await.unwrap();

        let exchange = async {
            let local = read_receipt(&mut peer).await;
            assert_eq!(local.kind, 1);
            assert_eq!(local.bytes, 0);
            assert_eq!(read_receipt(&mut peer).await, Frame { kind: 3, ..request });
            framing::write(&mut peer, 1, &Frame { kind: 3, ..local }.encode())
                .await
                .unwrap();
        };
        let (result, ()) = tokio::join!(driver, exchange);
        result.unwrap();
        assert_eq!(control.wait().await.unwrap().bytes, 0);
    })
    .await
    .unwrap();
}

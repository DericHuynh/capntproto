use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

struct Output {
    bytes: Vec<u8>,
    maximum: usize,
    vectored: bool,
    writes: usize,
    flushes: usize,
    pending: bool,
    fail_flush: bool,
}
impl AsyncWrite for Output {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.pending {
            self.pending = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let count = bytes.len().min(self.maximum);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.writes += 1;
        self.pending = true;
        Poll::Ready(Ok(count))
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let bytes: Vec<_> = if self.vectored {
            bufs.iter().flat_map(|b| b.iter().copied()).collect()
        } else {
            bufs.iter().find(|b| !b.is_empty()).unwrap().to_vec()
        };
        self.as_mut().poll_write(cx, &bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.flushes += 1;
        Poll::Ready(if self.fail_flush {
            Err(io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(())
        })
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        panic!("framing must not shut down the connection")
    }
}
fn output(maximum: usize, vectored: bool) -> Output {
    Output {
        bytes: vec![],
        maximum,
        vectored,
        writes: 0,
        flushes: 0,
        pending: true,
        fail_flush: false,
    }
}

#[tokio::test]
async fn preserves_wire_bytes_across_every_partial_write_and_pending_boundary() {
    let payload: Vec<u8> = (0..32).collect();
    for vectored in [false, true] {
        for maximum in 1..=payload.len() + 5 {
            let mut sink = output(maximum, vectored);
            write(&mut sink, 1, &payload).await.unwrap();
            write(&mut sink, 0, &[7, 8]).await.unwrap();
            assert_eq!(
                sink.bytes,
                [&[1, 0, 0, 0, 32][..], &payload, &[0, 0, 0, 0, 2, 7, 8]].concat()
            );
            assert_eq!(sink.flushes, 2);
        }
    }
}

#[tokio::test]
async fn coalesces_a_ready_frame_and_propagates_write_zero_and_flush_failure() {
    let mut sink = output(usize::MAX, true);
    write(&mut sink, 0, &[7; 1024]).await.unwrap();
    assert_eq!(sink.writes, 1, "header and body must share the same write");
    assert_eq!(sink.flushes, 1);
    let mut sink = output(0, true);
    assert_eq!(
        write(&mut sink, 0, &[1]).await.unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(sink.flushes, 0);
    let mut sink = output(usize::MAX, true);
    sink.fail_flush = true;
    assert_eq!(
        write(&mut sink, 1, &[1]).await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(sink.flushes, 1);
}

#[tokio::test]
async fn batches_data_and_receipts_without_changing_framing_or_flush_fences() {
    let data = [17; 16384];
    let receipt = [29; crate::native_shutdown::FRAME_BYTES];
    let frames = [
        (0, &data[..]),
        (0, &data[..]),
        (0, &data[..]),
        (0, &data[..]),
        (0, &[31, 32][..]),
        (1, &receipt[..]),
    ];
    let mut expected = Vec::new();
    for (kind, bytes) in frames {
        expected.push(kind);
        expected.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        expected.extend_from_slice(bytes);
    }
    for vectored in [false, true] {
        for maximum in [16383, 16384, 16385, 65536, usize::MAX] {
            let mut sink = output(maximum, vectored);
            write_batch(&mut sink, &frames).await.unwrap();
            assert_eq!(sink.bytes, expected);
            assert_eq!(sink.flushes, 1);
            if vectored && maximum == usize::MAX {
                assert_eq!(sink.writes, 1);
            }
        }
    }
    let mut sink = output(usize::MAX, true);
    sink.fail_flush = true;
    assert_eq!(
        write_batch(&mut sink, &frames).await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(sink.bytes, expected);
    assert_eq!(sink.flushes, 1);

    let mut sink = output(usize::MAX, true);
    assert_eq!(
        write_batch(&mut sink, &[(0, &[][..]); BATCH_FRAMES + 1])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    write_batch(&mut sink, &[]).await.unwrap();
    assert!(sink.bytes.is_empty());
    assert_eq!(sink.writes, 0);
    assert_eq!(sink.flushes, 0);
}

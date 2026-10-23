//! Tokio-to-futures IO retaining scatter/gather writes. tokio-util's Compat
//! forwards scalar writes only, splitting RPC headers from their message bodies
//! and defeating the write queue's batching even on a vectored TCP socket.
#![forbid(unsafe_code)]

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(crate) fn defer(cx: &mut Context<'_>) {
    // yield_now's first poll places this waker on Tokio's deferred queue,
    // independently of the future's lifetime. TaskSet owns the resume state.
    let mut yielding = std::pin::pin!(tokio::task::yield_now());
    let _ = std::future::Future::poll(yielding.as_mut(), cx);
}

// Encryption transports consume each plaintext slice themselves. Coalescing a
// bounded batch avoids rustls's temporary slice vector and quiche's separate
// stream-send allocation for each RPC segment table. Plain sockets keep their
// scatter/gather path. The scratch buffer grows only to the largest small batch.
const ENCRYPTED_BATCH_BYTES: usize = 128 * 1024;
pub(crate) struct TokioIo<T> {
    inner: T,
    encrypted_batch: Option<Vec<u8>>,
}
impl<T> TokioIo<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self {
            inner,
            encrypted_batch: None,
        }
    }
    pub(crate) fn encrypted(inner: T) -> Self {
        Self {
            inner,
            encrypted_batch: Some(Vec::new()),
        }
    }
}

impl<T: AsyncRead + Unpin> futures::AsyncRead for TokioIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut buffer = ReadBuf::new(bytes);
        Pin::new(&mut self.get_mut().inner)
            .poll_read(cx, &mut buffer)
            .map_ok(|()| buffer.filled().len())
    }
}

impl<T: AsyncWrite + Unpin> futures::AsyncWrite for TokioIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, bytes)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Some(batch) = &mut this.encrypted_batch {
            if buffers.len() > 1 {
                let length = buffers.iter().try_fold(0usize, |sum, b| {
                    sum.checked_add(b.len())
                        .filter(|n| *n <= ENCRYPTED_BATCH_BYTES)
                });
                if let Some(length) = length {
                    batch.clear();
                    // Exact reserve keeps retained capacity within the bound.
                    batch.reserve_exact(length);
                    for buffer in buffers {
                        batch.extend_from_slice(buffer);
                    }
                    // AsyncWrite cannot retain this borrow after returning.
                    // Pending/partial writes consume only the reported prefix;
                    // the caller supplies its remaining slices on the next poll.
                    return Pin::new(&mut this.inner).poll_write(cx, batch);
                }
            }
        }
        Pin::new(&mut this.inner).poll_write_vectored(cx, buffers)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn deferred_wake_allows_other_ready_local_tasks_to_run() {
        use std::{cell::RefCell, rc::Rc};
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let events = Rc::new(RefCell::new(Vec::new()));
            let writer_events = events.clone();
            let mut yielded = false;
            let writer = tokio::task::spawn_local(std::future::poll_fn(move |cx| {
                if !yielded {
                    yielded = true;
                    defer(cx);
                    Poll::Pending
                } else {
                    writer_events.borrow_mut().push("write");
                    Poll::Ready(())
                }
            }));
            let caller_events = events.clone();
            tokio::task::spawn_local(async move {
                caller_events.borrow_mut().push("call");
            });
            writer.await.unwrap();
            assert_eq!(&*events.borrow(), &["call", "write"]);
        }));
    }

    #[derive(Default)]
    struct Writer {
        calls: usize,
        pending: bool,
        bytes: Vec<u8>,
        flushed: bool,
        closed: bool,
    }
    impl AsyncWrite for Writer {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            panic!("vectored writes must reach the transport intact")
        }
        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buffers: &[io::IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            self.calls += 1;
            if !self.pending {
                self.pending = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            self.pending = false;
            // Partial progress can cross a buffer boundary. The caller must
            // keep the suffix and retry it unchanged after backpressure.
            let bytes: Vec<u8> = buffers
                .iter()
                .flat_map(|b| b.iter().copied())
                .take(3)
                .collect();
            self.bytes.extend(&bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.flushed = true;
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.closed = true;
            Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
        }
    }

    #[test]
    fn framing_keeps_vectored_writes_through_pending_and_partial_progress() {
        let mut writer = TokioIo::new(Writer::default());
        let mut message = capnp::message::Builder::new_default();
        message.set_root("vectored framing").unwrap();
        let expected = capnp::serialize::write_message_to_words(&message);
        futures::executor::block_on(async {
            capnp_futures::serialize::write_messages(&mut writer, &[message])
                .await
                .unwrap();
            writer.flush().await.unwrap();
            assert_eq!(
                writer.close().await.unwrap_err().kind(),
                io::ErrorKind::BrokenPipe
            );
        });
        assert!(writer.inner.calls > 2);
        assert_eq!(writer.inner.bytes, expected);
        assert!(writer.inner.flushed && writer.inner.closed);
    }

    #[derive(Default)]
    struct EncryptedWriter {
        bytes: Vec<u8>,
        scalar: usize,
        vectored: usize,
        pending: bool,
        limit: usize,
        failed: bool,
    }
    impl EncryptedWriter {
        fn ready(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            if self.failed {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            if !self.pending {
                self.pending = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            self.pending = false;
            Poll::Ready(Ok(()))
        }
    }
    impl AsyncWrite for EncryptedWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.scalar += 1;
            std::task::ready!(self.ready(cx))?;
            let count = self.limit.min(bytes.len());
            self.bytes.extend_from_slice(&bytes[..count]);
            Poll::Ready(Ok(count))
        }
        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buffers: &[io::IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            self.vectored += 1;
            std::task::ready!(self.ready(cx))?;
            let mut count = 0;
            for buffer in buffers {
                let length = buffer.len().min(self.limit - count);
                self.bytes.extend_from_slice(&buffer[..length]);
                count += length;
            }
            Poll::Ready(Ok(count))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn encrypted_batches_preserve_framing_through_pending_and_partial_writes() {
        for limit in [3, usize::MAX] {
            let mut writer = TokioIo::encrypted(EncryptedWriter {
                limit,
                ..Default::default()
            });
            let mut first = capnp::message::Builder::new_default();
            first.set_root("first encrypted message").unwrap();
            let mut second = capnp::message::Builder::new_default();
            second.set_root("second encrypted message").unwrap();
            let expected: Vec<_> = [&first, &second]
                .into_iter()
                .flat_map(capnp::serialize::write_message_to_words)
                .collect();
            futures::executor::block_on(async {
                capnp_futures::serialize::write_messages(&mut writer, &[first, second])
                    .await
                    .unwrap();
                writer.flush().await.unwrap();
            });
            assert_eq!(writer.inner.bytes, expected);
            assert!(writer.inner.scalar > 0);
            if limit == usize::MAX {
                assert_eq!(writer.inner.vectored, 0);
            }
            assert!(writer.encrypted_batch.as_ref().unwrap().capacity() <= ENCRYPTED_BATCH_BYTES);
            writer.inner.failed = true;
            let error = futures::executor::block_on(
                writer.write_vectored(&[io::IoSlice::new(b"header"), io::IoSlice::new(b"payload")]),
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(writer.inner.bytes, expected);
        }
    }

    #[test]
    fn encrypted_large_batches_keep_scatter_gather_without_growing_scratch() {
        let mut writer = TokioIo::encrypted(EncryptedWriter {
            limit: usize::MAX,
            ..Default::default()
        });
        let payload = vec![7; ENCRYPTED_BATCH_BYTES];
        let buffers = [io::IoSlice::new(b"header"), io::IoSlice::new(&payload)];
        let written = futures::executor::block_on(writer.write_vectored(&buffers)).unwrap();
        assert_eq!(written, 6 + payload.len());
        assert_eq!(writer.inner.scalar, 0);
        assert!(writer.inner.vectored > 0);
        assert_eq!(writer.encrypted_batch.as_ref().unwrap().capacity(), 0);
        assert_eq!(&writer.inner.bytes[..6], b"header");
        assert_eq!(&writer.inner.bytes[6..], payload);
    }

    #[tokio::test]
    async fn reads_preserve_short_reads_and_eof() {
        let (mut writer, reader) = tokio::io::duplex(8);
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"abc")
            .await
            .unwrap();
        drop(writer);
        let mut reader = TokioIo::new(reader);
        let mut bytes = [0; 8];
        assert_eq!(reader.read(&mut bytes).await.unwrap(), 3);
        assert_eq!(&bytes[..3], b"abc");
        assert_eq!(reader.read(&mut bytes).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn scalar_writes_preserve_partial_progress_backpressure_and_disconnect() {
        let (writer, mut reader) = tokio::io::duplex(1);
        let mut writer = TokioIo::new(writer);
        assert_eq!(writer.write(b"ab").await.unwrap(), 1);
        let mut pending = Box::pin(writer.write(b"b"));
        assert!(futures::poll!(pending.as_mut()).is_pending());
        let mut byte = [0];
        tokio::io::AsyncReadExt::read_exact(&mut reader, &mut byte)
            .await
            .unwrap();
        assert_eq!(byte, *b"a");
        assert_eq!(pending.await.unwrap(), 1);
        tokio::io::AsyncReadExt::read_exact(&mut reader, &mut byte)
            .await
            .unwrap();
        assert_eq!(byte, *b"b");
        writer.flush().await.unwrap();
        drop(reader);
        assert_eq!(
            writer.write(b"c").await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        writer.close().await.unwrap();
    }
}

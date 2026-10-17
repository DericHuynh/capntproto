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

pub(crate) struct TokioIo<T>(pub(crate) T);

impl<T> TokioIo<T> {
    pub(crate) fn get_ref(&self) -> &T {
        &self.0
    }
}

impl<T: AsyncRead + Unpin> futures::AsyncRead for TokioIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut buffer = ReadBuf::new(bytes);
        Pin::new(&mut self.get_mut().0)
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
        Pin::new(&mut self.get_mut().0).poll_write(cx, bytes)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write_vectored(cx, buffers)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{AsyncReadExt, AsyncWriteExt};

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
        let mut writer = TokioIo(Writer::default());
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
        assert!(writer.0.calls > 2);
        assert_eq!(writer.0.bytes, expected);
        assert!(writer.0.flushed && writer.0.closed);
    }

    #[tokio::test]
    async fn reads_preserve_short_reads_and_eof() {
        let (mut writer, reader) = tokio::io::duplex(8);
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"abc")
            .await
            .unwrap();
        drop(writer);
        let mut reader = TokioIo(reader);
        let mut bytes = [0; 8];
        assert_eq!(reader.read(&mut bytes).await.unwrap(), 3);
        assert_eq!(&bytes[..3], b"abc");
        assert_eq!(reader.read(&mut bytes).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn scalar_writes_preserve_partial_progress_backpressure_and_disconnect() {
        let (writer, mut reader) = tokio::io::duplex(1);
        let mut writer = TokioIo(writer);
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

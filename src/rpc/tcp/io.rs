//! TCP readiness and direct-body progress for the deferred RPC scheduler.
//! Small and very large reads drain like KJ. Reads within the body-hint cap
//! use Tokio's short-read optimization. Exact body sizes bound Linux readiness.
#![forbid(unsafe_code)]

use std::{
    io::{self, IoSlice},
    pin::Pin,
    task::{ready, Context, Poll},
};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

/// Owned TCP input used by the RPC network. Small reads preserve readiness until
/// WouldBlock. Body reads within the readiness cap use Tokio's short-read
/// optimization; all partial large reads notify the scheduler. Incomplete bodies
/// supply a bounded readiness minimum without disabling TCP receive autotuning.
pub struct ReadHalf {
    inner: OwnedReadHalf,
    #[cfg(target_os = "linux")]
    minimum: Option<Minimum>,
}

#[cfg(target_os = "linux")]
struct Minimum {
    original: usize,
    current: usize,
    enabled: bool,
}

impl ReadHalf {
    pub(super) fn new(inner: OwnedReadHalf) -> Self {
        #[cfg(target_os = "linux")]
        let minimum = super::low_water::get(inner.as_ref())
            .ok()
            .map(|original| Minimum {
                original,
                current: original,
                enabled: true,
            });
        Self {
            inner,
            #[cfg(target_os = "linux")]
            minimum,
        }
    }

    pub(super) fn set_read_hint(&mut self, bytes: usize) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        if let Some(minimum) = &mut self.minimum {
            let wanted = if bytes >= 128 * 1024 {
                bytes.min(1024 * 1024)
            } else {
                1
            };
            if wanted == minimum.current || (!minimum.enabled && wanted != 1) {
                return Ok(());
            }
            match super::low_water::set(self.inner.as_ref(), wanted) {
                Ok(()) => minimum.current = wanted,
                Err(_) if minimum.current == 1 => minimum.enabled = false,
                Err(error) => return Err(error),
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = bytes;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for ReadHalf {
    fn drop(&mut self) {
        if let Some(minimum) = &self.minimum {
            if minimum.current != minimum.original {
                let _ = super::low_water::set(self.inner.as_ref(), minimum.original);
            }
        }
    }
}

impl futures::AsyncRead for ReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if bytes.len() > 64 * 1024 && bytes.len() <= 1024 * 1024 {
            // Within the receive-hint cap, a short read clears stale cached
            // readiness before the framer raises the remaining-body minimum.
            // Larger reads drain immediately; their remainder eventually uses
            // this bounded path. Publish partial progress before yielding so
            // queued output still waits for the next input turn.
            let this = self.get_mut();
            let mut buffer = tokio::io::ReadBuf::new(bytes);
            ready!(tokio::io::AsyncRead::poll_read(
                Pin::new(&mut this.inner),
                cx,
                &mut buffer,
            ))?;
            let count = buffer.filled().len();
            if count != 0 && buffer.remaining() != 0 {
                cx.waker().wake_by_ref();
            }
            return Poll::Ready(Ok(count));
        }
        loop {
            ready!(self.inner.as_ref().poll_read_ready(cx))?;
            match self.inner.try_read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => {
                    if let Ok(count) = result {
                        if bytes.len() > 64 * 1024 && count != 0 && count < bytes.len() {
                            cx.waker().wake_by_ref();
                        }
                    }
                    return Poll::Ready(result);
                }
            }
        }
    }
}

pub(super) struct WriteHalf(pub(super) OwnedWriteHalf);

impl futures::AsyncWrite for WriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            ready!(self.0.as_ref().poll_write_ready(cx))?;
            match self.0.try_write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => return Poll::Ready(result),
            }
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        loop {
            ready!(self.0.as_ref().poll_write_ready(cx))?;
            match self.0.try_write_vectored(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => return Poll::Ready(result),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.get_mut().0), cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{AsyncRead, AsyncReadExt, AsyncWriteExt};
    use std::io::{Read, Write};

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn exact_body_minimum_resets_for_small_frames_and_restores_on_drop() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        super::super::low_water::set(&socket, 7).unwrap();
        let (read, write) = socket.into_split();
        let mut read = ReadHalf::new(read);
        read.set_read_hint(128 * 1024).unwrap();
        assert_eq!(
            super::super::low_water::get(write.as_ref()).unwrap(),
            128 * 1024
        );
        peer.write_all(b"one").unwrap();
        let mut bytes = vec![0; 128 * 1024];
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(Pin::new(&mut read)
            .poll_read(&mut cx, &mut bytes)
            .is_pending());
        peer.write_all(&vec![0x61; bytes.len() - 3]).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read.read_exact(&mut bytes),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&bytes[..3], b"one");
        assert!(bytes[3..].iter().all(|b| *b == 0x61));
        read.set_read_hint(1).unwrap();
        peer.write_all(b"x").unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read.read_exact(&mut bytes[..1]),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(bytes[0], b'x');
        read.set_read_hint(usize::MAX).unwrap();
        assert_eq!(
            super::super::low_water::get(write.as_ref()).unwrap(),
            1024 * 1024
        );
        // A canceled body read can leave a raised minimum. Dropping the input
        // restores the caller's original value even while the writer lives.
        drop(read);
        assert_eq!(super::super::low_water::get(write.as_ref()).unwrap(), 7);
    }

    #[tokio::test]
    async fn partial_direct_body_read_notifies_progress_and_resumes_after_readiness() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        use std::task::{Wake, Waker};
        struct Notified(AtomicBool);
        impl Wake for Notified {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::Release);
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        peer.write_all(b"one").unwrap();
        socket.readable().await.unwrap();
        let (read, _write) = socket.into_split();
        let mut read = ReadHalf::new(read);
        let notified = Arc::new(Notified(AtomicBool::new(false)));
        let waker = Waker::from(notified.clone());
        let mut cx = Context::from_waker(&waker);
        let mut bytes = vec![0; 128 * 1024];
        assert!(matches!(
            Pin::new(&mut read).poll_read(&mut cx, &mut bytes),
            Poll::Ready(Ok(3))
        ));
        assert_eq!(&bytes[..3], b"one");
        assert!(notified.0.load(Ordering::Acquire));
        assert!(Pin::new(&mut read)
            .poll_read(&mut cx, &mut bytes)
            .is_pending());
        peer.write_all(b"two").unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read.read_exact(&mut bytes[..3]),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&bytes[..3], b"two");
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(read.read(&mut bytes).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn short_read_keeps_readiness_and_would_block_registers_the_next_wake() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        peer.write_all(b"one").unwrap();
        socket.readable().await.unwrap();
        let (read, write) = socket.into_split();
        let mut read = ReadHalf::new(read);
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        let mut bytes = [0; 64];
        assert!(matches!(
            Pin::new(&mut read).poll_read(&mut cx, &mut bytes),
            Poll::Ready(Ok(3))
        ));
        assert_eq!(&bytes[..3], b"one");
        // No reactor poll between the short read and the next socket write.
        peer.write_all(b"two").unwrap();
        assert!(matches!(
            Pin::new(&mut read).poll_read(&mut cx, &mut bytes),
            Poll::Ready(Ok(3))
        ));
        assert_eq!(&bytes[..3], b"two");
        assert!(Pin::new(&mut read)
            .poll_read(&mut cx, &mut bytes)
            .is_pending());
        peer.write_all(b"three").unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read.read_exact(&mut bytes[..5]),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&bytes[..5], b"three");
        let mut write = WriteHalf(write);
        write.write_all(b"reply").await.unwrap();
        write.close().await.unwrap();
        let mut reply = Vec::new();
        peer.read_to_end(&mut reply).unwrap();
        assert_eq!(reply, b"reply");
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(read.read(&mut bytes).await.unwrap(), 0);
    }
}

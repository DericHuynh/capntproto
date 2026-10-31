//! Preserve scatter/gather writes when passing Tokio IO to futures-based RPC.
use ::tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

pin_project_lite::pin_project! {
    /// Adapt Tokio IO for futures-based clients without splitting vectored writes.
    ///
    /// This wrapper adds no buffering or tasks. In particular, passing a TLS
    /// stream to RPC preserves batching across frame headers and message bodies.
    pub struct FuturesIo<T> {
        #[pin]
        inner: T,
    }
}
impl<T> FuturesIo<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
    pub fn get_ref(&self) -> &T {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
    pub fn into_inner(self) -> T {
        self.inner
    }
}
impl<T: AsyncRead> futures::AsyncRead for FuturesIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut buffer = ReadBuf::new(bytes);
        self.project()
            .inner
            .poll_read(cx, &mut buffer)
            .map_ok(|()| buffer.filled().len())
    }
}
impl<T: AsyncWrite> futures::AsyncWrite for FuturesIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write(cx, bytes)
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write_vectored(cx, buffers)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;

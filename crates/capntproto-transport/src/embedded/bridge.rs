//! Scoped adaptation of embedded async operations to a futures IO RPC driver.
use super::{Aes128GcmSha256, TlsConnection, TlsError};
use futures::{future::poll_fn, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt};
use std::{cell::RefCell, io, num::NonZeroUsize, pin::Pin, rc::Rc};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

/// Cloneable embedded IO over a futures stream. A mutable borrow exists only
/// while polling the underlying IO, never across an await or TLS processing.
pub struct FuturesIo<S>(Rc<RefCell<S>>);
impl<S> FuturesIo<S> {
    pub fn new(io: S) -> Self {
        Self(Rc::new(RefCell::new(io)))
    }
    pub fn into_inner(self) -> Result<S, Self> {
        Rc::try_unwrap(self.0)
            .map(RefCell::into_inner)
            .map_err(Self)
    }
}
impl<S> Clone for FuturesIo<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<S> embedded_io::ErrorType for FuturesIo<S> {
    type Error = io::Error;
}
impl<S: AsyncRead + Unpin> embedded_io_async::Read for FuturesIo<S> {
    async fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        poll_fn(|cx| Pin::new(&mut *self.0.borrow_mut()).poll_read(cx, bytes)).await
    }
}
impl<S: AsyncWrite + Unpin> embedded_io_async::Write for FuturesIo<S> {
    async fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        poll_fn(|cx| Pin::new(&mut *self.0.borrow_mut()).poll_write(cx, bytes)).await
    }
    async fn flush(&mut self) -> io::Result<()> {
        poll_fn(|cx| Pin::new(&mut *self.0.borrow_mut()).poll_flush(cx)).await
    }
}
fn error(error: TlsError) -> io::Error {
    io::Error::other(error)
}

/// Drive one authenticated embedded TLS client and an RPC/application scope.
/// The closure borrows plaintext IO, so it cannot return that IO past the scope.
/// Accepted plaintext is drained before TLS close_notify. Errors/cancellation
/// destroy the TLS connection instead of reusing a possibly partial record.
///
/// This hosted bridge allocates bounded duplex pipes (`capacity` per direction).
/// The `embedded-tls` backend itself remains allocator-free without this feature.
pub async fn with_futures_io<S, F, T, E>(
    mut connection: TlsConnection<'_, FuturesIo<S>, Aes128GcmSha256>,
    capacity: NonZeroUsize,
    application: F,
) -> Result<T, E>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: for<'a> AsyncFnOnce(&'a mut Compat<tokio::io::DuplexStream>) -> Result<T, E>,
    E: From<io::Error>,
{
    let result = {
        use embedded_io_async::{Read, Write};
        let (mut encrypted_read, mut encrypted_write) = connection.split();
        let (plaintext, pipe) = tokio::io::duplex(capacity.get());
        let mut plaintext = plaintext.compat();
        let (mut pipe_read, mut pipe_write) = pipe.compat().split();
        let outgoing = async move {
            let mut bytes = [0; 4096];
            loop {
                let n = pipe_read.read(&mut bytes).await?;
                if n == 0 {
                    break;
                }
                encrypted_write
                    .write_all(&bytes[..n])
                    .await
                    .map_err(error)?;
                encrypted_write.flush().await.map_err(error)?;
            }
            encrypted_write.flush().await.map_err(error)
        }
        .fuse();
        futures::pin_mut!(outgoing);
        let result = {
            let incoming = async move {
                let mut bytes = [0; 4096];
                loop {
                    let n = match encrypted_read.read(&mut bytes).await {
                        Ok(n) => n,
                        Err(TlsError::ConnectionClosed) => 0,
                        Err(err) => return Err(error(err)),
                    };
                    if n == 0 {
                        break;
                    }
                    pipe_write.write_all(&bytes[..n]).await?;
                }
                pipe_write.close().await
            }
            .fuse();
            let application = application(&mut plaintext).fuse();
            futures::pin_mut!(incoming, application);
            loop {
                futures::select_biased! {
                    sent = outgoing => sent.map_err(E::from)?,
                    received = incoming => received.map_err(E::from)?,
                    result = application => break result,
                }
            }
        };
        drop(plaintext);
        if !futures::future::FusedFuture::is_terminated(&outgoing) {
            outgoing.await.map_err(E::from)?;
        }
        result
    };
    let closed = connection
        .close()
        .await
        .map_err(|(_, err)| E::from(error(err)));
    match result {
        Ok(value) => {
            closed?;
            Ok(value)
        }
        Err(err) => Err(err),
    }
}

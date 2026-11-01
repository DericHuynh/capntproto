use crate::{measure, Args, Result};
use capntproto_transport::rustls;
use capntproto_transport::tokio::FuturesIo;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead as TokioRead, AsyncWrite as TokioWrite, ReadBuf};

struct Socket<S>(S);
impl<S: TokioRead + Unpin> TokioRead for Socket<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.0).poll_read(cx, buffer);
        if matches!(result, Poll::Ready(Ok(()))) {
            measure::read(buffer.filled().len() - before);
        }
        result
    }
}
impl<S: TokioWrite + Unpin> TokioWrite for Socket<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_write(cx, bytes);
        if let Poll::Ready(Ok(n)) = &result {
            measure::write(*n);
        }
        result
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_write_vectored(cx, bytes);
        if let Poll::Ready(Ok(n)) = &result {
            measure::write(*n);
        }
        result
    }
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
pub fn run(args: &Args) -> Result<measure::Sample> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(tokio::task::LocalSet::new().run_until(tls_tokio(args)))
}
async fn tls_tokio(args: &Args) -> Result<measure::Sample> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let config = args.server_config()?;
    let client = args.client_config()?;
    let serving = async {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let tls = rustls::TlsStream::accept(Socket(socket), config).await?;
        crate::serve(FuturesIo::new(tls), args).await
    };
    let calling = async {
        let socket = tokio::net::TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        let tls =
            rustls::TlsStream::connect(Socket(socket), client, "localhost".try_into()?).await?;
        crate::call(FuturesIo::new(tls), args).await
    };
    let (server, client) = futures::join!(serving, calling);
    server?;
    client
}

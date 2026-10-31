//! Native async WASI Preview 3 component with browser-owned WebTransport IO.
use futures::{AsyncRead, AsyncWrite, Future, FutureExt};
use std::{
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};
// The generated canonical ABI expands values into machine arguments.
#[allow(clippy::too_many_arguments)]
mod bindings {
    wit_bindgen::generate!({path:"wit",world:"frontend",generate_all});
}
use bindings::{capntproto, wasi, Guest};
use capntproto::browser::transport::{self, Connection};
type Operation<T> = Pin<Box<dyn Future<Output = Result<T, String>>>>;
struct Stream {
    connection: Rc<Connection>,
    read: Option<Operation<Vec<u8>>>,
    write: Option<Operation<()>>,
    close: Option<Operation<()>>,
    buffered: Vec<u8>,
    offset: usize,
    closed: bool,
}
impl Stream {
    fn new(connection: Connection) -> Self {
        Self {
            connection: Rc::new(connection),
            read: None,
            write: None,
            close: None,
            buffered: Vec::new(),
            offset: 0,
            closed: false,
        }
    }
}
impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if output.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.offset == self.buffered.len() {
            if self.read.is_none() {
                let connection = self.connection.clone();
                self.read = Some(Box::pin(async move { connection.read(16 * 1024).await }));
            }
            let bytes = std::task::ready!(self.read.as_mut().unwrap().as_mut().poll(cx));
            self.read = None;
            self.buffered = bytes.map_err(io::Error::other)?;
            self.offset = 0;
        }
        let length = output.len().min(self.buffered.len() - self.offset);
        output[..length].copy_from_slice(&self.buffered[self.offset..self.offset + length]);
        self.offset += length;
        Poll::Ready(Ok(length))
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.write.is_none() {
            let connection = self.connection.clone();
            let bytes = bytes.to_vec();
            self.write = Some(Box::pin(async move { connection.write(bytes).await }));
        }
        let result = std::task::ready!(self.write.as_mut().unwrap().as_mut().poll(cx));
        self.write = None;
        Poll::Ready(result.map(|()| bytes.len()).map_err(io::Error::other))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        if self.close.is_none() {
            let connection = self.connection.clone();
            self.close = Some(Box::pin(async move { connection.close().await }));
        }
        let result = std::task::ready!(self.close.as_mut().unwrap().as_mut().poll(cx));
        self.close = None;
        self.closed = result.is_ok();
        Poll::Ready(result.map_err(io::Error::other))
    }
}
struct Frontend;
bindings::export!(Frontend with_types_in bindings);
impl Guest for Frontend {
    async fn greet(
        url: String,
        pin: Vec<u8>,
        prefix: String,
        value: String,
    ) -> Result<String, String> {
        if !pin.is_empty() && pin.len() != 32 {
            return Err("certificate pin must contain 32 SHA-256 bytes".into());
        }
        let application = async {
            use futures::AsyncReadExt;
            let connection = transport::connect(url, pin).await?;
            let (read, write) = Stream::new(connection).split();
            let epoch = wasi::clocks::monotonic_clock::now();
            let network = service::rpc::twoparty::VatNetwork::new_with_clock(
                read,
                write,
                service::rpc::rpc_twoparty_capnp::Side::Client,
                Default::default(),
                move || {
                    std::time::Duration::from_nanos(
                        wasi::clocks::monotonic_clock::now().saturating_sub(epoch),
                    )
                },
            );
            let driver = service::rpc::twoparty::TwoPartyClient::from_network_with_driver(
                network,
                None,
                service::protocol().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            service::greeting_scoped(driver, &prefix, &value)
                .await
                .map_err(|e| e.to_string())
        }
        .fuse();
        let timeout = wasi::clocks::monotonic_clock::wait_for(15_000_000_000).fuse();
        futures::pin_mut!(application, timeout);
        futures::select! {result=application=>result,()=timeout=>Err("RPC deadline exceeded".into())}
    }
}

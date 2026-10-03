//! Standard QUIC v1/v2 with quiche, TLS 1.3 and optional mandatory mTLS.
//!
//! One client-initiated stream carries ordinary Cap'n Proto RPC bytes. Configure
//! CA trust explicitly. No resumption or application 0-RTT is enabled. All endpoint
//! and session tasks run within a Tokio LocalSet.
mod config;
mod driver;
mod endpoint;
mod retry;
use super::tls::{self, AcceptOptions};
pub use super::QuicVersion as Version;
use capnp::{capability::Client, message::ReaderOptions};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
pub use config::{
    client_config, client_config_for_version, server_config, ClientConfig, ServerConfig,
};
pub use endpoint::{Endpoint, Incoming};
use futures::{stream::FuturesUnordered, StreamExt};
use rustls::pki_types::CertificateDer;
use std::{
    cell::RefCell,
    io,
    net::SocketAddr,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Authenticated RPC IO. Drop cancels the driver even when diagnostics are retained.
pub struct Stream {
    io: DuplexStream,
    state: Rc<RefCell<driver::State>>,
    close: Option<tokio::sync::oneshot::Sender<()>>,
    certificates: Vec<CertificateDer<'static>>,
    version: u32,
    _endpoint: Rc<endpoint::Core>,
}
impl Stream {
    pub fn peer_certificates(&self) -> &[CertificateDer<'static>] {
        &self.certificates
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn is_closed(&self) -> bool {
        self.state.borrow().closed
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(close) = self.close.take() {
            let _ = close.send(());
        }
    }
}
impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buffer)
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(error) = self.state.borrow().error() {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.io).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    /// Wait for transport acknowledgement of every sent byte and FIN. The
    /// receive half stays open; this does not acknowledge application execution.
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.io).poll_shutdown(cx) {
            Poll::Ready(Ok(())) => (),
            other => return other,
        }
        let mut state = self.state.borrow_mut();
        if state.acknowledged {
            return Poll::Ready(Ok(()));
        }
        if let Some(error) = state.error() {
            return Poll::Ready(Err(error));
        }
        state.waiter = Some(cx.waker().clone());
        Poll::Pending
    }
}
/// Dial and authenticate, checking the certificate chain and DNS name or IP SAN.
pub async fn connect(
    endpoint: &Endpoint,
    address: SocketAddr,
    server_name: &str,
    timeout: Duration,
) -> io::Result<Stream> {
    rustls::pki_types::ServerName::try_from(server_name).map_err(tls::invalid_config)?;
    let ip = server_name.parse::<std::net::IpAddr>().ok();
    let id = crate::transport::cid();
    let mut conn = {
        let mut config = endpoint.0.client.borrow_mut();
        let config = config
            .as_mut()
            .ok_or_else(|| tls::invalid_config("missing QUIC client configuration"))?;
        quiche::connect(
            ip.is_none().then_some(server_name),
            &quiche::ConnectionId::from_ref(&id),
            endpoint.local_addr()?,
            address,
            &mut config.0,
        )
        .map_err(io::Error::other)?
    };
    if let Some(ip) = ip {
        conn.set_verify_peer_ip(ip).map_err(tls::invalid_config)?;
    }
    let route = endpoint::register(&endpoint.0, vec![id.to_vec()])?;
    driver::spawn(endpoint.0.clone(), route, Box::new(conn), timeout).await
}
/// Complete authentication and await the first RPC stream under one deadline.
pub async fn accept(incoming: Incoming, timeout: Duration) -> io::Result<Stream> {
    let core = incoming
        .core
        .upgrade()
        .ok_or_else(|| io::Error::other("QUIC endpoint closed"))?;
    driver::spawn(core, incoming.route, incoming.connection, timeout).await
}
pub type Network = twoparty::VatNetwork<Compat<tokio::io::ReadHalf<Stream>>>;
pub fn network(stream: Stream, side: Side, options: ReaderOptions) -> Network {
    let (read, write) = tokio::io::split(stream);
    twoparty::VatNetwork::new(read.compat(), write.compat_write(), side, options)
}
pub fn client(
    stream: Stream,
    bootstrap: Option<Client>,
    side: Side,
    options: ReaderOptions,
) -> twoparty::TwoPartyClient<'static> {
    twoparty::TwoPartyClient::from_network(network(stream, side, options), bootstrap)
}
/// Bounded simultaneous handshakes. Canceling stops admission and pending setup;
/// established streams remain owned by the RPC server driver.
pub async fn listen(
    server: &twoparty::TwoPartyServer,
    endpoint: &Endpoint,
    reader_options: ReaderOptions,
    options: AcceptOptions,
    mut on_rejected: impl FnMut(SocketAddr, io::Error),
) -> capnp::Result<()> {
    struct Stop<'a>(&'a Endpoint);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.stop_accepting();
        }
    }
    endpoint.0.admitting.set(true);
    let _stop = Stop(endpoint);
    let mut pending = FuturesUnordered::new();
    loop {
        tokio::select! {
            incoming = endpoint.accept(), if pending.len() < options.max_pending.get() => {
                let Some(incoming) = incoming else { return Ok(()) };
                let peer = incoming.remote_address();
                pending.push(async move { (peer, accept(incoming, options.timeout).await) });
            }
            Some((peer, result)) = pending.next(), if !pending.is_empty() => {
                match result { Ok(stream) => server.accept_network(network(stream, Side::Server, reader_options))?, Err(error) => on_rejected(peer, error) }
            }
        }
    }
}

//! Upstream QUIC v1 with quiche, TLS 1.3 and optional mandatory mTLS.
//!
//! One client-initiated stream carries ordinary Cap'n Proto RPC bytes. Configure
//! CA trust explicitly. No resumption or application 0-RTT is enabled. All endpoint
//! and session tasks run within a Tokio LocalSet.
mod config;
mod driver;
mod endpoint;
mod retry;
use super::io::TokioIo;
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
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

/// Authenticated RPC IO. Drop cancels an open writer immediately. After successful
/// write shutdown, drop cancels reading but lets Quiche finish queued transmission
/// until the stream is collected or the connection closes/times out.
pub struct Stream {
    state: Rc<driver::State>,
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
        self.state.closed.get()
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
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let state = &self.state;
        if buffer.remaining() == 0 || state.read_closed.get() {
            return Poll::Ready(Ok(()));
        }
        // Tokio's BufMut implementation exposes the destination's spare
        // capacity directly; quiche initializes and advances only bytes received.
        let result = state.conn.borrow_mut().stream_recv_buf(0, &mut *buffer);
        match result {
            Ok((_n, fin)) => {
                state.read_closed.set(fin);
                // Reading returns flow-control credit to the packet engine.
                state.request_flush();
                Poll::Ready(Ok(()))
            }
            Err(quiche::Error::Done | quiche::Error::InvalidStreamState(_)) => {
                if state.closed.get() {
                    return Poll::Ready(Ok(()));
                }
                state.reader.borrow_mut().replace(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let state = &self.state;
        if state.write_closed.get() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "QUIC write half closed",
            )));
        }
        if let Some(error) = state.error() {
            return Poll::Ready(Err(error));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        // Quiche applies connection/stream flow control and congestion capacity;
        // no second application byte queue or relay task is needed.
        let result = state.conn.borrow_mut().stream_send(0, bytes, false);
        match result {
            Ok(n) if n != 0 => {
                state.wrote();
                Poll::Ready(Ok(n))
            }
            Ok(_) | Err(quiche::Error::Done) => {
                state.request_flush();
                state.writer.borrow_mut().replace(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // The common physical frame writer ends its batch here. Keep the
        // packet driver's bounded coalescing fence: RPC batch completion alone
        // cannot certify that an endpoint receive burst has finished.
        self.state.finish_output_turn();
        // Bytes are owned by Quiche; this is not a peer acknowledgement.
        match self.state.error() {
            Some(error) => Poll::Ready(Err(error)),
            None => Poll::Ready(Ok(())),
        }
    }
    /// Queue FIN in the upstream transport. The receive half stays open.
    /// This is not a peer-delivery or application receipt.
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let state = &self.state;
        if state.write_closed.get() {
            return Poll::Ready(Ok(()));
        }
        if let Some(error) = state.error() {
            return Poll::Ready(Err(error));
        }
        let result = state.conn.borrow_mut().stream_send(0, &[], true);
        state.request_flush();
        match result {
            Ok(_) => {
                state.write_closed.set(true);
                Poll::Ready(Ok(()))
            }
            Err(quiche::Error::Done) => {
                state.writer.borrow_mut().replace(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
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
    let conn = {
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
    let route = endpoint::register(&endpoint.0, vec![id.to_vec()])?;
    let stream = driver::spawn(endpoint.0.clone(), route, Box::new(conn), timeout).await?;
    if let Some(ip) = ip {
        // Quiche verified the chain and server usage during TLS. IP literals
        // bypass DNS/SNI; verify the IP SAN before exposing any application IO.
        let certificate = stream
            .certificates
            .first()
            .ok_or_else(|| tls::invalid_config("missing server certificate"))?;
        let certificate = boring::x509::X509::from_der(certificate).map_err(tls::invalid_config)?;
        if !certificate
            .check_ip_asc(&ip.to_string())
            .map_err(tls::invalid_config)?
        {
            return Err(tls::invalid_config(
                "server certificate does not match the IP address",
            ));
        }
    }
    Ok(stream)
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
    let mut network =
        twoparty::VatNetwork::new(read.compat(), TokioIo::encrypted(write), side, options);
    network.set_task_yield(super::io::defer);
    network
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

//! TLS 1.3 authentication and encryption layered over TCP.
//!
//! Configuration helpers validate chains and server names with rustls. Passing
//! client roots to [`server_config`](crate::rpc::tcp::tls::server_config) makes
//! a valid client certificate mandatory.
//! Inspect the authenticated peer's certificates before choosing a bootstrap
//! when different identities should receive different capabilities.

use super::super::io::{deadline, TokioIo};
use super::super::{security::rustls, AcceptOptions, ALPN};
use capnp::{capability::Client, message::ReaderOptions};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
pub use capntproto_transport::rustls::TlsStream;
use futures::{stream::FuturesUnordered, StreamExt};
use rustls::pki_types::ServerName;
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

// One shared implementation/configuration for generic IO and TCP.
pub use capntproto_transport::rustls::{client_config, server_config};
pub use capntproto_transport::{SystemClock, TlsClock};

fn check_alpn(stream: TlsStream<TcpStream>) -> io::Result<TlsStream<TcpStream>> {
    if stream.get_ref().1.alpn_protocol() != Some(ALPN) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "RPC ALPN was not negotiated",
        ));
    }
    Ok(stream)
}

/// Dial TCP and complete TLS authentication within one deadline, including DNS.
/// No RPC driver or bootstrap is installed until this succeeds.
pub async fn connect(
    address: impl ToSocketAddrs,
    server_name: ServerName<'static>,
    config: Arc<rustls::ClientConfig>,
    timeout: Duration,
) -> io::Result<TlsStream<TcpStream>> {
    deadline(timeout, async move {
        let socket = TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        let stream = TlsStream::connect(socket, config, server_name).await?;
        check_alpn(stream)
    })
    .await
}

/// Authenticate one accepted TCP socket. Inspect `get_ref().1.peer_certificates()`
/// before assigning per-identity capabilities. Cancellation drops the socket.
pub async fn accept(
    socket: TcpStream,
    config: Arc<rustls::ServerConfig>,
    timeout: Duration,
) -> io::Result<TlsStream<TcpStream>> {
    deadline(timeout, async move {
        socket.set_nodelay(true)?;
        let stream = TlsStream::accept(socket, config).await?;
        check_alpn(stream)
    })
    .await
}

pub type Network = twoparty::VatNetwork<Compat<tokio::io::ReadHalf<TlsStream<TcpStream>>>>;

/// Adapt an authenticated stream, using its TCP send-buffer size at admission
/// as the streaming flow-control window (64 KiB fallback if the query fails).
pub fn network(stream: TlsStream<TcpStream>, side: Side, options: ReaderOptions) -> Network {
    let window = socket2::SockRef::from(stream.get_ref().0)
        .send_buffer_size()
        .ok();
    let (read, write) = tokio::io::split(stream);
    let mut network = twoparty::VatNetwork::new_with_send_buffer(
        read.compat(),
        TokioIo::new(write),
        side,
        options,
        move |_| window,
    );
    network.set_task_yield(super::super::io::defer);
    network
}

/// Build the RPC driver after authentication. Poll it within a Tokio LocalSet.
pub fn driver(
    stream: TlsStream<TcpStream>,
    bootstrap: Option<Client>,
    side: Side,
    options: ReaderOptions,
) -> twoparty::TwoPartyClient<'static> {
    twoparty::TwoPartyClient::from_network(network(stream, side, options), bootstrap)
}

/// Concurrent, bounded handshakes for a server sharing one bootstrap.
/// Invalid peers are reported and isolated. Canceling this future cancels pending
/// handshakes; established RPC connections remain owned by the ServerDriver.
pub async fn listen(
    server: &twoparty::TwoPartyServer,
    listener: &TcpListener,
    config: Arc<rustls::ServerConfig>,
    reader_options: ReaderOptions,
    options: AcceptOptions,
    mut on_rejected: impl FnMut(SocketAddr, io::Error),
) -> capnp::Result<()> {
    let mut pending = FuturesUnordered::new();
    loop {
        tokio::select! {
            accepted = listener.accept(), if pending.len() < options.max_pending.get() => {
                let (socket, peer) = accepted?;
                let config = config.clone();
                pending.push(async move { (peer, accept(socket, config, options.timeout).await) });
            }
            Some((peer, result)) = pending.next(), if !pending.is_empty() => {
                match result {
                    Ok(stream) => server.accept_network(network(stream, Side::Server, reader_options))?,
                    Err(error) => on_rejected(peer, error),
                }
            }
        }
    }
}

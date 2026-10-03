//! TLS 1.3 over TCP and shared certificate configuration for standard QUIC.
//!
//! Configuration helpers validate chains and server names with rustls. Passing
//! client roots to [`server_config`] makes a valid client certificate mandatory.
//! Inspect the authenticated peer's certificates before choosing a bootstrap
//! when different identities should receive different capabilities.

use capnp::{capability::Client, message::ReaderOptions};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
use futures::{stream::FuturesUnordered, StreamExt};
pub use rustls;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use std::{io, net::SocketAddr, num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
pub use tokio_rustls::TlsStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Private application protocol for ordinary Cap'n Proto two-party framing.
/// This is not HTTP/3 or an upstream-assigned Cap'n Proto ALPN identifier.
pub const ALPN: &[u8] = b"capntproto-rpc/1";

/// A leaf-first certificate chain and its matching private key, in DER form.
pub struct Identity {
    pub certificates: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
}

/// Bounds concurrent unauthenticated sessions and their setup time.
#[derive(Clone, Copy, Debug)]
pub struct AcceptOptions {
    /// Includes the handshake and, for QUIC, arrival of the first RPC stream.
    pub timeout: Duration,
    pub max_pending: NonZeroUsize,
}

impl Default for AcceptOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            max_pending: NonZeroUsize::new(64).unwrap(),
        }
    }
}

/// Trust only the supplied roots; optionally present a client identity for mTLS.
/// TLS 1.3 and this crate's RPC ALPN are selected; early data is disabled.
pub fn client_config(
    roots: rustls::RootCertStore,
    identity: Option<Identity>,
) -> Result<Arc<rustls::ClientConfig>, rustls::Error> {
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .with_root_certificates(roots);
    let mut config = match identity {
        Some(identity) => {
            builder.with_client_auth_cert(identity.certificates, identity.private_key)?
        }
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.enable_early_data = false;
    Ok(Arc::new(config))
}

/// `Some(client_roots)` requires a trusted client certificate on every session.
/// `None` enables server-authenticated TLS. Neither mode bypasses verification.
pub fn server_config(
    identity: Identity,
    client_roots: Option<rustls::RootCertStore>,
) -> io::Result<Arc<rustls::ServerConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(invalid_config)?;
    let builder = match client_roots {
        Some(roots) => builder.with_client_cert_verifier(
            rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .map_err(invalid_config)?,
        ),
        None => builder.with_no_client_auth(),
    };
    let mut config = builder
        .with_single_cert(identity.certificates, identity.private_key)
        .map_err(invalid_config)?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.max_early_data_size = 0;
    Ok(Arc::new(config))
}

pub(super) fn invalid_config(
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

pub(super) async fn deadline<T>(
    timeout: Duration,
    future: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "RPC connection setup timed out"))?
}

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
        let stream = tokio_rustls::TlsConnector::from(config)
            .connect(server_name, socket)
            .await?;
        check_alpn(stream.into())
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
        let stream = tokio_rustls::TlsAcceptor::from(config)
            .accept(socket)
            .await?;
        check_alpn(stream.into())
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
    twoparty::VatNetwork::new_with_send_buffer(
        read.compat(),
        write.compat_write(),
        side,
        options,
        move |_| window,
    )
}

/// Build the RPC driver after authentication. Poll it within a Tokio LocalSet.
pub fn client(
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

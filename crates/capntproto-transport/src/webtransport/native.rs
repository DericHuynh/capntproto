use crate::TlsClock;
use futures::{AsyncRead, AsyncWrite};
pub use rustls_quic as tls;
use std::{
    any::Any,
    fmt,
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use wtransport::{endpoint::endpoint_side, Endpoint, VarInt};

struct Clock(Arc<dyn TlsClock + Send + Sync>);
impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallerTlsClock")
    }
}
impl tls::time_provider::TimeProvider for Clock {
    fn current_time(&self) -> Option<tls::pki_types::UnixTime> {
        self.0.now().map(|now| {
            tls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(now))
        })
    }
}
fn error(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(error)
}

/// Client endpoint using explicit roots and certificate-verification time.
#[derive(Clone)]
pub struct Client(Arc<Endpoint<endpoint_side::Client>>);
impl Client {
    pub fn new(
        roots: tls::RootCertStore,
        clock: Arc<dyn TlsClock + Send + Sync>,
    ) -> io::Result<Self> {
        let mut config = tls::ClientConfig::builder_with_details(
            Arc::new(tls::crypto::ring::default_provider()),
            Arc::new(Clock(clock)),
        )
        .with_protocol_versions(&[&tls::version::TLS13])
        .map_err(error)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        config.alpn_protocols = vec![wtransport::tls::WEBTRANSPORT_ALPN.to_vec()];
        config.enable_early_data = false;
        let config = wtransport::ClientConfig::builder()
            .with_bind_default()
            .with_custom_tls(config)
            .build();
        Ok(Self(Arc::new(Endpoint::client(config)?)))
    }
    pub async fn connect(&self, url: &str) -> io::Result<Session> {
        let url = super::checked_url(url)?;
        let connection = self.0.connect(url.as_str()).await.map_err(error)?;
        Ok(Session(Arc::new(Owner {
            connection,
            _endpoint: self.0.clone(),
        })))
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

/// Server endpoint. A session's path and origin must be authorized by the caller
/// before accepting it and exposing application capabilities.
#[derive(Clone)]
pub struct Server(Arc<Endpoint<endpoint_side::Server>>);
impl Server {
    pub fn bind(
        address: SocketAddr,
        certificates: Vec<tls::pki_types::CertificateDer<'static>>,
        key: tls::pki_types::PrivateKeyDer<'static>,
        clock: Arc<dyn TlsClock + Send + Sync>,
    ) -> io::Result<Self> {
        let mut config = tls::ServerConfig::builder_with_details(
            Arc::new(tls::crypto::ring::default_provider()),
            Arc::new(Clock(clock)),
        )
        .with_protocol_versions(&[&tls::version::TLS13])
        .map_err(error)?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(error)?;
        config.alpn_protocols = vec![wtransport::tls::WEBTRANSPORT_ALPN.to_vec()];
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        let config = wtransport::ServerConfig::builder()
            .with_bind_address(address)
            .with_custom_tls(config)
            .build();
        Ok(Self(Arc::new(Endpoint::server(config)?)))
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
    /// Admit the initial QUIC packet separately from completing TLS/HTTP setup.
    /// Poll several Incoming futures with bounded deadlines for concurrent setup.
    pub async fn incoming(&self) -> Incoming {
        Incoming {
            handshake: self.0.accept().await,
            endpoint: self.0.clone(),
        }
    }
    pub async fn accept(&self) -> io::Result<Request> {
        self.incoming().await.accept().await
    }
}

pub struct Incoming {
    handshake: wtransport::endpoint::IncomingSession,
    endpoint: Arc<Endpoint<endpoint_side::Server>>,
}
impl Incoming {
    pub async fn accept(self) -> io::Result<Request> {
        Ok(Request {
            request: self.handshake.await.map_err(error)?,
            endpoint: self.endpoint,
        })
    }
}

pub struct Request {
    request: wtransport::endpoint::SessionRequest,
    endpoint: Arc<Endpoint<endpoint_side::Server>>,
}
impl Request {
    pub fn path(&self) -> &str {
        self.request.path()
    }
    pub fn authority(&self) -> &str {
        self.request.authority()
    }
    pub fn origin(&self) -> Option<&str> {
        self.request.origin()
    }
    pub async fn accept(self) -> io::Result<Session> {
        Ok(Session(Arc::new(Owner {
            connection: self.request.accept().await.map_err(error)?,
            _endpoint: self.endpoint,
        })))
    }
    pub async fn reject(self) {
        self.request.forbidden().await;
    }
}

struct Owner {
    connection: wtransport::Connection,
    _endpoint: Arc<dyn Any + Send + Sync>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.connection
            .close(VarInt::from_u32(0), b"RPC session dropped");
    }
}
#[derive(Clone)]
pub struct Session(Arc<Owner>);
impl Session {
    pub async fn open_bi(&mut self) -> io::Result<Stream> {
        let (send, recv) = self
            .0
            .connection
            .open_bi()
            .await
            .map_err(error)?
            .await
            .map_err(error)?;
        Ok(Stream {
            recv: recv.compat(),
            send: send.compat_write(),
            _session: self.0.clone(),
            closing: None,
            close_done: false,
            close_failed: false,
        })
    }
    pub async fn accept_bi(&mut self) -> io::Result<Stream> {
        let (send, recv) = self.0.connection.accept_bi().await.map_err(error)?;
        Ok(Stream {
            recv: recv.compat(),
            send: send.compat_write(),
            _session: self.0.clone(),
            closing: None,
            close_done: false,
            close_failed: false,
        })
    }
    pub fn close(&self, code: u32, reason: &str) -> io::Result<()> {
        self.0
            .connection
            .close(VarInt::from_u32(code), reason.as_bytes());
        Ok(())
    }
}

/// Backpressured futures IO over one WebTransport bidirectional stream.
pub struct Stream {
    recv: Compat<wtransport::RecvStream>,
    send: Compat<wtransport::SendStream>,
    _session: Arc<Owner>,
    closing: Option<Pin<Box<dyn Future<Output = io::Result<()>> + Send>>>,
    close_done: bool,
    close_failed: bool,
}
impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.recv).poll_read(cx, bytes)
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.close_done || self.closing.is_some() || self.close_failed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WebTransport write half closed",
            )));
        }
        Pin::new(&mut self.send).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.close_done {
            return Poll::Ready(Ok(()));
        }
        if self.close_failed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WebTransport close failed",
            )));
        }
        if self.closing.is_none() {
            if let Err(err) = self.send.get_mut().quic_stream_mut().finish() {
                self.close_failed = true;
                return Poll::Ready(Err(error(err)));
            }
            // Quinn's plain AsyncWrite::shutdown only queues FIN. Retain its
            // acknowledgement before the last session owner may close QUIC.
            let stopped = self.send.get_ref().quic_stream().stopped();
            self.closing = Some(Box::pin(async move {
                match stopped.await.map_err(error)? {
                    None => Ok(()),
                    Some(_) => Err(io::Error::new(
                        io::ErrorKind::ConnectionReset,
                        "WebTransport peer stopped the stream",
                    )),
                }
            }));
        }
        let result = std::task::ready!(self.closing.as_mut().unwrap().as_mut().poll(cx));
        self.closing = None;
        self.close_done = result.is_ok();
        self.close_failed = result.is_err();
        Poll::Ready(result)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if !self.close_done {
            let _ = self.send.get_mut().reset(VarInt::from_u32(0));
        }
    }
}

/// Bounded concurrent TLS/HTTP3/first-stream setup. Authorize path/origin before
/// exposing bootstrap capabilities. Canceling stops setup and admission; the
/// caller continues polling its TwoPartyServer driver for established sessions.
pub async fn listen(
    rpc: &capnp_rpc::twoparty::TwoPartyServer,
    endpoint: &Server,
    reader: capnp::message::ReaderOptions,
    timeout: std::time::Duration,
    max_pending: std::num::NonZeroUsize,
    authorize: impl Fn(&Request) -> bool,
    mut on_rejected: impl FnMut(io::Error),
) -> capnp::Result<()> {
    use futures::{stream::FuturesUnordered, StreamExt};
    let mut pending = FuturesUnordered::new();
    loop {
        tokio::select! {
            incoming = endpoint.incoming(), if pending.len() < max_pending.get() => {
                let authorize = &authorize;
                pending.push(async move {
                    tokio::time::timeout(timeout, async move {
                        let request = incoming.accept().await?;
                        if !authorize(&request) {
                            request.reject().await;
                            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "WebTransport request rejected"));
                        }
                        request.accept().await?.accept_bi().await
                    }).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "WebTransport setup timed out"))?
                });
            }
            Some(result) = pending.next(), if !pending.is_empty() => match result {
                Ok(stream) => rpc.accept_network(crate::rpc::network(stream, capnp_rpc::rpc_twoparty_capnp::Side::Server, reader))?,
                Err(err) => on_rejected(err),
            }
        }
    }
}

//! Monoio completion IO. No Tokio runtime, background task, buffer relay, or
//! per-operation boxed future is introduced by this adapter.
use crate::{
    io::{Close, Completion, Read, Timer, Write},
    rustls::OwnedBuffer,
};
use ::monoio::{
    buf::{IoBuf, IoBufMut},
    io::{AsyncReadRent, AsyncWriteRent, Splitable},
};
use std::{
    io,
    net::SocketAddr,
    time::{Duration, Instant},
};

pub use ::monoio as runtime;
pub struct Reader<R>(pub R);
pub struct Writer<W>(pub W);
impl<R: AsyncReadRent> Read for Reader<R> {
    type Buffer = OwnedBuffer;
    type Error = io::Error;
    async fn read(&mut self, buffer: OwnedBuffer) -> Completion<OwnedBuffer, io::Error> {
        let (bytes, range) = buffer.into_parts();
        // Monoio truncates a Vec to the received prefix. Fixed initialized TLS
        // storage must survive a short receive without clearing 82 KiB again.
        // Exact-sized Vec/Box conversions preserve this allocation.
        let (result, bytes) = self
            .0
            .read(bytes.into_boxed_slice().slice_mut(range.clone()))
            .await;
        (
            result,
            OwnedBuffer::new(bytes.into_inner().into_vec(), range).unwrap(),
        )
    }
}
impl<W: AsyncWriteRent> Write for Writer<W> {
    type Buffer = OwnedBuffer;
    type Error = io::Error;
    async fn write(&mut self, buffer: OwnedBuffer) -> Completion<OwnedBuffer, io::Error> {
        let (bytes, range) = buffer.into_parts();
        let (result, bytes) = self.0.write(bytes.slice(range.clone())).await;
        (result, OwnedBuffer::new(bytes.into_inner(), range).unwrap())
    }
    async fn flush(&mut self) -> io::Result<()> {
        self.0.flush().await
    }
}
impl<W: AsyncWriteRent> Close for Writer<W> {
    async fn close(&mut self) -> io::Result<()> {
        self.0.shutdown().await
    }
}

/// An explicit epoch for monotonic deadlines. Certificate time still comes from
/// the application's separate TlsClock. Requires a timer-enabled monoio runtime.
pub struct Clock(Instant);
impl Default for Clock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Timer for Clock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    async fn wait_until(&self, deadline: Duration) {
        match self.0.checked_add(deadline) {
            Some(at) => ::monoio::time::sleep_until(at.into()).await,
            None => core::future::pending().await,
        }
    }
}

pub mod tls {
    use super::*;
    use crate::rustls;
    use pki_types::ServerName;
    pub use rustls::{
        client_config, pki_types, roots_from_pem_file, server_config, ClientConfig, Identity,
        RootCertStore, ServerConfig, Stream,
    };
    use std::sync::Arc;

    pub fn client(
        socket: ::monoio::net::TcpStream,
        config: Arc<ClientConfig>,
        name: ServerName<'static>,
    ) -> io::Result<(Stream, impl crate::driven::Driver<Error = io::Error>)> {
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        rustls::client(Reader(reader), Writer(writer), config, name)
    }
    pub fn server(
        socket: ::monoio::net::TcpStream,
        config: Arc<ServerConfig>,
    ) -> io::Result<(Stream, impl crate::driven::Driver<Error = io::Error>)> {
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        rustls::server(Reader(reader), Writer(writer), config)
    }
    /// TCP setup only. Construct the TLS driver using client(), then drive it
    /// alongside ready() and RPC. DNS and setup deadlines belong to the caller.
    pub async fn tcp_connect(address: SocketAddr) -> io::Result<::monoio::net::TcpStream> {
        ::monoio::net::TcpStream::connect(address).await
    }
}

#[cfg(feature = "webtransport-core")]
pub mod webtransport {
    use super::*;
    use crate::{
        io::{Datagram, DatagramRead, DatagramWrite},
        webtransport::driven,
    };
    pub use driven::{tls, Incoming, Limits, Request, Session, Stream};
    pub use driven::{Client as DrivenClient, Server as DrivenServer};
    use std::{rc::Rc, sync::Arc};
    pub struct UdpReader {
        socket: Rc<::monoio::net::udp::UdpSocket>,
    }
    pub struct UdpWriter {
        socket: Rc<::monoio::net::udp::UdpSocket>,
        ipv6: bool,
    }
    fn canonical(peer: SocketAddr) -> SocketAddr {
        match peer {
            SocketAddr::V6(v) => v
                .ip()
                .to_ipv4_mapped()
                .map(|ip| SocketAddr::new(ip.into(), v.port()))
                .unwrap_or(peer),
            _ => peer,
        }
    }
    fn destination(peer: SocketAddr, ipv6: bool) -> SocketAddr {
        match peer {
            SocketAddr::V4(v) if ipv6 => SocketAddr::new(v.ip().to_ipv6_mapped().into(), v.port()),
            _ => peer,
        }
    }
    impl DatagramRead for UdpReader {
        type Buffer = Vec<u8>;
        type Error = io::Error;
        async fn receive(&mut self, buffer: Vec<u8>) -> (io::Result<Datagram>, Vec<u8>) {
            let (result, buffer) = self.socket.recv_from(buffer.into_boxed_slice()).await;
            (
                result.map(|(length, peer)| Datagram {
                    length,
                    peer: canonical(peer),
                }),
                buffer.into_vec(),
            )
        }
    }
    impl DatagramWrite for UdpWriter {
        type Buffer = Vec<u8>;
        type Error = io::Error;
        async fn transmit(
            &mut self,
            buffer: Vec<u8>,
            peer: SocketAddr,
        ) -> Completion<Vec<u8>, io::Error> {
            self.socket
                .send_to(buffer, destination(peer, self.ipv6))
                .await
        }
    }
    pub fn datagrams(
        socket: ::monoio::net::udp::UdpSocket,
    ) -> io::Result<(SocketAddr, UdpReader, UdpWriter)> {
        let address = socket.local_addr()?;
        let socket = Rc::new(socket);
        Ok((
            address,
            UdpReader {
                socket: socket.clone(),
            },
            UdpWriter {
                socket,
                ipv6: address.is_ipv6(),
            },
        ))
    }
    pub fn client_driven(
        roots: tls::RootCertStore,
        clock: Arc<dyn crate::TlsClock + Send + Sync>,
    ) -> io::Result<(DrivenClient, impl crate::driven::Driver<Error = io::Error>)> {
        let socket = crate::webtransport::client_socket()?;
        // from_std does not configure O_NONBLOCK. Epoll must never enter a
        // blocking recv, while io_uring retains the socket's blocking mode.
        socket.set_nonblocking(::monoio::utils::is_legacy())?;
        let socket = ::monoio::net::udp::UdpSocket::from_std(socket)?;
        let (address, reader, writer) = datagrams(socket)?;
        Ok(driven::client(
            driven::client_config(roots, clock)?,
            address,
            reader,
            writer,
            Clock::default(),
            Limits::default(),
        ))
    }
    pub fn server_driven(
        address: SocketAddr,
        certificates: Vec<tls::pki_types::CertificateDer<'static>>,
        key: tls::pki_types::PrivateKeyDer<'static>,
        clock: Arc<dyn crate::TlsClock + Send + Sync>,
    ) -> io::Result<(DrivenServer, impl crate::driven::Driver<Error = io::Error>)> {
        let (address, reader, writer) = datagrams(::monoio::net::udp::UdpSocket::bind(address)?)?;
        Ok(driven::server(
            driven::server_config(certificates, key, clock)?,
            address,
            reader,
            writer,
            Clock::default(),
            Limits::default(),
        ))
    }
    #[derive(Clone)]
    pub struct Client {
        inner: DrivenClient,
    }
    impl Client {
        pub fn new(
            roots: tls::RootCertStore,
            clock: Arc<dyn crate::TlsClock + Send + Sync>,
        ) -> io::Result<Self> {
            let (inner, driver) = client_driven(roots, clock)?;
            drop(::monoio::spawn(driver));
            Ok(Self { inner })
        }
        pub fn local_addr(&self) -> io::Result<SocketAddr> {
            self.inner.local_addr()
        }
        /// Convenience DNS uses the host resolver. Use connect_to or the
        /// externally driven client to supply async resolution explicitly.
        pub async fn connect(&self, url: &str) -> io::Result<Session> {
            use std::net::ToSocketAddrs;
            let parsed = crate::webtransport::checked_url(url)?;
            let host = parsed.host_str().unwrap().trim_matches(['[', ']']);
            let peers = (host, parsed.port_or_known_default().unwrap()).to_socket_addrs()?;
            let mut last = io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "WebTransport host did not resolve",
            );
            for peer in peers {
                match self.inner.connect(peer, url).await {
                    Ok(session) => return Ok(session),
                    Err(error) => last = error,
                }
            }
            Err(last)
        }
        pub async fn connect_to(&self, peer: SocketAddr, url: &str) -> io::Result<Session> {
            self.inner.connect(peer, url).await
        }
    }
    #[derive(Clone)]
    pub struct Server {
        inner: DrivenServer,
    }
    impl Server {
        pub fn bind(
            address: SocketAddr,
            certificates: Vec<tls::pki_types::CertificateDer<'static>>,
            key: tls::pki_types::PrivateKeyDer<'static>,
            clock: Arc<dyn crate::TlsClock + Send + Sync>,
        ) -> io::Result<Self> {
            let (inner, driver) = server_driven(address, certificates, key, clock)?;
            drop(::monoio::spawn(driver));
            Ok(Self { inner })
        }
        pub fn local_addr(&self) -> io::Result<SocketAddr> {
            self.inner.local_addr()
        }
        pub async fn incoming(&self) -> io::Result<Incoming> {
            self.inner.incoming().await
        }
        pub async fn accept(&self) -> io::Result<Request> {
            self.inner.accept().await
        }
    }
    pub async fn listen(
        rpc: &capnp_rpc::twoparty::TwoPartyServer,
        endpoint: &Server,
        reader: capnp::message::ReaderOptions,
        timeout: Duration,
        max_pending: std::num::NonZeroUsize,
        authorize: impl Fn(&Request) -> bool,
        on_rejected: impl FnMut(io::Error),
    ) -> capnp::Result<()> {
        driven::listen(
            rpc,
            &endpoint.inner,
            driven::ListenOptions {
                reader,
                timeout,
                max_pending,
            },
            &Clock::default(),
            authorize,
            on_rejected,
        )
        .await
    }
}

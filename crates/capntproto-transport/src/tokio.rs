//! Tokio convenience types and explicit completion-IO adapters. The existing
//! direct poll TLS stream remains available without a relay or extra task.
#[cfg(any(feature = "rustls", feature = "webtransport-core"))]
use crate::io::Completion;
use crate::io::Timer;
#[cfg(feature = "rustls")]
use crate::{
    io::{Close, Read, Write},
    rustls::OwnedBuffer,
};
pub use ::tokio as runtime;
#[cfg(feature = "rustls")]
use ::tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(any(feature = "rustls", feature = "webtransport-core"))]
use std::io;
use std::time::Duration;
#[cfg(feature = "rustls")]
pub struct Reader<R>(pub R);
#[cfg(feature = "rustls")]
pub struct Writer<W>(pub W);
#[cfg(feature = "rustls")]
impl<R: AsyncRead + Unpin> Read for Reader<R> {
    type Buffer = OwnedBuffer;
    type Error = io::Error;
    async fn read(&mut self, mut buffer: OwnedBuffer) -> Completion<OwnedBuffer, io::Error> {
        let result = self.0.read(buffer.as_mut()).await;
        (result, buffer)
    }
}
#[cfg(feature = "rustls")]
impl<W: AsyncWrite + Unpin> Write for Writer<W> {
    type Buffer = OwnedBuffer;
    type Error = io::Error;
    async fn write(&mut self, buffer: OwnedBuffer) -> Completion<OwnedBuffer, io::Error> {
        let result = self.0.write(buffer.as_ref()).await;
        (result, buffer)
    }
    async fn flush(&mut self) -> io::Result<()> {
        self.0.flush().await
    }
}
#[cfg(feature = "rustls")]
impl<W: AsyncWrite + Unpin> Close for Writer<W> {
    async fn close(&mut self) -> io::Result<()> {
        self.0.shutdown().await
    }
}
pub struct Clock(::tokio::time::Instant);
impl Default for Clock {
    fn default() -> Self {
        Self(::tokio::time::Instant::now())
    }
}
impl Timer for Clock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    async fn wait_until(&self, deadline: Duration) {
        match self.0.checked_add(deadline) {
            Some(at) => ::tokio::time::sleep_until(at).await,
            None => core::future::pending().await,
        }
    }
}
#[cfg(feature = "rustls")]
pub mod tls {
    use super::*;
    pub use crate::rustls::{client_config, server_config, ClientConfig, ServerConfig, TlsStream};
    use std::sync::Arc;
    pub fn client_driven(
        socket: ::tokio::net::TcpStream,
        config: Arc<ClientConfig>,
        name: crate::rustls::pki_types::ServerName<'static>,
    ) -> io::Result<(
        crate::rustls::Stream,
        impl crate::driven::Driver<Error = io::Error>,
    )> {
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        crate::rustls::client(Reader(reader), Writer(writer), config, name)
    }
    pub fn server_driven(
        socket: ::tokio::net::TcpStream,
        config: Arc<ServerConfig>,
    ) -> io::Result<(
        crate::rustls::Stream,
        impl crate::driven::Driver<Error = io::Error>,
    )> {
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        crate::rustls::server(Reader(reader), Writer(writer), config)
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
    use std::{net::SocketAddr, sync::Arc};
    pub struct UdpReader {
        socket: Arc<::tokio::net::UdpSocket>,
    }
    pub struct UdpWriter {
        socket: Arc<::tokio::net::UdpSocket>,
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
        type Buffer = driven::ReceiveBuffer;
        type Error = io::Error;
        async fn receive(
            &mut self,
            mut buffer: driven::ReceiveBuffer,
        ) -> (io::Result<Datagram>, driven::ReceiveBuffer) {
            let result = self
                .socket
                .recv_from(&mut buffer)
                .await
                .map(|(length, peer)| Datagram {
                    length,
                    peer: canonical(peer),
                });
            (result, buffer)
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
            let result = self
                .socket
                .send_to(&buffer, destination(peer, self.ipv6))
                .await;
            (result, buffer)
        }
    }
    pub fn datagrams(
        socket: ::tokio::net::UdpSocket,
    ) -> io::Result<(SocketAddr, UdpReader, UdpWriter)> {
        let address = socket.local_addr()?;
        let socket = Arc::new(socket);
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
        socket.set_nonblocking(true)?;
        let (address, reader, writer) = datagrams(::tokio::net::UdpSocket::from_std(socket)?)?;
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
        let socket = std::net::UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        let (address, reader, writer) = datagrams(::tokio::net::UdpSocket::from_std(socket)?)?;
        Ok(driven::server(
            driven::server_config(certificates, key, clock)?,
            address,
            reader,
            writer,
            Clock::default(),
            Limits::default(),
        ))
    }

    /// Convenience ownership of a local Tokio driver. Explicit driving remains
    /// available through client_driven and the runtime-neutral driven module.
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
            ::tokio::task::spawn_local(driver);
            Ok(Self { inner })
        }
        pub fn local_addr(&self) -> io::Result<SocketAddr> {
            self.inner.local_addr()
        }
        pub async fn connect(&self, url: &str) -> io::Result<Session> {
            let parsed = crate::webtransport::checked_url(url)?;
            let host = parsed.host_str().unwrap().trim_matches(['[', ']']);
            let peers =
                ::tokio::net::lookup_host((host, parsed.port_or_known_default().unwrap())).await?;
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
            ::tokio::task::spawn_local(driver);
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

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
    use nix::{
        errno::Errno,
        sys::socket::{recvmsg, sendto, MsgFlags, SockaddrIn, SockaddrIn6, SockaddrStorage},
    };
    use std::{os::fd::AsRawFd, rc::Rc, sync::Arc};
    pub struct UdpReader {
        socket: Rc<::monoio::net::udp::UdpSocket>,
        readiness: bool,
    }
    pub struct UdpWriter {
        socket: Rc<::monoio::net::udp::UdpSocket>,
        ipv6: bool,
        readiness: bool,
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
            // Readiness backends can drain queued packets without boxed recvmsg
            // metadata. Keep io_uring's completion boundary: draining its queue
            // synchronously delays ACK/application work and amplifies Quinn's
            // linear unacknowledged-segment lookups in deep pipelines.
            if self.readiness {
                let mut iov = [io::IoSliceMut::new(&mut buffer)];
                let received = recvmsg::<SockaddrStorage>(
                    self.socket.as_raw_fd(),
                    &mut iov,
                    None,
                    MsgFlags::MSG_DONTWAIT,
                )
                .map(|packet| (packet.bytes, packet.address));
                match received {
                    Ok((length, Some(peer))) => {
                        let peer = peer
                            .as_sockaddr_in()
                            .map(|peer| SocketAddr::from(*peer))
                            .or_else(|| peer.as_sockaddr_in6().map(|peer| SocketAddr::from(*peer)));
                        let result = peer
                            .map(|peer| Datagram {
                                length,
                                peer: canonical(peer),
                            })
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "UDP packet has no IP source address",
                                )
                            });
                        return (result, buffer);
                    }
                    Ok((_, None)) => {
                        return (
                            Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "UDP packet has no source address",
                            )),
                            buffer,
                        )
                    }
                    Err(Errno::EAGAIN | Errno::EINTR) => (),
                    Err(error) => return (Err(error.into()), buffer),
                }
            }
            let (result, buffer) = self.socket.recv_from(buffer).await;
            (
                result.map(|(length, peer)| Datagram {
                    length,
                    peer: canonical(peer),
                }),
                buffer,
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
            let peer = destination(peer, self.ipv6);
            if !self.readiness {
                return self.socket.send_to(buffer, peer).await;
            }
            // UDP normally accepts a complete datagram immediately. Bypass the
            // boxed sendmsg metadata and completion round trip in that case.
            let result = match peer {
                SocketAddr::V4(peer) => sendto(
                    self.socket.as_raw_fd(),
                    &buffer,
                    &SockaddrIn::from(peer),
                    MsgFlags::MSG_DONTWAIT,
                ),
                SocketAddr::V6(peer) => sendto(
                    self.socket.as_raw_fd(),
                    &buffer,
                    &SockaddrIn6::from(peer),
                    MsgFlags::MSG_DONTWAIT,
                ),
            };
            match result {
                Ok(count) => (Ok(count), buffer),
                Err(Errno::EAGAIN | Errno::EINTR) => self.socket.send_to(buffer, peer).await,
                Err(error) => (Err(error.into()), buffer),
            }
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
                readiness: ::monoio::utils::is_legacy(),
            },
            UdpWriter {
                socket,
                ipv6: address.is_ipv6(),
                readiness: ::monoio::utils::is_legacy(),
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
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::io::{DatagramRead, DatagramWrite};
        use core::{
            future::Future,
            pin::{pin, Pin},
            task::{Context, Poll},
        };
        fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
            future.poll(&mut Context::from_waker(std::task::Waker::noop()))
        }
        async fn packets() {
            for dual_stack in [false, true] {
                let socket = if dual_stack {
                    let std = crate::webtransport::client_socket().unwrap();
                    std.set_nonblocking(::monoio::utils::is_legacy()).unwrap();
                    ::monoio::net::udp::UdpSocket::from_std(std).unwrap()
                } else {
                    ::monoio::net::udp::UdpSocket::bind("127.0.0.1:0").unwrap()
                };
                let (address, mut reader, mut writer) = datagrams(socket).unwrap();
                let destination = SocketAddr::from(([127, 0, 0, 1], address.port()));
                let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
                peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                peer.send_to(b"first", destination).unwrap();
                peer.send_to(b"", destination).unwrap();
                let mut buffer = driven::ReceiveBuffer::from(&[0xcc; 512][..]);
                let pointer = buffer.as_ptr();
                for bytes in [b"first".as_slice(), b""] {
                    let (result, returned) = if reader.readiness {
                        let (mut result, mut returned) = (None, None);
                        let allocations = allocation_counter::measure(|| {
                            let mut receiving = pin!(reader.receive(buffer));
                            match poll(receiving.as_mut()) {
                                Poll::Ready((r, b)) => {
                                    result = Some(r);
                                    returned = Some(b);
                                }
                                Poll::Pending => panic!(
                                "a queued datagram must complete without another ring submission"
                            ),
                            }
                        });
                        assert_eq!(
                            allocations.count_total, 0,
                            "ready receive must not allocate operation metadata"
                        );
                        (result.unwrap(), returned.unwrap())
                    } else {
                        reader.receive(buffer).await
                    };
                    let packet = result.unwrap();
                    buffer = returned;
                    assert_eq!(packet.length, bytes.len());
                    assert_eq!(packet.peer, peer.local_addr().unwrap());
                    assert_eq!(&buffer[..packet.length], bytes);
                    assert_eq!(buffer.len(), 512);
                    assert_eq!(buffer.as_ptr(), pointer);
                }
                let outbound = b"response".to_vec();
                let pointer = outbound.as_ptr();
                let (result, outbound) = if writer.readiness {
                    let mut sent = None;
                    let allocations = allocation_counter::measure(|| {
                        let mut sending =
                            pin!(writer.transmit(outbound, peer.local_addr().unwrap()));
                        match poll(sending.as_mut()) {
                            Poll::Ready(value) => sent = Some(value),
                            Poll::Pending => panic!("ready send must complete immediately"),
                        }
                    });
                    assert_eq!(
                        allocations.count_total, 0,
                        "ready send must not allocate operation metadata"
                    );
                    sent.unwrap()
                } else {
                    writer.transmit(outbound, peer.local_addr().unwrap()).await
                };
                assert_eq!(result.unwrap(), 8);
                assert_eq!(outbound.as_ptr(), pointer);
                let mut received = [0; 16];
                assert_eq!(peer.recv_from(&mut received).unwrap().0, 8);
                assert_eq!(&received[..8], b"response");

                // Empty sockets use the normal completion path, retaining the
                // initialized buffer over Pending and a later packet arrival.
                let mut pending = pin!(reader.receive(buffer));
                assert!(poll(pending.as_mut()).is_pending());
                peer.send_to(b"later", destination).unwrap();
                let (packet, buffer) = ::monoio::time::timeout(Duration::from_secs(2), pending)
                    .await
                    .unwrap();
                assert_eq!(packet.unwrap().length, 5);
                assert_eq!(&buffer[..5], b"later");
                assert_eq!(buffer.len(), 512);
                let bytes = b"invalid destination".to_vec();
                // An invalid IP family returns both the error and owned data.
                if !address.is_ipv6() {
                    let (error, returned) = writer
                        .transmit(bytes.clone(), "[::1]:1234".parse().unwrap())
                        .await;
                    assert!(error.is_err());
                    assert_eq!(returned, bytes);
                }
            }
        }
        #[test]
        fn udp_ready_and_pending_paths_preserve_packets_without_metadata_allocations_epoll() {
            ::monoio::RuntimeBuilder::<::monoio::LegacyDriver>::new()
                .enable_timer()
                .build()
                .unwrap()
                .block_on(packets());
        }
        #[cfg(target_os = "linux")]
        #[test]
        fn udp_completion_paths_preserve_owned_packets_uring() {
            match ::monoio::RuntimeBuilder::<::monoio::IoUringDriver>::new()
                .enable_timer()
                .build()
            {
                Ok(mut runtime) => runtime.block_on(packets()),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                    ) =>
                {
                    eprintln!("io_uring unavailable: {error}")
                }
                Err(error) => panic!("io_uring initialization failed: {error}"),
            }
        }
    }
}

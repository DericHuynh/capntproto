//! Raw datagram IO for dedicated paths, listener owners and migration probes.
//! Routed listener handles remain separate from owned candidate sockets.
use std::{io, net::SocketAddr};
use tokio::net::UdpSocket;

#[cfg(target_os = "linux")]
mod readiness;
#[cfg(target_os = "linux")]
use readiness::Socket;
#[cfg(not(target_os = "linux"))]
type Socket = UdpSocket;

/// Borrowed packet IO for discovery before a socket is moved into its owner.
/// Static dispatch keeps the production and simulated protocol loop identical.
/// Dropping a pending operation must not consume or emit a datagram.
pub(crate) trait DatagramIo {
    async fn recv_from(&self, bytes: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
    async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize>;
}
impl DatagramIo for UdpSocket {
    async fn recv_from(&self, bytes: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        UdpSocket::recv_from(self, bytes).await
    }
    async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        UdpSocket::send_to(self, bytes, to).await
    }
}

pub(crate) enum DatagramSocket {
    Udp(Socket),
    #[cfg(test)]
    Simulated(super::simulation::Socket),
}
impl DatagramSocket {
    pub(crate) fn new(socket: UdpSocket) -> io::Result<Self> {
        crate::rpc::packet_mtu::prepare(&socket)?;
        #[cfg(target_os = "linux")]
        let socket = Socket::new(socket)?;
        Ok(Self::Udp(socket))
    }
    pub(crate) async fn send_segments(
        &self,
        sender: &crate::rpc::packet_batch::Sender,
        bytes: &[u8],
        segment: usize,
        to: SocketAddr,
    ) -> io::Result<()> {
        match self {
            Self::Udp(socket) => sender.send(socket, bytes, segment, to).await,
            #[cfg(test)]
            Self::Simulated(socket) => {
                for packet in bytes.chunks(segment) {
                    socket.send_to(packet, to).await?;
                }
                Ok(())
            }
        }
    }
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Udp(s) => s.local_addr(),
            #[cfg(test)]
            Self::Simulated(s) => s.local_addr(),
        }
    }
    pub(crate) fn try_send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        match self {
            Self::Udp(s) => s.try_send_to(bytes, to),
            #[cfg(test)]
            Self::Simulated(s) => s.try_send_to(bytes, to),
        }
    }
    pub(super) fn check_open(&self) -> io::Result<()> {
        match self {
            Self::Udp(_) => Ok(()),
            #[cfg(test)]
            Self::Simulated(s) => s.check_open(),
        }
    }
}
impl DatagramIo for DatagramSocket {
    async fn recv_from(&self, bytes: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        match self {
            Self::Udp(s) => s.recv_from(bytes).await,
            #[cfg(test)]
            Self::Simulated(s) => s.recv_from(bytes).await,
        }
    }
    async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        match self {
            Self::Udp(s) => s.send_to(bytes, to).await,
            #[cfg(test)]
            Self::Simulated(s) => s.send_to(bytes, to).await,
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::{fd::OwnedFd, unix::net::UnixDatagram};

    #[tokio::test]
    async fn rejected_address_family_releases_the_owned_socket() {
        let (socket, peer) = UnixDatagram::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        // OwnedFd conversion is safe, but the descriptor's address family still
        // needs validation before configuring a QUIC path.
        let descriptor: OwnedFd = socket.into();
        let socket = UdpSocket::from_std(std::net::UdpSocket::from(descriptor)).unwrap();
        let error = DatagramSocket::new(socket).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(peer.send(b"rejected socket must be closed").is_err());
    }

    #[test]
    fn stopped_reactor_registration_releases_the_bound_port() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let socket = runtime.block_on(UdpSocket::bind("127.0.0.1:0")).unwrap();
        let address = socket.local_addr().unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        let _entered = handle.enter();
        assert!(DatagramSocket::new(socket).is_err());
        // A failed registration must relinquish ownership of the UDP socket.
        let rebound = std::net::UdpSocket::bind(address).unwrap();
        assert_eq!(rebound.local_addr().unwrap(), address);
    }
}

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

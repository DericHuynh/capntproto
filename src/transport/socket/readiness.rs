//! UDP does not need continuous writable notifications. On Linux a successful
//! send can produce another EPOLLOUT edge, waking an otherwise idle receiver.
use std::{io, net::SocketAddr};
use tokio::{
    io::{unix::AsyncFd, Interest},
    net::UdpSocket,
};

// recvmsg's control headers require cmsghdr alignment, including when the
// backing bytes live on the stack. No per-datagram allocation is needed.
#[repr(C)]
struct ReceiveControl {
    _align: [nix::libc::cmsghdr; 0],
    bytes: [u8; 256],
}

pub(crate) struct Socket {
    read: AsyncFd<std::net::UdpSocket>,
}
impl Socket {
    pub(super) fn new(socket: UdpSocket) -> io::Result<Self> {
        Ok(Self {
            read: AsyncFd::with_interest(socket.into_std()?, Interest::READABLE)?,
        })
    }
    pub(super) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.read.get_ref().local_addr()
    }
    pub(super) fn enable_recv_aggregation(&self) {
        // Optional Linux offload. Unsupported kernels keep individual packets.
        let _ = nix::sys::socket::setsockopt(
            self.read.get_ref(),
            nix::sys::socket::sockopt::UdpGroSegment,
            &true,
        );
    }
    pub(super) async fn recv_batch(
        &self,
        bytes: &mut [u8],
    ) -> io::Result<(usize, SocketAddr, usize)> {
        use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags, SockaddrStorage};
        use std::os::fd::AsRawFd;
        loop {
            let mut ready = self.read.readable().await?;
            if let Ok(result) = ready.try_io(|socket| {
                // Leave room for ordinary ancillary options on caller-owned
                // sockets (timestamps/pktinfo), in addition to UDP_GRO.
                let mut control = ReceiveControl {
                    _align: [],
                    bytes: [0; 256],
                };
                let mut buffers = [io::IoSliceMut::new(bytes)];
                let received = recvmsg::<SockaddrStorage>(
                    socket.get_ref().as_raw_fd(),
                    &mut buffers,
                    Some(&mut control.bytes),
                    MsgFlags::MSG_DONTWAIT,
                )?;
                let address = received
                    .address
                    .ok_or_else(|| io::Error::other("missing UDP source"))?;
                let from = if let Some(v4) = address.as_sockaddr_in() {
                    SocketAddr::new(v4.ip().into(), v4.port())
                } else if let Some(v6) = address.as_sockaddr_in6() {
                    SocketAddr::V6(std::net::SocketAddrV6::new(
                        v6.ip(),
                        v6.port(),
                        v6.flowinfo(),
                        v6.scope_id(),
                    ))
                } else {
                    return Err(io::Error::other("invalid UDP source"));
                };
                // Treat truncation as packet loss. Never feed a prefix or an
                // aggregate with missing boundaries to QUIC, and never let a
                // bad datagram close the shared listener.
                if received
                    .flags
                    .intersects(MsgFlags::MSG_TRUNC | MsgFlags::MSG_CTRUNC)
                {
                    return Ok((0, from, 1));
                }
                let mut segment = received.bytes.max(1);
                for message in received.cmsgs()? {
                    if let ControlMessageOwned::UdpGroSegments(size) = message {
                        let Some(size) = usize::try_from(size)
                            .ok()
                            .filter(|n| *n > 0 && *n <= received.bytes)
                        else {
                            return Ok((0, from, 1));
                        };
                        segment = size;
                    }
                }
                if received.bytes.div_ceil(segment) > 64 {
                    return Ok((0, from, 1));
                }
                Ok((received.bytes, from, segment))
            }) {
                return result;
            }
        }
    }
    pub(super) fn try_send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        self.read.get_ref().send_to(bytes, to)
    }
    pub(super) async fn recv_from(&self, bytes: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        loop {
            let mut ready = self.read.readable().await?;
            if let Ok(result) = ready.try_io(|socket| socket.get_ref().recv_from(bytes)) {
                return result;
            }
        }
    }
    async fn send_io(&self, mut send: impl FnMut() -> io::Result<usize>) -> io::Result<usize> {
        match send() {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
            result => return result,
        }
        // Subscribe only while backpressured. A duplicated descriptor refers to
        // the same socket, and dropping this future removes its registration.
        let write = AsyncFd::with_interest(self.read.get_ref().try_clone()?, Interest::WRITABLE)?;
        loop {
            let mut ready = write.writable().await?;
            if let Ok(result) = ready.try_io(|_| send()) {
                return result;
            }
        }
    }
    pub(super) async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        self.send_io(|| self.try_send_to(bytes, to)).await
    }
}
impl crate::rpc::packet_batch::DatagramSender for Socket {
    async fn send_packet(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        self.send_to(bytes, to).await
    }
    async fn send_gso(&self, bytes: &[u8], segment: u16, to: SocketAddr) -> io::Result<usize> {
        self.send_io(|| crate::rpc::packet_batch::try_gso(self.read.get_ref(), bytes, segment, to))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::{cell::Cell, time::Duration};

    async fn socket() -> Socket {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        crate::rpc::packet_mtu::prepare(&socket).unwrap();
        Socket::new(socket).unwrap()
    }

    #[tokio::test]
    async fn receive_aggregation_preserves_boundaries_sources_and_cancellation() {
        for address in ["127.0.0.1:0", "[::1]:0"] {
            for aggregation in [false, true] {
                let tx = Socket::new(UdpSocket::bind(address).await.unwrap()).unwrap();
                let rx = Socket::new(UdpSocket::bind(address).await.unwrap()).unwrap();
                if aggregation {
                    rx.enable_recv_aggregation();
                }
                let mut output = vec![0; 65535];
                assert!(rx.recv_batch(&mut output).now_or_never().is_none());
                let expected = [vec![17; 1200], vec![29; 1200], vec![31; 57]];
                let bytes = expected.concat();
                crate::rpc::packet_batch::Sender::default()
                    .send(&tx, &bytes, 1200, rx.local_addr().unwrap())
                    .await
                    .unwrap();
                let mut packets = Vec::new();
                while packets.len() < expected.len() {
                    let (length, from, segment) =
                        tokio::time::timeout(Duration::from_secs(1), rx.recv_batch(&mut output))
                            .await
                            .unwrap()
                            .unwrap();
                    assert_eq!(from, tx.local_addr().unwrap());
                    packets.extend(output[..length].chunks(segment).map(<[u8]>::to_vec));
                }
                assert_eq!(packets, expected);
                assert!(rx.recv_batch(&mut output).now_or_never().is_none());
                tx.send_to(b"next", rx.local_addr().unwrap()).await.unwrap();
                let (length, from, segment) =
                    tokio::time::timeout(Duration::from_secs(1), rx.recv_batch(&mut output))
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(&output[..length], b"next");
                assert_eq!(segment, length);
                assert_eq!(from, tx.local_addr().unwrap());
            }
        }
    }

    #[tokio::test]
    async fn truncated_aggregates_are_discarded_without_closing_the_socket() {
        let tx = socket().await;
        let rx = socket().await;
        rx.enable_recv_aggregation();
        crate::rpc::packet_batch::Sender::default()
            .send(&tx, &[7; 2400], 1200, rx.local_addr().unwrap())
            .await
            .unwrap();
        tx.send_to(b"after loss", rx.local_addr().unwrap())
            .await
            .unwrap();
        let mut output = [0; 64];
        let mut discarded = 0;
        loop {
            let (length, from, segment) =
                tokio::time::timeout(Duration::from_secs(1), rx.recv_batch(&mut output))
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(from, tx.local_addr().unwrap());
            if length == 0 {
                discarded += 1;
                assert_eq!(segment, 1);
            } else {
                assert_eq!(&output[..length], b"after loss");
                break;
            }
        }
        assert!((1..=2).contains(&discarded));
    }

    #[tokio::test]
    async fn canceled_reads_and_segmented_sends_preserve_datagram_boundaries() {
        let a = socket().await;
        let b = socket().await;
        let mut buffer = [0; 1500];
        assert!(b.recv_from(&mut buffer).now_or_never().is_none());
        let sender = crate::rpc::packet_batch::Sender::default();
        let bytes: Vec<_> = [vec![1; 1200], vec![2; 1200], vec![3; 17]].concat();
        sender
            .send(&a, &bytes, 1200, b.local_addr().unwrap())
            .await
            .unwrap();
        for expected in bytes.chunks(1200) {
            let (n, from) = tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&buffer[..n], expected);
            assert_eq!(from, a.local_addr().unwrap());
        }
        assert!(b.recv_from(&mut buffer).now_or_never().is_none());
        // A rejected probe must not close the socket or block subsequent data.
        sender
            .send(&a, &vec![0; 65508], 65508, b.local_addr().unwrap())
            .await
            .unwrap();
        a.send_to(b"after probe", b.local_addr().unwrap())
            .await
            .unwrap();
        let (n, _) = b.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..n], b"after probe");
    }

    #[tokio::test]
    async fn blocked_sends_wait_for_readiness_and_cancellation_releases_registration() {
        let socket = socket().await;
        let calls = Cell::new(0);
        let sent = tokio::time::timeout(
            Duration::from_secs(1),
            socket.send_io(|| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Err(io::ErrorKind::WouldBlock.into())
                } else {
                    Ok(17)
                }
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(sent, 17);
        assert_eq!(calls.get(), 2);
        assert!(socket
            .send_io(|| Err(io::ErrorKind::WouldBlock.into()))
            .now_or_never()
            .is_none());
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket
            .send_to(b"still open", peer.local_addr().unwrap())
            .await
            .unwrap();
        let mut buffer = [0; 20];
        let n = peer.recv(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..n], b"still open");
        assert_eq!(
            socket
                .send_io(|| Err(io::ErrorKind::ConnectionReset.into()))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::ConnectionReset
        );
    }
}

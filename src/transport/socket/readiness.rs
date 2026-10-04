//! UDP does not need continuous writable notifications. On Linux a successful
//! send can produce another EPOLLOUT edge, waking an otherwise idle receiver.
use std::{io, net::SocketAddr};
use tokio::{
    io::{unix::AsyncFd, Interest},
    net::UdpSocket,
};

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

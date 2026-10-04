//! Bounded QUIC aggregates. Every datagram keeps its wire boundary, destination
//! and pacing deadline; Linux can segment the aggregate in one kernel call.
use std::cell::Cell;
use std::{io, net::SocketAddr};
use tokio::net::UdpSocket;

pub(crate) trait DatagramSender {
    async fn send_packet(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize>;
    #[cfg(target_os = "linux")]
    async fn send_gso(&self, bytes: &[u8], segment: u16, to: SocketAddr) -> io::Result<usize>;
}
#[cfg(target_os = "linux")]
pub(crate) fn try_gso(
    socket: &impl std::os::fd::AsRawFd,
    bytes: &[u8],
    segment: u16,
    to: SocketAddr,
) -> io::Result<usize> {
    use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags, SockaddrStorage};
    sendmsg(
        socket.as_raw_fd(),
        &[io::IoSlice::new(bytes)],
        &[ControlMessage::UdpGsoSegments(&segment)],
        MsgFlags::MSG_DONTWAIT,
        Some(&SockaddrStorage::from(to)),
    )
    .map_err(io::Error::from)
}
impl DatagramSender for UdpSocket {
    async fn send_packet(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        self.send_to(bytes, to).await
    }
    #[cfg(target_os = "linux")]
    async fn send_gso(&self, bytes: &[u8], segment: u16, to: SocketAddr) -> io::Result<usize> {
        self.async_io(tokio::io::Interest::WRITABLE, || {
            try_gso(self, bytes, segment, to)
        })
        .await
    }
}

pub(crate) struct Batch {
    pub bytes: Vec<u8>,
    pub info: Option<quiche::SendInfo>,
    pub segment: usize,
    short: bool,
    quantum: usize,
    count: usize,
}
impl Batch {
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(65507),
            info: None,
            segment: 0,
            short: false,
            quantum: 0,
            count: 0,
        }
    }
    pub fn push(&mut self, bytes: &[u8], info: quiche::SendInfo, quantum: usize) -> bool {
        if let Some(previous) = &self.info {
            if self.short
                || bytes.len() > self.segment
                || previous.from != info.from
                || previous.to != info.to
                || self.bytes.len() + bytes.len() > self.quantum
                || self.bytes.len() + bytes.len() > 65507
                || self.count == 16
            {
                return false;
            }
        } else {
            self.segment = bytes.len();
            self.quantum = quantum.max(bytes.len());
        }
        self.short = bytes.len() < self.segment;
        self.count += 1;
        let at = self
            .info
            .as_ref()
            .map_or(info.at, |previous| previous.at.max(info.at));
        self.info = Some(quiche::SendInfo { at, ..info });
        self.bytes.extend_from_slice(bytes);
        true
    }
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.info = None;
        self.short = false;
        self.count = 0;
    }
    #[cfg(any(feature = "quic", test))]
    pub async fn send(&mut self, sender: &Sender, socket: &UdpSocket) -> io::Result<()> {
        if let Some(info) = self.info {
            super::pacing::wait_until(info.at).await;
            sender
                .send(socket, &self.bytes, self.segment, info.to)
                .await?;
            self.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn aggregates_preserve_boundaries_paths_quantum_and_latest_deadline() {
        let info = quiche::SendInfo {
            from: "127.0.0.1:1000".parse().unwrap(),
            to: "127.0.0.1:2000".parse().unwrap(),
            at: Instant::now(),
        };
        let later = quiche::SendInfo {
            at: info.at + Duration::from_millis(1),
            ..info
        };
        let mut batch = Batch::new();
        assert!(batch.push(&[1; 1200], info, 3000));
        assert!(!batch.push(&[2; 1201], info, 3000));
        assert!(!batch.push(
            &[2; 1200],
            quiche::SendInfo {
                to: info.from,
                ..info
            },
            3000
        ));
        assert!(!batch.push(
            &[2; 1200],
            quiche::SendInfo {
                from: info.to,
                ..info
            },
            3000
        ));
        assert!(batch.push(&[2; 1200], later, 3000));
        assert!(!batch.push(&[3; 1200], info, 3000));
        assert!(batch.push(&[3; 600], info, 3000));
        assert_eq!(batch.info.unwrap().at, later.at);
        assert!(!batch.push(&[4; 1], info, 3000));
        assert_eq!(
            batch.bytes,
            [&[1; 1200][..], &[2; 1200], &[3; 600]].concat()
        );
        batch.clear();
        for _ in 0..16 {
            assert!(batch.push(&[0; 1350], info, usize::MAX));
        }
        assert!(!batch.push(&[0; 1350], info, usize::MAX));
        batch.clear();
        assert!(batch.push(&[0; 1350], info, 1)); // never split one QUIC datagram
        assert!(!batch.push(&[0; 1], info, 1));
        batch.clear();
        for _ in 0..3 {
            assert!(batch.push(&[0; 16384], info, usize::MAX));
        }
        assert!(!batch.push(&[0; 16384], info, usize::MAX));
    }

    #[tokio::test]
    async fn offload_and_fallback_deliver_identical_datagrams_without_duplicates() {
        for fallback in [false, true] {
            let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let sender = Sender::default();
            #[cfg(target_os = "linux")]
            sender.unsupported.set(fallback);
            let _ = fallback;
            let packets = [vec![1; 1200], vec![2; 1200], vec![3; 17]];
            let info = quiche::SendInfo {
                from: tx.local_addr().unwrap(),
                to: rx.local_addr().unwrap(),
                at: Instant::now(),
            };
            let mut batch = Batch::new();
            for packet in &packets {
                assert!(batch.push(packet, info, 64 * 1024));
            }
            batch.send(&sender, &tx).await.unwrap();
            assert!(batch.info.is_none());
            // Sending the emptied batch must not duplicate the last aggregate.
            batch.send(&sender, &tx).await.unwrap();
            let mut bytes = [0; 1500];
            for packet in packets {
                let (n, from) =
                    tokio::time::timeout(Duration::from_secs(2), rx.recv_from(&mut bytes))
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(from, info.from);
                assert_eq!(&bytes[..n], packet);
            }
            assert_eq!(
                rx.try_recv_from(&mut bytes).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn oversized_probe_is_loss_and_does_not_prevent_smaller_packets() {
        let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        super::super::packet_mtu::prepare(&tx).unwrap();
        let sender = Sender::default();
        // Above the absolute IPv4 UDP payload limit, even on loopback.
        sender
            .send(&tx, &vec![0; 65508], 65508, rx.local_addr().unwrap())
            .await
            .unwrap();
        assert!(sender.mtu_loss.get());
        sender
            .send(&tx, &[42; 1200], 1200, rx.local_addr().unwrap())
            .await
            .unwrap();
        let mut bytes = [0; 1500];
        let (n, _) = tokio::time::timeout(Duration::from_secs(1), rx.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes[..n], &[42; 1200]);
        assert_eq!(
            rx.try_recv_from(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}

/// Cache unsupported GSO per connection. A rejected aggregate is sent as the
/// original individual datagrams; a successful aggregate is never replayed.
#[derive(Default)]
pub(crate) struct Sender {
    #[cfg(target_os = "linux")]
    unsupported: Cell<bool>,
    mtu_loss: Cell<bool>,
}
impl Sender {
    pub fn check_path_mtu(&self, conn: &mut quiche::Connection) {
        // A probe that is too large is packet loss for quiche's DPLPMTUD. A
        // previously discovered path size can also become invalid; ask the
        // upstream engine to revalidate it rather than killing reliable RPC.
        if self.mtu_loss.replace(false) && conn.pmtu().is_some() {
            conn.revalidate_pmtu();
        }
    }
    pub async fn send(
        &self,
        socket: &impl DatagramSender,
        bytes: &[u8],
        segment: usize,
        to: SocketAddr,
    ) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        if bytes.len() > segment && !self.unsupported.get() {
            loop {
                let sent = socket.send_gso(bytes, segment as u16, to).await;
                match sent {
                    Ok(n) if n == bytes.len() => return Ok(()),
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "partial UDP aggregate",
                        ))
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(error) if error.raw_os_error() == Some(libc::EMSGSIZE) => break,
                    Err(error)
                        if matches!(
                            error.raw_os_error(),
                            Some(libc::EINVAL | libc::EIO | libc::ENOPROTOOPT | libc::EOPNOTSUPP)
                        ) =>
                    {
                        self.unsupported.set(true);
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        for packet in bytes.chunks(segment) {
            match socket.send_packet(packet, to).await {
                Ok(n) if n == packet.len() => (),
                Err(error) if super::packet_mtu::too_large(&error, packet.len()) => {
                    self.mtu_loss.set(true);
                }
                Err(error) => return Err(error),
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "partial UDP datagram",
                    ))
                }
            }
        }
        Ok(())
    }
}

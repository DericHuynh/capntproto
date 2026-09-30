//! Bounded STUN address discovery for an owned UDP socket (RFC 8489).
//! An observed address is routing data. Only the subsequent Noise handshake
//! authenticates a peer. This is not a complete ICE/TURN implementation.
use crate::transport::socket::DatagramIo;
use ring::rand::SecureRandom;
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use tokio::{net::UdpSocket, time::Instant};

mod maintenance;
pub(crate) use maintenance::MappingState;
pub use maintenance::{Mapping, MappingObserver, MappingOptions, MappingStatus};

const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];

fn fingerprint(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc ^ 0x5354_554e
}

pub(crate) fn address_ok(address: SocketAddr) -> bool {
    !address.ip().is_unspecified()
        && !address.ip().is_multicast()
        && address.ip() != IpAddr::V4(Ipv4Addr::BROADCAST)
        && address.port() != 0
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid STUN binding response")
}
fn request(transaction: [u8; 12]) -> [u8; 20] {
    let mut packet = [0; 20];
    packet[1] = 1;
    packet[4..8].copy_from_slice(&COOKIE);
    packet[8..].copy_from_slice(&transaction);
    packet
}
fn parse(packet: &[u8], transaction: [u8; 12]) -> io::Result<SocketAddr> {
    if packet.len() < 20
        || packet[..2] != [1, 1]
        || packet[4..8] != COOKIE
        || packet[8..20] != transaction
    {
        return Err(invalid());
    }
    let length = u16::from_be_bytes(packet[2..4].try_into().unwrap()) as usize;
    if !length.is_multiple_of(4) || length + 20 != packet.len() {
        return Err(invalid());
    }
    let mut rest = &packet[20..];
    let mut address = None;
    while !rest.is_empty() {
        if rest.len() < 4 {
            return Err(invalid());
        }
        let kind = u16::from_be_bytes(rest[..2].try_into().unwrap());
        let n = u16::from_be_bytes(rest[2..4].try_into().unwrap()) as usize;
        let padded = (n + 3) & !3;
        if rest.len() < 4 + padded {
            return Err(invalid());
        }
        let value = &rest[4..4 + n];
        if kind == 0x0020 && address.is_none() {
            // RFC 8489 sections 14.1/14.2 require receivers to ignore the
            // reserved first byte. The first address occurrence wins.
            if n < 4 {
                return Err(invalid());
            }
            let port = u16::from_be_bytes(value[2..4].try_into().unwrap()) ^ 0x2112;
            let mut mask = [0; 16];
            mask[..4].copy_from_slice(&COOKIE);
            mask[4..].copy_from_slice(&transaction);
            let ip = match (value[1], n) {
                (1, 8) => {
                    let mut ip = [0; 4];
                    for i in 0..4 {
                        ip[i] = value[4 + i] ^ mask[i];
                    }
                    IpAddr::V4(Ipv4Addr::from(ip))
                }
                (2, 20) => {
                    let mut ip = [0; 16];
                    for i in 0..16 {
                        ip[i] = value[4 + i] ^ mask[i];
                    }
                    IpAddr::V6(Ipv6Addr::from(ip))
                }
                _ => return Err(invalid()),
            };
            address = Some(SocketAddr::new(ip, port));
        } else if kind == 0x8028 {
            if n != 4
                || rest.len() != 8
                || u32::from_be_bytes(value.try_into().unwrap())
                    != fingerprint(&packet[..packet.len() - 8])
            {
                return Err(invalid());
            }
        } else if kind < 0x8000 && kind != 0x0020 && kind != 0x0001 {
            // This unauthenticated Binding usage requests no optional protocol
            // extensions. Do not silently accept comprehension-required ones.
            return Err(invalid());
        }
        rest = &rest[4 + padded..];
    }
    address.filter(|a| address_ok(*a)).ok_or_else(invalid)
}

/// Discover the mapping of this exact socket before starting its packet driver.
/// Retransmits at 500ms, 1s, then 2s intervals within a caller-supplied (0,10s]
/// budget. A response must match both server address and random transaction ID.
/// The budget includes blocked sends; a pending retry does not block replies.
/// No DNS redirects or alternate-server instructions are followed.
pub async fn discover(
    socket: &UdpSocket,
    server: SocketAddr,
    timeout: Duration,
) -> io::Result<SocketAddr> {
    discover_on(socket, server, timeout).await
}
pub(crate) async fn discover_on(
    socket: &impl DatagramIo,
    server: SocketAddr,
    timeout: Duration,
) -> io::Result<SocketAddr> {
    if !address_ok(server) || timeout.is_zero() || timeout > Duration::from_secs(10) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid STUN discovery settings",
        ));
    }
    let mut transaction = [0; 12];
    ring::rand::SystemRandom::new()
        .fill(&mut transaction)
        .map_err(|_| io::Error::other("OS randomness unavailable"))?;
    let packet = request(transaction);
    let deadline = Instant::now() + timeout;
    let mut next = Instant::now();
    let mut backoff = Duration::from_millis(500);
    // One extra byte distinguishes an oversized datagram from an exact limit
    // response; recv_from otherwise silently truncates to the buffer length.
    let mut buf = [0; 1025];
    loop {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => return Err(io::Error::new(io::ErrorKind::TimedOut, "STUN discovery timed out")),
            // Keep a pending UDP send inside the select: its readiness must
            // not hide the overall deadline or a reply to an earlier probe.
            sent = async {
                tokio::time::sleep_until(next).await;
                socket.send_to(&packet, server).await
            } => {
                sent?;
                next = Instant::now() + backoff;
                backoff = (backoff * 2).min(Duration::from_secs(2));
            },
            received = socket.recv_from(&mut buf) => {
                let (n, from) = received?;
                if from == server && n <= 1024 {
                    if let Ok(address) = parse(&buf[..n], transaction) { return Ok(address); }
                }
            }
        }
    }
}

/// A small Binding indication opens a UDP filtering entry without allocating a
/// session at the receiver. It carries no identity proof or application data.
pub(crate) async fn punch(
    socket: &crate::transport::socket::DatagramSocket,
    peer: SocketAddr,
) -> io::Result<()> {
    if !address_ok(peer) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid rendezvous address",
        ));
    }
    let mut packet = request(crate::transport::cid()[..12].try_into().unwrap());
    packet[0] = 0;
    packet[1] = 0x11;
    socket.send_to(&packet, peer).await?;
    Ok(())
}

pub(crate) fn is_binding_message(packet: &[u8]) -> bool {
    // Discovery retransmissions can leave delayed/duplicate replies on the
    // socket after Noise starts. They are not transport frames or authority.
    packet.len() >= 20
        && packet[4..8] == COOKIE
        && matches!(
            u16::from_be_bytes([packet[0], packet[1]]),
            0x0001 | 0x0011 | 0x0101 | 0x0111
        )
        && usize::from(u16::from_be_bytes([packet[2], packet[3]])) + 20 == packet.len()
        && (packet.len() - 20).is_multiple_of(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn response(id: [u8; 12], address: SocketAddr) -> Vec<u8> {
        let mut out = request(id).to_vec();
        out[0] = 1;
        let ip = match address.ip() {
            IpAddr::V4(ip) => ip.octets().to_vec(),
            IpAddr::V6(ip) => ip.octets().to_vec(),
        };
        let n = 4 + ip.len();
        out[2..4].copy_from_slice(&((n + 4) as u16).to_be_bytes());
        out.extend_from_slice(&[0, 0x20, 0, n as u8, 0, if ip.len() == 4 { 1 } else { 2 }]);
        out.extend_from_slice(&(address.port() ^ 0x2112).to_be_bytes());
        let mask: Vec<_> = COOKIE.into_iter().chain(id).collect();
        out.extend(ip.into_iter().zip(mask).map(|(a, b)| a ^ b));
        out
    }
    #[test]
    fn binding_parser_checks_transaction_framing_family_and_attributes() {
        for address in ["192.0.2.42:43123", "[2001:db8::42]:43123"] {
            let address = address.parse().unwrap();
            let packet = response([7; 12], address);
            assert_eq!(parse(&packet, [7; 12]).unwrap(), address);
            for n in 0..packet.len() {
                assert!(parse(&packet[..n], [7; 12]).is_err());
            }
            assert!(parse(&packet, [8; 12]).is_err());
            for at in [0, 1, 2, 3, 4, 8, 20, 21, 22, 23, 25] {
                let mut bad = packet.clone();
                bad[at] ^= 0x80;
                assert!(parse(&bad, [7; 12]).is_err(), "{at}");
            }
            let mut reserved = packet.clone();
            reserved[24] = 0x80;
            assert_eq!(parse(&reserved, [7; 12]).unwrap(), address);
            let other = response([7; 12], "192.0.2.1:34567".parse().unwrap());
            let mut duplicate = packet.clone();
            duplicate.extend_from_slice(&other[20..]);
            let length = (duplicate.len() - 20) as u16;
            duplicate[2..4].copy_from_slice(&length.to_be_bytes());
            assert_eq!(parse(&duplicate, [7; 12]).unwrap(), address);
        }
    }
    #[test]
    fn fingerprint_must_be_last_and_match_entire_message() {
        // Independent CRC32 check value from the standard check string.
        assert_eq!(fingerprint(b"123456789") ^ 0x5354_554e, 0xcbf4_3926);
        let address = "192.0.2.1:23456".parse().unwrap();
        let mut packet = response([7; 12], address);
        packet[2..4].copy_from_slice(&20u16.to_be_bytes());
        let crc = fingerprint(&packet);
        packet.extend_from_slice(&[0x80, 0x28, 0, 4]);
        packet.extend_from_slice(&crc.to_be_bytes());
        assert_eq!(parse(&packet, [7; 12]).unwrap(), address);
        *packet.last_mut().unwrap() ^= 1;
        assert!(parse(&packet, [7; 12]).is_err());
        *packet.last_mut().unwrap() ^= 1;
        packet.extend_from_slice(&[0x80, 0x22, 0, 0]);
        packet[2..4].copy_from_slice(&24u16.to_be_bytes());
        assert!(parse(&packet, [7; 12]).is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn discovery_ignores_wrong_source_and_transaction_and_reuses_socket() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let expected = socket.local_addr().unwrap();
        let answer = async {
            let mut buf = [0; 100];
            let (_, from) = server.recv_from(&mut buf).await.unwrap();
            let id = buf[8..20].try_into().unwrap();
            let fake = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            fake.send_to(&response(id, "192.0.2.1:5555".parse().unwrap()), from)
                .await
                .unwrap();
            server
                .send_to(&response([0; 12], from), from)
                .await
                .unwrap();
            server.send_to(&response(id, from), from).await.unwrap();
        };
        let (observed, ()) =
            tokio::join!(discover(&socket, address, Duration::from_secs(2)), answer);
        assert_eq!(observed.unwrap(), expected);
        assert_eq!(socket.local_addr().unwrap(), expected);
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn silent_server_is_bounded() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        assert_eq!(
            discover(
                &socket,
                server.local_addr().unwrap(),
                Duration::from_secs(2)
            )
            .await
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
    }
}

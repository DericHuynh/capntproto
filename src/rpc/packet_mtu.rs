//! Packet bounds and unfragmented path-MTU discovery for QUIC.
use std::io;
use tokio::net::UdpSocket;

#[cfg(target_os = "linux")]
pub(crate) const MIN: usize = 1200;
pub(crate) const RECEIVE_MAX: usize = 16 * 1024;
// Larger probes are enabled only where we configure the socket to forbid IP
// fragmentation. Other platforms retain the conservative outgoing bound.
pub(crate) const SEND_MAX: usize = if cfg!(target_os = "linux") {
    RECEIVE_MAX
} else {
    1350
};

pub(crate) fn prepare(socket: &UdpSocket) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::net::sockopt::{self, Ipv4PathMtuDiscovery, Ipv6PathMtuDiscovery};
        if socket.local_addr()?.is_ipv4() {
            sockopt::set_ip_mtu_discover(socket, Ipv4PathMtuDiscovery::DO)?;
        } else {
            sockopt::set_ipv6_mtu_discover(socket, Ipv6PathMtuDiscovery::DO)?;
            if !sockopt::ipv6_v6only(socket)? {
                sockopt::set_ip_mtu_discover(socket, Ipv4PathMtuDiscovery::DO)?;
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = socket;
    Ok(())
}

/// Reprobe a discovered MTU after consecutive timer expirations without ACK
/// progress. This also recovers from silent path black holes, where the local
/// UDP socket cannot report EMSGSIZE. Quiche controls probing and retransmits.
#[derive(Default)]
pub(crate) struct Recovery {
    previous_acked: Option<u64>,
}
impl Recovery {
    pub(crate) fn on_timeout(&mut self, conn: &mut quiche::Connection<impl quiche::BufFactory>) {
        // The adapter also wakes for migration polling and application-clock
        // deadlines. Those are not evidence of packet loss. In particular,
        // Tokio's clock may advance before quiche's system-clock timer expires.
        if conn.timeout() != Some(std::time::Duration::ZERO) {
            return;
        }
        let acked = conn.stats().acked_bytes;
        if self.previous_acked == Some(acked) && conn.pmtu().is_some() {
            conn.revalidate_pmtu();
        }
        self.previous_acked = Some(acked);
        conn.on_timeout();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn early_application_wakeups_do_not_count_as_recovery_expirations() {
        use crate::transport::{config, Identity};
        use std::time::Duration;
        let local = Identity::generate();
        let peer = Identity::generate();
        let mut config = config(&local, peer.public_key(), None, b"early recovery").unwrap();
        config.set_initial_rtt(Duration::from_secs(30));
        config.set_max_idle_timeout(60_000);
        let mut conn = quiche::connect(
            None,
            &quiche::ConnectionId::from_ref(&[1; 16]),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:4321".parse().unwrap(),
            &mut config,
        )
        .unwrap();
        let mut recovery = Recovery::default();
        // Before packet output there may be no armed recovery timer at all.
        recovery.on_timeout(&mut conn);
        assert_eq!(recovery.previous_acked, None);
        conn.send(&mut [0; 1350]).unwrap();
        assert!(conn.timeout().unwrap() > Duration::from_secs(1));
        // Two migration/cancellation polls without ACK progress must not look
        // like two expired recovery timers and initiate a spurious MTU reprobe.
        recovery.on_timeout(&mut conn);
        recovery.on_timeout(&mut conn);
        assert_eq!(recovery.previous_acked, None);
        assert!(!conn.is_closed());
    }

    #[tokio::test]
    async fn probes_forbid_fragmentation_for_ipv4_ipv6_and_mapped_ipv4() {
        use rustix::net::sockopt::{self, Ipv4PathMtuDiscovery, Ipv6PathMtuDiscovery};
        for address in ["127.0.0.1:0", "[::1]:0"] {
            let socket = UdpSocket::bind(address).await.unwrap();
            prepare(&socket).unwrap();
            if socket.local_addr().unwrap().is_ipv6() {
                assert_eq!(
                    sockopt::ipv6_mtu_discover(&socket).unwrap(),
                    Ipv6PathMtuDiscovery::DO
                );
                if sockopt::ipv6_v6only(&socket).unwrap() {
                    continue;
                }
            }
            assert_eq!(
                sockopt::ip_mtu_discover(&socket).unwrap(),
                Ipv4PathMtuDiscovery::DO
            );
        }
        assert!(too_large(
            &io::Error::from_raw_os_error(libc::EMSGSIZE),
            1201
        ));
        assert!(!too_large(
            &io::Error::from_raw_os_error(libc::EMSGSIZE),
            1200
        ));
        assert!(!too_large(&io::Error::from_raw_os_error(libc::EIO), 1400));
    }
}

pub(crate) fn too_large(error: &io::Error, bytes: usize) -> bool {
    #[cfg(target_os = "linux")]
    return bytes > MIN && error.raw_os_error() == Some(rustix::io::Errno::MSGSIZE.raw_os_error());
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (error, bytes);
        false
    }
}

//! WebTransport over HTTP/3. Each reliable bidirectional stream can carry an
//! independent Cap'n Proto RPC connection. Session/stream owners keep the endpoint
//! alive; closing one RPC stream does not close its siblings.
//!
//! Native endpoints use rustls with the caller's clock. Browsers implement their
//! own certificate validation and clock; this API cannot override either.
#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
mod native;
#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
pub use native::*;
#[cfg(all(target_arch = "wasm32", feature = "webtransport"))]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
pub mod driven;
#[cfg(all(target_arch = "wasm32", feature = "webtransport"))]
pub use browser::*;

pub(crate) fn checked_url(value: &str) -> std::io::Result<url::Url> {
    let url = url::Url::parse(value).map_err(std::io::Error::other)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "WebTransport requires an HTTPS URL without user information or a fragment",
        ));
    }
    Ok(url)
}

pub use crate::rpc::{driver, network};

// Select dual-stack explicitly instead of relying on IPV6_V6ONLY OS defaults.
// Runtime adapters own binding; the protocol/driver factories stay IO-free.
#[cfg(all(
    not(target_arch = "wasm32"),
    any(feature = "tokio", feature = "monoio")
))]
pub(crate) fn client_socket() -> std::io::Result<std::net::UdpSocket> {
    let ipv6 = || -> std::io::Result<std::net::UdpSocket> {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;
        socket.set_only_v6(false)?;
        socket.bind(&"[::]:0".parse::<std::net::SocketAddr>().unwrap().into())?;
        Ok(socket.into())
    };
    ipv6().or_else(|_| std::net::UdpSocket::bind("0.0.0.0:0"))
}

#[cfg(test)]
mod tests {
    #[cfg(all(
        not(target_arch = "wasm32"),
        any(feature = "tokio", feature = "monoio")
    ))]
    #[test]
    fn default_client_socket_accepts_ipv4_and_available_ipv6() {
        use std::{
            net::{SocketAddr, UdpSocket},
            time::Duration,
        };
        let client = super::client_socket().unwrap();
        if client.local_addr().unwrap().is_ipv6() {
            assert!(!socket2::SockRef::from(&client).only_v6().unwrap());
        }
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let address = match peer.local_addr().unwrap() {
            SocketAddr::V4(peer) if client.local_addr().unwrap().is_ipv6() => {
                SocketAddr::new(peer.ip().to_ipv6_mapped().into(), peer.port())
            }
            address => address,
        };
        client.send_to(b"v4", address).unwrap();
        let mut bytes = [0; 2];
        assert_eq!(peer.recv_from(&mut bytes).unwrap().0, 2);
        assert_eq!(&bytes, b"v4");
        if client.local_addr().unwrap().is_ipv6() {
            if let Ok(peer) = UdpSocket::bind("[::1]:0") {
                peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                client.send_to(b"v6", peer.local_addr().unwrap()).unwrap();
                assert_eq!(peer.recv_from(&mut bytes).unwrap().0, 2);
                assert_eq!(&bytes, b"v6");
            }
        }
    }

    #[test]
    fn rejects_insecure_or_ambiguous_session_urls() {
        for url in [
            "http://localhost/rpc",
            "https://user@localhost/rpc",
            "https://user:password@localhost/rpc",
            "https://localhost/rpc#secret",
            "file:///rpc",
            "invalid",
        ] {
            assert!(super::checked_url(url).is_err(), "{url}");
        }
        for url in ["https://localhost/rpc", "https://[::1]:4433/rpc?version=1"] {
            assert!(super::checked_url(url).is_ok(), "{url}");
        }
    }
}

//! Optional transports for the hosted portable RPC driver, and allocator-free TLS.
//!
//! Browser WebTransport owns its QUIC/TLS implementation and verification clock.
//! Library-managed TLS requires a caller-supplied [`TlsClock`]. No default clock,
//! system trust store or certificate-verification bypass is selected implicitly.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

/// Unix time in seconds used for certificate validity and TLS authentication.
/// Return `None` when time is unavailable; TLS certificate validation then fails closed. Implementations
/// passed to native rustls must also be `Send + Sync`.
pub trait TlsClock {
    fn now(&self) -> Option<u64>;
}

/// An explicit opt-in to the host's system clock. Browser TLS uses the browser's
/// clock instead; WASM applications implementing TLS supply their own clock.
#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;
#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
impl TlsClock for SystemClock {
    fn now(&self) -> Option<u64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|time| time.as_secs())
    }
}

/// Capntproto's TLS byte-stream ALPN. WebTransport uses HTTP/3's `h3` ALPN.
pub const ALPN: &[u8] = b"capntproto-rpc/1";
pub mod driven;
#[cfg(feature = "embassy")]
pub mod embassy;
#[cfg(feature = "embedded-tls")]
pub mod embedded;
pub mod io;
#[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
pub mod monoio;
#[cfg(feature = "rpc")]
pub mod rpc;
#[cfg(feature = "rustls")]
pub mod rustls;
#[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
pub mod tokio;
#[cfg(feature = "webtransport-core")]
pub mod webtransport;

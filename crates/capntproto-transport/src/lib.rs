//! Tokio byte streams backed by rustls's Unbuffered API.
//!
//! Library-managed TLS requires a caller-supplied [`TlsClock`]. No default clock,
//! system trust store or certificate-verification bypass is selected implicitly.
#![forbid(unsafe_code)]

/// Unix time in seconds used for certificate validity and TLS authentication.
/// Return `None` when time is unavailable; TLS certificate validation then fails closed. Implementations
/// passed to native rustls must also be `Send + Sync`.
pub trait TlsClock {
    fn now(&self) -> Option<u64>;
}

/// An explicit opt-in to the host's system clock for certificate verification.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;
impl TlsClock for SystemClock {
    fn now(&self) -> Option<u64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|time| time.as_secs())
    }
}

/// Capntproto's TLS byte-stream ALPN.
pub const ALPN: &[u8] = b"capntproto-rpc/1";
pub mod rustls;
pub mod tokio;

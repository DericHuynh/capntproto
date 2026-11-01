//! Scoped legacy RPC connections over Tokio TCP with rustls TLS.
//! Retain `Connection` while using its bootstrap capabilities.
#[cfg(not(target_arch = "wasm32"))]
mod connection;
#[cfg(not(target_arch = "wasm32"))]
mod io;
#[cfg(not(target_arch = "wasm32"))]
pub use connection::Connection;
#[cfg(not(target_arch = "wasm32"))]
pub mod raw;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod task;

pub use capnp_rpc::twoparty::ConnectionOptions;

/// Shared RPC task completion. Observers do not retain the task or transport.
pub type Completion = futures::future::Shared<capnp::capability::Promise<(), capnp::Error>>;

#[cfg(all(feature = "tls", not(target_arch = "wasm32")))]
pub mod security;
#[cfg(not(target_arch = "wasm32"))]
pub mod tcp;

/// Private RPC ALPN identifier.
pub const ALPN: &[u8] = b"capntproto-rpc/1";

/// Bounds simultaneous unauthenticated sessions and their setup time.
#[derive(Clone, Copy, Debug)]
pub struct AcceptOptions {
    /// Includes TCP setup and TLS authentication.
    pub timeout: std::time::Duration,
    /// Maximum number of concurrent sessions still completing setup.
    pub max_pending: std::num::NonZeroUsize,
}

impl Default for AcceptOptions {
    fn default() -> Self {
        Self {
            timeout: std::time::Duration::from_secs(10),
            max_pending: std::num::NonZeroUsize::new(64).unwrap(),
        }
    }
}
/// Clock-controlled unbuffered TLS byte streams using Tokio IO traits.
#[cfg(feature = "tls")]
pub use capntproto_transport::rustls as tls;
#[cfg(feature = "tls")]
pub use capntproto_transport::{SystemClock, TlsClock};

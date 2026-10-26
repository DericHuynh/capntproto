//! Scoped RPC connections over TCP, QUIC and generic Tokio streams.
//!
//! Retain [`Connection`] while using its bootstrap capabilities. Its drop cancels
//! the driver; [`Connection::shutdown`] completes bounded graceful cleanup. Use
//! [`tcp`] or `quic` (feature `quic`) to choose a transport. TLS secures TCP
//! through `tcp::tls` (feature `tls`); QUIC includes TLS 1.3. The `security`
//! module holds shared certificates when TLS is enabled.
//! Use [`raw`] only when an
//! integration deliberately manages detached task handles.
mod connection;
mod io;
#[cfg(feature = "native")]
pub(crate) mod local_io;
pub use connection::Connection;
#[cfg(feature = "native")]
pub(crate) mod pacing;
#[cfg(feature = "native")]
pub(crate) mod packet_batch;
#[cfg(feature = "native")]
pub(crate) mod packet_mtu;
pub mod raw;
pub(crate) mod task;

// Leave room for RPC framing around a 64 KiB payload while keeping each
// QUIC application bridge and staging allocation bounded.
#[cfg(feature = "native")]
pub(crate) const QUIC_BUFFER_BYTES: usize = 128 * 1024;

/// Shared RPC task completion. Observers do not retain the task or transport.
pub type Completion = futures::future::Shared<capnp::capability::Promise<(), capnp::Error>>;

#[cfg(feature = "quic")]
pub mod quic;
#[cfg(feature = "tls")]
pub mod security;
pub mod tcp;

/// Private RPC ALPN identifier, shared by TCP with TLS and QUIC.
pub const ALPN: &[u8] = b"capntproto-rpc/1";

/// Bounds simultaneous unauthenticated sessions and their setup time.
#[derive(Clone, Copy, Debug)]
pub struct AcceptOptions {
    /// Includes authentication and, for QUIC, arrival of the first RPC stream.
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
/// QUIC wire protocol, selected explicitly without automatic downgrade.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QuicVersion {
    /// RFC 9000. The default for existing peers.
    #[default]
    V1,
    /// RFC 9369 identifier. Unsupported by the selected upstream Quiche release;
    /// configuring this version returns an error without downgrading.
    V2,
}

impl QuicVersion {
    /// QuicVersion field in QUIC long headers.
    pub const fn wire_id(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 0x6b33_43cf,
        }
    }
}

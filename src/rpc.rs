//! Tokio adapters for TCP, TLS, standard QUIC, and Native RPC streams.
mod connection;
pub use connection::Connection;
#[cfg(feature = "native")]
pub(crate) mod pacing;
pub(crate) mod task;

/// Shared RPC task completion. Observers do not retain the task or transport.
pub type Completion = futures::future::Shared<capnp::capability::Promise<(), capnp::Error>>;

#[cfg(feature = "quic")]
pub mod quic;
pub mod tcp;
#[cfg(feature = "tls")]
pub mod tls;
use tokio_util::compat::TokioAsyncReadCompatExt;
/// Low-level task adapter. Dropping the returned join handle detaches the task;
/// prefer [`Connection`] when a scope should own and cancel the driver.
pub fn serve(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    bootstrap: capnp::capability::Client,
) -> tokio::task::JoinHandle<capnp::Result<()>> {
    tokio::task::spawn_local(capnp_rpc::twoparty::TwoPartyClient::with_bootstrap(
        io.compat(),
        Some(bootstrap),
        capnp_rpc::rpc_twoparty_capnp::Side::Server,
    ))
}
/// Low-level task adapter returning an initial typed bootstrap. Prefer
/// [`Connection`] for bounded shutdown and cancellation when the owner is dropped.
pub fn client<T: capnp::capability::FromClientHook>(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
) -> (T, tokio::task::JoinHandle<capnp::Result<()>>) {
    let mut system = capnp_rpc::twoparty::TwoPartyClient::new(io.compat());
    let client = system.bootstrap();
    (client, tokio::task::spawn_local(system))
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

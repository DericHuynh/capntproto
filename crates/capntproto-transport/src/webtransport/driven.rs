//! Explicitly driven native QUIC/HTTP3. Protocol operations never open a socket,
//! spawn a task, or obtain monotonic time. Runtime adapters own that work.
mod api;
mod protocol;
pub use api::*;
pub use protocol::{client_config, server_config, Limits};
pub use rustls_quic as tls;

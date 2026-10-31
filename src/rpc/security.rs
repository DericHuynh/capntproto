//! Certificate material shared by TCP with TLS and QUIC. This is security
//! configuration; transport selection remains in [`super::tcp`] and `quic`.
pub use capntproto_transport::rustls::{certificates_from_pem_file, roots_from_pem_file, Identity};
pub use rustls;
#[cfg(feature = "quic")]
use std::io;

#[cfg(feature = "quic")]
pub(crate) fn invalid_config(
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

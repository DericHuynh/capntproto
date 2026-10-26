//! Certificate material shared by TCP with TLS and QUIC. This is security
//! configuration; transport selection remains in [`super::tcp`] and `quic`.
pub use rustls;
use rustls::pki_types::{
    pem::{Error as PemError, PemObject},
    CertificateDer, PrivateKeyDer,
};
use std::{io, path::Path, time::Duration};

/// A leaf-first certificate chain and its matching private key, in DER form.
pub struct Identity {
    pub certificates: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
}

impl Identity {
    /// Read a PEM certificate chain and PKCS#8, PKCS#1 or SEC1 private key.
    /// Transport configuration checks that the key matches the certificate.
    pub fn from_pem_files(
        certificate: impl AsRef<Path>,
        key: impl AsRef<Path>,
    ) -> io::Result<Self> {
        Ok(Self {
            certificates: certificates_from_pem_file(certificate)?,
            private_key: PrivateKeyDer::from_pem_file(key).map_err(pem_error)?,
        })
    }
}

fn pem_error(error: PemError) -> io::Error {
    match error {
        PemError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other),
    }
}

/// Read all PEM certificates, rejecting an empty chain. Suitable for a QUIC
/// trust configuration or a leaf-first server chain.
pub fn certificates_from_pem_file(
    path: impl AsRef<Path>,
) -> io::Result<Vec<CertificateDer<'static>>> {
    let certificates = CertificateDer::pem_file_iter(path)
        .map_err(pem_error)?
        .map(|certificate| certificate.map_err(pem_error))
        .collect::<io::Result<Vec<_>>>()?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no PEM certificates found",
        ));
    }
    Ok(certificates)
}

/// Read a PEM CA bundle into a rustls trust store. No system roots or
/// certificate-verification bypasses are enabled implicitly.
pub fn roots_from_pem_file(path: impl AsRef<Path>) -> io::Result<rustls::RootCertStore> {
    let mut roots = rustls::RootCertStore::empty();
    for certificate in certificates_from_pem_file(path)? {
        roots.add(certificate).map_err(invalid_config)?;
    }
    Ok(roots)
}

pub(crate) fn invalid_config(
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

pub(crate) async fn deadline<T>(
    timeout: Duration,
    future: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "RPC connection setup timed out"))?
}

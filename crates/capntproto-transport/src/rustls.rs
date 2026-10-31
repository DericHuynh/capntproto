//! TLS 1.3 streams using the existing rustls Unbuffered implementation.
//! Configuration requires explicit trust and an explicit verification clock.
use crate::{TlsClock, ALPN};
pub use rustls_impl as implementation;
use rustls_impl::pki_types::{
    pem::{Error as PemError, PemObject},
    CertificateDer, PrivateKeyDer,
};
pub use rustls_impl::{pki_types, ClientConfig, RootCertStore, ServerConfig};
use std::{fmt, io, path::Path, sync::Arc};
mod stream;
pub use stream::TlsStream;

struct Clock(Arc<dyn TlsClock + Send + Sync>);
impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallerTlsClock")
    }
}
impl rustls_impl::time_provider::TimeProvider for Clock {
    fn current_time(&self) -> Option<pki_types::UnixTime> {
        self.0
            .now()
            .map(|now| pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(now)))
    }
}

/// Trust the supplied roots and optionally present a client identity. TLS 1.3
/// and the RPC ALPN are selected; early data is disabled. The clock supplies
/// certificate-verification time and must return Some during authentication.
pub fn client_config(
    roots: RootCertStore,
    identity: Option<Identity>,
    clock: Arc<dyn TlsClock + Send + Sync>,
) -> Result<Arc<ClientConfig>, rustls_impl::Error> {
    let builder = ClientConfig::builder_with_details(
        Arc::new(rustls_impl::crypto::ring::default_provider()),
        Arc::new(Clock(clock)),
    )
    .with_protocol_versions(&[&rustls_impl::version::TLS13])?
    .with_root_certificates(roots);
    let mut config = match identity {
        Some(identity) => {
            builder.with_client_auth_cert(identity.certificates, identity.private_key)?
        }
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.enable_early_data = false;
    Ok(Arc::new(config))
}

/// Configure a TLS 1.3 server. Some(client_roots) requires client certificates;
/// their validity uses the supplied clock. None enables server authentication
/// without requesting a client certificate.
pub fn server_config(
    identity: Identity,
    client_roots: Option<RootCertStore>,
    clock: Arc<dyn TlsClock + Send + Sync>,
) -> io::Result<Arc<ServerConfig>> {
    let provider = Arc::new(rustls_impl::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_details(provider.clone(), Arc::new(Clock(clock)))
        .with_protocol_versions(&[&rustls_impl::version::TLS13])
        .map_err(invalid_config)?;
    let builder = match client_roots {
        Some(roots) => builder.with_client_cert_verifier(
            rustls_impl::server::WebPkiClientVerifier::builder_with_provider(
                Arc::new(roots),
                provider,
            )
            .build()
            .map_err(invalid_config)?,
        ),
        None => builder.with_no_client_auth(),
    };
    let mut config = builder
        .with_single_cert(identity.certificates, identity.private_key)
        .map_err(invalid_config)?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.max_early_data_size = 0;
    Ok(Arc::new(config))
}

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
pub fn roots_from_pem_file(path: impl AsRef<Path>) -> io::Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates_from_pem_file(path)? {
        roots.add(certificate).map_err(invalid_config)?;
    }
    Ok(roots)
}

fn invalid_config(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

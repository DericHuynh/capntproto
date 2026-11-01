//! Tokio/rustls service configuration with explicit server trust and optional mutual TLS.
use capntproto::{
    capnp_rpc::twoparty::ConnectionOptions,
    rpc::{security, tcp::tls, SystemClock},
};
pub use service::{greeting_capnp, Service};
use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};
use tokio_util::compat::TokioAsyncReadCompatExt;
pub type Result<T> = service::Result<T>;
pub const DEADLINE: Duration = Duration::from_secs(15);
pub struct Pki {
    pub root: Vec<u8>,
    pub server: security::Identity,
    pub client: security::Identity,
    pub root_pem: String,
    pub server_pem: String,
    pub client_pem: String,
}
impl Pki {
    /// Explicit development fixtures; applications provision their own credentials.
    pub fn generate() -> Result<Self> {
        use rcgen::{
            BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
            KeyPair, KeyUsagePurpose,
        };
        let mut ca = CertificateParams::new(Vec::<String>::new())?;
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let ca = CertifiedIssuer::self_signed(ca, KeyPair::generate()?)?;
        let leaf = |client: bool| -> Result<(security::Identity, String)> {
            let mut params = CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()])?;
            params.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(1);
            params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(7);
            params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
            params.extended_key_usages = vec![if client {
                ExtendedKeyUsagePurpose::ClientAuth
            } else {
                ExtendedKeyUsagePurpose::ServerAuth
            }];
            let key = KeyPair::generate()?;
            let cert = params.signed_by(&key, &ca)?;
            Ok((
                security::Identity {
                    certificates: vec![cert.der().clone()],
                    private_key: security::rustls::pki_types::PrivatePkcs8KeyDer::from(
                        key.serialize_der(),
                    )
                    .into(),
                },
                cert.pem(),
            ))
        };
        let (server, server_pem) = leaf(false)?;
        let (client, client_pem) = leaf(true)?;
        Ok(Self {
            root: ca.der().to_vec(),
            server,
            client,
            root_pem: ca.pem(),
            server_pem,
            client_pem,
        })
    }
    pub fn write(&self, directory: &Path) -> Result<()> {
        if directory.exists() && std::fs::read_dir(directory)?.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "credential directory must be empty",
            )
            .into());
        }
        std::fs::create_dir_all(directory)?;
        for (name, text) in [
            ("ca.pem", self.root_pem.as_bytes()),
            ("server.pem", self.server_pem.as_bytes()),
            ("client.pem", self.client_pem.as_bytes()),
        ] {
            write_new(&directory.join(name), text, false)?;
        }
        for (name, identity) in [
            ("server-key.pem", &self.server),
            ("client-key.pem", &self.client),
        ] {
            let key = rcgen::KeyPair::try_from(identity.private_key.secret_der())?;
            write_new(&directory.join(name), key.serialize_pem().as_bytes(), true)?;
        }
        write_new(&directory.join("ca.der"), &self.root, false)
    }
}
fn write_new(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    options.open(path)?.write_all(bytes)?;
    Ok(())
}
pub fn roots(der: &[u8]) -> Result<security::rustls::RootCertStore> {
    let mut roots = security::rustls::RootCertStore::empty();
    roots.add(security::rustls::pki_types::CertificateDer::from(
        der.to_vec(),
    ))?;
    Ok(roots)
}
pub fn copy_identity(identity: &security::Identity) -> security::Identity {
    security::Identity {
        certificates: identity.certificates.clone(),
        private_key: identity.private_key.clone_key(),
    }
}
/// Connect to the authenticated greeting service using Tokio and rustls.
pub async fn client(
    address: SocketAddr,
    ca: &[u8],
    identity: Option<security::Identity>,
    prefix: &str,
    value: &str,
) -> Result<String> {
    let config = tls::client_config(roots(ca)?, identity, Arc::new(SystemClock))?;
    let stream = tls::connect(address, "localhost".try_into()?, config, DEADLINE).await?;
    service::greeting(stream.compat(), prefix, value).await
}
pub async fn server_stream<S: futures::AsyncRead + futures::AsyncWrite + Unpin + 'static>(
    stream: S,
) -> Result<()> {
    let bootstrap =
        capntproto::capnp_rpc::new_client::<greeting_capnp::greeting::Client, _>(Service);
    ConnectionOptions::new()
        .server(stream, bootstrap.client)?
        .await?;
    Ok(())
}

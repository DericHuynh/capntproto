//! One schema with explicitly selected runtime, transport and RPC implementation.
use capntproto::{
    capnp_rpc::twoparty::{ConnectionOptions, RpcDriver},
    rpc::{security, tcp::tls, SystemClock},
};
use clap::ValueEnum;
pub use service::{greeting_capnp, Service};
use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};
use tokio_util::compat::TokioAsyncReadCompatExt;
pub type Result<T> = service::Result<T>;
pub const DEADLINE: Duration = Duration::from_secs(15);
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Driver {
    Legacy,
    Portable,
}
impl Driver {
    pub fn protocol(self) -> capntproto::capnp::Result<RpcDriver> {
        match self {
            Self::Legacy => Ok(RpcDriver::Legacy),
            Self::Portable => service::protocol(),
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Runtime {
    Tokio,
    #[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
    Monoio,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Transport {
    Tcp,
    Tls,
    #[cfg(feature = "webtransport")]
    Webtransport,
}
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
pub async fn tokio_client(
    address: SocketAddr,
    transport: Transport,
    driver: Driver,
    ca: &[u8],
    client_identity: Option<security::Identity>,
    prefix: &str,
    value: &str,
) -> Result<String> {
    match transport {
        Transport::Tcp => {
            service::greeting_with_protocol(
                tokio::net::TcpStream::connect(address).await?.compat(),
                prefix,
                value,
                driver.protocol()?,
            )
            .await
        }
        Transport::Tls => {
            let config = tls::client_config(roots(ca)?, client_identity, Arc::new(SystemClock))?;
            let io = tls::connect(address, "localhost".try_into()?, config, DEADLINE).await?;
            service::greeting_with_protocol(io.compat(), prefix, value, driver.protocol()?).await
        }
        #[cfg(feature = "webtransport")]
        Transport::Webtransport => {
            let client = capntproto::rpc::webtransport::Client::new(
                capntproto::rpc::webtransport::tls::RootCertStore {
                    roots: roots(ca)?.roots,
                },
                Arc::new(SystemClock),
            )?;
            let mut session = client.connect(&format!("https://{address}/rpc")).await?;
            service::greeting_with_protocol(
                session.open_bi().await?,
                prefix,
                value,
                driver.protocol()?,
            )
            .await
        }
    }
}
#[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
pub async fn monoio_client(
    address: SocketAddr,
    transport: Transport,
    driver: Driver,
    ca: &[u8],
    client_identity: Option<security::Identity>,
    prefix: &str,
    value: &str,
) -> Result<String> {
    use capntproto::rpc::{driven::DriverExt, monoio};
    match transport {
        Transport::Tcp => Err("monoio example requires TLS; use tls or webtransport".into()),
        Transport::Tls => {
            let socket = monoio::tls::tcp_connect(address).await?;
            let config = tls::client_config(roots(ca)?, client_identity, Arc::new(SystemClock))?;
            let (mut stream, transport) =
                monoio::tls::client(socket, config, "localhost".try_into()?)?;
            transport
                .run_until(async {
                    stream.ready().await?;
                    service::greeting_with_protocol(stream, prefix, value, driver.protocol()?).await
                })
                .await
                .map_err(|e| std::io::Error::other(format!("transport: {e:?}")))?
        }
        #[cfg(feature = "webtransport")]
        Transport::Webtransport => {
            let client = monoio::webtransport::Client::new(
                capntproto::rpc::webtransport::tls::RootCertStore {
                    roots: roots(ca)?.roots,
                },
                Arc::new(SystemClock),
            )?;
            let mut session = client
                .connect_to(address, &format!("https://{address}/rpc"))
                .await?;
            service::greeting_with_protocol(
                session.open_bi().await?,
                prefix,
                value,
                driver.protocol()?,
            )
            .await
        }
    }
}
pub async fn server_stream<S: futures::AsyncRead + futures::AsyncWrite + Unpin + 'static>(
    stream: S,
    driver: Driver,
) -> Result<()> {
    let service: greeting_capnp::greeting::Client = capntproto::capnp_rpc::new_client(Service);
    ConnectionOptions::new()
        .driver(driver.protocol()?)
        .server(stream, service.client)?
        .await?;
    Ok(())
}

//! Shared benchmark-only TLS policy, also compiled by the independent upstream fixture.
//! No plaintext fallback, public roots, client resumption or early data.
use crate::Result;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream};

pub const DEADLINE: Duration = Duration::from_secs(10);
pub fn directory() -> Result<PathBuf> {
    match std::env::var_os("CAPNTPROTO_BENCH_CERTS") {
        Some(directory) => Ok(directory.into()),
        None => Ok(std::env::current_exe()?
            .parent()
            .ok_or("missing executable directory")?
            .join("certificates")),
    }
}
pub fn read(name: &str) -> Result<Vec<u8>> {
    Ok(std::fs::read(directory()?.join(name))?)
}
pub fn name() -> String {
    std::env::var("CAPNTPROTO_BENCH_SERVER_NAME").unwrap_or_else(|_| "localhost".into())
}
pub fn roots() -> Result<rustls::RootCertStore> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(read("ca.der")?))?;
    Ok(roots)
}
pub fn identity() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    Ok((
        vec![CertificateDer::from(read("server.der")?)],
        PrivateKeyDer::try_from(read("server-key.der")?).map_err(std::io::Error::other)?,
    ))
}
pub fn client_config() -> Result<Arc<rustls::ClientConfig>> {
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .with_root_certificates(roots()?)
    .with_no_client_auth();
    config.resumption = rustls::client::Resumption::disabled();
    config.enable_early_data = false;
    // KJ's public TLS API has no ALPN configuration. Ordinary two-party TLS uses
    // no ALPN on either peer; standard QUIC uses the production RPC ALPN.
    Ok(Arc::new(config))
}
pub fn server_config() -> Result<Arc<rustls::ServerConfig>> {
    let (certificates, key) = identity()?;
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .with_no_client_auth()
    .with_single_cert(certificates, key)?;
    config.send_tls13_tickets = 0;
    Ok(Arc::new(config))
}
pub async fn connect(address: &str) -> Result<TlsStream<TcpStream>> {
    let config = client_config()?;
    tokio::time::timeout(DEADLINE, async {
        let socket = TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        TlsConnector::from(config)
            .connect(ServerName::try_from(name())?, socket)
            .await
            .map(TlsStream::from)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    })
    .await?
}
pub async fn accept(
    socket: TcpStream,
    config: Arc<rustls::ServerConfig>,
) -> Result<TlsStream<TcpStream>> {
    socket.set_nodelay(true)?;
    Ok(
        tokio::time::timeout(DEADLINE, TlsAcceptor::from(config).accept(socket))
            .await??
            .into(),
    )
}
pub fn metadata() -> serde_json::Value {
    serde_json::json!({"version":1,"transport":"tls-tcp","tls":"1.3","authentication":"server-certificate",
        "server_name":name(),"early_data":false,"setup_timed":false,"alpn":null})
}

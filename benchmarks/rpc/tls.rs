//! Buffered TLS adapter for the independent, published upstream RPC baseline.
#[path = "tls_policy.rs"]
mod policy;
use crate::Result;
pub use policy::*;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream};

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

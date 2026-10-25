//! The maintained legacy and portable fixtures share the production Unbuffered adapter.
#[path = "../tls_policy.rs"]
mod policy;
use crate::Result;
use capntproto::rpc::tls::{rustls, TlsStream};
pub use policy::*;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;

pub async fn connect(address: &str) -> Result<TlsStream<TcpStream>> {
    let config = client_config()?;
    tokio::time::timeout(DEADLINE, async {
        let socket = TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        TlsStream::connect(socket, config, ServerName::try_from(name())?)
            .await
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    })
    .await?
}
pub async fn accept(
    socket: TcpStream,
    config: Arc<rustls::ServerConfig>,
) -> Result<TlsStream<TcpStream>> {
    socket.set_nodelay(true)?;
    Ok(tokio::time::timeout(DEADLINE, TlsStream::accept(socket, config)).await??)
}

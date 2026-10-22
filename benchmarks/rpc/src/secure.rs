//! One encrypted transport selection shared by every RPC edge, including service hops.
use crate::{benchmark_tls as tls, Result};
use capnp_rpc::rpc_twoparty_capnp::Side;
use capntproto::rpc::quic;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};

pub type Network = Box<dyn capnp_rpc::VatNetwork<Side>>;
pub enum Stream {
    Tls(Box<tokio_rustls::TlsStream<TcpStream>>),
    Quic(quic::Stream),
}
impl Stream {
    pub fn network(self, side: Side) -> Network {
        match self {
            Self::Tls(stream) => Box::new(capntproto::rpc::tls::network(
                *stream,
                side,
                Default::default(),
            )),
            Self::Quic(stream) => Box::new(quic::network(stream, side, Default::default())),
        }
    }
}
pub fn transport() -> Result<&'static str> {
    match std::env::var("CAPNTPROTO_BENCH_TRANSPORT").as_deref() {
        Ok("tls") | Err(std::env::VarError::NotPresent) => Ok("tls-tcp"),
        Ok("quic") => Ok("quic-v1"),
        _ => Err("CAPNTPROTO_BENCH_TRANSPORT must be tls or quic (no plaintext fallback)".into()),
    }
}
pub fn metadata() -> Result<serde_json::Value> {
    let mut metadata = tls::metadata();
    metadata["transport"] = transport()?.into();
    if transport()? == "quic-v1" {
        metadata["alpn"] = "capntproto-rpc/1".into();
    }
    Ok(metadata)
}
fn identity() -> Result<capntproto::rpc::tls::Identity> {
    let (certificates, private_key) = tls::identity()?;
    Ok(capntproto::rpc::tls::Identity {
        certificates,
        private_key,
    })
}
pub enum Listener {
    Tls(TcpListener, Arc<tokio_rustls::rustls::ServerConfig>),
    Quic(quic::Endpoint),
}
impl Listener {
    pub async fn bind(address: &str) -> Result<Self> {
        match transport()? {
            "tls-tcp" => Ok(Self::Tls(
                TcpListener::bind(address).await?,
                tls::server_config()?,
            )),
            _ => Ok(Self::Quic(quic::Endpoint::server(
                quic::server_config(identity()?, None)?,
                address.parse()?,
            )?)),
        }
    }
    pub fn local_addr(&self) -> Result<std::net::SocketAddr> {
        Ok(match self {
            Self::Tls(listener, _) => listener.local_addr()?,
            Self::Quic(endpoint) => endpoint.local_addr()?,
        })
    }
    pub async fn accept(&self) -> Result<Stream> {
        match self {
            Self::Tls(listener, config) => Ok(Stream::Tls(Box::new(
                tls::accept(listener.accept().await?.0, config.clone()).await?,
            ))),
            Self::Quic(endpoint) => Ok(Stream::Quic(
                quic::accept(
                    endpoint.accept().await.ok_or("QUIC listener closed")?,
                    tls::DEADLINE,
                )
                .await?,
            )),
        }
    }
}
pub async fn connect(address: &str) -> Result<Stream> {
    if transport()? == "tls-tcp" {
        return Ok(Stream::Tls(Box::new(tls::connect(address).await?)));
    }
    let address: std::net::SocketAddr = address.parse()?;
    let bind = if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let mut endpoint = quic::Endpoint::client(bind.parse()?)?;
    endpoint.set_default_client_config(quic::client_config(
        vec![tls::read("ca.der")?.into()],
        None,
    )?);
    Ok(Stream::Quic(
        quic::connect(&endpoint, address, &tls::name(), tls::DEADLINE).await?,
    ))
}

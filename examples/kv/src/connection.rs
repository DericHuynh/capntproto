//! Connection setup shared by the command-line clients, separate from service code.
use capntproto::rpc::security;
use capntproto::{
    capnp_rpc::rpc_twoparty_capnp::Side,
    rpc::{self, tcp, tcp::tls},
};
use clap::{Args, ValueEnum};
#[cfg(feature = "quic")]
use std::io;
use std::{path::PathBuf, time::Duration};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Transport {
    #[default]
    Tcp,
    #[cfg(feature = "quic")]
    Quic,
}

#[derive(Args)]
pub struct ClientArgs {
    #[arg(long, default_value = "127.0.0.1:4000")]
    pub server: String,
    #[arg(long, value_enum, default_value_t)]
    pub transport: Transport,
    /// PEM CA certificates: enable TLS on TCP; required by QUIC.
    #[arg(long)]
    pub ca: Option<PathBuf>,
    /// Certificate DNS name, independent of the network address.
    #[arg(long, default_value = "localhost")]
    pub server_name: String,
}

impl ClientArgs {
    pub async fn connect(&self) -> Result<rpc::Connection> {
        let driver = match self.transport {
            Transport::Tcp => match &self.ca {
                Some(ca) => {
                    let roots = security::roots_from_pem_file(ca)?;
                    let stream = tls::connect(
                        &self.server,
                        self.server_name.clone().try_into()?,
                        tls::client_config(roots, None)?,
                        TIMEOUT,
                    )
                    .await?;
                    tls::driver(stream, None, Side::Client, Default::default())
                }
                None => {
                    let stream = tcp::connect(&self.server, TIMEOUT).await?;
                    tcp::driver(stream, None, Side::Client, Default::default())
                }
            },
            #[cfg(feature = "quic")]
            Transport::Quic => {
                let ca = self
                    .ca
                    .as_ref()
                    .ok_or_else(|| io::Error::other("QUIC requires --ca"))?;
                let address = tokio::net::lookup_host(&self.server)
                    .await?
                    .next()
                    .ok_or_else(|| io::Error::other("server address did not resolve"))?;
                let bind = if address.is_ipv4() {
                    "0.0.0.0:0"
                } else {
                    "[::]:0"
                };
                let mut endpoint = rpc::quic::Endpoint::client(bind.parse()?)?;
                endpoint.set_default_client_config(rpc::quic::client_config(
                    security::certificates_from_pem_file(ca)?,
                    None,
                )?);
                let stream =
                    rpc::quic::connect(&endpoint, address, &self.server_name, TIMEOUT).await?;
                rpc::quic::driver(stream, None, Side::Client, Default::default())
            }
        };
        Ok(rpc::Connection::spawn(driver))
    }
}

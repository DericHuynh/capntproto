//! Connection setup shared by the command-line clients, separate from service code.
use capntproto::rpc::security;
use capntproto::{
    capnp_rpc::rpc_twoparty_capnp::Side,
    rpc::{self, tcp, tcp::tls},
};
use clap::{Args, ValueEnum};
#[cfg(feature = "quic")]
use futures::{stream::FuturesUnordered, StreamExt};
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
                let stream = tokio::time::timeout(TIMEOUT, self.connect_quic())
                    .await
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "QUIC connection setup timed out")
                    })??;
                rpc::quic::driver(stream, None, Side::Client, Default::default())
            }
        };
        Ok(rpc::Connection::spawn(driver))
    }

    #[cfg(feature = "quic")]
    async fn connect_quic(&self) -> io::Result<rpc::quic::Stream> {
        let ca = self
            .ca
            .as_ref()
            .ok_or_else(|| io::Error::other("QUIC requires --ca"))?;
        let roots = security::certificates_from_pem_file(ca)?;
        connect_quic_addresses(
            tokio::net::lookup_host(&self.server).await?,
            roots,
            &self.server_name,
        )
        .await
    }
}

#[cfg(feature = "quic")]
async fn connect_quic_addresses(
    addresses: impl IntoIterator<Item = std::net::SocketAddr>,
    roots: Vec<security::rustls::pki_types::CertificateDer<'static>>,
    server_name: &str,
) -> io::Result<rpc::quic::Stream> {
    let mut pending = FuturesUnordered::new();
    for (index, address) in addresses.into_iter().enumerate() {
        let roots = &roots;
        pending.push(async move {
            // Stagger attempts so a silent IPv6 or IPv4 destination cannot
            // consume the whole setup deadline before trying other addresses.
            if index != 0 {
                tokio::time::sleep(Duration::from_millis(250).saturating_mul(index as u32)).await;
            }
            let bind = if address.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            };
            let mut endpoint = rpc::quic::Endpoint::client(bind.parse().unwrap())?;
            endpoint.set_default_client_config(rpc::quic::client_config(roots.clone(), None)?);
            rpc::quic::connect(&endpoint, address, server_name, TIMEOUT).await
        });
    }
    let mut last_error = io::Error::other("server address did not resolve");
    while let Some(result) = pending.next().await {
        match result {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(all(test, feature = "quic"))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test(flavor = "current_thread")]
    async fn silent_first_address_does_not_block_a_reachable_fallback() {
        tokio::task::LocalSet::new()
            .run_until(async {
                tokio::time::timeout(Duration::from_secs(2), async {
                    let cert =
                        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
                    let roots = vec![cert.cert.der().clone()];
                    let server = rpc::quic::Endpoint::server(
                        rpc::quic::server_config(
                            security::Identity {
                                certificates: roots.clone(),
                                private_key: security::rustls::pki_types::PrivatePkcs8KeyDer::from(
                                    cert.signing_key.serialize_der(),
                                )
                                .into(),
                            },
                            None,
                        )
                        .unwrap(),
                        "127.0.0.1:0".parse().unwrap(),
                    )
                    .unwrap();
                    // This socket accepts UDP but never replies to the handshake.
                    // There is no reliance on resolver order or ICMP delivery.
                    let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                    let addresses = [silent.local_addr().unwrap(), server.local_addr().unwrap()];
                    let connect = async {
                        let mut stream = connect_quic_addresses(addresses, roots, "localhost")
                            .await
                            .unwrap();
                        stream.write_all(b"f").await.unwrap();
                        stream.shutdown().await.unwrap();
                        stream
                    };
                    let accept = async {
                        let mut stream = rpc::quic::accept(server.accept().await.unwrap(), TIMEOUT)
                            .await
                            .unwrap();
                        assert_eq!(stream.read_u8().await.unwrap(), b'f');
                        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
                        stream.shutdown().await.unwrap();
                        stream
                    };
                    let (mut client, peer) = tokio::join!(connect, accept);
                    assert_eq!(client.read(&mut [0; 1]).await.unwrap(), 0);
                    drop((client, peer));
                    tokio::time::timeout(TIMEOUT, server.wait_idle())
                        .await
                        .unwrap();
                })
                .await
                .expect("silent DNS candidate blocked a reachable address");
            })
            .await;
    }
}

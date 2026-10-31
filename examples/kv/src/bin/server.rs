//! A shared, in-memory key/value service. Ctrl-C stops admission and all sessions.
use capntproto_kv_example::{connection as common, key_value_capnp};
#[path = "../service.rs"]
mod service;
use capntproto::{
    capnp_rpc,
    rpc::{tcp, tcp::tls},
};
use clap::Parser;
use common::{Result, Transport};
use std::{io, path::PathBuf};

#[derive(Parser)]
#[command(about = "Serve a capability-based key/value store over TCP or QUIC")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4000")]
    listen: String,
    #[arg(long, value_enum, default_value_t)]
    transport: Transport,
    /// PEM server certificate chain: enable TLS on TCP; required by QUIC.
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,
    /// PEM private key matching the server certificate.
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new().run_until(run(args)).await
}

async fn run(args: Args) -> Result<()> {
    let bootstrap: key_value_capnp::directory::Client =
        capnp_rpc::new_client(service::Directory::default());
    let (server, driver) = capnp_rpc::twoparty::TwoPartyServer::new(bootstrap.client);
    let identity = match (&args.cert, &args.key) {
        (Some(cert), Some(key)) => Some(capntproto::rpc::security::Identity::from_pem_files(
            cert, key,
        )?),
        (None, None) => None,
        _ => return Err(io::Error::other("--cert and --key must be provided together").into()),
    };
    let listen = async {
        match args.transport {
            Transport::Tcp => {
                let listener = tokio::net::TcpListener::bind(&args.listen).await?;
                println!(
                    "Listening on tcp://{} ({})",
                    listener.local_addr()?,
                    if identity.is_some() {
                        "TLS 1.3"
                    } else {
                        "plaintext"
                    }
                );
                match identity {
                    Some(identity) => {
                        tls::listen(
                            &server,
                            &listener,
                            tls::server_config(
                                identity,
                                None,
                                std::sync::Arc::new(capntproto::rpc::SystemClock),
                            )?,
                            Default::default(),
                            Default::default(),
                            |peer, error| eprintln!("Rejected {peer}: {error}"),
                        )
                        .await?
                    }
                    None => tcp::listen(&server, &listener, Default::default()).await?,
                }
            }
            #[cfg(feature = "quic")]
            Transport::Quic => {
                use capntproto::rpc::quic;
                let identity =
                    identity.ok_or_else(|| io::Error::other("QUIC requires --cert and --key"))?;
                let address = tokio::net::lookup_host(&args.listen)
                    .await?
                    .next()
                    .ok_or_else(|| io::Error::other("listen address did not resolve"))?;
                let endpoint =
                    quic::Endpoint::server(quic::server_config(identity, None)?, address)?;
                println!("Listening on quic://{} (TLS 1.3)", endpoint.local_addr()?);
                quic::listen(
                    &server,
                    &endpoint,
                    Default::default(),
                    Default::default(),
                    |peer, error| eprintln!("Rejected {peer}: {error}"),
                )
                .await?;
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::select! {
        result = driver => result?,
        result = listen => result?,
        result = tokio::signal::ctrl_c() => result?,
    }
    Ok(())
}

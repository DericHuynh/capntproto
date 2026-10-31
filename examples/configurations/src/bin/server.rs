use capntproto::{
    capnp_rpc::twoparty::TwoPartyServer,
    rpc::{security::Identity, tcp, tcp::tls, SystemClock},
};
use capntproto_configuration_example::{greeting_capnp, Driver, Result, Service, Transport};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4433")]
    listen: SocketAddr,
    #[arg(long, value_enum, default_value = "tls")]
    transport: Transport,
    #[arg(long, value_enum, default_value = "portable")]
    driver: Driver,
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
    #[arg(long)]
    client_ca: Option<PathBuf>,
    #[arg(long, default_value = "http://localhost:8080")]
    origin: String,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new().run_until(run(args)).await
}
async fn run(args: Args) -> Result<()> {
    let bootstrap: greeting_capnp::greeting::Client = capntproto::capnp_rpc::new_client(Service);
    let (server, driver) =
        TwoPartyServer::with_driver(bootstrap.client, args.driver.protocol()?, |error| {
            eprintln!("RPC: {error}")
        });
    let identity = match (&args.cert, &args.key) {
        (Some(cert), Some(key)) => Some(Identity::from_pem_files(cert, key)?),
        _ => None,
    };
    let client_ca = args
        .client_ca
        .map(capntproto::rpc::security::roots_from_pem_file)
        .transpose()?;
    let listener = async {
        match args.transport {
            Transport::Tcp => {
                if identity.is_some() || client_ca.is_some() {
                    return Err(
                        "TCP plaintext does not use TLS credentials; select --transport tls".into(),
                    );
                }
                let listener = tokio::net::TcpListener::bind(args.listen).await?;
                println!(
                    "{}",
                    serde_json::json!({"address":listener.local_addr()?.to_string()})
                );
                tcp::listen(&server, &listener, Default::default()).await?;
            }
            Transport::Tls => {
                let config = tls::server_config(
                    identity.ok_or("TLS requires --cert and --key")?,
                    client_ca,
                    Arc::new(SystemClock),
                )?;
                let listener = tokio::net::TcpListener::bind(args.listen).await?;
                println!(
                    "{}",
                    serde_json::json!({"address":listener.local_addr()?.to_string()})
                );
                tls::listen(
                    &server,
                    &listener,
                    config,
                    Default::default(),
                    Default::default(),
                    |_, error| eprintln!("TLS: {error}"),
                )
                .await?;
            }
            #[cfg(feature = "webtransport")]
            Transport::Webtransport => {
                if client_ca.is_some() {
                    return Err(
                        "--client-ca is for TLS/TCP; WebTransport uses application authentication"
                            .into(),
                    );
                }
                let identity = identity.ok_or("WebTransport requires --cert and --key")?;
                let endpoint = capntproto::rpc::webtransport::Server::bind(
                    args.listen,
                    identity.certificates,
                    identity.private_key,
                    Arc::new(SystemClock),
                )?;
                println!(
                    "{}",
                    serde_json::json!({"address":endpoint.local_addr()?.to_string()})
                );
                capntproto::rpc::webtransport::listen(
                    &server,
                    &endpoint,
                    Default::default(),
                    std::time::Duration::from_secs(5),
                    16.try_into().unwrap(),
                    |request| {
                        request.path() == "/rpc"
                            && request.origin().is_none_or(|origin| origin == args.origin)
                    },
                    |error| eprintln!("WebTransport: {error}"),
                )
                .await?;
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    futures::pin_mut!(listener, driver);
    tokio::select! {result=&mut listener=>result?,result=&mut driver=>result?,result=tokio::signal::ctrl_c()=>result?}
    Ok(())
}

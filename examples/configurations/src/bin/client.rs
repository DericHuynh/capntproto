use capntproto_configuration_example::{self as example, Driver, Result, Runtime, Transport};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4433")]
    address: SocketAddr,
    #[arg(long, value_enum, default_value = "tls")]
    transport: Transport,
    #[arg(long, value_enum, default_value = "tokio")]
    runtime: Runtime,
    #[arg(long, value_enum, default_value = "portable")]
    driver: Driver,
    #[arg(long)]
    ca: Option<PathBuf>,
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
    #[arg(long, default_value = "Hello, ")]
    prefix: String,
    #[arg(default_value = "world")]
    value: String,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let certificates = args
        .ca
        .map(capntproto::rpc::security::certificates_from_pem_file)
        .transpose()?;
    let ca = match certificates.as_ref().and_then(|certs| certs.first()) {
        Some(ca) => ca.as_ref(),
        None if matches!(args.transport, Transport::Tcp) => &[],
        None => return Err("TLS and WebTransport require a nonempty --ca file".into()),
    };
    let identity = match (&args.cert, &args.key) {
        (Some(cert), Some(key)) => Some(capntproto::rpc::security::Identity::from_pem_files(
            cert, key,
        )?),
        _ => None,
    };
    if matches!(args.transport, Transport::Tcp) && identity.is_some() {
        return Err("client certificates require --transport tls".into());
    }
    let result = match args.runtime {
        Runtime::Tokio => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(tokio::task::LocalSet::new().run_until(async {
                tokio::time::timeout(
                    example::DEADLINE,
                    example::tokio_client(
                        args.address,
                        args.transport,
                        args.driver,
                        ca,
                        identity,
                        &args.prefix,
                        &args.value,
                    ),
                )
                .await?
            })),
        #[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
        Runtime::Monoio => capntproto::rpc::monoio::runtime::RuntimeBuilder::<
            capntproto::rpc::monoio::runtime::LegacyDriver,
        >::new()
        .enable_timer()
        .build()?
        .block_on(async {
            capntproto::rpc::monoio::runtime::time::timeout(
                example::DEADLINE,
                example::monoio_client(
                    args.address,
                    args.transport,
                    args.driver,
                    ca,
                    identity,
                    &args.prefix,
                    &args.value,
                ),
            )
            .await?
        }),
    }?;
    println!("{result}");
    Ok(())
}

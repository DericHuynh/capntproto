use capntproto_configuration_example::{self as example, Result};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4433")]
    address: SocketAddr,
    #[arg(long)]
    ca: PathBuf,
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
    #[arg(long, default_value = "Hello, ")]
    prefix: String,
    #[arg(default_value = "world")]
    value: String,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let roots = capntproto::rpc::security::certificates_from_pem_file(args.ca)?;
    let ca = roots.first().ok_or("CA file is empty")?;
    let identity = match (args.cert, args.key) {
        (Some(cert), Some(key)) => Some(capntproto::rpc::security::Identity::from_pem_files(
            cert, key,
        )?),
        _ => None,
    };
    let reply = tokio::task::LocalSet::new()
        .run_until(example::client(
            args.address,
            ca,
            identity,
            &args.prefix,
            &args.value,
        ))
        .await?;
    println!("{reply}");
    Ok(())
}

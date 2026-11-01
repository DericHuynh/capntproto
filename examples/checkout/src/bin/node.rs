use capntproto_checkout_example::{serve, Config, NodeConfig, Result, Role};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    role: Role,
    #[arg(long, default_value = "127.0.0.1:4433")]
    listen: SocketAddr,
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    cert: PathBuf,
    #[arg(long)]
    key: PathBuf,
    #[arg(long)]
    ca: PathBuf,
    #[arg(long)]
    client_cert: Option<PathBuf>,
    #[arg(long)]
    client_key: Option<PathBuf>,
    #[arg(long)]
    inventory: Option<SocketAddr>,
    #[arg(long)]
    payments: Option<SocketAddr>,
    /// Test failpoints only. A normal application leaves this unset.
    #[arg(long)]
    faults: Option<PathBuf>,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config: Config = serde_json::from_slice(&std::fs::read(&args.config)?)?;
    tokio::task::LocalSet::new()
        .run_until(serve(NodeConfig {
            role: args.role,
            listen: args.listen,
            db: args.db,
            config,
            cert: args.cert,
            key: args.key,
            ca: args.ca,
            client_cert: args.client_cert,
            client_key: args.client_key,
            inventory: args.inventory,
            payments: args.payments,
            faults: args.faults,
        }))
        .await
}

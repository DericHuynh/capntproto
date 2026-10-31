use capntproto_checkout_example::{Client, Driver, Result, Wire};
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4433")]
    address: SocketAddr,
    #[arg(long)]
    ca: PathBuf,
    #[arg(long)]
    account: String,
    /// Provision a token via the environment instead of putting it in shell history.
    #[arg(long, env = "CHECKOUT_TOKEN", hide_env_values = true)]
    token: String,
    #[arg(long, value_enum, default_value = "portable")]
    driver: Driver,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Buy { key: String, quantity: u32 },
    Cancel { key: String },
    View { key: String },
    List,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                let client = Client::connect(
                    &Wire::new(args.address, args.driver, &args.ca, None)?,
                    &args.account,
                    &args.token,
                )
                .await?;
                let value = match args.command {
                    Command::Buy { key, quantity } => {
                        serde_json::to_value(client.buy(&key, quantity).await?)?
                    }
                    Command::Cancel { key } => serde_json::to_value(client.cancel(&key).await?)?,
                    Command::View { key } => serde_json::to_value(client.view(&key).await?)?,
                    Command::List => serde_json::to_value(client.list().await?)?,
                };
                println!("{}", serde_json::to_string_pretty(&value)?);
                client.close().await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            })
            .await?
        })
        .await
}

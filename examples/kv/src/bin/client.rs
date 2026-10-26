//! Independent CLI client. Each invocation connects to the running service.
use capntproto_kv_example::{connection as common, key_value_capnp};
use clap::{Parser, Subcommand};
use common::Result;

#[derive(Parser)]
#[command(about = "Read and write the remote key/value service")]
struct Args {
    #[command(flatten)]
    connection: common::ClientArgs,
    /// Slash-separated directory path; each component scopes a capability.
    #[arg(long, default_value = "")]
    path: String,
    #[command(subcommand)]
    operation: Operation,
}

#[derive(Subcommand)]
enum Operation {
    Put { key: String, value: String },
    Get { key: String },
    Delete { key: String },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new()
        .run_until(async {
            let connection = args.connection.connect().await?;
            tokio::time::timeout(common::TIMEOUT, execute(&connection, args)).await??;
            connection.shutdown(common::TIMEOUT).await?;
            Ok(())
        })
        .await
}

async fn execute(connection: &capntproto::rpc::Connection, args: Args) -> Result<()> {
    let mut directory: key_value_capnp::directory::Client = connection.bootstrap();
    for name in args.path.split('/').filter(|name| !name.is_empty()) {
        let mut request = directory.open_request();
        request.get().set_name(name);
        directory = request.send().promise.await?.get()?.get_directory()?;
    }
    match args.operation {
        Operation::Put { key, value } => {
            let mut request = directory.put_request();
            request.get().set_key(&key);
            request.get().set_value(value.as_bytes());
            request.send().promise.await?;
            println!("stored {key}");
        }
        Operation::Get { key } => {
            let mut request = directory.get_request();
            request.get().set_key(&key);
            let response = request.send().promise.await?;
            let result = response.get()?;
            if !result.get_found() {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, key).into());
            }
            println!("{}", String::from_utf8(result.get_value()?.to_vec())?);
        }
        Operation::Delete { key } => {
            let mut request = directory.delete_request();
            request.get().set_key(&key);
            println!(
                "deleted={}",
                request.send().promise.await?.get()?.get_deleted()
            );
        }
    }
    Ok(())
}

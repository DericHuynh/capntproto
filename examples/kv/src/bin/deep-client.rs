//! Concurrent, pipelined calls against the independently running key/value server.
use capntproto::capnp;
use capntproto_kv_example::{connection as common, key_value_capnp};
use clap::Parser;
use common::Result;

#[derive(Parser)]
#[command(about = "Exercise deep concurrent capability access on the running service")]
struct Args {
    #[command(flatten)]
    connection: common::ClientArgs,
    #[arg(long, default_value_t = 32, value_parser = clap::value_parser!(u32).range(1..=128))]
    depth: u32,
    #[arg(long, default_value_t = 32, value_parser = clap::value_parser!(u32).range(1..=128))]
    concurrent: u32,
}

async fn roundtrip(
    mut directory: key_value_capnp::directory::Client,
    depth: u32,
    id: u32,
) -> capnp::Result<()> {
    let mut parents = Vec::with_capacity(depth as usize);
    for level in 0..depth {
        let mut request = directory.open_request();
        request.get().set_name(format!("level-{level}"));
        let call = request.send();
        directory = call.pipeline.get_directory();
        parents.push(call.promise);
    }
    let key = format!("client-{id}");
    let value = format!("value-{id}");
    let mut put = directory.put_request();
    put.get().set_key(&key);
    put.get().set_value(value.as_bytes());
    put.send().promise.await?;
    let mut get = directory.get_request();
    get.get().set_key(&key);
    let response = get.send().promise.await?;
    let result = response.get()?;
    assert!(result.get_found());
    assert_eq!(result.get_value()?, value.as_bytes());
    for parent in parents {
        parent.await?;
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new()
        .run_until(async {
            let connection = args.connection.connect().await?;
            let directory: key_value_capnp::directory::Client = connection.bootstrap();
            tokio::time::timeout(
                common::TIMEOUT,
                futures::future::try_join_all(
                    (0..args.concurrent).map(|id| roundtrip(directory.clone(), args.depth, id)),
                ),
            )
            .await??;
            println!(
                "Verified {} concurrent writes/reads through {} directory levels",
                args.concurrent, args.depth
            );
            drop(directory);
            connection.shutdown(common::TIMEOUT).await?;
            Ok(())
        })
        .await
}

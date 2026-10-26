//! One request over a scoped, in-memory RPC connection.
use capntproto::capnp;
use std::time::Duration;

pub mod rpc_examples_capnp {
    include!(concat!(env!("OUT_DIR"), "/rpc_examples_capnp.rs"));
}
#[path = "support/rpc_service.rs"]
mod service;

#[tokio::main(flavor = "current_thread")]
async fn main() -> capnp::Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (server, connection, node) = service::connect();
            let response = node.value_request().send().promise.await?;
            assert_eq!(response.get()?.get_depth(), 0);
            println!("Remote node depth: {}", response.get()?.get_depth());
            drop(response);
            drop(node);
            connection.shutdown(Duration::from_secs(1)).await?;
            drop(server);
            Ok(())
        })
        .await
}

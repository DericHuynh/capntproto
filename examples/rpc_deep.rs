//! Thirty-two concurrent calls through 32 levels of promise-pipelined capabilities.
use capntproto::capnp;
use rpc_examples_capnp::node;
use std::time::Duration;

pub mod rpc_examples_capnp {
    include!(concat!(env!("OUT_DIR"), "/rpc_examples_capnp.rs"));
}
#[path = "support/rpc_service.rs"]
mod service;

async fn read_deep(mut node: node::Client, depth: u32) -> capnp::Result<u32> {
    let mut parents = Vec::with_capacity(depth as usize);
    for _ in 0..depth {
        let call = node.child_request().send();
        // No await: the peer can route the next call through this promise.
        node = call.pipeline.get_node();
        parents.push(call.promise);
    }
    let response = node.value_request().send().promise.await?;
    let value = response.get()?.get_depth();
    // Retain parent interests through the dependent call and observe errors.
    for parent in parents {
        parent.await?;
    }
    Ok(value)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> capnp::Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            const DEPTH: u32 = 32;
            const CONCURRENT: usize = 32;
            let (server, connection, node) = service::connect();
            let values = futures::future::try_join_all(
                (0..CONCURRENT).map(|_| read_deep(node.clone(), DEPTH)),
            )
            .await?;
            assert!(values.iter().all(|&value| value == DEPTH));
            println!("Completed {CONCURRENT} concurrent pipelines of depth {DEPTH}");
            drop(node);
            connection.shutdown(Duration::from_secs(1)).await?;
            drop(server);
            Ok(())
        })
        .await
}

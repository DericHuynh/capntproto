//! Matched application workflows. Setup is excluded; validation and response
//! destruction are included. A sample is an entire workflow (a batch for fanout).
use crate::{echo_capnp::workflows, Result};
use serde_json::Value;
use tokio::time::timeout;

#[path = "../capability-workflows.rs"]
mod capability;
pub use capability::measure;
use capability::Service;
fn implementation() -> Result<&'static str> {
    crate::tcp_driver()
}

mod application;
pub mod comparison;
mod grpc;
#[path = "../workflow-support.rs"]
pub mod support;
mod websocket;
pub use support::VERSION;
// Includes protocol metadata and segment/frame headers around the 4 MiB payload.
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

fn portable_network(
    network: crate::secure::Network,
    bootstrap: Option<capnp::capability::Client>,
    side: capnp_rpc::rpc_twoparty_capnp::Side,
) -> Result<capnp_rpc::portable::RpcSystem<capnp_rpc::rpc_twoparty_capnp::Side>> {
    crate::portable_network_bounded(
        network,
        bootstrap,
        side,
        MAX_MESSAGE_BYTES,
        32 * 1024 * 1024,
    )
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("../workflows.json")).expect("workflow catalog")
}
pub async fn serve(address: &str) -> Result<()> {
    let listener = crate::secure::Listener::bind(address).await?;
    crate::ready(listener.local_addr()?, None, "tcp")?;
    loop {
        let network = listener
            .accept()
            .await?
            .network(capnp_rpc::rpc_twoparty_capnp::Side::Server);
        let bootstrap: workflows::Client = capnp_rpc::new_client(Service);
        tokio::task::spawn_local(async move {
            let result = if crate::tcp_driver()? == "portable" {
                portable_network(
                    network,
                    Some(bootstrap.client),
                    capnp_rpc::rpc_twoparty_capnp::Side::Server,
                )?
                .run()
                .await
            } else {
                capnp_rpc::RpcSystem::new(network, Some(bootstrap.client)).await
            };
            if let Err(error) = result {
                eprintln!("workflow connection closed: {error}");
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        });
    }
}
async fn connect(
    address: &str,
) -> Result<(
    workflows::Client,
    Option<capnp_rpc::portable::RpcSystem<capnp_rpc::rpc_twoparty_capnp::Side>>,
)> {
    let side = capnp_rpc::rpc_twoparty_capnp::Side::Client;
    let network = crate::secure::connect(address).await?.network(side);
    if crate::tcp_driver()? == "portable" {
        let mut driver = portable_network(network, None, side)?;
        let client = driver.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);
        Ok((client, Some(driver)))
    } else {
        let mut driver = capnp_rpc::RpcSystem::new(network, None);
        let client = driver.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);
        tokio::task::spawn_local(driver);
        Ok((client, None))
    }
}

//! Controlled runtime comparison using the production portable engine and the
//! same generated services, workflow operations, validation and deadlines.
#![forbid(unsafe_code)]
// Instantiate the identical fixture with each host's deadline implementation.
#![allow(clippy::duplicate_mod)]

use capnp_rpc::{portable, rpc_twoparty_capnp::Side, VatNetwork};
use std::io::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[allow(dead_code)]
mod echo_capnp {
    include!(concat!(env!("OUT_DIR"), "/echo_capnp.rs"));
}
#[cfg(feature = "compio-experiment")]
mod compio_host;
mod tokio_host;

#[path = "../../rpc/workflow-support.rs"]
#[allow(dead_code)]
mod support;

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("../../rpc/workflows.json")).expect("workflow catalog")
}
fn ready(address: std::net::SocketAddr) -> Result<()> {
    println!(
        "{}",
        serde_json::json!({"address":address.to_string(),"public":null})
    );
    std::io::stdout().flush()?;
    Ok(())
}
fn system(
    network: Box<dyn VatNetwork<Side>>,
    bootstrap: Option<capnp::capability::Client>,
) -> Result<portable::RpcSystem<Side>> {
    // Identical admission and frame limits to rpc/src/workflows.rs.
    let options = portable::Options::bounded(
        1.try_into()?,
        256.try_into()?,
        (8 * 1024 * 1024).try_into()?,
        (32 * 1024 * 1024).try_into()?,
    )?;
    Ok(portable::RpcSystem::new(network, bootstrap, options)?)
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match std::env::var("CAPNTPROTO_BENCH_RUNTIME")
        .as_deref()
        .unwrap_or("tokio")
    {
        "tokio" => tokio_host::run(args),
        #[cfg(feature = "compio-experiment")]
        "compio-poll" | "compio-completion" => compio_host::run(args),
        #[cfg(not(feature = "compio-experiment"))]
        "compio-poll" | "compio-completion" => {
            Err("Compio is disabled; rebuild with --features compio-experiment to reproduce the runtime comparison".into())
        }
        _ => Err("CAPNTPROTO_BENCH_RUNTIME must be tokio, compio-poll or compio-completion".into()),
    }
}

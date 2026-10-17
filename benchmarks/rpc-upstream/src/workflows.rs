use crate::{echo_capnp::workflows, Result};
use capnp_rpc::rpc_twoparty_capnp::Side;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

#[path = "../../rpc/capability-workflows.rs"]
mod capability;
pub use capability::measure;
#[path = "../../rpc/workflow-support.rs"]
mod support;
use support::VERSION;

fn implementation() -> Result<&'static str> {
    Ok("capnp-rust")
}
fn cases() -> Vec<serde_json::Value> {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../rpc/workflows.json")).expect("workflow catalog");
    cases
        .into_iter()
        .filter(|case| support::supported("capnp-rust", case["name"].as_str().unwrap()))
        .collect()
}
pub async fn serve(address: &str) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    crate::ready(listener.local_addr()?)?;
    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let bootstrap: workflows::Client = capnp_rpc::new_client(capability::Service);
        tokio::task::spawn_local(async move {
            if let Err(error) = crate::rpc(socket, Some(bootstrap.client), Side::Server).await {
                eprintln!("upstream workflow connection closed: {error}");
            }
        });
    }
}
async fn connect(address: &str) -> Result<workflows::Client> {
    let socket = TcpStream::connect(address).await?;
    socket.set_nodelay(true)?;
    let mut system = crate::rpc(socket, None, Side::Client);
    let client = system.bootstrap(Side::Server);
    tokio::task::spawn_local(system);
    Ok(client)
}

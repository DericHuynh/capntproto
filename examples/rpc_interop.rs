//! Same scenarios as the upstream peer, using our generated bindings and drivers.
#![forbid(unsafe_code)]
use capntproto_test_support::interop_capnp;
#[path = "../tests/interop/rpc_peer.rs"]
mod rpc_peer;

async fn tail(
    results: interop_capnp::peer::TailResults,
    call: capnp::capability::Request<
        interop_capnp::peer::echo_params::Owned,
        interop_capnp::peer::echo::Owned,
    >,
) -> capnp::Result<()> {
    results.hook.tail_call(call.hook).await
}

fn start<T: futures::AsyncRead + Unpin + 'static>(
    mut network: capnp_rpc::twoparty::VatNetwork<T>,
    bootstrap: Option<capnp::capability::Client>,
    driver: &str,
) -> rpc_peer::Connection {
    let is_client = bootstrap.is_none();
    if driver == "legacy-transfer" {
        network.set_tail_call_optimization(true);
    }
    let (client, disconnect, task) = if driver == "portable" {
        let options = capnp_rpc::portable::Options::bounded(
            4.try_into().unwrap(),
            512.try_into().unwrap(),
            (8 * 1024 * 1024).try_into().unwrap(),
            (64 * 1024 * 1024).try_into().unwrap(),
        )
        .unwrap();
        let mut system =
            capnp_rpc::portable::RpcSystem::new(Box::new(network), bootstrap, options).unwrap();
        let client =
            is_client.then(|| system.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server));
        let disconnect: rpc_peer::Disconnect = Box::pin(system.get_disconnector());
        (client, disconnect, tokio::task::spawn_local(system))
    } else {
        assert!(matches!(driver, "legacy" | "legacy-transfer"));
        let mut system = capnp_rpc::RpcSystem::new(Box::new(network), bootstrap);
        let client =
            is_client.then(|| system.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server));
        let disconnect: rpc_peer::Disconnect = Box::pin(system.get_disconnector());
        (client, disconnect, tokio::task::spawn_local(system))
    };
    rpc_peer::Connection {
        client,
        disconnect,
        task,
    }
}

fn main() -> rpc_peer::Result<()> {
    rpc_peer::main()
}

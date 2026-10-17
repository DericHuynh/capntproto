//! This executable links only the published upstream Rust implementation.
#![forbid(unsafe_code)]
mod interop_capnp {
    include!(concat!(env!("OUT_DIR"), "/interop_capnp.rs"));
}
#[allow(dead_code, clippy::extra_unused_type_parameters)] // Unused parts of the shared schema.
mod runtime_test_capnp {
    include!(concat!(env!("OUT_DIR"), "/runtime_test_capnp.rs"));
}
mod bounded;
// Adapt generated signatures while sharing the entire behavioral contract.
macro_rules! impl_peer_server {
    ($server:ty; $($name:ident $context:ident $params:ident $results:ident;)*) => {
        impl crate::interop_capnp::peer::Server for $server {
            $(fn $name(self: std::rc::Rc<Self>, params: crate::interop_capnp::peer::$params, results: crate::interop_capnp::peer::$results) -> impl core::future::Future<Output = capnp::Result<()>> {
                Self::$name(self, (params, results))
            })*
            fn stream(self: std::rc::Rc<Self>, params: crate::interop_capnp::peer::StreamParams) -> impl core::future::Future<Output = capnp::Result<()>> {
                Self::stream(self, params)
            }
        }
    };
}
pub(crate) use impl_peer_server;

#[path = "../../../tests/interop/rpc_peer.rs"]
mod rpc_peer;

async fn tail(
    mut results: interop_capnp::peer::TailResults,
    call: capnp::capability::Request<
        interop_capnp::peer::echo_params::Owned,
        interop_capnp::peer::echo::Owned,
    >,
) -> capnp::Result<()> {
    // capnp-rpc 0.27.0's ResultsHook::tail_call() is unimplemented. Use the
    // supported application-level forwarding API. Independent C++ controls
    // still exercise optimized wire transfers and report the upstream failures.
    let response = call.send().promise.await?;
    let p = response.get()?;
    let mut r = results.get();
    r.set_value(p.get_value());
    r.set_payload(p.get_payload()?);
    r.set_text(p.get_text()?);
    Ok(())
}

fn start<T: futures::AsyncRead + Unpin + 'static>(
    network: capnp_rpc::twoparty::VatNetwork<T>,
    bootstrap: Option<capnp::capability::Client>,
    _: &str,
) -> rpc_peer::Connection {
    let is_client = bootstrap.is_none();
    let mut system = capnp_rpc::RpcSystem::new(Box::new(network), bootstrap);
    let client = is_client.then(|| system.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server));
    let disconnect = Box::pin(system.get_disconnector());
    let task = tokio::task::spawn_local(system);
    rpc_peer::Connection {
        client,
        disconnect,
        task,
    }
}

fn main() -> rpc_peer::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("bounded-server") {
        return bounded::main();
    }
    rpc_peer::main()
}

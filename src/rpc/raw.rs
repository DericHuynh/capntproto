//! Detached Tokio task adapters for integrations that manage task lifetime directly.
//!
//! Dropping a returned join handle leaves its task running. Most applications
//! should retain an owning [`super::Connection`] instead.
use super::io::TokioIo;
/// Low-level task adapter. Dropping the returned join handle detaches the task;
/// prefer [`super::Connection`] when a scope should own and cancel the driver.
pub fn serve(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    bootstrap: capnp::capability::Client,
) -> tokio::task::JoinHandle<capnp::Result<()>> {
    tokio::task::spawn_local(capnp_rpc::twoparty::TwoPartyClient::with_bootstrap(
        TokioIo::new(io),
        Some(bootstrap),
        capnp_rpc::rpc_twoparty_capnp::Side::Server,
    ))
}
/// Low-level task adapter returning an initial typed bootstrap. Prefer
/// [`super::Connection`] for bounded shutdown and cancellation when the owner is dropped.
pub fn client<T: capnp::capability::FromClientHook>(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
) -> (T, tokio::task::JoinHandle<capnp::Result<()>>) {
    let mut system = capnp_rpc::twoparty::TwoPartyClient::new(TokioIo::new(io));
    let client = system.bootstrap();
    (client, tokio::task::spawn_local(system))
}

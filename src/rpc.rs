//! Tokio adapters for ordered RPC streams, including the Noise duplex transport.
pub mod tcp;
use tokio_util::compat::TokioAsyncReadCompatExt;
pub fn serve(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    bootstrap: capnp::capability::Client,
) -> tokio::task::JoinHandle<capnp::Result<()>> {
    tokio::task::spawn_local(capnp_rpc::twoparty::TwoPartyClient::with_bootstrap(
        io.compat(),
        Some(bootstrap),
        capnp_rpc::rpc_twoparty_capnp::Side::Server,
    ))
}
pub fn client<T: capnp::capability::FromClientHook>(
    io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
) -> (T, tokio::task::JoinHandle<capnp::Result<()>>) {
    let mut system = capnp_rpc::twoparty::TwoPartyClient::new(io.compat());
    let client = system.bootstrap();
    (client, tokio::task::spawn_local(system))
}

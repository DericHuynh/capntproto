//! Executor-neutral framing over a transport's bidirectional byte stream.
use capnp::{capability::Client, message::ReaderOptions};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
use futures::{AsyncRead, AsyncReadExt, AsyncWrite};

/// One Cap'n Proto RPC connection per reliable bidirectional transport stream.
/// Retain and poll the returned driver on the platform's local executor.
pub fn driver<S: AsyncRead + AsyncWrite + Unpin + 'static>(
    stream: S,
    bootstrap: Option<Client>,
    side: Side,
    protocol: twoparty::RpcDriver,
) -> capnp::Result<twoparty::TwoPartyClient<'static>> {
    twoparty::TwoPartyClient::with_driver(stream, bootstrap, side, protocol)
}

pub fn network<S: AsyncRead + AsyncWrite + Unpin + 'static>(
    stream: S,
    side: Side,
    options: ReaderOptions,
) -> twoparty::VatNetwork<futures::io::ReadHalf<S>> {
    let (read, write) = stream.split();
    twoparty::VatNetwork::new(read, write, side, options)
}

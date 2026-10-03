//! TCP adapters with automatic socket-informed streaming flow control. Generic
//! byte IO has no socket metadata; use these entry points to sample SO_SNDBUF.
use capnp::{capability::Client, message::ReaderOptions};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
use tokio::net::{tcp::OwnedReadHalf, TcpListener, TcpStream, ToSocketAddrs};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Dial a plaintext TCP peer with Nagle's algorithm disabled for RPC latency.
/// Wrap this future in `tokio::time::timeout` to bound DNS and connection setup.
/// Use `rpc::tls` (with the `tls` feature) for authenticated encryption.
pub async fn connect(
    address: impl ToSocketAddrs,
    bootstrap: Option<Client>,
    options: ReaderOptions,
) -> std::io::Result<twoparty::TwoPartyClient<'static>> {
    let socket = TcpStream::connect(address).await?;
    socket.set_nodelay(true)?;
    Ok(client(socket, bootstrap, Side::Client, options))
}

/// Build a network from an already connected socket. Each stream uses its live
/// send-buffer size, with a connection-wide 64 KiB fallback on query failure.
/// Explicit flow-policy setters on the returned network override this default
/// for subsequently created streams. No socket is duplicated for the query.
pub fn network(
    socket: TcpStream,
    side: Side,
    options: ReaderOptions,
) -> twoparty::VatNetwork<Compat<OwnedReadHalf>> {
    let (read, write) = socket.into_split();
    twoparty::VatNetwork::new_with_send_buffer(
        read.compat(),
        write.compat_write(),
        side,
        options,
        |write| {
            socket2::SockRef::from(write.get_ref().as_ref())
                .send_buffer_size()
                .ok()
        },
    )
}

/// Obtain bootstrap/observation handles, then poll this driver on a local
/// executor. The socket is owned by the driver and its connection handles.
pub fn client(
    socket: TcpStream,
    bootstrap: Option<Client>,
    side: Side,
    options: ReaderOptions,
) -> twoparty::TwoPartyClient<'static> {
    twoparty::TwoPartyClient::from_network(network(socket, side, options), bootstrap)
}

/// Accept sockets into the common server. Poll its ServerDriver concurrently.
/// Canceling this future stops acceptance and leaves accepted connections alive.
pub async fn listen(
    server: &twoparty::TwoPartyServer,
    listener: &TcpListener,
    options: ReaderOptions,
) -> capnp::Result<()> {
    let incoming = futures::stream::try_unfold(listener, move |listener| async move {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        Ok::<_, std::io::Error>(Some((network(socket, Side::Server, options), listener)))
    });
    server.listen_networks(incoming).await
}

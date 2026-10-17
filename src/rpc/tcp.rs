//! TCP adapters with automatic socket-informed streaming flow control. Generic
//! byte IO has no socket metadata; use these entry points to sample SO_SNDBUF.
mod io;
#[cfg(target_os = "linux")]
mod low_water;
use capnp::{capability::Client, message::ReaderOptions};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
pub use io::ReadHalf;
use io::WriteHalf;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

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
/// On Linux, incomplete large bodies supply a bounded readiness minimum. This
/// preserves TCP receive autotuning; complete frames restore ordinary readiness.
pub fn network(
    socket: TcpStream,
    side: Side,
    options: ReaderOptions,
) -> twoparty::VatNetwork<ReadHalf> {
    let (read, write) = socket.into_split();
    let mut network = twoparty::VatNetwork::new_with_send_buffer(
        ReadHalf::new(read),
        WriteHalf(write),
        side,
        options,
        |write| {
            socket2::SockRef::from(write.0.as_ref())
                .send_buffer_size()
                .ok()
        },
    );
    network.set_task_yield(super::io::defer);
    network
        .set_read_hint(ReadHalf::set_read_hint)
        .expect("new network owns its input");
    network
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

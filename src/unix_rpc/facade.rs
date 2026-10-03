//! Linux/macOS descriptor-aware conveniences backed by the common RPC facade.
use super::{Options, VatNetwork};
use capnp::capability::Client;
use capnp_rpc::{
    rpc_twoparty_capnp::Side,
    twoparty::{self, ServerDriver, TwoPartyClient},
};
use std::os::fd::AsFd;
use tokio::net::{UnixListener, UnixStream};

/// Construct a descriptor-aware client, optionally exposing a local bootstrap
/// on either side. Poll the returned driver on a Tokio LocalSet.
pub fn client(
    socket: UnixStream,
    bootstrap: Option<Client>,
    side: Side,
    options: Options,
) -> TwoPartyClient<'static> {
    TwoPartyClient::from_network(VatNetwork::new(socket, side, options), bootstrap)
}

fn duplicate(socket: &UnixStream) -> std::io::Result<UnixStream> {
    // A duplicate owns its descriptor, shares the connected socket, and retains
    // CLOEXEC. The existing socket is already nonblocking. No raw lifetime cast.
    UnixStream::from_std(std::os::unix::net::UnixStream::from(
        socket.as_fd().try_clone_to_owned()?,
    ))
}

/// Borrow a connected Unix socket. RPC owns a duplicated descriptor and borrows
/// the original exclusively until driver disposal. Cancellation releases only
/// the duplicate; normal protocol shutdown may shut down the shared write half.
/// Consumed framing/ancillary input is not returned for subsequent reuse.
///
/// ```compile_fail
/// # fn example(mut socket: tokio::net::UnixStream) -> std::io::Result<()> {
/// let driver = capntproto::unix_rpc::client_borrowed(&mut socket, None,
///     capnp_rpc::rpc_twoparty_capnp::Side::Client, Default::default())?;
/// drop(socket);
/// drop(driver);
/// # Ok(()) }
/// ```
pub fn client_borrowed(
    socket: &mut UnixStream,
    bootstrap: Option<Client>,
    side: Side,
    options: Options,
) -> std::io::Result<TwoPartyClient<'_>> {
    let network = VatNetwork::with_ownership(duplicate(socket)?, side, options, true);
    Ok(TwoPartyClient::from_scoped_network(
        network, bootstrap, socket,
    ))
}

/// Descriptor-aware server. The returned common driver owns accepted sockets;
/// borrowed accepts are driven separately and excluded from drain. Dropping a
/// listener future stops acceptance without canceling existing connections.
pub struct TwoPartyServer {
    server: twoparty::TwoPartyServer,
    options: Options,
}
impl TwoPartyServer {
    pub fn new(bootstrap: Client, options: Options) -> (Self, ServerDriver) {
        Self::with_error_handler(bootstrap, options, |_| {})
    }
    pub fn with_error_handler(
        bootstrap: Client,
        options: Options,
        on_error: impl FnMut(capnp::Error) + 'static,
    ) -> (Self, ServerDriver) {
        let (server, driver) = twoparty::TwoPartyServer::with_error_handler(bootstrap, on_error);
        (Self { server, options }, driver)
    }
    pub fn accept(&self, socket: UnixStream) -> capnp::Result<()> {
        self.server
            .accept_network(VatNetwork::new(socket, Side::Server, self.options))
    }
    pub fn accept_borrowed<'a>(
        &self,
        socket: &'a mut UnixStream,
    ) -> capnp::Result<TwoPartyClient<'a>> {
        let network =
            VatNetwork::with_ownership(duplicate(socket)?, Side::Server, self.options, true);
        self.server.accept_scoped_network(network, socket)
    }
    /// Applies to subsequently accepted connections. Configure the server before
    /// listening; an active listener borrows it. Excess incoming FDs are closed.
    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }
    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&capnp::Error) -> String + 'static) {
        self.server.set_trace_encoder(encoder);
    }
    pub fn drain(&self) -> capnp::capability::Promise<(), capnp::Error> {
        self.server.drain()
    }
    /// Listen on a connected-socket receiver. Authorization of accepted peers
    /// remains the application's responsibility, just as for `VatNetwork::new`.
    pub async fn listen(&self, listener: &UnixListener) -> capnp::Result<()> {
        let options = self.options;
        let incoming = futures::stream::try_unfold(listener, move |listener| async move {
            let (socket, _) = listener.accept().await?;
            Ok::<_, std::io::Error>(Some((
                VatNetwork::new(socket, Side::Server, options),
                listener,
            )))
        });
        self.server.listen_networks(incoming).await
    }
}

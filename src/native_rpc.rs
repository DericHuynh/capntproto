//! Multiparty RPC over authenticated Native sessions. Install a direct session
//! for each permitted route, or configure a connector for on-demand dialing.
//! Contacts identify a host; only the application connector chooses its address.
//! Construct and run the network inside a Tokio `LocalSet`, like the RPC engine.
//! [`Vat::builder`] combines network setup, typed bootstrap access, capability
//! joins and owned task shutdown. [`Network`] remains the executor-level API.
use crate::transport::{self, AuthenticatedSession, Identity};
use capnp::{capability::Promise, Error};
use capnp_rpc::{
    third_party::ThirdPartyExchange, Connection, IncomingMessage, OutgoingMessage, VatNetwork,
};
use futures::{
    channel::{mpsc, oneshot},
    StreamExt,
};
use ring::rand::SecureRandom;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    net::SocketAddr,
    rc::{Rc, Weak},
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Authenticated public identity of a native RPC vat.
pub type VatId = [u8; 32];
type Key = (VatId, VatId, [u8; 32]); // introducer, recipient, nonce
const MAX_ROUTES: usize = 64;
const MAX_PROVISIONS: usize = 4096;
const MAX_WAITERS: usize = 4096;
fn failed(s: &str) -> Error {
    Error::failed(s.into())
}
fn gone() -> Error {
    Error::disconnected("Native RPC route unavailable".into())
}
fn random() -> capnp::Result<[u8; 32]> {
    let mut bytes = [0; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| failed("OS randomness unavailable"))?;
    Ok(bytes)
}
fn token<const N: usize>(p: capnp::any_pointer::Reader<'_>) -> capnp::Result<[u8; N]> {
    p.get_as::<capnp::data::Reader<'_>>()?
        .try_into()
        .map_err(|_| failed("invalid Native introduction token"))
}
#[derive(Default)]
struct Exchange {
    values: HashMap<Key, Rc<ThirdPartyExchange>>,
    waiting: HashMap<Key, HashMap<u64, oneshot::Sender<Rc<ThirdPartyExchange>>>>,
    retired: HashSet<Key>,
    next: u64,
    published: u64,
}
struct Registration(Weak<RefCell<Exchange>>, Key);
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(exchange) = self.0.upgrade() {
            let removed = {
                let mut e = exchange.borrow_mut();
                e.retired.insert(self.1);
                (e.values.remove(&self.1), e.waiting.remove(&self.1))
            };
            drop(removed);
        }
    }
}
struct Waiter(Weak<RefCell<Exchange>>, Key, u64);
impl Drop for Waiter {
    fn drop(&mut self) {
        if let Some(exchange) = self.0.upgrade() {
            let mut e = exchange.borrow_mut();
            if let Some(waiters) = e.waiting.get_mut(&self.1) {
                waiters.remove(&self.2);
                if waiters.is_empty() {
                    e.waiting.remove(&self.1);
                }
            }
        }
    }
}
/// Application route policy. Resolve only permitted identities and return a
/// Native-authenticated session. The network validates both session identities
/// before forwarding any RPC bytes. Futures run inside the network's LocalSet;
/// dropping a route, disconnecting, or dropping the network cancels its dial.
/// The connector owns discovery and listener provisioning. The opt-in network
/// arbitration profile can coalesce crossed authenticated sessions. A peer-supplied
/// contact is never an address or authorization grant.
pub trait Connector {
    fn connect(&self, peer: [u8; 32]) -> Promise<AuthenticatedSession, Error>;
}

/// Explicit transport choice for an authorized directory route.
#[derive(Clone, Copy, Debug, Default)]
pub enum Backend {
    Tcp,
    #[default]
    Quiche,
    /// Reserved wire selection. Upstream Quiche currently rejects QUIC v2.
    QuicheV2,
}
/// Fixed allowlist of pinned peer endpoints for on-demand native RPC routes.
/// Destinations provide either a dedicated listener or a single-use shared
/// listener reservation, with matching PSK and application context. Listener
/// discovery and authorized provisioning remain application policy.
pub struct DirectoryConnector {
    identity: Rc<Identity>,
    bind: SocketAddr,
    peers: RefCell<HashMap<VatId, DialTarget>>,
}
#[derive(Clone)]
struct DialTarget {
    backend: Backend,
    initial_id: Option<[u8; 16]>,
    used: Cell<bool>,
    address: SocketAddr,
    psk: Option<[u8; 32]>,
    context: Vec<u8>,
}
impl Drop for DialTarget {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.psk.zeroize();
    }
}
impl DirectoryConnector {
    /// `bind` chooses the local interface; use port zero for an ephemeral socket
    /// per connection. The identity's private key remains owned by its Rc.
    pub fn new(identity: Rc<Identity>, bind: SocketAddr) -> Self {
        Self {
            identity,
            bind,
            peers: RefCell::new(HashMap::new()),
        }
    }
    /// Configure or replace a permitted destination. Replacing a directory entry
    /// affects subsequent dials, not existing authenticated sessions.
    pub fn insert(
        &mut self,
        peer: VatId,
        address: SocketAddr,
        psk: Option<[u8; 32]>,
        context: &[u8],
    ) -> capnp::Result<()> {
        self.insert_with_backend(peer, address, psk, context, Backend::Quiche)
    }
    /// Select TCP/TLS or quiche for subsequent dials of this route.
    pub fn insert_with_backend(
        &self,
        peer: VatId,
        address: SocketAddr,
        psk: Option<[u8; 32]>,
        context: &[u8],
        backend: Backend,
    ) -> capnp::Result<()> {
        self.put(peer, address, psk, context, None, backend)
    }
    /// Provision a single-use shared-listener target. Updating this directory
    /// affects only subsequent dials, never existing RPC route identities.
    pub fn insert_reserved(
        &self,
        target: crate::native_listener::Target,
        psk: Option<[u8; 32]>,
        context: &[u8],
    ) -> capnp::Result<()> {
        if context.len() > crate::native_listener::MAX_CONTEXT_BYTES {
            return Err(failed("reserved Native context limit"));
        }
        self.put(
            target.host,
            target.address,
            psk,
            context,
            Some(target.connection_id),
            Backend::Quiche,
        )
    }
    /// Select the client QUIC backend for a shared listener reservation.
    pub fn insert_reserved_for_version(
        &self,
        target: crate::native_listener::Target,
        psk: Option<[u8; 32]>,
        context: &[u8],
        version: transport::QuicVersion,
    ) -> capnp::Result<()> {
        if context.len() > crate::native_listener::MAX_CONTEXT_BYTES {
            return Err(failed("reserved context limit"));
        }
        self.put(
            target.host,
            target.address,
            psk,
            context,
            Some(target.connection_id),
            match version {
                transport::QuicVersion::V1 => Backend::Quiche,
                transport::QuicVersion::V2 => Backend::QuicheV2,
            },
        )
    }
    #[allow(clippy::too_many_arguments)] // Each field is part of the authorized route.
    fn put(
        &self,
        peer: VatId,
        address: SocketAddr,
        psk: Option<[u8; 32]>,
        context: &[u8],
        initial_id: Option<[u8; 16]>,
        backend: Backend,
    ) -> capnp::Result<()> {
        let target = DialTarget {
            backend,
            initial_id,
            used: Cell::new(false),
            address,
            psk,
            context: context.to_vec(),
        };
        if peer == self.identity.public_key()
            || address.ip().is_unspecified()
            || address.port() == 0
        {
            return Err(failed("invalid pinned Native destination"));
        }
        let mut peers = self.peers.borrow_mut();
        if !peers.contains_key(&peer) && peers.len() >= MAX_ROUTES {
            return Err(Error::overloaded("Native directory limit".into()));
        }
        peers.insert(peer, target);
        Ok(())
    }
}
impl Connector for DirectoryConnector {
    fn connect(&self, peer: VatId) -> Promise<AuthenticatedSession, Error> {
        let target = {
            let peers = self.peers.borrow();
            let Some(target) = peers.get(&peer) else {
                return Promise::err(gone());
            };
            if target.initial_id.is_some() && target.used.replace(true) {
                return Promise::err(gone());
            }
            target.clone()
        };
        let identity = self.identity.clone();
        let bind = self.bind;
        Promise::from_future(async move {
            if matches!(target.backend, Backend::Tcp) {
                return transport::tcp::connect_bound(
                    bind,
                    target.address,
                    &identity,
                    peer,
                    target.psk,
                    &target.context,
                    std::time::Duration::from_secs(10),
                )
                .await
                .map_err(dial_error);
            }
            let socket = tokio::net::UdpSocket::bind(bind)
                .await
                .map_err(|e| failed(&e.to_string()))?;
            let result = if let Some(connection_id) = target.initial_id {
                let reserved = crate::native_listener::Target {
                    address: target.address,
                    host: peer,
                    connection_id,
                };
                crate::native_listener::connect_for_version(
                    socket,
                    reserved,
                    &identity,
                    target.psk,
                    &target.context,
                    if matches!(target.backend, Backend::QuicheV2) {
                        transport::QuicVersion::V2
                    } else {
                        transport::QuicVersion::V1
                    },
                )
                .await
            } else {
                transport::connect_for_version(
                    socket,
                    target.address,
                    &identity,
                    peer,
                    target.psk,
                    &target.context,
                    if matches!(target.backend, Backend::QuicheV2) {
                        transport::QuicVersion::V2
                    } else {
                        transport::QuicVersion::V1
                    },
                )
                .await
            };
            result.map_err(dial_error)
        })
    }
}

mod generation;
mod vat;
pub use vat::{ShutdownReport, Vat, VatBuilder};
mod policy;
mod session;
pub use generation::RouteGeneration;
pub(crate) use policy::dial_error;
pub use policy::{Options, RetryPolicy, RetryingConnector};
use session::SessionTask;
pub use session::{FailureKind, RouteFailure, RouteObserver, RouteStatus, Termination};
#[path = "native_join.rs"]
mod join;
pub use join::Stats as JoinStats;

struct State {
    local: VatId,
    connector: Option<Rc<dyn Connector>>,
    options: Options,
    closed: Cell<bool>,
    admission_changed: Rc<tokio::sync::Notify>,
    generations: generation::Generations,
    routes: RefCell<HashMap<VatId, Weak<Endpoint>>>,
    incoming: mpsc::UnboundedSender<Rc<Endpoint>>,
    exchange: Rc<RefCell<Exchange>>,
    joins: Rc<RefCell<join::Table>>,
}
struct Endpoint {
    peer: VatId,
    state: Weak<State>,
    wire: RefCell<Box<dyn Connection<capnp_rpc::rpc_twoparty_capnp::Side>>>,
    session: SessionTask,
    output_closed: futures::future::Shared<Promise<(), Error>>,
}
#[derive(Clone)]
struct Route(Rc<Endpoint>);
/// Cloneable installation/control handle. Only a successfully authenticated
/// session can enter this network; its local identity must match this vat.
#[derive(Clone)]
pub struct Handle(Rc<State>);
pub struct Network {
    state: Rc<State>,
    incoming: Rc<RefCell<Option<mpsc::UnboundedReceiver<Rc<Endpoint>>>>>,
}
impl Network {
    pub fn new(local: VatId) -> (Self, Handle) {
        Self::configured(local, None, Options::default())
    }
    pub fn with_connector(local: VatId, connector: Rc<dyn Connector>) -> (Self, Handle) {
        Self::configured(local, Some(connector), Options::default())
    }
    /// Both peers must opt in. Candidate sessions negotiate one shared stream
    /// before queued RPC bytes are forwarded; crossed dials reuse one endpoint.
    pub fn with_arbitration(
        local: VatId,
        connector: Option<Rc<dyn Connector>>,
        limits: crate::native_arbitration::Limits,
    ) -> capnp::Result<(Self, Handle)> {
        Self::with_options(
            local,
            connector,
            Options {
                arbitration: Some(limits),
                ..Options::default()
            },
        )
    }
    /// Configure setup deadlines and optional failed-generation recovery.
    pub fn with_options(
        local: VatId,
        connector: Option<Rc<dyn Connector>>,
        options: Options,
    ) -> capnp::Result<(Self, Handle)> {
        options.validate()?;
        Ok(Self::configured(local, connector, options))
    }
    fn configured(
        local: VatId,
        connector: Option<Rc<dyn Connector>>,
        options: Options,
    ) -> (Self, Handle) {
        let (sender, receiver) = mpsc::unbounded();
        let state = Rc::new(State {
            local,
            connector,
            options,
            closed: Cell::new(false),
            admission_changed: Rc::new(tokio::sync::Notify::new()),
            generations: generation::Generations::new(),
            routes: RefCell::new(HashMap::new()),
            incoming: sender,
            exchange: Rc::new(RefCell::new(Exchange::default())),
            joins: Rc::new(RefCell::new(join::Table::default())),
        });
        (
            Self {
                state: state.clone(),
                incoming: Rc::new(RefCell::new(Some(receiver))),
            },
            Handle(state),
        )
    }
}
/// Non-secret operational counters for bounded rendezvous storage.
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub provisions: usize,
    pub waiters: usize,
    pub retired: usize,
    pub published: u64,
}
impl Handle {
    pub(crate) fn is_closed(&self) -> bool {
        self.0.closed.get()
    }
    pub(crate) fn admission_changed(&self) -> Rc<tokio::sync::Notify> {
        self.0.admission_changed.clone()
    }
    pub fn join_stats(&self) -> JoinStats {
        self.0.joins.borrow().stats()
    }
    pub fn identity(&self) -> [u8; 32] {
        self.0.local
    }
    pub fn arbitrates(&self) -> bool {
        self.0.options.arbitration.is_some()
    }
    pub(crate) fn accepts_provision(&self, peer: VatId) -> bool {
        if self.is_closed() {
            return false;
        }
        match self.route_status(peer) {
            None => true,
            Some(RouteStatus::Connecting) => self.arbitrates(),
            Some(RouteStatus::Failed) => self.0.options.recover_failed_routes,
            _ => false,
        }
    }
    /// Non-secret session selector shared by both peers after arbitration.
    /// It does not confer capability authority or keep the route alive.
    pub fn selected_session(&self, peer: VatId) -> Option<[u8; 16]> {
        self.0
            .routes
            .borrow()
            .get(&peer)
            .and_then(Weak::upgrade)
            .and_then(|e| {
                e.session
                    .admission
                    .as_ref()
                    .and_then(|a| a.election.selected())
            })
    }
    /// Take the installed route's datagram lane once. Returns None while
    /// selection is pending, after failure, or after taking it.
    /// New realtime grants must be bound to this session; reconnect does not
    /// migrate old datagram capabilities.
    pub fn take_datagrams(&self, peer: VatId) -> Option<crate::transport::DatagramPort> {
        let endpoint = self.0.routes.borrow().get(&peer).and_then(Weak::upgrade)?;
        if endpoint.session.status() != RouteStatus::Authenticated {
            return None;
        }
        endpoint.session.take_datagrams()
    }
    /// Control this authenticated route's path/CIDs without replacing its RPC
    /// generation or replaying calls. Available after arbitration completes.
    pub fn mobility(&self, peer: VatId) -> Option<crate::transport::Mobility> {
        self.0
            .routes
            .borrow()
            .get(&peer)
            .and_then(Weak::upgrade)?
            .session
            .mobility()
    }
    /// Local pacing and send-burst policy for the selected authenticated session.
    /// A reconnect gets a fresh control and default policy; old controls never
    /// configure a replacement generation.
    pub fn scheduling(&self, peer: VatId) -> Option<crate::transport::Scheduling> {
        self.0
            .routes
            .borrow()
            .get(&peer)
            .and_then(Weak::upgrade)?
            .session
            .scheduling()
    }
    pub fn stats(&self) -> Stats {
        let e = self.0.exchange.borrow();
        Stats {
            provisions: e.values.len(),
            waiters: e.waiting.values().map(HashMap::len).sum(),
            retired: e.retired.len(),
            published: e.published,
        }
    }

    /// Inspect a live route without keeping its connection alive.
    pub fn route_status(&self, peer: VatId) -> Option<RouteStatus> {
        self.0
            .routes
            .borrow()
            .get(&peer)
            .and_then(Weak::upgrade)
            .map(|e| e.session.status())
    }
    /// Observe this generation without retaining its connection or capabilities.
    pub fn observe_route(&self, peer: VatId) -> Option<RouteObserver> {
        self.0
            .routes
            .borrow()
            .get(&peer)
            .and_then(Weak::upgrade)
            .map(|e| e.session.observe())
    }
    /// Install a direct route, or admit an authenticated candidate to an
    /// arbitrated endpoint. Arbitration must finish before its RPC is published.
    pub fn attach(&self, session: AuthenticatedSession) -> capnp::Result<()> {
        if self.0.closed.get() {
            return Err(gone());
        }
        if session.local != self.0.local || session.peer == self.0.local {
            return Err(failed("session belongs to a different/local vat"));
        }
        let existing = self.0.reusable_route(session.peer);
        if let Some(endpoint) = existing {
            if endpoint.session.status() == RouteStatus::Connecting {
                if let Some(admission) = &endpoint.session.admission {
                    return admission.submit(session);
                }
            }
            return Err(failed("duplicate live Native route"));
        }
        self.0.reserve()?;
        if self.0.options.arbitration.is_some() {
            let endpoint = self.0.arbitrated_endpoint(session.peer, None)?;
            endpoint
                .session
                .admission
                .as_ref()
                .unwrap()
                .submit(session)?;
            return self.0.incoming.unbounded_send(endpoint).map_err(|_| gone());
        }
        let peer = session.peer;
        let (io, session) =
            SessionTask::ready(self.0.generations.allocate()?, self.0.local, peer, session)?;
        let endpoint = self.0.endpoint(peer, io, session);
        self.0
            .incoming
            .unbounded_send(endpoint)
            .map_err(|_| gone())?;
        Ok(())
    }
    /// Stop accepting new messages on this route, flush the existing RPC write
    /// queue and wait for authenticated peer receipt. Application methods/replies
    /// and datagrams are not drained by this transport fence. Poll the RPC system
    /// while awaiting it. Cancellation or timeout aborts only this generation.
    /// An existing terminal failure/cancellation takes precedence over a receipt.
    /// When all three fences and the deadline are ready in the same poll, the
    /// completed fences win; a deadline bounds any still-incomplete drain.
    pub fn shutdown(
        &self,
        peer: VatId,
        timeout: std::time::Duration,
    ) -> Promise<crate::native_shutdown::Receipt, Error> {
        let endpoint = match self.0.routes.borrow().get(&peer).and_then(Weak::upgrade) {
            Some(endpoint) => endpoint,
            None => return Promise::err(gone()),
        };
        let control = match endpoint.session.lifecycle.begin_shutdown(timeout) {
            Ok(control) => control,
            Err(error) => return Promise::err(error),
        };
        let guard = ShutdownRoute(endpoint.clone());
        // Enqueue the end marker synchronously behind already-admitted messages.
        // The shutdown future closes the duplex half after this marker is processed.
        let flush = endpoint.wire.borrow_mut().shutdown(Ok(()));
        let output_closed = endpoint.output_closed.clone();
        Promise::from_future(async move {
            let _guard = guard;
            endpoint
                .session
                .lifecycle
                .complete_shutdown(flush, output_closed, &control, control.expired())
                .await
        })
    }
    /// Permanently close network admission and drain every current route.
    /// Results identify peers that acknowledged and peers that failed. Pending
    /// routes are aborted and reported as failures. Cancellation aborts the
    /// established generations already captured by the returned future.
    pub async fn shutdown_all(
        &self,
        timeout: std::time::Duration,
    ) -> capnp::Result<Vec<(VatId, capnp::Result<crate::native_shutdown::Receipt>)>> {
        if timeout.is_zero() || timeout > std::time::Duration::from_secs(60) {
            return Err(failed("shutdown timeout must be in (0, 60s]"));
        }
        self.0.closed.set(true);
        let peers: Vec<_> = self
            .0
            .routes
            .borrow()
            .iter()
            .filter_map(|(peer, endpoint)| endpoint.upgrade().map(|e| (*peer, e)))
            .collect();
        self.0.admission_changed.notify_waiters();
        let mut waits = Vec::new();
        for (peer, endpoint) in peers {
            let wait = if endpoint.session.status() == RouteStatus::Authenticated {
                self.shutdown(peer, timeout)
            } else {
                Promise::err(gone())
            };
            let guard = ShutdownRoute(endpoint);
            waits.push(async move {
                let _guard = guard;
                (peer, wait.await)
            });
        }
        Ok(futures::future::join_all(waits).await)
    }
    /// Stop a route and permit a new authenticated session for that peer.
    pub fn disconnect(&self, peer: VatId) {
        let endpoint = self
            .0
            .routes
            .borrow_mut()
            .remove(&peer)
            .and_then(|v| v.upgrade());
        if let Some(e) = endpoint {
            e.session.stop();
        }
    }
}
impl State {
    fn reusable_route(&self, peer: VatId) -> Option<Rc<Endpoint>> {
        let endpoint = self.routes.borrow().get(&peer).and_then(Weak::upgrade)?;
        if self.options.recover_failed_routes && endpoint.session.status() == RouteStatus::Failed {
            self.routes.borrow_mut().remove(&peer);
            // Terminate old workers before a replacement is installed. Its
            // retained clients and observer still refer to the old generation.
            endpoint.session.stop();
            None
        } else {
            Some(endpoint)
        }
    }
    fn arbitrated_endpoint(
        self: &Rc<Self>,
        peer: VatId,
        connector: Option<Rc<dyn Connector>>,
    ) -> capnp::Result<Rc<Endpoint>> {
        let generation = self.generations.allocate()?;
        let (admission, selection) = crate::native_arbitration::select(
            self.local,
            peer,
            connector,
            self.options.arbitration.unwrap(),
        )?;
        let timeout = self.options.connect_timeout;
        let selection = policy::within_deadline(timeout, selection);
        let (io, session) =
            SessionTask::pending(generation, self.local, peer, selection, Some(admission));
        Ok(self.endpoint(peer, io, session))
    }
    fn reserve(&self) -> capnp::Result<()> {
        if self.closed.get() {
            return Err(gone());
        }
        let mut routes = self.routes.borrow_mut();
        routes.retain(|_, v| v.strong_count() > 0);
        if routes.len() >= MAX_ROUTES {
            return Err(Error::overloaded("Native route limit".into()));
        }
        Ok(())
    }
    fn endpoint(
        self: &Rc<Self>,
        peer: VatId,
        io: crate::rpc::local_io::Stream,
        session: SessionTask,
    ) -> Rc<Endpoint> {
        let (read, write) = io.into_split();
        let mut network = capnp_rpc::twoparty::VatNetwork::new(
            read.compat(),
            write.compat_write(),
            capnp_rpc::rpc_twoparty_capnp::Side::Client,
            capnp::message::ReaderOptions::new(),
        );
        let wire = network
            .connect(capnp_rpc::rpc_twoparty_capnp::Side::Server)
            .unwrap();
        let output_closed = network.output_closed();
        session.watch_writer(network.drive_until_shutdown(), output_closed.clone());
        let endpoint = Rc::new(Endpoint {
            peer,
            state: Rc::downgrade(self),
            wire: RefCell::new(wire),
            session,
            output_closed,
        });
        self.routes
            .borrow_mut()
            .insert(peer, Rc::downgrade(&endpoint));
        endpoint
    }
    fn route(self: &Rc<Self>, peer: VatId) -> capnp::Result<Rc<Endpoint>> {
        if self.closed.get() {
            return Err(gone());
        }
        if let Some(route) = self.reusable_route(peer) {
            return Ok(route);
        }
        let connector = self.connector.clone().ok_or_else(gone)?;
        self.reserve()?;
        if self.options.arbitration.is_some() {
            return self.arbitrated_endpoint(peer, Some(connector));
        }
        let timeout = self.options.connect_timeout;
        let selection = policy::within_deadline(timeout, async move {
            connector
                .connect(peer)
                .await
                .map_err(|e| RouteFailure::new(FailureKind::Connect, e))
        });
        let (io, session) = SessionTask::pending(
            self.generations.allocate()?,
            self.local,
            peer,
            selection,
            None,
        );
        Ok(self.endpoint(peer, io, session))
    }
}
struct AcceptLease {
    owner: Rc<RefCell<Option<mpsc::UnboundedReceiver<Rc<Endpoint>>>>>,
    receiver: Option<mpsc::UnboundedReceiver<Rc<Endpoint>>>,
}
impl Drop for AcceptLease {
    fn drop(&mut self) {
        *self.owner.borrow_mut() = self.receiver.take();
    }
}
impl Drop for Network {
    fn drop(&mut self) {
        self.state.closed.set(true);
        let detached = {
            let mut exchange = self.state.exchange.borrow_mut();
            (
                std::mem::take(&mut exchange.values),
                std::mem::take(&mut exchange.waiting),
            )
        };
        drop(detached);
        let endpoints: Vec<_> = self
            .state
            .routes
            .borrow_mut()
            .drain()
            .filter_map(|(_, weak)| weak.upgrade())
            .collect();
        for endpoint in endpoints {
            endpoint.session.stop();
        }
        self.state.admission_changed.notify_waiters();
    }
}
impl VatNetwork<VatId> for Network {
    fn join_network(&self) -> Option<Rc<dyn capnp_rpc::multiparty::JoinNetwork<VatId>>> {
        Some(Rc::new(join::Profile(Rc::downgrade(&self.state))))
    }
    fn connect(&mut self, peer: VatId) -> Option<Box<dyn Connection<VatId>>> {
        if peer == self.state.local {
            return None;
        }
        Some(match self.state.route(peer) {
            Ok(e) => Box::new(Route(e)),
            Err(_) => Box::new(Missing(peer)),
        })
    }

    fn accept(&mut self) -> Promise<Box<dyn Connection<VatId>>, Error> {
        let Some(receiver) = self.incoming.borrow_mut().take() else {
            return Promise::err(failed("concurrent network accept"));
        };
        let mut lease = AcceptLease {
            owner: self.incoming.clone(),
            receiver: Some(receiver),
        };
        Promise::from_future(async move {
            let endpoint = lease
                .receiver
                .as_mut()
                .unwrap()
                .next()
                .await
                .ok_or_else(gone)?;
            Ok(Box::new(Route(endpoint)) as Box<dyn Connection<VatId>>)
        })
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        Promise::from_future(futures::future::pending())
    }
}
impl Route {
    fn state(&self) -> capnp::Result<Rc<State>> {
        self.0
            .state
            .upgrade()
            .filter(|s| !s.closed.get())
            .ok_or_else(gone)
    }
    fn complete(
        &self,
        recipient: VatId,
        p: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<ThirdPartyExchange>, Error> {
        if join::is_completion(p) {
            let state = capnp_rpc::pry!(self.state());
            return Promise::from(join::accept(&state, recipient, p));
        }
        let bytes = capnp_rpc::pry!(token::<64>(p));
        let state = capnp_rpc::pry!(self.state());
        let key = (
            bytes[..32].try_into().unwrap(),
            recipient,
            bytes[32..].try_into().unwrap(),
        );
        let mut e = state.exchange.borrow_mut();
        if e.retired.contains(&key) {
            return Promise::err(gone());
        }
        if let Some(value) = e.values.get(&key) {
            return Promise::ok(value.clone());
        }
        if e.waiting.values().map(HashMap::len).sum::<usize>() >= MAX_WAITERS {
            return Promise::err(Error::overloaded("Native rendezvous waiter limit".into()));
        }
        let id = e.next;
        e.next = capnp_rpc::pry!(id
            .checked_add(1)
            .ok_or_else(|| Error::overloaded("Native waiter IDs exhausted".into())));
        let (tx, rx) = oneshot::channel();
        e.waiting.entry(key).or_default().insert(id, tx);
        let waiter = Waiter(Rc::downgrade(&state.exchange), key, id);
        Promise::from_future(async move {
            let result = rx.await.map_err(|_| gone());
            drop(waiter);
            result
        })
    }
}
impl Connection<VatId> for Route {
    fn get_peer_vat_id(&self) -> VatId {
        self.0.peer
    }
    fn connection_id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn new_outgoing_message(&mut self, n: u32) -> Box<dyn OutgoingMessage> {
        self.0.wire.borrow_mut().new_outgoing_message(n)
    }
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        self.0.wire.borrow_mut().receive_incoming_message()
    }
    fn when_write_finished(&self) -> Option<Promise<(), Error>> {
        self.0.wire.borrow().when_write_finished()
    }
    fn set_idle(&mut self, idle: bool) {
        self.0.wire.borrow_mut().set_idle(idle);
    }
    fn new_stream(&mut self) -> (Box<dyn capnp_rpc::FlowController>, Promise<(), Error>) {
        self.0.wire.borrow_mut().new_stream()
    }
    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), Error> {
        self.0.wire.borrow_mut().shutdown(result)
    }
    fn supports_multiparty_join(&self) -> bool {
        true
    }
    fn supports_third_party(&self) -> bool {
        true
    }
    fn supports_third_party_answers(&self) -> bool {
        true
    }
    fn supports_pipeline_join_fence(&self) -> bool {
        true
    }
    fn third_party_answer_timeout(&self) -> Promise<(), Error> {
        let state = capnp_rpc::pry!(self.state());
        let timer = tokio::time::sleep(state.options.answer_setup_timeout);
        Promise::from_future(async move {
            timer.await;
            Ok(())
        })
    }
    fn generate_embargo_id(&mut self) -> capnp::Result<Vec<u8>> {
        let mut id = self.state()?.local.to_vec();
        id.extend_from_slice(&random()?);
        Ok(id)
    }
    fn introduce_to(
        &mut self,
        recipient: VatId,
        mut contact: capnp::any_pointer::Builder<'_>,
        mut await_token: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<bool> {
        let state = self.state()?;
        if recipient != state.local
            && state
                .routes
                .borrow()
                .get(&recipient)
                .and_then(Weak::upgrade)
                .is_none()
        {
            return Ok(false);
        }
        let nonce = random()?;
        let mut c = self.0.peer.to_vec();
        c.extend_from_slice(&recipient);
        c.extend_from_slice(&nonce);
        let mut a = recipient.to_vec();
        a.extend_from_slice(&nonce);
        contact.set_as::<capnp::data::Owned>(&c[..])?;
        await_token.set_as::<capnp::data::Owned>(&a[..])?;
        Ok(true)
    }
    fn connect_to_introduced(
        &mut self,
        contact: capnp::any_pointer::Reader<'_>,
        mut completion: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<Option<Box<dyn Connection<VatId>>>> {
        let bytes = token::<96>(contact)?;
        let state = self.state()?;
        if bytes[32..64] != state.local {
            return Err(failed("introduction recipient mismatch"));
        }
        let host: VatId = bytes[..32].try_into().unwrap();
        let route = if host == state.local {
            None
        } else {
            Some(state.route(host)?)
        };
        let mut c = self.0.peer.to_vec();
        c.extend_from_slice(&bytes[64..]);
        completion.set_as::<capnp::data::Owned>(&c[..])?;
        Ok(route.map(|e| Box::new(Route(e)) as Box<dyn Connection<VatId>>))
    }
    fn await_third_party(
        &mut self,
        recipient: capnp::any_pointer::Reader<'_>,
        value: Rc<ThirdPartyExchange>,
    ) -> capnp::Result<Box<dyn std::any::Any>> {
        let bytes = token::<64>(recipient)?;
        let state = self.state()?;
        let key = (
            self.0.peer,
            bytes[..32].try_into().unwrap(),
            bytes[32..].try_into().unwrap(),
        );
        let mut e = state.exchange.borrow_mut();
        if e.values.contains_key(&key) || e.retired.contains(&key) {
            return Err(failed("duplicate/retired Native provision"));
        }
        if e.values.len() + e.retired.len() >= MAX_PROVISIONS {
            return Err(Error::overloaded("Native provision limit".into()));
        }
        e.values.insert(key, value.clone());
        e.published += 1;
        let waiters = e.waiting.remove(&key).unwrap_or_default();
        drop(e);
        for (_, waiter) in waiters {
            let _ = waiter.send(value.clone());
        }
        Ok(Box::new(Registration(Rc::downgrade(&state.exchange), key)))
    }
    fn complete_third_party(
        &mut self,
        p: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<ThirdPartyExchange>, Error> {
        self.complete(self.0.peer, p)
    }
    fn complete_third_party_local(
        &mut self,
        p: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<ThirdPartyExchange>, Error> {
        let state = capnp_rpc::pry!(self.state());
        self.complete(state.local, p)
    }
}
struct Missing(VatId);
struct Unsent(capnp::message::Builder<capnp::message::HeapAllocator>);
impl OutgoingMessage for Unsent {
    fn get_body(&mut self) -> capnp::Result<capnp::any_pointer::Builder<'_>> {
        self.0.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.0.get_root_as_reader()
    }
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
    ) {
        (Promise::err(gone()), Rc::new(self.0))
    }
    fn take(self: Box<Self>) -> capnp::message::Builder<capnp::message::HeapAllocator> {
        self.0
    }
    fn size_in_words(&self) -> usize {
        self.0.size_in_words()
    }
}
impl Connection<VatId> for Missing {
    fn get_peer_vat_id(&self) -> VatId {
        self.0
    }
    fn connection_id(&self) -> usize {
        self as *const Self as usize
    }
    fn new_outgoing_message(&mut self, _: u32) -> Box<dyn OutgoingMessage> {
        Box::new(Unsent(capnp::message::Builder::new_default()))
    }
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        Promise::err(gone())
    }
    fn shutdown(&mut self, _: capnp::Result<()>) -> Promise<(), Error> {
        Promise::ok(())
    }
}

struct ShutdownRoute(Rc<Endpoint>);
impl Drop for ShutdownRoute {
    fn drop(&mut self) {
        if let Some(state) = self.0.state.upgrade() {
            let mut routes = state.routes.borrow_mut();
            if routes
                .get(&self.0.peer)
                .is_some_and(|e| e.ptr_eq(&Rc::downgrade(&self.0)))
            {
                routes.remove(&self.0.peer);
            }
        }
        self.0.session.stop();
    }
}

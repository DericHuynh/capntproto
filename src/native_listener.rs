//! Bounded, single-use Native connection reservations on one UDP socket.
//! A destination connection ID routes packets; the pinned Native proof grants
//! session identity. Provision targets/PSKs through an authorized application.
use crate::transport::{
    self,
    socket::{DatagramIo, DatagramSocket},
    AuthenticatedSession, Identity, PacketSocket,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    net::SocketAddr,
    rc::{Rc, Weak},
    time::Duration,
};
use tokio::{net::UdpSocket, sync::Notify, time::Instant};

pub const MAX_ROUTES: usize = 64;
pub const MAX_ISSUED: usize = 4096;
pub const MAX_PACKET_BYTES: usize = crate::rpc::packet_mtu::RECEIVE_MAX;
/// Bound queued payload memory independently of the discovered path MTU.
pub const MAX_QUEUED_BYTES: usize = 128 * 1024;
pub const MAX_CONTEXT_BYTES: usize = 1024;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub routes: usize,
    pub queue: usize,
    pub reservation_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            routes: MAX_ROUTES,
            queue: 64,
            reservation_timeout: Duration::from_secs(10),
        }
    }
}
impl Limits {
    fn validate(self) -> io::Result<()> {
        if self.routes == 0
            || self.routes > MAX_ROUTES
            || self.queue == 0
            || self.queue > 64
            || self.reservation_timeout.is_zero()
            || self.reservation_timeout > Duration::from_secs(60)
        {
            return Err(invalid("invalid shared listener limits"));
        }
        Ok(())
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "Native listener reservation closed",
    )
}
fn transient_datagram_error(error: &io::Error) -> bool {
    // ICMP from an auxiliary STUN probe or a dead peer does not prove that
    // other authenticated peers on this shared socket are disconnected.
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::NetworkDown
            | io::ErrorKind::TimedOut
            | io::ErrorKind::Interrupted
    )
}
/// Routing information, not authorization. Override address when advertising a
/// listener bound to a wildcard or reached through a configured port mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub address: SocketAddr,
    pub host: [u8; 32],
    pub connection_id: [u8; 16],
}
struct Packet {
    bytes: Vec<u8>,
    from: SocketAddr,
}
struct Route {
    queue: VecDeque<Packet>,
    queued_bytes: usize,
    capacity: usize,
    signal: Rc<Notify>,
    shutdown: Rc<Notify>,
    expires: Option<Instant>,
}
impl Drop for Route {
    fn drop(&mut self) {
        self.signal.notify_one();
        self.shutdown.notify_waiters();
    }
}
struct Registry {
    closed: bool,
    accepting: bool,
    routes: BTreeMap<[u8; 16], Route>,
    issued: BTreeSet<[u8; 16]>,
    aliases: BTreeMap<[u8; 16], [u8; 16]>,
    reservations: usize,
    mapping: Weak<crate::nat::MappingState>,
    changed: Rc<Notify>,
}
// Detach packet queues before dropping/waking them. Callers may supply custom
// wakers to accept(); those callbacks must never run under a registry borrow.
fn expire(state: &RefCell<Registry>, now: Instant) {
    let retired = {
        let mut state = state.borrow_mut();
        let ids: Vec<_> = state
            .routes
            .iter()
            .filter_map(|(id, r)| r.expires.filter(|t| now >= *t).map(|_| *id))
            .collect();
        state.aliases.retain(|_, owner| !ids.contains(owner));
        ids.into_iter()
            .filter_map(|id| state.routes.remove(&id))
            .collect::<Vec<_>>()
    };
    drop(retired);
}
fn close(state: &RefCell<Registry>) {
    let (retired, mapping, changed) = {
        let mut state = state.borrow_mut();
        state.closed = true;
        state.accepting = false;
        state.aliases.clear();
        (
            std::mem::take(&mut state.routes),
            state.mapping.upgrade(),
            state.changed.clone(),
        )
    };
    drop(retired);
    if let Some(mapping) = mapping {
        mapping.close();
    }
    changed.notify_waiters();
}
fn dispatch(state: &RefCell<Registry>, bytes: &[u8], from: SocketAddr, now: Instant) {
    expire(state, now);
    if bytes.len() > MAX_PACKET_BYTES {
        return;
    }
    let mut bytes = bytes.to_vec();
    let Ok(header) = quiche::Header::from_slice(&mut bytes, 16) else {
        return;
    };
    let Ok(id) = <[u8; 16]>::try_from(header.dcid.as_ref()) else {
        return;
    };
    let signal = {
        let mut state = state.borrow_mut();
        if state.closed {
            return;
        }
        let Some(owner) = state.aliases.get(&id).copied() else {
            return;
        };
        let Some(route) = state.routes.get_mut(&owner) else {
            return;
        };
        if route.queue.len() >= route.capacity
            || route.queued_bytes + bytes.len() > MAX_QUEUED_BYTES
        {
            return;
        }
        route.queued_bytes += bytes.len();
        route.queue.push_back(Packet { bytes, from });
        route.signal.clone()
    };
    signal.notify_one();
}
fn authenticated(state: &RefCell<Registry>, id: [u8; 16], now: Instant) -> io::Result<()> {
    expire(state, now);
    let mut state = state.borrow_mut();
    if state.closed {
        return Err(closed());
    }
    state.routes.get_mut(&id).ok_or_else(closed)?.expires = None;
    Ok(())
}
struct Guard(Rc<RefCell<Registry>>);
impl Drop for Guard {
    fn drop(&mut self) {
        close(&self.0);
    }
}
struct Owner {
    identity: Rc<Identity>,
    socket: Rc<DatagramSocket>,
    state: Rc<RefCell<Registry>>,
    limits: Limits,
    task: tokio::task::JoinHandle<io::Result<()>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        close(&self.state);
        self.task.abort();
    }
}
/// Cloneable listener owner. Dropping its final clone or calling close() stops
/// both pending reservations and established sessions on this listener.
#[derive(Clone)]
pub struct Listener(Rc<Owner>);
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub pending: usize,
    pub authenticated: usize,
    pub issued: usize,
    /// All issued routing IDs, including replacements. Subject to MAX_ISSUED.
    pub connection_ids: usize,
    pub closed: bool,
    pub accepting: bool,
}
impl Listener {
    pub(crate) fn owns_mapping(&self, mapping: &crate::nat::Mapping) -> bool {
        self.0
            .state
            .borrow()
            .mapping
            .ptr_eq(&Rc::downgrade(&mapping.state))
    }
    pub(crate) fn admission_changed(&self) -> Rc<Notify> {
        self.0.state.borrow().changed.clone()
    }
    pub fn identity(&self) -> [u8; 32] {
        self.0.identity.public_key()
    }
    /// Run on a Tokio LocalSet. No unconfigured inbound connection is allocated.
    pub async fn bind(
        address: SocketAddr,
        identity: Rc<Identity>,
        limits: Limits,
    ) -> io::Result<Self> {
        limits.validate()?;
        let socket = Rc::new(UdpSocket::bind(address).await?.into());
        Ok(Self::start(socket, identity, limits))
    }
    /// Discover this listener's public mapping before starting packet routing.
    /// The same bound socket serves discovery, rendezvous and Native sessions.
    pub async fn bind_discovered(
        address: SocketAddr,
        identity: Rc<Identity>,
        limits: Limits,
        stun_server: SocketAddr,
    ) -> io::Result<(Self, SocketAddr)> {
        limits.validate()?;
        let socket = UdpSocket::bind(address).await?;
        let observed = crate::nat::discover(&socket, stun_server, Duration::from_secs(3)).await?;
        Ok((
            Self::start(Rc::new(socket.into()), identity, limits),
            observed,
        ))
    }
    pub(crate) async fn punch(&self, peer: SocketAddr) -> io::Result<()> {
        if self.0.state.borrow().closed {
            return Err(closed());
        }
        crate::nat::punch(&self.0.socket, peer).await
    }
    /// Start one periodic STUN refresh service on this listener's exact socket.
    /// Refresh packets share the existing receive driver with Native routes.
    /// Keep the returned owner alive; close/drop stops probes. Admission drain
    /// preserves refreshes until close(), so established routes can drain.
    pub fn maintain_mapping(
        &self,
        server: SocketAddr,
        options: crate::nat::MappingOptions,
    ) -> io::Result<crate::nat::Mapping> {
        let mapping = crate::nat::MappingState::new(server, options)?;
        let mut state = self.0.state.borrow_mut();
        if state.closed || !state.accepting {
            return Err(closed());
        }
        if state.mapping.upgrade().is_some_and(|m| !m.stopped()) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "STUN maintenance already active",
            ));
        }
        state.mapping = Rc::downgrade(&mapping);
        Ok(crate::nat::Mapping::new(mapping))
    }
    pub(crate) fn start(
        socket: Rc<DatagramSocket>,
        identity: Rc<Identity>,
        limits: Limits,
    ) -> Self {
        let state = Rc::new(RefCell::new(Registry {
            closed: false,
            accepting: true,
            routes: BTreeMap::new(),
            issued: BTreeSet::new(),
            aliases: BTreeMap::new(),
            reservations: 0,
            mapping: Weak::new(),
            changed: Rc::new(Notify::new()),
        }));
        let guard = Guard(state.clone());
        let input = socket.clone();
        let task = tokio::task::spawn_local(async move {
            let _guard = guard;
            let mut packet = vec![0; MAX_PACKET_BYTES + 1];
            let mut timer =
                tokio::time::interval(limits.reservation_timeout.min(Duration::from_millis(100)));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let mapping = _guard.0.borrow().mapping.upgrade();
                if let Some(mapping) = mapping {
                    mapping.tick(&input, Instant::now());
                }
                tokio::select! {
                    biased;
                    _ = timer.tick() => expire(&_guard.0,Instant::now()),
                    result = input.recv_from(&mut packet) => {
                        let (n,from) = match result {
                            Ok(packet) => packet,
                            Err(error) if transient_datagram_error(&error) => {
                                let mapping = _guard.0.borrow().mapping.upgrade();
                                if let Some(mapping) = mapping { mapping.socket_error(Instant::now()); }
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        if crate::nat::is_binding_message(&packet[..n]) {
                            let mapping = _guard.0.borrow().mapping.upgrade();
                            if let Some(mapping) = mapping { mapping.receive(&packet[..n],from,Instant::now()); }
                        } else {
                            dispatch(&_guard.0,&packet[..n],from,Instant::now());
                        }
                    },
                }
            }
        });
        Self(Rc::new(Owner {
            identity,
            socket,
            state,
            limits,
            task,
        }))
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.socket.local_addr()
    }
    pub fn stats(&self) -> Stats {
        expire(&self.0.state, Instant::now());
        let s = self.0.state.borrow();
        Stats {
            pending: s.routes.values().filter(|r| r.expires.is_some()).count(),
            authenticated: s.routes.values().filter(|r| r.expires.is_none()).count(),
            issued: s.reservations,
            connection_ids: s.issued.len(),
            closed: s.closed,
            accepting: s.accepting,
        }
    }
    /// Retire pending reservations and refuse new ones while preserving the
    /// packet routes of authenticated sessions. Drain their RPC networks with
    /// Handle::shutdown_all(), then call close(). Repeated calls are harmless.
    pub fn stop_accepting(&self) {
        let (retired, changed) = {
            let mut state = self.0.state.borrow_mut();
            state.accepting = false;
            let ids: Vec<_> = state
                .routes
                .iter()
                .filter_map(|(id, route)| route.expires.map(|_| *id))
                .collect();
            state.aliases.retain(|_, owner| !ids.contains(owner));
            let retired = ids
                .into_iter()
                .filter_map(|id| state.routes.remove(&id))
                .collect::<Vec<_>>();
            (retired, state.changed.clone())
        };
        drop(retired);
        changed.notify_waiters();
    }
    pub fn close(&self) {
        close(&self.0.state);
        self.0.task.abort();
    }
    /// Reserve one connection for a pinned identity. PSK/context must also be
    /// supplied to connect(). The reservation timeout starts at this call,
    /// including time before accept() is polled. Retired IDs are never reused.
    pub fn reserve(
        &self,
        peer: [u8; 32],
        psk: Option<[u8; 32]>,
        context: &[u8],
    ) -> io::Result<Reservation> {
        self.reserve_at(peer, psk, context, Instant::now())
    }
    fn reserve_at(
        &self,
        peer: [u8; 32],
        psk: Option<[u8; 32]>,
        context: &[u8],
        now: Instant,
    ) -> io::Result<Reservation> {
        use ring::rand::SecureRandom;
        if peer == self.0.identity.public_key()
            || peer == [0; 32]
            || context.len() > MAX_CONTEXT_BYTES
        {
            return Err(invalid("invalid reserved peer/context"));
        }
        expire(&self.0.state, now);
        let mut state = self.0.state.borrow_mut();
        if state.closed || !state.accepting {
            return Err(closed());
        }
        if state.routes.len() >= self.0.limits.routes || state.issued.len() >= MAX_ISSUED {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Native listener reservation limit",
            ));
        }
        let mut id = [0; 16];
        loop {
            ring::rand::SystemRandom::new()
                .fill(&mut id)
                .map_err(|_| io::Error::other("OS randomness unavailable"))?;
            if state.issued.insert(id) {
                break;
            }
        }
        let deadline = now + self.0.limits.reservation_timeout;
        state.reservations += 1;
        state.aliases.insert(id, id);
        let signal = Rc::new(Notify::new());
        let shutdown = Rc::new(Notify::new());
        state.routes.insert(
            id,
            Route {
                queue: VecDeque::new(),
                queued_bytes: 0,
                capacity: self.0.limits.queue,
                signal: signal.clone(),
                shutdown: shutdown.clone(),
                expires: Some(deadline),
            },
        );
        let target = Target {
            address: self.local_addr()?,
            host: self.0.identity.public_key(),
            connection_id: id,
        };
        let shared = SharedSocket {
            socket: self.0.socket.clone(),
            state: Rc::downgrade(&self.0.state),
            id,
            signal,
            shutdown,
        };
        Ok(Reservation {
            target,
            peer,
            psk,
            context: context.to_vec(),
            deadline,
            identity: self.0.identity.clone(),
            socket: Some(shared),
        })
    }
}
pub struct Reservation {
    target: Target,
    peer: [u8; 32],
    psk: Option<[u8; 32]>,
    context: Vec<u8>,
    deadline: Instant,
    identity: Rc<Identity>,
    socket: Option<SharedSocket>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.psk.zeroize();
    }
}
impl Reservation {
    pub fn target(&self) -> Target {
        self.target
    }
    /// Consuming, cancel-safe accept. Only a completed pinned Native handshake
    /// and encrypted native stream preface can produce AuthenticatedSession.
    pub async fn accept(mut self) -> io::Result<AuthenticatedSession> {
        let shared = self.socket.take().expect("single accept");
        let state = shared.state.clone();
        let binding = binding(self.target.connection_id, &self.context);
        let mut config = transport::config(&self.identity, self.peer, self.psk, &binding)
            .map_err(transport::error)?;
        let id = self.target.connection_id;
        let mut socket = PacketSocket::Shared(shared);
        let local = socket.local_addr()?;
        let accept = async {
            let mut packet = vec![0; MAX_PACKET_BYTES];
            let (n, remote) = loop {
                let (n, remote) = socket.recv_from(&mut packet).await?;
                let Ok(header) = quiche::Header::from_slice(&mut packet[..n], 16) else {
                    continue;
                };
                if header.ty == quiche::Type::Initial
                    && quiche::version_is_supported(header.version)
                    && header.dcid.as_ref() == id
                {
                    break (n, remote);
                }
            };
            let mut conn = quiche::accept(
                &quiche::ConnectionId::from_ref(&id),
                None,
                local,
                remote,
                &mut config,
            )
            .map_err(transport::error)?;
            conn.recv(
                &mut packet[..n],
                quiche::RecvInfo {
                    from: remote,
                    to: local,
                },
            )
            .map_err(transport::error)?;
            let session = transport::authenticated(
                socket,
                Box::new(conn),
                self.identity.public_key(),
                self.peer,
            )
            .await?;
            let registry = state.upgrade().ok_or_else(closed)?;
            authenticated(&registry, id, Instant::now())?;
            Ok(session)
        };
        tokio::time::timeout_at(self.deadline, accept).await?
    }
}
pub(crate) fn binding(id: [u8; 16], context: &[u8]) -> Vec<u8> {
    let mut binding = b"ReProto native RPC v1\0ReProto shared listener v1\0".to_vec();
    binding.extend_from_slice(&id);
    binding.extend_from_slice(context);
    binding
}
/// Connect to an application-provisioned target. The target's address is routing
/// data; host identity, reservation ID and context are bound by Native.
pub async fn connect(
    socket: UdpSocket,
    target: Target,
    identity: &Identity,
    psk: Option<[u8; 32]>,
    context: &[u8],
) -> io::Result<AuthenticatedSession> {
    connect_for_version(
        socket,
        target,
        identity,
        psk,
        context,
        transport::QuicVersion::V1,
    )
    .await
}
/// Dial a shared reservation with an explicitly selected QUIC version.
pub async fn connect_for_version(
    socket: UdpSocket,
    target: Target,
    identity: &Identity,
    psk: Option<[u8; 32]>,
    context: &[u8],
    version: transport::QuicVersion,
) -> io::Result<AuthenticatedSession> {
    connect_socket_version(socket.into(), target, identity, psk, context, version).await
}
#[cfg(test)]
pub(crate) async fn connect_socket(
    socket: DatagramSocket,
    target: Target,
    identity: &Identity,
    psk: Option<[u8; 32]>,
    context: &[u8],
) -> io::Result<AuthenticatedSession> {
    connect_socket_version(
        socket,
        target,
        identity,
        psk,
        context,
        transport::QuicVersion::V1,
    )
    .await
}
async fn connect_socket_version(
    socket: DatagramSocket,
    target: Target,
    identity: &Identity,
    psk: Option<[u8; 32]>,
    context: &[u8],
    version: transport::QuicVersion,
) -> io::Result<AuthenticatedSession> {
    if target.address.ip().is_unspecified()
        || target.address.port() == 0
        || target.host == identity.public_key()
        || context.len() > MAX_CONTEXT_BYTES
    {
        return Err(invalid("invalid reserved Native destination"));
    }
    let mut config = transport::config_for_version(
        identity,
        target.host,
        psk,
        &binding(target.connection_id, context),
        version,
    )
    .map_err(transport::error)?;
    let conn = quiche::connect_with_dcid(
        None,
        &quiche::ConnectionId::from_ref(&transport::cid()),
        &quiche::ConnectionId::from_ref(&target.connection_id),
        socket.local_addr()?,
        target.address,
        &mut config,
    )
    .map_err(transport::error)?;
    transport::authenticated(
        PacketSocket::Dedicated(socket),
        Box::new(conn),
        identity.public_key(),
        target.host,
    )
    .await
}

pub(crate) struct SharedSocket {
    socket: Rc<DatagramSocket>,
    state: Weak<RefCell<Registry>>,
    id: [u8; 16],
    signal: Rc<Notify>,
    shutdown: Rc<Notify>,
}
impl Drop for SharedSocket {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let retired = {
                let mut state = state.borrow_mut();
                state.aliases.retain(|_, owner| *owner != self.id);
                state.routes.remove(&self.id)
            };
            drop(retired);
        }
    }
}
impl SharedSocket {
    pub(crate) fn prepare(&self) -> io::Result<()> {
        self.check_open()?;
        self.socket.prepare()
    }
    pub(crate) async fn send_segments(
        &self,
        sender: &crate::rpc::packet_batch::Sender,
        bytes: &[u8],
        segment: usize,
        to: SocketAddr,
    ) -> io::Result<()> {
        self.check_open()?;
        self.socket.send_segments(sender, bytes, segment, to).await
    }
    pub(crate) fn register_cid(&self, id: [u8; 16]) -> io::Result<()> {
        self.check_open()?;
        let state = self.state.upgrade().ok_or_else(closed)?;
        let mut state = state.borrow_mut();
        if state.issued.len() >= MAX_ISSUED || !state.issued.insert(id) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Native listener CID budget exhausted",
            ));
        }
        state.aliases.insert(id, self.id);
        Ok(())
    }
    pub(crate) fn retire_cid(&self, id: &[u8]) {
        if let (Some(state), Ok(id)) = (self.state.upgrade(), <[u8; 16]>::try_from(id)) {
            let mut state = state.borrow_mut();
            if state.aliases.get(&id) == Some(&self.id) {
                state.aliases.remove(&id);
            }
        }
    }
    pub(crate) fn stopped(&self) -> futures::future::LocalBoxFuture<'static, ()> {
        let state = self.state.clone();
        let id = self.id;
        let shutdown = self.shutdown.clone();
        Box::pin(async move {
            let notified = shutdown.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let active = state.upgrade().is_some_and(|s| {
                let s = s.borrow();
                !s.closed && s.routes.contains_key(&id)
            });
            if active {
                notified.await;
            }
        })
    }
    pub(crate) fn check_open(&self) -> io::Result<()> {
        let state = self.state.upgrade().ok_or_else(closed)?;
        expire(&state, Instant::now());
        let state = state.borrow();
        if state.closed || !state.routes.contains_key(&self.id) {
            Err(closed())
        } else {
            Ok(())
        }
    }
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
    pub(crate) async fn recv_from(&mut self, bytes: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let signal = self.signal.clone();
        let packet = loop {
            let notified = signal.notified();
            self.check_open()?;
            let state = self.state.upgrade().ok_or_else(closed)?;
            let packet = {
                let mut state = state.borrow_mut();
                let route = state.routes.get_mut(&self.id).ok_or_else(closed)?;
                let packet = route.queue.pop_front();
                if let Some(packet) = &packet {
                    route.queued_bytes -= packet.bytes.len();
                }
                packet
            };
            if let Some(packet) = packet {
                break packet;
            }
            notified.await;
        };
        if packet.bytes.len() > bytes.len() {
            return Err(invalid("packet receive buffer too small"));
        }
        bytes[..packet.bytes.len()].copy_from_slice(&packet.bytes);
        Ok((packet.bytes.len(), packet.from))
    }
    pub(crate) async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        self.check_open()?;
        self.socket.send_to(bytes, to).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn shared_sends_preserve_datagrams_and_reject_closed_reservations() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let listener = Listener::bind(
                    "127.0.0.1:0".parse().unwrap(),
                    Rc::new(Identity::generate()),
                    Limits::default(),
                )
                .await
                .unwrap();
                let reservation = listener.reserve([1; 32], None, b"").unwrap();
                let socket = reservation.socket.as_ref().unwrap();
                let receiver = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let to = receiver.local_addr().unwrap();
                let sender = crate::rpc::packet_batch::Sender::default();
                assert_eq!(socket.send_to(b"one", to).await.unwrap(), 3);
                socket
                    .send_segments(&sender, b"twothree", 3, to)
                    .await
                    .unwrap();
                let mut bytes = [0; 32];
                for expected in [b"one".as_slice(), b"two", b"thr", b"ee"] {
                    let (n, from) = tokio::time::timeout(
                        Duration::from_secs(2),
                        receiver.recv_from(&mut bytes),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                    assert_eq!(&bytes[..n], expected);
                    assert_eq!(from, listener.local_addr().unwrap());
                }
                listener.close();
                assert_eq!(
                    socket.send_to(b"closed", to).await.unwrap_err().kind(),
                    io::ErrorKind::BrokenPipe
                );
                assert_eq!(
                    socket
                        .send_segments(&sender, b"closed", 3, to)
                        .await
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::BrokenPipe
                );
                assert_eq!(
                    receiver.try_recv_from(&mut bytes).unwrap_err().kind(),
                    io::ErrorKind::WouldBlock
                );
            })
            .await;
    }
    fn packet(id: [u8; 16]) -> Vec<u8> {
        let mut packet = vec![0x40];
        packet.extend_from_slice(&id);
        packet.extend_from_slice(b"packet");
        packet
    }
    fn queued(listener: &Listener, id: [u8; 16]) -> usize {
        listener
            .0
            .state
            .borrow()
            .routes
            .get(&id)
            .map_or(0, |r| r.queue.len())
    }
    #[tokio::test(flavor = "current_thread")]
    async fn large_packet_queue_obeys_byte_budget_and_reuses_drained_space() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let listener = Listener::bind(
                    "127.0.0.1:0".parse().unwrap(),
                    Rc::new(Identity::generate()),
                    Limits::default(),
                )
                .await
                .unwrap();
                let mut slot = listener.reserve([1; 32], None, b"").unwrap();
                let id = slot.target().connection_id;
                let mut bytes = packet(id);
                bytes.resize(MAX_PACKET_BYTES, 0);
                let from = "127.0.0.1:12345".parse().unwrap();
                let capacity = MAX_QUEUED_BYTES / MAX_PACKET_BYTES;
                for _ in 0..capacity + 1 {
                    dispatch(&listener.0.state, &bytes, from, Instant::now());
                }
                assert_eq!(queued(&listener, id), capacity);
                let mut buffer = vec![0; MAX_PACKET_BYTES];
                for _ in 0..capacity {
                    let (n, source) = slot
                        .socket
                        .as_mut()
                        .unwrap()
                        .recv_from(&mut buffer)
                        .await
                        .unwrap();
                    assert_eq!(n, bytes.len());
                    assert_eq!(source, from);
                    assert_eq!(buffer, bytes);
                }
                assert_eq!(queued(&listener, id), 0);
                for _ in 0..capacity {
                    dispatch(&listener.0.state, &bytes, from, Instant::now());
                }
                assert_eq!(queued(&listener, id), capacity);
                assert_eq!(
                    listener.0.state.borrow().routes[&id].queued_bytes,
                    MAX_QUEUED_BYTES
                );
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn parser_queue_lifetime_limits_and_expiry_release_buffers() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let owner = Rc::new(Identity::generate());
                let listener = Listener::bind(
                    "127.0.0.1:0".parse().unwrap(),
                    owner.clone(),
                    Limits {
                        routes: 2,
                        queue: 2,
                        reservation_timeout: Duration::from_secs(1),
                    },
                )
                .await
                .unwrap();
                let now = Instant::now() + Duration::from_secs(3600);
                let first = listener.reserve_at([1; 32], None, b"", now).unwrap();
                let mut second = listener.reserve_at([2; 32], None, b"", now).unwrap();
                let (a, b) = (first.target().connection_id, second.target().connection_id);
                let from = "127.0.0.1:12345".parse().unwrap();
                for _ in 0..3 {
                    dispatch(&listener.0.state, &packet(a), from, now);
                }
                assert_eq!(queued(&listener, a), 2);
                assert_eq!(queued(&listener, b), 0);
                dispatch(&listener.0.state, &packet(b), from, now);
                let mut buffer = [0; MAX_PACKET_BYTES];
                let (n, source) = second
                    .socket
                    .as_mut()
                    .unwrap()
                    .recv_from(&mut buffer)
                    .await
                    .unwrap();
                assert_eq!(&buffer[..n], packet(b));
                assert_eq!(source, from);
                assert_eq!(queued(&listener, a), 2);
                assert_eq!(queued(&listener, b), 0);
                // Malformed and oversized frames allocate no route or packet queue.
                dispatch(&listener.0.state, b"bad", from, now);
                dispatch(&listener.0.state, &vec![0; MAX_PACKET_BYTES + 1], from, now);
                assert_eq!(listener.stats().issued, 2);
                authenticated(&listener.0.state, b, now).unwrap();
                expire(&listener.0.state, now + Duration::from_secs(1));
                assert_eq!(queued(&listener, a), 0);
                assert!(first.socket.as_ref().unwrap().check_open().is_err());
                assert!(second.socket.as_ref().unwrap().check_open().is_ok());
                assert!(authenticated(&listener.0.state, a, now + Duration::from_secs(1)).is_err());
                let new = listener
                    .reserve_at([3; 32], None, b"", now + Duration::from_secs(1))
                    .unwrap();
                assert_ne!(new.target().connection_id, a);
                drop(first);
                assert_eq!(listener.stats().pending, 1);
                assert_eq!(listener.stats().authenticated, 1);
                drop(new);
                drop(second);
                for _ in 3..MAX_ISSUED {
                    drop(listener.reserve_at([1; 32], None, b"", now).unwrap());
                }
                assert_eq!(listener.stats().issued, MAX_ISSUED);
                assert!(listener.reserve_at([1; 32], None, b"", now).is_err());
                assert!(listener.reserve(owner.public_key(), None, b"").is_err());
                assert!(listener
                    .reserve([1; 32], None, &vec![0; MAX_CONTEXT_BYTES + 1])
                    .is_err());
                listener.close();
                assert!(listener.reserve([1; 32], None, b"").is_err());
            })
            .await;
    }
    thread_local! {
        static INSPECT: RefCell<Option<Weak<RefCell<Registry>>>> = const { RefCell::new(None) };
    }
    struct InspectWake(std::sync::atomic::AtomicUsize);
    impl std::task::Wake for InspectWake {
        fn wake(self: std::sync::Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            INSPECT.with(|state| {
                let state = state.borrow().as_ref().unwrap().upgrade().unwrap();
                // Would panic if queue delivery/removal woke us under a borrow.
                let _ = state.borrow_mut().routes.len();
            });
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn packet_and_cleanup_wakers_can_reenter_registry() {
        use std::{
            future::Future,
            sync::{atomic::Ordering, Arc},
            task::{Context, Poll},
        };
        tokio::task::LocalSet::new()
            .run_until(async {
                let listener = Listener::bind(
                    "127.0.0.1:0".parse().unwrap(),
                    Rc::new(Identity::generate()),
                    Limits::default(),
                )
                .await
                .unwrap();
                INSPECT.with(|state| *state.borrow_mut() = Some(Rc::downgrade(&listener.0.state)));
                let mut slot = listener.reserve([1; 32], None, b"").unwrap();
                let id = slot.target().connection_id;
                let signal = Arc::new(InspectWake(std::sync::atomic::AtomicUsize::new(0)));
                let waker = std::task::Waker::from(signal.clone());
                let mut context = Context::from_waker(&waker);
                let mut bytes = [0; MAX_PACKET_BYTES];
                {
                    let mut read = Box::pin(slot.socket.as_mut().unwrap().recv_from(&mut bytes));
                    assert!(read.as_mut().poll(&mut context).is_pending());
                    dispatch(
                        &listener.0.state,
                        &packet(id),
                        "127.0.0.1:1234".parse().unwrap(),
                        Instant::now(),
                    );
                    assert_eq!(signal.0.load(Ordering::SeqCst), 1);
                    assert!(matches!(
                        read.as_mut().poll(&mut context),
                        Poll::Ready(Ok(_))
                    ));
                }
                let mut stopped = slot.socket.as_ref().unwrap().stopped();
                assert!(stopped.as_mut().poll(&mut context).is_pending());
                {
                    let mut read = Box::pin(slot.socket.as_mut().unwrap().recv_from(&mut bytes));
                    assert!(read.as_mut().poll(&mut context).is_pending());
                    listener.close();
                    assert_eq!(signal.0.load(Ordering::SeqCst), 3);
                    assert!(matches!(
                        read.as_mut().poll(&mut context),
                        Poll::Ready(Err(_))
                    ));
                }
                assert!(stopped.as_mut().poll(&mut context).is_ready());
                assert!(slot
                    .socket
                    .as_ref()
                    .unwrap()
                    .stopped()
                    .as_mut()
                    .poll(&mut context)
                    .is_ready());
                drop(slot);
                INSPECT.with(|state| state.borrow_mut().take());
            })
            .await;
    }
    #[derive(serde::Deserialize)]
    struct Trace {
        slots: usize,
        queue: usize,
        steps: Vec<Step>,
    }
    #[derive(serde::Deserialize)]
    struct Step {
        action: String,
        item: usize,
        state: Vec<u64>,
    }
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_listener_traces() {
        let path = capntproto_test_support::verification::input("CAPNTPROTO_LISTENER_TRACES")
            .expect("run this test through its verification driver");
        let cases: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(!cases.is_empty());
        tokio::task::LocalSet::new()
            .run_until(async {
                let owner = Rc::new(Identity::generate());
                // Identity verification is exercised by bind() in integration
                // tests. Each trace needs a fresh registry, not another DH.
                let socket: Rc<DatagramSocket> =
                    Rc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap().into());
                for (index, trace) in cases.into_iter().enumerate() {
                    let listener = Listener::start(
                        socket.clone(),
                        owner.clone(),
                        Limits {
                            routes: trace.slots,
                            queue: trace.queue,
                            reservation_timeout: Duration::from_secs(1),
                        },
                    );
                    let base = Instant::now() + Duration::from_secs(3600);
                    let mut time = 0;
                    let mut slots: [Option<Reservation>; 2] = [None, None];
                    let mut ids = [[0; 16]; 2];
                    let mut issued = [false; 2];
                    let mut valid = [false; 2];
                    let from = "127.0.0.1:12345".parse().unwrap();
                    for step in trace.steps {
                        let i = step.item.saturating_sub(1);
                        let now = base + Duration::from_secs(time);
                        match step.action.as_str() {
                            "reserve" => {
                                match listener.reserve_at([i as u8 + 1; 32], None, b"trace", now) {
                                    Ok(slot) => {
                                        assert!(!issued[i]);
                                        ids[i] = slot.target().connection_id;
                                        issued[i] = true;
                                        slots[i] = Some(slot);
                                    }
                                    Err(e) => assert_eq!(
                                        e.kind(),
                                        if listener.stats().accepting {
                                            io::ErrorKind::WouldBlock
                                        } else {
                                            io::ErrorKind::BrokenPipe
                                        }
                                    ),
                                }
                            }
                            "enqueue" => dispatch(&listener.0.state, &packet(ids[i]), from, now),
                            "read" => {
                                let mut buffer = [0; MAX_PACKET_BYTES];
                                let (n, source) = slots[i]
                                    .as_mut()
                                    .unwrap()
                                    .socket
                                    .as_mut()
                                    .unwrap()
                                    .recv_from(&mut buffer)
                                    .await
                                    .unwrap();
                                assert_eq!(&buffer[..n], packet(ids[i]));
                                assert_eq!(source, from);
                            }
                            "authenticate" => {
                                authenticated(&listener.0.state, ids[i], now).unwrap();
                                valid[i] = true;
                            }
                            "reject" | "release" => drop(slots[i].take()),
                            "tick" => {
                                time += 1;
                                expire(&listener.0.state, base + Duration::from_secs(time));
                            }
                            "unknown" => {
                                dispatch(&listener.0.state, b"bad", from, now);
                                dispatch(&listener.0.state, &packet([0xff; 16]), from, now);
                            }
                            "late_authenticate" => {
                                assert!(authenticated(&listener.0.state, ids[i], now).is_err())
                            }
                            "close" => listener.close(),
                            "stop" => listener.stop_accepting(),
                            action => panic!("unexpected {action}"),
                        }
                        let state = listener.0.state.borrow();
                        let phase =
                            |i: usize| {
                                state.routes.get(&ids[i]).map_or(
                                    if issued[i] { 3 } else { 0 },
                                    |r| if r.expires.is_some() { 1 } else { 2 },
                                )
                            };
                        let count = |i: usize| {
                            state
                                .routes
                                .get(&ids[i])
                                .map_or(0, |r| r.queue.len() as u64)
                        };
                        let deadline = |i: usize| {
                            state
                                .routes
                                .get(&ids[i])
                                .and_then(|r| r.expires)
                                .map_or(0, |t| t.duration_since(base).as_secs())
                        };
                        let actual = vec![
                            phase(0),
                            phase(1),
                            count(0),
                            count(1),
                            deadline(0),
                            deadline(1),
                            valid[0] as u64,
                            valid[1] as u64,
                            time,
                            state.closed as u64,
                            (!state.accepting) as u64,
                            0,
                            0,
                        ];
                        assert_eq!(
                            actual, step.state,
                            "trace {index} {} {}",
                            step.action, step.item
                        );
                    }
                    drop(slots);
                    drop(listener);
                    tokio::task::yield_now().await;
                }
            })
            .await;
    }
}

//! Bounded UDP demultiplexing. Session ownership is independent of the listener.
use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, Mutex, Notify},
};

const MAX_CONNECTIONS: usize = 1024;
// All locally issued connection IDs come from transport::cid().
const LOCAL_CID_LEN: usize = 16;
const MAX_PENDING: usize = 64;
pub(super) struct Packet {
    pub bytes: Vec<u8>,
    pub from: SocketAddr,
}
#[derive(Clone)]
enum Destination {
    Pending(mpsc::Sender<Packet>),
    Active(Weak<driver::State>),
}
type Routes = Rc<RefCell<HashMap<Vec<u8>, Destination>>>;
/// A shared UDP endpoint. Construct and poll within a Tokio LocalSet.
pub struct Endpoint(pub(super) Rc<Core>);
pub(super) struct Core {
    pub socket: Rc<UdpSocket>,
    pub local: SocketAddr,
    pub client: RefCell<Option<ClientConfig>>,
    pub server: RefCell<Option<ServerConfig>>,
    routes: Routes,
    tokens: retry::Tokens,
    accept_lock: Mutex<()>,
    incoming: RefCell<mpsc::Receiver<Incoming>>,
    pub admitting: Cell<bool>,
    changed: Rc<Notify>,
    router: RefCell<Option<tokio::task::JoinHandle<()>>>,
}
impl Drop for Core {
    fn drop(&mut self) {
        if let Some(task) = self.router.get_mut().take() {
            task.abort();
        }
    }
}
impl Endpoint {
    pub fn client(address: SocketAddr) -> io::Result<Self> {
        Self::new(address, None)
    }
    pub fn server(config: ServerConfig, address: SocketAddr) -> io::Result<Self> {
        Self::new(address, Some(config))
    }
    fn new(address: SocketAddr, server: Option<ServerConfig>) -> io::Result<Self> {
        let socket = std::net::UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        let socket = UdpSocket::from_std(socket)?;
        crate::rpc::packet_mtu::prepare(&socket)?;
        let (tx, rx) = mpsc::channel(MAX_PENDING);
        let core = Rc::new(Core {
            local: socket.local_addr()?,
            socket: Rc::new(socket),
            client: RefCell::new(None),
            server: RefCell::new(server),
            routes: Rc::new(RefCell::new(HashMap::new())),
            tokens: retry::Tokens::new(),
            accept_lock: Mutex::new(()),
            incoming: RefCell::new(rx),
            admitting: Cell::new(true),
            changed: Rc::new(Notify::new()),
            router: RefCell::new(None),
        });
        *core.router.borrow_mut() = Some(tokio::task::spawn_local(route(
            Rc::downgrade(&core),
            core.socket.clone(),
            tx,
        )));
        Ok(Self(core))
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.socket.local_addr()
    }
    pub fn set_default_client_config(&mut self, config: ClientConfig) {
        *self.0.client.borrow_mut() = Some(config);
    }
    /// Canceling this future leaves queued incoming sessions available.
    /// Concurrent calls are served in order without replacing each other's wakeups.
    pub async fn accept(&self) -> Option<Incoming> {
        // A channel receiver registers one task waker. Keep other acceptors on
        // the cancellation-safe lock queue until that receiver is available.
        let _accept = self.0.accept_lock.lock().await;
        futures::future::poll_fn(|cx| self.0.incoming.borrow_mut().poll_recv(cx)).await
    }
    /// Wait until every admitted or pending session has released its route.
    pub async fn wait_idle(&self) {
        loop {
            let notified = self.0.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.0.routes.borrow().is_empty() {
                return;
            }
            notified.await;
        }
    }
    pub(super) fn stop_accepting(&self) {
        self.0.admitting.set(false);
        // Drop after releasing the receiver borrow: a route destructor wakes waiters.
        loop {
            let next = self.0.incoming.borrow_mut().try_recv().ok();
            if next.is_none() {
                break;
            }
            drop(next);
        }
    }
}
/// An untrusted connection awaiting TLS authentication. Dropping it retires its route.
pub struct Incoming {
    pub(super) core: Weak<Core>,
    pub(super) route: Route,
    pub(super) connection: Box<quiche::Connection>,
    peer: SocketAddr,
}
impl Incoming {
    pub fn remote_address(&self) -> SocketAddr {
        self.peer
    }
}
pub(super) struct Route {
    pub packets: mpsc::Receiver<Packet>,
    keys: Vec<Vec<u8>>,
    routes: Routes,
    changed: Rc<Notify>,
}
impl Route {
    pub fn activate(&mut self, state: &Rc<driver::State>, local: SocketAddr) {
        // Admission remains bounded until accept starts the session. Activation
        // cannot race the router: neither draining nor replacing routes awaits.
        while let Ok(mut packet) = self.packets.try_recv() {
            state.receive(&mut packet.bytes, packet.from, local);
        }
        let mut routes = self.routes.borrow_mut();
        for key in &self.keys {
            *routes.get_mut(key).expect("registered QUIC route") =
                Destination::Active(Rc::downgrade(state));
        }
        self.packets.close();
    }
}
impl Drop for Route {
    fn drop(&mut self) {
        {
            let mut routes = self.routes.borrow_mut();
            for key in &self.keys {
                routes.remove(key);
            }
        }
        self.changed.notify_waiters();
    }
}
pub(super) fn register(core: &Core, keys: Vec<Vec<u8>>) -> io::Result<Route> {
    let mut routes = core.routes.borrow_mut();
    if routes.len() + keys.len() > MAX_CONNECTIONS * 2
        || keys.iter().any(|key| routes.contains_key(key))
    {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "QUIC endpoint route limit",
        ));
    }
    let (sender, packets) = mpsc::channel(64);
    for key in &keys {
        routes.insert(key.clone(), Destination::Pending(sender.clone()));
    }
    Ok(Route {
        packets,
        keys,
        routes: core.routes.clone(),
        changed: core.changed.clone(),
    })
}
async fn route(core: Weak<Core>, socket: Rc<UdpSocket>, incoming: mpsc::Sender<Incoming>) {
    let mut bytes = vec![0; 65535];
    let mut received = 0;
    loop {
        // Decryption now runs here too: bound each turn so another session,
        // application reader or recovery timer can run under sustained ingress.
        if received == 16 {
            tokio::task::yield_now().await;
            received = 0;
        }
        let received_packet = futures::future::poll_fn(|cx| {
            let mut buffer = ReadBuf::new(&mut bytes);
            match socket.poll_recv_from(cx, &mut buffer) {
                Poll::Ready(Ok(from)) => Poll::Ready(Ok((buffer.filled().len(), from))),
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => {
                    // A socket wait already yielded this task. Start a fresh
                    // budget for its next burst, not the preceding flight.
                    received = 0;
                    Poll::Pending
                }
            }
        })
        .await;
        let (n, from) = match received_packet {
            Ok(packet) => packet,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => break,
        };
        received += 1;
        let Some(core) = core.upgrade() else {
            return;
        };
        let Some(first) = bytes[..n].first().copied() else {
            continue;
        };
        // Established packets only need the destination ID for routing. Its
        // bytes are outside header protection; quiche still authenticates and
        // validates the complete packet in receive(). Avoid its owned Header
        // parser here, which would copy this ID before recv() parses it again.
        let header = if first & 0x80 == 0 {
            None
        } else {
            let Ok(header) = quiche::Header::from_slice(&mut bytes[..n], LOCAL_CID_LEN) else {
                continue;
            };
            Some(header)
        };
        let dcid = match &header {
            Some(header) => header.dcid.as_ref(),
            None => {
                let Some(dcid) = bytes[..n].get(1..1 + LOCAL_CID_LEN) else {
                    continue;
                };
                dcid
            }
        };
        let destination = core.routes.borrow().get(dcid).cloned();
        if let Some(destination) = destination {
            match destination {
                Destination::Pending(sender) => {
                    let _ = sender.try_send(Packet {
                        bytes: bytes[..n].to_vec(),
                        from,
                    });
                }
                Destination::Active(state) => {
                    if let Some(state) = state.upgrade() {
                        // Quiche consumes the socket buffer in place. No packet
                        // allocation, channel hop or second scheduling turn.
                        state.receive(&mut bytes[..n], from, core.local);
                    }
                }
            }
            continue;
        }
        let Some(header) = header else {
            // Unknown short-header destinations cannot admit a connection.
            continue;
        };
        if !core.admitting.get()
            || header.ty != quiche::Type::Initial
            || n < 1200
            || !quiche::version_is_supported(header.version)
            || header.dcid.len() < 8
        {
            continue;
        }
        let requires_retry = match core.server.borrow().as_ref() {
            Some(config) => config.1,
            None => continue,
        };
        let (id, original) = if requires_retry {
            let token = header.token.as_deref().unwrap_or_default();
            if token.is_empty() {
                let id = crate::transport::cid();
                let token = core.tokens.mint(from, header.version, &header.dcid, &id);
                let mut out = [0; 1350];
                if let Ok(n) = quiche::retry(
                    &header.scid,
                    &header.dcid,
                    &quiche::ConnectionId::from_ref(&id),
                    &token,
                    header.version,
                    &mut out,
                ) {
                    let _ = socket.send_to(&out[..n], from).await;
                }
                continue;
            }
            let Some(original) = core
                .tokens
                .validate(from, header.version, &header.dcid, token)
            else {
                continue;
            };
            let Ok(id) = <[u8; 16]>::try_from(header.dcid.as_ref()) else {
                continue;
            };
            (id, Some(original.to_vec()))
        } else {
            (crate::transport::cid(), None)
        };
        let Ok(permit) = incoming.try_reserve() else {
            continue;
        };
        let mut keys = vec![id.to_vec()];
        if header.dcid.as_ref() != id {
            keys.push(header.dcid.to_vec());
        }
        let Ok(route) = register(&core, keys) else {
            continue;
        };
        let Ok(local) = socket.local_addr() else {
            return;
        };
        let mut config = core.server.borrow_mut();
        let Some(config) = config.as_mut() else {
            continue;
        };
        let Ok(mut connection) = quiche::accept(
            &quiche::ConnectionId::from_ref(&id),
            original
                .as_deref()
                .map(quiche::ConnectionId::from_ref)
                .as_ref(),
            local,
            from,
            &mut config.0,
        ) else {
            continue;
        };
        // Authentication failures are reported by accept(), not admitted as RPC.
        if connection
            .recv(&mut bytes[..n], quiche::RecvInfo { from, to: local })
            .is_err()
        {
            continue;
        }
        permit.send(Incoming {
            core: Rc::downgrade(&core),
            route,
            connection: Box::new(connection),
            peer: from,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn short_header_routing_matches_quiche_and_ignores_unknown_or_truncated_packets() {
        tokio::task::LocalSet::new()
            .run_until(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
                    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                    let address = endpoint.local_addr().unwrap();
                    let id = crate::transport::cid();
                    assert_eq!(id.len(), LOCAL_CID_LEN);
                    let mut route = register(&endpoint.0, vec![id.to_vec()]).unwrap();
                    let mut packet = vec![0];
                    packet.extend_from_slice(&id);

                    // Header form is the only flag used for demultiplexing.
                    // Reserved/fixed/key-phase bits remain quiche's concern.
                    for first in 0..128 {
                        packet[0] = first;
                        let header =
                            quiche::Header::from_slice(&mut packet, LOCAL_CID_LEN).unwrap();
                        assert_eq!(header.ty, quiche::Type::Short);
                        assert_eq!(header.dcid.as_ref(), id);
                        peer.send_to(&packet, address).await.unwrap();
                        let received = route.packets.recv().await.unwrap();
                        assert_eq!(received.bytes, packet);
                        assert_eq!(received.from, peer.local_addr().unwrap());
                    }

                    for length in 0..packet.len() {
                        assert!(
                            quiche::Header::from_slice(&mut packet[..length], LOCAL_CID_LEN)
                                .is_err()
                        );
                        peer.send_to(&packet[..length], address).await.unwrap();
                    }
                    let mut unknown = packet.clone();
                    unknown[1] ^= 1;
                    peer.send_to(&unknown, address).await.unwrap();
                    // A malformed long header also cannot enter an existing
                    // route or create a pending connection.
                    peer.send_to(&[0x80], address).await.unwrap();
                    // This known packet is a socket-order barrier, not a sleep.
                    peer.send_to(&packet, address).await.unwrap();
                    assert_eq!(route.packets.recv().await.unwrap().bytes, packet);
                    assert!(matches!(
                        endpoint.0.incoming.borrow_mut().try_recv(),
                        Err(mpsc::error::TryRecvError::Empty)
                    ));
                    assert_eq!(endpoint.0.routes.borrow().len(), 1);
                    drop(route);
                    endpoint.wait_idle().await;
                })
                .await
                .unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stopped_listener_rejects_initials_but_routes_existing_packets() {
        tokio::task::LocalSet::new()
            .run_until(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let cert =
                        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
                    let config = server_config(
                        Identity {
                            certificates: vec![cert.cert.der().clone()],
                            private_key: rustls::pki_types::PrivatePkcs8KeyDer::from(
                                cert.signing_key.serialize_der(),
                            )
                            .into(),
                        },
                        None,
                    )
                    .unwrap();
                    let endpoint =
                        Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
                    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                    let address = endpoint.local_addr().unwrap();
                    let mut config = client_config(vec![cert.cert.der().clone()], None).unwrap();
                    let mut client = quiche::connect(
                        Some("localhost"),
                        &quiche::ConnectionId::from_ref(&[1; 16]),
                        peer.local_addr().unwrap(),
                        address,
                        &mut config.0,
                    )
                    .unwrap();
                    let mut initial = [0; 1350];
                    let (length, _) = client.send(&mut initial).unwrap();
                    peer.send_to(&initial[..length], address).await.unwrap();
                    drop(endpoint.accept().await.unwrap());
                    endpoint.wait_idle().await;

                    let id = vec![42; 16];
                    let mut existing = register(&endpoint.0, vec![id.clone()]).unwrap();
                    endpoint.stop_accepting();
                    peer.send_to(&initial[..length], address).await.unwrap();
                    // A known-route packet on the same socket is a processing
                    // barrier for the preceding Initial; no scheduling sleep.
                    let mut packet = vec![0x40];
                    packet.extend_from_slice(&id);
                    packet.push(0);
                    peer.send_to(&packet, address).await.unwrap();
                    assert_eq!(existing.packets.recv().await.unwrap().bytes, packet);
                    assert!(matches!(
                        endpoint.0.incoming.borrow_mut().try_recv(),
                        Err(mpsc::error::TryRecvError::Empty)
                    ));
                    assert_eq!(endpoint.0.routes.borrow().len(), 1);
                    drop(existing);
                    endpoint.wait_idle().await;
                })
                .await
                .unwrap();
            })
            .await;
    }
}

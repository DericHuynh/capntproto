//! Authenticated TCP/TLS and QUIC sessions for native capability RPC.
#[cfg(test)]
pub(crate) mod backend_tests;
pub(crate) mod buffers;
pub mod bulk;
#[cfg(test)]
mod clock_tests;
#[cfg(test)]
mod datagram_owned_tests;
mod engine;
#[cfg(test)]
mod engine_tests;
mod identity;
mod mobility;
#[cfg(test)]
mod mtu_tests;
mod scheduling;
mod shutdown;
#[cfg(test)]
mod simulation;
pub(crate) mod socket;
mod stream;
#[cfg(test)]
mod stream_planes_probe;
#[cfg(test)]
mod stream_tests;
pub mod tcp;
use crate::native_shutdown::{Control, DriverGuard, Receipt};
pub use crate::rpc::QuicVersion;
use futures::FutureExt;
pub use identity::{Identity, IdentityError};
pub use mobility::{Mobility, Path};
pub use scheduling::{DatagramPacing, Schedule, ScheduleStats, Scheduling};
use shutdown::ShutdownDriver;
use socket::DatagramIo;
use socket::DatagramSocket;
use std::{io, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    net::UdpSocket,
};

pub fn config(
    identity: &Identity,
    peer: [u8; 32],
    psk: Option<[u8; 32]>,
    context: &[u8],
) -> quiche::Result<quiche::Config> {
    config_for_version(identity, peer, psk, context, QuicVersion::V1)
}
/// Select a standard QUIC version for the quiche backend.
pub fn config_for_version(
    identity: &Identity,
    peer: [u8; 32],
    psk: Option<[u8; 32]>,
    context: &[u8],
    version: QuicVersion,
) -> quiche::Result<quiche::Config> {
    let mut c = identity.quiche_config(peer, psk, context, version)?;
    c.set_application_protos(&[b"capntproto/3"])?;
    c.set_max_idle_timeout(10_000);
    // Discover the path limit within the adapter's bound. Application packets
    // stay at QUIC's minimum until larger unfragmented probes succeed.
    c.set_max_send_udp_payload_size(crate::rpc::packet_mtu::SEND_MAX);
    c.set_max_recv_udp_payload_size(crate::rpc::packet_mtu::RECEIVE_MAX);
    c.discover_pmtu(true);
    c.set_initial_max_data(2 * 1024 * 1024);
    c.set_initial_max_stream_data_bidi_local(1024 * 1024);
    c.set_initial_max_stream_data_bidi_remote(1024 * 1024);
    c.set_initial_max_streams_bidi(64);
    c.set_initial_max_streams_uni(64);
    c.set_initial_max_stream_data_uni(128);
    c.enable_dgram(true, 64, 64);
    c.set_active_connection_id_limit(4);
    Ok(c)
}
pub(crate) fn cid() -> [u8; 16] {
    ring::rand::generate(&ring::rand::SystemRandom::new())
        .expect("OS randomness")
        .expose()
}
pub(crate) fn error(e: quiche::Error) -> io::Error {
    io::Error::other(e.to_string())
}

/// Connect one pinned peer. Returns ordered RPC IO and its network task.
/// Execute on a Tokio LocalSet alongside capnp-rpc's RpcSystem.
pub async fn connect(
    socket: UdpSocket,
    remote: SocketAddr,
    config: &mut quiche::Config,
) -> io::Result<(DuplexStream, tokio::task::JoinHandle<io::Result<()>>)> {
    let local = socket.local_addr()?;
    let conn = quiche::connect_with_buffer_factory::<buffers::Factory>(
        None,
        &quiche::ConnectionId::from_ref(&cid()),
        local,
        remote,
        config,
    )
    .map_err(error)?;
    spawn(socket, conn)
}
/// Accept one peer on a dedicated socket. Multi-connection routing is external.
pub async fn accept(
    socket: UdpSocket,
    config: &mut quiche::Config,
) -> io::Result<(DuplexStream, tokio::task::JoinHandle<io::Result<()>>)> {
    let mut buf = vec![0; 65535];
    let (n, remote) =
        tokio::time::timeout(Duration::from_secs(10), socket.recv_from(&mut buf)).await??;
    let local = socket.local_addr()?;
    let mut conn = quiche::accept_with_buf_factory::<buffers::Factory>(
        &quiche::ConnectionId::from_ref(&cid()),
        None,
        local,
        remote,
        config,
    )
    .map_err(error)?;
    conn.recv(
        &mut buf[..n],
        quiche::RecvInfo {
            from: remote,
            to: local,
        },
    )
    .map_err(error)?;
    spawn(socket, conn)
}
fn spawn(
    socket: UdpSocket,
    conn: buffers::Connection,
) -> io::Result<(DuplexStream, tokio::task::JoinHandle<io::Result<()>>)> {
    let (app, network) = tokio::io::duplex(crate::rpc::QUIC_BUFFER_BYTES);
    let (reader, writer) = tokio::io::split(network);
    Ok((
        app,
        tokio::task::spawn_local(drive(
            PacketSocket::Dedicated(DatagramSocket::new(socket)?),
            Box::new(conn),
            (stream::CopyInput(reader), stream::CopyOutput(writer)),
            SessionDrivers {
                established: None,
                datagrams: None,
                shutdown: None,
                mobility: None,
                scheduling: scheduling::pair().1,
                bulk: None,
            },
        )),
    ))
}
/// Conservative datagram payload bound, including paths at QUIC's minimum MTU.
pub const MAX_DATAGRAM_BYTES: usize = 1024;
pub const DATAGRAM_QUEUE: usize = 64;

#[cfg(feature = "services")]
mod datagram_batch;

/// A bounded, unreliable lane belonging to one authenticated session. Taking it
/// does not transfer ownership of the session or keep a disconnected route alive.
pub struct DatagramPort {
    sender: DatagramSender,
    incoming: tokio::sync::mpsc::Receiver<Vec<u8>>,
}
#[derive(Clone)]
pub struct DatagramSender(tokio::sync::mpsc::Sender<Vec<u8>>);
/// Failed local admission returns the original allocation to its owner.
#[derive(Debug)]
pub struct DatagramSendError {
    error: io::Error,
    payload: Vec<u8>,
}
impl DatagramSendError {
    /// Why local queue admission failed.
    pub fn kind(&self) -> io::ErrorKind {
        self.error.kind()
    }
    /// Recover the admission error and the original owned allocation.
    pub fn into_parts(self) -> (io::Error, Vec<u8>) {
        (self.error, self.payload)
    }
}
impl std::fmt::Display for DatagramSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for DatagramSendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
impl DatagramSender {
    /// Reserve a validated batch without publishing or waking the receiver.
    #[cfg(feature = "services")]
    pub(crate) fn reserve_batch<'a>(
        &'a self,
        packets: &'a [Vec<u8>],
    ) -> io::Result<datagram_batch::Batch<'a>> {
        datagram_batch::reserve(&self.0, packets)
    }

    /// Failure admits nothing; success still makes no delivery or execution
    /// guarantee. Callers with protocol state to commit must use reserve_batch.
    #[cfg(feature = "services")]
    pub(crate) fn try_send_batch(&self, packets: &[Vec<u8>]) -> io::Result<()> {
        self.reserve_batch(packets)?.send();
        Ok(())
    }

    /// Success means local queue admission only. Packets may be dropped later;
    /// a full queue returns WouldBlock without retaining the payload.
    pub fn try_send(&self, bytes: &[u8]) -> io::Result<()> {
        // Reserve before copying: backpressure must not allocate a discarded packet.
        self.reserve_packet(bytes.len())?.send(bytes.to_vec());
        Ok(())
    }

    /// Transfer a buffer into the bounded queue without copying its payload.
    /// Errors return the original buffer, including its capacity. Success proves
    /// only local admission; it does not keep the session alive or ensure delivery.
    pub fn try_send_owned(&self, payload: Vec<u8>) -> Result<(), DatagramSendError> {
        match self.reserve_packet(payload.len()) {
            Ok(permit) => {
                permit.send(payload);
                Ok(())
            }
            Err(error) => Err(DatagramSendError { error, payload }),
        }
    }

    fn reserve_packet(&self, length: usize) -> io::Result<tokio::sync::mpsc::Permit<'_, Vec<u8>>> {
        if length > MAX_DATAGRAM_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "datagram too large",
            ));
        }
        self.0.try_reserve().map_err(|e| match e {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "datagram queue full")
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "datagram session closed")
            }
        })
    }
}
impl DatagramPort {
    pub fn sender(&self) -> DatagramSender {
        self.sender.clone()
    }
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.incoming.recv().await
    }
}
struct DatagramDriver {
    outgoing: tokio::sync::mpsc::Receiver<Vec<u8>>,
    incoming: tokio::sync::mpsc::Sender<Vec<u8>>,
    send_closed: bool,
}
fn datagram_pair() -> (DatagramPort, DatagramDriver) {
    let (tx, outgoing) = tokio::sync::mpsc::channel(DATAGRAM_QUEUE);
    let (incoming, rx) = tokio::sync::mpsc::channel(DATAGRAM_QUEUE);
    (
        DatagramPort {
            sender: DatagramSender(tx),
            incoming: rx,
        },
        DatagramDriver {
            outgoing,
            incoming,
            send_closed: false,
        },
    )
}
// The packet engine is identical for dedicated sockets and shared-listener
// reservations. Only authenticated() can construct a published session.
pub(crate) enum PacketSocket {
    Dedicated(socket::DatagramSocket),
    Shared(crate::native_listener::SharedSocket),
}
impl PacketSocket {
    async fn flush_batch(
        &self,
        sender: &crate::rpc::packet_batch::Sender,
        batch: &mut crate::rpc::packet_batch::Batch,
    ) -> io::Result<()> {
        if let Some(info) = batch.info {
            crate::rpc::pacing::wait_until(info.at).await;
            match self {
                Self::Dedicated(s) => {
                    s.send_segments(sender, batch.bytes(), batch.segment, info.to)
                        .await?
                }
                Self::Shared(s) => {
                    s.send_segments(sender, batch.bytes(), batch.segment, info.to)
                        .await?
                }
            }
            batch.clear();
        }
        Ok(())
    }
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Dedicated(s) => s.local_addr(),
            Self::Shared(s) => s.local_addr(),
        }
    }
    pub(crate) async fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        match self {
            Self::Dedicated(s) => s.recv_from(buf).await,
            Self::Shared(s) => s.recv_from(buf).await,
        }
    }
    async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        match self {
            Self::Dedicated(s) => s.send_to(bytes, to).await,
            Self::Shared(s) => s.send_to(bytes, to).await,
        }
    }
    fn check_open(&self) -> io::Result<()> {
        match self {
            Self::Dedicated(s) => s.check_open(),
            Self::Shared(s) => s.check_open(),
        }
    }
    fn stopped(&self) -> futures::future::LocalBoxFuture<'static, ()> {
        match self {
            // Dedicated sockets can be replaced during migration. Only a
            // listener has an owner whose shutdown spans the entire session.
            Self::Dedicated(_) => Box::pin(std::future::pending()),
            Self::Shared(s) => s.stopped(),
        }
    }
}
struct SessionDrivers {
    bulk: Option<bulk::Driver>,
    established: Option<tokio::sync::oneshot::Sender<()>>,
    datagrams: Option<DatagramDriver>,
    shutdown: Option<ShutdownDriver>,
    mobility: Option<mobility::Driver>,
    scheduling: scheduling::Driver,
}
async fn drive(
    socket: PacketSocket,
    conn: Box<buffers::Connection>,
    io: (impl stream::Input, impl stream::Output),
    drivers: SessionDrivers,
) -> io::Result<()> {
    // Listener shutdown must also cancel packet pacing and blocked writes,
    // not only wake the socket receive branch.
    let stopped = socket.stopped();
    let control = drivers.shutdown.as_ref().map(|s| s.control.clone());
    let _guard = control.clone().map(DriverGuard);
    let expired = async {
        match &control {
            Some(control) => control.expired().await,
            None => std::future::pending().await,
        }
    };
    let result = tokio::select! {
        biased;
        _ = stopped => Err(io::Error::new(io::ErrorKind::BrokenPipe,"shared listener route closed")),
        _ = expired => Err(io::Error::new(io::ErrorKind::TimedOut,"Native shutdown acknowledgement timed out")),
        result = drive_packets(socket,conn,io,drivers) => result,
    };
    if let (Some(control), Err(error)) = (&control, &result) {
        control.finish(Err(io::Error::new(error.kind(), error.to_string())));
    }
    result
}
async fn application_turn() {
    let mut yielded = false;
    futures::future::poll_fn(move |cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}

async fn drive_packets(
    mut socket: PacketSocket,
    conn: Box<buffers::Connection>,
    io: (impl stream::Input, impl stream::Output),
    drivers: SessionDrivers,
) -> io::Result<()> {
    let SessionDrivers {
        bulk,
        mut established,
        mut datagrams,
        shutdown,
        mut mobility,
        scheduling,
    } = drivers;
    let schedule_changed = scheduling.changed();
    let mut engine = engine::Engine::new(conn, established.is_some(), shutdown, scheduling);
    engine.bulk = bulk;
    let mut udp = vec![0; 65535];
    let mut candidate_packet = vec![0; 65535];
    let mut batch = crate::rpc::packet_batch::Batch::new();
    let sender = crate::rpc::packet_batch::Sender::default();
    let mut mtu_recovery = crate::rpc::packet_mtu::Recovery::default();
    let (mut reader, mut writer) = io;
    let mut local = socket.local_addr()?;
    // Keep one registration alive while packet and application events run.
    // Recreating these futures per event churns notification and timer state.
    let changed = schedule_changed.notified();
    tokio::pin!(changed);
    changed.as_mut().enable();
    let recovery_timer = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(recovery_timer);
    loop {
        socket.check_open()?;
        // Consume a bounded receive burst before generating acknowledgements.
        // Flushing after each datagram creates an ACK and scheduler round trip
        // per packet even when the rest of the same stream write is queued.
        for _ in 0..16 {
            let Some(packet) = socket.recv_from(&mut udp).now_or_never() else {
                break;
            };
            let (n, from) = packet?;
            if !crate::nat::is_binding_message(&udp[..n]) {
                match engine
                    .conn
                    .recv(&mut udp[..n], quiche::RecvInfo { from, to: local })
                {
                    Ok(_) | Err(quiche::Error::Done | quiche::Error::CryptoFail) => (),
                    Err(e) => return Err(error(e)),
                }
            }
        }
        if !engine.step(tokio::time::Instant::now)? {
            return Ok(());
        }
        if let Some(mobility) = &mut mobility {
            if let Some(migrated) =
                mobility.step(&mut engine.conn, &mut socket, tokio::time::Instant::now)?
            {
                local = migrated;
            }
        }
        if engine.ready() {
            if let Some(d) = &mut datagrams {
                // Never block reliable RPC on an unread unreliable lane.
                for _ in 0..DATAGRAM_QUEUE {
                    match engine.conn.dgram_recv(&mut udp) {
                        Ok(n) if n <= MAX_DATAGRAM_BYTES => {
                            if let Ok(permit) = d.incoming.try_reserve() {
                                permit.send(udp[..n].to_vec());
                            }
                        }
                        Ok(_) => (),
                        Err(quiche::Error::Done) => break,
                        Err(e) => return Err(error(e)),
                    }
                }
            }
            if let Some(ready) = established.take() {
                let _ = ready.send(());
            }
        }
        if engine.closing() {
            datagrams = None;
        }
        // Let the RPC consumer produce a reply before flushing its ACK. A
        // nonblocking bridge write preserves receive backpressure. A bounded
        // number of cooperative turns lets the RPC tasks produce their output.
        let mut application_progress = false;
        if !engine.rx.pending().is_empty() {
            if let Some(written) = writer.write_from(&mut engine.rx).now_or_never() {
                engine.delivered(written?)?;
                application_progress = true;
                // Finish delivering already-buffered input before waiting for
                // a reply. Otherwise every partial large message pays four
                // scheduler turns although the RPC reader still needs bytes.
                if engine.tx.can_read()
                    && engine.rx.pending().is_empty()
                    && !engine.conn.stream_readable(0)
                {
                    for _ in 0..8 {
                        application_turn().await;
                        if engine.tx.can_read() {
                            if let Some(read) = engine.tx.read_from(&mut reader).now_or_never() {
                                engine.tx.read_owned(read?)?;
                                break;
                            }
                        }
                    }
                }
            }
        }
        if engine.tx.can_read() {
            if let Some(read) = engine.tx.read_from(&mut reader).now_or_never() {
                engine.tx.read_owned(read?)?;
                application_progress = true;
            }
        }
        if engine.rx.needs_shutdown() {
            writer.shutdown().await?;
            engine.rx.closed()?;
            application_progress = true;
        }
        // Delivery can complete a shutdown receipt even without a reply.
        if application_progress && !engine.step(tokio::time::Instant::now)? {
            return Ok(());
        }
        let mut burst = engine.scheduling.burst();
        let mut exhausted = true;
        while burst.permit() {
            let start = batch.len();
            match engine.conn.send(batch.output_buffer()) {
                Ok((n, info)) => {
                    if info.from == local {
                        if !batch.push_prepared(n, info, engine.conn.send_quantum()) {
                            socket.flush_batch(&sender, &mut batch).await?;
                            batch.restart(start, n, info, engine.conn.send_quantum());
                        }
                    } else if let Some(mobility) = &mut mobility {
                        socket.flush_batch(&sender, &mut batch).await?;
                        crate::rpc::pacing::wait_until(info.at).await;
                        mobility.send(&socket, batch.packet(start, n), info).await?;
                    }
                    engine.scheduling.sent();
                }
                Err(quiche::Error::Done) => {
                    exhausted = false;
                    break;
                }
                Err(e) => return Err(error(e)),
            }
        }
        socket.flush_batch(&sender, &mut batch).await?;
        sender.check_path_mtu(&mut engine.conn);
        if !exhausted && engine.packets_drained() {
            return Ok(());
        }
        // Quiche uses system time; Tokio may have a paused/advanced clock.
        // Preserve the relative recovery delay when crossing those domains.
        let timeout = engine
            .conn
            .timeout()
            .unwrap_or(Duration::from_secs(10))
            .min(mobility.as_ref().map_or(Duration::from_secs(10), |m| {
                m.timeout(tokio::time::Instant::now)
            }));
        let datagram_deadline = engine.datagram_deadline(tokio::time::Instant::now);
        let bulk_deadline = engine.bulk.as_ref().and_then(|b| b.deadline());
        let can_accept_datagram = engine.can_accept_datagram();
        let can_read = engine.tx.can_read();
        let can_write = !engine.rx.pending().is_empty();
        let more_stream_data = engine.rx.needs_shutdown()
            || (engine.rx.can_receive() && engine.conn.stream_readable(0));
        recovery_timer
            .as_mut()
            .reset(tokio::time::Instant::now() + timeout);
        tokio::select! {
            _ = async { tokio::time::sleep_until(bulk_deadline.unwrap()).await }, if bulk_deadline.is_some() => {},
            _ = &mut changed => {
                changed.set(schedule_changed.notified());
                changed.as_mut().enable();
            },
            _ = tokio::task::yield_now(), if exhausted => engine.scheduling.yielded(),
            _ = tokio::task::yield_now(), if more_stream_data => {},
            _ = async { tokio::time::sleep_until(datagram_deadline.unwrap()).await }, if datagram_deadline.is_some() => {},
            event=async { mobility.as_mut().unwrap().event(&mut candidate_packet).await }, if mobility.is_some() => {
                match event? {
                    mobility::Event::Command(command)=>mobility.as_mut().unwrap().command(command,&mut engine.conn,&socket,tokio::time::Instant::now()),
                    mobility::Event::Packet(n,from,to) if !crate::nat::is_binding_message(&candidate_packet[..n])=>match engine.conn.recv(&mut candidate_packet[..n],quiche::RecvInfo {from,to}) {
                        Ok(_)|Err(quiche::Error::Done)|Err(quiche::Error::CryptoFail)=>(),
                        Err(e)=>mobility.as_mut().unwrap().fail_candidate(error(e)),
                    },
                    mobility::Event::Packet(..)=>(),
                    mobility::Event::CandidateError(error)=>mobility.as_mut().unwrap().fail_candidate(error),
                }
            },
            packet=async { datagrams.as_mut().unwrap().outgoing.recv().await },
                if can_accept_datagram && datagrams.as_ref().is_some_and(|d| !d.send_closed) => {
                match packet {
                    Some(bytes) => engine.datagram(bytes)?,
                    None => datagrams.as_mut().unwrap().send_closed = true,
                }
            },
            r=socket.recv_from(&mut udp) => {
                let(n,from)=r?;
                if !crate::nat::is_binding_message(&udp[..n]) {
                    match engine.conn.recv(&mut udp[..n],quiche::RecvInfo {from,to:local}) {
                        Ok(_)|Err(quiche::Error::Done)|Err(quiche::Error::CryptoFail) => {},
                        Err(e)=>return Err(error(e)),
                    }
                }
            },
            r=engine.tx.read_from(&mut reader), if can_read => engine.tx.read_owned(r?)?,
            r=writer.write_from(&mut engine.rx), if can_write => engine.delivered(r?)?,
            _=&mut recovery_timer => mtu_recovery.on_timeout(&mut engine.conn),
        }
    }
}

/// A session whose pinned Native handshake has completed. Its peer identity
/// cannot be supplied separately when attaching it to the multiparty network.
pub struct AuthenticatedSession {
    bulk: Option<bulk::Plane>,
    pub(crate) peer: [u8; 32],
    pub(crate) local: [u8; 32],
    pub(crate) io: Option<crate::rpc::local_io::Stream>,
    datagrams: Option<DatagramPort>,
    mobility: Mobility,
    scheduling: Scheduling,
    pub(crate) shutdown: Control,
    pub(crate) driver: tokio::task::JoinHandle<io::Result<()>>,
}
impl AuthenticatedSession {
    /// Session-bound bulk grants. TCP sessions use ordinary capability RPC.
    pub fn bulk(&self) -> Option<bulk::Plane> {
        self.bulk.clone()
    }
    pub fn scheduling(&self) -> Scheduling {
        self.scheduling.clone()
    }
    pub fn mobility(&self) -> Mobility {
        self.mobility.clone()
    }
    /// Take the session-local datagram lane once, before attaching RPC if needed.
    pub fn take_datagrams(&mut self) -> Option<DatagramPort> {
        self.datagrams.take()
    }
    pub fn peer(&self) -> [u8; 32] {
        self.peer
    }
    /// Drain and close an unattached session. Dropping this future aborts it.
    /// A receipt acknowledges transport delivery, not application execution.
    pub async fn shutdown(mut self, timeout: Duration) -> io::Result<Receipt> {
        self.shutdown.begin(timeout)?;
        self.io
            .as_mut()
            .ok_or_else(|| io::Error::other("session already attached"))?
            .shutdown()
            .await?;
        self.shutdown.wait().await
    }
}
impl Drop for AuthenticatedSession {
    fn drop(&mut self) {
        self.shutdown.finish(Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "Native session dropped",
        )));
        self.driver.abort();
    }
}
pub(crate) async fn authenticated(
    socket: PacketSocket,
    conn: Box<buffers::Connection>,
    local: [u8; 32],
    peer: [u8; 32],
) -> io::Result<AuthenticatedSession> {
    let (app, network) = crate::rpc::local_io::pair(crate::rpc::QUIC_BUFFER_BYTES);
    let (ready, wait) = tokio::sync::oneshot::channel();
    let (datagrams, datagram_driver) = datagram_pair();
    let shutdown = Control::new();
    let shutdown_driver = ShutdownDriver::new(shutdown.clone(), conn.is_server());
    let (mobility, mobility_driver) = mobility::pair();
    let (scheduling, scheduling_driver) = scheduling::pair();
    // Bulk admission/IO shares the existing policy wakeup. Idle ordinary RPC
    // does not register another Notify waiter or sample a bulk deadline clock.
    let (bulk, bulk_driver) = bulk::pair(
        &conn,
        local,
        peer,
        shutdown.clone(),
        scheduling_driver.changed(),
    );
    let session = AuthenticatedSession {
        bulk: Some(bulk),
        mobility,
        scheduling,
        shutdown,
        datagrams: Some(datagrams),
        local,
        peer,
        io: Some(app),
        driver: tokio::task::spawn_local(drive(
            socket,
            conn,
            network.into_split(),
            SessionDrivers {
                established: Some(ready),
                datagrams: Some(datagram_driver),
                shutdown: Some(shutdown_driver),
                mobility: Some(mobility_driver),
                scheduling: scheduling_driver,
                bulk: Some(bulk_driver),
            },
        )),
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await?
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Native authentication failed",
            )
        })?;
    Ok(session)
}
/// Dial a pinned peer and wait for authentication before exposing RPC IO.
/// Run inside a Tokio `LocalSet`. Cancellation closes the pending session.
pub async fn connect_authenticated(
    socket: UdpSocket,
    remote: SocketAddr,
    identity: &Identity,
    peer: [u8; 32],
    psk: Option<[u8; 32]>,
    context: &[u8],
) -> io::Result<AuthenticatedSession> {
    connect_for_version(
        socket,
        remote,
        identity,
        peer,
        psk,
        context,
        QuicVersion::V1,
    )
    .await
}
/// Dial a pinned peer using an explicitly selected quiche wire version.
pub async fn connect_for_version(
    socket: UdpSocket,
    remote: SocketAddr,
    identity: &Identity,
    peer: [u8; 32],
    psk: Option<[u8; 32]>,
    context: &[u8],
    version: QuicVersion,
) -> io::Result<AuthenticatedSession> {
    let mut binding = b"ReProto native RPC v1\0".to_vec();
    binding.extend_from_slice(context);
    let mut config = config_for_version(identity, peer, psk, &binding, version).map_err(error)?;
    let conn = quiche::connect_with_buffer_factory::<buffers::Factory>(
        None,
        &quiche::ConnectionId::from_ref(&cid()),
        socket.local_addr()?,
        remote,
        &mut config,
    )
    .map_err(error)?;
    authenticated(
        PacketSocket::Dedicated(DatagramSocket::new(socket)?),
        Box::new(conn),
        identity.public_key(),
        peer,
    )
    .await
}
/// Accept a pinned peer on a dedicated socket, waiting for its Native proof.
/// Run inside a Tokio `LocalSet`. Cancellation closes the pending session.
pub async fn accept_authenticated(
    socket: UdpSocket,
    identity: &Identity,
    peer: [u8; 32],
    psk: Option<[u8; 32]>,
    context: &[u8],
) -> io::Result<AuthenticatedSession> {
    let mut binding = b"ReProto native RPC v1\0".to_vec();
    binding.extend_from_slice(context);
    let mut config = config(identity, peer, psk, &binding).map_err(error)?;
    let mut buf = vec![0; 65535];
    let (n, remote) =
        tokio::time::timeout(Duration::from_secs(10), socket.recv_from(&mut buf)).await??;
    let local = socket.local_addr()?;
    let mut conn = quiche::accept_with_buf_factory::<buffers::Factory>(
        &quiche::ConnectionId::from_ref(&cid()),
        None,
        local,
        remote,
        &mut config,
    )
    .map_err(error)?;
    conn.recv(
        &mut buf[..n],
        quiche::RecvInfo {
            from: remote,
            to: local,
        },
    )
    .map_err(error)?;
    authenticated(
        PacketSocket::Dedicated(DatagramSocket::new(socket)?),
        Box::new(conn),
        identity.public_key(),
        peer,
    )
    .await
}

#[cfg(all(test, feature = "services"))]
mod datagram_tests {
    use super::*;
    use std::rc::Rc;
    #[derive(serde::Deserialize)]
    struct BatchTrace {
        steps: Vec<BatchStep>,
    }
    #[derive(serde::Deserialize)]
    struct BatchStep {
        action: String,
        state: Vec<u64>,
    }
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_datagram_batch_traces() {
        use crate::{
            realtime::{Config, MonotonicClock},
            realtime_datagram::{Router, Sender},
        };
        let path = capntproto_test_support::verification::input("CAPNTPROTO_DATAGRAM_BATCH_TRACES")
            .expect("run this test through its verification driver");
        let traces: Vec<BatchTrace> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for (index, trace) in traces.into_iter().enumerate() {
            let (port, mut driver) = datagram_pair();
            let outbound = port.sender();
            // Exercise production queue capacity with two available slots.
            let _reserved = outbound.0.try_reserve_many(DATAGRAM_QUEUE - 2).unwrap();
            let (router, _router_driver) = Router::new(port);
            let (_, control) = router
                .bind(
                    Config::new("batch-model", 0, 1, 1, 2, 1100, 1).unwrap(),
                    Rc::new(MonotonicClock::new(tokio::time::Instant::now(), 0)),
                )
                .unwrap();
            let sender = Sender::connect(control, outbound.clone()).await.unwrap();
            let mut receipt: Option<crate::realtime_datagram::Receipt> = None;
            let mut before_queue = 0;
            let mut before_next = 1;
            for step in trace.steps {
                let mut result = 0;
                if matches!(step.action.as_str(), "offer" | "retry") {
                    before_queue = driver.outgoing.len() as u64;
                    before_next = sender.next_sequence_for_test();
                }
                match step.action.as_str() {
                    "pressure" => outbound.try_send(b"pressure").unwrap(),
                    "offer" => match sender.offer(0, u64::MAX, &[37; 1100]) {
                        Ok(r) => {
                            receipt = Some(r);
                            result = 1;
                        }
                        Err(e) => {
                            result = if e.kind == capnp::ErrorKind::Overloaded {
                                2
                            } else {
                                3
                            };
                        }
                    },
                    "retry" => {
                        result = match receipt.as_ref().unwrap().resend() {
                            Ok(()) => 1,
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => 2,
                            Err(_) => 3,
                        };
                    }
                    "drain" => {
                        driver.outgoing.try_recv().unwrap();
                    }
                    "close" => driver.outgoing.close(),
                    other => panic!("unknown action {other}"),
                }
                let actual = vec![
                    driver.outgoing.len() as u64,
                    sender.next_sequence_for_test(),
                    receipt.as_ref().map_or(0, |r| r.sequence()),
                    outbound.0.is_closed() as u64,
                    before_queue,
                    before_next,
                    result,
                ];
                assert_eq!(actual, step.state, "trace {index} action {}", step.action);
            }
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fragment_batch_admission_is_atomic_and_preserves_sender_sequences() {
        use crate::{
            realtime::{Config, MonotonicClock},
            realtime_datagram::{Router, Sender, MAX_SNAPSHOT_BYTES},
        };
        let (port, mut driver) = datagram_pair();
        let outbound = port.sender();
        let (router, _router_driver) = Router::new(port);
        let (_, control) = router
            .bind(
                Config::new("batch", 0, 1, 1, 4, MAX_SNAPSHOT_BYTES as u32, 1).unwrap(),
                Rc::new(MonotonicClock::new(tokio::time::Instant::now(), 0)),
            )
            .unwrap();
        let sender = Sender::connect(control, outbound.clone()).await.unwrap();
        for _ in 0..DATAGRAM_QUEUE - 1 {
            outbound.try_send(b"existing").unwrap();
        }
        let payload = vec![7; MAX_SNAPSHOT_BYTES];
        assert_eq!(
            sender.offer(0, u64::MAX, &payload).err().unwrap().kind,
            capnp::ErrorKind::Overloaded
        );
        for _ in 0..DATAGRAM_QUEUE - 1 {
            assert_eq!(driver.outgoing.try_recv().unwrap(), b"existing");
        }
        assert!(
            driver.outgoing.try_recv().is_err(),
            "failed offer admitted a prefix"
        );
        let first = sender.offer(0, u64::MAX, &payload).unwrap();
        assert_eq!(first.sequence(), 1, "rejected batch consumed a sequence");
        let mut original = Vec::new();
        for _ in 0..DATAGRAM_QUEUE {
            original.push(driver.outgoing.try_recv().unwrap());
        }
        outbound.try_send(b"pressure").unwrap();
        assert_eq!(
            first.resend().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(driver.outgoing.try_recv().unwrap(), b"pressure");
        assert!(driver.outgoing.try_recv().is_err());
        first.resend().unwrap();
        for packet in original {
            assert_eq!(driver.outgoing.try_recv().unwrap(), packet);
        }
        let second = sender.offer(0, u64::MAX, b"small").unwrap();
        assert_eq!(second.sequence(), 2);
        assert_eq!(&driver.outgoing.try_recv().unwrap()[..4], b"RDS1");
        for invalid in [
            vec![],
            vec![vec![]; DATAGRAM_QUEUE + 1],
            vec![vec![0; MAX_DATAGRAM_BYTES + 1]],
        ] {
            assert_eq!(
                outbound.try_send_batch(&invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(driver.outgoing.try_recv().is_err());
        }
        drop(driver);
        assert_eq!(
            first.resend().unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert!(sender.offer(0, u64::MAX, &payload).is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn bounded_queue_backpressure_drop_and_disconnect() {
        let (mut port, mut driver) = datagram_pair();
        let sender = port.sender();
        for _ in 0..DATAGRAM_QUEUE {
            sender.try_send(&[1; MAX_DATAGRAM_BYTES]).unwrap();
        }
        assert_eq!(
            sender.try_send(b"full").unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            driver.outgoing.recv().await.unwrap().len(),
            MAX_DATAGRAM_BYTES
        );
        sender.try_send(b"available").unwrap();
        for _ in 0..DATAGRAM_QUEUE {
            driver
                .incoming
                .try_send(vec![2; MAX_DATAGRAM_BYTES])
                .unwrap();
        }
        assert!(driver.incoming.try_send(vec![3]).is_err());
        assert_eq!(port.recv().await.unwrap().len(), MAX_DATAGRAM_BYTES);
        drop(driver);
        assert_eq!(
            sender.try_send(b"closed").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        for _ in 1..DATAGRAM_QUEUE {
            assert!(port.recv().await.is_some());
        }
        assert!(port.recv().await.is_none());
    }
}

#[cfg(all(test, feature = "services"))]
mod datagram_reentry_tests;

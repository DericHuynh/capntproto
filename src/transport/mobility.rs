//! Validated, bounded client source-address migration without replaying RPC.
use super::socket::DatagramSocket;
use super::*;
use futures::FutureExt;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path {
    pub local: SocketAddr,
    pub peer: SocketAddr,
}
pub(super) enum Command {
    Migrate(
        DatagramSocket,
        SocketAddr,
        Instant,
        oneshot::Sender<io::Result<Path>>,
    ),
    Rotate(oneshot::Sender<io::Result<()>>),
}
pub(super) enum Event {
    Command(Option<Command>),
    Packet(usize, SocketAddr, SocketAddr),
    CandidateError(io::Error),
}
/// Control for one authenticated session. It does not keep the session alive.
#[derive(Clone)]
pub struct Mobility(Option<mpsc::Sender<Command>>);
impl Mobility {
    pub(super) fn unavailable() -> Self {
        Self(None)
    }
    /// Whether this backend exposes validated migration and CID rotation.
    pub fn is_supported(&self) -> bool {
        self.0.is_some()
    }
    fn sender(&self) -> io::Result<&mpsc::Sender<Command>> {
        self.0.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "backend does not expose migration controls",
            )
        })
    }

    /// Probe from a new, concrete UDP socket, then switch only after encrypted
    /// PATH_RESPONSE validation. Until then, application traffic uses the old
    /// path. Dropping the future cancels a pending probe before commitment.
    /// Client only; a server follows an authenticated client migration.
    pub async fn migrate(
        &self,
        socket: UdpSocket,
        peer: SocketAddr,
        timeout: Duration,
    ) -> io::Result<Path> {
        self.migrate_socket(socket.into(), peer, timeout).await
    }
    pub(super) async fn migrate_socket(
        &self,
        socket: DatagramSocket,
        peer: SocketAddr,
        timeout: Duration,
    ) -> io::Result<Path> {
        if timeout.is_zero()
            || timeout > Duration::from_secs(10)
            || !crate::nat::address_ok(peer)
            || !crate::nat::address_ok(socket.local_addr()?)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Native migration path or deadline",
            ));
        }
        let (tx, rx) = oneshot::channel();
        self.sender()?
            .try_send(Command::Migrate(socket, peer, Instant::now() + timeout, tx))
            .map_err(queue_error)?;
        rx.await.map_err(|_| closed())?
    }
    /// Advertise a fresh source CID and ask the peer to retire the oldest if
    /// its negotiated allowance is full. Shared-listener demultiplexing is
    /// installed before publication and removed on retirement.
    pub async fn rotate_connection_id(&self) -> io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender()?
            .try_send(Command::Rotate(tx))
            .map_err(queue_error)?;
        rx.await.map_err(|_| closed())?
    }
}
fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "Native mobility session closed")
}
fn queue_error<T>(error: mpsc::error::TrySendError<T>) -> io::Error {
    match error {
        mpsc::error::TrySendError::Full(_) => {
            io::Error::new(io::ErrorKind::WouldBlock, "Native mobility queue full")
        }
        _ => closed(),
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Gate {
    #[default]
    Probing,
    Validated,
    Committed,
    Retired,
}
impl Gate {
    fn validate(&mut self) {
        if *self == Self::Probing {
            *self = Self::Validated;
        }
    }
    fn retire(&mut self) {
        if *self != Self::Committed {
            *self = Self::Retired;
        }
    }
    fn commit(&mut self) -> bool {
        if *self != Self::Validated {
            return false;
        }
        *self = Self::Committed;
        true
    }
}
struct Pending {
    socket: DatagramSocket,
    path: Path,
    deadline: Instant,
    reply: oneshot::Sender<io::Result<Path>>,
    gate: Gate,
}
pub(super) struct Driver {
    commands: mpsc::Receiver<Command>,
    pending: Option<Pending>,
    issued: u16,
    probes: u8,
    commands_closed: bool,
}
pub(super) fn pair() -> (Mobility, Driver) {
    let (tx, rx) = mpsc::channel(8);
    (
        Mobility(Some(tx)),
        Driver {
            commands: rx,
            pending: None,
            issued: 0,
            probes: 0,
            commands_closed: false,
        },
    )
}
impl Driver {
    fn issue(
        &mut self,
        conn: &mut quiche::Connection,
        socket: &PacketSocket,
        retire: bool,
    ) -> io::Result<()> {
        if self.issued >= 128 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Native session CID budget exhausted",
            ));
        }
        let id = cid();
        if let PacketSocket::Shared(shared) = socket {
            shared.register_cid(id)?;
        }
        if let Err(e) = conn.new_scid(
            &quiche::ConnectionId::from_ref(&id),
            u128::from_be_bytes(cid()),
            retire,
        ) {
            if let PacketSocket::Shared(shared) = socket {
                shared.retire_cid(&id);
            }
            return Err(error(e));
        }
        self.issued += 1;
        Ok(())
    }
    pub(super) fn step(
        &mut self,
        conn: &mut quiche::Connection,
        socket: &mut PacketSocket,
        now: Instant,
    ) -> io::Result<()> {
        while let Some(id) = conn.retired_scid_next() {
            if let PacketSocket::Shared(shared) = socket {
                shared.retire_cid(id.as_ref());
            }
        }
        if conn.is_established() {
            while conn.scids_left() > 0 && self.issued < 128 {
                // A listener's lifetime CID budget may be exhausted without
                // breaking established traffic. An explicit rotation reports it.
                if self.issue(conn, socket, false).is_err() {
                    break;
                }
            }
        }
        while conn.path_event_next().is_some() {}
        if let Some(pending) = &mut self.pending {
            let result = if pending.reply.is_closed() {
                pending.gate.retire();
                Some(Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Native migration canceled",
                )))
            } else if now >= pending.deadline {
                pending.gate.retire();
                Some(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Native path validation timed out",
                )))
            } else if conn
                .is_path_validated(pending.path.local, pending.path.peer)
                .unwrap_or(false)
            {
                pending.gate.validate();
                Some(
                    conn.migrate(pending.path.local, pending.path.peer)
                        .map(|_| {
                            assert!(pending.gate.commit());
                            pending.path
                        })
                        .map_err(error),
                )
            } else {
                None
            };
            if let Some(result) = result {
                let pending = self.pending.take().unwrap();
                if result.is_ok() {
                    *socket = PacketSocket::Dedicated(pending.socket);
                }
                let _ = pending.reply.send(result);
            }
        }
        Ok(())
    }
    pub(super) fn timeout(&self, now: Instant) -> Duration {
        self.pending.as_ref().map_or(Duration::from_secs(10), |p| {
            p.deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(20))
        })
    }
    pub(super) async fn send(
        &mut self,
        socket: &PacketSocket,
        bytes: &[u8],
        info: quiche::SendInfo,
    ) -> io::Result<()> {
        if socket.local_addr()? == info.from {
            socket.send_to(bytes, info.to).await?;
        } else if let Some(pending) = self.pending.as_ref().filter(|p| p.path.local == info.from) {
            // Candidate backpressure must not suspend the established path or
            // hide cancellation/deadlines. UDP send is cancellation-safe: poll
            // once, treating Pending as packet loss. quiche's probe timeout
            // retransmits with a fresh packet; no unbounded queue is needed.
            if let Some(Err(error)) = pending.socket.send_to(bytes, info.to).now_or_never() {
                self.fail_candidate(error);
            }
        }
        // quiche can retransmit a probe for a canceled/retired local socket.
        // Dropping it is safe; never emit it on a different source address.
        Ok(())
    }
    pub(super) fn fail_candidate(&mut self, error: io::Error) {
        if let Some(mut pending) = self.pending.take() {
            pending.gate.retire();
            let _ = pending.reply.send(Err(error));
        }
    }
    pub(super) async fn event(&mut self, buf: &mut [u8]) -> io::Result<Event> {
        tokio::select! {
            command=self.commands.recv(), if !self.commands_closed => Ok(Event::Command(command)),
            received=async {
                let p=self.pending.as_mut().unwrap();
                Ok::<_,io::Error>(match p.socket.recv_from(buf).await {
                    Ok((n,from))=>Event::Packet(n,from,p.path.local),
                    Err(error)=>Event::CandidateError(error),
                })
            }, if self.pending.is_some() => received,
            else => std::future::pending().await,
        }
    }
    pub(super) fn command(
        &mut self,
        command: Option<Command>,
        conn: &mut quiche::Connection,
        socket: &PacketSocket,
        now: Instant,
    ) {
        match command {
            None => self.commands_closed = true,
            Some(Command::Rotate(reply)) => {
                if !reply.is_closed() {
                    let result = self.issue(conn, socket, true);
                    let _ = reply.send(result);
                }
            }
            Some(Command::Migrate(candidate, peer, deadline, reply)) => {
                if reply.is_closed() {
                    return;
                }
                let result = (|| {
                    if self.pending.is_some()
                        || self.probes >= 8
                        || conn.is_server()
                        || !conn.is_established()
                        || !matches!(socket, PacketSocket::Dedicated(_))
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Native migration unavailable or exhausted",
                        ));
                    }
                    if now >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Native migration deadline elapsed",
                        ));
                    }
                    let local = candidate.local_addr()?;
                    if local == socket.local_addr()? {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "migration requires a fresh socket",
                        ));
                    }
                    self.probes += 1;
                    conn.probe_path(local, peer).map_err(error)?;
                    Ok(Path { local, peer })
                })();
                match result {
                    Ok(path) => {
                        self.pending = Some(Pending {
                            socket: candidate,
                            path,
                            deadline,
                            reply,
                            gate: Gate::default(),
                        })
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn candidate_socket_error_retires_probe_and_preserves_original_socket() {
        let old = PacketSocket::Dedicated(UdpSocket::bind("127.0.0.1:0").await.unwrap().into());
        let candidate = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        // This test targets an actual send error, not initial IO readiness.
        candidate.writable().await.unwrap();
        let path = Path {
            local: candidate.local_addr().unwrap(),
            peer: "[::1]:12345".parse().unwrap(),
        };
        let (reply, wait) = oneshot::channel();
        let (_control, mut driver) = pair();
        driver.pending = Some(Pending {
            socket: candidate.into(),
            path,
            deadline: Instant::now() + Duration::from_secs(1),
            reply,
            gate: Gate::default(),
        });
        // An IPv4 socket cannot emit this IPv6 packet. Candidate failure is
        // reported to the operation; it must not escape as a session failure.
        driver
            .send(
                &old,
                b"probe",
                quiche::SendInfo {
                    from: path.local,
                    to: path.peer,
                    at: std::time::Instant::now(),
                },
            )
            .await
            .unwrap();
        assert!(wait.await.unwrap().is_err());
        assert!(driver.pending.is_none());
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = receiver.local_addr().unwrap();
        driver
            .send(
                &old,
                b"still live",
                quiche::SendInfo {
                    from: old.local_addr().unwrap(),
                    to: address,
                    at: std::time::Instant::now(),
                },
            )
            .await
            .unwrap();
        let mut bytes = [0; 32];
        let (n, _) = receiver.recv_from(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..n], b"still live");
    }
    #[test]
    fn replay_tlc_path_validation_gate() {
        use reproto_test_support::verification::exploration;
        let config = include_str!("../../verification/NativePathMigration.cfg");
        let live =
            config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY Settles\n";
        exploration::controls(
            "verification/NativePathMigration.tla",
            "native-path-migration",
            config,
            &[
                ("early", "Authenticated"),
                ("resurrect", "NoResurrection"),
                ("lateCancel", "StableCommit"),
            ],
            Some(&live),
        )
        .unwrap();
        let traces = exploration::traces(
            "verification/NativePathMigration.tla",
            "native-path-migration",
            config,
        )
        .unwrap();
        for trace in traces {
            let mut gate = Gate::default();
            for state in trace {
                match state["event"] {
                    1 | 4 => gate.validate(),
                    2 | 5 => gate.retire(),
                    3 => assert!(gate.commit()),
                    _ => panic!("unexpected migration event"),
                }
                let phase = match gate {
                    Gate::Probing => 0,
                    Gate::Validated => 1,
                    Gate::Committed => 2,
                    Gate::Retired => 3,
                };
                assert_eq!(phase, state["phase"]);
            }
        }
    }
}

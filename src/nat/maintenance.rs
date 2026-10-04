//! Periodic Binding transactions multiplexed by the shared listener driver.
use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug)]
pub struct MappingOptions {
    /// Time between completed transactions and the next refresh: 15–120s.
    pub interval: Duration,
    /// Per-transaction budget: (0,10s]. Retransmissions use the same ID.
    pub timeout: Duration,
}
impl Default for MappingOptions {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(15),
            timeout: Duration::from_secs(3),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingStatus {
    Discovering,
    /// A routing hint, not proof of peer reachability or a guaranteed NAT lease.
    Observed {
        address: SocketAddr,
        observed_at: Instant,
    },
    Unavailable,
    Stopped,
}
struct Probe {
    transaction: [u8; 12],
    deadline: Instant,
    next: Instant,
    backoff: Duration,
}
pub(crate) struct MappingState {
    server: SocketAddr,
    options: MappingOptions,
    next: Cell<Instant>,
    probe: RefCell<Option<Probe>>,
    status: watch::Sender<MappingStatus>,
}
impl MappingState {
    pub(crate) fn new(server: SocketAddr, options: MappingOptions) -> io::Result<Rc<Self>> {
        if !address_ok(server)
            || options.interval < Duration::from_secs(15)
            || options.interval > Duration::from_secs(120)
            || options.timeout.is_zero()
            || options.timeout > Duration::from_secs(10)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid STUN maintenance settings",
            ));
        }
        Ok(Rc::new(Self {
            server,
            options,
            next: Cell::new(Instant::now()),
            probe: RefCell::new(None),
            status: watch::channel(MappingStatus::Discovering).0,
        }))
    }
    pub(crate) fn stopped(&self) -> bool {
        *self.status.borrow() == MappingStatus::Stopped
    }
    pub(crate) fn close(&self) {
        self.probe.borrow_mut().take();
        self.status.send_replace(MappingStatus::Stopped);
    }
    fn finish(&self, status: MappingStatus, now: Instant) {
        self.probe.borrow_mut().take();
        self.next.set(now + self.options.interval);
        // Watch notifications may call custom wakers; release the probe borrow.
        self.status.send_replace(status);
    }
    pub(crate) fn socket_error(&self, now: Instant) {
        if !self.stopped() && self.probe.borrow().is_some() {
            self.finish(MappingStatus::Unavailable, now);
        }
    }
    pub(crate) fn tick(&self, socket: &crate::transport::socket::DatagramSocket, now: Instant) {
        if self.stopped() {
            return;
        }
        if self
            .probe
            .borrow()
            .as_ref()
            .is_some_and(|p| now >= p.deadline)
        {
            self.finish(MappingStatus::Unavailable, now);
            return;
        }
        if self.probe.borrow().is_none() && now >= self.next.get() {
            let mut transaction = [0; 12];
            if ring::rand::SystemRandom::new()
                .fill(&mut transaction)
                .is_err()
            {
                self.finish(MappingStatus::Unavailable, now);
                return;
            }
            *self.probe.borrow_mut() = Some(Probe {
                transaction,
                deadline: now + self.options.timeout,
                next: now,
                backoff: Duration::from_millis(500),
            });
        }
        let packet = {
            let mut probe = self.probe.borrow_mut();
            probe.as_mut().filter(|p| now >= p.next).map(|p| {
                p.next = now + p.backoff;
                p.backoff = (p.backoff * 2).min(Duration::from_secs(2));
                request(p.transaction)
            })
        };
        if let Some(packet) = packet {
            // Never suspend the shared packet driver for an auxiliary probe.
            // WouldBlock is retried on the bounded transaction schedule.
            if let Err(error) = socket.try_send_to(&packet, self.server) {
                if error.kind() != io::ErrorKind::WouldBlock {
                    self.finish(MappingStatus::Unavailable, now);
                }
            }
        }
    }
    pub(crate) fn receive(&self, packet: &[u8], from: SocketAddr, now: Instant) {
        if self.stopped() || from != self.server || packet.len() > 1024 {
            return;
        }
        let address = self
            .probe
            .borrow()
            .as_ref()
            .filter(|p| now < p.deadline)
            .and_then(|p| parse(packet, p.transaction).ok());
        if let Some(address) = address {
            self.finish(
                MappingStatus::Observed {
                    address,
                    observed_at: now,
                },
                now,
            );
        }
    }
    fn status(&self, now: Instant) -> MappingStatus {
        let status = *self.status.borrow();
        if let MappingStatus::Observed { observed_at, .. } = status {
            if now >= observed_at + self.options.interval + self.options.timeout {
                return MappingStatus::Unavailable;
            }
        }
        status
    }
}

/// Owns one listener's refresh service without keeping the listener alive.
/// Drop/close stops refreshes. A failed
/// transaction withdraws the hint and retries after the configured interval.
#[must_use = "dropping the mapping stops refreshes"]
pub struct Mapping {
    pub(crate) state: Rc<MappingState>,
    observer: MappingObserver,
}
impl Mapping {
    pub(crate) fn new(state: Rc<MappingState>) -> Self {
        let observer = MappingObserver::new(&state);
        Self { state, observer }
    }
    /// Latest observation, with stale hints suppressed even on a stalled driver.
    pub fn status(&self) -> MappingStatus {
        self.state.status(Instant::now())
    }
    pub fn address(&self) -> Option<SocketAddr> {
        match self.status() {
            MappingStatus::Observed { address, .. } => Some(address),
            _ => None,
        }
    }
    /// Wait for the next observation, failure or stop. Changes may coalesce.
    /// Stopped is terminal and returns immediately on subsequent calls.
    pub async fn changed(&mut self) -> MappingStatus {
        self.observer.changed().await
    }
    /// Independent observation without ownership of the mapping or listener.
    pub fn subscribe(&self) -> MappingObserver {
        MappingObserver::new(&self.state)
    }
    pub fn close(&self) {
        self.state.close();
    }
}

#[derive(Clone)]
pub struct MappingObserver {
    state: Weak<MappingState>,
    changes: watch::Receiver<MappingStatus>,
    expired_observation: Option<Instant>,
}
impl MappingObserver {
    fn new(state: &Rc<MappingState>) -> Self {
        Self {
            state: Rc::downgrade(state),
            changes: state.status.subscribe(),
            expired_observation: None,
        }
    }
    pub fn status(&self) -> MappingStatus {
        self.state
            .upgrade()
            .map_or(MappingStatus::Stopped, |s| s.status(Instant::now()))
    }
    /// Includes observation expiry, even if the packet driver has not yet
    /// reported a failed refresh. Expiry is reported once per observation.
    pub async fn changed(&mut self) -> MappingStatus {
        if self.status() == MappingStatus::Stopped {
            return MappingStatus::Stopped;
        }
        let expiration = self.state.upgrade().and_then(|s| {
            let status = *s.status.borrow();
            match status {
                MappingStatus::Observed { observed_at, .. }
                    if self.expired_observation != Some(observed_at) =>
                {
                    Some((
                        observed_at,
                        observed_at + s.options.interval + s.options.timeout,
                    ))
                }
                _ => None,
            }
        });
        tokio::select! {
            biased;
            _ = self.changes.changed() => {},
            _ = async {
                match expiration {
                    Some((_, deadline)) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            } => { self.expired_observation = expiration.map(|(observed_at, _)| observed_at); }
        }
        let status = self.status();
        if status == MappingStatus::Unavailable {
            if let MappingStatus::Observed { observed_at, .. } = *self.changes.borrow() {
                self.expired_observation = Some(observed_at);
            }
        }
        status
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::response;
    use super::*;
    async fn udp() -> crate::transport::socket::DatagramSocket {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket.writable().await.unwrap();
        crate::transport::socket::DatagramSocket::new(socket).unwrap()
    }
    #[tokio::test(start_paused = true)]
    async fn observers_report_expiry_once_and_do_not_own_mapping() {
        use futures::FutureExt;
        let state = MappingState::new(
            "127.0.0.1:12345".parse().unwrap(),
            MappingOptions::default(),
        )
        .unwrap();
        let mapping = Mapping::new(state.clone());
        let mut observer = mapping.subscribe();
        let address = "127.0.0.1:32123".parse().unwrap();
        state.finish(
            MappingStatus::Observed {
                address,
                observed_at: Instant::now(),
            },
            Instant::now(),
        );
        // The observation is unread when it becomes stale. No packet driver is
        // running: notification of expiry must not depend on that driver.
        tokio::time::advance(Duration::from_secs(18)).await;
        assert_eq!(observer.changed().await, MappingStatus::Unavailable);
        assert!(observer.changed().now_or_never().is_none());
        state.finish(
            MappingStatus::Observed {
                address,
                observed_at: Instant::now(),
            },
            Instant::now(),
        );
        assert!(matches!(
            observer.changed().await,
            MappingStatus::Observed { .. }
        ));
        let mut second = mapping.subscribe();
        let expires = observer.changed();
        tokio::pin!(expires);
        assert!(expires.as_mut().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(18)).await;
        assert_eq!(expires.await, MappingStatus::Unavailable);
        drop(mapping);
        assert_eq!(second.changed().await, MappingStatus::Stopped);
    }
    fn observed(mapping: &MappingState, now: Instant) -> u64 {
        match mapping.status(now) {
            MappingStatus::Observed { address, .. } => u64::from(address.port()),
            _ => 0,
        }
    }
    #[tokio::test]
    async fn refresh_deadline_source_transaction_and_bounds() {
        let socket = udp().await;
        let server = udp().await;
        let address = server.local_addr().unwrap();
        let options = MappingOptions::default();
        let state = MappingState::new(address, options).unwrap();
        let owner = Mapping::new(state.clone());
        let now = Instant::now();
        state.tick(&socket, now);
        let id = state.probe.borrow().as_ref().unwrap().transaction;
        let packet = response(id, "127.0.0.1:12345".parse().unwrap());
        state.receive(&packet, address, now + options.timeout);
        assert!(owner.address().is_none()); // deadline wins even before timer cleanup
        state.tick(&socket, now + options.timeout);
        assert_eq!(
            state.status(now + options.timeout),
            MappingStatus::Unavailable
        );
        let later = now + options.timeout + options.interval;
        state.tick(&socket, later);
        let next = state.probe.borrow().as_ref().unwrap().transaction;
        assert_ne!(next, id);
        state.receive(&packet, address, later);
        assert_eq!(observed(&state, later), 0);
        let packet = response(next, "127.0.0.1:12345".parse().unwrap());
        state.receive(&packet, socket.local_addr().unwrap(), later);
        assert_eq!(observed(&state, later), 0);
        state.receive(&packet, address, later);
        assert_eq!(observed(&state, later), 12345);
        assert_eq!(
            state.status(later + options.interval + options.timeout),
            MappingStatus::Unavailable
        );
        state.tick(&socket, later + options.interval);
        state.socket_error(later + options.interval);
        assert_eq!(
            state.status(later + options.interval),
            MappingStatus::Unavailable
        );
        state.receive(&packet, address, later + options.interval);
        assert_eq!(observed(&state, later + options.interval), 0);
        drop(owner);
        state.tick(&socket, later + options.interval);
        state.receive(&packet, address, later + options.interval);
        assert!(state.stopped());
        assert!(state.probe.borrow().is_none());
        assert!(MappingState::new(
            address,
            MappingOptions {
                interval: Duration::from_secs(14),
                ..options
            }
        )
        .is_err());
        assert!(MappingState::new(
            address,
            MappingOptions {
                timeout: Duration::ZERO,
                ..options
            }
        )
        .is_err());
    }
    #[tokio::test]
    async fn replay_tlc_mapping_refresh() {
        use capntproto_test_support::verification::exploration;
        let config = include_str!("../../verification/NativeMappingRefresh.cfg");
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
            + "\nPROPERTY ProbeSettles\n";
        exploration::controls(
            "verification/NativeMappingRefresh.tla",
            "native-mapping-refresh",
            config,
            &[
                ("oldResponse", "Transaction"),
                ("foreignResponse", "Source"),
                ("staleHint", "Withdraw"),
            ],
            Some(&live),
        )
        .unwrap();
        let traces = exploration::traces(
            "verification/NativeMappingRefresh.tla",
            "native-mapping-refresh",
            config,
        )
        .unwrap();
        let socket = udp().await;
        let receiver = udp().await;
        let server = receiver.local_addr().unwrap();
        for trace in traces {
            let state = MappingState::new(server, MappingOptions::default()).unwrap();
            let mut now = Instant::now();
            let mut first = [0; 12];
            for model in trace {
                match model["event"] {
                    1 => {
                        now = now.max(state.next.get());
                        state.tick(&socket, now);
                        if model["request"] == 1 {
                            first = state.probe.borrow().as_ref().unwrap().transaction;
                        }
                    }
                    2 => {
                        let id = state.probe.borrow().as_ref().unwrap().transaction;
                        state.receive(
                            &response(
                                id,
                                SocketAddr::from(([127, 0, 0, 1], model["request"] as u16)),
                            ),
                            server,
                            now,
                        );
                    }
                    3 => {
                        now = state.probe.borrow().as_ref().unwrap().deadline;
                        state.tick(&socket, now);
                    }
                    4 => state.receive(
                        &response(first, "127.0.0.1:1".parse().unwrap()),
                        server,
                        now,
                    ),
                    5 => {
                        let id = state
                            .probe
                            .borrow()
                            .as_ref()
                            .map_or(first, |p| p.transaction);
                        state.receive(
                            &response(
                                id,
                                SocketAddr::from(([127, 0, 0, 1], model["request"] as u16)),
                            ),
                            socket.local_addr().unwrap(),
                            now,
                        );
                    }
                    6 => state.close(),
                    _ => panic!("unknown refresh action"),
                }
                assert_eq!(observed(&state, now), model["observed"]);
                assert_eq!(state.probe.borrow().is_some(), model["pending"] == 1);
                assert_eq!(state.stopped(), model["closed"] == 1);
            }
        }
    }
}

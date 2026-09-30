//! Capability-authorized unreliable snapshots on a single authenticated session.
//! Local queue admission and an Unknown status never establish nonexecution.
use crate::{
    realtime::{Clock, Config, Outcome, Receiver},
    realtime_capnp::{datagram_snapshots as wire, datagram_status},
    transport::{DatagramPort, DatagramSender, MAX_DATAGRAM_BYTES},
};
use capnp::Error;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::{Rc, Weak},
};

const MAGIC: &[u8; 4] = b"RDS1";
const HEADER: usize = 56;
/// Maximum snapshot in the compact single-packet RDS1 encoding.
pub const MAX_PAYLOAD_BYTES: usize = MAX_DATAGRAM_BYTES - HEADER;
mod fragment;
pub use fragment::{MAX_FRAGMENTS, MAX_REASSEMBLIES, MAX_SNAPSHOT_BYTES};
/// Lifetime issuance bound: retired tokens are never reused within this router.
pub const MAX_GRANTS: usize = 64;
fn failed(message: &str) -> Error {
    Error::failed(message.into())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Unknown,
    Pending,
    Terminal(Outcome),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub status: Status,
    pub closed: bool,
}
struct State {
    closed: bool,
    issued: BTreeSet<[u8; 32]>,
    grants: BTreeMap<[u8; 32], Weak<Grant>>,
}
/// One router per session lane. Tokens identify fresh receiver capabilities, not
/// global objects, peer identities or reusable connection IDs.
pub struct Router(Rc<RefCell<State>>);
struct DriverGuard(Rc<RefCell<State>>);
impl Drop for DriverGuard {
    fn drop(&mut self) {
        let grants = {
            let mut state = self.0.borrow_mut();
            state.closed = true;
            std::mem::take(&mut state.grants)
        };
        for grant in grants.into_values().filter_map(|g| g.upgrade()) {
            grant.close();
        }
    }
}
impl Router {
    /// Poll the returned driver on the session's LocalSet. Dropping it, including
    /// before its first poll, revokes all grants and closes pending snapshots.
    pub fn new(mut port: DatagramPort) -> (Self, impl std::future::Future<Output = ()>) {
        let state = Rc::new(RefCell::new(State {
            closed: false,
            issued: BTreeSet::new(),
            grants: BTreeMap::new(),
        }));
        let guard = DriverGuard(state.clone());
        let router = Self(state);
        let driver = async move {
            while let Some(packet) = port.recv().await {
                Self::dispatch(&guard.0, &packet);
            }
            drop(guard);
        };
        (router, driver)
    }
    /// Export the returned control capability only to an authorized holder over
    /// a confidential route. Knowing the token grants access on this session.
    pub fn bind(
        &self,
        config: Config,
        clock: Rc<dyn Clock>,
    ) -> capnp::Result<(Receiver, wire::Client)> {
        if config.max_payload_bytes() as usize > MAX_SNAPSHOT_BYTES {
            return Err(failed("snapshot exceeds datagram payload bound"));
        }
        let (receiver, _) = Receiver::new(config.clone(), clock.clone());
        let mut state = self.0.borrow_mut();
        if state.closed {
            return Err(Error::disconnected("datagram router closed".into()));
        }
        if state.issued.len() == MAX_GRANTS {
            return Err(Error::overloaded("datagram grant lifetime limit".into()));
        }
        use ring::rand::SecureRandom;
        let mut token = [0; 32];
        loop {
            ring::rand::SystemRandom::new()
                .fill(&mut token)
                .map_err(|_| failed("token randomness unavailable"))?;
            if state.issued.insert(token) {
                break;
            }
        }
        let grant = Rc::new(Grant {
            token,
            config,
            receiver: receiver.clone(),
            router: Rc::downgrade(&self.0),
            clock,
            fragments: RefCell::new(fragment::Fragments::default()),
        });
        state.grants.insert(token, Rc::downgrade(&grant));
        drop(state);
        Ok((receiver, capnp_rpc::new_client(Service(grant))))
    }
    /// Release expired incomplete payloads, including on an otherwise idle lane.
    /// Ingress and status queries also sweep. No background clock is assumed.
    pub fn expire(&self) {
        let grants: Vec<_> = self
            .0
            .borrow()
            .grants
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        for grant in grants {
            grant.prune();
        }
    }
    fn dispatch(state: &RefCell<State>, packet: &[u8]) {
        if packet.len() < HEADER
            || packet.len() > MAX_DATAGRAM_BYTES
            || (&packet[..4] != MAGIC && &packet[..4] != fragment::MAGIC)
        {
            return;
        }
        let token: [u8; 32] = packet[4..36].try_into().unwrap();
        let grant = {
            let state = state.borrow();
            if state.closed {
                return;
            }
            state.grants.get(&token).and_then(Weak::upgrade)
        };
        if let Some(grant) = grant {
            grant.prune();
            if &packet[..4] == fragment::MAGIC {
                grant.fragment(packet);
                return;
            }
            let sequence = u64::from_be_bytes(packet[36..44].try_into().unwrap());
            let key = u32::from_be_bytes(packet[44..48].try_into().unwrap());
            let deadline = u64::from_be_bytes(packet[48..56].try_into().unwrap());
            // A fragmented sequence cannot be reinterpreted as an RDS1 snapshot.
            if grant.fragments.borrow().contains(sequence) {
                return;
            }
            // Admission is synchronous. Drop only the receipt waiter, preserving
            // accepted work. Malformed or conflicting datagrams have no reply.
            drop(
                grant
                    .receiver
                    .offer(sequence, key, deadline, &packet[HEADER..]),
            );
        }
    }
}
struct Grant {
    token: [u8; 32],
    config: Config,
    receiver: Receiver,
    router: Weak<RefCell<State>>,
    clock: Rc<dyn Clock>,
    fragments: RefCell<fragment::Fragments>,
}
impl Grant {
    fn close(&self) {
        self.fragments.borrow_mut().clear();
        self.receiver.close();
    }
    fn revoke(&self) {
        if let Some(state) = self.router.upgrade() {
            state.borrow_mut().grants.remove(&self.token);
        }
        self.close();
    }
    fn observe(&self, sequence: u64) -> capnp::Result<Observation> {
        if sequence == 0 || sequence > self.config.max_sequence() {
            return Err(failed("sequence outside datagram capability namespace"));
        }
        self.prune();
        let status = match self.receiver.status(sequence) {
            Some(outcome) => Status::Terminal(outcome),
            None if self.receiver.pending().contains(&sequence) => Status::Pending,
            None => Status::Unknown,
        };
        Ok(Observation {
            status,
            closed: self.receiver.is_closed(),
        })
    }
}
impl Drop for Grant {
    fn drop(&mut self) {
        self.revoke();
    }
}
struct Service(Rc<Grant>);
impl wire::Server for Service {
    async fn describe(
        self: Rc<Self>,
        _: wire::DescribeParams,
        mut r: wire::DescribeResults,
    ) -> capnp::Result<()> {
        if self.0.receiver.is_closed() {
            return Err(failed("datagram capability closed"));
        }
        let mut r = r.get();
        self.0.config.write(r.reborrow().init_config());
        r.set_token(&self.0.token);
        Ok(())
    }
    async fn status(
        self: Rc<Self>,
        p: wire::StatusParams,
        mut r: wire::StatusResults,
    ) -> capnp::Result<()> {
        let observation = self.0.observe(p.get()?.get_sequence())?;
        let mut r = r.get();
        r.set_closed(observation.closed);
        let mut status = r.init_status();
        match observation.status {
            Status::Unknown => status.set_unknown(()),
            Status::Pending => status.set_pending(()),
            Status::Terminal(outcome) => status.set_terminal(outcome),
        }
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        p: wire::CancelParams,
        mut r: wire::CancelResults,
    ) -> capnp::Result<()> {
        let sequence = p.get()?.get_sequence();
        let outcome = self.0.receiver.cancel(sequence)?;
        self.0.fragments.borrow_mut().remove(sequence);
        r.get().set_outcome(outcome);
        Ok(())
    }
    async fn close(
        self: Rc<Self>,
        _: wire::CloseParams,
        _: wire::CloseResults,
    ) -> capnp::Result<()> {
        self.0.revoke();
        Ok(())
    }
}
fn packet(token: &[u8; 32], sequence: u64, key: u32, deadline: u64, bytes: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(HEADER + bytes.len());
    packet.extend_from_slice(MAGIC);
    packet.extend_from_slice(token);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&key.to_be_bytes());
    packet.extend_from_slice(&deadline.to_be_bytes());
    packet.extend_from_slice(bytes);
    packet
}
struct SendState {
    control: wire::Client,
    port: DatagramSender,
    token: [u8; 32],
    config: Config,
    next: Cell<u64>,
    closed: Cell<bool>,
}
/// Create one allocator per fresh capability; clones share its sequence space.
#[derive(Clone)]
pub struct Sender(Rc<SendState>);
impl Sender {
    #[cfg(test)]
    pub(crate) fn next_sequence_for_test(&self) -> u64 {
        self.0.next.get()
    }
    #[cfg(test)]
    pub(crate) fn is_closed_for_test(&self) -> bool {
        self.0.closed.get()
    }
    pub async fn connect(control: wire::Client, port: DatagramSender) -> capnp::Result<Self> {
        let response = control.describe_request().send().promise.await?;
        let response = response.get()?;
        let config = Config::read(response.get_config()?)?;
        if config.max_payload_bytes() as usize > MAX_SNAPSHOT_BYTES {
            return Err(failed("remote datagram payload bound"));
        }
        let token = response
            .get_token()?
            .try_into()
            .map_err(|_| failed("invalid datagram token"))?;
        Ok(Self(Rc::new(SendState {
            control,
            port,
            token,
            config,
            next: Cell::new(1),
            closed: Cell::new(false),
        })))
    }
    /// Returns after bounded local queue admission. No receipt is implied until
    /// a subsequent status query observes a terminal outcome.
    pub fn offer(&self, key: u32, not_after: u64, bytes: &[u8]) -> capnp::Result<Receipt> {
        let s = &self.0;
        let sequence = s.next.get();
        if s.closed.get()
            || sequence > s.config.max_sequence()
            || key >= s.config.keys()
            || bytes.len() > s.config.max_payload_bytes() as usize
        {
            return Err(failed("closed or invalid datagram offer"));
        }
        let packets = fragment::packets(&s.token, sequence, key, not_after, bytes);
        let batch = s.port.reserve_batch(&packets).map_err(|e| match e.kind() {
            std::io::ErrorKind::WouldBlock => Error::overloaded(e.to_string()),
            _ => Error::disconnected(e.to_string()),
        })?;
        // Publishing can invoke a waker that reenters offer/close through a
        // clone. Commit first and do not overwrite state after any wakeup.
        s.next.set(sequence + 1);
        batch.send();
        Ok(Receipt {
            sequence,
            packets,
            sender: self.clone(),
        })
    }
    pub async fn status(&self, sequence: u64) -> capnp::Result<Observation> {
        let mut request = self.0.control.status_request();
        request.get().set_sequence(sequence);
        let response = request.send().promise.await?;
        let response = response.get()?;
        let status = match response.get_status()?.which()? {
            datagram_status::Unknown(()) => Status::Unknown,
            datagram_status::Pending(()) => Status::Pending,
            datagram_status::Terminal(outcome) => Status::Terminal(outcome?),
        };
        Ok(Observation {
            status,
            closed: response.get_closed(),
        })
    }
    pub async fn cancel(&self, sequence: u64) -> capnp::Result<Outcome> {
        let mut request = self.0.control.cancel_request();
        request.get().set_sequence(sequence);
        request
            .send()
            .promise
            .await?
            .get()?
            .get_outcome()
            .map_err(Into::into)
    }
    /// Seal the local sender immediately, even if the returned close wait is
    /// canceled. Only a successful reply establishes remote closure.
    pub fn close(&self) -> impl std::future::Future<Output = capnp::Result<()>> {
        self.0.closed.set(true);
        let response = self.0.control.close_request().send();
        async move {
            response.promise.await?;
            Ok(())
        }
    }
}
/// A retry retains exactly the same sequence, metadata and payload. The receiver
/// deduplicates it even after application, cancellation or supersession.
pub struct Receipt {
    sequence: u64,
    packets: Vec<Vec<u8>>,
    sender: Sender,
}
impl Receipt {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn resend(&self) -> std::io::Result<()> {
        if self.sender.0.closed.get() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "datagram sender closed",
            ));
        }
        self.sender.0.port.try_send_batch(&self.packets)
    }
    pub async fn status(&self) -> capnp::Result<Observation> {
        self.sender.status(self.sequence).await
    }
    pub async fn cancel(&self) -> capnp::Result<Outcome> {
        self.sender.cancel(self.sequence).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct TestClock;
    impl Clock for TestClock {
        fn now(&self) -> u64 {
            0
        }
    }
    fn config() -> Config {
        Config::new("trace", 0, 1, 1, 2, 32, 1).unwrap()
    }
    fn router() -> (Router, DriverGuard) {
        let state = Rc::new(RefCell::new(State {
            closed: false,
            issued: BTreeSet::new(),
            grants: BTreeMap::new(),
        }));
        (Router(state.clone()), DriverGuard(state))
    }
    fn grant(router: &Router) -> Rc<Grant> {
        router
            .0
            .borrow()
            .grants
            .values()
            .next()
            .unwrap()
            .upgrade()
            .unwrap()
    }
    #[tokio::test(flavor = "current_thread")]
    async fn ingress_rejects_malformed_conflicting_and_retired_tokens() {
        let (router, _guard) = router();
        let (r, control) = router.bind(config(), Rc::new(TestClock)).unwrap();
        let grant = grant(&router);
        let good = packet(&grant.token, 1, 0, 10, b"valid");
        for n in 0..HEADER {
            Router::dispatch(&router.0, &good[..n]);
        }
        let mut bad = good.clone();
        bad[0] ^= 1;
        Router::dispatch(&router.0, &bad);
        bad = good.clone();
        bad[4] ^= 1;
        Router::dispatch(&router.0, &bad);
        Router::dispatch(&router.0, &vec![0; MAX_DATAGRAM_BYTES + 1]);
        Router::dispatch(
            &router.0,
            &packet(&grant.token, 0, 0, 10, b"invalid sequence"),
        );
        Router::dispatch(&router.0, &packet(&grant.token, 1, 1, 10, b"invalid key"));
        Router::dispatch(&router.0, &packet(&grant.token, 1, 0, 10, &[0; 33]));
        assert!(r.pending().is_empty());
        Router::dispatch(&router.0, &good);
        Router::dispatch(&router.0, &packet(&grant.token, 1, 0, 10, b"changed"));
        assert_eq!(r.waiter_count(), 0);
        assert_eq!(r.apply(1).unwrap(), Outcome::Applied);
        assert_eq!(r.get(0).unwrap().bytes(), b"valid");
        let value = r.get(0).unwrap();
        Router::dispatch(&router.0, &good);
        assert!(Rc::ptr_eq(&value, &r.get(0).unwrap()));
        control.close_request().send().promise.await.unwrap();
        Router::dispatch(&router.0, &packet(&grant.token, 2, 0, 10, b"closed"));
        assert!(r.pending().is_empty());
        assert_eq!(r.status(2), None);
        assert!(router.0.borrow().grants.is_empty());
        assert!(router.0.borrow().issued.contains(&grant.token));
    }
    #[tokio::test(flavor = "current_thread")]
    async fn capability_release_and_lifetime_quota() {
        let (router, guard) = router();
        for _ in 0..MAX_GRANTS {
            let (r, cap) = router.bind(config(), Rc::new(TestClock)).unwrap();
            let token = *router.0.borrow().grants.keys().next().unwrap();
            Router::dispatch(&router.0, &packet(&token, 1, 0, 10, b"pending"));
            assert_eq!(r.pending(), [1]);
            drop(cap);
            assert!(r.is_closed());
            assert_eq!(r.status(1), Some(Outcome::Closed));
            assert!(router.0.borrow().grants.is_empty());
        }
        assert!(router.bind(config(), Rc::new(TestClock)).is_err());
        assert_eq!(router.0.borrow().issued.len(), MAX_GRANTS);
        drop(guard);
        assert!(router.0.borrow().closed);
    }
    #[derive(serde::Deserialize)]
    struct Trace {
        steps: Vec<Step>,
    }
    #[derive(serde::Deserialize)]
    struct Step {
        action: String,
        state: Vec<u64>,
    }
    fn status_code(status: Status) -> u64 {
        match status {
            Status::Unknown => 0,
            Status::Pending => 1,
            Status::Terminal(Outcome::Applied) => 2,
            Status::Terminal(Outcome::Canceled) => 3,
            Status::Terminal(Outcome::Closed) => 4,
            s => panic!("unmodeled status {s:?}"),
        }
    }
    async fn query(control: &wire::Client) -> Observation {
        let mut request = control.status_request();
        request.get().set_sequence(1);
        let response = request.send().promise.await.unwrap();
        let response = response.get().unwrap();
        let status = match response.get_status().unwrap().which().unwrap() {
            datagram_status::Unknown(()) => Status::Unknown,
            datagram_status::Pending(()) => Status::Pending,
            datagram_status::Terminal(outcome) => Status::Terminal(outcome.unwrap()),
        };
        Observation {
            status,
            closed: response.get_closed(),
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_datagram_traces() {
        let path = reproto_test_support::verification::input("REPROTO_DATAGRAM_TRACES")
            .expect("run this test through its verification driver");
        let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        tokio::task::LocalSet::new()
            .run_until(async {
                for (index, trace) in traces.into_iter().enumerate() {
                    let (router, guard) = router();
                    let mut guard = Some(guard);
                    let (r, service) = router.bind(config(), Rc::new(TestClock)).unwrap();
                    let grant = grant(&router);
                    let good = packet(&grant.token, 1, 0, 10, b"snapshot");
                    let mut bad = good.clone();
                    bad[4] ^= 1;
                    let (a, b) = tokio::io::duplex(4096);
                    let server = crate::rpc::serve(b, service.client);
                    let (control, client): (wire::Client, _) = crate::rpc::client(a);
                    let mut queue = VecDeque::new();
                    let mut applied = 0;
                    let mut canceled = false;
                    let mut unauthorized = false;
                    let mut response: Option<Observation> = None;
                    let mut reply: Option<Observation> = None;
                    for step in trace.steps {
                        let old = r.get(0);
                        match step.action.as_str() {
                            "enqueue" => queue.push_back(good.clone()),
                            "deliver" => Router::dispatch(&router.0, &queue.pop_front().unwrap()),
                            "lose" => {
                                queue.pop_front().unwrap();
                            }
                            "cancel" => {
                                let mut request = control.cancel_request();
                                request.get().set_sequence(1);
                                canceled = request
                                    .send()
                                    .promise
                                    .await
                                    .unwrap()
                                    .get()
                                    .unwrap()
                                    .get_outcome()
                                    .unwrap()
                                    == Outcome::Canceled;
                            }
                            "apply" => {
                                r.apply(1).unwrap();
                            }
                            "revoke" => {
                                control.close_request().send().promise.await.unwrap();
                            }
                            "stop" => {
                                drop(guard.take());
                                queue.clear();
                            }
                            "bad_token" => {
                                let before = grant.observe(1).unwrap();
                                Router::dispatch(&router.0, &bad);
                                unauthorized |= before != grant.observe(1).unwrap();
                            }
                            "malformed" => Router::dispatch(&router.0, b"invalid"),
                            "query" => {
                                response = Some(query(&control).await);
                            }
                            "reply" => {
                                reply = response.take();
                            }
                            other => panic!("unexpected action {other}"),
                        }
                        if r.get(0)
                            .is_some_and(|v| old.as_ref().is_none_or(|old| !Rc::ptr_eq(old, &v)))
                        {
                            applied += 1;
                        }
                        let state = router.0.borrow();
                        let observed = grant.observe(1).unwrap();
                        let actual = vec![
                            state.grants.contains_key(&grant.token) as u64,
                            (!state.closed) as u64,
                            status_code(observed.status),
                            queue.len() as u64,
                            applied,
                            canceled as u64,
                            r.is_closed() as u64,
                            unauthorized as u64,
                            response.map_or(0, |o| status_code(o.status) + 1),
                            response.is_some_and(|o| o.closed) as u64,
                            reply.map_or(0, |o| status_code(o.status) + 1),
                            reply.is_some_and(|o| o.closed) as u64,
                        ];
                        assert_eq!(actual, step.state, "trace {index} action {}", step.action);
                        assert_eq!(r.waiter_count(), 0);
                    }
                    server.abort();
                    client.abort();
                    // Release completed and aborted LocalSet tasks between traces.
                    tokio::task::yield_now().await;
                }
            })
            .await;
    }
}

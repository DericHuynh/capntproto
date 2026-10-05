//! Authenticated, session-bound receipt fences for orderly Native route shutdown.
//!
//! A receipt covers bytes delivered to the peer's bounded RPC input bridge. It
//! does not acknowledge method execution, outstanding replies, or datagrams.
use std::{cell::RefCell, io, rc::Rc, time::Duration};
use tokio::{sync::Notify, time::Instant};

pub(crate) const FRAME_BYTES: usize = 29;
/// The peer acknowledged all these ordered stream bytes, including arbitration
/// when enabled. The native one-byte stream preface is excluded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub bytes: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    pub kind: u8,
    pub nonce: [u8; 16],
    pub bytes: u64,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Native shutdown fence")
}
impl Frame {
    pub(crate) fn encode(self) -> [u8; FRAME_BYTES] {
        let mut bytes = [0; FRAME_BYTES];
        bytes[..4].copy_from_slice(b"RPS1");
        bytes[4] = self.kind;
        bytes[5..21].copy_from_slice(&self.nonce);
        bytes[21..].copy_from_slice(&self.bytes.to_le_bytes());
        bytes
    }
    pub(crate) fn decode(bytes: [u8; FRAME_BYTES]) -> io::Result<Self> {
        if &bytes[..4] != b"RPS1" || !matches!(bytes[4], 1..=3) {
            return Err(invalid());
        }
        Ok(Self {
            kind: bytes[4],
            nonce: bytes[5..21].try_into().unwrap(),
            bytes: u64::from_le_bytes(bytes[21..].try_into().unwrap()),
        })
    }
}
/// Production transition logic, also exercised by exported TLC traces.
#[derive(Default)]
pub(crate) struct Protocol {
    pub(crate) sent: Option<Frame>,
    pub(crate) peer: Option<Frame>,
    pub(crate) delivered: u64,
    pub(crate) ack_sent: bool,
    pub(crate) acknowledged: bool,
    pub(crate) expects_peer: bool,
}
impl Protocol {
    pub(crate) fn request(&mut self, nonce: [u8; 16], bytes: u64) -> io::Result<Frame> {
        if self.sent.is_some() {
            return Err(invalid());
        }
        let frame = Frame {
            kind: 1,
            nonce,
            bytes,
        };
        self.sent = Some(frame);
        Ok(frame)
    }
    pub(crate) fn receive(&mut self, frame: Frame) -> io::Result<()> {
        match frame.kind {
            1 if self.peer.is_none()
                && frame.bytes >= self.delivered
                && (!self.acknowledged || self.expects_peer) =>
            {
                self.peer = Some(frame)
            }
            2 | 3
                if !self.acknowledged
                    && (frame.kind == 3 || self.peer.is_none())
                    && self.sent.is_some_and(|sent| {
                        sent.nonce == frame.nonce && sent.bytes == frame.bytes
                    }) =>
            {
                self.acknowledged = true;
                self.expects_peer = frame.kind == 3;
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
    pub(crate) fn ready(&self, reply_delivered: bool) -> bool {
        self.acknowledged
            && (!self.expects_peer || (self.peer.is_some() && self.ack_sent && reply_delivered))
    }
    pub(crate) fn deliver(&mut self, bytes: usize) -> io::Result<()> {
        let next = self
            .delivered
            .checked_add(bytes as u64)
            .ok_or_else(invalid)?;
        if self.peer.is_some_and(|peer| next > peer.bytes) {
            return Err(invalid());
        }
        self.delivered = next;
        Ok(())
    }
    pub(crate) fn acknowledge(&mut self) -> Option<Frame> {
        let peer = self.peer?;
        if self.ack_sent || self.delivered != peer.bytes {
            return None;
        }
        self.ack_sent = true;
        Some(Frame {
            kind: if self.sent.is_some() { 3 } else { 2 },
            ..peer
        })
    }
}
#[derive(Clone)]
struct Failure(io::ErrorKind, String);
type Outcome = Result<Receipt, Failure>;
#[derive(Default)]
struct State {
    deadline: Option<Instant>,
    peer_closing: bool,
    outcome: Option<Outcome>,
}
struct Shared {
    state: RefCell<State>,
    changed: Notify,
}
/// A generation-local control handle. It does not keep its driver/session alive.
#[derive(Clone)]
pub(crate) struct Control(Rc<Shared>);
impl Control {
    pub(crate) fn new() -> Self {
        Self(Rc::new(Shared {
            state: RefCell::new(State::default()),
            changed: Notify::new(),
        }))
    }
    pub(crate) fn begin(&self, timeout: Duration) -> io::Result<()> {
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "shutdown timeout must be in (0, 60s]",
            ));
        }
        {
            let mut state = self.0.state.borrow_mut();
            if state.deadline.is_some() || state.outcome.is_some() || state.peer_closing {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Native shutdown already started or session stopped",
                ));
            }
            state.deadline = Some(Instant::now() + timeout);
        }
        self.0.changed.notify_waiters();
        Ok(())
    }
    pub(crate) fn peer_closing(&self) {
        self.0.state.borrow_mut().peer_closing = true;
    }
    pub(crate) fn requested(&self) -> bool {
        self.0.state.borrow().deadline.is_some()
    }
    pub(crate) fn accepting(&self) -> bool {
        let state = self.0.state.borrow();
        state.deadline.is_none() && state.outcome.is_none() && !state.peer_closing
    }
    pub(crate) fn finish(&self, outcome: io::Result<Receipt>) {
        {
            let mut state = self.0.state.borrow_mut();
            if state.outcome.is_some() {
                return;
            }
            state.outcome = Some(outcome.map_err(|e| Failure(e.kind(), e.to_string())));
        }
        self.0.changed.notify_waiters();
    }
    pub(crate) async fn wait(&self) -> io::Result<Receipt> {
        loop {
            let wait = self.0.changed.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if let Some(outcome) = self.0.state.borrow().outcome.clone() {
                return outcome.map_err(|Failure(kind, text)| io::Error::new(kind, text));
            }
            wait.await;
        }
    }
    pub(crate) async fn expired(&self) {
        loop {
            let wait = self.0.changed.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            let deadline = self.0.state.borrow().deadline;
            if let Some(deadline) = deadline {
                tokio::time::sleep_until(deadline).await;
                return;
            }
            wait.await;
        }
    }
}
pub(crate) struct DriverGuard(pub(crate) Control);
impl Drop for DriverGuard {
    fn drop(&mut self) {
        self.0.finish(Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "Native session stopped before shutdown acknowledgement",
        )));
    }
}

#[cfg(test)]
#[path = "native_shutdown/schedule_tests.rs"]
mod schedule_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn wire(frame: Frame) -> Frame {
        Frame::decode(frame.encode()).unwrap()
    }
    #[tokio::test(flavor = "current_thread")]
    async fn frames_receipts_and_terminal_outcomes_are_bound_to_the_session() {
        let mut p = Protocol::default();
        let request = p.request([1; 16], 10).unwrap();
        for frame in [Frame { kind: 2, ..request }, Frame { kind: 3, ..request }] {
            for field in 0..2 {
                let mut bad = frame;
                if field == 0 {
                    bad.nonce[0] ^= 1;
                } else {
                    bad.bytes += 1;
                }
                assert!(p.receive(wire(bad)).is_err());
                assert!(!p.acknowledged);
            }
        }
        assert!(p.request([2; 16], 10).is_err());
        let mut unsolicited = Protocol::default();
        assert!(unsolicited.receive(Frame { kind: 2, ..request }).is_err());
        for field in [0, 4] {
            let mut bytes = request.encode();
            bytes[field] = 255;
            assert!(Frame::decode(bytes).is_err());
        }
        p.receive(Frame { kind: 3, ..request }).unwrap();
        assert!(!p.ready(false));
        assert!(p.receive(Frame { kind: 3, ..request }).is_err());
        let peer = Frame {
            kind: 1,
            nonce: [2; 16],
            bytes: 2,
        };
        p.receive(peer).unwrap();
        assert!(p.acknowledge().is_none());
        p.deliver(1).unwrap();
        assert!(p.acknowledge().is_none());
        p.deliver(1).unwrap();
        let ack = p.acknowledge().unwrap();
        assert_eq!(ack.kind, 3);
        assert!(!p.ready(false));
        assert!(p.ready(true));
        assert!(p.acknowledge().is_none());
        assert!(p.receive(peer).is_err());
        assert!(p.deliver(1).is_err());
        assert_eq!(p.delivered, 2);
        let mut late = Protocol::default();
        late.deliver(3).unwrap();
        assert!(late.receive(peer).is_err());
        let control = Control::new();
        assert!(control.begin(Duration::ZERO).is_err());
        assert!(control.begin(Duration::from_secs(61)).is_err());
        control.begin(Duration::from_secs(1)).unwrap();
        assert!(control.begin(Duration::from_secs(1)).is_err());
        control.finish(Err(io::Error::new(io::ErrorKind::TimedOut, "deadline")));
        control.finish(Ok(Receipt { bytes: 10 }));
        assert_eq!(
            control.wait().await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        let control = Control::new();
        control.peer_closing();
        assert!(control.begin(Duration::from_secs(1)).is_err());
        let control = Control::new();
        control.finish(Ok(Receipt { bytes: 10 }));
        drop(DriverGuard(control.clone()));
        assert_eq!(control.wait().await.unwrap().bytes, 10);
    }
    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TraceState {
        drained: u64,
        sent: u64,
        seen: u64,
        data: u64,
        peer_data: u64,
        peer_seen: u64,
        ack: u64,
        ack_seen: u64,
        reply: u64,
        reply_seen: u64,
        confirmed: u64,
        done: u64,
        failed: u64,
        bad: u64,
        late: u64,
    }
    #[derive(serde::Deserialize)]
    struct Trace {
        bytes: u64,
        crossed: bool,
        steps: Vec<Step>,
    }
    #[derive(serde::Deserialize)]
    struct Step {
        action: String,
        state: TraceState,
    }
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_native_shutdown_traces() {
        let path =
            capntproto_test_support::verification::input("CAPNTPROTO_NATIVE_SHUTDOWN_TRACES")
                .expect("run this test through its verification driver");
        let traces: Vec<Trace> = capntproto_test_support::traces::read(path, "RpcNativeShutdown");
        assert!(!traces.is_empty());
        for (index, trace) in traces.iter().enumerate() {
            let mut a = Protocol::default();
            let mut b = Protocol::default();
            let control = Control::new();
            control.begin(Duration::from_secs(60)).unwrap();
            let peer = trace
                .crossed
                .then(|| b.request([2; 16], trace.bytes).unwrap());
            let mut drained = false;
            let mut ack = None;
            let mut reply = None;
            let mut reply_seen = false;
            let mut confirmed = false;
            let mut bad = false;
            let mut late = false;
            for step in &trace.steps {
                match step.action.as_str() {
                    "drain" => drained = true,
                    "send" => {
                        assert!(drained);
                        a.request([1; 16], trace.bytes).unwrap();
                    }
                    "request" => b.receive(wire(a.sent.unwrap())).unwrap(),
                    "data" => b.deliver(1).unwrap(),
                    "ack" => {
                        ack = Some(wire(b.acknowledge().unwrap()));
                    }
                    "receive" => a.receive(wire(ack.unwrap())).unwrap(),
                    "peerRequest" => a.receive(wire(peer.unwrap())).unwrap(),
                    "peerData" => a.deliver(1).unwrap(),
                    "reply" => {
                        assert!(a.sent.is_some());
                        reply = Some(wire(a.acknowledge().unwrap()));
                    }
                    "replyReceive" => {
                        b.receive(wire(reply.unwrap())).unwrap();
                        reply_seen = true;
                    }
                    "confirm" => {
                        assert!(reply_seen);
                        confirmed = true;
                    }
                    "finish" => {
                        assert!(a.ready(confirmed));
                        control.finish(Ok(Receipt { bytes: trace.bytes }));
                    }
                    "peerClose" => {
                        assert!(b.acknowledged && a.acknowledged);
                        control.finish(Ok(Receipt { bytes: trace.bytes }));
                    }
                    "fail" => control.finish(Err(io::Error::new(
                        if index % 2 == 0 {
                            io::ErrorKind::TimedOut
                        } else {
                            io::ErrorKind::ConnectionAborted
                        },
                        "failed",
                    ))),
                    "badAck" => {
                        bad = true;
                        assert!(a
                            .receive(wire(Frame {
                                kind: 2,
                                nonce: [9; 16],
                                bytes: trace.bytes
                            }))
                            .is_err());
                        control.finish(Err(invalid()));
                    }
                    "late" => {
                        late = true;
                        control.finish(Ok(Receipt { bytes: trace.bytes }));
                    }
                    other => panic!("unknown shutdown action {other}"),
                }
                let outcome = control.0.state.borrow().outcome.clone();
                let actual = TraceState {
                    drained: drained as u64,
                    sent: a.sent.is_some() as u64,
                    seen: b.peer.is_some() as u64,
                    data: b.delivered,
                    peer_data: a.delivered,
                    peer_seen: a.peer.is_some() as u64,
                    ack: b.ack_sent as u64,
                    ack_seen: a.acknowledged as u64,
                    reply: a.ack_sent as u64,
                    reply_seen: reply_seen as u64,
                    confirmed: confirmed as u64,
                    done: matches!(outcome, Some(Ok(_))) as u64,
                    failed: matches!(outcome, Some(Err(_))) as u64,
                    bad: bad as u64,
                    late: late as u64,
                };
                assert_eq!(actual, step.state, "trace {index} action {}", step.action);
            }
        }
    }
}

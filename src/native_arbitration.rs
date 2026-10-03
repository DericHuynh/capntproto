//! Opt-in duplicate-session arbitration before native RPC stream publication.
//! The smaller authenticated vat key selects one candidate for each live route.
//! Both peers must use this profile. Failed selections require a new RPC route.
use crate::{
    native_rpc::{Connector, FailureKind, RouteFailure},
    transport::AuthenticatedSession,
};
use futures::{future::LocalBoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use std::{cell::Cell, io, rc::Rc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAGIC: &[u8; 8] = b"RPA1\0\0\0\0";
pub const MAX_CANDIDATES: usize = 4;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub candidates: usize,
    pub timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            candidates: MAX_CANDIDATES,
            timeout: Duration::from_secs(10),
        }
    }
}
impl Limits {
    pub(crate) fn validate(self) -> capnp::Result<()> {
        if self.candidates == 0
            || self.candidates > MAX_CANDIDATES
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(10)
        {
            return Err(capnp::Error::failed(
                "invalid Native arbitration limits".into(),
            ));
        }
        Ok(())
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Native arbitration frame",
    )
}
fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "Native arbitration generation closed",
    )
}
fn token() -> io::Result<[u8; 16]> {
    use ring::rand::SecureRandom;
    let mut value = [0; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut value)
        .map_err(|_| io::Error::other("OS randomness unavailable"))?;
    Ok(value)
}
pub(crate) struct Election {
    epoch: [u8; 16],
    issued: Cell<usize>,
    limit: usize,
    winner: Cell<Option<(usize, [u8; 16])>>,
    published: Cell<bool>,
    stopped: Cell<bool>,
}
impl Election {
    fn new(epoch: [u8; 16], limit: usize) -> Rc<Self> {
        Rc::new(Self {
            epoch,
            issued: Cell::new(0),
            limit,
            winner: Cell::new(None),
            published: Cell::new(false),
            stopped: Cell::new(false),
        })
    }
    fn register(&self) -> io::Result<usize> {
        if self.stopped.get() || self.published.get() {
            return Err(closed());
        }
        if self.issued.get() == self.limit {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Native candidate limit",
            ));
        }
        let id = self.issued.get() + 1;
        self.issued.set(id);
        Ok(id)
    }
    fn claim(&self, id: usize, token: [u8; 16]) -> bool {
        if self.stopped.get() || self.winner.get().is_some() {
            return false;
        }
        assert!(id > 0 && id <= self.issued.get());
        self.winner.set(Some((id, token)));
        true
    }
    fn publish(&self, id: usize) -> io::Result<()> {
        if self.stopped.get() || self.winner.get().map(|v| v.0) != Some(id) {
            return Err(closed());
        }
        self.published.set(true);
        Ok(())
    }
    fn fail(&self, id: usize) {
        if !self.published.get() && self.winner.get().map(|v| v.0) == Some(id) {
            self.close();
        }
    }
    pub(crate) fn close(&self) {
        self.stopped.set(true);
    }
    pub(crate) fn selected(&self) -> Option<[u8; 16]> {
        self.published.get().then(|| self.winner.get().unwrap().1)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Hello,
    Decision,
    Ack,
    SendCommit,
    Commit,
    Ready,
    Rejected,
}
#[derive(Clone, Copy)]
struct Frame {
    kind: u8,
    sender: [u8; 16],
    receiver: [u8; 16],
    token: [u8; 16],
}
impl Frame {
    fn bytes(self) -> [u8; 49] {
        let mut bytes = [0; 49];
        bytes[0] = self.kind;
        bytes[1..17].copy_from_slice(&self.sender);
        bytes[17..33].copy_from_slice(&self.receiver);
        bytes[33..].copy_from_slice(&self.token);
        bytes
    }
    fn parse(bytes: [u8; 49]) -> Self {
        Self {
            kind: bytes[0],
            sender: bytes[1..17].try_into().unwrap(),
            receiver: bytes[17..33].try_into().unwrap(),
            token: bytes[33..].try_into().unwrap(),
        }
    }
}
struct Handshake {
    election: Rc<Election>,
    id: usize,
    leader: bool,
    stage: Stage,
    remote: [u8; 16],
    token: [u8; 16],
}
impl Drop for Handshake {
    fn drop(&mut self) {
        if self.stage != Stage::Ready {
            self.election.fail(self.id);
        }
    }
}
impl Handshake {
    fn new(election: Rc<Election>, id: usize, leader: bool) -> Self {
        Self {
            election,
            id,
            leader,
            stage: Stage::Hello,
            remote: [0; 16],
            token: [0; 16],
        }
    }
    fn hello(&self) -> [u8; 24] {
        let mut bytes = [0; 24];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..].copy_from_slice(&self.election.epoch);
        bytes
    }
    fn frame(&self, kind: u8) -> Frame {
        Frame {
            kind,
            sender: self.election.epoch,
            receiver: self.remote,
            token: self.token,
        }
    }
    fn start(&mut self, hello: [u8; 24], token: [u8; 16]) -> io::Result<Option<Frame>> {
        if self.stage != Stage::Hello || &hello[..8] != MAGIC {
            return Err(invalid());
        }
        self.remote = hello[8..].try_into().unwrap();
        if self.leader {
            self.token = token;
            let selected = self.election.claim(self.id, token);
            self.stage = if selected {
                Stage::Ack
            } else {
                Stage::Rejected
            };
            Ok(Some(self.frame(if selected { 1 } else { 0 })))
        } else {
            self.stage = Stage::Decision;
            Ok(None)
        }
    }
    fn receive(&mut self, frame: Frame) -> io::Result<Option<Frame>> {
        if frame.sender != self.remote || frame.receiver != self.election.epoch {
            return Err(invalid());
        }
        match self.stage {
            Stage::Decision => {
                if frame.kind == 0 {
                    self.stage = Stage::Rejected;
                    return Ok(None);
                }
                if frame.kind != 1 {
                    return Err(invalid());
                }
                self.token = frame.token;
                let selected = self.election.claim(self.id, self.token);
                self.stage = if selected {
                    Stage::Commit
                } else {
                    Stage::Rejected
                };
                Ok(Some(self.frame(if selected { 2 } else { 0 })))
            }
            Stage::Ack => {
                if frame.token != self.token {
                    return Err(invalid());
                }
                if frame.kind == 0 {
                    self.election.fail(self.id);
                    self.stage = Stage::Rejected;
                    return Ok(None);
                }
                if frame.kind != 2 {
                    return Err(invalid());
                }
                self.stage = Stage::SendCommit;
                Ok(Some(self.frame(3)))
            }
            Stage::Commit => {
                if frame.kind != 3 || frame.token != self.token {
                    return Err(invalid());
                }
                self.election.publish(self.id)?;
                self.stage = Stage::Ready;
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
    fn sent_commit(&mut self) -> io::Result<()> {
        if self.stage != Stage::SendCommit {
            return Err(invalid());
        }
        self.election.publish(self.id)?;
        self.stage = Stage::Ready;
        Ok(())
    }
}
async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    election: Rc<Election>,
    id: usize,
    leader: bool,
) -> io::Result<bool> {
    let mut handshake = Handshake::new(election, id, leader);
    io.write_all(&handshake.hello()).await?;
    io.flush().await?;
    let mut hello = [0; 24];
    io.read_exact(&mut hello).await?;
    if let Some(frame) = handshake.start(hello, token()?)? {
        io.write_all(&frame.bytes()).await?;
        io.flush().await?;
    }
    loop {
        match handshake.stage {
            Stage::Ready => return Ok(true),
            Stage::Rejected => return Ok(false),
            _ => {}
        }
        let mut bytes = [0; 49];
        io.read_exact(&mut bytes).await?;
        if let Some(frame) = handshake.receive(Frame::parse(bytes))? {
            io.write_all(&frame.bytes()).await?;
            io.flush().await?;
            if handshake.stage == Stage::SendCommit {
                handshake.sent_commit()?;
            }
        }
    }
}
struct Candidate {
    id: usize,
    session: AuthenticatedSession,
}
pub(crate) struct Admission {
    pub(crate) election: Rc<Election>,
    sender: tokio::sync::mpsc::Sender<Candidate>,
    local: [u8; 32],
    peer: [u8; 32],
}
impl Admission {
    pub(crate) fn close(&self) {
        self.election.close();
    }
    pub(crate) fn submit(&self, session: AuthenticatedSession) -> capnp::Result<()> {
        if session.local != self.local || session.peer != self.peer {
            return Err(error(invalid()));
        }
        let id = self.election.register().map_err(error)?;
        self.sender
            .try_send(Candidate { id, session })
            .map_err(|_| error(closed()))
    }
}
fn error(error: io::Error) -> capnp::Error {
    if error.kind() == io::ErrorKind::WouldBlock {
        capnp::Error::overloaded(error.to_string())
    } else {
        capnp::Error::disconnected(error.to_string())
    }
}
type Pending = FuturesUnordered<LocalBoxFuture<'static, io::Result<Option<AuthenticatedSession>>>>;
fn candidate(
    pending: &Pending,
    election: Rc<Election>,
    local: [u8; 32],
    peer: [u8; 32],
    value: Candidate,
) {
    pending.push(
        async move {
            let mut session = value.session;
            let selected = negotiate(
                session.io.as_mut().ok_or_else(invalid)?,
                election,
                value.id,
                local < peer,
            )
            .await?;
            Ok(selected.then_some(session))
        }
        .boxed_local(),
    );
}
async fn choose(
    mut incoming: tokio::sync::mpsc::Receiver<Candidate>,
    election: Rc<Election>,
    local: [u8; 32],
    peer: [u8; 32],
    connector: Option<Rc<dyn Connector>>,
) -> io::Result<AuthenticatedSession> {
    let mut pending = Pending::new();
    let mut dial = async move {
        match connector {
            Some(connector) => connector.connect(peer).await,
            None => futures::future::pending().await,
        }
    }
    .boxed_local();
    let mut dial_done = false;
    loop {
        tokio::select! {
            result = &mut dial, if !dial_done => {
                dial_done = true;
                if let Ok(session) = result {
                    if session.local == local && session.peer == peer {
                        if let Ok(id) = election.register() {
                            candidate(&pending, election.clone(), local, peer, Candidate { id, session });
                        }
                    }
                }
            }
            Some(value) = incoming.recv() => candidate(&pending, election.clone(), local, peer, value),
            Some(result) = pending.next(), if !pending.is_empty() => {
                if election.stopped.get() { return Err(closed()); }
                if let Ok(Some(session)) = result { return Ok(session); }
            }
            else => return Err(closed()),
        }
    }
}
pub(crate) fn select(
    local: [u8; 32],
    peer: [u8; 32],
    connector: Option<Rc<dyn Connector>>,
    limits: Limits,
) -> capnp::Result<(
    Admission,
    LocalBoxFuture<'static, Result<AuthenticatedSession, RouteFailure>>,
)> {
    let election = Election::new(token().map_err(error)?, limits.candidates);
    let deadline = tokio::time::Instant::now() + limits.timeout;
    let (sender, receiver) = tokio::sync::mpsc::channel(limits.candidates);
    let admission = Admission {
        election: election.clone(),
        sender,
        local,
        peer,
    };
    let selection = async move {
        if tokio::time::Instant::now() >= deadline {
            return Err(RouteFailure::new(
                FailureKind::Timeout,
                "Native arbitration timed out before polling",
            ));
        }
        match tokio::time::timeout_at(
            deadline,
            choose(receiver, election.clone(), local, peer, connector),
        )
        .await
        {
            Ok(Ok(session))
                if !election.stopped.get() && tokio::time::Instant::now() < deadline =>
            {
                Ok(session)
            }
            Ok(Err(error)) => Err(RouteFailure::new(FailureKind::Arbitration, error)),
            _ => Err(RouteFailure::new(
                FailureKind::Timeout,
                "Native arbitration timed out",
            )),
        }
    }
    .boxed_local();
    Ok((admission, selection))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pair() -> (Handshake, Handshake) {
        let l = Election::new([1; 16], 2);
        let f = Election::new([2; 16], 2);
        let li = l.register().unwrap();
        let fi = f.register().unwrap();
        (Handshake::new(l, li, true), Handshake::new(f, fi, false))
    }
    fn begun(l: &mut Handshake, f: &mut Handshake) -> Frame {
        f.start(l.hello(), [0; 16]).unwrap();
        l.start(f.hello(), [3; 16]).unwrap().unwrap()
    }
    fn corrupt(mut frame: Frame, field: usize) -> Frame {
        match field {
            0 => frame.kind = 255,
            1 => frame.sender[0] ^= 1,
            2 => frame.receiver[0] ^= 1,
            3 => frame.token[0] ^= 1,
            _ => unreachable!(),
        }
        Frame::parse(frame.bytes())
    }
    #[test]
    fn version_epochs_tokens_order_quotas_and_failed_selection_are_checked() {
        let (mut l, f) = pair();
        let mut hello = f.hello();
        hello[0] ^= 1;
        assert!(l.start(hello, [0; 16]).is_err());
        assert!(l.election.winner.get().is_none());
        for stage in 0..3 {
            for field in 0..if stage == 0 { 3 } else { 4 } {
                let (mut l, mut f) = pair();
                let decision = begun(&mut l, &mut f);
                match stage {
                    0 => assert!(f.receive(corrupt(decision, field)).is_err()),
                    1 => {
                        let ack = f.receive(decision).unwrap().unwrap();
                        assert!(l.receive(corrupt(ack, field)).is_err());
                    }
                    2 => {
                        let ack = f.receive(decision).unwrap().unwrap();
                        let commit = l.receive(ack).unwrap().unwrap();
                        assert!(f.receive(corrupt(commit, field)).is_err());
                    }
                    _ => unreachable!(),
                }
            }
        }
        let (mut l, mut f) = pair();
        let decision = begun(&mut l, &mut f);
        assert!(l.sent_commit().is_err());
        let ack = f.receive(decision).unwrap().unwrap();
        let commit = l.receive(ack).unwrap().unwrap();
        assert!(!l.election.published.get());
        assert!(!f.election.published.get());
        l.sent_commit().unwrap();
        f.receive(commit).unwrap();
        assert_eq!(l.election.selected(), f.election.selected());
        assert!(l.receive(ack).is_err());
        assert!(f.receive(commit).is_err());
        let (mut l, mut f) = pair();
        begun(&mut l, &mut f);
        let slot = l.election.clone();
        drop(l);
        assert!(slot.stopped.get());
        assert!(slot.register().is_err());
        let slot = Election::new([1; 16], MAX_CANDIDATES);
        for i in 1..=MAX_CANDIDATES {
            assert_eq!(slot.register().unwrap(), i);
        }
        assert_eq!(
            slot.register().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn framed_negotiation_handles_partial_io_and_selects_one_stream() {
        let l = Election::new([1; 16], 2);
        let f = Election::new([2; 16], 2);
        let (mut a, mut b) = tokio::io::duplex(24);
        let (mut c, mut d) = tokio::io::duplex(24);
        let (li, fi, lj, fj) = (
            l.register().unwrap(),
            f.register().unwrap(),
            l.register().unwrap(),
            f.register().unwrap(),
        );
        let (x, y, z, w) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(
                negotiate(&mut a, l.clone(), li, true),
                negotiate(&mut b, f.clone(), fi, false),
                negotiate(&mut c, l.clone(), lj, true),
                negotiate(&mut d, f.clone(), fj, false)
            )
        })
        .await
        .unwrap();
        let (x, y, z, w) = (x.unwrap(), y.unwrap(), z.unwrap(), w.unwrap());
        assert_eq!(x, y);
        assert_eq!(z, w);
        assert_ne!(x, z);
        assert_eq!(l.selected(), f.selected());
        assert!(l.selected().is_some());
    }

    #[derive(serde::Deserialize)]
    struct Trace {
        count: usize,
        steps: Vec<Step>,
    }
    #[derive(serde::Deserialize)]
    struct Step {
        action: String,
        item: usize,
        state: Vec<u64>,
    }
    fn phase(value: &Option<Handshake>, leader: bool) -> u64 {
        match value.as_ref().map(|h| h.stage) {
            None => 5,
            Some(Stage::Hello) => 0,
            Some(Stage::Decision) => 1,
            Some(Stage::Ack) => {
                assert!(leader);
                1
            }
            Some(Stage::SendCommit) => {
                assert!(leader);
                2
            }
            Some(Stage::Commit) => 2,
            Some(Stage::Ready) => 3,
            Some(Stage::Rejected) => 4,
        }
    }
    fn wire(frame: Frame) -> Frame {
        Frame::parse(frame.bytes())
    }
    #[test]
    fn replay_tlc_arbitration_traces() {
        let path = capntproto_test_support::verification::input("CAPNTPROTO_ARBITRATION_TRACES")
            .expect("run this test through its verification driver");
        let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(!traces.is_empty());
        for (index, trace) in traces.into_iter().enumerate() {
            let le = Election::new([1; 16], trace.count);
            let fe = Election::new([2; 16], trace.count);
            let mut left = Vec::new();
            let mut right = Vec::new();
            for _ in 0..trace.count {
                let l = Handshake::new(le.clone(), le.register().unwrap(), true);
                let mut f = Handshake::new(fe.clone(), fe.register().unwrap(), false);
                f.start(l.hello(), [0; 16]).unwrap();
                left.push(Some(l));
                right.push(Some(f));
            }
            let mut decisions = [None; 2];
            let mut acknowledgements = [None; 2];
            let mut prepared = [None; 2];
            let mut commits = [None; 2];
            let mut sent = [false; 2];
            let (mut firstl, mut firstf, mut dl, mut df) = (0, 0, 0, 0);
            for step in trace.steps {
                let i = step.item.saturating_sub(1);
                match step.action.as_str() {
                    "select" => {
                        let mut hello = [0; 24];
                        hello[..8].copy_from_slice(MAGIC);
                        hello[8..].copy_from_slice(&fe.epoch);
                        let frame = left[i]
                            .as_mut()
                            .unwrap()
                            .start(hello, [i as u8 + 1; 16])
                            .unwrap()
                            .unwrap();
                        if frame.kind == 1 && firstl == 0 {
                            firstl = i as u64 + 1;
                        }
                        decisions[i] = Some(wire(frame));
                    }
                    "decision" => {
                        let frame = right[i]
                            .as_mut()
                            .unwrap()
                            .receive(decisions[i].take().unwrap())
                            .unwrap();
                        if frame.is_some_and(|frame| frame.kind == 2) && firstf == 0 {
                            firstf = i as u64 + 1;
                        }
                        acknowledgements[i] = frame.map(wire);
                    }
                    "ack" => {
                        prepared[i] = left[i]
                            .as_mut()
                            .unwrap()
                            .receive(acknowledgements[i].take().unwrap())
                            .unwrap()
                            .map(wire)
                    }
                    "commit" => {
                        commits[i] = Some(prepared[i].take().unwrap());
                        sent[i] = true;
                        if left[i].as_mut().unwrap().sent_commit().is_err() {
                            drop(left[i].take());
                        }
                    }
                    "confirm" => {
                        if right[i]
                            .as_mut()
                            .unwrap()
                            .receive(commits[i].take().unwrap())
                            .is_err()
                        {
                            drop(right[i].take());
                        }
                    }
                    "drop_l" => drop(left[i].take()),
                    "drop_f" => drop(right[i].take()),
                    "close_l" => le.close(),
                    "close_f" => fe.close(),
                    "data_l" => {
                        assert!(le.published.get() && !le.stopped.get());
                        assert_eq!(phase(&left[i], true), 3);
                        dl = i as u64 + 1;
                    }
                    "data_f" => {
                        assert!(fe.published.get() && !fe.stopped.get());
                        assert_eq!(phase(&right[i], false), 3);
                        df = i as u64 + 1;
                    }
                    "bad_epoch" => {
                        let f = right[i].as_mut().unwrap();
                        let frame = Frame {
                            kind: 3,
                            sender: [9; 16],
                            receiver: fe.epoch,
                            token: f.token,
                        };
                        assert!(f.receive(wire(frame)).is_err());
                        drop(right[i].take());
                    }
                    other => panic!("unknown action {other}"),
                }
                let l = |i| {
                    if i < trace.count {
                        phase(&left[i], true)
                    } else {
                        0
                    }
                };
                let f = |i| {
                    if i < trace.count {
                        phase(&right[i], false)
                    } else {
                        0
                    }
                };
                let d =
                    |i: usize| decisions[i].map_or(0, |f: Frame| if f.kind == 1 { 1 } else { 2 });
                let a = |i: usize| {
                    acknowledgements[i].map_or(0, |f: Frame| if f.kind == 2 { 1 } else { 2 })
                };
                let actual = vec![
                    l(0),
                    l(1),
                    f(0),
                    f(1),
                    d(0),
                    d(1),
                    a(0),
                    a(1),
                    commits[0].is_some() as u64,
                    commits[1].is_some() as u64,
                    sent[0] as u64,
                    sent[1] as u64,
                    le.winner.get().map_or(0, |v| v.0 as u64),
                    fe.winner.get().map_or(0, |v| v.0 as u64),
                    firstl,
                    firstf,
                    le.published.get() as u64,
                    fe.published.get() as u64,
                    le.stopped.get() as u64,
                    fe.stopped.get() as u64,
                    dl,
                    df,
                    0,
                    0,
                ];
                assert_eq!(
                    actual, step.state,
                    "trace {index} {} {}",
                    step.action, step.item
                );
            }
        }
    }
}

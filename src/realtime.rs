//! Deadline-aware snapshot capability. The atomic effect is publication into
//! this receiver's latest-value store, not execution of an arbitrary callback.
use crate::realtime_capnp as wire;
use capnp::{capability::Promise, Error};
use futures::{
    channel::oneshot,
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
    time::Duration,
};
mod config;
pub use config::Config;
mod record;
use record::{Metadata, Record, Tombstone};
mod snapshot;
pub use snapshot::Snapshot;
pub use wire::Outcome;
fn failed(s: &str) -> Error {
    Error::failed(s.into())
}

/// Nondecreasing ticks in [`Config::clock_domain`]. Producer deadlines must already
/// be mapped into this domain with the advertised maximum clock error.
pub trait Clock {
    fn now(&self) -> u64;
}
pub struct MonotonicClock {
    origin: tokio::time::Instant,
    epoch: u64,
}
impl MonotonicClock {
    /// A microsecond clock; choosing/mapping its reference epoch is external.
    pub fn new(origin: tokio::time::Instant, epoch: u64) -> Self {
        Self { origin, epoch }
    }
}
impl Clock for MonotonicClock {
    fn now(&self) -> u64 {
        let ticks = tokio::time::Instant::now()
            .saturating_duration_since(self.origin)
            .as_micros();
        u64::try_from(ticks)
            .ok()
            .and_then(|t| self.epoch.checked_add(t))
            .unwrap_or(u64::MAX)
    }
}
type Notices = Vec<(oneshot::Sender<Outcome>, Outcome)>;
fn notify(notices: Notices) {
    for (sender, outcome) in notices {
        let _ = sender.send(outcome);
    }
}
struct State {
    config: Config,
    closed: bool,
    last_clock: u64,
    high: Vec<u64>,
    visible: Vec<Option<Rc<Snapshot>>>,
    pending: BTreeMap<u32, u64>,
    records: BTreeMap<u64, Record>,
    next_waiter: u64,
    waiters: usize,
}
impl State {
    fn valid_sequence(&self, sequence: u64) -> capnp::Result<()> {
        if sequence == 0 || sequence > self.config.max_sequence() {
            Err(failed("realtime sequence outside stream namespace"))
        } else {
            Ok(())
        }
    }
    fn timely(&self, now: u64, deadline: u64) -> bool {
        now.checked_add(self.config.clock_skew())
            .is_some_and(|n| n < deadline)
    }
    fn finish(&mut self, sequence: u64, outcome: Outcome, notices: &mut Notices) {
        let record = self.records.get_mut(&sequence).expect("known sequence");
        let Some((key, waiters)) = record.finish(outcome) else {
            return;
        };
        if self.pending.get(&key) == Some(&sequence) {
            self.pending.remove(&key);
        }
        self.waiters -= waiters.len();
        notices.extend(waiters.into_values().map(|s| (s, outcome)));
    }

    fn close(&mut self, notices: &mut Notices) {
        self.closed = true;
        let pending: Vec<_> = self.pending.values().copied().collect();
        for sequence in pending {
            self.finish(sequence, Outcome::Closed, notices);
        }
    }
    fn observe_clock(&mut self, now: u64, notices: &mut Notices) -> capnp::Result<()> {
        if now < self.last_clock {
            self.close(notices);
            return Err(failed("realtime clock regressed; stream closed"));
        }
        self.last_clock = now;
        Ok(())
    }
    fn admit(
        &mut self,
        sequence: u64,
        key: u32,
        deadline: u64,
        bytes: &[u8],
        now: u64,
        notices: &mut Notices,
    ) -> capnp::Result<()> {
        self.valid_sequence(sequence)?;
        if key >= self.config.keys() || bytes.len() > self.config.max_payload_bytes() as usize {
            return Err(failed("realtime key/payload limit"));
        }
        let meta = Metadata {
            key,
            deadline,
            digest: ring::digest::digest(&ring::digest::SHA256, bytes)
                .as_ref()
                .try_into()
                .unwrap(),
        };
        if let Some(record) = self.records.get(&sequence) {
            if record.metadata().is_some_and(|old| old != meta) {
                return Err(failed("realtime sequence reused with different contents"));
            }
            if record.outcome().is_some() {
                return Ok(());
            }
            if self.waiters >= self.config.max_waiters() as usize || self.next_waiter == u64::MAX {
                return Err(Error::overloaded("realtime receipt limit".into()));
            }
            return Ok(());
        }
        // Exhaustion rejects before admitting or superseding any work.
        if self.waiters >= self.config.max_waiters() as usize || self.next_waiter == u64::MAX {
            return Err(Error::overloaded("realtime receipt limit".into()));
        }
        self.observe_clock(now, notices)?;
        let initial = if self.closed {
            Some(Outcome::Closed)
        } else if sequence < self.high[key as usize] {
            Some(Outcome::Superseded)
        } else {
            None
        };
        if let Some(outcome) = initial {
            self.records.insert(
                sequence,
                Record::Finished {
                    metadata: meta,
                    outcome,
                },
            );
            return Ok(());
        }
        if let Some(old) = self.pending.get(&key).copied() {
            self.finish(old, Outcome::Superseded, notices);
        }
        self.high[key as usize] = sequence;
        let terminal = if !self.timely(now, deadline) {
            Some(Outcome::Expired)
        } else if self.pending.len() >= self.config.capacity() as usize {
            Some(Outcome::Busy)
        } else {
            None
        };
        let record = if let Some(outcome) = terminal {
            Record::Finished {
                metadata: meta,
                outcome,
            }
        } else {
            self.pending.insert(key, sequence);
            Record::queued(sequence, meta, bytes)
        };
        self.records.insert(sequence, record);
        Ok(())
    }
}
struct Inner {
    state: RefCell<State>,
    clock: Rc<dyn Clock>,
}
impl Inner {
    fn close(&self) {
        let mut notices = Vec::new();
        self.state.borrow_mut().close(&mut notices);
        notify(notices);
    }
}
struct Owner(Rc<Inner>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.close();
    }
}
/// Application-side controller. Dropping its last clone closes the stream.
#[derive(Clone)]
pub struct Receiver(Rc<Owner>);
struct Waiter(Weak<Inner>, u64, u64);
impl Drop for Waiter {
    fn drop(&mut self) {
        if let Some(inner) = self.0.upgrade() {
            let removed = {
                let mut state = inner.state.borrow_mut();
                let removed = state
                    .records
                    .get_mut(&self.1)
                    .and_then(Record::pending_mut)
                    .and_then(|r| r.waiters.remove(&self.2));
                if removed.is_some() {
                    state.waiters -= 1;
                }
                removed
            };
            // A sender's destructor can wake a user-supplied waker. Release
            // the state borrow before running it, just as for normal receipts.
            drop(removed);
        }
    }
}
fn offer(
    inner: &Rc<Inner>,
    sequence: u64,
    key: u32,
    deadline: u64,
    bytes: &[u8],
) -> Promise<Outcome, Error> {
    let now = inner.clock.now();
    let mut notices = Vec::new();
    let result = (|| {
        let mut state = inner.state.borrow_mut();
        state.admit(sequence, key, deadline, bytes, now, &mut notices)?;
        if let Some(outcome) = state.records[&sequence].outcome() {
            return Ok(Promise::ok(outcome));
        }
        let id = state.next_waiter;
        state.next_waiter = id
            .checked_add(1)
            .ok_or_else(|| Error::overloaded("realtime waiter IDs exhausted".into()))?;
        let (tx, rx) = oneshot::channel();
        state
            .records
            .get_mut(&sequence)
            .and_then(Record::pending_mut)
            .expect("admitted pending snapshot")
            .waiters
            .insert(id, tx);
        state.waiters += 1;
        let guard = Waiter(Rc::downgrade(inner), sequence, id);
        Ok(Promise::from_future(async move {
            let outcome = rx
                .await
                .map_err(|_| Error::disconnected("realtime receipt lost".into()));
            drop(guard);
            outcome
        }))
    })();
    notify(notices);
    result.unwrap_or_else(Promise::err)
}
fn cancel(inner: &Inner, sequence: u64) -> capnp::Result<Outcome> {
    let mut notices = Vec::new();
    let outcome = {
        let mut state = inner.state.borrow_mut();
        state.valid_sequence(sequence)?;
        if let Some(outcome) = state.records.get(&sequence).and_then(Record::outcome) {
            return Ok(outcome);
        }
        let tombstone = if state.closed {
            Tombstone::Closed
        } else {
            Tombstone::Canceled
        };
        let outcome = tombstone.outcome();
        if let std::collections::btree_map::Entry::Vacant(entry) = state.records.entry(sequence) {
            entry.insert(Record::Tombstone(tombstone));
        } else {
            state.finish(sequence, outcome, &mut notices);
        }
        outcome
    };
    notify(notices);
    Ok(outcome)
}
impl Receiver {
    /// Allocate a stream from checked limits and its application clock.
    #[must_use = "dropping the receiver immediately closes the stream"]
    pub fn new(config: Config, clock: Rc<dyn Clock>) -> (Self, wire::snapshots::Client) {
        let inner = Rc::new(Inner {
            state: RefCell::new(State {
                high: vec![0; config.keys() as usize],
                visible: vec![None; config.keys() as usize],
                config,
                closed: false,
                last_clock: clock.now(),
                pending: BTreeMap::new(),
                records: BTreeMap::new(),
                next_waiter: 0,
                waiters: 0,
            }),
            clock,
        });
        let client = capnp_rpc::new_client(Service(inner.clone()));
        (Self(Rc::new(Owner(inner))), client)
    }
    /// Also usable by an authenticated unordered adapter. The caller must hold
    /// authority to this Receiver; sequence numbers alone never grant access.
    pub fn offer(
        &self,
        sequence: u64,
        key: u32,
        deadline: u64,
        bytes: &[u8],
    ) -> Promise<Outcome, Error> {
        offer(&self.0 .0, sequence, key, deadline, bytes)
    }
    pub fn cancel(&self, sequence: u64) -> capnp::Result<Outcome> {
        cancel(&self.0 .0, sequence)
    }
    pub fn close(&self) {
        self.0 .0.close();
    }
    pub fn is_closed(&self) -> bool {
        self.0 .0.state.borrow().closed
    }
    pub fn get(&self, key: u32) -> Option<Rc<Snapshot>> {
        self.0
             .0
            .state
            .borrow()
            .visible
            .get(key as usize)
            .and_then(Clone::clone)
    }
    pub fn pending(&self) -> Vec<u64> {
        self.0 .0.state.borrow().pending.values().copied().collect()
    }
    pub fn status(&self, sequence: u64) -> Option<Outcome> {
        self.0
             .0
            .state
            .borrow()
            .records
            .get(&sequence)
            .and_then(Record::outcome)
    }
    pub fn waiter_count(&self) -> usize {
        self.0 .0.state.borrow().waiters
    }
    /// Commit one queued snapshot. The final clock check and visible publication
    /// occur synchronously with no application callback or await in between.
    pub fn apply(&self, sequence: u64) -> capnp::Result<Outcome> {
        let now = self.0 .0.clock.now();
        let mut notices = Vec::new();
        let result = (|| {
            let mut state = self.0 .0.state.borrow_mut();
            if let Some(outcome) = state.records.get(&sequence).and_then(Record::outcome) {
                return Ok(outcome);
            }
            state.observe_clock(now, &mut notices)?;
            let snapshot = state
                .records
                .get(&sequence)
                .and_then(Record::pending)
                .map(|pending| pending.snapshot.clone())
                .ok_or_else(|| failed("snapshot not queued"))?;
            let outcome = if state.closed {
                Outcome::Closed
            } else if state.high[snapshot.key() as usize] != sequence {
                Outcome::Superseded
            } else if !state.timely(now, snapshot.not_after()) {
                Outcome::Expired
            } else {
                Outcome::Applied
            };
            if outcome == Outcome::Applied {
                let key = snapshot.key() as usize;
                state.visible[key] = Some(snapshot);
            }
            state.finish(sequence, outcome, &mut notices);
            Ok(outcome)
        })();
        notify(notices);
        result
    }
    /// Expire queued snapshots without publishing them.
    pub fn expire(&self) -> capnp::Result<()> {
        let now = self.0 .0.clock.now();
        let mut notices = Vec::new();
        let result = (|| {
            let mut state = self.0 .0.state.borrow_mut();
            state.observe_clock(now, &mut notices)?;
            let expired: Vec<_> = state
                .pending
                .values()
                .copied()
                .filter(|s| {
                    !state.timely(
                        now,
                        state.records[s]
                            .pending()
                            .expect("queued index")
                            .snapshot
                            .not_after(),
                    )
                })
                .collect();
            for seq in expired {
                state.finish(seq, Outcome::Expired, &mut notices);
            }
            Ok(())
        })();
        notify(notices);
        result
    }
    /// Optional soft-realtime worker. Scheduling controls replacement/coalescing;
    /// it cannot promise delivery latency. Visible state remains readable on close.
    pub async fn run(self, period: Duration) -> capnp::Result<()> {
        if period.is_zero() {
            return Err(failed("realtime worker period must be positive"));
        }
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while !self.is_closed() {
            interval.tick().await;
            self.expire()?;
            for seq in self.pending() {
                self.apply(seq)?;
            }
        }
        Ok(())
    }
}
struct Service(Rc<Inner>);
impl wire::snapshots::Server for Service {
    async fn describe(
        self: Rc<Self>,
        _: wire::snapshots::DescribeParams,
        mut r: wire::snapshots::DescribeResults,
    ) -> capnp::Result<()> {
        self.0.state.borrow().config.write(r.get().init_config());
        Ok(())
    }
    async fn offer(
        self: Rc<Self>,
        p: wire::snapshots::OfferParams,
        mut r: wire::snapshots::OfferResults,
    ) -> capnp::Result<()> {
        let receipt = {
            let reader = p.get()?;
            offer(
                &self.0,
                reader.get_sequence(),
                reader.get_key(),
                reader.get_not_after(),
                reader.get_snapshot()?,
            )
        };
        drop(p);
        r.get().set_outcome(receipt.await?);
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        p: wire::snapshots::CancelParams,
        mut r: wire::snapshots::CancelResults,
    ) -> capnp::Result<()> {
        r.get()
            .set_outcome(cancel(&self.0, p.get()?.get_sequence())?);
        Ok(())
    }
    async fn close(
        self: Rc<Self>,
        _: wire::snapshots::CloseParams,
        _: wire::snapshots::CloseResults,
    ) -> capnp::Result<()> {
        self.0.close();
        Ok(())
    }
}
/// Clones share a sequence allocator and close barrier. Create one Sender for
/// each fresh stream capability, never a new allocator for an existing stream.
#[derive(Clone)]
pub struct Sender {
    client: wire::snapshots::Client,
    config: Config,
    state: Rc<RefCell<(u64, bool)>>,
}
#[derive(Clone)]
pub struct Receipt {
    sequence: u64,
    result: Shared<LocalBoxFuture<'static, capnp::Result<Outcome>>>,
}
impl Receipt {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub async fn outcome(&self) -> capnp::Result<Outcome> {
        self.result.clone().await
    }
    /// None means unknown, not expired/non-execution. The retained receipt can
    /// still be awaited after this local wait times out.
    pub async fn wait_until(
        &self,
        deadline: tokio::time::Instant,
    ) -> capnp::Result<Option<Outcome>> {
        match tokio::time::timeout_at(deadline, self.result.clone()).await {
            Ok(r) => r.map(Some),
            Err(_) => Ok(None),
        }
    }
}
impl Sender {
    pub async fn connect(client: wire::snapshots::Client) -> capnp::Result<Self> {
        let response = client.describe_request().send().promise.await?;
        Ok(Self {
            config: Config::read(response.get()?.get_config()?)?,
            client,
            state: Rc::new(RefCell::new((1, false))),
        })
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn offer(&self, key: u32, not_after: u64, bytes: &[u8]) -> capnp::Result<Receipt> {
        let mut state = self.state.borrow_mut();
        if state.1 {
            return Err(failed("realtime sender closed"));
        }
        if state.0 > self.config.max_sequence()
            || key >= self.config.keys()
            || bytes.len() > self.config.max_payload_bytes() as usize
        {
            return Err(failed("realtime offer outside negotiated limits"));
        }
        let sequence = state.0;
        state.0 += 1;
        drop(state);
        let mut request = self.client.offer_request();
        let mut p = request.get();
        p.set_sequence(sequence);
        p.set_key(key);
        p.set_not_after(not_after);
        p.set_snapshot(bytes);
        let promise = request.send().promise;
        Ok(Receipt {
            sequence,
            result: async move { Ok(promise.await?.get()?.get_outcome()?) }
                .boxed_local()
                .shared(),
        })
    }
    pub fn cancel(&self, sequence: u64) -> Promise<Outcome, Error> {
        if sequence == 0 || sequence > self.config.max_sequence() {
            return Promise::err(failed("realtime sequence outside namespace"));
        }
        let mut request = self.client.cancel_request();
        request.get().set_sequence(sequence);
        let promise = request.send().promise;
        Promise::from_future(async move { Ok(promise.await?.get()?.get_outcome()?) })
    }
    /// Stop submission immediately; the promise acknowledges the receiver's
    /// close barrier. A lost close reply does not establish remote closure.
    pub fn close(&self) -> Promise<(), Error> {
        self.state.borrow_mut().1 = true;
        let promise = self.client.close_request().send().promise;
        Promise::from_future(async move {
            promise.await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod lifecycle_tests;

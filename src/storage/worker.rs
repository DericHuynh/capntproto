//! Bounded, asynchronous access to one exclusively owned [`Store`].
//!
//! This is a privileged local host API, not a capability authorization boundary.
//! Submission copies borrowed payloads only after reserving count/byte credit.
//! Dropping a request cancels queued work, but cannot undo started I/O.
#![forbid(unsafe_code)]
use super::{
    Compaction, ComponentSnapshot, ComponentUpdate, Limits, ObjectKey, Retention, Revision,
    Snapshot, Store, Update,
};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Condvar, Mutex,
    },
    task::{Context, Poll},
    thread,
    time::Instant,
};
use tokio::sync::{oneshot, watch};

mod operations;
pub use operations::ObjectStatus;
#[cfg(test)]
mod tests;

/// Independent credit pool. Credits include preparing, queued, executing and
/// completed-but-unobserved requests, not just channel occupancy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub requests: usize,
    pub bytes: usize,
}

/// Host policy. Small and large requests never borrow each other's credit,
/// preventing one size class from consuming every admission slot.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub small_payload_bytes: usize,
    pub small: Budget,
    pub large: Budget,
    /// Aggregate allowance for one host-selected principal across both pools.
    pub per_principal: Budget,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            small_payload_bytes: 4096,
            small: Budget {
                requests: 48,
                bytes: 1024 * 1024,
            },
            large: Budget {
                requests: 16,
                bytes: 32 * 1024 * 1024,
            },
            per_principal: Budget {
                requests: 8,
                bytes: 16 * 1024 * 1024,
            },
        }
    }
}
impl Config {
    fn validate(self) -> Result<(), StartError> {
        if self.small.requests == 0
            || self.large.requests == 0
            || self.per_principal.requests == 0
            || self.per_principal.bytes == 0
            || self.small.bytes < self.small_payload_bytes
            || self.large.bytes <= self.small_payload_bytes
            || self
                .small
                .requests
                .checked_add(self.large.requests)
                .is_none()
            || self.small.bytes.checked_add(self.large.bytes).is_none()
        {
            return Err(StartError::Config);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    WholeEntry,
    Components,
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("invalid storage worker budgets")]
    Config,
    #[error(transparent)]
    Storage(#[from] super::Error),
    #[error("could not spawn storage owner: {0}")]
    Thread(#[source] std::io::Error),
    #[error("storage owner stopped during startup")]
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Requests,
    Bytes,
    PrincipalRequests,
    PrincipalBytes,
}

/// Submission failure: this attempt has not been enqueued or executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdmissionError {
    #[error("storage admission is closed")]
    Closed,
    #[error("storage owner is degraded")]
    Degraded,
    #[error("storage admission exhausted {0:?}")]
    Overloaded(Resource),
    #[error("request cannot fit its payload budget")]
    TooLarge,
    #[error("invalid request shape")]
    InvalidRequest,
}

/// Execution result, kept separate from admission errors and caller timeouts.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request cancelled before execution")]
    Cancelled,
    #[error("request deadline expired before execution")]
    Expired,
    #[error("storage owner stopped before executing this request")]
    Unavailable,
    #[error("request not applied: {0}")]
    NotApplied(#[source] super::Error),
    #[error("storage outcome uncertain; owner quarantined: {0}")]
    Uncertain(#[source] super::Error),
    #[error("storage owner lost during execution; outcome uncertain")]
    OwnerLost,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Checked on the owner immediately before starting. This is not an I/O
    /// timeout and does not promise completion before the deadline.
    pub deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Running,
    Closing,
    Degraded,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub requests: usize,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Diagnostics {
    pub phase: Phase,
    pub degraded: bool,
    pub small: Usage,
    pub large: Usage,
    pub queued: usize,
    pub executing: bool,
    pub oldest_queued_at: Option<Instant>,
    pub admitted: u64,
    pub rejected: u64,
    pub succeeded: u64,
    pub not_applied: u64,
    pub cancelled: u64,
    pub expired: u64,
    pub uncertain: u64,
    pub lost_replies: u64,
    pub last_completed_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownMode {
    /// Finish admitted requests, subject to their cancellation and deadlines.
    Drain,
    /// Reject queued work. An executing request is still allowed to finish.
    CancelQueued,
}

/// The Store has been dropped before this report is published. Snapshots held
/// by callers or unread replies may still retain file locks.
#[derive(Clone, Copy, Debug)]
pub struct ShutdownReport {
    pub degraded: bool,
    pub owner_panicked: bool,
    pub succeeded: u64,
    pub not_applied: u64,
    pub cancelled: u64,
    pub expired: u64,
    pub uncertain: u64,
}

/// A resumable shutdown observation. Dropping it does not interrupt draining.
pub struct Shutdown {
    done: watch::Receiver<Option<ShutdownReport>>,
}
impl Shutdown {
    pub async fn wait(mut self) -> ShutdownReport {
        loop {
            if let Some(report) = *self.done.borrow_and_update() {
                return report;
            }
            self.done
                .changed()
                .await
                .expect("owner retains completion sender");
        }
    }
}

/// Owns the storage thread's lifetime. Dropping an open Worker stops admission
/// and cancels queued work without blocking the calling executor. An explicitly
/// requested drain continues if the Worker or shutdown observer is dropped.
pub struct Worker {
    shared: Arc<Shared>,
}
impl Worker {
    /// Open and recover on the dedicated owner thread. Cancelling startup closes
    /// the owner once the outstanding open/recovery call returns.
    pub async fn open(
        path: impl AsRef<Path>,
        format: Format,
        limits: Limits,
        config: Config,
    ) -> Result<Self, StartError> {
        let path = path.as_ref().to_owned();
        let (worker, ready) = Self::spawn(config, move || match format {
            Format::WholeEntry => Store::open_with_limits(path, limits),
            Format::Components => Store::open_components_with_limits(path, limits),
        })?;
        ready.await.map_err(|_| StartError::Stopped)??;
        Ok(worker)
    }

    /// Move an already-open Store to its owner. The caller is responsible for
    /// isolating the original synchronous open/recovery operation.
    pub async fn from_store(store: Store, config: Config) -> Result<Self, StartError> {
        let (worker, ready) = Self::spawn(config, move || {
            store.healthy()?;
            Ok(store)
        })?;
        ready.await.map_err(|_| StartError::Stopped)??;
        Ok(worker)
    }

    fn spawn(
        config: Config,
        open: impl FnOnce() -> super::Result<Store> + Send + 'static,
    ) -> Result<(Self, oneshot::Receiver<super::Result<()>>), StartError> {
        config.validate()?;
        let shared = Arc::new(Shared::new(config));
        let owner = shared.clone();
        let (ready_tx, ready_rx) = oneshot::channel();
        thread::Builder::new()
            .name("reproto-storage".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match open() {
                        Ok(mut store) => {
                            if ready_tx.send(Ok(())).is_ok() {
                                owner.run(&mut store);
                            }
                            // Drop all Store-owned mappings/locks before reporting completion.
                            drop(store);
                        }
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                        }
                    }
                }));
                owner.finish(result.is_err());
            })
            .map_err(StartError::Thread)?;
        Ok((Self { shared }, ready_rx))
    }

    /// A trusted quota identity. Clones and repeated calls with the same ID
    /// share one allowance. Do not let remote callers choose arbitrary IDs.
    pub fn client(&self, principal: u64) -> Client {
        Client {
            shared: self.shared.clone(),
            principal,
        }
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.shared.diagnostics()
    }
    /// Close admission synchronously, independent of queue capacity. Calling
    /// CancelQueued later can escalate a previous Drain; shutdown never reopens.
    pub fn shutdown(&self, mode: ShutdownMode) -> Shutdown {
        self.shared.close(mode, false);
        Shutdown {
            done: self.shared.done.subscribe(),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.close(ShutdownMode::CancelQueued, true);
    }
}

/// Cloneable, thread-safe access to one Store. All admitted operations execute
/// in FIFO enqueue order across principals and size classes.
#[derive(Clone)]
pub struct Client {
    shared: Arc<Shared>,
    principal: u64,
}

const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;
const FINISHED: u8 = 3;

/// Cancellation confirmed while queued, or too late to promise non-execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancellation {
    Cancelled,
    TooLate,
}

/// Future for one admitted operation. It holds result credit until polled ready
/// or dropped. Drop attempts queued cancellation; started work keeps running.
#[must_use = "await the outcome; dropping only cancels work that has not started"]
pub struct Pending<T> {
    state: Arc<AtomicU8>,
    result: oneshot::Receiver<Completion<T>>,
}
impl<T> Pending<T> {
    /// Nonblocking arbitration against the owner's start transition. Confirmed
    /// cancellation prevents execution, but credit is reclaimed when the owner
    /// removes the queued payload. Awaiting still returns Cancelled.
    pub fn cancel(&self) -> Cancellation {
        match self
            .state
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(CANCELLED) => Cancellation::Cancelled,
            Err(_) => Cancellation::TooLate,
        }
    }
}
impl<T> Future for Pending<T> {
    type Output = Result<T, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.result).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(completed)) => Poll::Ready(completed.result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(match self.state.load(Ordering::Acquire) {
                CANCELLED => Error::Cancelled,
                QUEUED => Error::Unavailable,
                // Missing completion after start/finish is never evidence
                // of rollback, even if the operation had already returned.
                _ => Error::OwnerLost,
            })),
        }
    }
}
impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Completion<T> {
    result: Result<T, Error>,
    _permit: Permit,
}
struct Permit {
    shared: Arc<Shared>,
    lane: usize,
    principal: u64,
    bytes: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        let usage = &mut inner.usage[self.lane];
        usage.requests -= 1;
        usage.bytes -= self.bytes;
        let usage = inner.principals.get_mut(&self.principal).unwrap();
        usage.requests -= 1;
        usage.bytes -= self.bytes;
        if usage.requests == 0 {
            inner.principals.remove(&self.principal);
        }
    }
}

trait Job: Send {
    fn begin(&self) -> Start;
    fn run(self: Box<Self>, store: &mut Store, start: Start) -> bool;
    fn reject(self: Box<Self>);
}
enum Start {
    Execute,
    Cancelled,
    Expired,
}
struct Queued {
    at: Instant,
    job: Box<dyn Job>,
}
struct Task<T, F> {
    operation: F,
    reply: oneshot::Sender<Completion<T>>,
    state: Arc<AtomicU8>,
    options: Options,
    permit: Permit,
}
impl<T: Send + 'static, F: FnOnce(&mut Store) -> super::Result<T> + Send + 'static> Job
    for Task<T, F>
{
    fn begin(&self) -> Start {
        if self.options.deadline.is_some_and(|d| Instant::now() >= d) {
            match self
                .state
                .compare_exchange(QUEUED, FINISHED, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => Start::Expired,
                Err(_) => Start::Cancelled,
            }
        } else {
            match self
                .state
                .compare_exchange(QUEUED, STARTED, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => Start::Execute,
                Err(_) => Start::Cancelled,
            }
        }
    }
    fn run(self: Box<Self>, store: &mut Store, start: Start) -> bool {
        let Self {
            operation,
            reply,
            state,
            permit,
            ..
        } = *self;
        let result = match start {
            Start::Expired => {
                drop(operation);
                Err(Error::Expired)
            }
            Start::Cancelled => {
                drop(operation);
                Err(Error::Cancelled)
            }
            Start::Execute => match operation(store) {
                Ok(value) => Ok(value),
                Err(error) if store.poisoned || matches!(error, super::Error::Io(_)) => {
                    Err(Error::Uncertain(error))
                }
                Err(error) => Err(Error::NotApplied(error)),
            },
        };
        let degraded = matches!(result, Err(Error::Uncertain(_)));
        permit.shared.completed(&result);
        state.store(FINISHED, Ordering::Release);
        deliver(reply, result, permit);
        degraded
    }
    fn reject(self: Box<Self>) {
        let Self {
            operation,
            reply,
            state,
            permit,
            ..
        } = *self;
        drop(operation);
        let result = Err(if state.swap(FINISHED, Ordering::AcqRel) == CANCELLED {
            Error::Cancelled
        } else {
            Error::Unavailable
        });
        permit.shared.completed(&result);
        deliver(reply, result, permit);
    }
}
fn deliver<T>(reply: oneshot::Sender<Completion<T>>, result: Result<T, Error>, permit: Permit) {
    let shared = permit.shared.clone();
    if reply
        .send(Completion {
            result,
            _permit: permit,
        })
        .is_err()
    {
        shared.inner.lock().unwrap().stats.lost_replies += 1;
    }
}

struct Inner {
    phase: Phase,
    cancel_queued: bool,
    usage: [Usage; 2],
    principals: BTreeMap<u64, Usage>,
    queue: VecDeque<Queued>,
    stats: Diagnostics,
}
struct Shared {
    config: Config,
    inner: Mutex<Inner>,
    wake: Condvar,
    done: watch::Sender<Option<ShutdownReport>>,
}
impl Shared {
    fn new(config: Config) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                phase: Phase::Running,
                cancel_queued: false,
                usage: [Usage::default(); 2],
                principals: BTreeMap::new(),
                queue: VecDeque::new(),
                stats: Diagnostics {
                    phase: Phase::Running,
                    degraded: false,
                    small: Usage::default(),
                    large: Usage::default(),
                    queued: 0,
                    executing: false,
                    oldest_queued_at: None,
                    admitted: 0,
                    rejected: 0,
                    succeeded: 0,
                    not_applied: 0,
                    cancelled: 0,
                    expired: 0,
                    uncertain: 0,
                    lost_replies: 0,
                    last_completed_at: None,
                },
            }),
            wake: Condvar::new(),
            done: watch::channel(None).0,
        }
    }
    fn diagnostics(&self) -> Diagnostics {
        let inner = self.inner.lock().unwrap();
        Diagnostics {
            phase: inner.phase,
            small: inner.usage[0],
            large: inner.usage[1],
            queued: inner.queue.len(),
            oldest_queued_at: inner.queue.front().map(|q| q.at),
            ..inner.stats
        }
    }
    fn closed(inner: &Inner) -> Result<(), AdmissionError> {
        if inner.stats.degraded {
            Err(AdmissionError::Degraded)
        } else if inner.phase != Phase::Running {
            Err(AdmissionError::Closed)
        } else {
            Ok(())
        }
    }
    fn reserve(self: &Arc<Self>, principal: u64, bytes: usize) -> Result<Permit, AdmissionError> {
        let mut inner = self.inner.lock().unwrap();
        let lane = usize::from(bytes > self.config.small_payload_bytes);
        let budget = [self.config.small, self.config.large][lane];
        let result = Self::closed(&inner).and_then(|()| {
            if bytes > budget.bytes || bytes > self.config.per_principal.bytes {
                return Err(AdmissionError::TooLarge);
            }
            let usage = inner.usage[lane];
            let own = inner
                .principals
                .get(&principal)
                .copied()
                .unwrap_or_default();
            let exhausted = if own.requests >= self.config.per_principal.requests {
                Some(Resource::PrincipalRequests)
            } else if bytes > self.config.per_principal.bytes - own.bytes {
                Some(Resource::PrincipalBytes)
            } else if usage.requests >= budget.requests {
                Some(Resource::Requests)
            } else if bytes > budget.bytes - usage.bytes {
                Some(Resource::Bytes)
            } else {
                None
            };
            exhausted.map_or(Ok(()), |r| Err(AdmissionError::Overloaded(r)))
        });
        if let Err(error) = result {
            inner.stats.rejected += 1;
            return Err(error);
        }
        inner.usage[lane].requests += 1;
        inner.usage[lane].bytes += bytes;
        let own = inner.principals.entry(principal).or_default();
        own.requests += 1;
        own.bytes += bytes;
        Ok(Permit {
            shared: self.clone(),
            lane,
            principal,
            bytes,
        })
    }
    fn completed<T>(&self, result: &Result<T, Error>) {
        let mut inner = self.inner.lock().unwrap();
        let stats = &mut inner.stats;
        match result {
            Ok(_) => stats.succeeded += 1,
            Err(Error::Cancelled) => stats.cancelled += 1,
            Err(Error::Expired) => stats.expired += 1,
            Err(Error::Uncertain(_) | Error::OwnerLost) => stats.uncertain += 1,
            Err(Error::NotApplied(_) | Error::Unavailable) => stats.not_applied += 1,
        }
        stats.last_completed_at = Some(Instant::now());
        if matches!(result, Err(Error::Uncertain(_))) {
            inner.stats.degraded = true;
            inner.phase = Phase::Degraded;
        }
    }
    fn close(&self, mode: ShutdownMode, only_if_running: bool) {
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.phase == Phase::Stopped || (only_if_running && inner.phase != Phase::Running) {
                return;
            }
            if inner.phase != Phase::Degraded {
                inner.phase = Phase::Closing;
            }
            inner.cancel_queued |= mode == ShutdownMode::CancelQueued;
        }
        self.wake.notify_one();
    }
    fn run(&self, store: &mut Store) {
        loop {
            let (queued, start) = {
                let mut inner = self.inner.lock().unwrap();
                loop {
                    if let Some(queued) = inner.queue.pop_front() {
                        inner.stats.executing = true;
                        // Start and shutdown are ordered by the same queue lock;
                        // cancellation can also win this transition atomically.
                        let start = if inner.cancel_queued {
                            None
                        } else {
                            Some(queued.job.begin())
                        };
                        break (queued, start);
                    }
                    if inner.phase != Phase::Running {
                        return;
                    }
                    inner = self.wake.wait(inner).unwrap();
                }
            };
            let degraded = match start {
                Some(start) => queued.job.run(store, start),
                None => {
                    queued.job.reject();
                    false
                }
            };
            let mut inner = self.inner.lock().unwrap();
            inner.stats.executing = false;
            if degraded {
                inner.stats.degraded = true;
                inner.phase = Phase::Degraded;
                return;
            }
        }
    }
    fn finish(&self, panicked: bool) {
        let queued = {
            let mut inner = self.inner.lock().unwrap();
            if panicked {
                inner.stats.degraded = true;
                if inner.stats.executing {
                    inner.stats.uncertain += 1;
                }
            }
            inner.stats.executing = false;
            inner.phase = Phase::Stopped;
            std::mem::take(&mut inner.queue)
        };
        for queued in queued {
            queued.job.reject();
        }
        let stats = self.diagnostics();
        self.done.send_replace(Some(ShutdownReport {
            degraded: stats.degraded,
            owner_panicked: panicked,
            succeeded: stats.succeeded,
            not_applied: stats.not_applied,
            cancelled: stats.cancelled,
            expired: stats.expired,
            uncertain: stats.uncertain,
        }));
    }
}

impl Client {
    pub fn diagnostics(&self) -> Diagnostics {
        self.shared.diagnostics()
    }
    fn rejection(&self, error: AdmissionError) -> AdmissionError {
        self.shared.inner.lock().unwrap().stats.rejected += 1;
        error
    }
    fn submit<T: Send + 'static, F: FnOnce(&mut Store) -> super::Result<T> + Send + 'static>(
        &self,
        bytes: usize,
        options: Options,
        prepare: impl FnOnce() -> F,
    ) -> Result<Pending<T>, AdmissionError> {
        let permit = self.shared.reserve(self.principal, bytes)?;
        let operation = prepare();
        let state = Arc::new(AtomicU8::new(QUEUED));
        let (reply, result) = oneshot::channel();
        let task = Task {
            operation,
            reply,
            state: state.clone(),
            options,
            permit,
        };
        let mut inner = self.shared.inner.lock().unwrap();
        if let Err(error) = Shared::closed(&inner) {
            inner.stats.rejected += 1;
            drop(inner);
            return Err(error);
        }
        inner.queue.push_back(Queued {
            at: Instant::now(),
            job: Box::new(task),
        });
        inner.stats.admitted += 1;
        drop(inner);
        self.shared.wake.notify_one();
        Ok(Pending { state, result })
    }
}

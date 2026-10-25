//! One turn has two phases: application/protocol progress, then transport output.
use super::RpcSystem;
use alloc::rc::Rc;
use alloc::sync::Arc;
use alloc::task::Wake;
use alloc::{boxed::Box, vec::Vec};
use capnp::{capability::Promise, Error};
use capntproto_rpc_engine::completion::shared;
use core::cell::RefCell;
use core::future::Future;
use core::num::NonZeroUsize;
use core::pin::pin;
use core::pin::Pin;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use core::task::Waker;
use futures::task::AtomicWaker;

trait Driver {
    fn progress(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>>;
    fn output(&mut self, cx: &mut Context<'_>);
}
impl<V: 'static> Driver for RpcSystem<V> {
    fn progress(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        self.poll_progress(cx)
    }
    fn output(&mut self, cx: &mut Context<'_>) {
        self.poll_output(cx);
    }
}

struct Running {
    driver: Box<dyn Driver>,
    completion: shared::Sender<capnp::Result<()>>,
    progress: Arc<Lane>,
    progress_waker: Waker,
    output: Arc<Lane>,
    output_waker: Waker,
}
struct Pending {
    queued: Vec<Running>,
    active: usize,
    maximum: usize,
    closed: bool,
}
#[derive(Default)]
struct Activity {
    state: AtomicU8,
    parent: AtomicWaker,
}
impl Activity {
    fn notify(&self, source: u8) {
        if self.state.fetch_or(READY | source, Ordering::AcqRel) & POLLING == 0 {
            self.parent.wake();
        }
    }
}
// Each source owns its readiness. An application wake must not poll every
// sleeping connection, and a writer wake must not speculate on input readiness.
struct Lane {
    ready: AtomicBool,
    parent: Arc<Activity>,
    source: u8,
}
impl Wake for Lane {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        self.parent.notify(self.source);
    }
}
fn lane(parent: &Arc<Activity>, source: u8) -> (Arc<Lane>, Waker) {
    let lane = Arc::new(Lane {
        ready: AtomicBool::new(true),
        parent: parent.clone(),
        source,
    });
    let waker = Waker::from(lane.clone());
    (lane, waker)
}
const READY: u8 = 1;
const POLLING: u8 = 2;
const PROGRESS: u8 = 4;
struct Turn<'a>(&'a Activity);
impl Drop for Turn<'_> {
    fn drop(&mut self) {
        if self.0.state.fetch_and(!POLLING, Ordering::AcqRel) & READY != 0 {
            self.0.parent.wake();
        }
    }
}

/// Coordinates local application work and RPC systems before flushing output.
///
/// Put related connections in the same runtime, then pass the application to
/// [`run`](Self::run). Each turn progresses ready systems and drains ready
/// application continuations and ready protocol input before polling writers.
/// Output-only wakes do not extend the progress phase. A bounded number of rounds prevents a
/// continuously waking application from starving output or its executor.
/// Dropping the runtime aborts its systems and closes its handles.
pub struct Runtime {
    pending: Rc<RefCell<Pending>>,
    systems: Vec<Running>,
    activity: Arc<Activity>,
    application: Arc<Lane>,
    application_waker: Waker,
}
impl Runtime {
    /// Bound the number of systems, including systems queued by a handle.
    pub fn new(maximum: NonZeroUsize) -> Self {
        let activity = Arc::new(Activity::default());
        let (application, application_waker) = lane(&activity, PROGRESS);
        Self {
            pending: Rc::new(RefCell::new(Pending {
                queued: Vec::new(),
                active: 0,
                maximum: maximum.get(),
                closed: false,
            })),
            systems: Vec::new(),
            activity,
            application,
            application_waker,
        }
    }
    pub fn handle(&self) -> RuntimeHandle {
        RuntimeHandle {
            pending: self.pending.clone(),
            activity: self.activity.clone(),
        }
    }
    /// Drive the application and all admitted systems together. System errors
    /// are delivered to their completion promises and outstanding RPC clients;
    /// one disconnected peer does not terminate other systems or the application.
    ///
    /// Completion returns immediately; it does not drain or close RPC systems.
    /// Await their disconnectors inside the application when orderly shutdown is
    /// required. Dropping the runtime instead aborts outstanding work.
    pub async fn run<F: Future>(&mut self, application: F) -> F::Output {
        let mut application = pin!(application);
        self.application.ready.store(true, Ordering::Release);
        core::future::poll_fn(|cx| self.poll(application.as_mut(), cx)).await
    }
    fn poll<F: Future>(
        &mut self,
        mut application: Pin<&mut F>,
        cx: &mut Context<'_>,
    ) -> Poll<F::Output> {
        self.activity.parent.register(cx.waker());
        self.activity.state.fetch_or(POLLING, Ordering::AcqRel);
        let _turn = Turn(&self.activity);
        let application_waker = self.application_waker.clone();
        let mut application_cx = Context::from_waker(&application_waker);
        let mut rounds = 0;
        loop {
            loop {
                rounds += 1;
                self.activity
                    .state
                    .fetch_and(!(READY | PROGRESS), Ordering::AcqRel);
                if self.application.ready.swap(false, Ordering::AcqRel) {
                    if let Poll::Ready(result) = application.as_mut().poll(&mut application_cx) {
                        return Poll::Ready(result);
                    }
                }
                // Never hold admission storage while polling or destroying user
                // futures: either can add another system through a handle.
                self.systems.append(&mut self.pending.borrow_mut().queued);
                let mut index = 0;
                while index < self.systems.len() {
                    let running = &mut self.systems[index];
                    if !running.progress.ready.swap(false, Ordering::AcqRel) {
                        index += 1;
                        continue;
                    }
                    match running
                        .driver
                        .progress(&mut Context::from_waker(&running.progress_waker))
                    {
                        Poll::Pending => index += 1,
                        Poll::Ready(done) => {
                            let system = self.systems.swap_remove(index);
                            self.pending.borrow_mut().active -= 1;
                            let _ = system.completion.send(done);
                            drop(system.driver);
                        }
                    }
                }
                // The runtime is the sole hosted batching owner. A buffered
                // input wake gets another bounded progress round; an output
                // wake only requests the physical output phase below.
                if rounds == 32
                    || self
                        .activity
                        .state
                        .fetch_and(!(READY | PROGRESS), Ordering::AcqRel)
                        & PROGRESS
                        == 0
                {
                    break;
                }
            }
            for system in &mut self.systems {
                if system.output.ready.swap(false, Ordering::AcqRel) {
                    system
                        .driver
                        .output(&mut Context::from_waker(&system.output_waker));
                }
            }
            if rounds == 32 {
                self.activity.state.fetch_or(READY, Ordering::AcqRel);
                return Poll::Pending;
            }
            // Output completion can make protocol/application work ready. Run
            // the next turn directly, sharing the same fairness allowance,
            // rather than forcing an otherwise unnecessary executor round trip.
            if self
                .activity
                .state
                .fetch_and(!(READY | PROGRESS), Ordering::AcqRel)
                & READY
                == 0
            {
                return Poll::Pending;
            }
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let queued = {
            let mut pending = self.pending.borrow_mut();
            pending.closed = true;
            pending.active = 0;
            core::mem::take(&mut pending.queued)
        };
        drop(queued);
        self.systems.clear();
        self.activity.parent.take();
    }
}

/// Admission into a runtime, usable from its application or server callbacks.
/// Retaining this handle does not keep the runtime or its systems alive.
#[derive(Clone)]
pub struct RuntimeHandle {
    pending: Rc<RefCell<Pending>>,
    activity: Arc<Activity>,
}
impl RuntimeHandle {
    /// Transfer a system into the runtime. Dropping the returned completion
    /// promise cancels interest in completion, not the running system.
    pub fn add<V: 'static>(&self, system: RpcSystem<V>) -> capnp::Result<Promise<(), Error>> {
        self.add_driver(Box::new(system))
    }
    fn add_driver(&self, driver: Box<dyn Driver>) -> capnp::Result<Promise<(), Error>> {
        let (completion, receiver) = shared::channel();
        {
            let mut pending = self.pending.borrow_mut();
            if pending.closed {
                return Err(Error::disconnected("RPC runtime was dropped".into()));
            }
            if pending.active == pending.maximum {
                return Err(Error::overloaded("RPC runtime system limit reached".into()));
            }
            pending
                .queued
                .try_reserve(1)
                .map_err(|_| Error::overloaded("RPC runtime admission allocation failed".into()))?;
            let (progress, progress_waker) = lane(&self.activity, PROGRESS);
            let (output, output_waker) = lane(&self.activity, 0);
            pending.queued.push(Running {
                driver,
                completion,
                progress,
                progress_waker,
                output,
                output_waker,
            });
            pending.active += 1;
        }
        self.activity.notify(PROGRESS);
        Ok(Promise::from_future(async move {
            receiver
                .await
                .map_err(|_| Error::disconnected("RPC runtime was dropped".into()))?
        }))
    }
}

#[cfg(test)]
mod tests;

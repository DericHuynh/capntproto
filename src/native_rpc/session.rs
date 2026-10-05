//! One owner for a route generation's transitions and tasks.
#![forbid(unsafe_code)]
use super::{failed, RouteGeneration, VatId};
use crate::{
    native_arbitration::Admission, native_shutdown::Control, transport::AuthenticatedSession,
};
use capnp::capability::Promise;
use std::{cell::RefCell, io, rc::Rc, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteStatus {
    Connecting,
    Authenticated,
    Draining,
    Failed,
    Stopped,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    Connect,
    Identity,
    Arbitration,
    Transport,
    Bridge,
    Writer,
    Timeout,
    PeerClosed,
}
/// Structured terminal diagnostics, including the originating error message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteFailure {
    pub kind: FailureKind,
    pub message: String,
    pub io_kind: Option<io::ErrorKind>,
}
impl RouteFailure {
    pub(crate) fn new(kind: FailureKind, message: impl ToString) -> Self {
        Self {
            kind,
            message: message.to_string(),
            io_kind: None,
        }
    }
    fn io(kind: FailureKind, error: io::Error) -> Self {
        Self {
            kind,
            message: error.to_string(),
            io_kind: Some(error.kind()),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Termination {
    ShutdownAcknowledged,
    Canceled,
    Failed(RouteFailure),
}
/// An observer does not retain the session or any RPC capability. Keep it to
/// inspect the first terminal cause after disconnect or generation replacement.
#[derive(Clone)]
pub struct RouteObserver(Rc<Lifecycle>);
impl RouteObserver {
    #[must_use]
    pub fn generation(&self) -> RouteGeneration {
        self.0.generation
    }
    pub fn status(&self) -> RouteStatus {
        self.0.status()
    }
    pub fn termination(&self) -> Option<Termination> {
        match &*self.0.phase.borrow() {
            Phase::Terminal(cause) => Some(cause.clone()),
            _ => None,
        }
    }
}
enum Phase {
    Connecting,
    Authenticated(Control),
    Draining(Control),
    Terminal(Termination),
}
pub(super) struct Lifecycle {
    generation: RouteGeneration,
    phase: RefCell<Phase>,
    datagrams: RefCell<Option<crate::transport::DatagramPort>>,
    mobility: RefCell<Option<crate::transport::Mobility>>,
    scheduling: RefCell<Option<crate::transport::Scheduling>>,
    terminal_changed: tokio::sync::Notify,
}
impl Lifecycle {
    fn new(generation: RouteGeneration) -> Rc<Self> {
        Rc::new(Self {
            generation,
            phase: RefCell::new(Phase::Connecting),
            datagrams: RefCell::new(None),
            mobility: RefCell::new(None),
            scheduling: RefCell::new(None),
            terminal_changed: tokio::sync::Notify::new(),
        })
    }
    pub(super) fn status(&self) -> RouteStatus {
        match &*self.phase.borrow() {
            Phase::Connecting => RouteStatus::Connecting,
            Phase::Authenticated(_) => RouteStatus::Authenticated,
            Phase::Draining(_) => RouteStatus::Draining,
            Phase::Terminal(Termination::Failed(_)) => RouteStatus::Failed,
            Phase::Terminal(_) => RouteStatus::Stopped,
        }
    }
    fn activate(&self, control: Control) -> capnp::Result<()> {
        let mut phase = self.phase.borrow_mut();
        if !matches!(*phase, Phase::Connecting) {
            return Err(super::gone());
        }
        *phase = Phase::Authenticated(control);
        Ok(())
    }
    pub(super) fn begin_shutdown(&self, timeout: Duration) -> capnp::Result<Control> {
        let mut phase = self.phase.borrow_mut();
        let Phase::Authenticated(control) = &*phase else {
            return Err(super::gone());
        };
        control.begin(timeout).map_err(|e| failed(&e.to_string()))?;
        let control = control.clone();
        *phase = Phase::Draining(control.clone());
        drop(phase);
        self.datagrams.borrow_mut().take();
        Ok(control)
    }
    /// Compose local queue/output fences with the generation's peer receipt.
    /// The caller retains the route cleanup guard; the deadline is supplied so
    /// tests can schedule expiration independently of an OS clock.
    pub(super) async fn complete_shutdown(
        &self,
        flush: Promise<(), capnp::Error>,
        output_closed: futures::future::Shared<Promise<(), capnp::Error>>,
        control: &Control,
        expired: impl std::future::Future<Output = ()>,
    ) -> capnp::Result<crate::native_shutdown::Receipt> {
        let outcome = tokio::select! {
            // An existing terminal cause wins. Otherwise a complete fence
            // chain wins simultaneous expiration; incomplete drains time out.
            biased;
            error = self.terminal_error() => return Err(error),
            result = async {
                flush.await.map_err(|e| RouteFailure::new(FailureKind::Writer, e))?;
                output_closed.await.map_err(|e| RouteFailure::new(FailureKind::Writer, e))?;
                control.wait().await.map_err(|e| RouteFailure::new(FailureKind::Transport, e))
            } => result,
            _ = expired => Err(RouteFailure::new(FailureKind::Timeout, "Native shutdown timed out")),
        };
        match outcome {
            Ok(receipt) => {
                self.finish(Termination::ShutdownAcknowledged);
                Ok(receipt)
            }
            Err(error) => {
                self.fail(error.clone());
                Err(capnp::Error::disconnected(error.message))
            }
        }
    }
    async fn terminal_error(&self) -> capnp::Error {
        loop {
            let changed = self.terminal_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let error = match &*self.phase.borrow() {
                Phase::Terminal(Termination::Failed(error)) => {
                    Some(capnp::Error::disconnected(error.message.clone()))
                }
                Phase::Terminal(Termination::Canceled) => {
                    Some(capnp::Error::disconnected("Native route canceled".into()))
                }
                Phase::Terminal(Termination::ShutdownAcknowledged) => Some(
                    capnp::Error::disconnected("Native shutdown already completed".into()),
                ),
                _ => None,
            };
            if let Some(error) = error {
                return error;
            }
            changed.await;
        }
    }
    pub(super) fn finish(&self, cause: Termination) {
        let old = {
            let mut phase = self.phase.borrow_mut();
            if matches!(*phase, Phase::Terminal(_)) {
                return;
            }
            std::mem::replace(&mut *phase, Phase::Terminal(cause.clone()))
        };
        if let Phase::Authenticated(control) | Phase::Draining(control) = old {
            if cause != Termination::ShutdownAcknowledged {
                control.finish(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    match &cause {
                        Termination::Failed(error) => error.message.as_str(),
                        _ => "Native route canceled",
                    },
                )));
            }
        }
        self.datagrams.borrow_mut().take();
        self.terminal_changed.notify_waiters();
    }
    pub(super) fn fail(&self, failure: RouteFailure) {
        self.finish(Termination::Failed(failure));
    }
}
/// The route policy is independent of the local executor. Implementations must
/// enqueue without polling the new future and abort without running other tasks.
/// This boundary is private: authenticated sessions still come from transport.
pub(super) trait LocalExecutor {
    type Abort;
    fn spawn(&self, work: impl std::future::Future<Output = ()> + 'static) -> Self::Abort;
    fn abort(task: &Self::Abort);
}
pub(super) struct TokioExecutor;
impl LocalExecutor for TokioExecutor {
    type Abort = tokio::task::AbortHandle;
    fn spawn(&self, work: impl std::future::Future<Output = ()> + 'static) -> Self::Abort {
        tokio::task::spawn_local(work).abort_handle()
    }
    fn abort(task: &Self::Abort) {
        task.abort();
    }
}
pub(super) type SessionTask = OwnedSession<TokioExecutor>;

/// Holds all abort handles for one route, including its RPC writer. Workers
/// retain only Lifecycle, so neither observers nor tasks keep this owner alive.
pub(super) struct OwnedSession<E: LocalExecutor> {
    pub(super) lifecycle: Rc<Lifecycle>,
    pub(super) admission: Option<Admission>,
    tasks: RefCell<Vec<E::Abort>>,
    executor: E,
}
impl<E: LocalExecutor> OwnedSession<E> {
    fn with_executor(
        generation: RouteGeneration,
        admission: Option<Admission>,
        executor: E,
    ) -> Self {
        Self {
            lifecycle: Lifecycle::new(generation),
            admission,
            tasks: RefCell::new(Vec::new()),
            executor,
        }
    }
    pub(super) fn observe(&self) -> RouteObserver {
        RouteObserver(self.lifecycle.clone())
    }
    pub(super) fn status(&self) -> RouteStatus {
        self.lifecycle.status()
    }
    pub(super) fn take_datagrams(&self) -> Option<crate::transport::DatagramPort> {
        if self.status() != RouteStatus::Authenticated {
            return None;
        }
        self.lifecycle.datagrams.borrow_mut().take()
    }
    pub(super) fn mobility(&self) -> Option<crate::transport::Mobility> {
        if self.status() != RouteStatus::Authenticated {
            return None;
        }
        self.lifecycle.mobility.borrow().clone()
    }
    pub(super) fn scheduling(&self) -> Option<crate::transport::Scheduling> {
        if self.status() != RouteStatus::Authenticated {
            return None;
        }
        self.lifecycle.scheduling.borrow().clone()
    }
    pub(super) fn spawn(&self, work: impl std::future::Future<Output = ()> + 'static) {
        let task = self.executor.spawn(work);
        self.tasks.borrow_mut().push(task);
    }
    pub(super) fn stop(&self) {
        self.lifecycle.finish(Termination::Canceled);
        if let Some(admission) = &self.admission {
            admission.close();
        }
        for task in self.tasks.borrow().iter() {
            E::abort(task);
        }
    }
    fn pending_with_executor(
        generation: RouteGeneration,
        local: VatId,
        peer: VatId,
        selection: impl std::future::Future<Output = Result<AuthenticatedSession, RouteFailure>>
            + 'static,
        admission: Option<Admission>,
        executor: E,
    ) -> (crate::rpc::local_io::Stream, Self) {
        let (io, bridge) = crate::rpc::local_io::pair(crate::rpc::QUIC_BUFFER_BYTES);
        let owner = Self::with_executor(generation, admission, executor);
        let lifecycle = owner.lifecycle.clone();
        owner.spawn(async move {
            match selection
                .await
                .and_then(|session| Installed::new(local, peer, session, lifecycle.clone()))
            {
                Ok((io, installed)) => installed.run(Some((bridge, io))).await,
                Err(error) => lifecycle.fail(error),
            }
        });
        (io, owner)
    }
    pub(super) fn watch_writer(
        &self,
        mut writer: Promise<(), capnp::Error>,
        output_closed: futures::future::Shared<Promise<(), capnp::Error>>,
    ) {
        let lifecycle = self.lifecycle.clone();
        self.spawn(async move {
            let report = |result: capnp::Result<()>| {
                // Requested drains own their receipt/deadline result.
                if lifecycle.status() != RouteStatus::Draining {
                    if let Err(error) = result {
                        lifecycle.fail(RouteFailure::new(FailureKind::Writer, error));
                    }
                }
            };
            tokio::select! {
                // Drive the producer before observing its output fence. This
                // also makes simultaneous readiness independent of select!'s
                // random poll order; both futures report the same write result.
                biased;
                result = &mut writer => report(result),
                result = output_closed => {
                    report(result); // Report failure before input EOF/disconnect.
                    report(writer.await);
                }
            }
        });
    }
}
impl SessionTask {
    fn new(generation: RouteGeneration, admission: Option<Admission>) -> Self {
        Self::with_executor(generation, admission, TokioExecutor)
    }
    pub(super) fn ready(
        generation: RouteGeneration,
        local: VatId,
        peer: VatId,
        session: AuthenticatedSession,
    ) -> capnp::Result<(crate::rpc::local_io::Stream, Self)> {
        let owner = Self::new(generation, None);
        let (io, installed) = Installed::new(local, peer, session, owner.lifecycle.clone())
            .map_err(|e| failed(&e.message))?;
        owner.spawn(installed.run(None));
        Ok((io, owner))
    }
    pub(super) fn pending(
        generation: RouteGeneration,
        local: VatId,
        peer: VatId,
        selection: impl std::future::Future<Output = Result<AuthenticatedSession, RouteFailure>>
            + 'static,
        admission: Option<Admission>,
    ) -> (crate::rpc::local_io::Stream, Self) {
        Self::pending_with_executor(generation, local, peer, selection, admission, TokioExecutor)
    }
}
impl<E: LocalExecutor> Drop for OwnedSession<E> {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
use super::generation::fixture as generation;
#[cfg(test)]
mod output_tests;
#[cfg(test)]
mod schedule_tests;
#[cfg(test)]
mod shutdown_tests;
#[cfg(test)]
mod test_executor;
struct Installed {
    session: AuthenticatedSession,
    lifecycle: Rc<Lifecycle>,
}
impl Installed {
    fn new(
        local: VatId,
        peer: VatId,
        mut session: AuthenticatedSession,
        lifecycle: Rc<Lifecycle>,
    ) -> Result<(crate::rpc::local_io::Stream, Self), RouteFailure> {
        if session.local != local || session.peer != peer || local == peer {
            return Err(RouteFailure::new(
                FailureKind::Identity,
                "session identity does not match route",
            ));
        }
        let io = session
            .io
            .take()
            .ok_or_else(|| RouteFailure::new(FailureKind::Transport, "session already attached"))?;
        lifecycle
            .activate(session.shutdown.clone())
            .map_err(|e| RouteFailure::new(FailureKind::Transport, e))?;
        *lifecycle.datagrams.borrow_mut() = session.take_datagrams();
        *lifecycle.mobility.borrow_mut() = Some(session.mobility());
        *lifecycle.scheduling.borrow_mut() = Some(session.scheduling());
        Ok((io, Self { session, lifecycle }))
    }
    async fn run(
        mut self,
        bridge: Option<(crate::rpc::local_io::Stream, crate::rpc::local_io::Stream)>,
    ) {
        let copy = async move {
            if let Some((mut bridge, mut io)) = bridge {
                tokio::io::copy_bidirectional(&mut bridge, &mut io)
                    .await
                    .map(|_| ())
                    .map_err(|e| RouteFailure::io(FailureKind::Bridge, e))
            } else {
                futures::future::pending().await
            }
        };
        let result = tokio::select! {
            result = &mut self.session.driver => match result {
                Ok(result) => result.map_err(|e| RouteFailure::io(FailureKind::Transport, e)),
                Err(error) => Err(RouteFailure::new(FailureKind::Transport, error)),
            },
            result = copy => result,
        };
        // Shutdown owns its terminal result. DriverGuard wakes it even if this
        // task exits; a valid receipt already recorded by the driver survives.
        if self.lifecycle.status() != RouteStatus::Draining {
            self.lifecycle.fail(result.err().unwrap_or_else(|| {
                RouteFailure::new(FailureKind::PeerClosed, "peer closed transport")
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_observer_survives_owner_and_preserves_first_cause() {
        let owner = SessionTask::new(generation(1), None);
        let observer = owner.observe();
        let control = Control::new();
        owner.lifecycle.activate(control).unwrap();
        owner.lifecycle.fail(RouteFailure::new(
            FailureKind::Identity,
            "identity rejected",
        ));
        drop(owner);
        assert_eq!(observer.generation().get(), 1);
        assert_eq!(observer.status(), RouteStatus::Failed);
        assert!(matches!(
            observer.termination(),
            Some(Termination::Failed(RouteFailure {
                kind: FailureKind::Identity,
                ..
            }))
        ));
        assert!(observer.0.activate(Control::new()).is_err());
        assert!(observer.0.begin_shutdown(Duration::from_secs(1)).is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn observer_does_not_retain_owned_tasks() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let owner = SessionTask::new(generation(5), None);
                let observer = owner.observe();
                let (send, receive) = tokio::sync::oneshot::channel::<()>();
                owner.spawn(async {
                    let _send = send;
                    futures::future::pending::<()>().await;
                });
                drop(owner);
                assert!(receive.await.is_err());
                assert_eq!(observer.termination(), Some(Termination::Canceled));
            })
            .await;
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Trace {
        steps: Vec<Step>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Step {
        action: String,
        state: Snapshot,
    }
    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Snapshot {
        phase: u64,
        cause: u64,
        first: u64,
        drained: u64,
        closed: u64,
        receipt: u64,
        late: u64,
    }
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_route_lifecycle_traces() {
        let path =
            capntproto_test_support::verification::input("CAPNTPROTO_ROUTE_LIFECYCLE_TRACES")
                .expect("run verification driver");
        let traces: Vec<Trace> = capntproto_test_support::traces::read(path, "RpcRouteLifecycle");
        for trace in traces {
            let lifecycle = Lifecycle::new(generation(1));
            let observer = RouteObserver(lifecycle.clone());
            let control = Control::new();
            let (mut drained, mut closed, mut receipt, mut late, mut first) = (0, 0, 0, 0, 0);
            for step in trace.steps {
                match step.action.as_str() {
                    "install" => lifecycle.activate(control.clone()).unwrap(),
                    "begin" => {
                        lifecycle.begin_shutdown(Duration::from_secs(60)).unwrap();
                    }
                    "drain" => drained = 1,
                    "close" => {
                        assert_eq!(drained, 1);
                        closed = 1;
                    }
                    "receive" => {
                        assert_eq!(closed, 1);
                        receipt = 1;
                        control.finish(Ok(crate::native_shutdown::Receipt { bytes: 0 }));
                    }
                    "finish" => {
                        control.wait().await.unwrap();
                        lifecycle.finish(Termination::ShutdownAcknowledged);
                    }
                    "fail" => lifecycle.fail(RouteFailure::new(
                        FailureKind::Transport,
                        "test transport failure",
                    )),
                    "cancel" => lifecycle.finish(Termination::Canceled),
                    "lateInstall" => {
                        late = 1;
                        assert!(lifecycle.activate(Control::new()).is_err());
                    }
                    "lateCause" => {
                        late = 1;
                        lifecycle.finish(Termination::Canceled);
                        lifecycle.fail(RouteFailure::new(FailureKind::Writer, "late failure"));
                    }
                    other => panic!("unknown lifecycle action {other}"),
                }
                let phase = match observer.status() {
                    RouteStatus::Connecting => 0,
                    RouteStatus::Authenticated => 1,
                    RouteStatus::Draining => 2,
                    RouteStatus::Failed => 3,
                    RouteStatus::Stopped => 4,
                };
                let cause = match observer.termination() {
                    None => 0,
                    Some(Termination::ShutdownAcknowledged) => 1,
                    Some(Termination::Canceled) => 2,
                    Some(Termination::Failed(_)) => 3,
                };
                if first == 0 && cause != 0 {
                    first = cause;
                }
                assert_eq!(
                    Snapshot {
                        phase,
                        cause,
                        first,
                        drained,
                        closed,
                        receipt,
                        late
                    },
                    step.state,
                    "{}",
                    step.action
                );
            }
        }
    }
}

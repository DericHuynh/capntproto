use super::*;
use std::{cell::Cell, collections::VecDeque, task::Waker};

fn options() -> Options {
    Options::bounded(
        NonZeroUsize::new(2).unwrap(),
        NonZeroUsize::new(32).unwrap(),
        NonZeroUsize::new(4096).unwrap(),
        NonZeroUsize::new(1 << 20).unwrap(),
    )
    .unwrap()
}
// Deliberately neither Clone nor Copy: transport identity need not be either.
struct Peer(usize);
#[derive(Default)]
struct Probe {
    accepts: RefCell<VecDeque<usize>>,
    accept_error: RefCell<Option<Error>>,
    live: Cell<usize>,
    peer_reads: Cell<usize>,
    two_party: Cell<bool>,
    mode_reads: Cell<usize>,
    feature_reads: Cell<usize>,
    writes: RefCell<Vec<usize>>,
    idle: RefCell<Vec<bool>>,
    shutdowns: Cell<usize>,
    closed: Cell<usize>,
    close_ready: Cell<bool>,
    close_error: RefCell<Option<Error>>,
    end_network: Cell<bool>,
    network_error: RefCell<Option<Error>>,
    network_drops: Cell<usize>,
    drives: Cell<usize>,
    wakes: RefCell<Vec<Waker>>,
    on_peer: RefCell<Option<Box<dyn Fn()>>>,
}
impl Probe {
    fn shutdown(self: &Rc<Self>) -> Promise<(), Error> {
        self.shutdowns.set(self.shutdowns.get() + 1);
        let probe = self.clone();
        Promise::from_future(std::future::poll_fn(move |cx| {
            if probe.close_ready.get() {
                probe.closed.set(probe.closed.get() + 1);
                Poll::Ready(probe.close_error.borrow().clone().map_or(Ok(()), Err))
            } else {
                probe.register(cx);
                Poll::Pending
            }
        }))
    }
    fn wake(&self) {
        let wakes = std::mem::take(&mut *self.wakes.borrow_mut());
        for wake in wakes {
            wake.wake();
        }
    }
    fn register(&self, cx: &Context<'_>) {
        self.wakes.borrow_mut().push(cx.waker().clone());
    }
    fn connection(self: &Rc<Self>, id: usize) -> Box<dyn crate::Connection<Peer>> {
        self.live.set(self.live.get() + 1);
        Box::new(Transport {
            probe: self.clone(),
            id,
        })
    }
}
struct Transport {
    probe: Rc<Probe>,
    id: usize,
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.probe.live.set(self.probe.live.get() - 1);
    }
}
struct FrameInput;
impl crate::IncomingFrameStream for FrameInput {
    fn poll_frame(&mut self, _: &mut Context<'_>) -> Poll<capnp::Result<Option<&[u8]>>> {
        Poll::Pending
    }
    fn frame(&self) -> Option<&[u8]> {
        None
    }
    fn finish_frame(&mut self) {}
}
struct FrameOutput {
    probe: Rc<Probe>,
    id: usize,
}
impl crate::OutgoingMessageStream for FrameOutput {
    fn reserve(&mut self) -> capnp::Result<()> {
        Ok(())
    }
    fn cancel_reservation(&mut self) {}

    fn shutdown(&mut self, _: capnp::Result<()>) -> Promise<(), Error> {
        self.probe.shutdown()
    }
    fn publish_reserved(&mut self, frame: crate::OutgoingFrame) -> capnp::Result<()> {
        assert!(frame.size_in_words() > 0);
        self.probe.writes.borrow_mut().push(self.id);
        Ok(())
    }
    fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl crate::Connection<Peer> for Transport {
    fn control_mode(&self) -> crate::ControlMode {
        self.probe.mode_reads.set(self.probe.mode_reads.get() + 1);
        if self.probe.two_party.get() {
            crate::ControlMode::TwoParty
        } else {
            crate::ControlMode::Network
        }
    }
    fn supports_third_party_answers(&self) -> bool {
        self.probe
            .feature_reads
            .set(self.probe.feature_reads.get() + 1);
        false
    }
    fn take_frame_io(&mut self) -> capnp::Result<crate::FrameIo> {
        Ok(crate::FrameIo {
            incoming: Box::new(FrameInput),
            outgoing: Box::new(FrameOutput {
                probe: self.probe.clone(),
                id: self.id,
            }),
            write_finished: None,
        })
    }
    fn set_idle(&mut self, idle: bool) {
        self.probe.idle.borrow_mut().push(idle);
    }
    fn connection_id(&self) -> usize {
        self.id
    }
    fn get_peer_vat_id(&self) -> Peer {
        self.probe.peer_reads.set(self.probe.peer_reads.get() + 1);
        let callback = self.probe.on_peer.borrow_mut().take();
        if let Some(callback) = callback {
            callback();
        }
        Peer(self.id)
    }
    fn new_outgoing_message(&mut self, _: u32) -> Box<dyn crate::OutgoingMessage> {
        panic!("portable systems must never construct legacy messages")
    }
    fn send_frame(&mut self, frame: crate::OutgoingFrame) -> Promise<(), Error> {
        assert!(frame.size_in_words() > 0);
        self.probe.writes.borrow_mut().push(self.id);
        Promise::ok(())
    }
    fn receive_incoming_message(
        &mut self,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage>>, Error> {
        Promise::from_future(std::future::pending())
    }
    fn shutdown(&mut self, _: capnp::Result<()>) -> Promise<(), Error> {
        self.probe.shutdown()
    }
}
struct TransportNetwork(Rc<Probe>);
impl Drop for TransportNetwork {
    fn drop(&mut self) {
        self.0.network_drops.set(self.0.network_drops.get() + 1);
    }
}
impl VatNetwork<Peer> for TransportNetwork {
    fn connect(&mut self, id: Peer) -> Option<Box<dyn crate::Connection<Peer>>> {
        (id.0 != 0).then(|| self.0.connection(id.0))
    }
    fn accept(&mut self) -> Promise<Box<dyn crate::Connection<Peer>>, Error> {
        let probe = self.0.clone();
        Promise::from_future(std::future::poll_fn(move |cx| {
            if let Some(error) = probe.accept_error.borrow_mut().take() {
                return Poll::Ready(Err(error));
            }
            if let Some(id) = probe.accepts.borrow_mut().pop_front() {
                return Poll::Ready(Ok(probe.connection(id)));
            }
            probe.register(cx);
            Poll::Pending
        }))
    }
    fn take_output_driver(&mut self) -> Option<capnp::capability::Promise<(), capnp::Error>> {
        None
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        self.0.drives.set(self.0.drives.get() + 1);
        let probe = self.0.clone();
        Promise::from_future(std::future::poll_fn(move |cx| {
            if probe.end_network.get() {
                Poll::Ready(probe.network_error.borrow().clone().map_or(Ok(()), Err))
            } else {
                probe.register(cx);
                Poll::Pending
            }
        }))
    }
}
fn fixture(options: Options) -> (RpcSystem<Peer>, Rc<Probe>) {
    let probe = Rc::new(Probe::default());
    let system = RpcSystem::new(Box::new(TransportNetwork(probe.clone())), None, options).unwrap();
    (system, probe)
}
fn poll<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}
fn poll_system(system: &mut RpcSystem<Peer>) -> Poll<capnp::Result<()>> {
    let mut cx = Context::from_waker(Waker::noop());
    let progress = system.poll_progress(&mut cx);
    system.poll_output(&mut cx);
    progress
}
fn pump(system: &mut RpcSystem<Peer>) {
    for _ in 0..32 {
        assert!(poll_system(system).is_pending());
    }
}
fn finish(system: &mut RpcSystem<Peer>) -> capnp::Result<()> {
    for _ in 0..32 {
        if let Poll::Ready(result) = poll_system(system) {
            return result;
        }
    }
    panic!("network completion did not retire connection tasks");
}
fn broken(client: capability::Client) -> Error {
    match poll(&mut client.when_resolved()) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected a broken client, got {other:?}"),
    }
}
#[test]
fn invalid_configuration_is_rejected_before_network_use() {
    let mut invalid = options();
    invalid.table_entries = NonZeroUsize::new(1).unwrap();
    assert!(invalid.validate().is_err());
    invalid = options();
    invalid.retained_bytes = invalid.message_bytes;
    assert!(invalid.validate().is_err());
    invalid = options();
    invalid.message_bytes = NonZeroUsize::new(usize::MAX).unwrap();
    assert!(invalid.validate().is_err());
    let probe = Rc::new(Probe::default());
    assert!(RpcSystem::new(Box::new(TransportNetwork(probe.clone())), None, invalid).is_err());
    assert_eq!(probe.drives.get(), 0);
    assert_eq!(probe.network_drops.get(), 1);
}
#[test]
fn idle_transport_becomes_active_synchronously_on_reuse() {
    let (mut system, probe) = fixture(options());
    probe.accepts.borrow_mut().push_back(1);
    pump(&mut system);
    assert_eq!(&*probe.idle.borrow(), &[false, true]);
    pump(&mut system);
    assert_eq!(&*probe.idle.borrow(), &[false, true]);
    let client: capability::Client = system.bootstrap(Peer(1));
    assert_eq!(&*probe.idle.borrow(), &[false, true, false]);
    pump(&mut system);
    drop(client);
    pump(&mut system);
    // A canceled question still waits for the peer's Return.
    assert_eq!(&*probe.idle.borrow(), &[false, true, false]);
}
#[test]
fn unpolled_system_drop_breaks_pending_clients_and_all_shutdown_observers() {
    let (mut system, probe) = fixture(options());
    let remote = system.get_bootstrapper();
    let client: capability::Client = system.bootstrap(Peer(1));
    let mut left = system.get_disconnector();
    let mut right = system.get_disconnector();
    assert!(poll(&mut left).is_pending());
    drop(system);
    assert_eq!(probe.live.get(), 0);
    assert_eq!(probe.network_drops.get(), 1);
    for observer in [&mut left, &mut right] {
        assert!(
            matches!(poll(observer), Poll::Ready(Err(e)) if e.kind == capnp::ErrorKind::Disconnected)
        );
    }
    assert_eq!(broken(client).kind, capnp::ErrorKind::Disconnected);
    assert_eq!(
        broken(remote.bootstrap(Peer(0))).kind,
        capnp::ErrorKind::Disconnected
    );
}
#[test]
fn duplicate_dial_and_accept_share_one_task_and_shutdown_waits_for_close() {
    let (mut system, probe) = fixture(options());
    probe.accepts.borrow_mut().push_back(1);
    let _a: capability::Client = system.bootstrap(Peer(1));
    let _b: capability::Client = system.bootstrap(Peer(1));
    pump(&mut system);
    assert_eq!(probe.peer_reads.get(), 1);
    assert_eq!(probe.live.get(), 1);
    assert_eq!(&*probe.writes.borrow(), &[1, 1]);
    let remote = system.get_bootstrapper();
    let mut left = system.get_disconnector();
    let mut right = system.get_disconnector();
    assert!(poll(&mut left).is_pending());
    assert_eq!(
        broken(remote.bootstrap(Peer(1))).kind,
        capnp::ErrorKind::Disconnected
    );
    pump(&mut system);
    assert_eq!(probe.shutdowns.get(), 1);
    assert!(poll(&mut right).is_pending());
    probe.close_ready.set(true);
    probe.wake();
    pump(&mut system);
    assert!(matches!(poll(&mut left), Poll::Ready(Ok(()))));
    assert!(matches!(poll(&mut right), Poll::Ready(Ok(()))));
    assert_eq!(probe.closed.get(), 1);
    assert_eq!(probe.live.get(), 0);
}
#[test]
fn automatic_accept_and_new_dials_obey_connection_limit_without_rejecting_live_reuse() {
    let mut policy = options();
    policy.connections = NonZeroUsize::new(1).unwrap();
    let (mut system, probe) = fixture(policy);
    probe.accepts.borrow_mut().extend([1, 2]);
    pump(&mut system);
    assert_eq!(probe.peer_reads.get(), 1);
    assert_eq!(probe.live.get(), 1);
    let _existing: capability::Client = system.bootstrap(Peer(1));
    let rejected: capability::Client = system.bootstrap(Peer(2));
    assert_eq!(broken(rejected).kind, capnp::ErrorKind::Overloaded);
    pump(&mut system);
    assert_eq!(&*probe.writes.borrow(), &[1]);
    let close = system.get_disconnector();
    close.abort(Error::disconnected("test done".into()));
    pump(&mut system);
    assert_eq!(probe.live.get(), 0);
}
#[test]
fn deadline_can_abort_an_already_blocked_graceful_close_and_preserves_its_reason() {
    let (mut system, probe) = fixture(options());
    let _client: capability::Client = system.bootstrap(Peer(1));
    pump(&mut system);
    let mut closed = system.get_disconnector();
    assert!(poll(&mut closed).is_pending());
    pump(&mut system);
    assert_eq!(probe.shutdowns.get(), 1);
    assert_eq!(probe.closed.get(), 0);
    closed.abort(Error::failed("caller deadline".into()));
    pump(&mut system);
    assert!(matches!(poll(&mut closed), Poll::Ready(Err(e)) if e.extra == "caller deadline"));
    assert_eq!(probe.live.get(), 0);
    assert_eq!(probe.closed.get(), 0);
}
#[test]
fn physical_close_failure_is_retained_after_connection_task_retires() {
    let (mut system, probe) = fixture(options());
    let _client: capability::Client = system.bootstrap(Peer(1));
    pump(&mut system);
    let mut closed = system.get_disconnector();
    assert!(poll(&mut closed).is_pending());
    *probe.close_error.borrow_mut() = Some(Error::failed("close fault".into()));
    probe.close_ready.set(true);
    probe.wake();
    pump(&mut system);
    assert!(matches!(poll(&mut closed), Poll::Ready(Err(e)) if e.extra == "close fault"));
    assert_eq!(probe.live.get(), 0);
    assert!(
        matches!(poll(&mut system.get_disconnector()), Poll::Ready(Err(e)) if e.extra == "close fault")
    );
}
#[test]
fn network_completion_cannot_report_successful_close_for_abandoned_connection_task() {
    let (mut system, probe) = fixture(options());
    let client: capability::Client = system.bootstrap(Peer(1));
    let mut closed = system.get_disconnector();
    probe.end_network.set(true);
    finish(&mut system).unwrap();
    assert_eq!(broken(client).kind, capnp::ErrorKind::Disconnected);
    assert!(
        matches!(poll(&mut closed), Poll::Ready(Err(e)) if e.extra == "network ended before protocol shutdown")
    );
    assert_eq!(probe.live.get(), 0);
    assert_eq!(probe.closed.get(), 0);
}

#[test]
fn network_completion_waits_for_pending_close_receipt_and_storage_teardown() {
    let (mut system, probe) = fixture(options());
    let _client: capability::Client = system.bootstrap(Peer(1));
    pump(&mut system);
    let mut closed = system.get_disconnector();
    assert!(poll(&mut closed).is_pending());
    pump(&mut system);
    assert_eq!(probe.shutdowns.get(), 1);
    probe.end_network.set(true);
    probe.wake();
    pump(&mut system);
    assert!(poll(&mut closed).is_pending());
    assert_eq!(probe.live.get(), 1);
    probe.close_ready.set(true);
    probe.wake();
    finish(&mut system).unwrap();
    assert_eq!(probe.closed.get(), 1);
    assert_eq!(probe.live.get(), 0);
    assert!(matches!(poll(&mut closed), Poll::Ready(Ok(()))));
}

#[test]
fn network_failure_is_not_masked_by_an_already_completed_empty_disconnector() {
    let (mut system, probe) = fixture(options());
    assert!(matches!(
        poll(&mut system.get_disconnector()),
        Poll::Ready(Ok(()))
    ));
    *probe.network_error.borrow_mut() = Some(Error::failed("listener failed".into()));
    probe.end_network.set(true);
    assert!(
        matches!(poll_system(&mut system), Poll::Ready(Err(error)) if error.extra == "listener failed")
    );
}

#[test]
fn accept_failure_retires_unpolled_and_live_connections_before_failing_the_system() {
    for started in [false, true] {
        let (mut system, probe) = fixture(options());
        let remote = system.get_bootstrapper();
        let client: capability::Client = system.bootstrap(Peer(1));
        let mut closed = system.get_disconnector();
        if started {
            pump(&mut system);
        }
        *probe.accept_error.borrow_mut() = Some(Error::failed("accept fault".into()));
        probe.wake();
        assert_eq!(finish(&mut system).unwrap_err().extra, "accept fault");
        let client_error = broken(client);
        if started {
            assert_eq!(client_error.kind, capnp::ErrorKind::Disconnected);
            assert_eq!(client_error.extra, "accept fault");
        } else {
            assert_eq!(client_error.extra, "accept fault");
        }
        assert!(matches!(poll(&mut closed), Poll::Ready(Err(e)) if e.extra == "accept fault"));
        assert_eq!(probe.live.get(), 0);
        assert_eq!(probe.closed.get(), 0);
        assert_eq!(
            broken(remote.bootstrap(Peer(2))).kind,
            capnp::ErrorKind::Disconnected
        );
    }
}

#[test]
fn accept_error_during_orderly_shutdown_does_not_abort_output_drain() {
    let (mut system, probe) = fixture(options());
    let _client: capability::Client = system.bootstrap(Peer(1));
    pump(&mut system);
    let mut closed = system.get_disconnector();
    assert!(poll(&mut closed).is_pending());
    pump(&mut system);
    *probe.accept_error.borrow_mut() = Some(Error::disconnected("listener stopped".into()));
    probe.wake();
    pump(&mut system);
    assert!(poll(&mut closed).is_pending());
    assert_eq!(probe.live.get(), 1);
    probe.close_ready.set(true);
    probe.wake();
    pump(&mut system);
    assert_eq!(probe.closed.get(), 1);
    assert!(matches!(poll(&mut closed), Poll::Ready(Ok(()))));
}
#[test]
fn local_bootstrap_factory_runs_per_request_without_allocating_connection_tasks() {
    struct Factory(Rc<Cell<usize>>);
    impl BootstrapFactory<Peer> for Factory {
        fn create_for(&self, peer: &Peer) -> capnp::Result<capability::Client> {
            assert_eq!(peer.0, 0);
            self.0.set(self.0.get() + 1);
            Err(Error::failed(format!("local policy {}", self.0.get())))
        }
    }
    let calls = Rc::new(Cell::new(0));
    let probe = Rc::new(Probe::default());
    let mut system = RpcSystem::new_with_bootstrap_factory(
        Box::new(TransportNetwork(probe.clone())),
        Peer(0),
        Rc::new(Factory(calls.clone())),
        options(),
    )
    .unwrap();
    for count in 1..=2 {
        assert_eq!(
            broken(system.bootstrap(Peer(0))).extra,
            format!("local policy {count}")
        );
    }
    assert_eq!(calls.get(), 2);
    assert_eq!(probe.live.get(), 0);
    assert!(matches!(
        poll(&mut system.get_disconnector()),
        Poll::Ready(Ok(()))
    ));
}

#[test]
fn bootstrap_admission_is_bounded_before_first_poll_and_abandoned_commands_do_not_send() {
    let mut policy = options();
    policy.concurrency = NonZeroUsize::new(1).unwrap();
    let (mut system, probe) = fixture(policy);
    let first: capability::Client = system.bootstrap(Peer(1));
    let rejected: capability::Client = system.bootstrap(Peer(1));
    assert_eq!(broken(rejected).kind, capnp::ErrorKind::Overloaded);
    drop(first);
    pump(&mut system);
    assert!(probe.writes.borrow().is_empty());
    let _next: capability::Client = system.bootstrap(Peer(1));
    pump(&mut system);
    assert_eq!(&*probe.writes.borrow(), &[1]);
}

#[test]
fn reentrant_abort_during_transport_construction_retires_reservation_and_preserves_reason() {
    let (mut system, probe) = fixture(options());
    let close = system.get_disconnector();
    let mut observed = system.get_disconnector();
    *probe.on_peer.borrow_mut() = Some(Box::new(move || {
        close.abort(Error::failed("construction deadline".into()));
    }));
    let client: capability::Client = system.bootstrap(Peer(1));
    assert_eq!(broken(client).kind, capnp::ErrorKind::Disconnected);
    assert!(poll(&mut observed).is_pending());
    pump(&mut system);
    assert!(
        matches!(poll(&mut observed), Poll::Ready(Err(e)) if e.extra == "construction deadline")
    );
    assert_eq!(probe.live.get(), 0);
    assert!(probe.writes.borrow().is_empty());
}

#[test]
fn diagnostics_are_live_sorted_weak_and_policy_reaches_current_and_future_connections() {
    let (mut system, _) = fixture(options());
    let diagnostics = system.diagnostics();
    system.control().set_outgoing_call_limit(3);
    let first: capability::Client = system.bootstrap(Peer(7));
    let second: capability::Client = system.bootstrap(Peer(2));
    let snapshots = diagnostics.snapshot();
    assert_eq!(
        snapshots
            .iter()
            .map(|s| s.connection_id)
            .collect::<Vec<_>>(),
        [2, 7]
    );
    for snapshot in snapshots {
        assert_eq!(snapshot.outgoing_call_limit, 3);
        assert_eq!(snapshot.outgoing_calls, 0); // Bootstrap isn't a Call.
        assert_eq!(snapshot.questions, 0); // Task has never been polled.
    }
    system.control().set_outgoing_call_limit(0);
    pump(&mut system);
    for snapshot in diagnostics.snapshot() {
        assert_eq!(snapshot.outgoing_call_limit, 0);
        assert_eq!(snapshot.questions, 1);
        assert_eq!(snapshot.outgoing_calls, 0);
    }
    drop(first);
    drop(second);
    pump(&mut system);
    assert!(diagnostics.snapshot().iter().all(|s| s.questions == 1)); // Late Return still owed.
    drop(system);
    assert!(diagnostics.snapshot().is_empty());
}

#[test]
fn two_party_selection_preserves_idle_and_shutdown_without_network_feature_probes() {
    for two_party in [true, false] {
        let (mut system, probe) = fixture(options());
        probe.two_party.set(two_party);
        probe.accepts.borrow_mut().push_back(1);
        pump(&mut system);
        assert_eq!(probe.mode_reads.get(), 1);
        assert_eq!(probe.live.get(), 1);
        assert_eq!(&*probe.idle.borrow(), &[false, true]);
        if two_party {
            assert_eq!(probe.feature_reads.get(), 0);
        } else {
            assert!(probe.feature_reads.get() > 0);
        }
        let client: capability::Client = system.bootstrap(Peer(1));
        assert_eq!(&*probe.idle.borrow(), &[false, true, false]);
        pump(&mut system);
        assert_eq!(probe.mode_reads.get(), 1);
        if two_party {
            assert_eq!(probe.feature_reads.get(), 0);
        }
        drop(client);
        let mut close = system.get_disconnector();
        assert!(poll(&mut close).is_pending());
        pump(&mut system);
        assert_eq!(
            probe.live.get(),
            1,
            "transport retained through pending close"
        );
        probe.close_ready.set(true);
        probe.wake();
        pump(&mut system);
        assert!(matches!(poll(&mut close), Poll::Ready(Ok(()))));
        assert_eq!(probe.closed.get(), 1);
        assert_eq!(probe.live.get(), 0);
    }
}

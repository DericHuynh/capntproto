use super::*;
mod lifecycle;
mod network;
mod plain;
mod policy;
mod worker;
use capntproto_rpc_engine::{connection as storage, ids::WireId, imports};
use capntproto_rpc_wire::clients::Route;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

type TestPeer<'p, 'id, 's> = Peer<'p, 'id, 's, (), u8, 8, 8>;
type TestCoordinator<'p, 'id, 's> = Coordinator<'p, 'id, 's, (), u8, 8, 8>;
fn with_peer(
    f: impl for<'p, 'id, 's, 'm, 'b> FnOnce(TestPeer<'p, 'id, 's>, &mut messages::Messages<'p, 'm, 'b>),
) {
    let mut buffers = messages::GrowingBuffers::new(32, 1 << 20, 8 << 20);
    messages::with_growing_messages(
        &mut buffers,
        storage::Growth {
            initial: 0,
            maximum: 16,
        },
        |messages| {
            storage::with_domain(|domain| {
                let mut slots = connection::FixedStorage::<
                    (),
                    server::Results,
                    u8,
                    Rc<dyn ClientHook>,
                    8,
                    8,
                    32,
                    128,
                >::new();
                f(slots.bind(domain, 1).unwrap(), messages);
            })
        },
    )
    .unwrap();
}
fn options() -> Options {
    Options {
        input_bytes: NonZeroUsize::new(if cfg!(miri) { 2048 } else { 128 << 10 }).unwrap(),
        output_bytes: NonZeroUsize::new(512).unwrap(),
        budget: NonZeroUsize::new(7).unwrap(),
        reader: Default::default(),
    }
}
fn reclaimed(messages: &mut messages::Messages<'_, '_, '_>) {
    let writers: Vec<_> = (0..32)
        .map(|_| messages.reserve(options().input_bytes.get()).unwrap())
        .collect();
    for writer in writers {
        messages.discard(writer);
    }
}
#[derive(Default)]
struct Policy {
    stopped: bool,
    polls: usize,
    wake: Option<Waker>,
    wake_during_poll: bool,
    publish_on_poll: bool,
    preserve_plain: bool,
    direct_returns: bool,
    notify_receive: bool,
    idle_transitions: Vec<bool>,
}
impl<'p, 'id, 's> Control<'p, 'id, 's, (), u8, 8, 8> for Policy {
    fn accepts_direct_plain_return(&self) -> bool {
        self.direct_returns
    }
    fn plain_return_preserves_idle(&self) -> bool {
        self.direct_returns
    }
    fn plain_call_preserves_idle(&self) -> bool {
        self.preserve_plain
    }
    fn direct_publication_preserves_idle(&self) -> bool {
        self.preserve_plain
    }
    fn received(
        &mut self,
        event: connection::Received<'id, server::Results>,
        _: &mut TestCoordinator<'p, 'id, 's>,
        _: &mut messages::Messages<'p, '_, '_>,
        _: &mut messages::Queue<'p, '_>,
    ) -> capnp::Result<()> {
        if self.notify_receive {
            self.wake.as_ref().expect("registered policy").wake_by_ref();
        }
        match event {
            connection::Received::Bootstrap(_)
            | connection::Received::Network
            | connection::Received::QueuedNetwork => Err(Error::unimplemented(
                "test network has no bootstrap/handoff policy".into(),
            )),
            _ => Ok(()),
        }
    }
    fn poll(
        &mut self,
        _: &mut TestCoordinator<'p, 'id, 's>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
        _: &mut [u32],
        _: &NoResources,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        self.polls += 1;
        self.wake = Some(cx.waker().clone());
        if std::mem::take(&mut self.publish_on_poll) {
            let writer = messages.reserve_exact(8).unwrap();
            let frame = messages.seal(writer, 8).unwrap();
            let offer = messages.offer(&frame).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(frame);
            return Poll::Ready(Ok(true));
        }
        if std::mem::take(&mut self.wake_during_poll) {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
    fn set_idle(&mut self, idle: bool) {
        self.idle_transitions.push(idle);
    }
    fn shutdown(
        &mut self,
        _: &mut TestCoordinator<'p, 'id, 's>,
        _: &mut messages::Messages<'p, '_, '_>,
    ) {
        self.stopped = true;
    }
}

#[test]
fn idle_controls_resume_on_notifications_progress_and_output_capacity() {
    with_peer(|driver, messages| {
        let mut coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut writer = Writer(Rc::default());
        let mut control = Policy {
            wake_during_poll: true,
            ..Default::default()
        };
        let mut scratch = [0; 8];
        let first = Arc::new(WakeCount::default());
        let first_waker = Waker::from(first.clone());
        let mut cx = Context::from_waker(&first_waker);
        macro_rules! poll {
            () => {
                coordinator
                    .poll_io_work::<(), _, _>(
                        messages,
                        &mut queue,
                        &mut transport::Writer(&mut writer),
                        &mut scratch,
                        &NoResources,
                        &mut control,
                        &mut cx,
                    )
                    .unwrap();
            };
        }
        poll!();
        assert!(
            !coordinator.control_idle,
            "a wake before initial parent registration must survive"
        );
        for _ in 0..256 {
            poll!();
        }
        assert!(coordinator.control_idle);
        let polls = control.polls;
        for _ in 0..8 {
            poll!();
        }
        assert_eq!(control.polls, polls, "IO-only polls retain the certificate");
        // Even a completely idle poll must register the latest outer task.
        let latest = Arc::new(WakeCount::default());
        let latest_waker = Waker::from(latest.clone());
        cx = Context::from_waker(&latest_waker);
        poll!();
        first.0.store(0, Ordering::SeqCst);
        latest.0.store(0, Ordering::SeqCst);
        control.wake.as_ref().unwrap().wake_by_ref();
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(latest.0.load(Ordering::SeqCst), 1);
        poll!();
        assert!(control.polls > polls, "independent control notification");
        coordinator.changed();
        control.wake_during_poll = true;
        poll!();
        assert!(!coordinator.control_idle, "a callback wake cannot be lost");
        let polls = control.polls;
        poll!();
        assert!(control.polls > polls);
        // A pending control may have waited for FIFO capacity rather than an
        // application future. Draining that FIFO must also invalidate it.
        let writer_slot = messages.reserve_exact(8).unwrap();
        let frame = messages.seal(writer_slot, 8).unwrap();
        let offer = messages.offer(&frame).unwrap();
        queue.reserve().unwrap().accept(messages, offer);
        messages.release(frame);
        coordinator.control_idle = true;
        let polls = control.polls;
        for _ in 0..4 {
            coordinator
                .poll_io_work::<(), _, _>(
                    messages,
                    &mut queue,
                    &mut transport::Writer(&mut writer),
                    &mut scratch,
                    &NoResources,
                    &mut control,
                    &mut cx,
                )
                .unwrap();
        }
        assert!(queue.is_empty());
        assert!(control.polls > polls, "released FIFO capacity");
    });
}
#[derive(Default)]
struct Signal {
    stopped: Cell<bool>,
    wake: RefCell<Option<Waker>>,
}
impl Signal {
    fn stop(&self) {
        self.stopped.set(true);
        if let Some(wake) = self.wake.borrow_mut().take() {
            wake.wake();
        }
    }
    async fn wait(&self) -> capnp::Result<()> {
        std::future::poll_fn(|cx| {
            *self.wake.borrow_mut() = Some(cx.waker().clone());
            if self.stopped.get() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}
#[derive(Default)]
struct WakeCount(AtomicUsize);
impl std::task::Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[derive(Default)]
struct Pipe {
    bytes: VecDeque<u8>,
    buffered: VecDeque<u8>,
    read_wake: Option<Waker>,
    write_wake: Option<Waker>,
    read_calls: usize,
    write_calls: usize,
    flush_calls: usize,
    close_calls: usize,
    fail_read: bool,
    fail_write: bool,
    fail_flush: bool,
    fail_close: bool,
    block_flush: bool,
    block_close: bool,
    closed: bool,
    eof: bool,
    blocked: bool,
}
struct Reader(Rc<RefCell<Pipe>>);
struct Writer(Rc<RefCell<Pipe>>);
impl io::Read for Reader {
    type Error = &'static str;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize, io::IoError<Self::Error>>> {
        let mut pipe = self.0.borrow_mut();
        pipe.read_calls += 1;
        if pipe.fail_read {
            return Poll::Ready(Err(io::IoError::Failed("read failed")));
        }
        if pipe.read_calls.is_multiple_of(11) {
            return Poll::Ready(Err(io::IoError::Interrupted));
        }
        if pipe.read_calls.is_multiple_of(7) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if pipe.bytes.is_empty() {
            pipe.read_wake = Some(cx.waker().clone());
            return if pipe.eof {
                Poll::Ready(Ok(0))
            } else {
                Poll::Pending
            };
        }
        let count = output.len().min(pipe.bytes.len()).min(113);
        for byte in &mut output[..count] {
            *byte = pipe.bytes.pop_front().unwrap();
        }
        if let Some(wake) = pipe.write_wake.take() {
            wake.wake();
        }
        Poll::Ready(Ok(count))
    }
}
impl capntproto_rpc_engine::io::Flush for Writer {
    type Error = &'static str;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<Self::Error>>> {
        let mut pipe = self.0.borrow_mut();
        pipe.flush_calls += 1;
        assert!(!pipe.closed);
        if pipe.fail_flush {
            return Poll::Ready(Err(io::IoError::Failed("flush failed")));
        }
        if pipe.block_flush {
            pipe.write_wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let Pipe {
            bytes, buffered, ..
        } = &mut *pipe;
        bytes.append(buffered);
        if let Some(wake) = pipe.read_wake.take() {
            wake.wake();
        }
        Poll::Ready(Ok(()))
    }
}
impl io::Close for Writer {
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<Self::Error>>> {
        let mut pipe = self.0.borrow_mut();
        pipe.close_calls += 1;
        assert!(pipe.buffered.is_empty(), "close must follow flush");
        assert!(!pipe.closed, "closed writer cannot be polled again");
        if pipe.fail_close {
            return Poll::Ready(Err(io::IoError::Failed("close failed")));
        }
        if pipe.block_close {
            pipe.write_wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        pipe.closed = true;
        pipe.eof = true;
        if let Some(wake) = pipe.read_wake.take() {
            wake.wake();
        }
        Poll::Ready(Ok(()))
    }
}
impl io::Write for Writer {
    fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, io::IoError<Self::Error>>> {
        let mut pipe = self.0.borrow_mut();
        assert!(!pipe.closed);
        pipe.write_calls += 1;
        if pipe.fail_write {
            return Poll::Ready(Err(io::IoError::Failed("write failed")));
        }
        if pipe.write_calls.is_multiple_of(13) {
            return Poll::Ready(Err(io::IoError::Interrupted));
        }
        if pipe.write_calls.is_multiple_of(5) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let count = bytes
            .len()
            .min(97)
            .min(256usize.saturating_sub(pipe.bytes.len() + pipe.buffered.len()));
        if count == 0 || pipe.blocked {
            pipe.write_wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        pipe.buffered.extend(&bytes[..count]);
        Poll::Ready(Ok(count))
    }
}
#[derive(Clone)]
struct Echo(Rc<Cell<usize>>);
impl capability::Server for Echo {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        params: capability::Params<any_pointer::Owned>,
        mut results: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        self.0.set(self.0.get() + 1);
        let result = params.get().and_then(|params| results.get().set_as(params));
        capability::DispatchCallResult {
            promise: Promise::from(result),
            is_streaming: false,
            allow_cancellation: true,
        }
    }
}
fn expose(driver: &mut TestPeer<'_, '_, '_>, hook: Rc<dyn ClientHook>) -> ImportId {
    let client = driver
        .runtime
        .endpoint(
            Route::local(hook),
            capntproto_rpc_engine::clients::Location::Local,
        )
        .unwrap_or_else(|e| panic!("{}", e.error));
    let mut batch = driver.ledgers.exports.batch();
    let id = capntproto_rpc_wire::clients::stage(&mut driver.runtime, &mut batch, &client)
        .unwrap()
        .id();
    batch.commit();
    driver.runtime.release(client);
    ImportId::from_wire(id.to_wire())
}
fn remote<'p, 'id>(
    driver: &mut TestPeer<'p, 'id, '_>,
    hooks: &mut ClientHooks<'id>,
    id: ImportId,
) -> capability::Client {
    let mut received = [imports::ReceiveSlot::new(id, ())];
    driver
        .ledgers
        .imports
        .prepare_receive(&mut received)
        .unwrap()
        .commit();
    let receipt = received[0].take_local().unwrap();
    let cap = driver
        .imports
        .bind(&mut driver.runtime, &mut driver.ledgers.imports, &receipt)
        .unwrap();
    driver.ledgers.imports.release(receipt);
    let client = hooks.export_client(&mut driver.runtime, &cap).unwrap();
    driver.runtime.release(cap);
    client
}

#[test]
fn hook_collection_reactivates_already_idle_maintenance() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(8, 8);
        let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
        let mut coordinator = Coordinator::new(
            driver,
            hooks,
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        let mut scratch = [0; 8];
        let mut cx = Context::from_waker(Waker::noop());
        drop(client);
        // Reproduce a rotation where maintenance went idle before hook
        // collection observes the final application owner disappearing.
        coordinator.active = 1;
        coordinator.cursor = 0;
        coordinator.wake.changed.store(0, Ordering::Release);
        for _ in 0..256 {
            let result =
                coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx);
            if let Poll::Ready(result) = result {
                result.unwrap();
            }
            if !queue.is_empty() {
                break;
            }
            assert_ne!(
                coordinator.active, 0,
                "retired import must keep cleanup runnable"
            );
        }
        {
            let window = queue
                .window(messages)
                .unwrap()
                .expect("Release publication");
            let mut bytes = window.bytes();
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                Default::default(),
            )
            .unwrap();
            let root = reader
                .get_root::<crate::rpc_capnp::message::Reader>()
                .unwrap();
            let crate::rpc_capnp::message::Release(release) = root.which().unwrap() else {
                panic!("Release")
            };
            assert_eq!(release.unwrap().get_id(), 7);
        }
        futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
            messages,
            &mut queue,
            &mut Policy::default(),
            Ok(()),
        ))
        .unwrap();
        let recovered: Vec<_> = (0..32).map(|_| messages.reserve(512).unwrap()).collect();
        for writer in recovered {
            messages.discard(writer);
        }
    });
}

#[test]
fn fragmented_duplex_calls_survive_single_slot_output_and_execution_limits() {
    duplex_calls(BootstrapMode::Seeded);
}

#[test]
fn published_direct_call_sleeps_until_cancellation_or_new_admission() {
    for cancel in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(16, 16);
            let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
            let call = || {
                client
                    .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                        7,
                        0,
                        None,
                        capability::CallHints {
                            no_promise_pipelining: true,
                            ..Default::default()
                        },
                    )
                    .send()
            };
            let mut request = Some(call());
            let mut opts = options();
            opts.budget = NonZeroUsize::new(64).unwrap();
            let mut coordinator =
                Coordinator::new(driver, hooks, NonZeroUsize::new(4).unwrap(), None, opts).unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let mut scratch = [0; 32];
            let mut cx = Context::from_waker(Waker::noop());
            assert!(matches!(
                coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx),
                Poll::Ready(Ok(()))
            ));
            assert_eq!(queue.len(), 1);
            assert_eq!(coordinator.active & (1 << 2), 0);
            {
                let window = queue.window(messages).unwrap().unwrap();
                let size = window.bytes().len();
                window.advance(size).unwrap();
            }
            for _ in 0..256 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.active == 0 {
                    break;
                }
            }
            assert_eq!(coordinator.active, 0);
            assert!(queue.is_empty());
            assert_eq!(coordinator.wake.changed.load(Ordering::Acquire), 0);
            let second = if cancel {
                drop(request.take());
                None
            } else {
                Some(call())
            };
            assert_ne!(coordinator.wake.changed.load(Ordering::Acquire), 0);
            for _ in 0..256 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            {
                let window = queue.window(messages).unwrap().expect("woken publication");
                let mut bytes = window.bytes();
                let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                    &mut bytes,
                    Default::default(),
                )
                .unwrap();
                let root = reader
                    .get_root::<crate::rpc_capnp::message::Reader>()
                    .unwrap();
                if cancel {
                    assert!(matches!(
                        root.which().unwrap(),
                        crate::rpc_capnp::message::Finish(_)
                    ));
                } else {
                    assert!(matches!(
                        root.which().unwrap(),
                        crate::rpc_capnp::message::Call(_)
                    ));
                }
            }
            drop((second, request, client));
            futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                messages,
                &mut queue,
                &mut Policy::default(),
                Ok(()),
            ))
            .unwrap();
            let recovered: Vec<_> = (0..32).map(|_| messages.reserve(512).unwrap()).collect();
            for writer in recovered {
                messages.discard(writer);
            }
        });
    }
}

#[test]
fn ready_remote_call_reaches_output_within_one_budgeted_turn() {
    for budget in [8, 64] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(16, 16);
            let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
            let request = client
                .new_call::<any_pointer::Owned, any_pointer::Owned>(7, 0, None)
                .send();
            let mut opts = options();
            opts.budget = NonZeroUsize::new(budget).unwrap();
            let mut coordinator =
                Coordinator::new(driver, hooks, NonZeroUsize::new(4).unwrap(), None, opts).unwrap();
            let output = Rc::new(RefCell::new(Pipe::default()));
            let mut writer = Writer(output.clone());
            let mut writer = transport::Writer(&mut writer);
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            coordinator
                .poll_io_work::<(), _, _>(
                    messages,
                    &mut queue,
                    &mut writer,
                    &mut scratch,
                    &NoResources,
                    &mut policy,
                    &mut Context::from_waker(Waker::noop()),
                )
                .unwrap();
            assert!(
                output.borrow().write_calls > 0,
                "ready request required an executor round trip before any IO (budget {budget})"
            );
            drop((request, client));
            futures::executor::block_on(coordinator.terminate::<NoResources, (), &'static str>(
                messages,
                &mut queue,
                &mut policy,
                Ok(()),
            ))
            .unwrap();
            reclaimed(messages);
        });
    }
}

#[test]
fn empty_application_lanes_need_no_output_slot_but_ready_calls_do() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(16, 16);
        let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
        let mut coordinator = Coordinator::new(
            driver,
            hooks,
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        // The other lanes have certified idle. Empty application/command
        // lanes cannot publish, so output backpressure must not hide their
        // idle certificates or turn an empty poll into a capacity failure.
        coordinator.active = (1 << 2) | (1 << 3) | (1 << 5);
        let mut queue = messages::Queue::new_growing(0, 0).unwrap();
        let mut scratch = [0; 32];
        let mut cx = Context::from_waker(Waker::noop());
        assert!(coordinator
            .poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx,)
            .is_pending());
        assert_eq!(coordinator.active, 0);
        let request = client
            .new_call::<any_pointer::Owned, any_pointer::Owned>(7, 0, None)
            .send();
        assert!(matches!(
            coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx,),
            Poll::Ready(Err(_))
        ));
        assert!(
            coordinator.calls.is_empty(),
            "reject before admitting a Call"
        );
        assert_eq!(coordinator.hooks.inbox.queue.borrow().len(), 1);
        drop((request, client));
        futures::executor::block_on(coordinator.terminate::<NoResources, (), &'static str>(
            messages,
            &mut queue,
            &mut Policy::default(),
            Ok(()),
        ))
        .unwrap();
        reclaimed(messages);
    });
}

#[test]
fn bootstrap_pipelines_dispatch_before_return_and_factory_runs_once() {
    duplex_calls(BootstrapMode::Direct);
}

#[test]
fn failed_bootstrap_factory_leaves_connection_usable() {
    duplex_calls(BootstrapMode::Failed);
}

#[test]
fn owned_handle_bootstrap_pipelines_calls_through_the_same_driver() {
    duplex_calls(BootstrapMode::Handle);
}

#[test]
fn bootstrap_pending_capability_resolves_without_losing_pipelined_calls() {
    duplex_calls(BootstrapMode::Pending);
}

enum BootstrapMode {
    Seeded,
    Direct,
    Failed,
    Handle,
    Pending,
}
fn duplex_calls(mode: BootstrapMode) {
    let bootstrap = !matches!(mode, BootstrapMode::Seeded);
    let fail_first = matches!(mode, BootstrapMode::Failed);
    let use_handle = matches!(mode, BootstrapMode::Handle);
    let pending = matches!(mode, BootstrapMode::Pending);
    with_peer(|mut a, am| {
        with_peer(|mut b, bm| {
            let seen = Rc::new(Cell::new(0));
            let factory_calls = Cell::new(0);
            let (sender, receiver) = shared::channel::<capability::Client>();
            let mut sender = Some(sender);
            let hook: Rc<dyn ClientHook> = if pending {
                Rc::from(
                    crate::new_future_client::<capability::Client>(async move {
                        receiver.await.map_err(|_| {
                            Error::disconnected("bootstrap resolution canceled".into())
                        })
                    })
                    .hook,
                )
            } else {
                Rc::new(crate::local::Client::new(Echo(seen.clone())))
            };
            let mut ah = ClientHooks::new(32, 16);
            let seeded =
                (!bootstrap).then(|| remote(&mut a, &mut ah, expose(&mut b, hook.clone())));
            let mut a =
                Coordinator::new(a, ah, NonZeroUsize::new(1).unwrap(), None, options()).unwrap();
            let b = Coordinator::new(
                b,
                ClientHooks::new(32, 16),
                NonZeroUsize::new(1).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let (mut aq, mut bq) = (
                messages::Queue::new_growing(0, if fail_first { 2 } else { 1 }).unwrap(),
                messages::Queue::new_growing(0, 1).unwrap(),
            );
            let failed_client = fail_first.then(|| a.bootstrap(am, &mut aq).unwrap());
            let mut failure = failed_client
                .as_ref()
                .map(capability::Client::when_resolved);
            let handle = a.handle();
            let client = seeded.unwrap_or_else(|| {
                if use_handle {
                    handle.bootstrap()
                } else {
                    a.bootstrap(am, &mut aq).unwrap()
                }
            });
            let size = if cfg!(miri) || pending { 257 } else { 65536 };
            let payloads = [vec![5; size], vec![7; 9], vec![8; 513]];
            let mut promises: Vec<_> = payloads
                .iter()
                .map(|payload| {
                    let mut request: capability::Request<any_pointer::Owned, any_pointer::Owned> =
                        client.new_call(7, 0, None);
                    request
                        .get()
                        .set_as::<capnp::data::Owned>(payload.as_slice())
                        .unwrap();
                    Some(request.send().promise)
                })
                .collect();
            let ab = Rc::new(RefCell::new(Pipe::default()));
            let ba = Rc::new(RefCell::new(Pipe::default()));
            let (mut ar, mut aw, mut br, mut bw) = (
                Reader(ba.clone()),
                Writer(ab.clone()),
                Reader(ab),
                Writer(ba),
            );
            let mut ap = Policy::default();
            let mut bp = TwoParty::new(
                || {
                    factory_calls.set(factory_calls.get() + 1);
                    if fail_first && factory_calls.get() == 1 {
                        return Err(Error::overloaded("peer may retry bootstrap".into()));
                    }
                    Ok(capability::Client::new(hook.add_ref()))
                },
                NonZeroUsize::new(4).unwrap(),
            );
            let (astop, bstop) = (Signal::default(), Signal::default());
            let (mut ascratch, mut bscratch) = ([0; 32], [0; 32]);
            {
                let mut af = std::pin::pin!(a.run(
                    am,
                    &mut aq,
                    &mut ar,
                    &mut aw,
                    &mut ascratch,
                    |_| (),
                    &mut ap,
                    astop.wait()
                ));
                let mut bf = std::pin::pin!(b.run(
                    bm,
                    &mut bq,
                    &mut br,
                    &mut bw,
                    &mut bscratch,
                    |_| (),
                    &mut bp,
                    bstop.wait()
                ));
                let mut cx = Context::from_waker(Waker::noop());
                for iteration in 0..20000 {
                    if pending && iteration == 400 {
                        assert_eq!(factory_calls.get(), 1);
                        assert_eq!(
                            seen.get(),
                            0,
                            "pending bootstrap cannot execute before resolution"
                        );
                        assert!(sender
                            .take()
                            .unwrap()
                            .send(capability::Client::new(Box::new(
                                crate::local::Client::new(Echo(seen.clone()))
                            )))
                            .is_ok());
                    }
                    if let Poll::Ready(done) = af.as_mut().poll(&mut cx) {
                        panic!("client stopped: {done:?}")
                    }
                    if let Poll::Ready(done) = bf.as_mut().poll(&mut cx) {
                        panic!("server stopped: {done:?}")
                    }
                    if let Some(future) = &mut failure {
                        if let Poll::Ready(result) = Pin::new(future).poll(&mut cx) {
                            assert_eq!(result.unwrap_err().kind, capnp::ErrorKind::Overloaded);
                            failure = None;
                        }
                    }
                    for (promise, expected) in promises.iter_mut().zip(&payloads) {
                        if let Some(future) = promise {
                            if let Poll::Ready(result) = Pin::new(future).poll(&mut cx) {
                                assert_eq!(
                                    result
                                        .unwrap()
                                        .get()
                                        .unwrap()
                                        .get_as::<capnp::data::Reader>()
                                        .unwrap(),
                                    expected
                                );
                                *promise = None;
                            }
                        }
                    }
                    if promises.iter().all(Option::is_none) && failure.is_none() {
                        break;
                    }
                }
                assert!(promises.iter().all(Option::is_none), "calls did not finish");
                assert!(failure.is_none(), "bootstrap error did not resolve");
                assert_eq!(
                    seen.get(),
                    3,
                    "output retries must not execute a method twice"
                );
                drop(client);
                astop.stop();
                bstop.stop();
                let (mut adone, mut bdone) = (false, false);
                for _ in 0..2000 {
                    if !adone {
                        if let Poll::Ready(result) = af.as_mut().poll(&mut cx) {
                            result.unwrap();
                            adone = true;
                        }
                    }
                    if !bdone {
                        if let Poll::Ready(result) = bf.as_mut().poll(&mut cx) {
                            result.unwrap();
                            bdone = true;
                        }
                    }
                    if adone && bdone {
                        break;
                    }
                }
                assert!(adone && bdone);
            }
            assert!(ap.stopped);
            let closed: capability::Client = handle.bootstrap();
            let Poll::Ready(Err(error)) =
                Pin::new(&mut closed.when_resolved()).poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("closed handle")
            };
            assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
            assert_eq!(
                factory_calls.get(),
                usize::from(bootstrap) + usize::from(fail_first)
            );
            reclaimed(am);
            reclaimed(bm);
        })
    });
}

#[test]
fn idle_scan_sleeps_and_stop_recovers_partial_receive_before_any_more_io() {
    with_peer(|driver, messages| {
        let coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let input = Rc::new(RefCell::new(Pipe {
            bytes: [0, 0, 0].into(),
            ..Default::default()
        }));
        let mut reader = Reader(input.clone());
        let mut writer = Writer(Rc::new(RefCell::new(Pipe::default())));
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let stop = Signal::default();
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = std::pin::pin!(coordinator.run(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                stop.wait()
            ));
            let mut slept = false;
            for _ in 0..2048 {
                let before = count.0.load(Ordering::SeqCst);
                assert!(run.as_mut().poll(&mut cx).is_pending());
                if count.0.load(Ordering::SeqCst) == before {
                    slept = true;
                    break;
                }
            }
            assert!(
                slept,
                "an idle connection must reach Pending without a self wake"
            );
            assert!(input.borrow().bytes.is_empty());
            let reads = input.borrow().read_calls;
            stop.stop();
            let mut done = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    result.unwrap();
                    done = true;
                    break;
                }
            }
            assert!(done);
            assert_eq!(
                input.borrow().read_calls,
                reads,
                "stop must win over further input"
            );
        }
        assert!(policy.stopped);
        reclaimed(messages);
    });
}

#[test]
fn input_faults_and_rejected_controls_release_all_storage() {
    // Clean EOF, truncated frame, malformed root, transport failure, and an
    // unauthenticated/bootstrap request. No handler silently accepts controls.
    for case in 0..5 {
        with_peer(|driver, messages| {
            let bytes = match case {
                0 | 3 => VecDeque::new(),
                1 => vec![0; 3].into(),
                2 => vec![0; 16].into(),
                _ => {
                    let frame = Frame::build(messages, 512, Default::default(), |builder| {
                        capntproto_rpc_wire::calls::bootstrap(
                            builder,
                            capntproto_rpc_engine::ids::QuestionId::from_wire(0),
                        )
                    })
                    .unwrap();
                    let bytes = frame
                        .with(messages, |root| {
                            let mut message = capnp::message::Builder::new_default();
                            message
                                .set_root::<capntproto_rpc_wire::rpc_capnp::message::Owned>(root)
                                .unwrap();
                            capnp::serialize::write_message_to_words(&message)
                        })
                        .unwrap();
                    frame.release(messages);
                    bytes.into()
                }
            };
            let coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let mut reader = Reader(Rc::new(RefCell::new(Pipe {
                bytes,
                eof: true,
                fail_read: case == 3,
                ..Default::default()
            })));
            let mut writer = Writer(Rc::new(RefCell::new(Pipe::default())));
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            {
                let mut run = std::pin::pin!(coordinator.run(
                    messages,
                    &mut queue,
                    &mut reader,
                    &mut writer,
                    &mut scratch,
                    |_| (),
                    &mut policy,
                    std::future::pending()
                ));
                let mut outcome = None;
                for _ in 0..2000 {
                    if let Poll::Ready(result) =
                        run.as_mut().poll(&mut Context::from_waker(Waker::noop()))
                    {
                        outcome = Some(result);
                        break;
                    }
                }
                let outcome = outcome.expect("terminal cleanup completed");
                if case == 0 {
                    outcome.unwrap();
                } else {
                    assert!(outcome.unwrap_err().cleanup.is_none());
                }
            }
            assert!(policy.stopped);
            reclaimed(messages);
        });
    }
}

#[test]
fn write_failure_recovers_a_pending_reader_and_accepted_output() {
    with_peer(|mut driver, messages| {
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        driver
            .send(
                messages,
                &mut queue,
                512,
                Default::default(),
                capntproto_rpc_wire::calls::bootstrap,
            )
            .unwrap();
        let coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let input = Rc::new(RefCell::new(Pipe {
            bytes: [0, 0, 0].into(),
            ..Default::default()
        }));
        let output = Rc::new(RefCell::new(Pipe {
            blocked: true,
            ..Default::default()
        }));
        let mut reader = Reader(input.clone());
        let mut writer = Writer(output.clone());
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        {
            let mut run = std::pin::pin!(coordinator.run(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::pending()
            ));
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..8 {
                assert!(run.as_mut().poll(&mut cx).is_pending());
            }
            assert!(input.borrow().bytes.is_empty());
            let reads = input.borrow().read_calls;
            output.borrow_mut().fail_write = true;
            let mut completed = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    let error = result.unwrap_err();
                    assert!(matches!(
                        error.cause,
                        Some(Cause::Output(io::SendError::Io("write failed")))
                    ));
                    assert!(error.cleanup.is_none());
                    completed = true;
                    break;
                }
            }
            assert!(completed);
            assert_eq!(input.borrow().read_calls, reads);
        }
        assert!(policy.stopped);
        assert!(queue.is_failed() && queue.is_empty());
        reclaimed(messages);
    });
}

#[test]
fn transport_releases_unclaimed_resources_on_every_terminal_path() {
    use capntproto_rpc_wire::attachments::ImportReceiver;
    struct Input {
        reader: Reader,
        resource: Option<Rc<()>>,
        finishes: usize,
    }
    impl io::Read for Input {
        type Error = &'static str;
        fn poll_read(
            &mut self,
            cx: &mut Context<'_>,
            bytes: &mut [u8],
        ) -> Poll<Result<usize, io::IoError<Self::Error>>> {
            self.reader.poll_read(cx, bytes)
        }
    }
    impl ImportReceiver<()> for Input {
        fn make_import(&mut self, _: ImportId) {}
    }
    impl transport::Input<()> for Input {
        fn finish_frame(&mut self) {
            self.resource.take();
            self.finishes += 1;
        }
    }
    // EOF, truncated frame, invalid envelope, read failure, immediate stop,
    // stop with a partial header, and write failure with a partial header.
    for case in 0..7 {
        with_peer(|mut driver, messages| {
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            if case == 6 {
                driver
                    .bootstrap(messages, &mut queue, 512, Default::default())
                    .map(|client| driver.runtime.release(client))
                    .unwrap();
            }
            let coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let incoming = Rc::new(RefCell::new(Pipe {
                bytes: match case {
                    1 | 5 | 6 => vec![0; 3].into(),
                    2 => vec![0; 16].into(),
                    _ => VecDeque::new(),
                },
                eof: case < 3,
                fail_read: case == 3,
                ..Default::default()
            }));
            let outgoing = Rc::new(RefCell::new(Pipe {
                blocked: case == 6,
                ..Default::default()
            }));
            let resource = Rc::new(());
            let weak = Rc::downgrade(&resource);
            let mut input = Input {
                reader: Reader(incoming.clone()),
                resource: Some(resource),
                finishes: 0,
            };
            let mut writer = Writer(outgoing.clone());
            let stop = Signal::default();
            if case == 4 {
                stop.stop();
            }
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            {
                let mut writer = transport::Writer(&mut writer);
                let mut run = std::pin::pin!(coordinator.run_transport(
                    messages,
                    &mut queue,
                    &mut input,
                    &mut writer,
                    &mut scratch,
                    &NoResources,
                    &mut policy,
                    stop.wait()
                ));
                let mut cx = Context::from_waker(Waker::noop());
                if case >= 5 {
                    for _ in 0..8 {
                        assert!(run.as_mut().poll(&mut cx).is_pending());
                    }
                    assert!(incoming.borrow().bytes.is_empty());
                    assert!(
                        weak.upgrade().is_some(),
                        "partial input must retain resources"
                    );
                    if case == 5 {
                        stop.stop();
                    } else {
                        outgoing.borrow_mut().fail_write = true;
                    }
                }
                let mut completed = false;
                for _ in 0..2000 {
                    if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                        if matches!(case, 0 | 4 | 5) {
                            result.unwrap();
                        } else {
                            assert!(result.unwrap_err().cleanup.is_none());
                        }
                        completed = true;
                        break;
                    }
                }
                assert!(completed);
            }
            assert!(
                weak.upgrade().is_none(),
                "resources must close while caller retains input"
            );
            assert_eq!(input.finishes, 1);
            assert!(policy.stopped);
            reclaimed(messages);
        });
    }
}

#[test]
fn foreign_resolution_invalidates_an_idle_lane_during_an_unfinished_scan() {
    with_peer(|driver, messages| {
        let mut opts = options();
        opts.budget = NonZeroUsize::new(1).unwrap();
        let mut coordinator = Coordinator::new(
            driver,
            ClientHooks::new(16, 16),
            NonZeroUsize::new(4).unwrap(),
            None,
            opts,
        )
        .unwrap();
        let ready = Rc::new(Signal::default());
        let signal = ready.clone();
        let seen = Rc::new(Cell::new(0));
        let server = Echo(seen.clone());
        let pending: capability::Client = crate::new_future_client(async move {
            signal.wait().await?;
            Ok(capability::Client::new(Box::new(
                crate::local::Client::new(server),
            )))
        });
        let cap = coordinator
            .foreign
            .import(&mut coordinator.driver.runtime, &*pending.hook)
            .unwrap();
        let client = coordinator
            .hooks
            .export_client(&mut coordinator.driver.runtime, &cap)
            .unwrap();
        coordinator.driver.runtime.release(cap);
        let request: capability::Request<any_pointer::Owned, any_pointer::Owned> =
            client.new_call(7, 0, None);
        let mut response = request.send().promise;
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut scratch = [0; 32];
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        for _ in 0..256 {
            if let Poll::Ready(result) =
                coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
            {
                result.unwrap();
            }
            if coordinator.active & 1 == 0
                && coordinator.active != 0
                && ready.wake.borrow().is_some()
            {
                break;
            }
        }
        assert_eq!(
            coordinator.active & 1,
            0,
            "promise lane completed its pending scan"
        );
        assert_ne!(coordinator.active, 0, "other lanes are still scanning");
        ready.stop();
        if let Poll::Ready(result) =
            coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
        {
            result.unwrap();
        }
        assert_ne!(
            coordinator.active & 1,
            0,
            "application wake invalidates the idle certificate immediately"
        );
        let mut completed = false;
        for _ in 0..2048 {
            let before = count.0.load(Ordering::SeqCst);
            let progressed = if let Poll::Ready(result) =
                coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
            {
                result.unwrap();
                true
            } else {
                false
            };
            if let Poll::Ready(result) = Pin::new(&mut response).poll(&mut cx) {
                result.unwrap();
                completed = true;
                break;
            }
            // The outer pump reschedules successful progress. Only a genuine
            // Pending without a notification can put the connection to sleep.
            if !progressed && count.0.load(Ordering::SeqCst) == before {
                break;
            }
        }
        assert!(
            completed,
            "notification must reactivate a previously idle lane"
        );
        assert_eq!(seen.get(), 1);
        drop((response, client, pending));
        let mut reader = Reader(Rc::new(RefCell::new(Pipe::default())));
        let mut writer = Writer(Rc::new(RefCell::new(Pipe::default())));
        let mut policy = Policy::default();
        {
            let mut run = std::pin::pin!(coordinator.run(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::ready(Ok(()))
            ));
            let mut done = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    result.unwrap();
                    done = true;
                    break;
                }
            }
            assert!(done);
        }
        reclaimed(messages);
    });
}

#[test]
fn bootstrap_exception_details_survive_full_fifo_without_repeating_factory() {
    with_peer(|driver, messages| {
        let mut coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let calls = Cell::new(0);
        let mut control = TwoParty::new(
            || {
                calls.set(calls.get() + 1);
                let mut error = Error::failed("bootstrap denied for authenticated peer".into());
                error.set_detail(123, vec![1, 2, 3]);
                Err(error)
            },
            NonZeroUsize::new(2).unwrap(),
        );
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let incoming = Frame::build(messages, 512, Default::default(), |b| {
            capntproto_rpc_wire::calls::bootstrap(
                b,
                capntproto_rpc_engine::ids::QuestionId::from_wire(7),
            )
        })
        .unwrap();
        let received = coordinator
            .driver
            .receive(
                incoming,
                messages,
                &mut [0; 16],
                |_| (),
                server::Results::new,
            )
            .unwrap_or_else(|e| panic!("{}", e.error));
        <TwoParty<'_, _> as Control<'_, '_, '_, (), u8, 8, 8>>::received(
            &mut control,
            received,
            &mut coordinator,
            messages,
            &mut queue,
        )
        .unwrap();
        coordinator
            .driver
            .send(
                messages,
                &mut queue,
                512,
                Default::default(),
                capntproto_rpc_wire::calls::bootstrap,
            )
            .unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..5 {
            assert!(control
                .poll(
                    &mut coordinator,
                    messages,
                    &mut queue,
                    &mut [0; 32],
                    &NoResources,
                    &mut cx
                )
                .is_pending());
        }
        assert_eq!(calls.get(), 1);
        let window = queue.window(messages).unwrap().unwrap();
        let size = window.bytes().len();
        window.advance(size).unwrap();
        assert!(matches!(
            control.poll(
                &mut coordinator,
                messages,
                &mut queue,
                &mut [0; 32],
                &NoResources,
                &mut cx
            ),
            Poll::Ready(Ok(true))
        ));
        let window = queue.window(messages).unwrap().unwrap();
        let size = window.bytes().len();
        let mut bytes = window.bytes();
        let message =
            capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, Default::default())
                .unwrap();
        let root: capntproto_rpc_wire::rpc_capnp::message::Reader = message.get_root().unwrap();
        let capntproto_rpc_wire::rpc_capnp::message::Return(returned) = root.which().unwrap()
        else {
            panic!("return")
        };
        let returned = returned.unwrap();
        assert_eq!(returned.get_answer_id(), 7);
        let capntproto_rpc_wire::rpc_capnp::return_::Exception(error) = returned.which().unwrap()
        else {
            panic!("exception")
        };
        let error = error.unwrap();
        assert_eq!(
            error.get_reason().unwrap().to_str().unwrap(),
            "bootstrap denied for authenticated peer"
        );
        let detail = error.get_details().unwrap().get(0);
        assert_eq!(detail.get_detail_id(), 123);
        assert_eq!(detail.get_data().unwrap(), [1, 2, 3]);
        window.advance(size).unwrap();
        assert!(control
            .poll(
                &mut coordinator,
                messages,
                &mut queue,
                &mut [0; 32],
                &NoResources,
                &mut cx
            )
            .is_pending());
        assert_eq!(calls.get(), 1);
        let input = Rc::new(RefCell::new(Pipe::default()));
        let output = Rc::new(RefCell::new(Pipe::default()));
        let mut reader = Reader(input);
        let mut writer = Writer(output);
        let mut scratch = [0; 32];
        let run = coordinator.run(
            messages,
            &mut queue,
            &mut reader,
            &mut writer,
            &mut scratch,
            |_| (),
            &mut control,
            async { Ok(()) },
        );
        {
            let mut run = std::pin::pin!(run);
            let mut done = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    result.unwrap();
                    done = true;
                    break;
                }
            }
            assert!(done);
        }
        reclaimed(messages);
    });
}

#[test]
fn idle_handle_wakes_and_canceled_commands_never_publish_bootstrap() {
    with_peer(|driver, messages| {
        let coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(1).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let handle = coordinator.handle();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let input = Rc::new(RefCell::new(Pipe::default()));
        let output = Rc::new(RefCell::new(Pipe::default()));
        let mut reader = Reader(input);
        let mut writer = Writer(output.clone());
        let mut policy = Policy::default();
        let stop = Signal::default();
        let mut scratch = [0; 32];
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = std::pin::pin!(coordinator.run(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                stop.wait()
            ));
            // Reach a real idle scan, then enqueue through a handle with no
            // access to driver, message-pool or IO borrows.
            for _ in 0..2000 {
                let before = wakes.0.load(Ordering::SeqCst);
                assert!(run.as_mut().poll(&mut cx).is_pending());
                if wakes.0.load(Ordering::SeqCst) == before {
                    break;
                }
            }
            let before = wakes.0.load(Ordering::SeqCst);
            let abandoned: capability::Client = handle.bootstrap();
            assert!(
                wakes.0.load(Ordering::SeqCst) > before,
                "command must wake an idle driver"
            );
            let overloaded: capability::Client = handle.bootstrap();
            assert!(
                matches!(Pin::new(&mut overloaded.when_resolved()).poll(&mut cx), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Overloaded)
            );
            drop(abandoned);
            for _ in 0..2000 {
                let before = wakes.0.load(Ordering::SeqCst);
                assert!(run.as_mut().poll(&mut cx).is_pending());
                if wakes.0.load(Ordering::SeqCst) == before {
                    break;
                }
            }
            assert!(
                output.borrow().bytes.is_empty(),
                "abandoned command must not run a peer factory"
            );
            let pending: capability::Client = handle.bootstrap();
            let mut resolved = pending.when_resolved();
            assert!(Pin::new(&mut resolved).poll(&mut cx).is_pending());
            for _ in 0..2000 {
                assert!(run.as_mut().poll(&mut cx).is_pending());
                if !output.borrow().bytes.is_empty() {
                    break;
                }
            }
            assert!(
                !output.borrow().bytes.is_empty(),
                "live command published Bootstrap"
            );
            stop.stop();
            let mut completed = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    result.unwrap();
                    completed = true;
                    break;
                }
            }
            assert!(completed);
            assert!(
                matches!(Pin::new(&mut resolved).poll(&mut cx), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected)
            );
        }
        let client: capability::Client = handle.bootstrap();
        assert!(
            matches!(Pin::new(&mut client.when_resolved()).poll(&mut cx), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected)
        );
        reclaimed(messages);
    });
}

#[test]
fn stop_before_admission_rejects_owned_commands_without_io() {
    with_peer(|driver, messages| {
        let coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(1).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let handle = coordinator.handle();
        let client: capability::Client = handle.bootstrap();
        let mut result = client.when_resolved();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let input = Rc::new(RefCell::new(Pipe::default()));
        let output = Rc::new(RefCell::new(Pipe::default()));
        let mut reader = Reader(input.clone());
        let mut writer = Writer(output.clone());
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let mut cx = Context::from_waker(Waker::noop());
        {
            let mut run = std::pin::pin!(coordinator.run(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                async { Err(Error::overloaded("caller stopped this connection".into())) }
            ));
            let mut done = false;
            for _ in 0..2000 {
                if let Poll::Ready(outcome) = run.as_mut().poll(&mut cx) {
                    assert!(outcome.unwrap_err().cleanup.is_none());
                    done = true;
                    break;
                }
            }
            assert!(done);
        }
        let Poll::Ready(Err(error)) = Pin::new(&mut result).poll(&mut cx) else {
            panic!("pending command must close")
        };
        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(error.extra, "caller stopped this connection");
        assert_eq!(input.borrow().read_calls, 0);
        assert_eq!(output.borrow().write_calls, 0);
        reclaimed(messages);
    });
}

#[test]
fn newly_published_output_cannot_report_idle_before_physical_flush() {
    with_peer(|driver, messages| {
        let mut opts = options();
        opts.budget = NonZeroUsize::new(1024).unwrap();
        let mut coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            opts,
        )
        .unwrap();
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        let pipe = Rc::new(RefCell::new(Pipe::default()));
        let mut writer = Writer(pipe.clone());
        let mut control = Policy::default();
        let mut scratch = [0; 8];
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..8 {
            coordinator
                .poll_io_work::<(), _, _>(
                    messages,
                    &mut queue,
                    &mut transport::Writer(&mut writer),
                    &mut scratch,
                    &NoResources,
                    &mut control,
                    &mut cx,
                )
                .unwrap();
        }
        assert!(coordinator.idle);
        control.idle_transitions.clear();
        control.publish_on_poll = true;
        control.wake.as_ref().unwrap().wake_by_ref();
        pipe.borrow_mut().block_flush = true;
        coordinator
            .poll_io_work::<(), _, _>(
                messages,
                &mut queue,
                &mut transport::Writer(&mut writer),
                &mut scratch,
                &NoResources,
                &mut control,
                &mut cx,
            )
            .unwrap();
        assert!(queue.is_empty(), "frame accepted by transport");
        assert!(!pipe.borrow().buffered.is_empty(), "flush remains pending");
        assert!(
            !control.idle_transitions.contains(&true),
            "the earlier empty-output poll cannot certify newly published output"
        );
        assert!(!coordinator.idle);
        pipe.borrow_mut().block_flush = false;
        if let Some(wake) = pipe.borrow_mut().write_wake.take() {
            wake.wake();
        }
        for _ in 0..8 {
            coordinator
                .poll_io_work::<(), _, _>(
                    messages,
                    &mut queue,
                    &mut transport::Writer(&mut writer),
                    &mut scratch,
                    &NoResources,
                    &mut control,
                    &mut cx,
                )
                .unwrap();
        }
        assert!(pipe.borrow().buffered.is_empty());
        assert!(coordinator.idle);
        assert_eq!(control.idle_transitions, [false, true]);
        reclaimed(messages);
    });
}

#[test]
fn outgoing_notification_coalesces_without_losing_general_work() {
    use std::sync::atomic::AtomicUsize;
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let state = Arc::<WorkWake>::default();
    let outgoing = EnqueueWake(state.clone());
    let general = Waker::from(state.clone());
    let first = Arc::new(Count(AtomicUsize::new(0)));
    let second = Arc::new(Count(AtomicUsize::new(0)));
    for outgoing_first in [true, false] {
        state.parent.register(&Waker::from(first.clone()));
        state.changed.store(0, Ordering::Release);
        let before = first.0.load(Ordering::SeqCst);
        if outgoing_first {
            outgoing.notify();
        } else {
            general.wake_by_ref();
        }
        assert_eq!(
            state.changed.load(Ordering::Acquire),
            if outgoing_first { 4 } else { 0x3f }
        );
        if outgoing_first {
            general.wake_by_ref();
        } else {
            outgoing.notify();
        }
        assert_eq!(state.changed.load(Ordering::Acquire), 0x3f);
        assert_eq!(first.0.load(Ordering::SeqCst), before + 1);
        // The consumer registers before consuming the pending mask. A later
        // outgoing-only notification must reach the new task, not the old one.
        state.parent.register(&Waker::from(second.clone()));
        assert_eq!(state.changed.swap(0, Ordering::AcqRel), 0x3f);
        outgoing.notify();
        assert_eq!(state.changed.load(Ordering::Acquire), 4);
        assert_eq!(first.0.load(Ordering::SeqCst), before + 1);
    }
    assert_eq!(second.0.load(Ordering::SeqCst), 2);
}

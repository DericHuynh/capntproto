#![no_std]
//! Actual allocating legacy RPC without std, driven by application-owned transport.
extern crate alloc;
#[cfg(test)]
extern crate std;
use alloc::{boxed::Box, collections::VecDeque, rc::Rc, sync::Arc, task::Wake};
use capnp::{
    capability::{FromClientHook, Promise},
    message, Error,
};
use capnp_rpc::{
    ClientRegistry, Connection, IncomingMessage, OutgoingMessage, RpcSystem, VatNetwork,
};
use core::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
pub mod legacy_capnp {
    include!(concat!(env!("OUT_DIR"), "/legacy_capnp.rs"));
}
use legacy_capnp::node;

type Message = Rc<message::Builder<message::HeapAllocator>>;
#[derive(Default)]
struct Inbox {
    messages: VecDeque<Message>,
    wake: Option<Waker>,
    closed: bool,
    sent: usize,
    fragmented: bool,
    bytes: VecDeque<u8>,
    error: Option<capnp::ErrorKind>,
}
type Mailbox = Rc<RefCell<Inbox>>;
struct Frame {
    words: Box<[capnp::Word]>,
    len: usize,
}
impl AsRef<[u8]> for Frame {
    fn as_ref(&self) -> &[u8] {
        &capnp::Word::words_to_bytes(&self.words)[..self.len]
    }
}
enum Incoming {
    Direct(Message),
    Framed(message::Reader<capnp::serialize::NoAllocBufferSegments<Frame>>),
}
impl IncomingMessage for Incoming {
    fn size_in_words(&self) -> usize {
        match self {
            Self::Direct(message) => message.size_in_words(),
            Self::Framed(message) => {
                use capnp::message::ReaderSegments;
                let segments = message.get_segments();
                (0..segments.len())
                    .map(|index| segments.get_segment(index as u32).unwrap().len() / 8)
                    .sum()
            }
        }
    }
    fn get_body(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        match self {
            Self::Direct(message) => message.get_root_as_reader(),
            Self::Framed(message) => message.get_root(),
        }
    }
}
/// Three-byte reads, Interrupted and real Pending turns exercise the shared
/// core framing state machine. No complete frame is exposed until validated.
struct Fragmented {
    input: Mailbox,
    attempts: usize,
}
impl capnp_rpc::engine::io::Read for Fragmented {
    type Error = Error;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        destination: &mut [u8],
    ) -> Poll<Result<usize, capnp_rpc::engine::io::IoError<Error>>> {
        self.attempts += 1;
        if self.attempts.is_multiple_of(5) {
            return Poll::Ready(Err(capnp_rpc::engine::io::IoError::Interrupted));
        }
        if self.attempts.is_multiple_of(3) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let mut input = self.input.borrow_mut();
        if input.bytes.is_empty() {
            if input.closed {
                return Poll::Ready(Ok(0));
            }
            input.wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let count = destination.len().min(input.bytes.len()).min(3);
        for byte in &mut destination[..count] {
            *byte = input.bytes.pop_front().unwrap();
        }
        Poll::Ready(Ok(count))
    }
}
struct Outgoing {
    message: message::Builder<message::HeapAllocator>,
    output: Mailbox,
}
impl OutgoingMessage for Outgoing {
    fn get_body(&mut self) -> capnp::Result<capnp::any_pointer::Builder<'_>> {
        self.message.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.message.get_root_as_reader()
    }
    fn send(self: Box<Self>) -> (Promise<(), Error>, Message) {
        let message = Rc::new(self.message);
        let wake = {
            let mut output = self.output.borrow_mut();
            if output.fragmented {
                output
                    .bytes
                    .extend(capnp::serialize::write_message_to_words(&message));
            } else {
                output.messages.push_back(message.clone());
            }
            output.sent += 1;
            output.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        (Promise::ok(()), message)
    }
    fn take(self: Box<Self>) -> message::Builder<message::HeapAllocator> {
        self.message
    }
    fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
}
#[derive(Clone)]
struct Link {
    input: Mailbox,
    output: Mailbox,
    peer: bool,
}
impl Connection<bool> for Link {
    fn control_mode(&self) -> capnp_rpc::ControlMode {
        capnp_rpc::ControlMode::TwoParty
    }
    fn take_frame_io(&mut self) -> capnp::Result<capnp_rpc::FrameIo> {
        Err(Error::unimplemented("message transport".into()))
    }
    fn get_peer_vat_id(&self) -> bool {
        self.peer
    }
    fn connection_id(&self) -> usize {
        1
    }
    fn new_outgoing_message(&mut self, size: u32) -> Box<dyn OutgoingMessage> {
        let allocator = message::HeapAllocator::new().first_segment_words(size.max(8));
        Box::new(Outgoing {
            message: message::Builder::new(allocator),
            output: self.output.clone(),
        })
    }
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        let input = self.input.clone();
        if input.borrow().fragmented {
            return Promise::from_future(async move {
                let mut words = capnp::Word::allocate_zeroed_vec(256).into_boxed_slice();
                let mut receiver = capnp::serialize::FrameReceiver::new(
                    capnp::Word::words_to_bytes_mut(&mut words),
                    Default::default(),
                )?;
                let mut reader = Fragmented { input, attempts: 0 };
                let complete = poll_fn(|cx| {
                    capnp_rpc::bounded::io::poll_receive(
                        &mut reader,
                        &mut receiver,
                        cx,
                        core::num::NonZeroUsize::new(1).unwrap(),
                    )
                })
                .await;
                let complete = complete.map_err(|error| {
                    let error = match error {
                        capnp_rpc::bounded::io::ReceiveError::Io(error) => error,
                        capnp_rpc::bounded::io::ReceiveError::Wire(kind) => Error::from_kind(kind),
                    };
                    reader.input.borrow_mut().error = Some(error.kind);
                    error
                })?;
                if !complete {
                    return Ok(None);
                }
                let len = receiver.received_bytes();
                let segments = capnp::serialize::NoAllocBufferSegments::from_buffer_exact(
                    Frame { words, len },
                    Default::default(),
                )?;
                let message = message::Reader::new(segments, Default::default());
                Ok(Some(
                    Box::new(Incoming::Framed(message)) as Box<dyn IncomingMessage>
                ))
            });
        }
        Promise::from_future(poll_fn(move |cx| {
            let mut input = input.borrow_mut();
            if let Some(message) = input.messages.pop_front() {
                Poll::Ready(Ok(Some(
                    Box::new(Incoming::Direct(message)) as Box<dyn IncomingMessage>
                )))
            } else if input.closed {
                Poll::Ready(Ok(None))
            } else {
                input.wake = Some(cx.waker().clone());
                Poll::Pending
            }
        }))
    }
    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), Error> {
        let wake = {
            let mut output = self.output.borrow_mut();
            output.closed = true;
            output.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        Promise::from(result)
    }
}
struct Network {
    link: Link,
    accept: bool,
}
impl VatNetwork<bool> for Network {
    fn take_output_driver(&mut self) -> Option<Promise<(), Error>> {
        None
    }
    fn connect(&mut self, peer: bool) -> Option<Box<dyn Connection<bool>>> {
        (peer == self.link.peer).then(|| Box::new(self.link.clone()) as Box<dyn Connection<bool>>)
    }
    fn accept(&mut self) -> Promise<Box<dyn Connection<bool>>, Error> {
        if core::mem::take(&mut self.accept) {
            Promise::ok(Box::new(self.link.clone()))
        } else {
            Promise::from_future(core::future::pending())
        }
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        Promise::from_future(core::future::pending())
    }
}
struct DropCount(Rc<Cell<usize>>);
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
struct Service {
    registry: ClientRegistry,
    depth: u64,
    canceled: Rc<Cell<usize>>,
}
impl node::Server for Service {
    fn read(self: Rc<Self>, context: node::ReadContext<'_>) -> Promise<(), Error> {
        context.complete(|params, mut results| {
            results.set_value(params.get_value() + self.depth);
            Ok(())
        })
    }
    fn next(self: Rc<Self>, context: node::NextContext<'_>) -> Promise<(), Error> {
        let child: node::Client = self.registry.new_client(Service {
            registry: self.registry.clone(),
            depth: self.depth + 1,
            canceled: self.canceled.clone(),
        });
        context.complete(move |_, mut results| {
            results.set_child(child);
            Ok(())
        })
    }
    fn callback(self: Rc<Self>, context: node::CallbackContext<'_>) -> Promise<(), Error> {
        let (params, mut results) = context.into_owned();
        Promise::from_future(async move {
            let params = params.get()?;
            let target = params.get_target()?;
            let mut request = target.read_request();
            request.get().set_value(params.get_value());
            let response = request.send().promise.await?;
            results
                .get()
                .set_value(response.get()?.get_value() + self.depth);
            Ok(())
        })
    }
    fn hold(self: Rc<Self>, context: node::HoldContext<'_>) -> Promise<(), Error> {
        let (_params, results) = context.into_owned();
        let guard = DropCount(self.canceled.clone());
        Promise::from_future(async move {
            let _retained = (results, guard);
            core::future::pending::<capnp::Result<()>>().await
        })
    }
}
struct Ready(AtomicBool);
impl Ready {
    fn new() -> Arc<Self> {
        Arc::new(Self(AtomicBool::new(true)))
    }
    fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}
impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}
/// Host tests and embedded cargo-check instantiate this same legacy runtime.
pub struct Fixture {
    client_system: RpcSystem<bool>,
    server_system: RpcSystem<bool>,
    pub client: node::Client,
    registry: ClientRegistry,
    canceled: Rc<Cell<usize>>,
    queues: [Mailbox; 2],
    ready: [Arc<Ready>; 2],
    wakes: [Waker; 2],
}
impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_fragmented(false)
    }
    pub fn new_fragmented() -> Self {
        Self::with_fragmented(true)
    }
    fn with_fragmented(fragmented: bool) -> Self {
        let queues = [Mailbox::default(), Mailbox::default()];
        for queue in &queues {
            queue.borrow_mut().fragmented = fragmented;
        }
        let registry = ClientRegistry::new();
        let canceled = Rc::new(Cell::new(0));
        let server: node::Client = registry.new_client(Service {
            registry: registry.clone(),
            depth: 0,
            canceled: canceled.clone(),
        });
        let mut client_system = RpcSystem::new(
            Box::new(Network {
                link: Link {
                    input: queues[0].clone(),
                    output: queues[1].clone(),
                    peer: true,
                },
                accept: false,
            }),
            None,
        );
        let server_system = RpcSystem::new(
            Box::new(Network {
                link: Link {
                    input: queues[1].clone(),
                    output: queues[0].clone(),
                    peer: false,
                },
                accept: true,
            }),
            Some(capnp::capability::Client::new(server.into_client_hook())),
        );
        let client = client_system.bootstrap(true);
        let ready = [Ready::new(), Ready::new()];
        let wakes = [Waker::from(ready[0].clone()), Waker::from(ready[1].clone())];
        Self {
            ready,
            wakes,
            client_system,
            server_system,
            client,
            registry,
            canceled,
            queues,
        }
    }
    /// Poll a system only after its own wake notification. This fixture must not
    /// hide a lost wake by repeatedly polling a suspended system.
    pub fn turn(&mut self) -> bool {
        let mut polled = false;
        if self.ready[0].take() {
            let mut cx = Context::from_waker(&self.wakes[0]);
            assert!(Pin::new(&mut self.client_system).poll(&mut cx).is_pending());
            polled = true;
        }
        if self.ready[1].take() {
            let mut cx = Context::from_waker(&self.wakes[1]);
            assert!(Pin::new(&mut self.server_system).poll(&mut cx).is_pending());
            polled = true;
        }
        polled
    }
    pub fn settle(&mut self) {
        for _ in 0..20_000 {
            if !self.turn() {
                return;
            }
        }
        panic!("legacy systems did not become idle");
    }
    pub fn drive<F: Future>(&mut self, future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let ready = Ready::new();
        let wake = Waker::from(ready.clone());
        let mut cx = Context::from_waker(&wake);
        for _ in 0..20_000 {
            let mut polled = false;
            if ready.take() {
                polled = true;
                if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                    return result;
                }
            }
            polled |= self.turn();
            assert!(polled, "legacy conversation stalled without a wake");
        }
        panic!("legacy conversation exceeded turn bound");
    }
    pub fn canceled(&self) -> usize {
        self.canceled.get()
    }
    pub fn callback_target(&self, depth: u64) -> node::Client {
        self.registry.new_client(Service {
            registry: self.registry.clone(),
            depth,
            canceled: self.canceled.clone(),
        })
    }
    pub fn messages(&self) -> usize {
        self.queues.iter().map(|queue| queue.borrow().sent).sum()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    #[test]
    fn report_local_client_allocations() {
        let fixture = Fixture::new();
        // Warm and retain outer registry map capacity before tracing clients.
        let warm: [_; 32] = core::array::from_fn(|index| fixture.callback_target(index as u64));
        drop(warm);
        let samples: [_; 5] = core::array::from_fn(|_| {
            let mut clients: [Option<node::Client>; 32] = core::array::from_fn(|_| None);
            let allocations = allocation_counter::measure(|| {
                for (index, slot) in clients.iter_mut().enumerate() {
                    *slot = Some(fixture.callback_target(index as u64));
                }
            });
            assert!(clients.iter().all(Option::is_some));
            allocations
        });
        std::println!(
            r#"CLIENT_ALLOCATION_METRICS {{"blocks_samples":{:?},"bytes_samples":{:?},"pointer_bytes":{}}}"#,
            samples.each_ref().map(|sample| sample.count_total),
            samples.each_ref().map(|sample| sample.bytes_total),
            core::mem::size_of::<usize>()
        );
    }
    #[test]
    fn unary_and_retained_response() {
        let mut f = Fixture::new();
        let mut first = f.client.read_request();
        first.get().set_value(41);
        let retained = f.drive(first.send().promise).unwrap();
        for value in 0..16 {
            let mut r = f.client.read_request();
            r.get().set_value(value);
            assert_eq!(
                f.drive(r.send().promise)
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                value
            );
        }
        assert_eq!(retained.get().unwrap().get_value(), 41);
        assert!(f.messages() > 32);
    }
    #[test]
    fn deep_dependent_and_pipeline_with_concurrency() {
        let mut f = Fixture::new();
        let mut current = f.client.clone();
        for _ in 0..32 {
            current = f
                .drive(current.next_request().send().promise)
                .unwrap()
                .get()
                .unwrap()
                .get_child()
                .unwrap();
        }
        let mut calls = Vec::new();
        for value in 0..32 {
            let mut r = current.read_request();
            r.get().set_value(value);
            calls.push((value, r.send().promise));
        }
        for (value, promise) in calls {
            assert_eq!(
                f.drive(promise).unwrap().get().unwrap().get_value(),
                value + 32
            );
        }
        let mut pipelined = f.client.clone();
        let mut owners = Vec::new();
        for _ in 0..32 {
            let call = pipelined.next_request().send();
            pipelined = call.pipeline.get_child();
            owners.push(call.promise);
        }
        let mut read = pipelined.read_request();
        read.get().set_value(10);
        assert_eq!(
            f.drive(read.send().promise)
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            42
        );
        drop(owners);
    }
    #[test]
    fn callback_over_same_connection() {
        let mut f = Fixture::new();
        let target = f.callback_target(7);
        let mut r = f.client.callback_request();
        r.get().set_target(target);
        r.get().set_value(35);
        assert_eq!(
            f.drive(r.send().promise)
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            42
        );
    }
    #[test]
    fn cancellation_retires_suspended_method_and_connection_stays_live() {
        let mut f = Fixture::new();
        let pending = f.client.hold_request().send();
        f.settle();
        assert_eq!(f.canceled(), 0);
        drop(pending);
        f.settle();
        assert_eq!(f.canceled(), 1);
        let mut r = f.client.read_request();
        r.get().set_value(42);
        assert_eq!(
            f.drive(r.send().promise)
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            42
        );
    }
    #[test]
    fn fragmented_frames_preserve_pipeline_callback_results_and_cancellation() {
        let mut f = Fixture::new_fragmented();
        let mut read = f.client.read_request();
        read.get().set_value(42);
        let retained = f.drive(read.send().promise).unwrap();
        let mut child = f.client.clone();
        let mut owners = Vec::new();
        for _ in 0..32 {
            let next = child.next_request().send();
            child = next.pipeline.get_child();
            owners.push(next.promise);
        }
        let mut calls = Vec::new();
        for value in 0..32 {
            let mut r = child.read_request();
            r.get().set_value(value);
            calls.push((value, r.send().promise));
        }
        for (value, promise) in calls {
            assert_eq!(
                f.drive(promise).unwrap().get().unwrap().get_value(),
                value + 32
            );
        }
        let target = f.callback_target(7);
        let mut callback = f.client.callback_request();
        callback.get().set_target(target);
        callback.get().set_value(35);
        assert_eq!(
            f.drive(callback.send().promise)
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            42
        );
        let pending = f.client.hold_request().send();
        f.settle();
        assert_eq!(f.canceled(), 0);
        drop(pending);
        f.settle();
        assert_eq!(f.canceled(), 1);
        assert_eq!(retained.get().unwrap().get_value(), 42);
        drop(owners);
    }
    #[test]
    fn fragmented_capacity_failure_does_not_expose_partial_message() {
        let mut f = Fixture::new_fragmented();
        let pending = f.client.read_request().send();
        // A 256-word segment plus its table exceeds the 256-word frame buffer.
        let wake = {
            let mut input = f.queues[0].borrow_mut();
            input.bytes.extend([0, 0, 0, 0, 0, 1, 0, 0]);
            input.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        let error = match f.drive(pending.promise) {
            Ok(_) => panic!("oversized frame accepted"),
            Err(error) => error,
        };
        // RPC normalizes transport failures to Disconnected; the transport
        // retains the underlying framing cause for an exact assertion.
        assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
        assert_eq!(
            f.queues[0].borrow().error,
            Some(capnp::ErrorKind::BufferNotLargeEnough)
        );
    }
    #[test]
    fn fragmented_truncated_frame_fails_pending_calls() {
        let mut f = Fixture::new_fragmented();
        let pending = f.client.read_request().send();
        // A partial segment header followed by EOF must not expose a message.
        let wake = {
            let mut input = f.queues[0].borrow_mut();
            input.bytes.extend([0u8; 3]);
            input.closed = true;
            input.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        assert!(f.drive(pending.promise).is_err());
    }
    #[test]
    fn identity_and_revocation_use_explicit_domain() {
        let registry = ClientRegistry::new();
        let drops = Rc::new(Cell::new(0));
        let server = Rc::new(Service {
            registry: registry.clone(),
            depth: 0,
            canceled: drops,
        });
        let a: node::Client = registry.new_client_from_rc(server.clone());
        let b: node::Client = registry.new_client_from_rc(server.clone());
        assert_eq!(a.as_client_hook().get_ptr(), b.as_client_hook().get_ptr());
        let revocable =
            capnp_rpc::RevocableServer::<node::Client>::new_in(&registry, server.clone());
        // A live ordinary alias prevents constructing a bypassable revocable view.
        assert!(revocable.get_client().as_client_hook().get_ptr() != a.as_client_hook().get_ptr());
        drop(revocable);
        drop(a);
        drop(b);
        let owner = capnp_rpc::RevocableServer::<node::Client>::new_in(&registry, server.clone());
        let alias: node::Client = registry.new_client_from_rc(server);
        let mut f = Fixture::new();
        let error = match f.drive(alias.read_request().send().promise) {
            Ok(_) => panic!("revocable alias was admitted"),
            Err(error) => error,
        };
        assert!(error.extra.contains("cannot create another client"));
        owner.revoke();
        assert!(f
            .drive(owner.get_client().read_request().send().promise)
            .is_err());
    }
}

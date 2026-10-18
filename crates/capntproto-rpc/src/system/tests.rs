//! Exercise the public RpcSystem API with caller-controlled transport callbacks.
use super::*;
use capnp::{
    any_pointer,
    capability::{Client, Promise},
    message,
};
use futures::FutureExt;
use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Wake, Waker},
};

struct Transport {
    initialize: RefCell<Option<Box<dyn FnOnce()>>>,
    initializations: Cell<usize>,
    shutdowns: Cell<usize>,
    closed: Shared<Promise<(), Error>>,
}
impl Transport {
    fn system() -> (
        Rc<Self>,
        crate::RpcSystem<()>,
        shared::Sender<capnp::Result<()>>,
    ) {
        let (sender, receiver) = shared::channel();
        let transport = Rc::new(Self {
            initialize: RefCell::new(None),
            initializations: Cell::new(0),
            shutdowns: Cell::new(0),
            closed: Promise::from_future(async move { receiver.await.unwrap() }).shared(),
        });
        let system = crate::RpcSystem::new(Box::new(Network(transport.clone())), None);
        (transport, system, sender)
    }
}
struct Network(Rc<Transport>);
impl crate::VatNetwork<()> for Network {
    fn connect(&mut self, _: ()) -> Option<Box<dyn Connection<()>>> {
        Some(Box::new(Peer(self.0.clone())))
    }
    fn accept(&mut self) -> Promise<Box<dyn Connection<()>>, Error> {
        Promise::from_future(futures::future::pending())
    }
    fn take_output_driver(&mut self) -> Option<capnp::capability::Promise<(), capnp::Error>> {
        None
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        Promise::from_future(futures::future::pending())
    }
}
struct Peer(Rc<Transport>);
impl Connection<()> for Peer {
    fn get_peer_vat_id(&self) {}
    fn connection_id(&self) -> usize {
        1
    }
    fn new_outgoing_message(&mut self, _: u32) -> Box<dyn crate::OutgoingMessage> {
        Box::new(Message(message::Builder::new_default()))
    }
    fn receive_incoming_message(
        &mut self,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage>>, Error> {
        Promise::from_future(futures::future::pending())
    }
    fn when_write_finished(&self) -> Option<Promise<(), Error>> {
        self.0.initializations.set(self.0.initializations.get() + 1);
        let callback = self.0.initialize.borrow_mut().take();
        if let Some(callback) = callback {
            callback();
        }
        None
    }
    fn shutdown(&mut self, _: capnp::Result<()>) -> Promise<(), Error> {
        self.0.shutdowns.set(self.0.shutdowns.get() + 1);
        Promise::from_future(self.0.closed.clone())
    }
}
struct Message(message::Builder<message::HeapAllocator>);
impl crate::OutgoingMessage for Message {
    fn get_body(&mut self) -> capnp::Result<any_pointer::Builder<'_>> {
        self.0.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.0.get_root_as_reader()
    }
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<message::Builder<message::HeapAllocator>>,
    ) {
        (Promise::ok(()), Rc::new(self.0))
    }
    fn take(self: Box<Self>) -> message::Builder<message::HeapAllocator> {
        self.0
    }
    fn size_in_words(&self) -> usize {
        self.0.size_in_words()
    }
}
fn poll<T>(future: &mut (impl Future<Output = T> + Unpin)) -> Poll<T> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}
fn broken_kind(client: &Client) -> capnp::ErrorKind {
    client
        .hook
        .as_any()
        .unwrap()
        .downcast_ref::<crate::broken::Client>()
        .unwrap()
        .error()
        .kind
}
#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn concurrent_disconnectors_wake_after_real_transport_shutdown() {
    let (transport, mut system, shutdown) = Transport::system();
    let _client: Client = system.bootstrap(());
    let mut first = system.get_disconnector();
    let mut second = system.get_disconnector();
    let first_wakes = Arc::new(Wakes::default());
    let second_wakes = Arc::new(Wakes::default());
    let fw = Waker::from(first_wakes.clone());
    let sw = Waker::from(second_wakes.clone());
    assert!(Pin::new(&mut first)
        .poll(&mut Context::from_waker(&fw))
        .is_pending());
    assert!(Pin::new(&mut second)
        .poll(&mut Context::from_waker(&sw))
        .is_pending());
    assert_eq!(transport.shutdowns.get(), 1);
    assert!(system.diagnostics().snapshot().is_empty());
    assert!(poll(&mut system).is_pending());
    let before_first = first_wakes.0.load(Ordering::Relaxed);
    let before_second = second_wakes.0.load(Ordering::Relaxed);
    shutdown.send(Ok(())).unwrap();
    assert!(poll(&mut system).is_pending());
    assert!(first_wakes.0.load(Ordering::Relaxed) > before_first);
    assert!(second_wakes.0.load(Ordering::Relaxed) > before_second);
    assert!(matches!(poll(&mut first), Poll::Ready(Ok(()))));
    assert!(matches!(poll(&mut second), Poll::Ready(Ok(()))));
}

#[test]
fn dropping_unpolled_system_finishes_disconnector_with_cancellation() {
    let (transport, mut system, _shutdown) = Transport::system();
    let _client: Client = system.bootstrap(());
    let mut disconnector = system.get_disconnector();
    drop(system);
    assert_eq!(transport.shutdowns.get(), 1);
    assert!(matches!(poll(&mut disconnector), Poll::Ready(Err(error))
        if error.kind == capnp::ErrorKind::Disconnected));
}

#[test]
fn dropping_started_system_wakes_pending_shutdown_observer() {
    let (transport, mut system, _shutdown) = Transport::system();
    let _client: Client = system.bootstrap(());
    assert!(poll(&mut system).is_pending());
    let mut disconnector = system.get_disconnector();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(Pin::new(&mut disconnector)
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert!(poll(&mut system).is_pending());
    let before = wakes.0.load(Ordering::Relaxed);
    drop(system);
    assert_eq!(transport.shutdowns.get(), 1);
    assert!(wakes.0.load(Ordering::Relaxed) > before);
    assert!(matches!(poll(&mut disconnector), Poll::Ready(Err(error))
        if error.kind == capnp::ErrorKind::Disconnected));
}

#[test]
fn reentrant_bootstrap_cannot_register_a_second_driver() {
    let (transport, mut system, _shutdown) = Transport::system();
    let bootstrap = system.get_bootstrapper();
    *transport.initialize.borrow_mut() = Some(Box::new(move || {
        let client: Client = bootstrap.bootstrap(());
        assert_eq!(broken_kind(&client), capnp::ErrorKind::Overloaded);
    }));
    let _first: Client = system.bootstrap(());
    let _second: Client = system.bootstrap(());
    assert_eq!(transport.initializations.get(), 1);
    assert_eq!(system.diagnostics().snapshot().len(), 1);
}

#[test]
fn shutdown_during_construction_waits_for_reserved_connection() {
    let (transport, mut system, shutdown) = Transport::system();
    let mut disconnector = system.get_disconnector();
    let mut during_construction = system.get_disconnector();
    *transport.initialize.borrow_mut() = Some(Box::new(move || {
        assert!(poll(&mut during_construction).is_pending());
    }));
    let client: Client = system.bootstrap(());
    assert_eq!(broken_kind(&client), capnp::ErrorKind::Disconnected);
    assert_eq!(transport.shutdowns.get(), 1);
    assert!(system.diagnostics().snapshot().is_empty());
    assert!(poll(&mut system).is_pending());
    assert!(poll(&mut disconnector).is_pending());
    shutdown.send(Ok(())).unwrap();
    assert!(poll(&mut system).is_pending());
    assert!(matches!(poll(&mut disconnector), Poll::Ready(Ok(()))));
}

#[test]
fn transport_close_failure_reaches_every_disconnector() {
    let (_transport, mut system, shutdown) = Transport::system();
    let _client: Client = system.bootstrap(());
    let mut first = system.get_disconnector();
    let mut second = system.get_disconnector();
    assert!(poll(&mut first).is_pending());
    shutdown
        .send(Err(Error::failed("flush failed".into())))
        .unwrap();
    assert!(poll(&mut system).is_pending());
    for observer in [&mut first, &mut second] {
        assert!(matches!(poll(observer), Poll::Ready(Err(error)) if error.extra == "flush failed"));
    }
}

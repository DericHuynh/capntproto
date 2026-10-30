//! The portable engine and the default hosted driver share the production TCP
//! VatNetwork and interoperate without adapting application ClientHooks.
use capnp::{
    any_pointer,
    capability::{self, Client, Promise},
    private::capability::ClientHook,
};
use capnp_rpc::{
    bounded::owned::connection,
    engine::{connection as storage, messages},
    rpc_twoparty_capnp::Side,
    scoped::{
        wire::{
            coordinator::{network::Network, transport::vat, Coordinator, Options, TwoParty},
            resources::Import,
            server::Producer,
        },
        ClientHooks,
    },
    VatNetwork,
};
use futures::{channel::oneshot, FutureExt};
use std::{cell::Cell, num::NonZeroUsize, rc::Rc, time::Duration};
#[path = "scoped_vat/policy.rs"]
mod policy;

#[derive(Clone)]
struct Echo(Rc<Cell<usize>>, bool);
impl capability::Server for Echo {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        let (params, mut results) = context.into_owned();
        self.0.set(self.0.get() + 1);
        capability::DispatchCallResult {
            promise: Promise::from(params.get().and_then(|params| results.get().set_as(params))),
            is_streaming: false,
            allow_cancellation: !self.1,
        }
    }
}
fn echo(calls: Rc<Cell<usize>>) -> Client {
    let client: EchoClient = capnp_rpc::new_client(Echo(calls, false));
    client.0
}
struct EchoClient(Client);
impl capability::FromClientHook for EchoClient {
    fn new(hook: Box<dyn ClientHook>) -> Self {
        Self(Client::new(hook))
    }
    fn into_client_hook(self) -> Box<dyn ClientHook> {
        self.0.hook
    }
    fn as_client_hook(&self) -> &dyn ClientHook {
        &*self.0.hook
    }
}
#[derive(Clone)]
struct Dispatch(Rc<Echo>);
impl std::ops::Deref for Dispatch {
    type Target = Echo;
    fn deref(&self) -> &Echo {
        &self.0
    }
}
impl capability::FromServer<Echo> for EchoClient {
    type Dispatch = Dispatch;
    fn from_server(server: Rc<Echo>) -> Dispatch {
        Dispatch(server)
    }
}
impl capability::Server for Dispatch {
    fn as_ptr(&self) -> usize {
        // Distinct service instances share a counter, not capability identity.
        Rc::as_ptr(&self.0) as usize
    }
    fn dispatch_call(
        self,
        interface: u64,
        method: u16,
        context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        (*self.0).clone().dispatch_call(interface, method, context)
    }
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Legacy,
    Scoped,
    Owned,
}
async fn peer(
    mut transport: Box<dyn VatNetwork<Side>>,
    side: Side,
    mode: Mode,
    bootstrap: Client,
    ready: oneshot::Sender<Client>,
    stop: oneshot::Receiver<()>,
) {
    let other = if side == Side::Client {
        Side::Server
    } else {
        Side::Client
    };
    if matches!(mode, Mode::Legacy) {
        let mut system = capnp_rpc::RpcSystem::new(transport, Some(bootstrap));
        let root = system.bootstrap::<Client>(other);
        assert!(ready.send(root).is_ok());
        let closed = system.get_disconnector();
        let application = async move {
            stop.await.unwrap();
            closed.await.unwrap();
        };
        let (result, ()) = futures::join!(system, application);
        result.unwrap();
        return;
    }
    if matches!(mode, Mode::Owned) {
        let mut options = capnp_rpc::portable::Options::bounded(
            NonZeroUsize::new(1).unwrap(),
            NonZeroUsize::new(64).unwrap(),
            NonZeroUsize::new(128 << 10).unwrap(),
            NonZeroUsize::new(8 << 20).unwrap(),
        )
        .unwrap();
        options.concurrency = NonZeroUsize::new(4).unwrap();
        options.output_queue = NonZeroUsize::new(1).unwrap();
        options.poll_budget = NonZeroUsize::new(3).unwrap();
        let system =
            capnp_rpc::portable::RpcSystem::new(transport, Some(bootstrap), options).unwrap();
        let remote = system.get_bootstrapper();
        // The initial Bootstrap and its calls are queued before the task is polled.
        assert!(ready.send(remote.bootstrap(other)).is_ok());
        let closed = system.get_disconnector();
        let application = async move {
            stop.await.unwrap();
            closed.await.unwrap();
        };
        let (result, ()) = futures::join!(system.run(), application);
        result.unwrap();
        let client: Client = remote.bootstrap(other);
        assert!(client.when_resolved().await.is_err());
        return;
    }
    // Exercise both paths by accepting on the server and dialing on the client.
    let connection = if side == Side::Server {
        transport.accept().await.unwrap()
    } else {
        transport.connect(other).unwrap()
    };
    let driver = transport.drive_until_shutdown();
    // The driver and the adapted connection own IO; retaining the initial
    // network's unused accept handle would keep twoparty shutdown waiting.
    drop(transport);
    let mut buffers = messages::GrowingBuffers::new(64, 256 << 10, 8 << 20);
    messages::with_growing_messages_async(
        &mut buffers,
        storage::Growth {
            initial: 0,
            maximum: 8,
        },
        async |messages| {
            storage::with_domain_async(async |domain| {
                let bound = storage::Growth {
                    initial: 0,
                    maximum: 64,
                };
                let graph = domain
                    .bind_growing_adoptions(
                        storage::GrowingStorage {
                            notifications:
                                capntproto_rpc_engine::clients::NotificationGrowth::for_clients(
                                    bound,
                                ),
                            imports: bound,
                            exports: bound,
                            answers: bound,
                            questions: bound,
                            pipeline_questions: bound,
                            clients: bound,
                        },
                        storage::Growth {
                            initial: 0,
                            maximum: 256,
                        },
                    )
                    .unwrap();
                let driver_state: connection::Driver<
                    '_,
                    '_,
                    '_,
                    Import,
                    Producer,
                    u8,
                    Rc<dyn ClientHook>,
                    8,
                    8,
                > = connection::Driver::new_growing(
                    graph,
                    connection::GrowingStorage::uniform(0, 64),
                    1,
                )
                .unwrap_or_else(|_| panic!("driver storage"));
                let coordinator = Coordinator::new(
                    driver_state,
                    ClientHooks::new(64, 16),
                    NonZeroUsize::new(4).unwrap(),
                    None,
                    Options {
                        input_bytes: NonZeroUsize::new(128 << 10).unwrap(),
                        output_bytes: NonZeroUsize::new(512).unwrap(),
                        budget: NonZeroUsize::new(3).unwrap(),
                        reader: Default::default(),
                    },
                )
                .unwrap();
                let network = Network::new();
                let coordinator = coordinator.register_vat(&network, connection).unwrap();
                assert_eq!(*coordinator.peer(), other);
                assert!(ready.send(coordinator.handle().bootstrap()).is_ok());
                let mut controls =
                    TwoParty::new(|| Ok(bootstrap.clone()), NonZeroUsize::new(4).unwrap());
                let mut queue =
                    messages::Queue::<vat::Bundle>::growing_with_resources(0, 1).unwrap();
                let mut scratch = [0; 64];
                let policy = vat::Resources::default();
                let run = coordinator.run(
                    messages,
                    &mut queue,
                    &mut scratch,
                    &policy,
                    &mut controls,
                    async {
                        stop.await.unwrap();
                        Ok(())
                    },
                    async {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        capnp::Error::failed("portable TCP deadline".into())
                    },
                );
                let (run, driver) = futures::join!(run, driver);
                run.unwrap();
                // Native shutdown carries the disconnect reason, as legacy
                // does. The raw network task exposes that queue result; its
                // managed RPC owner normalizes Disconnected after local close.
                if let Err(error) = driver {
                    assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
                }
                network.get_disconnector().await.unwrap();
                assert!(queue.is_empty());
                let reserved: Vec<_> = (0..64).map(|_| messages.reserve(1024).unwrap()).collect();
                for writer in reserved {
                    messages.discard(writer);
                }
            })
            .await;
        },
    )
    .await
    .unwrap();
}

async fn payload(client: &Client, byte: u8) {
    let bytes = vec![byte; 64 << 10];
    let mut request = client.new_call::<any_pointer::Owned, any_pointer::Owned>(123, 0, None);
    request
        .get()
        .set_as::<capnp::data::Owned>(&bytes[..])
        .unwrap();
    let result = request
        .send()
        .promise
        .await
        .unwrap_or_else(|error| panic!("payload {byte}: {error}"));
    assert_eq!(
        result
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap(),
        &bytes
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_vat_adapter_roundtrips_large_payloads_and_promise_capabilities_with_default_driver() {
    tokio::time::timeout(Duration::from_secs(30), async {
        use Mode::{Legacy, Owned, Scoped};
        for (a_portable, b_portable) in [
            (Scoped, Scoped),
            (Scoped, Legacy),
            (Legacy, Scoped),
            (Owned, Owned),
            (Owned, Legacy),
            (Legacy, Owned),
            (Owned, Scoped),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let (client, accepted) = tokio::join!(
                tokio::net::TcpStream::connect(listener.local_addr().unwrap()),
                listener.accept(),
            );
            let client = client.unwrap();
            let server = accepted.unwrap().0;
            client.set_nodelay(true).unwrap();
            server.set_nodelay(true).unwrap();
            let a = capntproto::rpc::tcp::network(client, Side::Client, Default::default());
            let b = capntproto::rpc::tcp::network(server, Side::Server, Default::default());
            let calls = Rc::new(Cell::new(0));
            let (atx, arx) = oneshot::channel();
            let (btx, brx) = oneshot::channel();
            let (astop, astopped) = oneshot::channel();
            let (bstop, bstopped) = oneshot::channel();
            let application = async {
                let a = arx.await.unwrap();
                let b = brx.await.unwrap();
                // Both directions, before waiting separately for Bootstrap.
                futures::join!(payload(&a, 3), payload(&b, 9));
                let (resolve, pending) = oneshot::channel();
                let promised: Client = capnp_rpc::new_future_client(async {
                    pending
                        .await
                        .map_err(|_| capnp::Error::disconnected("test canceled".into()))
                });
                let mut request =
                    a.new_call::<any_pointer::Owned, any_pointer::Owned>(123, 0, None);
                request.get().set_as_capability(promised.hook.add_ref());
                let response = request.send();
                let pipelined = Client::new(response.pipeline.hook.get_pipelined_cap(&[]));
                // Send a nested call through the unresolved result before Resolve.
                let nested = payload(&pipelined, 27);
                futures::pin_mut!(nested);
                assert!(nested.as_mut().now_or_never().is_none());
                assert!(resolve.send(echo(calls.clone())).is_ok());
                nested.await;
                let reflected = response
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as_capability::<Client>()
                    .unwrap();
                payload(&reflected, 42).await;
                astop.send(()).unwrap();
                bstop.send(()).unwrap();
            };
            futures::join!(
                Box::pin(peer(
                    Box::new(a),
                    Side::Client,
                    a_portable,
                    if matches!(a_portable, Owned) {
                        capnp_rpc::new_client::<EchoClient, _>(Echo(calls.clone(), true)).0
                    } else {
                        echo(calls.clone())
                    },
                    atx,
                    astopped
                )),
                Box::pin(peer(
                    Box::new(b),
                    Side::Server,
                    b_portable,
                    if matches!(b_portable, Owned) {
                        capnp_rpc::new_client::<EchoClient, _>(Echo(calls.clone(), true)).0
                    } else {
                        echo(calls.clone())
                    },
                    btx,
                    bstopped
                )),
                application,
            );
            assert_eq!(calls.get(), 5);
        }
    })
    .await
    .expect("adapted TCP RPC must finish without a shutdown or pipeline stall");
}

#[tokio::test(flavor = "current_thread")]
async fn portable_rpcsystem_factory_uses_peer_identity_and_retries_after_bootstrap_exception() {
    use std::cell::RefCell;
    struct Factory {
        seen: Rc<RefCell<Vec<Side>>>,
        service: Client,
    }
    impl capnp_rpc::BootstrapFactory<Side> for Factory {
        fn create_for(&self, peer: &Side) -> capnp::Result<Client> {
            let mut seen = self.seen.borrow_mut();
            seen.push(*peer);
            if seen.len() == 1 {
                Err(capnp::Error::failed(
                    "first request denied by policy".into(),
                ))
            } else {
                Ok(self.service.clone())
            }
        }
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, server) = tokio::join!(
            tokio::net::TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept(),
        );
        let a = capntproto::rpc::tcp::network(client.unwrap(), Side::Client, Default::default());
        let b = capntproto::rpc::tcp::network(server.unwrap().0, Side::Server, Default::default());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let calls = Rc::new(Cell::new(0));
        let options = capnp_rpc::portable::Options::bounded(
            NonZeroUsize::new(1).unwrap(),
            NonZeroUsize::new(32).unwrap(),
            NonZeroUsize::new(128 << 10).unwrap(),
            NonZeroUsize::new(8 << 20).unwrap(),
        )
        .unwrap();
        let server = capnp_rpc::portable::RpcSystem::new_with_bootstrap_factory(
            Box::new(b),
            Side::Server,
            Rc::new(Factory {
                seen: seen.clone(),
                service: echo(calls.clone()),
            }),
            options,
        )
        .unwrap();
        let client = capnp_rpc::RpcSystem::new(Box::new(a), None);
        let bootstrap = client.get_bootstrapper();
        let client_closed = client.get_disconnector();
        let server_closed = server.get_disconnector();
        let application = async {
            let first: Client = bootstrap.bootstrap(Side::Server);
            let error = first.when_resolved().await.unwrap_err();
            assert!(error.extra.contains("first request denied by policy"));
            let second: Client = bootstrap.bootstrap(Side::Server);
            payload(&second, 55).await;
            assert_eq!(&*seen.borrow(), &[Side::Client, Side::Client]);
            assert_eq!(calls.get(), 1);
            futures::future::try_join(client_closed, server_closed)
                .await
                .unwrap();
        };
        let (client, server, ()) = futures::join!(client, server.run(), application);
        client.unwrap();
        server.unwrap();
    })
    .await
    .unwrap();
}

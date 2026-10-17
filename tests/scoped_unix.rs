#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Exercise the migrated coordinator against real ancillary socket IO.
use capnp::capability::Client;
use capnp_rpc::bounded as capntproto_rpc_wire;
use capnp_rpc::scoped::{
    wire::{
        coordinator::{
            network::Network,
            transport::{vat, FileDescriptors},
            Coordinator, Handle, Options, TwoParty,
        },
        resources::Import,
        server::Results,
    },
    ClientHooks,
};
use capntproto::unix_rpc::{ResourceReader, ResourceWriter};
use capntproto_rpc_engine::{connection as storage, messages};
use capntproto_rpc_wire::{attachments::Sender, owned::connection};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::channel::oneshot;
use std::{
    cell::Cell,
    io::Write,
    num::NonZeroUsize,
    os::{fd::OwnedFd, unix::fs::FileExt},
    rc::Rc,
};
use tokio::net::UnixStream;

#[derive(Clone, Copy)]
enum Mode {
    Direct,
    Adapted,
    Owned,
}
enum Bootstrap {
    Scoped(Handle),
    Owned(capnp_rpc::portable::Bootstrapper<capnp_rpc::rpc_twoparty_capnp::Side>),
}
impl Bootstrap {
    fn bootstrap<T: capnp::capability::FromClientHook>(&self) -> T {
        match self {
            Self::Scoped(handle) => handle.bootstrap(),
            Self::Owned(handle) => handle.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server),
        }
    }
}

fn fd(value: u8) -> OwnedFd {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&[value]).unwrap();
    file.into()
}
fn value(fd: Option<Rc<OwnedFd>>) -> u8 {
    fd.map(|fd| {
        let file = std::fs::File::from(fd.try_clone().unwrap());
        let mut byte = [0];
        file.read_exact_at(&mut byte, 0).unwrap();
        byte[0]
    })
    .unwrap_or(0)
}
struct Service {
    expected: u8,
    seen: Rc<Cell<usize>>,
    returned: harness::Client,
}
impl harness::Server for Service {
    fn bounce(
        self: Rc<Self>,
        context: harness::BounceContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let cap = params.get()?.get_cap()?;
            // Immediate inspection catches a missing metadata handoff; waiting for
            // a later coordinator turn would conceal the first-call regression.
            assert_eq!(value(cap.client.hook.get_fd()), self.expected);
            self.seen.set(self.seen.get() + 1);
            results.get().set_cap(self.returned.clone());
            Ok(())
        })
    }
}
struct Echo;
impl harness::Server for Echo {
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            results.get().set_value(params.get()?.get_value());
            Ok(())
        })
    }
}
async fn peer(
    socket: UnixStream,
    bootstrap: Client,
    handle: oneshot::Sender<Bootstrap>,
    stop: oneshot::Receiver<()>,
    limit: usize,
    mode: Mode,
) {
    if matches!(mode, Mode::Owned) {
        let transport = capntproto::unix_rpc::VatNetwork::new(
            socket,
            capnp_rpc::rpc_twoparty_capnp::Side::Client,
            capntproto::unix_rpc::Options {
                max_fds: limit,
                ..Default::default()
            },
        );
        let mut options = capnp_rpc::portable::Options::bounded(
            NonZeroUsize::new(1).unwrap(),
            NonZeroUsize::new(32).unwrap(),
            NonZeroUsize::new(4096).unwrap(),
            NonZeroUsize::new(32 * 4103).unwrap(),
        )
        .unwrap();
        options.concurrency = NonZeroUsize::new(2).unwrap();
        options.output_queue = NonZeroUsize::new(1).unwrap();
        options.poll_budget = NonZeroUsize::new(3).unwrap();
        options.maximum_fds = limit;
        let system =
            capnp_rpc::portable::RpcSystem::new(Box::new(transport), Some(bootstrap), options)
                .unwrap();
        assert!(handle
            .send(Bootstrap::Owned(system.get_bootstrapper()))
            .is_ok());
        let closed = system.get_disconnector();
        let application = async {
            stop.await.unwrap();
            closed.await.unwrap();
        };
        let (result, ()) = futures::join!(system, application);
        result.unwrap();
        return;
    }
    let mut buffers = messages::GrowingBuffers::new(32, 4096, 32 * 4103);
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
                    maximum: 32,
                };
                let graph = domain
                    .bind_growing_adoptions(
                        storage::GrowingStorage {
                            imports: bound,
                            exports: bound,
                            answers: bound,
                            questions: bound,
                            clients: bound,
                        },
                        storage::Growth {
                            initial: 0,
                            maximum: 128,
                        },
                    )
                    .unwrap();
                let driver: connection::Driver<
                    '_,
                    '_,
                    '_,
                    Import,
                    Results,
                    u8,
                    Rc<dyn capnp::private::capability::ClientHook>,
                    8,
                    8,
                > = connection::Driver::new_growing(
                    graph,
                    connection::GrowingStorage::uniform(0, 32),
                    1,
                )
                .unwrap_or_else(|_| panic!("driver storage"));
                let coordinator = Coordinator::new(
                    driver,
                    ClientHooks::new(32, 16),
                    NonZeroUsize::new(2).unwrap(),
                    None,
                    Options {
                        input_bytes: NonZeroUsize::new(4096).unwrap(),
                        output_bytes: NonZeroUsize::new(512).unwrap(),
                        budget: NonZeroUsize::new(3).unwrap(),
                        reader: Default::default(),
                    },
                )
                .unwrap();
                let network = Network::new();
                let mut controls =
                    TwoParty::new(|| Ok(bootstrap.clone()), NonZeroUsize::new(2).unwrap());
                // One output slot forces controls, Calls, Returns and Resolves
                // to compete for the same FIFO rather than bypassing resources.
                let mut queue =
                    messages::Queue::<Sender<Rc<OwnedFd>, 4>>::growing_with_resources(0, 1)
                        .unwrap();
                let shutdown = async {
                    stop.await.unwrap();
                    Ok(())
                };
                let abort = async {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    capnp::Error::failed("scoped Unix connection deadline".into())
                };
                if matches!(mode, Mode::Adapted) {
                    let mut queue =
                        messages::Queue::<vat::Bundle>::growing_with_resources(0, 1).unwrap();
                    use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork as _};
                    let mut transport = capntproto::unix_rpc::VatNetwork::new(
                        socket,
                        Side::Client,
                        capntproto::unix_rpc::Options {
                            max_fds: limit,
                            ..Default::default()
                        },
                    );
                    let connection = transport.accept().await.unwrap();
                    let driver = transport.drive_until_shutdown();
                    drop(transport);
                    let coordinator = coordinator.register_vat(&network, connection).unwrap();
                    assert_eq!(*coordinator.peer(), Side::Server);
                    assert!(handle.send(Bootstrap::Scoped(coordinator.handle())).is_ok());
                    let policy = vat::Resources::new(limit);
                    let mut scratch = [0; 32];
                    let run = coordinator.run(
                        messages,
                        &mut queue,
                        &mut scratch,
                        &policy,
                        &mut controls,
                        shutdown,
                        abort,
                    );
                    let (run, driver) = futures::join!(run, driver);
                    run.unwrap();
                    driver.unwrap();
                    assert!(queue.is_empty());
                } else {
                    let coordinator = coordinator.register(&network, 1).unwrap();
                    assert!(handle
                        .send(Bootstrap::Scoped(network.connection(1).unwrap()))
                        .is_ok());
                    coordinator
                        .run_connection(
                            messages,
                            &mut queue,
                            &mut ResourceReader::new(&socket, limit, |_| ()),
                            &mut ResourceWriter::new(&socket),
                            &mut [0; 32],
                            &FileDescriptors::<4> {
                                capacity: capntproto_rpc_wire::Capacity::Growing { maximum: limit },
                            },
                            &mut controls,
                            shutdown,
                            abort,
                        )
                        .await
                        .unwrap();
                    assert!(network.connection(1).is_none());
                }
                network.get_disconnector().await.unwrap();
                assert!(queue.is_empty());
                let reserved: Vec<_> = (0..32).map(|_| messages.reserve(4096).unwrap()).collect();
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

#[tokio::test(flavor = "current_thread")]
async fn scoped_bootstrap_call_return_and_resolve_transfer_real_descriptors() {
    descriptors(Mode::Direct).await;
}

#[tokio::test(flavor = "current_thread")]
async fn vatnetwork_adapter_bootstrap_call_return_and_resolve_transfer_real_descriptors() {
    descriptors(Mode::Adapted).await;
}

#[tokio::test(flavor = "current_thread")]
async fn portable_rpcsystem_bootstrap_call_return_and_resolve_transfer_real_descriptors() {
    descriptors(Mode::Owned).await;
}

async fn descriptors(mode: Mode) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        for limit in [0, 4] {
            for promised in [false, true] {
                let (a, b) = UnixStream::pair().unwrap();
                let (ah, ar) = oneshot::channel();
                let (bh, br) = oneshot::channel();
                let (astop, astopped) = oneshot::channel();
                let (bstop, bstopped) = oneshot::channel();
                let seen = Rc::new(Cell::new(0));
                let (resolve, pending) = oneshot::channel();
                let mut resolve = Some(resolve);
                let returned: harness::Client = if promised {
                    capnp_rpc::new_future_client(async {
                        pending
                            .await
                            .map_err(|_| capnp::Error::disconnected("test canceled".into()))
                    })
                } else {
                    capnp_rpc::new_fd_client(Echo, fd(3))
                };
                let service: harness::Client = capnp_rpc::new_fd_client(
                    Service {
                        expected: if limit == 0 { 0 } else { 2 },
                        seen: seen.clone(),
                        returned,
                    },
                    fd(1),
                );
                let client: harness::Client = capnp_rpc::new_client(Echo);
                let application = async {
                    let handle: Bootstrap = ar.await.unwrap();
                    let _other = br.await.unwrap();
                    let root: harness::Client = handle.bootstrap();
                    let mut request = root.bounce_request();
                    request.get().set_cap(capnp_rpc::new_fd_client(Echo, fd(2)));
                    let response = request.send(); // Pipeline before Bootstrap returns.
                    root.client.when_resolved().await.unwrap();
                    assert_eq!(
                        value(root.client.hook.get_fd()),
                        if limit == 0 { 0 } else { 1 }
                    );
                    let child = response
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_cap()
                        .unwrap();
                    if promised {
                        assert!(child.client.hook.get_fd().is_none());
                        assert!(resolve
                            .take()
                            .unwrap()
                            .send(capnp_rpc::new_fd_client(Echo, fd(3)))
                            .is_ok());
                    }
                    assert_eq!(
                        value(child.client.get_fd().await.unwrap()),
                        if limit == 0 { 0 } else { 3 }
                    );
                    let mut echo = child.echo_request();
                    echo.get().set_value(73);
                    assert_eq!(
                        echo.send()
                            .promise
                            .await
                            .unwrap()
                            .get()
                            .unwrap()
                            .get_value(),
                        73
                    );
                    assert_eq!(seen.get(), 1);
                    astop.send(()).unwrap();
                    bstop.send(()).unwrap();
                };
                futures::join!(
                    peer(a, client.client, ah, astopped, limit, mode),
                    peer(b, service.client, bh, bstopped, limit, mode),
                    application,
                );
            }
        }
    })
    .await
    .expect("resource-aware coordinators must not stall");
}

use futures::FutureExt;
use reproto::{
    native_discovery::{
        Binding, Directory, Discovery, DiscoveryConnector, DiscoveryGeneration, DiscoveryOptions,
    },
    native_discovery_capnp::directory,
    native_provisioning_capnp::provisioner,
    transport::Identity,
};
use std::{cell::Cell, net::SocketAddr, rc::Rc, time::Duration};

fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
struct PendingProvider(Rc<Cell<usize>>);
impl provisioner::Server for PendingProvider {
    async fn reserve(
        self: Rc<Self>,
        _: provisioner::ReserveParams,
        _: provisioner::ReserveResults,
    ) -> capnp::Result<()> {
        self.0.set(self.0.get() + 1);
        std::future::pending().await
    }
}
struct Advertised {
    generation: u64,
    host: [u8; 32],
    recipient: [u8; 32],
    calls: Rc<Cell<usize>>,
    provider: provisioner::Client,
}
impl directory::Server for Advertised {
    async fn resolve(
        self: Rc<Self>,
        _: directory::ResolveParams,
        mut out: directory::ResolveResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        let mut r = out.get().init_record();
        r.set_host(&self.host);
        r.set_recipient(&self.recipient);
        r.set_address("127.0.0.1:9000");
        r.set_context(b"validated");
        r.set_generation(self.generation);
        r.set_remaining_millis(60_000);
        r.set_provider(self.provider.clone());
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn generation_wire_import_is_nonzero_full_width_and_authoritative() {
    tokio::task::LocalSet::new()
        .run_until(async {
            assert!(DiscoveryGeneration::new(0).is_none());
            for generation in [0, 1, u64::MAX] {
                let calls = Rc::new(Cell::new(0));
                let fallback = Rc::new(Cell::new(0));
                let provider_calls = Rc::new(Cell::new(0));
                let provider: provisioner::Client =
                    capnp_rpc::new_client(PendingProvider(provider_calls.clone()));
                let reader = |generation, calls| {
                    capnp_rpc::new_client(Advertised {
                        generation,
                        host: [1; 32],
                        recipient: [2; 32],
                        calls,
                        provider: provider.clone(),
                    })
                };
                let discovery = Discovery::new(
                    vec![
                        reader(generation, calls.clone()),
                        reader(2, fallback.clone()),
                    ],
                    DiscoveryOptions::default(),
                )
                .unwrap();
                let result = discovery.resolve([2; 32], "service").await;
                assert_eq!(result.is_ok(), generation != 0);
                assert_eq!(calls.get(), 1);
                assert_eq!(fallback.get(), 0);
                assert_eq!(provider_calls.get(), 0);
                if let Ok(resolved) = result {
                    assert_eq!(resolved.generation().get(), generation);
                    let mut proposal = resolved.binding().clone();
                    proposal.host = [4; 32];
                    proposal.context.clear();
                    assert_eq!(resolved.binding().host, [1; 32]);
                    assert_eq!(resolved.binding().context, b"validated");
                    assert!(resolved.expires() > tokio::time::Instant::now());
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn resolved_setup_rechecks_recipient_expiry_and_in_progress_deadline() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let recipient = Rc::new(Identity::generate());
            let stranger = Rc::new(Identity::generate());
            let provider_calls = Rc::new(Cell::new(0));
            let directory = Directory::default();
            let _ = directory
                .publish(
                    "service",
                    Binding {
                        host: Identity::generate().public_key(),
                        recipient: recipient.public_key(),
                        address: "127.0.0.1:9000".parse().unwrap(),
                        context: b"validated".to_vec(),
                        provider: capnp_rpc::new_client(PendingProvider(provider_calls.clone())),
                    },
                    None,
                    Duration::from_millis(100),
                )
                .unwrap();
            let discovery = Discovery::from(directory.client(recipient.public_key()));
            let foreign = discovery
                .resolve(recipient.public_key(), "service")
                .await
                .unwrap();
            let expired = discovery
                .resolve(recipient.public_key(), "service")
                .await
                .unwrap();
            let pending = discovery
                .resolve(recipient.public_key(), "service")
                .await
                .unwrap();
            let deadline = pending.expires();
            let wrong =
                DiscoveryConnector::new(stranger, local(), discovery.clone(), None).unwrap();
            assert!(wrong.connect_resolved(foreign).await.is_err());
            assert_eq!(provider_calls.get(), 0);
            let connector = DiscoveryConnector::new(recipient, local(), discovery, None).unwrap();
            let mut setup = Box::pin(connector.connect_resolved(pending));
            for _ in 0..16 {
                assert!(setup.as_mut().now_or_never().is_none());
                if provider_calls.get() == 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert_eq!(provider_calls.get(), 1);
            tokio::time::advance(deadline.saturating_duration_since(tokio::time::Instant::now()))
                .await;
            let error = setup.await.err().expect("expired setup succeeded");
            assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
            assert!(connector.connect_resolved(expired).await.is_err());
            assert_eq!(provider_calls.get(), 1);
        })
        .await;
}

struct Task<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn captured_bindings_connect_their_original_authenticated_hosts_after_rotation() {
    use reproto::{
        native_listener::{Limits, Listener},
        native_provisioning::Provisioner,
        native_rpc::Network,
    };
    use reproto_test_support::runtime_test_capnp::harness;
    struct Echo;
    impl harness::Server for Echo {
        async fn echo(
            self: Rc<Self>,
            p: harness::EchoParams,
            mut r: harness::EchoResults,
        ) -> capnp::Result<()> {
            r.get().set_value(p.get()?.get_value());
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let recipient = Rc::new(Identity::generate());
                let directory = Directory::default();
                let discovery = Discovery::from(directory.client(recipient.public_key()));
                let connector =
                    DiscoveryConnector::new(recipient.clone(), local(), discovery.clone(), None)
                        .unwrap();
                let mut hosts = Vec::new();
                let mut snapshots = Vec::new();
                let mut expected = None;
                for context in [b"first".as_slice(), b"second".as_slice()] {
                    let identity = Rc::new(Identity::generate());
                    let listener = Listener::bind(local(), identity.clone(), Limits::default())
                        .await
                        .unwrap();
                    let (network, handle) = Network::new(identity.public_key());
                    let provider = Provisioner::new(
                        listener.clone(),
                        handle,
                        recipient.public_key(),
                        listener.local_addr().unwrap(),
                        context,
                    )
                    .unwrap();
                    let service: harness::Client = capnp_rpc::new_client(Echo);
                    let task = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                        Box::new(network),
                        Some(service.client),
                    )));
                    expected = Some(
                        directory
                            .publish(
                                "service",
                                Binding {
                                    host: identity.public_key(),
                                    recipient: recipient.public_key(),
                                    address: listener.local_addr().unwrap(),
                                    context: context.to_vec(),
                                    provider: provider.client(),
                                },
                                expected,
                                Duration::from_secs(30),
                            )
                            .unwrap(),
                    );
                    snapshots.push(
                        discovery
                            .resolve(recipient.public_key(), "service")
                            .await
                            .unwrap(),
                    );
                    hosts.push((identity, listener, provider, task));
                }
                assert!(snapshots[1].generation() > snapshots[0].generation());
                assert_eq!(snapshots[0].binding().context, b"first");
                assert_eq!(snapshots[1].binding().context, b"second");
                for (snapshot, (identity, listener, _provider, _task)) in
                    snapshots.into_iter().zip(&hosts)
                {
                    let session = connector.connect_resolved(snapshot).await.unwrap();
                    assert_eq!(session.peer(), identity.public_key());
                    let (network, handle) = Network::new(recipient.public_key());
                    handle.attach(session).unwrap();
                    let mut rpc = capnp_rpc::RpcSystem::new(Box::new(network), None);
                    let capability: harness::Client = rpc.bootstrap(identity.public_key());
                    let _client = Task(tokio::task::spawn_local(rpc));
                    let mut call = capability.echo_request();
                    call.get().set_value(73);
                    assert_eq!(
                        call.send()
                            .promise
                            .await
                            .unwrap()
                            .get()
                            .unwrap()
                            .get_value(),
                        73
                    );
                    assert_eq!(listener.stats().authenticated, 1);
                }
            })
            .await
            .unwrap();
        })
        .await;
}

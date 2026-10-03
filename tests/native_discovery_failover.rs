use capntproto::{
    native_discovery::{Binding, Directory, Discovery, DiscoveryConnector, DiscoveryOptions},
    native_discovery_capnp::directory,
    native_listener::{Limits, Listener},
    native_provisioning::Provisioner,
    native_rpc::{Connector, Network},
    transport::Identity,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{cell::Cell, net::SocketAddr, rc::Rc, time::Duration};

fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
struct Task<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Counted {
    reader: directory::Client,
    calls: Rc<Cell<usize>>,
}
impl directory::Server for Counted {
    async fn lookup(
        self: Rc<Self>,
        params: directory::LookupParams,
        mut results: directory::LookupResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        let mut call = self.reader.lookup_request();
        call.get().set_host(params.get()?.get_host()?);
        let response = call.send().promise.await?;
        results.get().set_record(response.get()?.get_record()?)
    }
    async fn resolve(
        self: Rc<Self>,
        params: directory::ResolveParams,
        mut results: directory::ResolveResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        let mut call = self.reader.resolve_request();
        call.get().set_name(params.get()?.get_name()?);
        let response = call.send().promise.await?;
        results.get().set_record(response.get()?.get_record()?)
    }
}
struct Control {
    reader: directory::Client,
    calls: Rc<Cell<usize>>,
    _client: Task<capnp::Result<()>>,
    server: Task<capnp::Result<()>>,
}
fn control(directory: &Directory, recipient: [u8; 32]) -> Control {
    let calls = Rc::new(Cell::new(0));
    let service: directory::Client = capnp_rpc::new_client(Counted {
        reader: directory.client(recipient),
        calls: calls.clone(),
    });
    let (a, b) = tokio::io::duplex(4096);
    let server = Task(capntproto::rpc::serve(b, service.client));
    let (reader, task) = capntproto::rpc::client(a);
    Control {
        reader,
        calls,
        _client: Task(task),
        server,
    }
}
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
async fn echo(cap: &harness::Client, value: u32) {
    let mut request = cap.echo_request();
    request.get().set_value(value);
    assert_eq!(
        request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        value
    );
}
fn options() -> DiscoveryOptions {
    DiscoveryOptions {
        attempt_timeout: Duration::from_millis(100),
        timeout: Duration::from_millis(500),
    }
}
fn publish(
    directory: &Directory,
    provider: &Provisioner,
    host: [u8; 32],
    recipient: [u8; 32],
    address: SocketAddr,
) {
    let _ = directory
        .publish(
            "service",
            Binding {
                host,
                recipient,
                address,
                context: b"failover".to_vec(),
                provider: provider.client(),
            },
            None,
            Duration::from_secs(30),
        )
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn lost_control_route_fails_over_then_native_rpc_survives_all_directory_routes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                for named in [false, true] {
                    let caller = Rc::new(Identity::generate());
                    let host = Rc::new(Identity::generate());
                    let listener = Listener::bind(local(), host.clone(), Limits::default())
                        .await
                        .unwrap();
                    let (network, handle) = Network::new(host.public_key());
                    let provider = Provisioner::new(
                        listener.clone(),
                        handle,
                        caller.public_key(),
                        listener.local_addr().unwrap(),
                        b"failover",
                    )
                    .unwrap();
                    let primary = control(&Directory::default(), caller.public_key());
                    primary.server.0.abort();
                    let directory = Directory::default();
                    publish(
                        &directory,
                        &provider,
                        host.public_key(),
                        caller.public_key(),
                        listener.local_addr().unwrap(),
                    );
                    let secondary = control(&directory, caller.public_key());
                    let discovery = Discovery::new(
                        vec![primary.reader.clone(), secondary.reader.clone()],
                        options(),
                    )
                    .unwrap();
                    let connector = Rc::new(
                        DiscoveryConnector::new(caller.clone(), local(), discovery, None).unwrap(),
                    );
                    let service: harness::Client = capnp_rpc::new_client(Echo);
                    let _host = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                        Box::new(network),
                        Some(service.client),
                    )));
                    let (network, handle) =
                        Network::with_connector(caller.public_key(), connector.clone());
                    if named {
                        let session = connector.connect_name("service").await.unwrap();
                        assert_eq!(session.peer(), host.public_key());
                        handle.attach(session).unwrap();
                    }
                    let mut rpc = capnp_rpc::RpcSystem::new(Box::new(network), None);
                    let cap: harness::Client = rpc.bootstrap(host.public_key());
                    let _caller = Task(tokio::task::spawn_local(rpc));
                    echo(&cap, 123).await;
                    assert_eq!(secondary.calls.get(), 1);
                    assert_eq!(listener.stats().issued, 1);
                    secondary.server.0.abort();
                    echo(&cap, 456).await;
                    assert_eq!(secondary.calls.get(), 1);
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn authoritative_absence_and_provisioning_failure_do_not_consult_another_reader() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let caller = Rc::new(Identity::generate());
                let host = Rc::new(Identity::generate());
                let listener = Listener::bind(local(), host.clone(), Limits::default())
                    .await
                    .unwrap();
                let (_network, handle) = Network::new(host.public_key());
                let provider = Provisioner::new(
                    listener.clone(),
                    handle,
                    caller.public_key(),
                    listener.local_addr().unwrap(),
                    b"failover",
                )
                .unwrap();
                let first = Directory::default();
                let second = Directory::default();
                publish(
                    &second,
                    &provider,
                    host.public_key(),
                    caller.public_key(),
                    listener.local_addr().unwrap(),
                );
                let primary = control(&first, caller.public_key());
                let secondary = control(&second, caller.public_key());
                let discovery = Discovery::new(
                    vec![primary.reader.clone(), secondary.reader.clone()],
                    options(),
                )
                .unwrap();
                let connector =
                    DiscoveryConnector::new(caller.clone(), local(), discovery, None).unwrap();
                assert!(connector.connect_name("service").await.is_err());
                assert!(connector.connect(host.public_key()).await.is_err());
                assert_eq!(primary.calls.get(), 2);
                assert_eq!(secondary.calls.get(), 0);
                // Successful discovery commits to one binding. Provisioning failure
                // must not silently switch to a second authority or repeat effects.
                publish(
                    &first,
                    &provider,
                    host.public_key(),
                    caller.public_key(),
                    listener.local_addr().unwrap(),
                );
                provider.close();
                assert!(connector.connect_name("service").await.is_err());
                assert!(connector.connect(host.public_key()).await.is_err());
                assert_eq!(primary.calls.get(), 4);
                assert_eq!(secondary.calls.get(), 0);
                assert_eq!(listener.stats().issued, 0);
            })
            .await
            .unwrap();
        })
        .await;
}

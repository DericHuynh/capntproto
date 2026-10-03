use capnp::schema_capnp::node;
use capntproto::{
    schema_exchange::{self as exchange, Catalog, Definition, Key, Limits, Retrieval},
    schema_exchange_capnp as wire,
};
use std::{cell::RefCell, rc::Rc};
fn key(id: u64, revision: u64) -> Key {
    Key { id, revision }
}
fn definition(id: u64, revision: u64, deps: &[u64]) -> Definition {
    let mut m = capnp::message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(id);
    n.set_display_name("schema");
    let mut fields = n.init_struct().init_fields(deps.len() as u32);
    for (i, dep) in deps.iter().enumerate() {
        let mut f = fields.reborrow().get(i as u32);
        f.set_name(format!("f{i}").as_str());
        f.set_discriminant_value(u16::MAX);
        let mut slot = f.init_slot();
        slot.reborrow().init_type().init_struct().set_type_id(*dep);
        slot.init_default_value().init_struct();
    }
    Definition::from_node(revision, m.get_root_as_reader().unwrap(), Limits::default()).unwrap()
}
#[test]
fn immutable_publication_and_limits_are_atomic() {
    let limits = Limits {
        nodes: 3,
        ..Limits::default()
    };
    let mut catalog = Catalog::new(limits);
    catalog.publish([definition(1, 1, &[2])]).unwrap();
    assert!(catalog
        .publish([definition(2, 1, &[]), definition(1, 1, &[3])])
        .is_err());
    assert!(catalog.get(key(2, 1)).is_none());
    catalog
        .publish([definition(1, 1, &[2]), definition(1, 2, &[3])])
        .unwrap();
    assert!(catalog
        .get(key(1, 1))
        .unwrap()
        .dependencies()
        .contains(&key(2, 1)));
    assert!(catalog
        .publish([definition(2, 1, &[]), definition(3, 1, &[])])
        .is_err());
    assert!(catalog.get(key(2, 1)).is_none());
    let d = definition(1, 1, &[]);
    let mut bytes = d.bytes().to_vec();
    bytes.extend_from_slice(&[0; 8]);
    assert!(Definition::decode(key(1, 1), &bytes, limits).is_err());
    assert!(Definition::decode(key(2, 1), d.bytes(), limits).is_err());
    assert!(Definition::decode(
        key(1, 1),
        d.bytes(),
        Limits {
            node_bytes: 8,
            ..limits
        }
    )
    .is_err());
    let mut retrieval = Retrieval::new(key(1, 1), Limits { nodes: 1, ..limits }).unwrap();
    assert_eq!(retrieval.request().unwrap(), Some(key(1, 1)));
    assert!(retrieval
        .receive(key(1, 1), Some(definition(1, 1, &[2])))
        .is_err());
    assert!(!retrieval.ready());
    assert!(retrieval.finish().is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn pinned_shared_and_cyclic_schema_graphs_over_rpc() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let catalog = Rc::new(RefCell::new(Catalog::new(Limits::default())));
            catalog
                .borrow_mut()
                .publish([
                    definition(1, 1, &[2, 3]),
                    definition(2, 1, &[1, 3]),
                    definition(3, 1, &[]),
                    definition(1, 2, &[]),
                ])
                .unwrap();
            let (a, b) = tokio::io::duplex(4096);
            let server = capntproto::rpc::serve(b, Catalog::service(catalog.clone()).client);
            let (client, driver): (wire::catalog::Client, _) = capntproto::rpc::client(a);
            let bundle = exchange::fetch(&client, key(1, 1), Limits::default(), 3)
                .await
                .unwrap();
            assert_eq!(bundle.len(), 3);
            assert_eq!(bundle.root(), key(1, 1));
            assert!(bundle.get(key(1, 2)).is_none());
            assert_eq!(
                bundle
                    .get(key(3, 1))
                    .unwrap()
                    .read()
                    .unwrap()
                    .get_root::<node::Reader>()
                    .unwrap()
                    .get_id(),
                3
            );
            assert_eq!(
                exchange::fetch(&client, key(1, 2), Limits::default(), 1)
                    .await
                    .unwrap()
                    .len(),
                1
            );
            assert!(exchange::fetch(&client, key(4, 1), Limits::default(), 2)
                .await
                .is_err());
            assert!(exchange::fetch(
                &client,
                key(1, 1),
                Limits {
                    nodes: 2,
                    ..Limits::default()
                },
                3
            )
            .await
            .is_err());
            server.abort();
            driver.abort();
        })
        .await;
}
#[test]
fn compiler_schema_graph_includes_brands_methods_and_annotations() {
    let output = std::process::Command::new("capnp")
        .args([
            "compile",
            "-o-",
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/dynamic-test.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let message = capnp::serialize::read_message(
        output.stdout.as_slice(),
        capnp::message::ReaderOptions::new(),
    )
    .unwrap();
    let request = message
        .get_root::<capnp::schema_capnp::code_generator_request::Reader>()
        .unwrap();
    let mut catalog = Catalog::new(Limits::default());
    let defs: Vec<_> = request
        .get_nodes()
        .unwrap()
        .iter()
        .map(|n| Definition::from_node(1, n, Limits::default()).unwrap())
        .collect();
    assert!(defs.len() > 10);
    let root = defs
        .iter()
        .find(|d| {
            d.read()
                .unwrap()
                .get_root::<node::Reader>()
                .unwrap()
                .get_display_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(":Wrap")
        })
        .unwrap()
        .key();
    catalog.publish_request(1, request).unwrap();
    let mut retrieval = Retrieval::new(root, Limits::default()).unwrap();
    while let Some(key) = retrieval.request().unwrap() {
        retrieval
            .receive(key, catalog.get(key).cloned())
            .unwrap_or_else(|e| panic!("missing {key:?}: {e}"));
    }
    let bundle = retrieval.finish().unwrap();
    assert!(
        bundle.len() > 10,
        "scope, annotation, group, method and branded type dependencies were omitted"
    );
}
struct WrongRevision;
impl wire::catalog::Server for WrongRevision {
    async fn get(
        self: Rc<Self>,
        _: wire::catalog::GetParams,
        mut out: wire::catalog::GetResults,
    ) -> capnp::Result<()> {
        let mut found = out.get().init_result().init_found();
        let mut key = found.reborrow().init_key();
        key.set_id(1);
        key.set_revision(2);
        found.set_node(definition(1, 2, &[]).bytes());
        Ok(())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn wrong_revision_cannot_become_an_active_bundle() {
    let client = capnp_rpc::new_client(WrongRevision);
    assert!(exchange::fetch(&client, key(1, 1), Limits::default(), 1)
        .await
        .is_err());
    let mut r = Retrieval::new(key(1, 1), Limits::default()).unwrap();
    r.request().unwrap();
    assert!(r.receive(key(1, 1), Some(definition(1, 2, &[]))).is_err());
    assert!(r.receive(key(1, 1), Some(definition(1, 1, &[]))).is_err());
    assert!(r.finish().is_err());
}

fn graph(scenario: &str, revision: u64) -> Vec<Definition> {
    let deps: [&[u64]; 3] = match scenario {
        "shared" => [&[2, 3], &[3], &[]],
        "cycle" => [&[2], &[1], &[]],
        "missing" => [&[3], &[], &[]],
        _ => [&[2], &[], &[]],
    };
    (1..=if scenario == "missing" { 2 } else { 3 })
        .map(|id| definition(id, revision, deps[id as usize - 1]))
        .collect()
}
fn write_definition(d: &Definition, mut out: wire::definition::Builder<'_>) {
    let mut key = out.reborrow().init_key();
    key.set_id(d.key().id);
    key.set_revision(d.key().revision);
    out.set_node(d.bytes());
}
struct Gates {
    catalog: Rc<RefCell<Catalog>>,
    receivers:
        Rc<RefCell<std::collections::BTreeMap<u64, futures::channel::oneshot::Receiver<bool>>>>,
}
impl wire::catalog::Server for Gates {
    async fn get(
        self: Rc<Self>,
        p: wire::catalog::GetParams,
        mut out: wire::catalog::GetResults,
    ) -> capnp::Result<()> {
        let k = p.get()?.get_key()?;
        let k = key(k.get_id(), k.get_revision());
        let receive = self.receivers.borrow_mut().remove(&k.id).unwrap();
        let wrong = receive
            .await
            .map_err(|_| capnp::Error::disconnected("cancelled schema reply".into()))?;
        let mut result = out.get().init_result();
        let catalog = self.catalog.borrow();
        match catalog.get(k) {
            Some(d) => {
                if wrong {
                    write_definition(&definition(k.id, 3 - k.revision, &[]), result.init_found());
                } else {
                    write_definition(d, result.init_found());
                }
            }
            None => result.set_missing(()),
        }
        Ok(())
    }
}
struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}
#[derive(serde::Deserialize)]
struct Trace {
    scenario: String,
    revision: u64,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    item: u64,
    wrong: bool,
    state: Vec<usize>,
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_schema_exchange_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_SCHEMA_EXCHANGE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                tokio::time::timeout(std::time::Duration::from_secs(3), replay(trace))
                    .await
                    .unwrap();
            }
        })
        .await;
}
async fn replay(trace: Trace) {
    use std::collections::BTreeMap;
    let catalog = Rc::new(RefCell::new(Catalog::new(Limits::default())));
    catalog
        .borrow_mut()
        .publish(graph(&trace.scenario, trace.revision))
        .unwrap();
    let mut receivers = BTreeMap::new();
    let mut releases = BTreeMap::new();
    for id in 1..=3 {
        let (tx, rx) = futures::channel::oneshot::channel();
        receivers.insert(id, rx);
        releases.insert(id, tx);
    }
    let service: wire::catalog::Client = capnp_rpc::new_client(Gates {
        catalog: catalog.clone(),
        receivers: Rc::new(RefCell::new(receivers)),
    });
    let (a, b) = tokio::io::duplex(4096);
    let server = capntproto::rpc::serve(b, service.client);
    let (client, driver): (wire::catalog::Client, _) = capntproto::rpc::client(a);
    let _tasks = Tasks(vec![server, driver]);
    let mut state = Some(Retrieval::new(key(1, trace.revision), Limits::default()).unwrap());
    let mut pending = BTreeMap::new();
    let mut active = false;
    let mut published = trace.revision;
    let mut counts = (1, 0, 0, false);
    for step in trace.steps {
        match step.action.as_str() {
            "ask" => {
                let k = state.as_mut().unwrap().request().unwrap().unwrap();
                assert_eq!(k, key(step.item, trace.revision));
                let mut request = client.get_request();
                let mut key = request.get().init_key();
                key.set_id(k.id);
                key.set_revision(k.revision);
                pending.insert(k.id, request.send().promise);
            }
            "receive" => {
                assert!(releases
                    .remove(&step.item)
                    .unwrap()
                    .send(step.wrong)
                    .is_ok());
                let response = pending.remove(&step.item).unwrap().await.unwrap();
                let d = match response
                    .get()
                    .unwrap()
                    .get_result()
                    .unwrap()
                    .which()
                    .unwrap()
                {
                    wire::catalog::result::Missing(()) => None,
                    wire::catalog::result::Found(d) => {
                        let d = d.unwrap();
                        let k = d.get_key().unwrap();
                        Some(
                            Definition::decode(
                                key(k.get_id(), k.get_revision()),
                                d.get_node().unwrap(),
                                Limits::default(),
                            )
                            .unwrap(),
                        )
                    }
                };
                let result = state
                    .as_mut()
                    .unwrap()
                    .receive(key(step.item, trace.revision), d);
                assert_eq!(result.is_err(), step.state[3] != 0);
            }
            "publish" => {
                catalog
                    .borrow_mut()
                    .publish(graph(&trace.scenario, 2))
                    .unwrap();
                published = 2;
            }
            "cancel" => {
                state.take();
                pending.clear();
                counts = (0, 0, 0, true);
            }
            "activate" => {
                let retrieval = state.take().unwrap();
                assert!(retrieval.ready());
                counts = retrieval.counts();
                let bundle = retrieval.finish().unwrap();
                assert_eq!(bundle.root(), key(1, trace.revision));
                assert_eq!(bundle.len(), counts.2);
                active = true;
            }
            _ => panic!(),
        }
        if let Some(state) = &state {
            counts = state.counts();
            assert_eq!(state.ready(), !counts.3 && counts.0 == counts.2);
        }
        assert_eq!(
            [
                counts.0,
                counts.1,
                counts.2,
                usize::from(counts.3),
                usize::from(active),
                published as usize
            ],
            step.state[..]
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn schema_service_over_authenticated_native() {
    use capntproto::{
        native_rpc::Network,
        transport::{self, Identity},
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let a = Identity::generate();
                let b = Identity::generate();
                let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let remote = right.local_addr().unwrap();
                let (aa, bb) = tokio::join!(
                    transport::connect_authenticated(
                        left,
                        remote,
                        &a,
                        b.public_key(),
                        None,
                        b"schemas"
                    ),
                    transport::accept_authenticated(right, &b, a.public_key(), None, b"schemas")
                );
                let (an, ah) = Network::new(a.public_key());
                let (bn, bh) = Network::new(b.public_key());
                ah.attach(aa.unwrap()).unwrap();
                bh.attach(bb.unwrap()).unwrap();
                let owner = Rc::new(RefCell::new(Catalog::new(Limits::default())));
                owner.borrow_mut().publish(graph("cycle", 1)).unwrap();
                let server =
                    capnp_rpc::RpcSystem::new(Box::new(bn), Some(Catalog::service(owner).client));
                let mut client = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let schemas: wire::catalog::Client = client.bootstrap(b.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(server),
                    tokio::task::spawn_local(client),
                ]);
                let bundle = exchange::fetch(&schemas, key(1, 1), Limits::default(), 3)
                    .await
                    .unwrap();
                assert_eq!(bundle.len(), 2);
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_fetch_releases_outstanding_schema_calls() {
    use std::{collections::BTreeMap, time::Duration};
    tokio::task::LocalSet::new().run_until(async {
        let catalog=Rc::new(RefCell::new(Catalog::new(Limits::default())));
        catalog.borrow_mut().publish(graph("shared",1)).unwrap();
        let mut receivers=BTreeMap::new(); let mut senders=BTreeMap::new();
        for id in 1..=3 { let (tx,rx)=futures::channel::oneshot::channel(); receivers.insert(id,rx); senders.insert(id,tx); }
        let receivers=Rc::new(RefCell::new(receivers));
        let service: wire::catalog::Client=capnp_rpc::new_client(Gates {catalog,receivers:receivers.clone()});
        let (a,b)=tokio::io::duplex(4096);
        let server=capntproto::rpc::serve(b,service.client);
        let (client,driver): (wire::catalog::Client,_) = capntproto::rpc::client(a);
        let _tasks=Tasks(vec![server,driver]);
        senders.remove(&1).unwrap().send(false).unwrap();
        let mut fetch=Box::pin(exchange::fetch(&client,key(1,1),Limits::default(),3));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    _ = &mut fetch => panic!("fetch completed before dependencies"),
                    _ = tokio::time::sleep(Duration::from_millis(1)) => if receivers.borrow().is_empty() { break; },
                }
            }
        }).await.unwrap();
        assert!(senders.values().all(|s| !s.is_canceled()));
        drop(fetch);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !senders.values().all(|s| s.is_canceled()) { tokio::task::yield_now().await; }
        }).await.unwrap();
    }).await;
}
#[test]
fn schema_payloads_reject_live_capabilities_and_zero_type_references() {
    let service = capnp_rpc::new_client::<wire::catalog::Client, _>(WrongRevision);
    let mut message = capnp::message::Builder::new_default();
    use capnp::traits::ImbueMut;
    let mut cap_table = capnp::private::layout::CapTable::new();
    let mut node = message.init_root::<node::Builder>();
    node.imbue_mut(&mut cap_table);
    node.set_id(1);
    let mut value = node.init_const();
    value
        .reborrow()
        .init_type()
        .init_any_pointer()
        .init_unconstrained()
        .set_any_kind(());
    value
        .init_value()
        .init_any_pointer()
        .set_as_capability(service.client.hook);
    assert!(
        Definition::from_node(1, message.get_root_as_reader().unwrap(), Limits::default()).is_err()
    );
    assert!(Definition::decode(
        key(1, 1),
        &capnp::serialize::write_message_to_words(&message),
        Limits::default()
    )
    .is_err());
    let mut m = capnp::message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(1);
    n.init_struct()
        .init_fields(1)
        .get(0)
        .init_slot()
        .init_type()
        .init_struct()
        .set_type_id(0);
    assert!(Definition::from_node(1, m.get_root_as_reader().unwrap(), Limits::default()).is_err());
}

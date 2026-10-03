use super::super::*;
use capnp::{
    dynamic_capability as compiled,
    schema_loader::{dynamic as loaded, SchemaLoader},
};

fn loader() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<harness::Owned>()
        .unwrap();
    loader
}
pub(super) async fn replay_loaded(
    path: &[capntproto_test_support::verification::exploration::State],
    words: u32,
) {
    let loader = loader();
    let schema = loader
        .get(
            result_type()
                .as_struct_schema()
                .unwrap()
                .get_proto()
                .get_id(),
        )
        .unwrap();
    let live = Rc::new(Cell::new(0));
    let mut message =
        message::Builder::new(message::HeapAllocator::new().first_segment_words(words));
    let mut caps = Vec::new();
    let mut p: capnp::any_pointer::Builder = message.get_root().unwrap();
    p.imbue_mut(&mut caps);
    let (mut root, token) = loaded::orphan::Root::new(p, schema.clone())
        .unwrap()
        .with_orphanage();
    let mut foreign = message::Builder::new_default();
    let mut wrong = loaded::orphan::Root::new(foreign.get_root().unwrap(), schema.clone()).unwrap();
    let mut orphan = None;
    for state in path {
        match state["event"] {
            k @ (1 | 2) => {
                let mut access = token.in_root(&mut root).unwrap();
                let mut owner = access
                    .new_struct(schema.clone())
                    .unwrap()
                    .release_as::<harness::pending_results::Owned>()
                    .unwrap();
                access
                    .edit_typed(&mut owner, |mut r| {
                        r.set_cap(Echo::client(k as u32, &live));
                        Ok(())
                    })
                    .unwrap();
                orphan = Some(owner.into_dynamic());
            }
            3 => root.adopt(orphan.take().unwrap()).unwrap(),
            4 => orphan = Some(root.disown(&token).unwrap()),
            5 => drop(orphan.take()),
            6 => root.clear(),
            7 => {
                let error = wrong.adopt(orphan.take().unwrap()).unwrap_err();
                assert_eq!(error.error.kind, capnp::ErrorKind::WrongArena);
                orphan = Some(error.orphan);
                assert!(wrong.is_null());
            }
            8 => {
                let error = orphan
                    .take()
                    .unwrap()
                    .release_as::<harness::value::Owned>()
                    .err()
                    .unwrap();
                assert_eq!(error.error.kind, capnp::ErrorKind::TypeMismatch);
                orphan = Some(error.orphan);
            }
            9 => {
                let cap = root
                    .as_reader()
                    .unwrap()
                    .downcast_native::<harness::pending_results::Owned>()
                    .unwrap()
                    .get_cap()
                    .unwrap();
                let mut source = capnp_rpc::PipelineBuilder::<capnp::any_pointer::Owned>::new();
                source
                    .get()
                    .init_as::<harness::pending_results::Builder>()
                    .set_cap(cap);
                let value =
                    loaded::Reader::new(source.get().into_reader(), schema.clone()).unwrap();
                orphan = Some(
                    token
                        .in_root(&mut root)
                        .unwrap()
                        .copy(loaded::Value::Struct(value))
                        .unwrap(),
                );
            }
            10 => {
                orphan = Some(
                    token
                        .in_root(&mut root)
                        .unwrap()
                        .null(capnp::schema_loader::Type::Struct(schema.clone()))
                        .unwrap(),
                )
            }
            _ => panic!(),
        }
        let cap = if root.is_null() {
            None
        } else {
            Some(
                root.as_reader()
                    .unwrap()
                    .downcast_native::<harness::pending_results::Owned>()
                    .unwrap()
                    .get_cap()
                    .unwrap(),
            )
        };
        assert_eq!(u64::from(value(cap).await), state["root"]);
        assert_eq!(u64::from(live.get()), state["live"]);
        if let Some(owner) = &mut orphan {
            let mut access = token.in_root(&mut root).unwrap();
            if state["orphan"] == 3 {
                assert!(access.is_null(owner).unwrap());
            } else {
                let cap = access
                    .read(owner, |v| {
                        let loaded::Value::Struct(s) = v else {
                            panic!()
                        };
                        s.downcast_native::<harness::pending_results::Owned>()?
                            .get_cap()
                    })
                    .unwrap();
                assert_eq!(u64::from(value(Some(cap)).await), state["orphan"]);
            }
        } else {
            assert_eq!(state["orphan"], 0);
        }
    }
    drop(orphan);
    root.clear();
    assert_eq!(live.get(), 0);
}

struct Service(Rc<Cell<u32>>, loaded::ServiceSchema);
impl compiled::Server for Service {
    fn get_schema(&self) -> capnp::schema::InterfaceSchema {
        harness::Client::schema()
    }
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(
        self: Rc<Self>,
        method: capnp::schema::Method,
        mut c: compiled::CallContext,
    ) -> Promise<(), Error> {
        Promise::from_future(async move {
            if method.get_result_type().is_none() {
                assert!(c.get_results_orphanage(None).is_err());
                assert!(c.get_results_with_size_hint(None).is_err());
                assert!(c.init_results(None).is_err());
                return Ok(());
            }
            c.get_results_with_size_hint(Some(MessageSize {
                word_count: 0,
                cap_count: 0,
            }))?
            .set_named(
                "cap",
                compiled::Client::from(Echo::client(1, &self.0)).into(),
            )?;
            c.init_results(None)?;
            assert_eq!(self.0.get(), 0);
            {
                let (mut root, token) = c.get_results_orphanage(None)?;
                let orphan = new_orphan(&mut root, &token, 73, &self.0);
                root.adopt(orphan).unwrap();
            }
            c.set_pipeline()?;
            Ok(())
        })
    }
}
impl loaded::Server for Service {
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(self: Rc<Self>, mut c: loaded::CallContext) -> Promise<(), Error> {
        Promise::from_future(async move {
            if c.method()?.get_proto().get_result_struct_type() == 0x995f9a3377c0b16e {
                assert!(c.get_results_orphanage(None).is_err());
                assert!(c.get_results_with_size_hint(None).is_err());
                assert!(c.init_results(None).is_err());
                return Ok(());
            }
            c.get_results_with_size_hint(Some(MessageSize {
                word_count: 0,
                cap_count: 0,
            }))?
            .set_named(
                "cap",
                loaded::Value::Capability(self.1.reflect(Echo::client(1, &self.0))?),
            )?;
            c.init_results(None)?;
            assert_eq!(self.0.get(), 0);
            {
                let (mut root, token) = c.get_results_orphanage(None)?;
                let mut access = token.in_root(&mut root)?;
                let schema = self.1.get()?.method("pending")?.results()?;
                let mut orphan = access
                    .new_struct(schema)?
                    .release_as::<harness::pending_results::Owned>()
                    .unwrap();
                access.edit_typed(&mut orphan, |mut r| {
                    r.set_cap(Echo::client(73, &self.0));
                    Ok(())
                })?;
                root.adopt(orphan.into_dynamic()).unwrap();
            }
            c.set_pipeline()?;
            Ok(())
        })
    }
}
#[tokio::test(flavor = "current_thread")]
async fn dynamic_contexts_allocate_reset_adopt_and_reject_streaming_results() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for use_loaded in [false, true] {
                    let live = Rc::new(Cell::new(0));
                    let schema = loaded::ServiceSchema::new(
                        Rc::new(loader()),
                        harness::Client::schema().get_proto().get_id(),
                    )
                    .unwrap();
                    let service = Service(live.clone(), schema.clone());
                    let client: harness::Client = if use_loaded {
                        capnp_rpc::new_loaded_client(service, schema).cast_to()
                    } else {
                        capnp_rpc::new_dynamic_client(service)
                            .as_client()
                            .unwrap()
                            .clone()
                            .cast_to()
                    };
                    let mut drivers = Drivers(Vec::new());
                    let client = if wire {
                        let hub = Rc::new(RefCell::new(support::Hub::default()));
                        let host = capnp_rpc::RpcSystem::new(
                            Box::new(support::Hub::network(&hub, 1)),
                            Some(client.client),
                        );
                        let mut caller = capnp_rpc::RpcSystem::new(
                            Box::new(support::Hub::network(&hub, 2)),
                            None,
                        );
                        let client = caller.bootstrap(1);
                        drivers.0.push(tokio::task::spawn_local(host));
                        drivers.0.push(tokio::task::spawn_local(caller));
                        client
                    } else {
                        client
                    };
                    let response = client.pending_request().send().promise.await.unwrap();
                    assert_eq!(
                        value(Some(response.get().unwrap().get_cap().unwrap())).await,
                        73
                    );
                    drop(response);
                    settle().await;
                    assert_eq!(live.get(), 0);
                    client.stream_request().send().await.unwrap();
                }
            }
        })
        .await;
}

#[test]
fn adoption_checks_root_schema_before_overwriting_existing_data() {
    let live = Rc::new(Cell::new(0));
    let mut m = message::Builder::new_default();
    let mut caps = Vec::new();
    let mut p: capnp::any_pointer::Builder = m.get_root().unwrap();
    p.imbue_mut(&mut caps);
    let (mut root, token) = Root::new(p, harness::value::Owned::introspect())
        .unwrap()
        .with_orphanage();
    root.get_typed::<harness::value::Owned>()
        .unwrap()
        .set_value(42);
    let orphan = new_orphan(&mut root, &token, 1, &live);
    let error = root.adopt(orphan).unwrap_err();
    assert_eq!(error.error.kind, capnp::ErrorKind::TypeMismatch);
    assert_eq!(
        root.get_typed::<harness::value::Owned>()
            .unwrap()
            .get_value(),
        42
    );
    assert_eq!(live.get(), 1);
    drop(error.orphan);
    assert_eq!(live.get(), 0);
    let loader = loader();
    let good = loader
        .get(
            result_type()
                .as_struct_schema()
                .unwrap()
                .get_proto()
                .get_id(),
        )
        .unwrap();
    let bad = loader
        .get(
            harness::value::Owned::introspect()
                .as_struct_schema()
                .unwrap()
                .get_proto()
                .get_id(),
        )
        .unwrap();
    let mut m = message::Builder::new_default();
    let mut caps = Vec::new();
    let mut p: capnp::any_pointer::Builder = m.get_root().unwrap();
    p.imbue_mut(&mut caps);
    let (mut root, token) = loaded::orphan::Root::new(p, bad).unwrap().with_orphanage();
    root.get()
        .unwrap()
        .set_named("value", loaded::Value::UInt32(42))
        .unwrap();
    let mut access = token.in_root(&mut root).unwrap();
    let mut orphan = access
        .new_struct(good)
        .unwrap()
        .release_as::<harness::pending_results::Owned>()
        .unwrap();
    access
        .edit_typed(&mut orphan, |mut r| {
            r.set_cap(Echo::client(1, &live));
            Ok(())
        })
        .unwrap();
    let error = root.adopt(orphan.into_dynamic()).unwrap_err();
    assert_eq!(error.error.kind, capnp::ErrorKind::TypeMismatch);
    assert!(matches!(
        root.as_reader().unwrap().get_named("value").unwrap(),
        loaded::Value::UInt32(42)
    ));
    assert_eq!(live.get(), 1);
    drop(error.orphan);
    assert_eq!(live.get(), 0);
}

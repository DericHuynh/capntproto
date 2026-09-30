use capnp::{
    message,
    schema_capnp::{code_generator_request, node},
    schema_loader::{
        dynamic::{self, Value},
        Kind, Limits, SchemaLoader, Type,
    },
};
fn compiler_request() -> message::Reader<capnp::serialize::OwnedSegments> {
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
    capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new()).unwrap()
}
fn id(loader: &SchemaLoader, name: &str) -> u64 {
    loader
        .get_all_loaded()
        .find(|s| {
            s.get_proto()
                .get_display_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
        })
        .unwrap_or_else(|| panic!("missing {name}"))
        .id()
}
fn compiled() -> SchemaLoader {
    let message = compiler_request();
    let mut loader = SchemaLoader::default();
    loader
        .load_request(
            message
                .get_root::<code_generator_request::Reader>()
                .unwrap(),
        )
        .unwrap();
    loader
}
fn definition(version: u16) -> message::Builder<message::HeapAllocator> {
    let mut message = message::Builder::new_default();
    let mut n = message.init_root::<node::Builder>();
    n.set_id(1);
    n.set_display_name("Example");
    let mut s = n.init_struct();
    s.set_pointer_count(1);
    s.set_data_word_count(1);
    let mut fields = s.init_fields(u32::from(version) + 1);
    for i in 0..u32::from(version) {
        let mut f = fields.reborrow().get(i);
        f.set_name(format!("f{i}").as_str());
        f.set_code_order(i as u16);
        f.reborrow().init_ordinal().set_explicit(i as u16);
        let mut slot = f.init_slot();
        slot.set_offset(i);
        slot.reborrow().init_type().set_uint32(());
        slot.init_default_value().set_uint32(42);
    }
    let mut f = fields.get(u32::from(version));
    f.set_name("child");
    f.set_code_order(version);
    let mut slot = f.init_slot();
    slot.reborrow().init_type().init_struct().set_type_id(2);
    slot.init_default_value().init_struct();
    message
}
// Keep child at ordinal zero so adding scalar fields is a compatible append.
fn version(v: u16) -> message::Builder<message::HeapAllocator> {
    let mut m = definition(v);
    let node::Struct(s) = m.get_root::<node::Builder>().unwrap().which().unwrap() else {
        unreachable!()
    };
    let mut fields = s.get_fields().unwrap();
    let child = fields.reborrow().get(u32::from(v)).into_reader();
    let mut temp = message::Builder::new_default();
    temp.set_root(child).unwrap();
    for i in (1..=u32::from(v)).rev() {
        let mut copy = message::Builder::new_default();
        copy.set_root(fields.reborrow().get(i - 1).into_reader())
            .unwrap();
        fields
            .set_with_caveats(i, copy.get_root_as_reader().unwrap())
            .unwrap();
    }
    fields
        .set_with_caveats(0, temp.get_root_as_reader().unwrap())
        .unwrap();
    for i in 0..=u32::from(v) {
        let mut f = fields.reborrow().get(i);
        f.set_code_order(i as u16);
        f.init_ordinal().set_explicit(i as u16);
    }
    m
}
fn child(kind: Kind) -> message::Builder<message::HeapAllocator> {
    let mut m = message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(2);
    match kind {
        Kind::Struct => {
            n.init_struct();
        }
        Kind::Enum => {
            n.init_enum();
        }
        _ => unreachable!(),
    };
    m
}
#[test]
fn loaded_compiler_schemas_build_and_read_typed_wire_messages() {
    use reproto_test_support::dynamic_test_capnp::orphan_case;
    let loader = compiled();
    let schema = loader.get(id(&loader, "OrphanCase")).unwrap();
    let mut message = message::Builder::new_default();
    let mut root = dynamic::Builder::init(message.init_root(), schema.clone()).unwrap();
    assert!(matches!(
        root.as_reader().get_named("flag").unwrap(),
        Value::Bool(true)
    ));
    assert!(matches!(root.as_reader().get_named("float").unwrap(), Value::Float64(x) if x == -1.5));
    assert!(
        matches!(root.as_reader().get_named("description").unwrap(), Value::Text(x) if x == "default")
    );
    root.set_named("flag", Value::Bool(false)).unwrap();
    root.set_named("description", Value::Text("loaded".into()))
        .unwrap();
    root.clear_named("flag").unwrap();
    assert!(matches!(
        root.as_reader().get_named("flag").unwrap(),
        Value::Bool(true)
    ));
    root.set_named("flag", Value::Bool(false)).unwrap();
    root.set_named("sentinel", Value::UInt32(9)).unwrap();
    assert!(root.set_named("arm", Value::UInt32(1)).is_err());
    assert_eq!(
        root.as_reader()
            .which()
            .unwrap()
            .unwrap()
            .get_proto()
            .get_name()
            .unwrap(),
        "sentinel"
    );
    root.reborrow()
        .init_struct("source")
        .unwrap()
        .set_named("number", Value::UInt32(73))
        .unwrap();
    let mut list = root.reborrow().init_list("numbers", 3).unwrap();
    for i in 0..3 {
        list.set(i, Value::UInt32(i + 10)).unwrap();
    }
    assert!(list.set(3, Value::UInt32(99)).is_err());
    root.reborrow()
        .get_list("numbers")
        .unwrap()
        .set(0, Value::UInt32(44))
        .unwrap();
    let mut items = root.reborrow().init_list("values", 2).unwrap();
    items
        .reborrow()
        .get_struct(1)
        .unwrap()
        .set_named("text", Value::Text("item".into()))
        .unwrap();
    let typed = message.get_root_as_reader::<orphan_case::Reader>().unwrap();
    assert!(!typed.get_flag());
    assert_eq!(typed.get_description().unwrap(), "loaded");
    assert_eq!(typed.get_source().unwrap().get_number(), 73);
    assert_eq!(typed.get_numbers().unwrap().get(2), 12);
    assert_eq!(
        typed.get_values().unwrap().get(1).get_text().unwrap(),
        "item"
    );
    let reader = dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema).unwrap();
    let Value::List(list) = reader.get_named("values").unwrap() else {
        panic!()
    };
    let Value::Struct(item) = list.get(1).unwrap() else {
        panic!()
    };
    assert!(matches!(
        item.get_named("number").unwrap(),
        Value::UInt32(42)
    ));
}
#[test]
fn loaded_generics_groups_and_inherited_methods_preserve_brands() {
    let loader = compiled();
    let envelope = loader.get(id(&loader, "BrandEnvelope")).unwrap();
    let Type::Struct(parcel) = envelope.field("item").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert!(matches!(
        parcel.field("value").unwrap().get_type().unwrap(),
        Type::Text
    ));
    let unbound = loader.get_unbound(id(&loader, "Parcel")).unwrap();
    assert!(matches!(
        unbound.field("value").unwrap().get_type().unwrap(),
        Type::Parameter(..)
    ));
    let envelope = loader.get(id(&loader, "Envelope")).unwrap();
    let Type::Interface(derived) = envelope.field("cap").unwrap().get_type().unwrap() else {
        panic!()
    };
    let transform = derived.method("transform").unwrap();
    let Type::Interface(cap) = transform
        .params()
        .unwrap()
        .field("cap")
        .unwrap()
        .get_type()
        .unwrap()
    else {
        panic!()
    };
    assert!(cap
        .get_proto()
        .get_display_name()
        .unwrap()
        .to_str()
        .unwrap()
        .ends_with(":Harness"));
    let group = loader.get(id(&loader, "OrphanGroup")).unwrap();
    let mut m = message::Builder::new_default();
    let mut b = dynamic::Builder::init(m.init_root(), group).unwrap();
    b.set_named("sibling", Value::UInt64(123)).unwrap();
    b.reborrow()
        .group("body")
        .unwrap()
        .group("nested")
        .unwrap()
        .set_named("text", Value::Text("nested".into()))
        .unwrap();
    let typed = m
        .get_root_as_reader::<reproto_test_support::dynamic_test_capnp::orphan_group::Reader>()
        .unwrap();
    assert_eq!(typed.get_sibling(), 123);
    let reproto_test_support::dynamic_test_capnp::orphan_group::body::Nested(nested) =
        typed.get_body().which().unwrap()
    else {
        panic!()
    };
    assert_eq!(nested.get_text().unwrap(), "nested");
}
#[test]
fn typed_stubs_replacements_and_failed_batches_are_atomic() {
    let mut loader = SchemaLoader::default();
    let v1 = version(1);
    let v2 = version(2);
    loader.load(v1.get_root_as_reader().unwrap()).unwrap();
    assert!(loader.get(2).unwrap().is_stub());
    assert_eq!(loader.get_all_loaded().count(), 1);
    loader.load(v2.get_root_as_reader().unwrap()).unwrap();
    assert_eq!(loader.get(1).unwrap().fields().unwrap().len(), 3);
    loader.load(v1.get_root_as_reader().unwrap()).unwrap();
    assert_eq!(loader.get(1).unwrap().fields().unwrap().len(), 3);
    let bad = child(Kind::Enum);
    assert!(loader.load(bad.get_root_as_reader().unwrap()).is_err());
    assert!(loader.get(2).unwrap().is_stub());
    let good = child(Kind::Struct);
    loader.load(good.get_root_as_reader().unwrap()).unwrap();
    assert!(!loader.get(2).unwrap().is_stub());
    let mut fresh = SchemaLoader::default();
    assert!(fresh
        .load_batch([
            v1.get_root_as_reader().unwrap(),
            bad.get_root_as_reader().unwrap()
        ])
        .is_err());
    assert!(fresh.is_empty());
    let mut tiny = SchemaLoader::new(Limits {
        nodes: 1,
        ..Limits::default()
    });
    assert!(tiny.load(v1.get_root_as_reader().unwrap()).is_err());
    assert!(tiny.is_empty());
}

struct LoadedEcho;
impl dynamic::Server for LoadedEcho {
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(
        self: std::rc::Rc<Self>,
        mut context: dynamic::CallContext,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::from_future(async move {
            let name = context.method()?.get_proto().get_name()?.to_str()?;
            match name {
                "echo" => {
                    let (params, mut results) = context.get()?;
                    let Value::UInt32(n) = params.get_named("value")? else {
                        panic!()
                    };
                    results.set_named("value", Value::UInt32(n + 1))?;
                }
                "bounce" => {
                    let (params, mut results) = context.get()?;
                    results.set_named("cap", params.get_named("cap")?)?;
                }
                _ => return Err(capnp::Error::unimplemented("method".into())),
            }
            Ok(())
        })
    }
}
#[tokio::test(flavor = "current_thread")]
async fn transmitted_schema_drives_rpc_servers_capability_calls_and_pipelines() {
    use reproto::schema_exchange::{self, Catalog, Key, Limits as ExchangeLimits};
    use std::{cell::RefCell, rc::Rc};
    tokio::task::LocalSet::new()
        .run_until(async {
            let request = compiler_request();
            let local_loader = Rc::new(compiled());
            let harness_id = id(&local_loader, "Harness");
            let service = dynamic::ServiceSchema::new(local_loader.clone(), harness_id).unwrap();
            let host = capnp_rpc::new_loaded_client(LoadedEcho, service.clone());
            let mut catalog = Catalog::new(ExchangeLimits::default());
            catalog
                .publish_request(1, request.get_root().unwrap())
                .unwrap();
            let (a, b) = tokio::io::duplex(4096);
            let catalog_server =
                reproto::rpc::serve(b, Catalog::service(Rc::new(RefCell::new(catalog))).client);
            let (catalog_client, catalog_driver) = reproto::rpc::client(a);
            let bundle = schema_exchange::fetch(
                &catalog_client,
                Key {
                    id: harness_id,
                    revision: 1,
                },
                ExchangeLimits::default(),
                4,
            )
            .await
            .unwrap();
            let remote_loader = bundle.load(Limits::default()).unwrap();
            assert_eq!(remote_loader.len(), bundle.len());
            let (a, b) = tokio::io::duplex(4096);
            let host_driver = reproto::rpc::serve(b, host.clone());
            let (client, client_driver): (capnp::capability::Client, _) = reproto::rpc::client(a);
            let schema = remote_loader.get(harness_id).unwrap();
            let client = dynamic::Client::new(client, schema.clone()).unwrap();
            let mut call = client.new_request("echo").unwrap();
            call.get()
                .unwrap()
                .set_named("value", Value::UInt32(41))
                .unwrap();
            assert!(matches!(
                call.send()
                    .unwrap()
                    .resolve()
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_named("value")
                    .unwrap(),
                Value::UInt32(42)
            ));
            let mut bounce = client.new_request("bounce").unwrap();
            bounce
                .get()
                .unwrap()
                .set_named(
                    "cap",
                    Value::Capability(dynamic::Client::new(host, schema.clone()).unwrap()),
                )
                .unwrap();
            let bounce = bounce.send().unwrap();
            let dynamic::PipelineValue::Capability(pipeline) = bounce.pipeline.get("cap").unwrap()
            else {
                panic!()
            };
            let mut call = pipeline.new_request("echo").unwrap();
            call.get()
                .unwrap()
                .set_named("value", Value::UInt32(99))
                .unwrap();
            assert!(matches!(
                call.send()
                    .unwrap()
                    .resolve()
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_named("value")
                    .unwrap(),
                Value::UInt32(100)
            ));
            let result = bounce.resolve().await.unwrap();
            let Value::Capability(cap) = result.get().unwrap().get_named("cap").unwrap() else {
                panic!()
            };
            drop(result);
            let mut call = cap.new_request("echo").unwrap();
            call.get()
                .unwrap()
                .set_named("value", Value::UInt32(6))
                .unwrap();
            assert!(matches!(
                call.send()
                    .unwrap()
                    .resolve()
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_named("value")
                    .unwrap(),
                Value::UInt32(7)
            ));
            let typed = reproto_test_support::runtime_test_capnp::harness::Client {
                client: cap.as_client().unwrap().clone(),
            };
            let mut call = typed.echo_request();
            call.get().set_value(8);
            assert_eq!(
                call.send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                9
            );
            catalog_server.abort();
            catalog_driver.abort();
            host_driver.abort();
            client_driver.abort();
        })
        .await;
}

fn malformed() -> message::Builder<message::HeapAllocator> {
    let mut m = version(1);
    let node::Struct(s) = m.get_root::<node::Builder>().unwrap().which().unwrap() else {
        unreachable!()
    };
    let capnp::schema_capnp::field::Slot(mut slot) =
        s.get_fields().unwrap().get(1).which().unwrap()
    else {
        unreachable!()
    };
    slot.set_offset(u32::MAX);
    m
}
#[test]
fn replay_tlc_schema_loader_traces() {
    let path = reproto_test_support::verification::input("REPROTO_SCHEMA_LOADER_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for trace in traces {
        let mut loader = SchemaLoader::default();
        for step in trace {
            let before = loader_state(&loader);
            let v1 = version(1);
            let v2 = version(2);
            let bad = malformed();
            let wrong = child(Kind::Enum);
            let good = child(Kind::Struct);
            let result = match step["event"].as_u64().unwrap() {
                1 => loader.load(v1.get_root_as_reader().unwrap()).map(|_| ()),
                2 => loader.load(v2.get_root_as_reader().unwrap()).map(|_| ()),
                3 => loader.load(good.get_root_as_reader().unwrap()).map(|_| ()),
                4 => loader.load_batch([
                    v1.get_root_as_reader().unwrap(),
                    wrong.get_root_as_reader().unwrap(),
                ]),
                5 => loader.load(bad.get_root_as_reader().unwrap()).map(|_| ()),
                6 => loader
                    .load_once(v1.get_root_as_reader().unwrap())
                    .map(|_| ()),
                7 => loader.load_batch([
                    v1.get_root_as_reader().unwrap(),
                    bad.get_root_as_reader().unwrap(),
                ]),
                _ => panic!("unknown event"),
            };
            let after = loader_state(&loader);
            let actual = [
                after.0,
                after.1,
                before.0,
                before.1,
                u64::from(result.is_err()),
            ];
            let expected: Vec<_> = step["state"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            assert_eq!(actual.as_slice(), expected, "{step}");
        }
    }
}
fn loader_state(loader: &SchemaLoader) -> (u64, u64) {
    (
        loader
            .try_get(1)
            .map_or(0, |s| s.fields().unwrap().len() as u64 - 1),
        loader.try_get(2).map_or(0, |s| {
            if s.is_stub() {
                1
            } else {
                assert_eq!(s.kind(), Kind::Struct);
                2
            }
        }),
    )
}

#[test]
fn pinned_cpp_loader_agrees_on_validation_and_version_selection() {
    let executable =
        reproto_test_support::verification::cpp::loader().expect("prepare verified trace corpus");
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut changed_default = version(2);
    let node::Struct(s) = changed_default
        .get_root::<node::Builder>()
        .unwrap()
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let capnp::schema_capnp::field::Slot(slot) = s.get_fields().unwrap().get(1).which().unwrap()
    else {
        unreachable!()
    };
    slot.init_default_value().set_uint32(99);
    let cases = [
        version(1),
        version(2),
        version(1),
        malformed(),
        changed_default,
        child(Kind::Struct),
        child(Kind::Enum),
    ];
    let mut input = message::Builder::new_default();
    let mut nodes = input
        .init_root::<code_generator_request::Builder>()
        .init_nodes(cases.len() as u32);
    let mut loader = SchemaLoader::default();
    let mut expected = String::new();
    for (i, m) in cases.iter().enumerate() {
        nodes
            .set_with_caveats(i as u32, m.get_root_as_reader().unwrap())
            .unwrap();
        match loader.load(m.get_root_as_reader().unwrap()) {
            Ok(s) => expected.push_str(&format!(
                "ok {} {} {}\n",
                s.id(),
                match s.kind() {
                    Kind::Struct => 1,
                    Kind::Enum => 2,
                    _ => panic!(),
                },
                s.fields().unwrap().len()
            )),
            Err(_) => expected.push_str("error\n"),
        }
    }
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&capnp::serialize::write_message_to_words(&input))
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn malformed_layouts_unions_defaults_and_cycles_leave_no_state() {
    type Mutation = Box<dyn Fn(&mut message::Builder<message::HeapAllocator>)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|m| {
            let node::Struct(mut s) = m.get_root::<node::Builder>().unwrap().which().unwrap()
            else {
                unreachable!()
            };
            s.set_pointer_count(0);
        }),
        Box::new(|m| {
            let node::Struct(mut s) = m.get_root::<node::Builder>().unwrap().which().unwrap()
            else {
                unreachable!()
            };
            s.set_discriminant_count(1);
        }),
        Box::new(|m| {
            let node::Struct(mut s) = m.get_root::<node::Builder>().unwrap().which().unwrap()
            else {
                unreachable!()
            };
            s.set_discriminant_count(2);
            s.set_discriminant_offset(u32::MAX);
        }),
        Box::new(|m| {
            let node::Struct(s) = m.get_root::<node::Builder>().unwrap().which().unwrap() else {
                unreachable!()
            };
            s.get_fields().unwrap().get(1).set_code_order(0);
        }),
        Box::new(|m| {
            let node::Struct(s) = m.get_root::<node::Builder>().unwrap().which().unwrap() else {
                unreachable!()
            };
            let capnp::schema_capnp::field::Slot(slot) =
                s.get_fields().unwrap().get(1).which().unwrap()
            else {
                unreachable!()
            };
            slot.init_default_value().set_text("wrong");
        }),
        Box::new(|m| {
            let node::Struct(s) = m.get_root::<node::Builder>().unwrap().which().unwrap() else {
                unreachable!()
            };
            s.get_fields().unwrap().get(1).set_name("child");
        }),
    ];
    for change in mutations {
        let mut m = version(1);
        change(&mut m);
        let mut loader = SchemaLoader::default();
        assert!(loader.load(m.get_root_as_reader().unwrap()).is_err());
        assert!(loader.is_empty());
    }
    let mut m = message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(1);
    n.init_interface().init_superclasses(1).get(0).set_id(1);
    let mut loader = SchemaLoader::default();
    assert!(loader.load(m.get_root_as_reader().unwrap()).is_err());
    assert!(loader.is_empty());
}

#[derive(Default)]
struct CallState {
    running: std::cell::Cell<bool>,
    completed: std::cell::Cell<bool>,
    gate: std::cell::RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
    allow: bool,
    early: bool,
    fail: bool,
}
struct Pending(std::rc::Rc<CallState>);
struct Running(std::rc::Rc<CallState>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}
impl dynamic::Server for Pending {
    fn allow_cancellation(&self) -> bool {
        self.0.allow
    }
    fn call(
        self: std::rc::Rc<Self>,
        mut context: dynamic::CallContext,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::from_future(async move {
            {
                let (params, mut results) = context.get()?;
                results.set_named("cap", params.get_named("cap")?)?;
            }
            context.release_params();
            let retained = if self.0.early { None } else { Some(context) };
            self.0.running.set(true);
            let _running = Running(self.0.clone());
            let gate = self.0.gate.borrow_mut().take().unwrap();
            gate.await
                .map_err(|_| capnp::Error::failed("gate gone".into()))?;
            self.0.completed.set(true);
            drop(retained);
            if self.0.fail {
                return Err(capnp::Error::failed("dynamic method failed".into()));
            }
            Ok(())
        })
    }
}
struct OwnedEcho(std::rc::Rc<std::cell::Cell<bool>>);
impl Drop for OwnedEcho {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl reproto_test_support::runtime_test_capnp::harness::Server for OwnedEcho {
    async fn echo(
        self: std::rc::Rc<Self>,
        _: reproto_test_support::runtime_test_capnp::harness::EchoParams,
        mut r: reproto_test_support::runtime_test_capnp::harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(42);
        Ok(())
    }
}
struct Drivers(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Drivers {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}
async fn loaded_call_trace(
    loader: &std::rc::Rc<SchemaLoader>,
    case: &serde_json::Value,
    wire: bool,
) {
    use std::{cell::Cell, rc::Rc};
    let state = Rc::new(CallState {
        allow: case["allow"].as_bool().unwrap(),
        early: case["early"].as_bool().unwrap(),
        fail: case["fail"].as_bool().unwrap(),
        ..Default::default()
    });
    let alive = Rc::new(Cell::new(true));
    let cap: reproto_test_support::runtime_test_capnp::harness::Client =
        capnp_rpc::new_client(OwnedEcho(alive.clone()));
    let (tx, rx) = futures::channel::oneshot::channel();
    let mut tx = Some(tx);
    *state.gate.borrow_mut() = Some(rx);
    let envelope = loader.get(id(loader, "Envelope")).unwrap();
    let f = envelope.field("cap").unwrap();
    let Type::Interface(schema) = f.get_type().unwrap() else {
        panic!()
    };
    let capnp::schema_capnp::field::Slot(slot) = f.get_proto().which().unwrap() else {
        panic!()
    };
    let capnp::schema_capnp::type_::Interface(ty) = slot.get_type().unwrap().which().unwrap()
    else {
        panic!()
    };
    let service = dynamic::ServiceSchema::new(loader.clone(), schema.id())
        .unwrap()
        .with_brand(ty.get_brand().unwrap())
        .unwrap();
    let (executor, driver) = capnp_rpc::new_call_executor();
    let server = capnp_rpc::new_loaded_client_from_rc(
        Rc::new(Pending(state.clone())),
        service,
        Some(executor),
    );
    let mut drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
    let client = if wire {
        let (a, b) = tokio::io::duplex(4096);
        drivers.0.push(reproto::rpc::serve(b, server));
        let (client, driver) = reproto::rpc::client::<capnp::capability::Client>(a);
        drivers.0.push(driver);
        client
    } else {
        server
    };
    let client = dynamic::Client::new(client, schema).unwrap();
    let mut req = client.new_request("pending").unwrap();
    req.get()
        .unwrap()
        .set_named(
            "cap",
            Value::Capability(
                dynamic::Client::new(cap, loader.get(id(loader, "Harness")).unwrap()).unwrap(),
            ),
        )
        .unwrap();
    let mut promise = Some(Box::pin(req.send().unwrap().resolve()));
    assert!(futures::poll!(promise.as_mut().unwrap()).is_pending());
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(state.running.get());
    assert!(alive.get());
    let mut response: Option<dynamic::Response<'_>> = None;
    let mut held: Option<dynamic::Client<'_>> = None;
    let mut outcome = 0;
    for step in case["steps"].as_array().unwrap() {
        match step["action"].as_str().unwrap() {
            "drop-caller" => {
                promise.take();
                response.take();
            }
            "complete" => {
                tx.take().unwrap().send(()).unwrap();
            }
            "extract" => {
                let Value::Capability(cap) = response
                    .as_ref()
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_named("cap")
                    .unwrap()
                else {
                    panic!()
                };
                held = Some(cap);
            }
            "drop-held" => {
                held.take();
            }
            "use" => {
                let result = held
                    .as_ref()
                    .unwrap()
                    .new_request("echo")
                    .unwrap()
                    .send()
                    .unwrap()
                    .resolve()
                    .await
                    .unwrap();
                assert!(matches!(
                    result.get().unwrap().get_named("value").unwrap(),
                    Value::UInt32(42)
                ));
            }
            _ => panic!(),
        }
        for _ in 0..64 {
            if let Some(p) = &mut promise {
                if let std::task::Poll::Ready(result) = futures::poll!(p) {
                    match result {
                        Ok(value) => {
                            response = Some(value);
                            outcome = 1;
                        }
                        Err(e) => {
                            assert!(e.extra.contains("dynamic method failed"));
                            outcome = 2;
                        }
                    }
                    promise.take();
                }
            }
            tokio::task::yield_now().await;
        }
        let expected = step["state"].as_array().unwrap();
        assert_eq!(state.completed.get(), expected[1] == 1, "{step}");
        assert_eq!(state.running.get(), expected[2] == 1, "{step}");
        assert_eq!(alive.get(), expected[3] == 1, "{step}, wire={wire}");
        assert_eq!(held.is_some(), expected[4] == 1, "{step}");
        if expected[0] == 1 && expected[1] == 1 {
            assert_eq!(outcome, if state.fail { 2 } else { 1 });
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_loaded_capability_traces() {
    let path = reproto_test_support::verification::input("REPROTO_LOADED_CAPABILITY_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let loader = std::rc::Rc::new(compiled());
            for case in cases {
                for wire in [false, true] {
                    loaded_call_trace(&loader, &case, wire).await;
                }
            }
        })
        .await;
}

fn list_schema(element_struct: bool, version: u16) -> message::Builder<message::HeapAllocator> {
    let mut m = message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(10);
    let mut s = n.init_struct();
    s.set_pointer_count(1);
    s.set_data_word_count(u16::from(version > 1));
    let mut fields = s.init_fields(u32::from(version));
    let mut f = fields.reborrow().get(0);
    f.set_name("values");
    let mut slot = f.init_slot();
    slot.reborrow().init_default_value().init_list();
    let mut element = slot.init_type().init_list().init_element_type();
    if element_struct {
        element.init_struct().set_type_id(11);
    } else {
        element.set_uint32(());
    }
    if version > 1 {
        let mut f = fields.get(1);
        f.set_name("extra");
        f.set_code_order(1);
        let mut slot = f.init_slot();
        slot.reborrow().init_type().set_bool(());
        slot.init_default_value().set_bool(false);
    }
    m
}
fn element_schema(offset: u32) -> message::Builder<message::HeapAllocator> {
    let mut m = message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(11);
    let mut s = n.init_struct();
    s.set_data_word_count(1);
    let mut f = s.init_fields(1).get(0);
    f.set_name("value");
    let mut slot = f.init_slot();
    slot.set_offset(offset);
    slot.reborrow().init_type().set_uint32(());
    slot.init_default_value().set_uint32(0);
    m
}
#[test]
fn list_struct_upgrades_retain_constraints_until_dependencies_arrive() {
    let mut loader = SchemaLoader::default();
    let old = list_schema(false, 1);
    let new = list_schema(true, 2);
    loader.load(old.get_root_as_reader().unwrap()).unwrap();
    loader.load(new.get_root_as_reader().unwrap()).unwrap();
    assert!(loader.get(11).unwrap().is_stub());
    let bad = element_schema(1);
    assert!(loader.load(bad.get_root_as_reader().unwrap()).is_err());
    let good = element_schema(0);
    loader.load(good.get_root_as_reader().unwrap()).unwrap();
    assert!(!loader.get(11).unwrap().is_stub());
    let schema = loader.get(10).unwrap();
    let mut message = message::Builder::new_default();
    let mut root = dynamic::Builder::init(message.init_root(), schema).unwrap();
    root.reborrow()
        .init_list("values", 1)
        .unwrap()
        .get_struct(0)
        .unwrap()
        .set_named("value", Value::UInt32(12))
        .unwrap();
    let Value::List(values) = root.as_reader().get_named("values").unwrap() else {
        panic!()
    };
    let Value::Struct(value) = values.get(0).unwrap() else {
        panic!()
    };
    assert!(matches!(
        value.get_named("value").unwrap(),
        Value::UInt32(12)
    ));
}

fn group_parent(version: u16) -> message::Builder<message::HeapAllocator> {
    let mut m = message::Builder::new_default();
    let mut n = m.init_root::<node::Builder>();
    n.set_id(10);
    let mut s = n.init_struct();
    s.set_data_word_count(1);
    let mut fields = s.init_fields(u32::from(version));
    let mut f = fields.reborrow().get(0);
    f.set_name("value");
    if version == 1 {
        let mut slot = f.init_slot();
        slot.reborrow().init_type().set_uint32(());
        slot.init_default_value().set_uint32(0);
    } else {
        f.init_group().set_type_id(11);
        let mut f = fields.get(1);
        f.set_name("extra");
        f.set_code_order(1);
        let mut slot = f.init_slot();
        slot.set_offset(1);
        slot.reborrow().init_type().set_uint32(());
        slot.init_default_value().set_uint32(0);
    }
    m
}
fn upgrade_element(group: bool, bad: bool) -> message::Builder<message::HeapAllocator> {
    let mut m = element_schema(u32::from(bad));
    if group {
        let mut n = m.get_root::<node::Builder>().unwrap();
        n.set_scope_id(10);
        let node::Struct(mut s) = n.which().unwrap() else {
            panic!()
        };
        s.set_is_group(true);
    }
    m
}
fn upgrade_state(loader: &SchemaLoader) -> (u64, u64) {
    let parent = loader
        .try_get(10)
        .filter(|s| !s.is_stub())
        .map_or(0, |s| s.fields().unwrap().len() as u64);
    let element = loader.try_get(11).map_or(0, |s| {
        if s.is_stub() {
            1
        } else {
            let f = s.fields().unwrap().remove(0);
            let capnp::schema_capnp::field::Slot(slot) = f.get_proto().which().unwrap() else {
                panic!()
            };
            2 + u64::from(slot.get_offset() != 0)
        }
    });
    (parent, element)
}
#[test]
fn replay_tlc_schema_upgrade_traces() {
    let path = reproto_test_support::verification::input("REPROTO_SCHEMA_UPGRADE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for group in [false, true] {
        for trace in &traces {
            let mut loader = SchemaLoader::default();
            for step in trace {
                let before = upgrade_state(&loader);
                let event = step["event"].as_u64().unwrap();
                let m = match event {
                    1 | 2 if group => group_parent(event as u16),
                    1 | 2 => list_schema(event == 2, event as u16),
                    3 | 4 => upgrade_element(group, event == 4),
                    _ => panic!(),
                };
                let failed = loader.load(m.get_root_as_reader().unwrap()).is_err();
                let after = upgrade_state(&loader);
                let actual = [after.0, after.1, before.0, before.1, u64::from(failed)];
                let expected: Vec<_> = step["state"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
                    .collect();
                assert_eq!(
                    actual.as_slice(),
                    expected,
                    "group={group}, {step}, trace={trace:?}"
                );
            }
        }
    }
}

#[test]
fn native_registration_gates_casts_and_capability_hints_follow_dependencies() {
    use reproto_test_support::dynamic_test_capnp::orphan_case;
    let mut loader = compiled();
    let schema_id = id(&loader, "OrphanCase");
    let mut m = message::Builder::new_default();
    let b = dynamic::Builder::init(m.init_root(), loader.get(schema_id).unwrap()).unwrap();
    assert!(b.downcast_native::<orphan_case::Owned>().is_err());
    loader
        .load_compiled_type_and_dependencies::<orphan_case::Owned>()
        .unwrap();
    let mut b = dynamic::Builder::new(m.get_root().unwrap(), loader.get(schema_id).unwrap())
        .unwrap()
        .downcast_native::<orphan_case::Owned>()
        .unwrap();
    b.set_flag(false);
    let r = dynamic::Reader::new(
        m.get_root_as_reader().unwrap(),
        loader.get(schema_id).unwrap(),
    )
    .unwrap();
    assert!(!r
        .downcast_native::<orphan_case::Owned>()
        .unwrap()
        .get_flag());
    assert!(loader
        .get(schema_id)
        .unwrap()
        .may_contain_capabilities()
        .unwrap());
    let plain = loader
        .get_all_loaded()
        .find(|s| {
            s.get_proto()
                .get_display_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with("Harness.PlainCycle")
        })
        .unwrap();
    assert!(!plain.may_contain_capabilities().unwrap());
    let mut isolated = SchemaLoader::default();
    isolated
        .load_compiled_type_and_dependencies::<orphan_case::Owned>()
        .unwrap();
    assert!(isolated
        .get(schema_id)
        .unwrap()
        .may_contain_capabilities()
        .unwrap());
    let mut lazy = SchemaLoader::default();
    assert_eq!(
        lazy.get_or_load(1, |loader, id| {
            assert_eq!(id, 1);
            loader
                .load(version(1).get_root_as_reader().unwrap())
                .map(|_| ())
        })
        .unwrap()
        .id(),
        1
    );
    lazy.get_or_load(1, |_, _| panic!("already loaded"))
        .unwrap();
}

#[test]
fn implicit_method_parameters_bind_both_request_and_result_types() {
    let output = std::process::Command::new("capnp")
        .args([
            "compile",
            "-o-",
            "--src-prefix=schemas",
            "schemas/loader-test.capnp",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let message =
        capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
            .unwrap();
    let mut loader = SchemaLoader::default();
    loader.load_request(message.get_root().unwrap()).unwrap();
    let method = loader
        .get(id(&loader, "Factory"))
        .unwrap()
        .method("exchange")
        .unwrap()
        .bind_implicit(&[Type::Text, Type::Data])
        .unwrap();
    assert!(matches!(
        method
            .params()
            .unwrap()
            .field("first")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Text
    ));
    assert!(matches!(
        method
            .params()
            .unwrap()
            .field("second")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Data
    ));
    assert!(matches!(
        method
            .results()
            .unwrap()
            .field("first")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Data
    ));
    assert!(matches!(
        method
            .results()
            .unwrap()
            .field("second")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Text
    ));
    assert!(method.bind_implicit(&[Type::UInt32, Type::Data]).is_err());
}
#[test]
fn future_type_discriminants_remain_inspectable_without_assuming_layout() {
    let mut m = message::Builder::new_default();
    m.init_root::<capnp::schema_capnp::type_::Builder>()
        .set_void(());
    let mut bytes = capnp::serialize::write_message_to_words(&m);
    // A single-segment Type message: framing, root pointer, then Type's tag.
    bytes[16..18].copy_from_slice(&999u16.to_le_bytes());
    let message =
        capnp::serialize::read_message(bytes.as_slice(), message::ReaderOptions::new()).unwrap();
    let ty = message
        .get_root::<capnp::schema_capnp::type_::Reader>()
        .unwrap();
    assert!(ty.which().is_err());
    let mut schema = message::Builder::new_default();
    let mut n = schema.init_root::<node::Builder>();
    n.set_id(123);
    let mut f = n.init_struct().init_fields(1).get(0);
    f.set_name("future");
    f.init_slot().set_type(ty).unwrap();
    let mut loader = SchemaLoader::default();
    let schema = loader.load(schema.get_root_as_reader().unwrap()).unwrap();
    let mut data = message::Builder::new_default();
    let mut builder = dynamic::Builder::init(data.init_root(), schema).unwrap();
    assert!(matches!(
        builder.as_reader().get_named("future").unwrap(),
        Value::Unknown(999)
    ));
    assert!(builder.set_named("future", Value::Unknown(999)).is_err());
    for mode in [dynamic::HasMode::NonNull, dynamic::HasMode::NonDefault] {
        assert!(!builder.has_named_with_mode("future", mode).unwrap());
        assert!(!builder
            .as_reader()
            .has_named_with_mode("future", mode)
            .unwrap());
    }
}

#[test]
fn future_type_with_unknown_default_can_be_loaded_repeatedly() {
    fn definition(type_tag: u16, default_tag: u16) -> message::Builder<message::HeapAllocator> {
        let mut message = message::Builder::new_default();
        let mut n = message.init_root::<node::Builder>();
        n.set_id(123);
        let mut s = n.init_struct();
        s.set_data_word_count(1);
        let mut f = s.init_fields(1).get(0);
        f.set_name("future");
        let mut slot = f.init_slot();
        capnp::any_struct::Builder::from_builder(slot.reborrow().init_type())
            .unwrap()
            .get_data_section()[..2]
            .copy_from_slice(&type_tag.to_le_bytes());
        capnp::any_struct::Builder::from_builder(slot.init_default_value())
            .unwrap()
            .get_data_section()[..2]
            .copy_from_slice(&default_tag.to_le_bytes());
        message
    }

    let mut loader = SchemaLoader::default();
    // Unknown defaults are opaque when their field type is also unknown. The
    // malformed-data seed sweep found that a second load used to decode them.
    for tag in [247, 247, u16::MAX] {
        let message = definition(999, tag);
        loader.load(message.get_root_as_reader().unwrap()).unwrap();
    }
    let schema = loader.get(123).unwrap();
    assert!(matches!(
        schema.field("future").unwrap().get_type().unwrap(),
        Type::Unknown(999)
    ));
    let mut empty = message::Builder::new_default();
    empty.init_root::<capnp::any_pointer::Builder>();
    let reader = dynamic::Reader::new(empty.get_root_as_reader().unwrap(), schema).unwrap();
    assert!(matches!(
        reader.get_named("future").unwrap(),
        Value::Unknown(999)
    ));

    let changed = definition(1000, 247);
    let error = loader
        .load(changed.get_root_as_reader().unwrap())
        .err()
        .unwrap();
    assert!(error.to_string().contains("unknown field type changed"));
    assert!(matches!(
        loader
            .get(123)
            .unwrap()
            .field("future")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Unknown(999)
    ));

    // A known field type still requires a known, matching default tag.
    let known = definition(8, 247); // UInt32 in schema.capnp.
    let mut fresh = SchemaLoader::default();
    let error = fresh
        .load(known.get_root_as_reader().unwrap())
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        capnp::ErrorKind::EnumValueOrUnionDiscriminantNotPresent(capnp::NotInSchema(247))
    );
    assert!(fresh.is_empty());
}

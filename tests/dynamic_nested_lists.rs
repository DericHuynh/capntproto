use capnp::{
    dynamic_list, dynamic_struct, dynamic_value,
    introspect::Introspect,
    message,
    schema_loader::{dynamic, Schema, SchemaLoader, Type},
};
use reproto_test_support::native_list_capnp::{self as fixture, nested_lists, service};

type Message = message::Builder<message::HeapAllocator>;
const FIELDS: &[&str] = &[
    "numbers", "flags", "choices", "records", "texts", "blobs", "caps", "voids", "triples",
];
fn loader() -> SchemaLoader {
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<nested_lists::Owned>()
        .unwrap();
    let mut loaded = SchemaLoader::default();
    loaded
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    loaded
}
fn schema(loader: &SchemaLoader) -> Schema<'_> {
    loader
        .get(
            nested_lists::Owned::introspect()
                .as_struct_schema()
                .unwrap()
                .get_proto()
                .get_id(),
        )
        .unwrap()
}
#[derive(Clone, Copy, Debug)]
enum Seed {
    Normal,
    Primitive,
    Short,
    Large,
    Bad,
}
fn seed(kind: Seed) -> Message {
    let mut message = Message::new_default();
    let mut root = message.init_root::<nested_lists::Builder>();
    // Two populated siblings, a null child, then an explicitly empty child.
    macro_rules! children {
        ($method:ident, $write:expr) => {{
            let mut outer = root.reborrow().$method(4);
            for i in 0..2 {
                let mut child = outer.reborrow().init(i, 2);
                ($write)(&mut child);
            }
            outer.init(3, 0);
        }};
    }
    children!(init_numbers, |c: &mut capnp::primitive_list::Builder<
        u32,
    >| {
        c.set(0, 11);
        c.set(1, 22);
    });
    children!(init_flags, |c: &mut capnp::primitive_list::Builder<
        bool,
    >| {
        c.set(0, false);
        c.set(1, true);
    });
    children!(init_choices, |c: &mut capnp::enum_list::Builder<
        fixture::Choice,
    >| {
        c.set(0, fixture::Choice::Zero);
        c.set(1, fixture::Choice::One);
    });
    children!(init_records, |c: &mut capnp::struct_list::Builder<
        fixture::item::Owned<capnp::text::Owned>,
    >| {
        for i in 0..2 {
            let mut r = c.reborrow().get(i);
            r.set_number(11 + i);
            r.set_payload("original").unwrap();
        }
    });
    children!(init_texts, |c: &mut capnp::text_list::Builder| {
        c.set(0, "alpha");
        c.set(1, "beta");
    });
    children!(init_blobs, |c: &mut capnp::data_list::Builder| {
        c.set(0, b"alpha");
        c.set(1, b"beta");
    });
    children!(init_caps, |_: &mut capnp::capability_list::Builder<
        service::Client,
    >| {});
    children!(init_voids, |_: &mut capnp::primitive_list::Builder<()>| {});
    children!(init_triples, |c: &mut capnp::list_list::Builder<
        capnp::primitive_list::Owned<u32>,
    >| {
        c.reborrow().init(0, 1).set(0, 7);
    });
    if !matches!(kind, Seed::Normal) {
        let mut raw = capnp::any_struct::Builder::from_builder(root).unwrap();
        let loader = loader();
        let capnp::schema_capnp::field::Slot(slot) = schema(&loader)
            .field("records")
            .unwrap()
            .get_proto()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let slot = slot.get_offset();
        let pointer = raw
            .get_pointer_section()
            .get(slot)
            .get_as::<capnp::any_pointer_list::Builder>()
            .unwrap()
            .get(0);
        match kind {
            Seed::Primitive => {
                let mut list = pointer.initn_as::<capnp::primitive_list::Builder<u32>>(2);
                list.set(0, 11);
                list.set(1, 12);
            }
            Seed::Short | Seed::Large => {
                let large = matches!(kind, Seed::Large);
                let mut list = pointer
                    .init_as_list_of_any_struct(
                        if large { 2 } else { 1 },
                        if large { 2 } else { 0 },
                        2,
                    )
                    .unwrap();
                for i in 0..2 {
                    let mut item = list.reborrow().get(i);
                    item.get_data_section()[..4].copy_from_slice(&(11 + i).to_le_bytes());
                    if large {
                        item.get_data_section()[8..16]
                            .copy_from_slice(&0x1122334455667788u64.to_le_bytes());
                        item.get_pointer_section()
                            .get(0)
                            .set_as::<capnp::text::Owned>("original")
                            .unwrap();
                        item.get_pointer_section()
                            .get(1)
                            .set_as::<capnp::text::Owned>("unknown retained")
                            .unwrap();
                    }
                }
            }
            Seed::Bad => {
                pointer
                    .initn_as::<capnp::primitive_list::Builder<bool>>(2)
                    .set(0, true);
            }
            Seed::Normal => unreachable!(),
        }
    }
    message
}
fn wire(message: &Message) -> Vec<u8> {
    capnp::Word::words_to_bytes(
        &capnp::any_struct::Reader::from_reader(
            message
                .get_root_as_reader::<nested_lists::Reader>()
                .unwrap(),
        )
        .canonicalize()
        .unwrap(),
    )
    .to_vec()
}
#[derive(Debug)]
struct Case {
    seed: Seed,
    action: &'static str,
    path: String,
    length: Option<u32>,
}
fn cases() -> Vec<Case> {
    let mut cases = vec![];
    for field in FIELDS {
        for index in [0, 1, 2, 3, 4, u32::MAX] {
            cases.push(Case {
                seed: Seed::Normal,
                action: "get",
                path: format!("{field}/{index}"),
                length: match index {
                    0 | 1 => Some(2),
                    2 | 3 => Some(0),
                    _ => None,
                },
            });
        }
    }
    for (action, path, length) in [
        ("number", "numbers/0", Some(2)),
        ("flag", "flags/0", Some(2)),
        ("enum", "choices/0", Some(2)),
        ("record", "records/0", Some(2)),
        ("text", "texts/0", Some(2)),
        ("data", "blobs/0", Some(2)),
        ("void", "voids/0", Some(2)),
        ("number", "triples/0/0", Some(1)),
        ("get", "triples/0/1", Some(0)),
        ("get", "numbers/0/0", None),
        ("get", "records/0/0", None),
    ] {
        cases.push(Case {
            seed: Seed::Normal,
            action,
            path: path.into(),
            length,
        });
    }
    for seed in [Seed::Primitive, Seed::Short, Seed::Large, Seed::Bad] {
        for action in ["get", "record"] {
            cases.push(Case {
                seed,
                action,
                path: "records/0".into(),
                length: if matches!(seed, Seed::Bad) {
                    None
                } else {
                    Some(2)
                },
            });
        }
    }
    cases
}
fn apply(message: &mut Message, loader: &SchemaLoader, case: &Case) -> capnp::Result<u32> {
    let mut path = case.path.split('/');
    let root = dynamic::Builder::new(message.get_root()?, schema(loader))?;
    let mut list = root.get_list(path.next().unwrap())?;
    for index in path {
        list = list.get_list(index.parse().unwrap())?;
    }
    let len = list.len();
    match case.action {
        "number" => list.set(0, dynamic::Value::UInt32(99))?,
        "flag" => list.set(0, dynamic::Value::Bool(true))?,
        "enum" => {
            let Type::Enum(schema) = list.as_reader().element_type() else {
                panic!()
            };
            list.set(0, dynamic::Value::Enum(1, schema))?;
        }
        "text" => list.set(0, dynamic::Value::Text("updated".into()))?,
        "data" => list.set(0, dynamic::Value::Data(b"updated"))?,
        "void" => list.set(0, dynamic::Value::Void)?,
        "record" => {
            let mut record = list.get_struct(0)?;
            assert!(matches!(
                record.schema().field("payload")?.get_type()?,
                Type::Text
            ));
            record.set_named("payload", dynamic::Value::Text("updated".into()))?;
        }
        "get" => (),
        _ => panic!(),
    }
    Ok(len)
}
fn compiled(message: &mut Message, case: &Case) -> capnp::Result<u32> {
    let mut path = case.path.split('/');
    let root = dynamic_value::Builder::from(message.get_root::<nested_lists::Builder>()?)
        .downcast::<dynamic_struct::Builder>();
    let mut list = root
        .get_named(path.next().unwrap())?
        .downcast::<dynamic_list::Builder>();
    for index in path {
        // Compiled indexing asserts bounds; only valid indices reach it here.
        let index = index.parse().unwrap();
        if index >= list.len() {
            return Err(capnp::Error::failed("bounds".into()));
        }
        let dynamic_value::Builder::List(child) = list.get(index)? else {
            return Err(capnp::Error::failed("type".into()));
        };
        list = child;
    }
    let len = list.len();
    match case.action {
        "number" => list.set(0, 99u32.into())?,
        "flag" => list.set(0, true.into())?,
        "enum" => list.set(0, fixture::Choice::One.into())?,
        "text" => list.set(0, dynamic_value::Reader::Text("updated".into()))?,
        "data" => list.set(0, dynamic_value::Reader::Data(b"updated"))?,
        "void" => list.set(0, dynamic_value::Reader::Void)?,
        "record" => list
            .get(0)?
            .downcast::<dynamic_struct::Builder>()
            .set_named("payload", dynamic_value::Reader::Text("updated".into()))?,
        "get" => (),
        _ => panic!(),
    }
    Ok(len)
}
#[test]
fn nested_getters_match_compiled_and_preserve_existing_storage() {
    let loader = loader();
    for case in cases() {
        let mut loaded = seed(case.seed);
        let before = capnp::serialize::write_message_to_words(&loaded);
        let result = apply(&mut loaded, &loader, &case);
        assert_eq!(
            result.as_ref().ok().copied(),
            case.length,
            "{case:?}: {result:?}"
        );
        if result.is_err()
            || (case.action == "get" && matches!(case.seed, Seed::Normal | Seed::Large))
        {
            assert_eq!(
                capnp::serialize::write_message_to_words(&loaded),
                before,
                "{case:?}"
            );
        }
        let mut native = seed(case.seed);
        assert_eq!(compiled(&mut native, &case).ok(), case.length, "{case:?}");
        assert_eq!(wire(&loaded), wire(&native), "{case:?}");
    }
}
#[test]
fn nested_struct_getters_upgrade_and_keep_unknown_fields_and_siblings() {
    let loader = loader();
    for kind in [Seed::Normal, Seed::Primitive, Seed::Short, Seed::Large] {
        let mut message = seed(kind);
        let case = Case {
            seed: kind,
            action: "record",
            path: "records/0".into(),
            length: Some(2),
        };
        apply(&mut message, &loader, &case).unwrap();
        let root = message
            .get_root_as_reader::<nested_lists::Reader>()
            .unwrap();
        let lists = root.get_records().unwrap();
        let child = lists.get(0).unwrap();
        assert_eq!(child.get(0).get_number(), 11);
        assert_eq!(child.get(0).get_payload().unwrap(), "updated");
        assert_eq!(child.get(1).get_number(), 12);
        assert_eq!(
            child.get(1).get_payload().unwrap(),
            if matches!(kind, Seed::Primitive | Seed::Short) {
                ""
            } else {
                "original"
            }
        );
        assert_eq!(
            lists.get(1).unwrap().get(0).get_payload().unwrap(),
            "original"
        );
        if matches!(kind, Seed::Large) {
            for i in 0..2 {
                let raw = capnp::any_struct::Reader::from_reader(child.get(i));
                assert_eq!(
                    &raw.get_data_section()[8..16],
                    &0x1122334455667788u64.to_le_bytes()
                );
                assert_eq!(
                    raw.get_pointer_section()
                        .get(1)
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "unknown retained"
                );
            }
        }
    }
}
#[test]
fn nested_getters_preserve_capability_ownership() {
    use capnp::traits::ImbueMut;
    use std::{cell::Cell, rc::Rc};
    struct Server(Rc<Cell<u32>>);
    impl service::Server for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let loader = loader();
    let drops = Rc::new(Cell::new(0));
    let client: service::Client = capnp_rpc::new_client(Server(drops.clone()));
    let identity = client.client.hook.get_ptr();
    let mut message = seed(Seed::Normal);
    let mut table = Vec::new();
    {
        let mut root = message.get_root::<nested_lists::Builder>().unwrap();
        root.imbue_mut(&mut table);
        root.get_caps()
            .unwrap()
            .get(0)
            .unwrap()
            .set(0, client.client.hook);
    }
    let before = capnp::serialize::write_message_to_words(&message);
    let retained = {
        let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
        pointer.imbue_mut(&mut table);
        let root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
        let mut outer = root.get_list("caps").unwrap();
        assert!(outer.reborrow().get_list(u32::MAX).is_err());
        let mut inner = outer.get_list(0).unwrap();
        assert!(inner.reborrow().get_list(0).is_err());
        let dynamic::Value::Capability(client) = inner.as_reader().get(0).unwrap() else {
            panic!()
        };
        assert_eq!(client.as_client().unwrap().hook.get_ptr(), identity);
        client
    };
    assert_eq!(table.len(), 1);
    assert_eq!(table[0].as_ref().unwrap().get_ptr(), identity);
    assert_eq!(capnp::serialize::write_message_to_words(&message), before);
    {
        let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
        pointer.imbue_mut(&mut table);
        dynamic::Builder::new(pointer, schema(&loader))
            .unwrap()
            .get_list("caps")
            .unwrap()
            .init_list(0, 0)
            .unwrap();
    }
    assert!(table[0].is_none());
    assert_eq!(drops.get(), 0, "extracted client keeps the server alive");
    drop(retained);
    assert_eq!(drops.get(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn nested_getters_match_pinned_cpp() {
    use reproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/dynamic-nested-lists");
    std::fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("nested-lists");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/dynamic-nested-lists.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&binary),
        &logs.join("build.log"),
        0,
    )
    .unwrap();
    let request = command(build.join("c++/src/capnp/capnp"))
        .args([
            "compile",
            "-o-",
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/native-list.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        request.status.success(),
        "{}",
        String::from_utf8_lossy(&request.stderr)
    );
    let schema_path = logs.join("schema.bin");
    std::fs::write(&schema_path, request.stdout).unwrap();
    let loader = loader();
    let cases = cases();
    for (i, case) in cases.iter().enumerate() {
        let mut message = seed(case.seed);
        let seed_path = logs.join(format!("{i}.seed"));
        std::fs::write(
            &seed_path,
            capnp::serialize::write_message_to_words(&message),
        )
        .unwrap();
        let expected = run(
            command(&binary)
                .arg(&schema_path)
                .arg(schema(&loader).id().to_string())
                .arg(&seed_path)
                .arg(case.action)
                .arg(&case.path),
            &logs.join(format!("{i}.out")),
            0,
        )
        .unwrap();
        let result = apply(&mut message, &loader, case);
        let status = match result {
            Ok(n) => format!("ok {n}"),
            Err(_) => "error".into(),
        };
        let hex: String = wire(&message).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(format!("{status} {hex}\n"), expected, "{case:?}");
    }
    std::fs::write(
        logs.join("summary.txt"),
        format!(
            "{} nested-list results, lengths and wire states match pinned C++ ({} rejected).\n",
            cases.len(),
            cases.iter().filter(|c| c.length.is_none()).count()
        ),
    )
    .unwrap();
}

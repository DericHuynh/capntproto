use capnp::{
    dynamic_struct, dynamic_value,
    introspect::Introspect,
    message::{Builder as Message, HeapAllocator},
    schema_capnp::{field, node},
    schema_loader::{dynamic, Schema, SchemaLoader, Type},
};
use capntproto_test_support::presence_capnp::mutable_access;

const TARGETS: &[(&str, bool)] = &[
    ("child", false),
    ("nums", true),
    ("children", true),
    ("group", false),
    ("branded", false),
    ("nested", true),
    ("caps", true),
    ("outside", false),
    ("plainNums", true),
];
fn native_schema() -> capnp::schema::StructSchema {
    mutable_access::Owned::introspect()
        .as_struct_schema()
        .unwrap()
}
fn loader() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<mutable_access::Owned>()
        .unwrap();
    loader
}
fn schema(loader: &SchemaLoader) -> Schema<'_> {
    loader.get(native_schema().get_proto().get_id()).unwrap()
}
fn tag(name: &str) -> u16 {
    native_schema()
        .get_field_by_name(name)
        .unwrap()
        .get_proto()
        .get_discriminant_value()
}
fn set_tag(message: &mut Message<HeapAllocator>, tag: u16) {
    let node::Struct(layout) = native_schema().get_proto().which().unwrap() else {
        panic!()
    };
    let offset = layout.get_discriminant_offset() as usize * 2;
    capnp::any_struct::Builder::from_builder(
        message.get_root::<mutable_access::Builder>().unwrap(),
    )
    .unwrap()
    .get_data_section()[offset..offset + 2]
        .copy_from_slice(&tag.to_le_bytes());
}
fn seed(stored: &str, populated: bool, tag: u16) -> Message<HeapAllocator> {
    let mut message = Message::new_default();
    let mut root = message.init_root::<mutable_access::Builder>();
    root.set_marker(0x12345678);
    if populated {
        match stored {
            "child" => {
                let mut p = root.init_child();
                p.set_value(91);
                p.set_text("stored-child");
            }
            "nums" => {
                let mut p = root.init_nums(2);
                p.set(0, 21);
                p.set(1, 22);
            }
            "children" => {
                root.init_children(1).get(0).set_value(93);
            }
            "group" => {
                let mut p = root.init_group();
                p.set_count(99);
                p.init_item().set_text("stored-group");
            }
            "branded" => {
                root.init_branded()
                    .init_body()
                    .set_value("stored-brand")
                    .unwrap();
            }
            "nested" => {
                root.init_nested(1).init(0, 1).set(0, 95);
            }
            "caps" => {
                root.init_caps(1);
            }
            _ => unreachable!(),
        }
    }
    set_tag(&mut message, tag);
    message
}
fn wire(message: &Message<HeapAllocator>) -> Vec<u8> {
    let root = message
        .get_root_as_reader::<mutable_access::Reader>()
        .unwrap();
    capnp::Word::words_to_bytes(
        &capnp::any_struct::Reader::from_reader(root)
            .canonicalize()
            .unwrap(),
    )
    .to_vec()
}
#[derive(Debug)]
struct Case {
    stored: &'static str,
    populated: bool,
    tag: u16,
    target: &'static str,
    list: bool,
}
impl Case {
    fn accepts(&self) -> bool {
        tag(self.target) == field::NO_DISCRIMINANT || tag(self.target) == self.tag
    }
    fn message(&self) -> Message<HeapAllocator> {
        seed(self.stored, self.populated, self.tag)
    }
}
fn cases() -> Vec<Case> {
    let mut cases = vec![];
    for &(target, list) in &TARGETS[..7] {
        for populated in [false, true] {
            for tag in [0, tag(target), u16::MAX] {
                cases.push(Case {
                    stored: target,
                    populated,
                    tag,
                    target,
                    list,
                });
            }
        }
        // Retained storage of a different pointer kind must not be inspected.
        let stored = if target == "nums" { "child" } else { "nums" };
        cases.push(Case {
            stored,
            populated: true,
            tag: tag(stored),
            target,
            list,
        });
    }
    for &(target, list) in &TARGETS[7..] {
        for tag in [0, u16::MAX] {
            cases.push(Case {
                stored: "",
                populated: false,
                tag,
                target,
                list,
            });
        }
    }
    cases
}
fn get(root: dynamic::Builder<'_, '_>, target: &str, list: bool) -> capnp::Result<()> {
    if list {
        root.get_list(target).map(|_| ())
    } else {
        root.get_struct(target).map(|_| ())
    }
}

#[test]
fn loaded_mutable_getters_match_compiled_results_and_preserve_rejected_messages() {
    let loader = loader();
    for case in cases() {
        let mut message = case.message();
        let before = capnp::serialize::write_message_to_words(&message);
        let result = get(
            dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap(),
            case.target,
            case.list,
        );
        assert_eq!(result.is_ok(), case.accepts(), "{case:?}: {result:?}");
        if !case.accepts() {
            assert!(result.unwrap_err().to_string().contains("inactive union"));
            assert_eq!(
                capnp::serialize::write_message_to_words(&message),
                before,
                "{case:?}"
            );
        }
        let mut compiled = case.message();
        let root =
            dynamic_value::Builder::from(compiled.get_root::<mutable_access::Builder>().unwrap())
                .downcast::<dynamic_struct::Builder>();
        assert_eq!(
            root.get_named(case.target).is_ok(),
            case.accepts(),
            "{case:?}"
        );
        assert_eq!(wire(&message), wire(&compiled), "{case:?}");
    }
}

#[test]
fn active_getters_materialize_defaults_and_return_writable_branded_views() {
    let loader = loader();
    let mut message = seed("child", false, tag("child"));
    let mut root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    assert!(!root.has_named("child").unwrap());
    let mut child = root.reborrow().get_struct("child").unwrap();
    assert!(matches!(
        child.as_reader().get_named("value").unwrap(),
        dynamic::Value::UInt32(17)
    ));
    child
        .set_named("value", dynamic::Value::UInt32(23))
        .unwrap();
    assert!(root.has_named("child").unwrap());
    let mutable_access::Child(child) = message
        .get_root_as_reader::<mutable_access::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(child.unwrap().get_value(), 23);

    let mut message = seed("nums", false, tag("nums"));
    let mut root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    assert!(!root.has_named("nums").unwrap());
    let mut nums = root.reborrow().get_list("nums").unwrap();
    assert_eq!(nums.len(), 2);
    assert!(matches!(
        nums.as_reader().get(0).unwrap(),
        dynamic::Value::UInt16(3)
    ));
    nums.set(1, dynamic::Value::UInt16(29)).unwrap();
    assert!(root.has_named("nums").unwrap());
    let mutable_access::Nums(nums) = message
        .get_root_as_reader::<mutable_access::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(nums.unwrap().get(1), 29);

    let mut message = seed("branded", false, tag("branded"));
    let root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    let mut body = root
        .get_struct("branded")
        .unwrap()
        .get_struct("body")
        .unwrap();
    assert!(matches!(
        body.schema().field("value").unwrap().get_type().unwrap(),
        Type::Text
    ));
    body.set_named("value", dynamic::Value::Text("written".into()))
        .unwrap();
    let mutable_access::Branded(branded) = message
        .get_root_as_reader::<mutable_access::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(branded.unwrap().get_body().get_value().unwrap(), "written");

    let mut message = seed("group", true, 0);
    let mut root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    assert!(root.reborrow().get_struct("group").is_err());
    let group = root.reborrow().group("group").unwrap();
    assert!(matches!(
        group.as_reader().get_named("count").unwrap(),
        dynamic::Value::UInt32(99)
    ));
    assert!(root.get_struct("group").is_ok());
}

#[test]
fn rejected_getters_cannot_expose_or_release_retained_capability_owners() {
    use capnp::traits::ImbueMut;
    use capntproto_test_support::runtime_test_capnp::harness;
    use std::{cell::Cell, rc::Rc};
    struct Server(Rc<Cell<usize>>);
    impl harness::Server for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let loader = loader();
    for &unknown in &[false, true] {
        let drops = Rc::new(Cell::new(0));
        let client: harness::Client = capnp_rpc::new_client(Server(drops.clone()));
        let mut message = Message::new_default();
        let mut caps = capnp::private::layout::CapTable::default();
        {
            let mut root = message.init_root::<mutable_access::Builder>();
            root.imbue_mut(&mut caps);
            root.reborrow()
                .init_cap()
                .set_as_capability(client.client.hook);
            root.set_none(());
        }
        if unknown {
            set_tag(&mut message, u16::MAX);
        }
        let before = capnp::serialize::write_message_to_words(&message);
        let owners: Vec<_> = caps
            .iter()
            .map(|c| c.as_ref().map(|c| c.get_ptr()))
            .collect();
        for &(target, list) in &TARGETS[..7] {
            let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut caps);
            let error = get(
                dynamic::Builder::new(pointer, schema(&loader)).unwrap(),
                target,
                list,
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("inactive union"),
                "{target}: {error}"
            );
            assert_eq!(capnp::serialize::write_message_to_words(&message), before);
            assert_eq!(
                caps.iter()
                    .map(|c| c.as_ref().map(|c| c.get_ptr()))
                    .collect::<Vec<_>>(),
                owners
            );
            assert_eq!(drops.get(), 0);
        }
        let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
        pointer.imbue_mut(&mut caps);
        dynamic::Builder::new(pointer, schema(&loader))
            .unwrap()
            .clear_named("cap")
            .unwrap();
        assert_eq!(drops.get(), 1);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn mutable_getters_match_pinned_cpp() {
    use capntproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/dynamic-getters");
    std::fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("getters");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/dynamic-groups.c++",
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
            "schemas/presence.capnp",
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
        let mut message = case.message();
        let seed_path = logs.join(format!("{i}.seed"));
        let input_path = logs.join(format!("{i}.input"));
        std::fs::write(
            &seed_path,
            capnp::serialize::write_message_to_words(&message),
        )
        .unwrap();
        std::fs::write(
            &input_path,
            format!(
                "{} {}\n",
                if case.list { "get-list" } else { "get-struct" },
                case.target
            ),
        )
        .unwrap();
        let expected = run(
            command(&binary)
                .arg(&schema_path)
                .arg(schema(&loader).id().to_string())
                .arg(&seed_path)
                .arg(&input_path)
                .arg("--report-errors"),
            &logs.join(format!("{i}.out")),
            0,
        )
        .unwrap();
        let result = get(
            dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap(),
            case.target,
            case.list,
        );
        assert_eq!(result.is_ok(), case.accepts(), "{case:?}");
        let bytes: String = wire(&message).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            format!("{} {bytes}\n", if result.is_ok() { "ok" } else { "error" }),
            expected,
            "{case:?}"
        );
    }
    std::fs::write(
        logs.join("summary.txt"),
        format!(
            "{} mutable-getter results and canonical wire states match pinned C++ ({} rejected).\n",
            cases.len(),
            cases.iter().filter(|c| !c.accepts()).count()
        ),
    )
    .unwrap();
}

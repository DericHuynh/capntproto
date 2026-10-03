use capnp::{
    dynamic_struct, dynamic_value,
    introspect::Introspect,
    message::{Builder as Message, HeapAllocator},
    schema_loader::{dynamic, Schema, SchemaLoader, Type},
};
use capntproto_test_support::presence_capnp::group_reset;

fn seed() -> Message<HeapAllocator> {
    let mut message = Message::new_default();
    let mut root = message.init_root::<group_reset::Builder>();
    root.set_marker(17);
    let mut body = root.reborrow().init_body();
    body.set_count(23);
    body.set_text("old-body");
    body.set_pointer("hidden-body");
    body.set_wide(u64::MAX);
    let mut nested = body.init_nested();
    nested.set_note("old-nested");
    nested.set_object("hidden-nested");
    nested.set_other(u64::MAX);
    let mut picked = root.reborrow().init_picked();
    picked.set_value(77);
    picked.set_note("old-picked");
    picked.init_inner().set_flag(false);
    root.set_idle(());
    let mut branded = root.init_branded().init_body();
    branded.set_value("old-branded").unwrap();
    branded.set_count(99);
    message
}

fn schema(loader: &SchemaLoader) -> Schema<'_> {
    loader
        .get(
            group_reset::Owned::introspect()
                .as_struct_schema()
                .unwrap()
                .get_proto()
                .get_id(),
        )
        .unwrap()
}
fn loader() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<group_reset::Owned>()
        .unwrap();
    loader
}
fn wire(message: &Message<HeapAllocator>) -> Vec<u8> {
    let root = message.get_root_as_reader::<group_reset::Reader>().unwrap();
    capnp::Word::words_to_bytes(
        &capnp::any_struct::Reader::from_reader(root)
            .canonicalize()
            .unwrap(),
    )
    .to_vec()
}

fn compiled<'a>(
    mut root: dynamic_struct::Builder<'a>,
    action: &str,
    path: &str,
) -> capnp::Result<()> {
    if let Some((parent, child)) = path.split_once('.') {
        return compiled(root.get_named(parent)?.downcast(), action, child);
    }
    match action {
        "init" => {
            root.init_named(path)?;
        }
        "clear" => root.clear_named(path)?,
        "get" => {
            root.get_named(path)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
fn loaded(mut root: dynamic::Builder<'_, '_>, action: &str, path: &str) -> capnp::Result<()> {
    if let Some((parent, child)) = path.split_once('.') {
        return loaded(root.get_struct(parent)?, action, child);
    }
    match action {
        "init" => {
            root.init_struct(path)?;
        }
        "clear" => root.clear_named(path)?,
        "get" => {
            root.get_struct(path)?;
        }
        "group" => {
            root.group(path)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn traces() -> Vec<Vec<(&'static str, &'static str)>> {
    let mut cases = vec![];
    for path in [
        "body",
        "body.nested",
        "body.nested.defaults",
        "picked",
        "branded.body",
    ] {
        for action in ["clear", "init"] {
            cases.push(vec![(action, path), (action, path)]);
        }
    }
    cases.extend([
        vec![
            ("get", "body"),
            ("get", "body.nested"),
            ("get", "branded.body"),
        ],
        vec![
            ("clear", "picked"),
            ("get", "picked.inner"),
            ("init", "picked.inner"),
        ],
        vec![
            ("clear", "body.nested.defaults"),
            ("clear", "body"),
            ("init", "picked"),
        ],
        vec![
            ("init", "body"),
            ("init", "body.nested"),
            ("clear", "body.nested.defaults"),
        ],
        vec![("init", "picked"), ("clear", "idle"), ("clear", "picked")],
        vec![
            ("clear", "body.pointer"),
            ("clear", "body"),
            ("init", "body.nested"),
        ],
    ]);
    cases
}

#[test]
fn loaded_group_resets_match_compiled_reflection() {
    let loader = loader();
    for trace in traces() {
        let mut actual = seed();
        let mut expected = seed();
        for (action, path) in trace {
            let root = dynamic::Builder::new(actual.get_root().unwrap(), schema(&loader)).unwrap();
            loaded(root, action, path).unwrap();
            let root =
                dynamic_value::Builder::from(expected.get_root::<group_reset::Builder>().unwrap())
                    .downcast();
            compiled(root, action, path).unwrap();
            assert_eq!(wire(&actual), wire(&expected), "{action} {path}");
        }
    }
}

#[test]
fn group_initialization_resets_defaults_but_views_preserve_values_and_brands() {
    let loader = loader();
    let mut message = seed();
    let before = wire(&message);
    for action in ["get", "group"] {
        loaded(
            dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap(),
            action,
            "body",
        )
        .unwrap();
        assert_eq!(wire(&message), before);
    }
    let mut root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    let mut body = root.reborrow().init_struct("body").unwrap();
    assert_eq!(
        body.as_reader()
            .which()
            .unwrap()
            .unwrap()
            .get_proto()
            .get_name()
            .unwrap(),
        "none"
    );
    assert!(matches!(
        body.as_reader().get_named("count").unwrap(),
        dynamic::Value::UInt32(7)
    ));
    // init returns a writable view into the existing parent.
    body.set_named("count", dynamic::Value::UInt32(31)).unwrap();
    let mut branded = root
        .reborrow()
        .get_struct("branded")
        .unwrap()
        .init_struct("body")
        .unwrap();
    assert!(matches!(
        branded.schema().field("value").unwrap().get_type().unwrap(),
        Type::Text
    ));
    branded
        .set_named("value", dynamic::Value::Text("new-branded".into()))
        .unwrap();
    root.reborrow().init_struct("picked").unwrap();
    let typed = message.get_root_as_reader::<group_reset::Reader>().unwrap();
    assert_eq!(typed.get_marker(), 17);
    let body = typed.get_body();
    assert_eq!(body.get_count(), 31);
    assert_eq!(body.get_text().unwrap(), "body");
    let nested = body.get_nested();
    assert_eq!(nested.get_note().unwrap(), "nested");
    let group_reset::body::nested::Defaults(defaults) = nested.which().unwrap() else {
        panic!()
    };
    assert_eq!(defaults.get_small(), 5);
    assert!(defaults.get_flag());
    let group_reset::Picked(picked) = typed.which().unwrap() else {
        panic!()
    };
    assert_eq!(picked.get_value(), 42);
    assert_eq!(picked.get_note().unwrap(), "picked");
    assert!(picked.get_inner().get_flag());
    assert_eq!(
        typed.get_branded().unwrap().get_body().get_value().unwrap(),
        "new-branded"
    );
    assert_eq!(typed.get_branded().unwrap().get_body().get_count(), 42);
}

#[cfg(target_os = "linux")]
#[test]
fn group_resets_match_pinned_cpp() {
    use capntproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/dynamic-groups");
    std::fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("groups");
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
    let seed_path = logs.join("seed.bin");
    std::fs::write(
        &seed_path,
        capnp::serialize::write_message_to_words(&seed()),
    )
    .unwrap();
    let loader = loader();
    let mut count = 0;
    for (i, trace) in traces().iter().enumerate() {
        let input = logs.join(format!("{i}.input"));
        std::fs::write(
            &input,
            trace
                .iter()
                .map(|(a, p)| format!("{a} {p}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let expected = run(
            command(&binary)
                .arg(&schema_path)
                .arg(schema(&loader).id().to_string())
                .arg(&seed_path)
                .arg(&input),
            &logs.join(format!("{i}.out")),
            0,
        )
        .unwrap();
        let mut message = seed();
        let mut actual = String::new();
        for (action, path) in trace {
            loaded(
                dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap(),
                action,
                path,
            )
            .unwrap();
            actual.push_str(
                &wire(&message)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            );
            actual.push('\n');
            count += 1;
        }
        assert_eq!(actual, expected, "trace {i}: {trace:?}");
    }
    std::fs::write(logs.join("summary.txt"), format!("{} traces; {count} group initialization/clearing/view observations match pinned C++ canonical wire.\n", traces().len())).unwrap();
}

#[test]
fn group_adoption_still_erases_inactive_storage_recursively() {
    let loader = loader();
    let mut message = seed();
    let root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    let Type::Struct(body) = root.schema().field("body").unwrap().get_type().unwrap() else {
        panic!()
    };
    let (mut root, token) = root.with_orphanage();
    let empty = token.in_struct(&mut root).unwrap().new_group(body).unwrap();
    root.adopt_named("body", empty).unwrap();
    let bytes = wire(&message);
    for stale in [b"hidden-body".as_slice(), b"hidden-nested"] {
        assert!(
            !bytes.windows(stale.len()).any(|w| w == stale),
            "retained {stale:?}"
        );
    }
    let root = message.get_root_as_reader::<group_reset::Reader>().unwrap();
    assert_eq!(root.get_marker(), 17);
    assert_eq!(root.get_body().get_count(), 7);
    assert_eq!(root.get_body().get_nested().get_note().unwrap(), "nested");
    assert_eq!(
        root.get_branded().unwrap().get_body().get_value().unwrap(),
        "old-branded"
    );
}

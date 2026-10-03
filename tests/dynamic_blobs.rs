use capnp::{
    dynamic_list, dynamic_struct, dynamic_value,
    introspect::Introspect,
    message,
    schema_capnp::{field, node},
    schema_loader::{dynamic, Schema, SchemaLoader},
};
use capntproto_test_support::presence_capnp::{access_capability, blob_access};

type Message = message::Builder<message::HeapAllocator>;
fn native_schema() -> capnp::schema::StructSchema {
    blob_access::Owned::introspect().as_struct_schema().unwrap()
}
fn loader() -> SchemaLoader {
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<blob_access::Owned>()
        .unwrap();
    let mut loaded = SchemaLoader::default();
    loaded
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    loaded
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
fn slot(name: &str) -> u32 {
    let field::Slot(slot) = native_schema()
        .get_field_by_name(name)
        .unwrap()
        .get_proto()
        .which()
        .unwrap()
    else {
        panic!()
    };
    slot.get_offset()
}
fn set_tag(message: &mut Message, tag: u16) {
    let node::Struct(layout) = native_schema().get_proto().which().unwrap() else {
        panic!()
    };
    let offset = layout.get_discriminant_offset() as usize * 2;
    capnp::any_struct::Builder::from_builder(message.get_root::<blob_access::Builder>().unwrap())
        .unwrap()
        .get_data_section()[offset..offset + 2]
        .copy_from_slice(&tag.to_le_bytes());
}
#[derive(Clone, Copy, Debug)]
enum Kind {
    Text,
    Data,
}
impl Kind {
    fn field(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Data => "data",
        }
    }
    fn plain(self) -> &'static str {
        match self {
            Self::Text => "plainText",
            Self::Data => "plainData",
        }
    }
    fn list(self) -> &'static str {
        match self {
            Self::Text => "texts",
            Self::Data => "datas",
        }
    }
    fn branded(self) -> &'static str {
        match self {
            Self::Text => "brandedText.body.value",
            Self::Data => "brandedData.body.value",
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Seed {
    Null,
    Stored,
    Empty,
    WrongLayout,
    MissingTerminator,
    BadUtf8,
}
#[derive(Clone, Copy, Debug)]
enum Action {
    Get,
    Edit,
    Init(u32),
}
#[derive(Debug)]
struct Case {
    kind: Kind,
    seed: Seed,
    tag: u16,
    path: String,
    action: Action,
    accepts: bool,
}
fn seed(kind: Kind, mode: Seed, tag: u16) -> Message {
    let mut message = Message::new_default();
    let mut root = message.init_root::<blob_access::Builder>();
    root.set_marker(73);
    root.reborrow().init_branded_text();
    root.reborrow().init_branded_data();
    let mut texts = root.reborrow().init_texts(4);
    texts.set(0, "sibling");
    texts.set(1, "");
    let mut datas = root.reborrow().init_datas(4);
    datas.set(0, b"sibling");
    datas.set(1, b"");
    root.reborrow().init_numbers(1).set(0, 19);
    match mode {
        Seed::Stored | Seed::Empty => {
            let text = if matches!(mode, Seed::Stored) {
                "stored"
            } else {
                ""
            };
            let data: &[u8] = if matches!(mode, Seed::Stored) {
                &[0, 255, 7]
            } else {
                b""
            };
            match kind {
                Kind::Text => root.set_text(text),
                Kind::Data => root.set_data(data),
            }
            root.set_plain_text(text);
            root.set_plain_data(data);
            root.reborrow()
                .get_branded_text()
                .unwrap()
                .get_body()
                .set_value(text)
                .unwrap();
            root.reborrow()
                .get_branded_data()
                .unwrap()
                .get_body()
                .set_value(data)
                .unwrap();
        }
        Seed::BadUtf8 => {
            root.reborrow()
                .init_text(3)
                .as_bytes_mut()
                .copy_from_slice(&[255, 0, 254]);
        }
        Seed::Null | Seed::WrongLayout | Seed::MissingTerminator => (),
    }
    let mut raw = capnp::any_struct::Builder::from_builder(root).unwrap();
    // Valid wire pointers with incompatible element storage.
    for name in ["texts", "datas"] {
        raw.get_pointer_section()
            .get(slot(name))
            .get_as::<capnp::any_pointer_list::Builder>()
            .unwrap()
            .get(3)
            .initn_as::<capnp::primitive_list::Builder<u16>>(1)
            .set(0, 123);
    }
    match mode {
        Seed::WrongLayout => {
            raw.get_pointer_section()
                .get(slot(kind.field()))
                .init_as_any_struct(1, 0);
        }
        Seed::MissingTerminator => {
            raw.get_pointer_section()
                .get(slot(kind.field()))
                .set_as::<capnp::data::Owned>(&b"not-terminated"[..])
                .unwrap();
        }
        _ => (),
    }
    set_tag(&mut message, tag);
    message
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for kind in [Kind::Text, Kind::Data] {
        let mut add = |seed, tag, path: &str, action, accepts| {
            cases.push(Case {
                kind,
                seed,
                tag,
                path: path.into(),
                action,
                accepts,
            })
        };
        for mode in [Seed::Null, Seed::Stored, Seed::Empty] {
            for action in [Action::Get, Action::Edit, Action::Init(0), Action::Init(5)] {
                add(mode, tag(kind.field()), kind.field(), action, true);
            }
            for action in [Action::Get, Action::Edit] {
                add(mode, u16::MAX, kind.plain(), action, true);
            }
            for action in [Action::Get, Action::Init(5)] {
                add(mode, 0, kind.branded(), action, true);
            }
        }
        let other = match kind {
            Kind::Text => "data",
            Kind::Data => "text",
        };
        for selection in [0, tag(other), u16::MAX] {
            for action in [Action::Get, Action::Edit, Action::Init(5)] {
                add(
                    Seed::Stored,
                    selection,
                    kind.field(),
                    action,
                    matches!(action, Action::Init(_)),
                );
            }
        }
        for action in [Action::Get, Action::Init(5)] {
            add(
                Seed::WrongLayout,
                tag(kind.field()),
                kind.field(),
                action,
                matches!(action, Action::Init(_)),
            );
        }
        if matches!(kind, Kind::Text) {
            for mode in [Seed::MissingTerminator, Seed::BadUtf8] {
                for action in [Action::Get, Action::Init(5)] {
                    add(
                        mode,
                        tag("text"),
                        "text",
                        action,
                        matches!(mode, Seed::BadUtf8) || matches!(action, Action::Init(_)),
                    );
                }
            }
        }
        for index in [0, 1, 2, 3, 4, u32::MAX] {
            for action in [Action::Get, Action::Init(3)] {
                add(
                    Seed::Null,
                    0,
                    &format!("{}/{index}", kind.list()),
                    action,
                    index < 4 && (index != 3 || matches!(action, Action::Init(_))),
                );
            }
        }
        add(
            Seed::Null,
            0,
            &format!("{}/0", kind.list()),
            Action::Edit,
            true,
        );
    }
    cases
}
fn loaded_blob<'a>(
    message: &'a mut Message,
    loader: &'a SchemaLoader,
    case: &Case,
) -> capnp::Result<&'a mut [u8]> {
    let mut root = dynamic::Builder::new(message.get_root()?, schema(loader))?;
    let mut path = case.path.split('.').peekable();
    let field = loop {
        let part = path.next().unwrap();
        if path.peek().is_none() {
            break part;
        }
        root = root.get_struct(part)?;
    };
    if let Some((name, index)) = field.split_once('/') {
        let list = root.get_list(name)?;
        let index = index.parse().unwrap();
        match (case.kind, case.action) {
            (Kind::Text, Action::Init(n)) => Ok(list.init_text(index, n)?.as_bytes_mut()),
            (Kind::Data, Action::Init(n)) => list.init_data(index, n),
            (Kind::Text, _) => Ok(list.get_text(index)?.as_bytes_mut()),
            (Kind::Data, _) => list.get_data(index),
        }
    } else {
        match (case.kind, case.action) {
            (Kind::Text, Action::Init(n)) => Ok(root.init_text(field, n)?.as_bytes_mut()),
            (Kind::Data, Action::Init(n)) => root.init_data(field, n),
            (Kind::Text, _) => Ok(root.get_text(field)?.as_bytes_mut()),
            (Kind::Data, _) => root.get_data(field),
        }
    }
}
fn compiled_blob<'a>(message: &'a mut Message, case: &Case) -> capnp::Result<&'a mut [u8]> {
    let mut root = dynamic_value::Builder::from(message.get_root::<blob_access::Builder>()?)
        .downcast::<dynamic_struct::Builder>();
    let mut path = case.path.split('.').peekable();
    let field = loop {
        let part = path.next().unwrap();
        if path.peek().is_none() {
            break part;
        }
        root = root.get_named(part)?.downcast();
    };
    let value = if let Some((name, index)) = field.split_once('/') {
        let list = root.get_named(name)?.downcast::<dynamic_list::Builder>();
        let index = index.parse().unwrap();
        if index >= list.len() {
            return Err(capnp::Error::failed("bounds".into()));
        }
        match case.action {
            Action::Init(n) => list.init(index, n)?,
            _ => list.get(index)?,
        }
    } else {
        match case.action {
            Action::Init(n) => root.initn_named(field, n)?,
            _ => root.get_named(field)?,
        }
    };
    Ok(match value {
        dynamic_value::Builder::Text(t) => t.as_bytes_mut(),
        dynamic_value::Builder::Data(d) => d,
        _ => panic!(),
    })
}
fn observe(bytes: &mut [u8], action: Action) -> Vec<u8> {
    if let Action::Init(n) = action {
        assert_eq!(bytes, vec![0; n as usize]);
    }
    if matches!(action, Action::Edit) && !bytes.is_empty() {
        bytes[0] = b'Z';
    }
    bytes.to_vec()
}
fn wire(message: &Message) -> Vec<u8> {
    capnp::Word::words_to_bytes(
        &capnp::any_struct::Reader::from_reader(
            message.get_root_as_reader::<blob_access::Reader>().unwrap(),
        )
        .canonicalize()
        .unwrap(),
    )
    .to_vec()
}
#[test]
fn blob_access_matches_compiled_defaults_edits_and_wire_states() {
    let loader = loader();
    for case in cases() {
        let mut loaded = seed(case.kind, case.seed, case.tag);
        let before = capnp::serialize::write_message_to_words(&loaded);
        let result =
            loaded_blob(&mut loaded, &loader, &case).map(|bytes| observe(bytes, case.action));
        assert_eq!(result.is_ok(), case.accepts, "{case:?}: {result:?}");
        if result.is_err() {
            assert_eq!(
                capnp::serialize::write_message_to_words(&loaded),
                before,
                "{case:?}"
            );
        }
        let mut compiled = seed(case.kind, case.seed, case.tag);
        let native = compiled_blob(&mut compiled, &case).map(|bytes| observe(bytes, case.action));
        assert_eq!(native.is_ok(), case.accepts, "{case:?}: {native:?}");
        assert_eq!(result.ok(), native.ok(), "{case:?}");
        assert_eq!(wire(&loaded), wire(&compiled), "{case:?}");
    }
}
#[test]
fn blob_getters_borrow_storage_and_keep_empty_defaults_null() {
    let loader = loader();
    for kind in [Kind::Text, Kind::Data] {
        for path in [
            kind.field().to_owned(),
            kind.plain().to_owned(),
            format!("{}/0", kind.list()),
            kind.branded().to_owned(),
        ] {
            let mut message = seed(kind, Seed::Stored, tag(kind.field()));
            let case = Case {
                kind,
                seed: Seed::Stored,
                tag: tag(kind.field()),
                path,
                action: Action::Get,
                accepts: true,
            };
            let before = capnp::serialize::write_message_to_words(&message);
            let original = compiled_blob(&mut message, &case).unwrap().as_ptr();
            let view = loaded_blob(&mut message, &loader, &case).unwrap();
            assert_eq!(view.as_ptr(), original);
            assert_eq!(capnp::serialize::write_message_to_words(&message), before);
        }
        for path in [
            kind.plain().to_owned(),
            format!("{}/2", kind.list()),
            kind.branded().to_owned(),
        ] {
            for compiled in [false, true] {
                let mut message = seed(kind, Seed::Null, 0);
                let before = capnp::serialize::write_message_to_words(&message);
                let case = Case {
                    kind,
                    seed: Seed::Null,
                    tag: 0,
                    path: path.clone(),
                    action: Action::Get,
                    accepts: true,
                };
                let view = if compiled {
                    compiled_blob(&mut message, &case).unwrap()
                } else {
                    loaded_blob(&mut message, &loader, &case).unwrap()
                };
                assert!(view.is_empty());
                assert_eq!(
                    capnp::serialize::write_message_to_words(&message),
                    before,
                    "{case:?}, compiled={compiled}"
                );
            }
        }
    }
    let mut message = seed(Kind::Text, Seed::Null, tag("text"));
    let mut root = dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
    assert!(!root.has_named("text").unwrap());
    let mut text = root.reborrow().get_text("text").unwrap();
    assert_eq!(text.reborrow().as_bytes(), b"default");
    text.as_bytes_mut()[0] = b'D';
    assert!(root.has_named("text").unwrap());
    assert_eq!(root.get_text("text").unwrap().as_bytes(), b"Default");
    let mut other = seed(Kind::Text, Seed::Null, tag("text"));
    assert_eq!(
        dynamic::Builder::new(other.get_root().unwrap(), schema(&loader))
            .unwrap()
            .get_text("text")
            .unwrap()
            .as_bytes(),
        b"default"
    );
}
#[test]
fn invalid_blob_initialization_preserves_selection_bytes_and_capability_owners() {
    use capnp::traits::ImbueMut;
    use std::{cell::Cell, rc::Rc};
    struct Server(Rc<Cell<u32>>);
    impl access_capability::Server for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let loader = loader();
    for kind in [Kind::Text, Kind::Data] {
        let mut active = seed(kind, Seed::Stored, tag(kind.field()));
        let before = capnp::serialize::write_message_to_words(&active);
        let mut root = dynamic::Builder::new(active.get_root().unwrap(), schema(&loader)).unwrap();
        match kind {
            Kind::Text => {
                assert!(root.reborrow().get_data("text").is_err());
                assert!(root.init_data("text", 1).is_err());
            }
            Kind::Data => {
                assert!(root.reborrow().get_text("data").is_err());
                assert!(root.init_text("data", 1).is_err());
            }
        }
        assert_eq!(capnp::serialize::write_message_to_words(&active), before);
        let drops = Rc::new(Cell::new(0));
        let client: access_capability::Client = capnp_rpc::new_client(Server(drops.clone()));
        let identity = client.client.hook.get_ptr();
        let mut message = seed(kind, Seed::Stored, tag("cap"));
        let mut table = Vec::new();
        {
            let mut root = message.get_root::<blob_access::Builder>().unwrap();
            root.imbue_mut(&mut table);
            root.init_cap().set_as_capability(client.client.hook);
        }
        let before = capnp::serialize::write_message_to_words(&message);
        let minimum_invalid = match kind {
            Kind::Text => (1 << 29) - 1,
            Kind::Data => 1 << 29,
        };
        for size in [minimum_invalid, 1 << 29, u32::MAX] {
            let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let mut root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            let error = match kind {
                Kind::Text => root.reborrow().init_text("text", size).err(),
                Kind::Data => root.reborrow().init_data("data", size).err(),
            }
            .unwrap();
            assert!(error.to_string().contains("wire limit"));
            let list = root.get_list(kind.list()).unwrap();
            assert!(match kind {
                Kind::Text => list.init_text(0, size).is_err(),
                Kind::Data => list.init_data(0, size).is_err(),
            });
            assert_eq!(capnp::serialize::write_message_to_words(&message), before);
            assert_eq!(table[0].as_ref().unwrap().get_ptr(), identity);
            assert_eq!(drops.get(), 0);
        }
        for target in ["marker", "child", "texts", "cap", "missing"] {
            let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let mut root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            assert!(root.reborrow().get_text(target).is_err());
            assert!(root.reborrow().get_data(target).is_err());
            assert!(root.reborrow().init_text(target, 1).is_err());
            assert!(root.reborrow().init_data(target, 1).is_err());
            assert_eq!(capnp::serialize::write_message_to_words(&message), before);
        }
        {
            let mut root =
                dynamic::Builder::new(message.get_root().unwrap(), schema(&loader)).unwrap();
            for name in ["numbers", "texts", "datas"] {
                let mut list = root.reborrow().get_list(name).unwrap();
                for index in [list.len(), u32::MAX] {
                    assert!(list.reborrow().get_text(index).is_err());
                    assert!(list.reborrow().get_data(index).is_err());
                    assert!(list.reborrow().init_text(index, 1).is_err());
                    assert!(list.reborrow().init_data(index, 1).is_err());
                }
                if name != "texts" {
                    assert!(list.reborrow().get_text(0).is_err());
                    assert!(list.reborrow().init_text(0, 1).is_err());
                }
                if name != "datas" {
                    assert!(list.reborrow().get_data(0).is_err());
                    assert!(list.reborrow().init_data(0, 1).is_err());
                }
            }
        }
        assert_eq!(capnp::serialize::write_message_to_words(&message), before);
        // Inactive access rejects before interpreting the retained capability.
        {
            let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let mut root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            assert!(root.reborrow().get_text("text").is_err());
            assert!(root.get_data("data").is_err());
        }
        assert_eq!(drops.get(), 0);
        {
            let mut pointer = message.get_root::<capnp::any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            match kind {
                Kind::Text => {
                    root.init_text("text", 3)
                        .unwrap()
                        .as_bytes_mut()
                        .copy_from_slice(b"new");
                }
                Kind::Data => {
                    root.init_data("data", 3).unwrap().copy_from_slice(b"new");
                }
            }
        }
        assert!(table[0].is_none());
        assert_eq!(drops.get(), 1);
        let root = message.get_root_as_reader::<blob_access::Reader>().unwrap();
        let pointer = capnp::any_struct::Reader::from_reader(root)
            .get_pointer_section()
            .get(slot(kind.field()));
        assert_eq!(
            pointer.get_as::<capnp::data::Reader>().unwrap(),
            match kind {
                Kind::Text => &b"new\0"[..],
                Kind::Data => &b"new"[..],
            }
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn blob_access_matches_pinned_cpp() {
    use capntproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/dynamic-blobs");
    std::fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("blobs");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/dynamic-blobs.c++",
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
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    for (i, case) in cases.iter().enumerate() {
        let mut message = seed(case.kind, case.seed, case.tag);
        let seed_path = logs.join(format!("{i}.seed"));
        std::fs::write(
            &seed_path,
            capnp::serialize::write_message_to_words(&message),
        )
        .unwrap();
        let (action, size) = match case.action {
            Action::Get => ("get", 0),
            Action::Edit => ("edit", 0),
            Action::Init(n) => ("init", n),
        };
        let expected = run(
            command(&binary)
                .arg(&schema_path)
                .arg(schema(&loader).id().to_string())
                .arg(&seed_path)
                .arg(action)
                .arg(case.kind.field())
                .arg(&case.path)
                .arg(size.to_string()),
            &logs.join(format!("{i}.out")),
            0,
        )
        .unwrap();
        let result =
            loaded_blob(&mut message, &loader, case).map(|bytes| observe(bytes, case.action));
        let status = match result {
            Ok(bytes) => format!("ok {}:{}", bytes.len(), hex(&bytes)),
            Err(_) => "error".into(),
        };
        assert_eq!(
            format!("{status} {}\n", hex(&wire(&message))),
            expected,
            "{case:?}"
        );
    }
    std::fs::write(logs.join("summary.txt"), format!("{} blob access results, contents and canonical wire states match pinned C++ ({} rejected).\n", cases.len(), cases.iter().filter(|c| !c.accepts).count())).unwrap();
}

use capnp::{
    any_list::{self, ElementSize as E},
    any_pointer, any_struct, dynamic_value,
    introspect::Introspect,
    message,
    schema_loader::{dynamic, Schema, SchemaLoader, Type},
};
use reproto_test_support::native_list_capnp::{cast_item, cast_types};
#[path = "loaded_view_casts/capabilities.rs"]
mod capabilities;
#[path = "loaded_view_casts/erasure.rs"]
mod erasure;

type Message = message::Builder<message::HeapAllocator>;
fn native() -> capnp::schema::StructSchema {
    cast_types::Owned::introspect().as_struct_schema().unwrap()
}
fn loader() -> SchemaLoader {
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<cast_types::Owned>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    loader
}
fn root_schema(loader: &SchemaLoader) -> Schema<'_> {
    loader.get(native().get_proto().get_id()).unwrap()
}
fn ty<'s>(loader: &'s SchemaLoader, field: &str) -> Type<'s> {
    root_schema(loader)
        .field(field)
        .unwrap()
        .get_type()
        .unwrap()
}
fn structure<'s>(loader: &'s SchemaLoader, field: &str) -> Schema<'s> {
    let Type::Struct(s) = ty(loader, field) else {
        panic!()
    };
    s
}
fn element<'s>(loader: &'s SchemaLoader, field: &str) -> Type<'s> {
    let Type::List(t) = ty(loader, field) else {
        panic!()
    };
    *t
}
#[derive(Clone, Copy, Debug)]
enum Shape {
    Struct(u16, u16),
    List(E, u32, u16, u16),
}
#[derive(Debug)]
struct Case {
    shape: Shape,
    target: &'static str,
    mutable: bool,
    accepts: bool,
}
fn cases() -> Vec<Case> {
    let mut cases = vec![];
    for target in ["text", "data"] {
        for (d, p) in [(0, 0), (1, 0), (0, 1), (1, 1), (2, 2)] {
            for mutable in [false, true] {
                cases.push(Case {
                    shape: Shape::Struct(d, p),
                    target,
                    mutable,
                    accepts: !mutable || d >= 1 && p >= 1,
                });
            }
        }
    }
    for (shape, target, readable, writable) in [
        (Shape::List(E::Void, 0, 0, 0), "records", true, false),
        (Shape::List(E::Void, 2, 0, 0), "records", true, false),
        (Shape::List(E::FourBytes, 2, 0, 0), "records", true, false),
        (Shape::List(E::Pointer, 2, 0, 0), "records", true, false),
        (
            Shape::List(E::InlineComposite, 2, 1, 0),
            "records",
            true,
            false,
        ),
        (
            Shape::List(E::InlineComposite, 2, 1, 1),
            "records",
            true,
            true,
        ),
        (
            Shape::List(E::InlineComposite, 2, 2, 2),
            "records",
            true,
            true,
        ),
        (Shape::List(E::Bit, 2, 0, 0), "records", false, false),
        (Shape::List(E::FourBytes, 2, 0, 0), "numbers", true, true),
        (
            Shape::List(E::InlineComposite, 2, 2, 2),
            "numbers",
            true,
            true,
        ),
        (Shape::List(E::TwoBytes, 2, 0, 0), "numbers", false, false),
        (Shape::List(E::Bit, 2, 0, 0), "numbers", false, false),
        (Shape::List(E::Bit, 2, 0, 0), "flags", true, true),
        (Shape::List(E::Byte, 2, 0, 0), "flags", false, false),
        (Shape::List(E::TwoBytes, 2, 0, 0), "choices", true, true),
        (Shape::List(E::Pointer, 2, 0, 0), "texts", true, true),
        (Shape::List(E::Pointer, 2, 0, 0), "blobs", true, true),
        (
            Shape::List(E::InlineComposite, 2, 2, 2),
            "texts",
            true,
            true,
        ),
        (
            Shape::List(E::InlineComposite, 2, 2, 2),
            "blobs",
            true,
            true,
        ),
        (Shape::List(E::Pointer, 2, 0, 0), "nested", true, true),
        (Shape::List(E::FourBytes, 2, 0, 0), "texts", false, false),
    ] {
        for mutable in [false, true] {
            cases.push(Case {
                shape,
                target,
                mutable,
                accepts: if mutable { writable } else { readable },
            });
        }
    }
    cases
}
fn fill(mut value: any_struct::Builder<'_>, target: &str, i: u32) {
    let bytes = value.get_data_section();
    if !bytes.is_empty() {
        bytes[0] = (41 + i) as u8;
    }
    if bytes.len() > 8 {
        bytes[8] = 77;
    }
    let mut pointers = value.get_pointer_section();
    if !pointers.is_empty() {
        if target == "nested" {
            pointers
                .reborrow()
                .get(0)
                .initn_as::<capnp::primitive_list::Builder<u32>>(1)
                .set(0, 41 + i);
        } else {
            pointers
                .reborrow()
                .get(0)
                .set_as::<capnp::text::Owned>("payload")
                .unwrap();
        }
    }
    if pointers.len() > 1 {
        pointers
            .get(1)
            .set_as::<capnp::text::Owned>("unknown")
            .unwrap();
    }
}
fn seed(case: &Case) -> Message {
    let mut message = Message::new(
        message::HeapAllocator::new()
            .first_segment_words(1)
            .allocation_strategy(message::AllocationStrategy::FixedSize),
    );
    let pointer = message.init_root::<any_pointer::Builder>();
    match case.shape {
        Shape::Struct(d, p) => fill(pointer.init_as_any_struct(d, p), case.target, 0),
        Shape::List(E::InlineComposite, n, d, p) => {
            let mut list = pointer.init_as_list_of_any_struct(d, p, n).unwrap();
            for i in 0..n {
                fill(list.reborrow().get(i), case.target, i);
            }
        }
        Shape::List(E::Bit, n, _, _) => {
            let mut list = pointer.initn_as::<capnp::primitive_list::Builder<bool>>(n);
            for i in 0..n {
                list.set(i, i % 2 == 0);
            }
        }
        Shape::List(e, n, _, _) => {
            let mut list = pointer
                .init_as_any_list(e, n)
                .unwrap()
                .get_as_struct_list()
                .unwrap();
            for i in 0..n {
                fill(list.reborrow().get(i), case.target, i);
            }
        }
    }
    message
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn describe(value: dynamic::Value<'_, '_>) -> capnp::Result<String> {
    Ok(match value {
        dynamic::Value::Struct(s) => format!(
            "{}:{}",
            describe(s.get_named("number")?)?,
            describe(s.get_named("payload")?)?
        ),
        dynamic::Value::List(l) => (0..l.len())
            .map(|i| describe(l.get(i)?))
            .collect::<capnp::Result<Vec<_>>>()?
            .join(","),
        dynamic::Value::UInt32(v) => v.to_string(),
        dynamic::Value::Bool(v) => u8::from(v).to_string(),
        dynamic::Value::Enum(v, _) => v.to_string(),
        dynamic::Value::Text(v) => hex(v.as_bytes()),
        dynamic::Value::Data(v) => hex(v),
        _ => panic!(),
    })
}
fn describe_native(value: dynamic_value::Reader<'_>) -> capnp::Result<String> {
    Ok(match value {
        dynamic_value::Reader::Struct(s) => format!(
            "{}:{}",
            describe_native(s.get_named("number")?)?,
            describe_native(s.get_named("payload")?)?
        ),
        dynamic_value::Reader::List(l) => (0..l.len())
            .map(|i| describe_native(l.get(i)?))
            .collect::<capnp::Result<Vec<_>>>()?
            .join(","),
        dynamic_value::Reader::UInt32(v) => v.to_string(),
        dynamic_value::Reader::Bool(v) => u8::from(v).to_string(),
        dynamic_value::Reader::Enum(v) => v.get_value().to_string(),
        dynamic_value::Reader::Text(v) => hex(v.as_bytes()),
        dynamic_value::Reader::Data(v) => hex(v),
        _ => panic!(),
    })
}
fn edit_struct(mut value: dynamic::Builder<'_, '_>, target: &str) -> capnp::Result<()> {
    value.set_named("number", dynamic::Value::UInt32(99))?;
    value.set_named(
        "payload",
        if target == "data" {
            dynamic::Value::Data(b"new")
        } else {
            dynamic::Value::Text("new".into())
        },
    )
}
fn apply(message: &mut Message, loader: &SchemaLoader, case: &Case) -> capnp::Result<String> {
    match case.shape {
        Shape::Struct(..) => {
            let schema = structure(loader, case.target);
            if case.mutable {
                edit_struct(
                    message
                        .get_root::<any_struct::Builder>()?
                        .get_as_loaded(schema.clone())?,
                    case.target,
                )?;
            }
            describe(dynamic::Value::Struct(
                message
                    .get_root_as_reader::<any_struct::Reader>()?
                    .get_as_loaded(schema)?,
            ))
        }
        Shape::List(..) => {
            let ty = element(loader, case.target);
            if case.mutable {
                let mut list = message
                    .get_root::<any_list::Builder>()?
                    .get_as_loaded(ty.clone())?;
                match case.target {
                    "records" => edit_struct(list.get_struct(0)?, "text")?,
                    "numbers" => list.set(0, dynamic::Value::UInt32(99))?,
                    "flags" => list.set(0, dynamic::Value::Bool(false))?,
                    "choices" => {
                        let Type::Enum(schema) = ty.clone() else {
                            panic!()
                        };
                        list.set(0, dynamic::Value::Enum(65535, schema))?;
                    }
                    "texts" => list.set(0, dynamic::Value::Text("new".into()))?,
                    "blobs" => list.set(0, dynamic::Value::Data(b"new"))?,
                    "nested" => list.get_list(0)?.set(0, dynamic::Value::UInt32(99))?,
                    _ => panic!(),
                }
            }
            describe(dynamic::Value::List(
                message
                    .get_root_as_reader::<any_list::Reader>()?
                    .get_as_loaded(ty)?,
            ))
        }
    }
}
fn apply_native(message: &mut Message, case: &Case) -> capnp::Result<String> {
    let target = native().get_field_by_name(case.target)?.get_type();
    match case.shape {
        Shape::Struct(..) => {
            let schema = target.as_struct_schema()?;
            if case.mutable {
                let mut value = message
                    .get_root::<any_struct::Builder>()?
                    .get_as_dynamic(schema)?;
                value.set_named("number", 99u32.into())?;
                value.set_named(
                    "payload",
                    if case.target == "data" {
                        dynamic_value::Reader::Data(b"new")
                    } else {
                        dynamic_value::Reader::Text("new".into())
                    },
                )?;
            }
            describe_native(
                message
                    .get_root_as_reader::<any_struct::Reader>()?
                    .get_as_dynamic(schema)
                    .into(),
            )
        }
        Shape::List(..) => {
            let capnp::introspect::TypeVariant::List(ty) = target.which() else {
                panic!()
            };
            if case.mutable {
                let mut list = message
                    .get_root::<any_list::Builder>()?
                    .get_as_dynamic(ty)?;
                match case.target {
                    "records" => {
                        let mut value = list.get(0)?.downcast::<capnp::dynamic_struct::Builder>();
                        value.set_named("number", 99u32.into())?;
                        value.set_named("payload", dynamic_value::Reader::Text("new".into()))?;
                    }
                    "numbers" => list.set(0, 99u32.into())?,
                    "flags" => list.set(0, false.into())?,
                    "choices" => {
                        let capnp::introspect::TypeVariant::Enum(s) = ty.which() else {
                            panic!()
                        };
                        list.set(0, capnp::dynamic_value::Enum::new(65535, s.into()).into())?;
                    }
                    "texts" => list.set(0, dynamic_value::Reader::Text("new".into()))?,
                    "blobs" => list.set(0, dynamic_value::Reader::Data(b"new"))?,
                    "nested" => list
                        .get(0)?
                        .downcast::<capnp::dynamic_list::Builder>()
                        .set(0, 99u32.into())?,
                    _ => panic!(),
                }
            }
            describe_native(
                message
                    .get_root_as_reader::<any_list::Reader>()?
                    .get_as_dynamic(ty)?
                    .into(),
            )
        }
    }
}
fn wire(message: &Message) -> Vec<u8> {
    // A struct envelope allows C++ to canonicalize either root pointer kind.
    let mut wrapper = Message::new_default();
    wrapper
        .init_root::<any_pointer::Builder>()
        .init_as_any_struct(0, 1)
        .get_pointer_section()
        .get(0)
        .set_as::<any_pointer::Owned>(message.get_root_as_reader::<any_pointer::Reader>().unwrap())
        .unwrap();
    let words = wrapper
        .get_root_as_reader::<any_struct::Reader>()
        .unwrap()
        .canonicalize()
        .unwrap();
    capnp::Word::words_to_bytes(&words).to_vec()
}
#[test]
fn loaded_casts_match_compiled_defaults_layout_checks_and_writes() {
    let loader = loader();
    for case in cases() {
        let mut message = seed(&case);
        let before = capnp::serialize::write_message_to_words(&message);
        let result = apply(&mut message, &loader, &case);
        assert_eq!(result.is_ok(), case.accepts, "{case:?}: {result:?}");
        if !case.mutable || !case.accepts {
            assert_eq!(
                capnp::serialize::write_message_to_words(&message),
                before,
                "{case:?}"
            );
        }
        let mut native = seed(&case);
        let compiled = apply_native(&mut native, &case);
        assert_eq!(compiled.is_ok(), case.accepts, "{case:?}: {compiled:?}");
        assert_eq!(compiled.ok(), result.ok(), "{case:?}");
        assert_eq!(wire(&message), wire(&native), "{case:?}");
    }
}
#[test]
fn loaded_casts_preserve_brands_unknown_fields_and_addresses() {
    let loader = loader();
    for shape in [
        Shape::Struct(2, 2),
        Shape::List(E::InlineComposite, 2, 2, 2),
    ] {
        let case = Case {
            shape,
            target: if matches!(shape, Shape::Struct(..)) {
                "text"
            } else {
                "records"
            },
            mutable: true,
            accepts: true,
        };
        let mut message = seed(&case);
        let address = match shape {
            Shape::Struct(..) => message
                .get_root_as_reader::<any_struct::Reader>()
                .unwrap()
                .get_data_section()
                .as_ptr(),
            _ => message
                .get_root_as_reader::<capnp::any_struct_list::Reader>()
                .unwrap()
                .get(0)
                .unwrap()
                .get_data_section()
                .as_ptr(),
        };
        apply(&mut message, &loader, &case).unwrap();
        let value = match shape {
            Shape::Struct(..) => message.get_root_as_reader::<any_struct::Reader>().unwrap(),
            _ => message
                .get_root_as_reader::<capnp::any_struct_list::Reader>()
                .unwrap()
                .get(0)
                .unwrap(),
        };
        assert_eq!(value.get_data_section().as_ptr(), address);
        assert_eq!(value.get_data_section()[8], 77);
        assert_eq!(
            value
                .get_pointer_section()
                .get(1)
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "unknown"
        );
    }
    let case = Case {
        shape: Shape::Struct(1, 1),
        target: "text",
        mutable: false,
        accepts: true,
    };
    let mut message = seed(&case);
    let raw = message.get_root_as_reader::<any_struct::Reader>().unwrap();
    let text = raw.get_as_loaded(structure(&loader, "text")).unwrap();
    let data = raw.get_as_loaded(structure(&loader, "data")).unwrap();
    assert!(!text.schema().equals(&data.schema()));
    assert!(matches!(
        text.schema().field("payload").unwrap().get_type().unwrap(),
        Type::Text
    ));
    assert!(matches!(
        data.schema().field("payload").unwrap().get_type().unwrap(),
        Type::Data
    ));
    // Schemas are unregistered: loaded reflection works, native recovery remains gated.
    assert!(text
        .downcast_native::<cast_item::Owned<capnp::text::Owned>>()
        .is_err());
    let mut value = message
        .get_root::<any_struct::Builder>()
        .unwrap()
        .get_as_loaded(structure(&loader, "data"))
        .unwrap();
    assert!(value
        .set_named("payload", dynamic::Value::Text("wrong".into()))
        .is_err());
    value
        .set_named("payload", dynamic::Value::Data(b"right"))
        .unwrap();
}
#[test]
fn loaded_casts_validate_metadata_and_keep_reader_limits() {
    let loader = loader();
    let schema = structure(&loader, "text");
    let service = match ty(&loader, "cap") {
        Type::Struct(s) => match s.field("payload").unwrap().get_type().unwrap() {
            Type::Interface(s) => s,
            _ => panic!(),
        },
        _ => panic!(),
    };
    let mut message = Message::new_default();
    message
        .init_root::<any_pointer::Builder>()
        .init_as_any_struct(1, 1);
    assert!(message
        .get_root_as_reader::<any_struct::Reader>()
        .unwrap()
        .get_as_loaded(service.clone())
        .is_err());
    assert!(message
        .get_root::<any_struct::Builder>()
        .unwrap()
        .get_as_loaded(service.clone())
        .is_err());
    let mut message = Message::new_default();
    message
        .init_root::<any_pointer::Builder>()
        .init_as_any_list(E::Void, 0)
        .unwrap();
    for invalid in [
        Type::Unknown(60000),
        Type::Parameter(1, 0),
        Type::Enum(schema.clone()),
        Type::Interface(schema.clone()),
        Type::Struct(service),
        Type::List(Box::new(Type::Unknown(60000))),
    ] {
        let before = capnp::serialize::write_message_to_words(&message);
        assert!(message
            .get_root_as_reader::<any_list::Reader>()
            .unwrap()
            .get_as_loaded(invalid.clone())
            .is_err());
        assert!(message
            .get_root::<any_list::Builder>()
            .unwrap()
            .get_as_loaded(invalid)
            .is_err());
        assert_eq!(capnp::serialize::write_message_to_words(&message), before);
    }
    let case = Case {
        shape: Shape::List(E::InlineComposite, 2, 1, 1),
        target: "records",
        mutable: false,
        accepts: true,
    };
    let source = seed(&case);
    let bytes = capnp::serialize::write_message_to_words(&source);
    let reader = capnp::serialize::read_message(
        bytes.as_slice(),
        message::ReaderOptions {
            nesting_limit: 1,
            ..message::ReaderOptions::new()
        },
    )
    .unwrap();
    assert!(reader
        .get_root::<any_list::Reader>()
        .unwrap()
        .get_as_loaded(element(&loader, "records"))
        .is_err());
    // Casting preserves the remaining traversal budget and does not visit children.
    let mut source = Message::new_default();
    fill(
        source
            .init_root::<any_pointer::Builder>()
            .init_as_any_struct(1, 1),
        "text",
        0,
    );
    let reader = message::Reader::new(
        &source,
        message::ReaderOptions {
            traversal_limit_in_words: Some(3),
            ..message::ReaderOptions::new()
        },
    );
    let raw = reader.get_root::<any_struct::Reader>().unwrap();
    for _ in 0..3 {
        assert!(raw.get_as_loaded(schema.clone()).is_ok());
    }
    assert!(raw
        .get_as_loaded(schema)
        .unwrap()
        .get_named("payload")
        .is_err());
}

#[test]
fn loaded_casts_defer_child_validation_and_retain_list_traversal_budgets() {
    let loader = loader();
    for list in [false, true] {
        let mut message = Message::new_default();
        let root = message.init_root::<any_pointer::Builder>();
        let mut value = if list {
            root.init_as_list_of_any_struct(0, 1, 1).unwrap().get(0)
        } else {
            root.init_as_any_struct(1, 1)
        };
        value.get_pointer_section().get(0).init_as_any_struct(0, 0);
        let before = capnp::serialize::write_message_to_words(&message);
        if list {
            let view = message
                .get_root_as_reader::<any_list::Reader>()
                .unwrap()
                .get_as_loaded(element(&loader, "texts"))
                .unwrap();
            assert!(view.get(0).is_err());
            let mut view = message
                .get_root::<any_list::Builder>()
                .unwrap()
                .get_as_loaded(element(&loader, "texts"))
                .unwrap();
            assert!(view.reborrow().get_text(0).is_err());
            let raw = view.into_any_list();
            assert!(raw
                .get_as::<capnp::text_list::Owned>()
                .unwrap()
                .get(0)
                .is_err());
        } else {
            let view = message
                .get_root_as_reader::<any_struct::Reader>()
                .unwrap()
                .get_as_loaded(structure(&loader, "text"))
                .unwrap();
            assert!(view.get_named("payload").is_err());
            let mut view = message
                .get_root::<any_struct::Builder>()
                .unwrap()
                .get_as_loaded(structure(&loader, "text"))
                .unwrap();
            assert!(view.reborrow().get_text("payload").is_err());
            let mut raw = view.into_any_struct();
            assert!(raw
                .get_pointer_section()
                .get(0)
                .get_as::<capnp::text::Builder>()
                .is_err());
        }
        assert_eq!(capnp::serialize::write_message_to_words(&message), before);
    }
    let mut message = Message::new_default();
    let mut list = message.initn_root::<capnp::text_list::Builder>(2);
    list.set(0, "first");
    list.set(1, "second");
    let reader = message::Reader::new(
        &message,
        message::ReaderOptions {
            traversal_limit_in_words: Some(3),
            ..message::ReaderOptions::new()
        },
    );
    let raw = reader.get_root::<any_list::Reader>().unwrap();
    for _ in 0..3 {
        assert!(raw.get_as_loaded(element(&loader, "texts")).is_ok());
    }
    assert!(raw
        .get_as_loaded(element(&loader, "texts"))
        .unwrap()
        .get(0)
        .is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn compatible_loaded_casts_match_pinned_cpp() {
    use reproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/loaded-view-casts");
    std::fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("casts");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/loaded-view-casts.c++",
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
    let mut compared = 0;
    let mut erased = 0;
    for (i, case) in cases.iter().enumerate().filter(|(_, c)| c.accepts) {
        let mut message = seed(case);
        let seed_path = logs.join(format!("{i}.seed"));
        std::fs::write(
            &seed_path,
            capnp::serialize::write_message_to_words(&message),
        )
        .unwrap();
        let expected = run(
            command(&binary)
                .arg(&schema_path)
                .arg(root_schema(&loader).id().to_string())
                .arg(&seed_path)
                .arg(case.target)
                .arg(if case.mutable { "write" } else { "read" }),
            &logs.join(format!("{i}.out")),
            0,
        )
        .unwrap();
        let value = apply(&mut message, &loader, case).unwrap();
        assert_eq!(
            format!("{value} {}\n", hex(&wire(&message))),
            expected,
            "{case:?}"
        );
        compared += 1;
        // C++ stores projected inline pointer builders at a shifted base. Its
        // raw erasure is not an oracle for full physical layout in those cases.
        if case.mutable
            && !(matches!(case.shape, Shape::List(E::InlineComposite, ..))
                && matches!(case.target, "texts" | "blobs"))
        {
            let expected = run(
                command(&binary)
                    .arg(&schema_path)
                    .arg(root_schema(&loader).id().to_string())
                    .arg(&seed_path)
                    .arg(case.target)
                    .arg("erase"),
                &logs.join(format!("erasure-{i}.out")),
                0,
            )
            .unwrap();
            let mut message = seed(case);
            let value = erasure::apply_erased(&mut message, &loader, case);
            assert_eq!(
                format!("{value} {}\n", hex(&wire(&message))),
                expected,
                "erased {case:?}"
            );
            erased += 1;
        }
    }
    std::fs::write(logs.join("erasure-summary.txt"), format!("{erased} builder erasures and raw mutations match pinned C++. Inline pointer projections are checked against compiled Rust.\n")).unwrap();
    std::fs::write(logs.join("summary.txt"), format!("{compared} compatible casts, values and canonical wire states match pinned C++. {} stricter layout rejections match compiled Rust.\n", cases.iter().filter(|c| !c.accepts).count())).unwrap();
}

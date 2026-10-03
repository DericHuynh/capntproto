use capnp::{
    schema_capnp::{code_generator_request, field, node, value},
    schema_loader::{dynamic::Value, SchemaLoader},
};
use capntproto_compiler::{compile, SchemaParser};

const SAMPLE: &str = include_str!("../examples/composites.capnp");

fn find<'a>(request: code_generator_request::Reader<'a>, name: &str) -> node::Reader<'a> {
    request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == name)
        .unwrap()
}

#[test]
fn loaded_composites_preserve_schema_defaults_and_nested_assignments() {
    let message = compile("composites.capnp", SAMPLE).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader.load_request(request).unwrap();
    let constant = |name| {
        loader
            .get(find(request, name).get_id())
            .unwrap()
            .constant_value()
            .unwrap()
    };
    let Value::Struct(empty) = constant("composites.capnp:empty") else {
        panic!()
    };
    assert!(matches!(
        empty.get_named("count").unwrap(),
        Value::UInt32(42)
    ));
    assert!(matches!(empty.get_named("name").unwrap(), Value::Text(v) if v == "default"));
    assert!(matches!(
        empty.get_named("state").unwrap(),
        Value::Enum(1, _)
    ));
    let Value::List(payloads) = empty.get_named("payloads").unwrap() else {
        panic!()
    };
    assert_eq!(payloads.len(), 2);
    assert!(matches!(payloads.get(0).unwrap(), Value::Data([0, 255])));
    assert!(!empty.has_named("payloads").unwrap());
    let Value::Struct(item) = constant("composites.capnp:item") else {
        panic!()
    };
    assert!(matches!(item.get_named("count").unwrap(), Value::UInt32(7)));
    assert!(matches!(item.get_named("name").unwrap(), Value::Text(v) if v == "hé🦀"));
    assert!(matches!(
        item.get_named("state").unwrap(),
        Value::Enum(0, _)
    ));
    let Value::Struct(metadata) = item.get_named("metadata").unwrap() else {
        panic!()
    };
    assert!(matches!(
        metadata.get_named("enabled").unwrap(),
        Value::Bool(false)
    ));
    assert!(matches!(metadata.get_named("label").unwrap(), Value::Text(v) if v == "selected"));
    assert!(metadata.get_named("absent").is_err());
    let Value::List(children) = item.get_named("children").unwrap() else {
        panic!()
    };
    assert_eq!(children.len(), 2);
    for (i, count) in [42, 9].into_iter().enumerate() {
        let Value::Struct(child) = children.get(i as u32).unwrap() else {
            panic!()
        };
        assert!(matches!(child.get_named("count").unwrap(), Value::UInt32(v) if v == count));
    }
    let Value::List(items) = constant("composites.capnp:items") else {
        panic!()
    };
    assert_eq!(items.len(), 2);
    let Value::List(rows) = constant("composites.capnp:rows") else {
        panic!()
    };
    let Value::List(first) = rows.get(0).unwrap() else {
        panic!()
    };
    assert!(matches!(first.get(0).unwrap(), Value::Int16(-1)));
    let Value::List(second) = rows.get(1).unwrap() else {
        panic!()
    };
    assert_eq!(second.len(), 0);
}

#[test]
fn erased_imported_values_keep_wire_data_without_emitting_type_dependencies() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            "@0xaaaaaaaaaaaaaaaa; const value :AnyPointer = import \"item.capnp\".item;",
        )
        .unwrap();
    parser.add_source("item.capnp", "@0xbbbbbbbbbbbbbbbb; struct Item { n @0 :UInt32 = 42; s @1 :Text; } const item :Item = (n = 7, s = \"imported\"); const unused :UInt32 = import \"missing.capnp\".x;").unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    assert_eq!(request.get_nodes().unwrap().len(), 2);
    assert_eq!(
        request
            .get_requested_files()
            .unwrap()
            .get(0)
            .get_imports()
            .unwrap()
            .len(),
        0
    );
    let node::Const(constant) = find(request, "main.capnp:value").which().unwrap() else {
        panic!()
    };
    let value::AnyPointer(pointer) = constant.get_value().unwrap().which().unwrap() else {
        panic!()
    };
    let value = pointer.get_as::<capnp::any_struct::Reader<'_>>().unwrap();
    assert_eq!(&value.get_data_section()[..4], &(7u32 ^ 42).to_le_bytes());
    assert_eq!(
        value
            .get_pointer_section()
            .get(0)
            .get_as::<capnp::text::Reader<'_>>()
            .unwrap(),
        "imported"
    );
    assert!(parser.parse(&["item.capnp"]).is_err());
    assert!(parser.parse(&["main.capnp"]).is_ok());
}

#[test]
fn explicit_empty_composites_remain_distinct_from_null_defaults() {
    let message = compile("empty.capnp", "@0xabcdefabcdefabcd; struct S { a @0 :List(UInt8); b @1 :List(UInt8) = []; c @2 :S; d @3 :S = (); }").unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let node::Struct(s) = find(request, "empty.capnp:S").which().unwrap() else {
        panic!()
    };
    for (i, field) in s.get_fields().unwrap().iter().enumerate() {
        let field::Slot(slot) = field.which().unwrap() else {
            panic!()
        };
        let (value::List(pointer) | value::Struct(pointer)) =
            slot.get_default_value().unwrap().which().unwrap()
        else {
            panic!()
        };
        assert_eq!(pointer.is_null(), i % 2 == 0);
        assert_eq!(slot.get_had_explicit_default(), i % 2 == 1);
    }
}

#[test]
fn composite_errors_and_truncated_inputs_have_valid_source_spans() {
    for (body, expected) in [
        (
            "struct S { x @0 :UInt8; } const x :S = (x = 256);",
            "out of range",
        ),
        ("struct S {} const x :S = (absent = 1);", "no field named"),
        (
            "struct S { x @0 :S; } const x :S = (x = .x);",
            "cyclic constant",
        ),
        (
            "const x :List(UInt16) = [1]; const y :List(UInt32) = .x;",
            "field type",
        ),
        (
            "const x :UInt32 = 1; const y :AnyPointer = .x;",
            "field type",
        ),
        (
            "const x :List(UInt32) = []; const y :AnyPointer = .x; const z :AnyPointer = .y;",
            "references to AnyPointer",
        ),
        (
            "struct S { x @0 :UInt8; } const x :S = (x = 1, 2);",
            "missing field name",
        ),
    ] {
        let source = format!("@0xabcdefabcdefabcd;\n# 🦀\n{body}");
        let error = compile("bad.capnp", &source).err().expect(body);
        assert!(error.message.contains(expected), "{body}: {error}");
        assert_eq!(error.filename, "bad.capnp");
        assert!(error.line >= 3);
        assert!(source.is_char_boundary(error.start) && source.is_char_boundary(error.end));
    }
    let source = "@0xabcdefabcdefabcd; struct S { x @0 :List(S); t @1 :Text; } const x :S = (x = [(), (t = \"hé🦀\")],);";
    for end in (0..=source.len()).filter(|&n| source.is_char_boundary(n)) {
        match compile("prefix.capnp", &source[..end]) {
            Ok(message) => SchemaLoader::default()
                .load_request(
                    message
                        .get_root_as_reader::<code_generator_request::Reader<'_>>()
                        .unwrap(),
                )
                .unwrap(),
            Err(error) => {
                assert!(source.is_char_boundary(error.start) && source.is_char_boundary(error.end))
            }
        }
    }
}

#[test]
fn expanded_value_depth_work_and_encoded_storage_are_bounded() {
    let mut diamond = String::from(
        "@0xabcdefabcdefabcd; struct S { left @0 :S; right @1 :S; } const c0 :S = ();",
    );
    for i in 1..24 {
        diamond.push_str(&format!(
            "const c{i} :S = (left = .c{}, right = .c{});",
            i - 1,
            i - 1
        ));
    }
    assert!(compile("diamond.capnp", &diamond)
        .err()
        .unwrap()
        .message
        .contains("expanded composite value work limit"));
    let mut chain = String::from("@0xabcdefabcdefabcd; struct S { next @0 :S; } const c0 :S = ();");
    for i in 1..70 {
        chain.push_str(&format!("const c{i} :S = (next = .c{});", i - 1));
    }
    assert!(compile("deep.capnp", &chain)
        .err()
        .unwrap()
        .message
        .contains("expanded composite nesting limit"));
    let mut wide = String::from("@0xabcdefabcdefabcd; struct Wide {");
    for i in 0..1024 {
        wide.push_str(&format!("f{i} @{i} :UInt64;"));
    }
    wide.push_str("} const values :List(Wide) = [");
    wide.push_str(&"(),".repeat(2100));
    wide.push_str("];");
    assert!(compile("wide.capnp", &wide)
        .err()
        .unwrap()
        .message
        .contains("encoded value size limit"));
}

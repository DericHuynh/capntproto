use capnp::{
    schema_capnp::{code_generator_request, node, type_, value},
    schema_loader::SchemaLoader,
};
use capnp_compiler::{compile, SchemaParser};

fn interface(n: node::Reader<'_>) -> node::interface::Reader<'_> {
    let node::Interface(i) = n.which().unwrap() else {
        panic!()
    };
    i
}

#[test]
fn method_ids_are_ordinal_based_and_auxiliary_nodes_have_no_wire_scope() {
    let signatures = ["f @0 ()", "renamed @0 ()", "f @0 () -> ()"];
    for signature in signatures {
        let message = compile(
            "test.capnp",
            &format!("@0xabcdefabcdefabcd; interface I {{ {signature}; }}"),
        )
        .unwrap();
        let request = message
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        SchemaLoader::default().load_request(request).unwrap();
        let nodes = request.get_nodes().unwrap();
        assert_eq!(nodes.len(), 4);
        let i = nodes.get(1);
        assert_eq!(i.get_id(), 16692991765471269535);
        assert!(i.has_nested_nodes());
        assert_eq!(i.get_nested_nodes().unwrap().len(), 0);
        let method = interface(i).get_methods().unwrap().get(0);
        assert_eq!(method.get_param_struct_type(), 10277579979061236087);
        assert_eq!(method.get_result_struct_type(), 15780131977263441957);
        assert!(method.has_implicit_parameters());
        assert!(!method.has_param_brand());
        assert!(!method.has_result_brand());
        for aux in [nodes.get(2), nodes.get(3)] {
            assert_eq!(aux.get_scope_id(), 0);
            assert!(!aux.has_nested_nodes());
            let name = aux.get_display_name().unwrap().to_str().unwrap();
            let suffix = &name[aux.get_display_name_prefix_length() as usize..];
            assert!(suffix.ends_with("$Params") || suffix.ends_with("$Results"));
        }
    }
}

#[test]
fn interface_and_capability_slots_use_distinct_type_and_default_tags() {
    let message = compile(
        "test.capnp",
        "@0xabcdefabcdefabcd; interface I {} struct S { i @0 :I; c @1 :Capability; }",
    )
    .unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    SchemaLoader::default().load_request(request).unwrap();
    let node::Struct(s) = request.get_nodes().unwrap().get(2).which().unwrap() else {
        panic!()
    };
    assert_eq!((s.get_data_word_count(), s.get_pointer_count()), (0, 2));
    for (i, field) in s.get_fields().unwrap().iter().enumerate() {
        let capnp::schema_capnp::field::Slot(slot) = field.which().unwrap() else {
            panic!()
        };
        if i == 0 {
            assert!(matches!(
                slot.get_type().unwrap().which().unwrap(),
                type_::Interface(_)
            ));
            assert!(matches!(
                slot.get_default_value().unwrap().which().unwrap(),
                value::Interface(())
            ));
        } else {
            let type_::AnyPointer(pointer) = slot.get_type().unwrap().which().unwrap() else {
                panic!()
            };
            let type_::any_pointer::Unconstrained(kind) = pointer.which().unwrap() else {
                panic!()
            };
            assert!(matches!(
                kind.which().unwrap(),
                type_::any_pointer::unconstrained::Capability(())
            ));
            assert!(
                matches!(slot.get_default_value().unwrap().which().unwrap(), value::AnyPointer(p) if p.is_null())
            );
        }
    }
}

#[test]
fn streaming_requires_explicit_import_roots_and_the_standard_result_id() {
    let source = include_str!("../examples/interfaces.capnp");
    assert!(compile("interfaces.capnp", source)
        .err()
        .unwrap()
        .message
        .contains("/capnp/stream.capnp"));
    let mut parser = SchemaParser::new();
    parser.add_source("interfaces.capnp", source).unwrap();
    parser
        .add_source(
            "include/capnp/stream.capnp",
            include_str!("../../../vendor/capnproto/c++/src/capnp/stream.capnp"),
        )
        .unwrap();
    parser
        .add_source(
            "include/capnp/c++.capnp",
            include_str!("../../../vendor/capnproto/c++/src/capnp/c++.capnp"),
        )
        .unwrap();
    parser.import_path("include").unwrap();
    let message = parser.parse(&["interfaces.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    SchemaLoader::default().load_request(request).unwrap();
    let service = request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == "interfaces.capnp:Service")
        .unwrap();
    assert_eq!(
        interface(service)
            .get_methods()
            .unwrap()
            .get(2)
            .get_result_struct_type(),
        0x995f9a3377c0b16e
    );
    let mut invalid = SchemaParser::new();
    invalid
        .add_source(
            "test.capnp",
            "@0xabcdefabcdefabcd; interface I { f @0 () -> stream; }",
        )
        .unwrap();
    invalid
        .add_source(
            "capnp/stream.capnp",
            "@0x86c366a91393f3f8; struct StreamResult {}",
        )
        .unwrap();
    invalid.import_path(".").unwrap();
    assert!(invalid
        .parse(&["test.capnp"])
        .err()
        .unwrap()
        .message
        .contains("standard StreamResult"));
}

#[test]
fn invalid_methods_and_truncated_prefixes_report_bounded_source_spans() {
    for (body, message) in [
        ("interface I(T) extends(T) {}", "superclass"),
        ("interface I { f @0 [T] (value :T = 1); }", "field type"),
        ("interface I { f @1 (); }", "contiguous"),
        ("interface I { f @0 (); f @1 (); }", "duplicate method"),
        (
            "interface I { f @0 (x :Bool, x :Text); }",
            "duplicate parameter",
        ),
        (
            "interface I { const f :Void = void; f @0 (); }",
            "duplicate declaration",
        ),
        ("struct S {} interface I extends(S) {}", "superclass must"),
        ("interface I { f @0 UInt32; }", "signature must"),
        ("interface I { f @0 stream; }", "only appear after"),
        (
            "annotation a(field) :Void; interface I { f @0 (x :Bool $a); }",
            "param",
        ),
    ] {
        let source = format!("@0xabcdefabcdefabcd;\n{body}");
        let error = compile("bad.capnp", &source).err().unwrap();
        assert!(error.message.contains(message), "{body}: {error}");
        assert_eq!(error.filename, "bad.capnp");
        assert!(source.is_char_boundary(error.start));
        assert!(source.is_char_boundary(error.end));
    }
    let source = "@0xabcdefabcdefabcd; annotation a(*) :Text; interface Base {} interface I extends(Base) $a(\"🦀\") { struct P { text @0 :Text; } f @0 (p :P = (text=\"x\") $a(\"p\")) -> (other :I) $a(\"f\"); g @1 P -> P; }";
    for end in (0..=source.len()).filter(|&i| source.is_char_boundary(i)) {
        match compile("prefix.capnp", &source[..end]) {
            Ok(message) => SchemaLoader::default()
                .load_request(message.get_root_as_reader().unwrap())
                .unwrap(),
            Err(error) => {
                assert!(error.start <= error.end);
                assert!(error.end <= end);
            }
        }
    }
    let source = format!(
        "@0xabcdefabcdefabcd; {}{}",
        "interface I {".repeat(100),
        "}".repeat(100)
    );
    assert!(compile("deep.capnp", &source)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
    let methods: String = (0..2100).map(|i| format!("m{i} @{i} (); ")).collect();
    assert!(compile(
        "wide.capnp",
        &format!("@0xabcdefabcdefabcd; interface I {{ {methods} }}")
    )
    .err()
    .unwrap()
    .message
    .contains("node limit"));
}

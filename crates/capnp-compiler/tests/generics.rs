use capnp::{
    schema_capnp::{brand, code_generator_request, node, type_},
    schema_loader::SchemaLoader,
};
use capnp_compiler::{compile, SchemaParser};

fn find<'a>(request: code_generator_request::Reader<'a>, name: &str) -> node::Reader<'a> {
    request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| {
            n.get_display_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(name)
        })
        .unwrap()
}
fn fields(
    node: node::Reader<'_>,
) -> capnp::struct_list::Reader<'_, capnp::schema_capnp::field::Owned> {
    let node::Struct(s) = node.which().unwrap() else {
        panic!()
    };
    s.get_fields().unwrap()
}
fn field_type(field: capnp::schema_capnp::field::Reader<'_>) -> type_::Reader<'_> {
    let capnp::schema_capnp::field::Slot(slot) = field.which().unwrap() else {
        panic!()
    };
    slot.get_type().unwrap()
}

#[test]
fn generic_schema_metadata_preserves_inherited_and_method_parameter_scopes() {
    let message = compile("generics.capnp", include_str!("../examples/generics.capnp")).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    SchemaLoader::default().load_request(request).unwrap();
    let envelope = find(request, ":Envelope");
    let type_::AnyPointer(pointer) = field_type(fields(envelope).get(0)).which().unwrap() else {
        panic!()
    };
    let type_::any_pointer::Parameter(parameter) = pointer.which().unwrap() else {
        panic!()
    };
    assert_eq!(parameter.get_scope_id(), envelope.get_id());
    assert_eq!(parameter.get_parameter_index(), 0);
    assert!(find(request, "Envelope.State").get_is_generic());
    let service = find(request, ":Service");
    let node::Interface(interface) = service.which().unwrap() else {
        panic!()
    };
    let method = interface.get_methods().unwrap().get(1);
    assert_eq!(
        method
            .get_implicit_parameters()
            .unwrap()
            .get(0)
            .get_name()
            .unwrap(),
        "U"
    );
    let params = find(request, ":Service.relay$Params");
    assert_eq!(params.get_scope_id(), 0);
    let type_::AnyPointer(pointer) = field_type(fields(params).get(0)).which().unwrap() else {
        panic!()
    };
    let type_::any_pointer::Parameter(parameter) = pointer.which().unwrap() else {
        panic!()
    };
    assert_eq!(parameter.get_scope_id(), params.get_id());
    let scopes = method.get_param_brand().unwrap().get_scopes().unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes.get(0).get_scope_id(), service.get_id());
    assert!(matches!(
        scopes.get(0).which().unwrap(),
        brand::scope::Inherit(())
    ));
}

#[test]
fn imported_alias_bindings_do_not_leak_between_instantiations_or_parse_calls() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            r#"@0xabcdefabcdefabcd;
        using G = import "generic.capnp".Generic;
        struct S { text @0 :G(Text).Item; data @1 :G(Data).Item; raw @2 :G.Item; }
    "#,
        )
        .unwrap();
    parser
        .add_source(
            "generic.capnp",
            "@0xbbbbbbbbbbbbbbbb; struct Generic(T) { using Item = T; }",
        )
        .unwrap();
    for _ in 0..2 {
        let message = parser.parse(&["main.capnp"]).unwrap();
        let request = message
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        SchemaLoader::default().load_request(request).unwrap();
        let fields = fields(find(request, ":S"));
        assert!(matches!(
            field_type(fields.get(0)).which().unwrap(),
            type_::Text(())
        ));
        assert!(matches!(
            field_type(fields.get(1)).which().unwrap(),
            type_::Data(())
        ));
        assert!(matches!(
            field_type(fields.get(2)).which().unwrap(),
            type_::AnyPointer(_)
        ));
    }
}

#[test]
fn expanded_generic_brands_and_alias_chains_are_bounded() {
    let mut source = "@0xabcdefabcdefabcd; struct Pair(A,B) {} using T0 = Text;".to_owned();
    for i in 1..25 {
        source.push_str(&format!("using T{i} = Pair(T{}, T{});", i - 1, i - 1));
    }
    source.push_str("struct S { value @0 :T24; }");
    let error = compile("wide.capnp", &source).err().unwrap();
    assert!(error.message.contains("work limit"), "{error}");
    let mut source = "@0xabcdefabcdefabcd; struct Box(T) {} using T0 = Text;".to_owned();
    for i in 1..100 {
        source.push_str(&format!("using T{i} = Box(T{});", i - 1));
    }
    let error = compile("deep.capnp", &source).err().unwrap();
    assert!(error.message.contains("nesting limit"), "{error}");
    let invalid = "@0xabcdefabcdefabcd; struct G(T) { using A = B; using B = A; } struct S { value @0 :G(Text).A; }";
    assert!(compile("cycle.capnp", invalid)
        .err()
        .unwrap()
        .message
        .contains("cyclic alias"));
}

#[test]
fn generic_annotations_defaults_and_keyword_fields_validate_without_cpp() {
    let message = compile(
        "test.capnp",
        r#"@0xabcdefabcdefabcd;
        struct G(T) { annotation label(struct) :T; value @0 :T; }
        struct S $G(Text).label("typed") {
            union { struct :group { const @0 :G(Text) = (value="hello"); } enum @1 :Void; }
        }
        interface I { struct @0 [T] (value :T = null) -> (data :Data = null); }
    "#,
    )
    .unwrap();
    SchemaLoader::default()
        .load_request(message.get_root_as_reader().unwrap())
        .unwrap();
    for body in [
        "struct G(T) {} struct S { value @0 :G(Text)(Data); }",
        "struct G(T) {} struct S { value @0 :G(UInt32); }",
        "interface I { f @0 [T] T; }",
        "interface I { f @0 (value :UInt32 = null); }",
        "struct G(T) { annotation a(struct) :T; } struct S $G.a(42) {}",
        "struct G(T) { value @0 :T = 42; }",
        "enum E(T) { zero @0; }",
    ] {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        let error = compile("bad.capnp", &source).err().expect(body);
        assert!(source.is_char_boundary(error.start) && source.is_char_boundary(error.end));
        assert_eq!(error.filename, "bad.capnp");
    }
}

#[test]
fn every_truncated_generic_prefix_is_handled_without_panicking() {
    let source = include_str!("../examples/generics.capnp");
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
}

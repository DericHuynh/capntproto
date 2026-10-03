#[path = "corpus/grammar.rs"]
mod corpus;
#[path = "corpus/numbers.rs"]
mod numbers;

#[test]
fn numeric_syntax_is_checked_in_literals_and_unused_imported_declarations() {
    for (expressions, accepted) in [(numbers::VALID, true), (numbers::INVALID, false)] {
        for expression in expressions {
            let source = format!(
                "@0xbbbbbbbbbbbbbbbb;\nstruct Used {{}}\nconst hidden :Float64 = {expression};\n"
            );
            let literal = capntproto_compiler::literal::parse_literal(expression);
            assert_eq!(literal.is_ok(), accepted, "{expression}: {literal:?}");
            // Requested declarations and dependency-only files must agree on
            // syntax, even though only the former evaluate the constant.
            let eager = capntproto_compiler::compile("types.capnp", &source);
            let mut parser = capntproto_compiler::SchemaParser::new();
            parser.add_source("types.capnp", &source).unwrap();
            parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { x @0 :D.Used; }").unwrap();
            let lazy = parser.parse(&["main.capnp"]);
            for result in [eager, lazy] {
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{expression}: {:?}",
                    result.as_ref().err()
                );
                match result {
                    Ok(message) => {
                        capnp::schema_loader::SchemaLoader::default()
                            .load_request(message.get_root_as_reader().unwrap())
                            .unwrap();
                    }
                    Err(error) => {
                        assert_eq!(error.filename, "types.capnp");
                        assert_eq!(error.line, 3);
                        assert!(error.start < error.end && error.end <= source.len());
                        assert!(
                            source.is_char_boundary(error.start)
                                && source.is_char_boundary(error.end)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn grammar_corpus_accepts_valid_schemas_and_rejects_invalid_syntax() {
    check_cases(corpus::cases());
}

#[test]
fn lexical_corpus_preserves_control_bytes_and_rejects_invalid_separators() {
    check_cases(corpus::lexical_cases());
    assert_eq!(
        capntproto_compiler::literal::parse_literal("0x\"\x0b00\x0bff\x0b\"").unwrap(),
        capntproto_compiler::literal::Literal::Data(vec![0, 255])
    );
}

fn check_cases(cases: Vec<corpus::Case>) {
    for case in cases {
        let result = capntproto_compiler::compile("grammar.capnp", &case.source());
        assert_eq!(
            result.is_ok(),
            case.accepted,
            "{}: {}: {:?}",
            case.name,
            case.body,
            result.as_ref().err()
        );
        if let Ok(message) = result {
            capnp::schema_loader::SchemaLoader::default()
                .load_request(message.get_root_as_reader().unwrap())
                .unwrap();
        } else if let Err(error) = result {
            let source = case.source();
            assert_eq!(error.filename, "grammar.capnp");
            assert!(error.line >= 2, "{}: {error}", case.name);
            assert!(error.start <= error.end && error.end <= source.len());
            assert!(source.is_char_boundary(error.start) && source.is_char_boundary(error.end));
        }
    }
}

#[test]
fn contextual_generic_parameters_resolve_through_instantiated_fields_and_methods() {
    let mut parser = capntproto_compiler::SchemaParser::new();
    parser.add_source("grammar.capnp", r#"@0xabcdefabcdefabcd;
struct Box(import, group) { item @0 :import; other @1 :group; }
struct Use { box @0 :Box(Text, Data); }
interface Api { call @0 [import, group] (item :import, other :group) -> (result :Box(import, group)); }
"#).unwrap();
    let parsed = parser.parse_schemas(&["grammar.capnp"]).unwrap();
    let use_type = parsed
        .get_file("grammar.capnp")
        .unwrap()
        .get_nested("Use")
        .unwrap();
    let capnp::schema_loader::Type::Struct(bound) =
        use_type.schema().field("box").unwrap().get_type().unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        bound.field("item").unwrap().get_type().unwrap(),
        capnp::schema_loader::Type::Text
    ));
    assert!(matches!(
        bound.field("other").unwrap().get_type().unwrap(),
        capnp::schema_loader::Type::Data
    ));
    let api = parsed
        .get_file("grammar.capnp")
        .unwrap()
        .get_nested("Api")
        .unwrap();
    let method = api
        .schema()
        .method("call")
        .unwrap()
        .bind_implicit(&[
            capnp::schema_loader::Type::Text,
            capnp::schema_loader::Type::Data,
        ])
        .unwrap();
    let params = method.params().unwrap();
    assert!(matches!(
        params.field("item").unwrap().get_type().unwrap(),
        capnp::schema_loader::Type::Text
    ));
    assert!(matches!(
        params.field("other").unwrap().get_type().unwrap(),
        capnp::schema_loader::Type::Data
    ));
    let capnp::schema_loader::Type::Struct(result) = method
        .results()
        .unwrap()
        .field("result")
        .unwrap()
        .get_type()
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        result.field("item").unwrap().get_type().unwrap(),
        capnp::schema_loader::Type::Text
    ));
    assert!(matches!(
        result.field("other").unwrap().get_type().unwrap(),
        capnp::schema_loader::Type::Data
    ));
}

#[test]
fn leading_zero_float_grammar_also_applies_to_text_value_parsing() {
    for value in ["08e1", "078.5", "-09.0", "008e+2"] {
        assert!(
            capntproto_compiler::literal::parse_literal(value).is_err(),
            "{value}"
        );
    }
    for value in ["0.89", "07.85", "0e89", "007e-2", "0x89"] {
        assert!(
            capntproto_compiler::literal::parse_literal(value).is_ok(),
            "{value}"
        );
    }
}

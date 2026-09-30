use capnp::schema_capnp::code_generator_request;
use capnp_compiler::SchemaParser;

#[path = "corpus/discovery.rs"]
mod corpus;

#[test]
fn import_discovery_preserves_the_first_semantic_spelling() {
    for case in corpus::cases() {
        let mut parser = SchemaParser::new();
        parser.import_path("src").unwrap();
        for &(name, source) in corpus::SOURCES {
            parser.add_source(name, source).unwrap();
        }
        parser.add_source("src/main.capnp", case.source()).unwrap();
        let message = parser.parse(&case.requested()).unwrap();
        let request = message
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        capnp::schema_loader::SchemaLoader::default()
            .load_request(request)
            .unwrap();
        let leaf = request
            .get_nodes()
            .unwrap()
            .iter()
            .find(|node| node.get_id() == corpus::LEAF_ID)
            .unwrap();
        assert_eq!(
            leaf.get_display_name().unwrap(),
            case.leaf_name,
            "{}",
            case.name
        );
        let cache = parser.into_concurrent(&case.requested()).unwrap();
        let snapshot = cache.schemas().unwrap();
        assert_eq!(
            snapshot.serialized_request(),
            capnp::serialize::write_message_to_words(&message)
        );
        let later = cache.get_nested(corpus::LEAF_ID, "empty").unwrap();
        let later_request = later.schemas().materialize().unwrap();
        assert_eq!(
            later_request
                .get(corpus::LEAF_ID)
                .unwrap()
                .schema()
                .get_proto()
                .get_display_name()
                .unwrap(),
            case.leaf_name,
            "{}",
            case.name
        );
        assert_eq!(
            snapshot.serialized_request(),
            capnp::serialize::write_message_to_words(&message)
        );
    }
}

#[test]
fn deferred_group_and_method_defaults_share_the_expansion_budget() {
    // Deferred defaults must still be charged as they are evaluated, including
    // auxiliary nodes compiled in the enclosing struct/interface's bootstrap.
    let payload = format!(
        "@0xaaaaaaaaaaaaaaaa; const text :Text = \"{}\";",
        "x".repeat(1024 * 1024)
    );
    for methods in [false, true] {
        for (count, accepted) in [(8, true), (17, false)] {
            let mut source = payload.clone();
            source.push_str(if methods {
                "interface Root {"
            } else {
                "struct Root {"
            });
            for index in 0..count {
                source.push_str(&if methods {
                    format!("f{index} @{index} (value :List(Text) = [.text]);")
                } else {
                    format!("g{index} :group {{ value @{index} :List(Text) = [.text]; }}")
                });
            }
            source.push('}');
            let result = capnp_compiler::compile("budget.capnp", &source);
            assert_eq!(result.is_ok(), accepted, "methods={methods}, count={count}");
            if !accepted {
                assert!(result
                    .err()
                    .unwrap()
                    .message
                    .contains("expanded value size limit"));
            }
        }
    }
}

use capnp::schema_capnp::{code_generator_request, node};
use capnp_compiler::{compile, SchemaParser};

const SAMPLE: &str = include_str!("../examples/documentation.capnp");

fn info<'a>(
    request: code_generator_request::Reader<'a>,
    name: &str,
) -> node::source_info::Reader<'a> {
    let id = request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == name)
        .unwrap()
        .get_id();
    request
        .get_source_info()
        .unwrap()
        .iter()
        .find(|i| i.get_id() == id)
        .unwrap()
}

#[test]
fn docs_and_byte_ranges_follow_declarations_and_ordinal_member_order() {
    let message = compile("docs.capnp", SAMPLE).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let file = info(request, "docs.capnp");
    assert_eq!(
        file.get_doc_comment().unwrap(),
        "Documentation for this file.\n"
    );
    assert_eq!((file.get_start_byte(), file.get_end_byte()), (0, 0));
    let record = info(request, "docs.capnp:Record");
    assert_eq!(
        record.get_doc_comment().unwrap(),
        "Documentation for Record, attached after its opening brace.\n"
    );
    assert_eq!(
        record.get_start_byte() as usize,
        SAMPLE.find("struct Record").unwrap()
    );
    let fields = record.get_members().unwrap();
    assert_eq!(fields.get(0).get_doc_comment().unwrap(), "First field.\n");
    assert_eq!(
        fields.get(1).get_doc_comment().unwrap(),
        "Documentation follows a field, independently of ordinal order.\n"
    );
    let group = info(request, "docs.capnp:Record.metadata");
    assert_eq!(
        group.get_doc_comment().unwrap(),
        fields.get(2).get_doc_comment().unwrap()
    );
    assert_eq!(group.get_start_byte(), fields.get(2).get_start_byte());
    assert_eq!(group.get_end_byte(), fields.get(2).get_end_byte());
    assert_eq!(
        info(request, "docs.capnp:Record.Nested")
            .get_doc_comment()
            .unwrap(),
        "Closing comments document a block without an opening comment.\n"
    );
    assert_eq!(
        info(request, "docs.capnp:State")
            .get_members()
            .unwrap()
            .get(0)
            .get_doc_comment()
            .unwrap(),
        "Unknown.\n"
    );
    let method = info(request, "docs.capnp:Service")
        .get_members()
        .unwrap()
        .get(0);
    assert_eq!(method.get_doc_comment().unwrap(), "Method documentation.\n");
    let params = info(request, "docs.capnp:Service.get$Params");
    let fields = params.get_members().unwrap();
    assert!(!fields.get(0).has_doc_comment());
    assert_eq!(
        &SAMPLE[fields.get(1).get_start_byte() as usize..fields.get(1).get_end_byte() as usize],
        "limit :UInt32 = 7"
    );
    let results = info(request, "docs.capnp:Service.ping$Results");
    assert_eq!((results.get_start_byte(), results.get_end_byte()), (0, 0));
    for n in request.get_nodes().unwrap() {
        let i = request
            .get_source_info()
            .unwrap()
            .iter()
            .find(|i| i.get_id() == n.get_id())
            .unwrap();
        assert_eq!(n.get_start_byte(), i.get_start_byte());
        assert_eq!(n.get_end_byte(), i.get_end_byte());
        assert!(SAMPLE.is_char_boundary(n.get_start_byte() as usize));
        assert!(SAMPLE.is_char_boundary(n.get_end_byte() as usize));
    }
}

#[test]
fn comment_spacing_blank_lines_crlf_and_eof_are_preserved() {
    let source = "# prelude\r\n@0xabcdefabcdefabcd; # file 🦀\r\n#  indented\r\n#\r\n\r\nstruct S {\r\n\r\n# unattached opening\r\nx @0 :Text; # field\r\n}\r\n# closing 🦀";
    let message = compile("docs.capnp", source).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    assert_eq!(
        info(request, "docs.capnp").get_doc_comment().unwrap(),
        "file 🦀\r\n indented\r\n\r\n"
    );
    let structure = info(request, "docs.capnp:S");
    assert_eq!(structure.get_doc_comment().unwrap(), "closing 🦀\n");
    assert_eq!(structure.get_end_byte() as usize, source.len());
    assert_eq!(
        structure
            .get_members()
            .unwrap()
            .get(0)
            .get_doc_comment()
            .unwrap(),
        "field\r\n"
    );
}

#[test]
fn imported_type_docs_keep_original_file_offsets_and_lazy_selection() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            r#"@0xabcdefabcdefabcd; using T = import "types.capnp"; struct Main { t @0 :T.Item; }"#,
        )
        .unwrap();
    let imported = "@0xbbbbbbbbbbbbbbbb; # imported file\nstruct Item { # imported type\nx @0 :UInt32; # imported field\n}\nstruct Unused {} # unused\n";
    parser.add_source("types.capnp", imported).unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let item = info(request, "types.capnp:Item");
    assert_eq!(item.get_doc_comment().unwrap(), "imported type\n");
    assert_eq!(
        item.get_start_byte() as usize,
        imported.find("struct Item").unwrap()
    );
    assert_eq!(
        request.get_source_info().unwrap().len(),
        request.get_nodes().unwrap().len()
    );
    assert!(!request
        .get_nodes()
        .unwrap()
        .iter()
        .any(|n| n.get_display_name().unwrap() == "types.capnp:Unused"));
}

#[test]
fn metadata_index_handles_truncation_and_bounds_duplicate_group_comments() {
    for end in (0..=SAMPLE.len()).filter(|&end| SAMPLE.is_char_boundary(end)) {
        let _ = compile("prefix.capnp", &SAMPLE[..end]);
    }
    assert!(compile(
        "deep.capnp",
        &format!("@0xabcdefabcdefabcd; {}", "{".repeat(70_000))
    )
    .is_err());
    let mut parser = SchemaParser::new();
    for i in 0..3 {
        parser
            .add_source(
                &format!("{i}.capnp"),
                format!(
                    "@0x{:016x}; struct S {{ g :group {{ # {}\nx @0 :UInt8; }} }}",
                    0xabcdefabcdefabcdu64 + i,
                    "x".repeat(3 * 1024 * 1024)
                ),
            )
            .unwrap();
    }
    let error = parser
        .parse(&["0.capnp", "1.capnp", "2.capnp"])
        .err()
        .unwrap();
    assert!(error.message.contains("documentation limit"), "{error}");
}

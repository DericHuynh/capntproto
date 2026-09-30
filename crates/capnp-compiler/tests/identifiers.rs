use capnp::schema_capnp::code_generator_request::{self, requested_file};
use capnp_compiler::{compile, SchemaParser};

fn references(file: requested_file::Reader<'_>) -> Vec<(usize, usize, u64)> {
    assert!(file.has_file_source_info());
    let info = file.get_file_source_info().unwrap();
    assert!(info.has_identifiers());
    let result: Vec<_> = info
        .get_identifiers()
        .unwrap()
        .iter()
        .map(|i| {
            let requested_file::file_source_info::identifier::TypeId(id) = i.which().unwrap()
            else {
                panic!("unexpected member resolution");
            };
            (i.get_start_byte() as usize, i.get_end_byte() as usize, id)
        })
        .collect();
    assert!(result.windows(2).all(|w| w[0] < w[1]));
    result
}

#[test]
fn builtin_targets_empty_tables_and_shadowing_are_explicit() {
    let builtins = [
        ("Void", 1014),
        ("Bool", 1015),
        ("Int8", 1016),
        ("Int16", 1017),
        ("Int32", 1018),
        ("Int64", 1019),
        ("UInt8", 1020),
        ("UInt16", 1021),
        ("UInt32", 1022),
        ("UInt64", 1023),
        ("Float32", 1024),
        ("Float64", 1025),
        ("Text", 1026),
        ("Data", 1027),
        ("AnyPointer", 1030),
        ("AnyStruct", 1031),
        ("AnyList", 1032),
        ("Capability", 1033),
    ];
    let mut source = "@0xabcdefabcdefabcd; struct S {".to_owned();
    for (i, (name, _)) in builtins.iter().enumerate() {
        source.push_str(&format!("f{i} @{i} :{name};"));
    }
    source.push('}');
    let message = compile("test.capnp", &source).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let actual = references(request.get_requested_files().unwrap().get(0));
    assert_eq!(
        actual
            .iter()
            .map(|&(s, e, id)| (&source[s..e], id))
            .collect::<Vec<_>>(),
        builtins
    );

    for body in ["", "struct Empty {}", "enum Empty {}", "interface Empty {}"] {
        let message = compile("empty.capnp", &format!("@0xabcdefabcdefabcd; {body}")).unwrap();
        let request = message
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        assert!(references(request.get_requested_files().unwrap().get(0)).is_empty());
    }
    let message = compile(
        "shadow.capnp",
        "@0xabcdefabcdefabcd; struct Text @0xbbbbbbbbbbbbbbbb {} struct S { t @0 :Text; }",
    )
    .unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    assert_eq!(
        references(request.get_requested_files().unwrap().get(0))[0].2,
        0xbbbbbbbbbbbbbbbb
    );
}

#[test]
fn qualified_names_aliases_constants_and_parentheses_keep_byte_ranges() {
    let source = "\u{feff}# 🦀\r\n@0xabcdefabcdefabcd;\n\
        struct S @0xbbbbbbbbbbbbbbbb (T) { using P = T; struct N @0xcccccccccccccccc {} }\n\
        using Alias = S(Text);\n\
        const n @0xdddddddddddddddd :UInt32 = 4;\n\
        struct Use { a @0 :Alias; b @1 :((S)).N; c @2 :S(Data).P; n @3 :UInt32 = .n; }";
    let message = compile("test.capnp", source).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let actual = references(request.get_requested_files().unwrap().get(0));
    assert_eq!(
        actual
            .iter()
            .map(|&(s, e, id)| (&source[s..e], id))
            .collect::<Vec<_>>(),
        [
            ("S", 0xbbbbbbbbbbbbbbbb),
            ("Text", 1026),
            ("UInt32", 1022),
            ("Alias", 0xbbbbbbbbbbbbbbbb),
            ("S", 0xbbbbbbbbbbbbbbbb),
            ("S)).N", 0xcccccccccccccccc),
            ("S", 0xbbbbbbbbbbbbbbbb),
            ("S(Data).P", 1027),
            ("Data", 1027),
            ("UInt32", 1022),
            (".n", 0xdddddddddddddddd),
        ]
    );
    for (start, end, _) in actual {
        assert!(source.is_char_boundary(start) && source.is_char_boundary(end));
    }
}

#[test]
fn import_retries_and_multiple_requested_files_keep_separate_reference_tables() {
    let source = "@0xabcdefabcdefabcd; using T = import \"types.capnp\"; struct S { value @0 :T.Item; n @1 :UInt32 = (import \"values.capnp\").n; }";
    let types = "@0xbbbbbbbbbbbbbbbb; struct Item @0xcccccccccccccccc { t @0 :Text; }";
    let values = "@0xdddddddddddddddd; const n @0xeeeeeeeeeeeeeeee :UInt32 = 5;";
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", source).unwrap();
    assert!(parser.parse(&["main.capnp"]).is_err());
    parser.add_source("types.capnp", types).unwrap();
    assert!(parser.parse(&["main.capnp"]).is_err());
    parser.add_source("values.capnp", values).unwrap();
    for _ in 0..2 {
        let message = parser.parse(&["main.capnp", "types.capnp"]).unwrap();
        let request = message
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        let files = request.get_requested_files().unwrap();
        assert_eq!(files.len(), 2);
        let main = files
            .iter()
            .find(|f| f.get_filename().unwrap() == "main.capnp")
            .unwrap();
        assert_eq!(
            references(main)
                .iter()
                .map(|&(s, e, id)| (&source[s..e], id))
                .collect::<Vec<_>>(),
            [
                ("import \"types.capnp\"", 0xbbbbbbbbbbbbbbbb),
                ("T", 0xbbbbbbbbbbbbbbbb),
                ("T.Item", 0xcccccccccccccccc),
                ("UInt32", 1022),
                ("import \"values.capnp\"", 0xdddddddddddddddd),
                ("import \"values.capnp\").n", 0xeeeeeeeeeeeeeeee),
            ]
        );
        let imported = files
            .iter()
            .find(|f| f.get_filename().unwrap() == "types.capnp")
            .unwrap();
        assert_eq!(
            references(imported)
                .iter()
                .map(|&(s, e, id)| (&types[s..e], id))
                .collect::<Vec<_>>(),
            [("Text", 1026)]
        );
        assert!(!request
            .get_nodes()
            .unwrap()
            .iter()
            .any(|n| n.get_id() == 0xeeeeeeeeeeeeeeee));
    }
}

#[test]
fn implicit_parameters_literals_and_stream_keyword_are_not_type_references() {
    let source = "@0xabcdefabcdefabcd; interface I { f @0 [T] (x :T = null, b :Bool = true, n :Float64 = inf) -> stream; }";
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", source).unwrap();
    parser
        .add_source(
            "capnp/stream.capnp",
            "@0xbbbbbbbbbbbbbbbb; struct StreamResult @0x995f9a3377c0b16e {}",
        )
        .unwrap();
    parser.import_path(".").unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    assert_eq!(
        references(request.get_requested_files().unwrap().get(0))
            .iter()
            .map(|&(s, e, id)| (&source[s..e], id))
            .collect::<Vec<_>>(),
        [("Bool", 1015), ("Float64", 1025)]
    );
}

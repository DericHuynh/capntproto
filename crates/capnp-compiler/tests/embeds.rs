use capnp::{
    any_pointer, any_struct, message,
    schema_capnp::{code_generator_request, node, value},
};
use capnp_compiler::{FileCompiler, SchemaParser};

fn constant<'a>(request: code_generator_request::Reader<'a>, name: &str) -> value::Reader<'a> {
    let node = request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == name)
        .unwrap();
    let node::Const(c) = node.which().unwrap() else {
        panic!()
    };
    c.get_value().unwrap()
}

fn blob() -> Vec<u8> {
    let mut message = message::Builder::new_default();
    let mut value = message
        .init_root::<any_pointer::Builder<'_>>()
        .init_as_any_struct(2, 2);
    value.get_data_section()[0] = 34;
    value.get_data_section()[8] = 99;
    value
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::text::Owned>("embedded")
        .unwrap();
    value
        .get_pointer_section()
        .get(1)
        .set_as::<capnp::data::Owned>(&b"unknown"[..])
        .unwrap();
    capnp::serialize::write_message_to_words(&message)
}

#[test]
fn embeds_preserve_binary_text_and_empty_values_without_schema_imports() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "dir/main.capnp",
            r#"@0xabcdefabcdefabcd;
        const text :Text = embed "bytes.bin";
        const data :Data = embed "/bytes.bin";
        const empty :Text = embed "empty";
        const source :Text = embed "main.capnp";
    "#,
        )
        .unwrap();
    parser
        .add_file("dir/bytes.bin", vec![0, 255, 0xc3, 0xa9])
        .unwrap();
    parser
        .add_file("include/bytes.bin", b"root".to_vec())
        .unwrap();
    parser.add_file("dir/empty", vec![]).unwrap();
    parser.import_path("include").unwrap();
    let message = parser.parse(&["dir/main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let value::Text(text) = constant(request, "dir/main.capnp:text").which().unwrap() else {
        panic!()
    };
    assert_eq!(text.unwrap().as_bytes(), &[0, 255, 0xc3, 0xa9]);
    let value::Data(data) = constant(request, "dir/main.capnp:data").which().unwrap() else {
        panic!()
    };
    assert_eq!(data.unwrap(), b"root");
    let value::Text(text) = constant(request, "dir/main.capnp:empty").which().unwrap() else {
        panic!()
    };
    assert_eq!(text.unwrap(), "");
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
    assert!(parser.add_file("dir/./bytes.bin", vec![]).is_err());
    assert!(parser
        .parse(&["dir/bytes.bin"])
        .err()
        .unwrap()
        .message
        .contains("UTF-8"));
}

#[test]
fn embedded_structs_keep_unknown_storage_and_truncate_it_in_inline_lists() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            r#"@0xabcdefabcdefabcd;
        struct S { n @0 :UInt32 = 7; text @1 :Text; }
        const value :S = embed "s.bin";
        const list :List(S) = [.value, embed "s.bin"];
        const erased :AnyPointer = .value;
    "#,
        )
        .unwrap();
    parser.add_file("s.bin", blob()).unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let value::Struct(pointer) = constant(request, "main.capnp:value").which().unwrap() else {
        panic!()
    };
    let value = pointer.get_as::<any_struct::Reader<'_>>().unwrap();
    assert_eq!(
        value.get_data_section(),
        &[34, 0, 0, 0, 0, 0, 0, 0, 99, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        value
            .get_pointer_section()
            .get(1)
            .get_as::<capnp::data::Reader<'_>>()
            .unwrap(),
        b"unknown"
    );
    let value::List(pointer) = constant(request, "main.capnp:list").which().unwrap() else {
        panic!()
    };
    let list = pointer
        .get_as::<capnp::any_struct_list::Reader<'_>>()
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap().get_data_section().len(), 8);
    assert_eq!(list.get(1).unwrap().get_pointer_section().len(), 1);
    let mut loader = capnp::schema_loader::SchemaLoader::default();
    loader.load_request(request).unwrap();
}

#[test]
fn imports_resolve_embeds_beside_the_declaring_file_and_leave_unused_embeds_unopened() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            r#"@0xabcdefabcdefabcd; const x :Text = import "sub/constants.capnp".text;"#,
        )
        .unwrap();
    parser.add_source("sub/constants.capnp", r#"@0xbbbbbbbbbbbbbbbb; const text :Text = embed "data"; const unused :Data = embed "missing";"#).unwrap();
    parser
        .add_file("sub/data", b"beside imported source".to_vec())
        .unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let value::Text(text) = constant(request, "main.capnp:x").which().unwrap() else {
        panic!()
    };
    assert_eq!(text.unwrap(), "beside imported source");
    let error = parser.parse(&["sub/constants.capnp"]).err().unwrap();
    assert_eq!(error.filename, "sub/constants.capnp");
    assert!(error.message.contains("missing"));
    assert!(parser.parse(&["main.capnp"]).is_ok());
}

#[test]
fn malformed_messages_and_embed_paths_are_rejected_with_source_locations() {
    let mut bad_messages = vec![vec![], vec![0], vec![0; 8], vec![255; 16]];
    let mut cycle = vec![0u8; 24];
    cycle[4..8].copy_from_slice(&2u32.to_le_bytes());
    cycle[14..16].copy_from_slice(&1u16.to_le_bytes());
    cycle[16..20].copy_from_slice(&0xfffffffcu32.to_le_bytes());
    cycle[22..24].copy_from_slice(&1u16.to_le_bytes());
    bad_messages.push(cycle);
    let mut wrong_root = message::Builder::new_default();
    wrong_root
        .set_root::<capnp::text::Owned>("not a struct")
        .unwrap();
    bad_messages.push(capnp::serialize::write_message_to_words(&wrong_root));
    for bytes in bad_messages {
        let mut parser = SchemaParser::new();
        parser
            .add_source(
                "bad.capnp",
                "@0xabcdefabcdefabcd;\nstruct S {}\nconst x :S = embed \"bad.bin\";",
            )
            .unwrap();
        parser.add_file("bad.bin", bytes).unwrap();
        let error = parser.parse(&["bad.capnp"]).err().unwrap();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("embedded message"), "{error}");
    }
    for path in [
        "",
        "../outside",
        "/../outside",
        "C:/file",
        r"a\b",
        r"a\0b",
        r"\xff",
    ] {
        let mut parser = SchemaParser::new();
        parser
            .add_source(
                "bad.capnp",
                format!(r#"@0xabcdefabcdefabcd; const x :Data = embed "{path}";"#),
            )
            .unwrap();
        assert!(parser.parse(&["bad.capnp"]).is_err(), "{path}");
    }
}

#[test]
fn disk_embeds_obey_roots_read_limits_and_schema_utf8_validation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("include")).unwrap();
    std::fs::write(
        dir.path().join("main.capnp"),
        r#"@0xabcdefabcdefabcd; const x :Data = embed "/blob";"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("include/blob"), [255]).unwrap();
    let mut compiler = FileCompiler::new();
    compiler
        .src_prefix(dir.path())
        .import_path(dir.path().join("include"));
    assert!(compiler.compile(&[dir.path().join("main.capnp")]).is_ok());
    let file = std::fs::File::create(dir.path().join("include/blob")).unwrap();
    file.set_len(capnp_compiler::MAX_SOURCE_BYTES as u64 + 1)
        .unwrap();
    assert!(compiler
        .compile(&[dir.path().join("main.capnp")])
        .err()
        .unwrap()
        .message
        .contains("4 MiB"));
    let mut parser = SchemaParser::new();
    assert!(parser
        .add_file("large", vec![0; capnp_compiler::MAX_SOURCE_BYTES + 1])
        .is_err());
}

#[test]
fn shared_embeds_cannot_bypass_expansion_or_total_input_limits() {
    let mut parser = SchemaParser::new();
    let declarations: String = (0..5)
        .map(|i| format!("const x{i} :Text = embed \"blob\";\n"))
        .collect();
    parser
        .add_source("main.capnp", format!("@0xabcdefabcdefabcd; {declarations}"))
        .unwrap();
    parser
        .add_file("blob", vec![b'x'; capnp_compiler::MAX_SOURCE_BYTES])
        .unwrap();
    assert!(parser
        .parse(&["main.capnp"])
        .err()
        .unwrap()
        .message
        .contains("expanded value size"));
    let mut parser = SchemaParser::new();
    let declarations: String = (0..4)
        .map(|i| format!("const x{i} :Data = embed \"blob{i}\";\n"))
        .collect();
    parser
        .add_source("main.capnp", format!("@0xabcdefabcdefabcd; {declarations}"))
        .unwrap();
    for i in 0..4 {
        parser
            .add_file(
                &format!("blob{i}"),
                vec![0; capnp_compiler::MAX_SOURCE_BYTES],
            )
            .unwrap();
    }
    assert!(parser
        .parse(&["main.capnp"])
        .err()
        .unwrap()
        .message
        .contains("total source/embed"));
}

#[test]
fn empty_embeds_count_toward_file_limits_and_rooted_sources_cannot_escape() {
    let mut parser = SchemaParser::new();
    let mut source = String::from("@0xabcdefabcdefabcd;");
    for i in 0..257 {
        source.push_str(&format!("const x{i} :Data = embed \"blob{i}\";"));
        parser.add_file(&format!("blob{i}"), vec![]).unwrap();
    }
    parser.add_source("main.capnp", source).unwrap();
    assert!(parser
        .parse(&["main.capnp"])
        .err()
        .unwrap()
        .message
        .contains("embedded file limit"));
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            r#"@0xabcdefabcdefabcd; const x :Data = import "/root.capnp".data;"#,
        )
        .unwrap();
    parser
        .add_source(
            "include/root.capnp",
            r#"@0xbbbbbbbbbbbbbbbb; const data :Data = embed "../outside";"#,
        )
        .unwrap();
    parser.add_file("outside", vec![1]).unwrap();
    parser.import_path("include").unwrap();
    let error = parser.parse(&["main.capnp"]).err().unwrap();
    assert!(error.message.contains("escapes its root"));
    assert_eq!(error.filename, "root.capnp");
}

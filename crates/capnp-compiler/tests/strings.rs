use capnp::schema_capnp::{code_generator_request, node, value};
use capnp_compiler::compile;

#[test]
fn strings_preserve_bytes_and_normalize_only_backtick_line_endings() {
    let source = concat!(
        "\u{feff}@0xabcdefabcdefabcd;\x0b\x0c\u{feff}",
        r#"const escapes :Data = "\a\b\f\n\r\t\v\'\"\\\?\0\12\123\377\777\x00\x80\xff";"#,
        "const lines :Text = \"prefix\"\n `a\\n#\"🦀\r\n `\r `last\n;",
        "const raw :Text = \"\t\r\0\u{7f}🦀\";",
        r#"const bytes :Text = "\xff\0\xc3" "\xa9";"#,
    );
    let message = compile("strings.capnp", source).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let values: Vec<_> = request
        .get_nodes()
        .unwrap()
        .iter()
        .filter_map(|n| {
            if let node::Const(c) = n.which().unwrap() {
                Some(c.get_value().unwrap())
            } else {
                None
            }
        })
        .collect();
    let value::Data(bytes) = values[0].which().unwrap() else {
        panic!()
    };
    assert_eq!(
        bytes.unwrap(),
        &[7, 8, 12, 10, 13, 9, 11, 39, 34, 92, 63, 0, 10, 83, 255, 255, 0, 128, 255]
    );
    let value::Text(text) = values[1].which().unwrap() else {
        panic!()
    };
    assert_eq!(text.unwrap(), "prefixa\\n#\"🦀\n\nlast\n");
    let value::Text(text) = values[2].which().unwrap() else {
        panic!()
    };
    assert_eq!(text.unwrap().as_bytes(), "\t\r\0\u{7f}🦀".as_bytes());
    let value::Text(text) = values[3].which().unwrap() else {
        panic!()
    };
    let text = text.unwrap();
    assert_eq!(text.as_bytes(), &[255, 0, 0xc3, 0xa9]);
    assert!(text.to_str().is_err());
}

#[test]
fn malformed_strings_and_all_utf8_prefixes_report_diagnostics_without_panics() {
    for value in [
        r#""\x""#,
        r#""\x0""#,
        r#""\xgg""#,
        r#""\8""#,
        r#""\u1234""#,
        r#""\🦀""#,
        "\"newline\n\"",
        "'single'",
    ] {
        let source = format!("@0xabcdefabcdefabcd; const bad :Text = {value};");
        let error = compile("bad.capnp", &source).err().expect(value);
        assert!(source.is_char_boundary(error.start));
        assert!(source.is_char_boundary(error.end));
    }
    let source = "@0xabcdefabcdefabcd; const text :Text = \"🦀\\x80\\123\"\n `more\r\n;";
    for end in (0..=source.len()).filter(|&end| source.is_char_boundary(end)) {
        let _ = compile("prefix.capnp", &source[..end]);
    }
}

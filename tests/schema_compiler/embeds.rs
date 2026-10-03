use super::*;

fn check(compiler: &std::path::Path, directory: &std::path::Path, body: &str, accepted: bool) {
    let source = format!("@0xabcdefabcdefabcd; {body}");
    fs::write(directory.join("test.capnp"), &source).unwrap();
    let reference = command(compiler)
        .current_dir(directory)
        .args(["compile", "-Iinclude", "-o-", "test.capnp"])
        .output()
        .unwrap();
    let mut frontend = capntproto_compiler::FileCompiler::new();
    frontend
        .src_prefix(directory)
        .import_path(directory.join("include"));
    let rust = frontend.compile(&[directory.join("test.capnp")]);
    assert_eq!(
        reference.status.success(),
        accepted,
        "{body}\n{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        rust.is_ok(),
        accepted,
        "{body}\n{}",
        rust.as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default()
    );
    if accepted {
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        eprintln!("string/embed case: {body}");
        compare_requests(
            rust.unwrap().get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
}

#[test]
fn extended_strings_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("include")).unwrap();
    let mut literals = vec![
        r#""\a\b\f\n\r\t\v\'\"\\\?""#.to_owned(),
        r#""\0\1\12\123\377\400\777\08\789\1234""#.to_owned(),
        r#""\x00\x7f\x80\xff\xFF0""#.to_owned(),
        r#""\xc3" "\xa9" "🦀""#.to_owned(),
        "\"\t\r\0\u{7f}\"".to_owned(),
        "\"prefix\"\n `a\\n#\"🦀\r\n `\r `last\n".to_owned(),
        "`first\n # between adjacent lines\n `second\n".to_owned(),
        "`\n".to_owned(),
    ];
    for n in [0, 1, 7, 8, 9, 10, 13, 31, 127, 128, 192, 255] {
        literals.push(format!(r#""\x{n:02x}""#));
        literals.push(format!(r#""\{n:03o}""#));
    }
    for literal in &literals {
        for ty in ["Text", "Data"] {
            check(
                &compiler,
                directory.path(),
                &format!("const value :{ty} = {literal}; struct S {{ value @0 :{ty} = .value; }}"),
                true,
            );
        }
    }
    check(
        &compiler,
        directory.path(),
        "\u{feff}\x0b\x0cstruct S {\u{feff} value @0 :Text = \"é\"; }",
        true,
    );
    for literal in [
        r#""\x""#,
        r#""\x0""#,
        r#""\xGG""#,
        r#""\8""#,
        r#""\9""#,
        r#""\u1234""#,
        r#""\U0001F980""#,
        r#""\z""#,
        r#""\🦀""#,
        "\"newline\n\"",
        "'single'",
        "`line without a terminating statement",
        "\"\\\n\"",
    ] {
        check(
            &compiler,
            directory.path(),
            &format!("const value :Text = {literal};"),
            false,
        );
    }
    fs::write(
        root().join("target/verification/schema-compiler/strings.txt"),
        format!(
            "{} accepted string requests matched; 13 malformed forms rejected by both frontends\n",
            literals.len() * 2 + 1
        ),
    )
    .unwrap();
}

fn message_blob(words: u16, pointers: u16, small_segments: bool) -> Vec<u8> {
    let mut message = message::Builder::new(
        message::HeapAllocator::new().first_segment_words(if small_segments { 0 } else { 1024 }),
    );
    let mut value = message
        .init_root::<capnp::any_pointer::Builder<'_>>()
        .init_as_any_struct(words, pointers);
    if words > 0 {
        value.get_data_section()[0] = 34;
    }
    if words > 1 {
        value.get_data_section()[8] = 99;
    }
    if pointers > 0 {
        value
            .get_pointer_section()
            .get(0)
            .set_as::<capnp::text::Owned>("embedded")
            .unwrap();
    }
    if pointers > 1 {
        value
            .get_pointer_section()
            .get(1)
            .set_as::<capnp::data::Owned>(&b"unknown"[..])
            .unwrap();
    }
    capnp::serialize::write_message_to_words(&message)
}

#[test]
fn embeds_and_imported_embed_values_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("include")).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("data"), [0, 255, 0xc3, 0xa9]).unwrap();
    fs::write(dir.path().join("empty"), []).unwrap();
    fs::write(dir.path().join("include/data"), "root").unwrap();
    fs::write(dir.path().join("sub/data"), "relative imported embed").unwrap();
    fs::write(
        dir.path().join("sub/source.capnp"),
        r#"@0xbbbbbbbbbbbbbbbb;
        const value :Text = embed "data";
        const unused :Data = embed "missing";
    "#,
    )
    .unwrap();
    let mut cases: Vec<String> = vec![
        r#"const text :Text = embed "data"; const data :Data = embed "data";"#,
        r#"const text :Text = embed "empty"; const data :Data = embed "empty";"#,
        r#"const value :Text = embed "/data";"#,
        r#"const value :Text = embed "test.capnp";"#,
        r#"const embed :Text = "literal"; const value :Text = .embed;"#,
        r#"const value :Data = (embed "data");"#,
        r#"const value :Data = embed "d\x61ta";"#,
        r#"const value :Text = import "sub/\x73ource.capnp".value;"#,
        r#"annotation a(*) :Data; $a(embed "data"); struct S $a(embed "empty") {}"#,
        r#"struct S { t @0 :Text = embed "data"; d @1 :Data = embed "empty"; }"#,
        r#"const value :List(Data) = [embed "data", embed "empty", embed "/data"];"#,
        r#"const value :Text = import "sub/source.capnp".value;"#,
        r#"using X = import "sub/source.capnp"; const value :Text = X.value;"#,
        r#"interface I { f @0 (t :Text = embed "data") -> (d :Data = embed "empty"); }"#,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (name, words, pointers, far) in [
        ("emptymsg", 0, 0, false),
        ("oldmsg", 1, 0, false),
        ("exactmsg", 1, 1, false),
        ("newmsg", 2, 2, false),
        ("farmsg", 2, 2, true),
    ] {
        fs::write(dir.path().join(name), message_blob(words, pointers, far)).unwrap();
        for value in [format!("embed \"{name}\""), format!("(embed \"{name}\")")] {
            cases.push(format!("struct S {{ n @0 :UInt32 = 7; text @1 :Text = \"fallback\"; }} const value :S = {value}; const list :List(S) = [.value, {value}]; struct Holder {{ s @0 :S = .value; }} const erased :AnyPointer = .value;"));
        }
    }
    fs::write(
        dir.path().join("nullmsg"),
        [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();
    let mut trailing = message_blob(1, 1, false);
    trailing.extend_from_slice(&[0xff; 8]);
    fs::write(dir.path().join("trailing"), trailing).unwrap();
    for name in ["nullmsg", "trailing"] {
        cases.push(format!(
            "struct S {{ n @0 :UInt32 = 7; text @1 :Text; }} const value :S = embed \"{name}\";"
        ));
    }
    cases.extend([
        r#"struct G(T) { value @0 :T; const x :G(T) = embed "farmsg"; } const value :G(Text) = G(Text).x;"#,
        r#"struct G(T) { value @0 :T; } const value :G(Text) = embed "farmsg"; const values :List(G(Text)) = [.value];"#,
        r#"struct S { n @0 :UInt32 = 7; text @1 :Text; } annotation a(*) :S; $a(embed "farmsg");"#,
        r#"struct S { union { a @0 :UInt32; b @1 :Text; } } const value :S = embed "farmsg";"#,
    ].map(str::to_owned));
    for body in &cases {
        check(&compiler, dir.path(), body, true);
    }
    let mut malformed = vec![vec![0; 8], vec![255; 16]];
    let mut wrong_root = message::Builder::new_default();
    wrong_root
        .set_root::<capnp::text::Owned>("not a struct")
        .unwrap();
    malformed.push(capnp::serialize::write_message_to_words(&wrong_root));
    let mut truncated = message_blob(1, 1, false);
    truncated.truncate(truncated.len() - 8);
    malformed.push(truncated);
    for bytes in &malformed {
        fs::write(dir.path().join("malformed"), bytes).unwrap();
        check(
            &compiler,
            dir.path(),
            r#"struct S {} const value :S = embed "malformed";"#,
            false,
        );
    }
    let invalid = [
        r#"struct S { g :group { n @0 :UInt32; text @1 :Text; } } const value :S = (g=embed "farmsg");"#,
        r#"const value :Text = embed "missing";"#,
        r#"const value :UInt32 = embed "data";"#,
        r#"const value :List(UInt32) = embed "data";"#,
        r#"const value :AnyPointer = embed "data";"#,
        r#"const value :AnyStruct = embed "farmsg";"#,
        r#"interface I {} const value :I = embed "farmsg";"#,
        r#"struct S {} const value :S = embed "data";"#,
        r#"struct S {} const value :S = embed "empty";"#,
        r#"const value :Text = embed "data" "empty";"#,
        r#"const value :Text = embed ("data");"#,
        r#"struct G(T) { const value :T = embed "data"; }"#,
    ];
    for body in invalid {
        check(&compiler, dir.path(), body, false);
    }
    fs::write(root().join("target/verification/schema-compiler/embeds.txt"), format!("{} accepted embed requests matched; {} malformed/type-invalid cases rejected by both frontends\n",cases.len(),invalid.len() + malformed.len())).unwrap();
}

#[test]
fn embeds_generate_working_rust_constants_and_mutable_defaults() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let mut parser = capntproto_compiler::SchemaParser::new();
    parser
        .add_source(
            "embedded.capnp",
            r#"@0xabcdefabcdefabcd;
        struct S { n @0 :UInt32 = 7; text @1 :Text = "fallback"; }
        const blob :Data = embed "data";
        const text :Text = "\x48\145llo\0" "🦀";
        const lines :Text =
            `first\n is literal
            `second
            ;
        const value :S = embed "message";
        const values :List(S) = [.value, embed "old"];
        struct Holder {
            item @0 :S = .value;
            bytes @1 :Data = embed "data";
            empty @2 :S = embed "old";
        }
    "#,
        )
        .unwrap();
    parser
        .add_file("message", message_blob(2, 2, true))
        .unwrap();
    parser.add_file("old", message_blob(0, 0, false)).unwrap();
    parser.add_file("data", vec![0, 255, 128]).unwrap();
    let request = parser.parse(&["embedded.capnp"]).unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-embed-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ package = \"capntproto-core\", path = {:?} }}\n",root().join("crates/capntproto-core"))).unwrap();
    fs::write(project.path().join("src/lib.rs"), r#"
pub mod embedded_capnp { include!("embedded_capnp.rs"); }
#[cfg(test)]
mod tests {
    use super::embedded_capnp::*;
    #[test]
    fn bytes_defaults_and_unknown_storage_survive_generation() {
        assert_eq!(BLOB, &[0,255,128]);
        assert_eq!(TEXT, "Hello\0🦀");
        assert_eq!(LINES, "first\\n is literal\nsecond\n");
        assert_eq!(VALUE.get().unwrap().get_n(), 37);
        assert_eq!(VALUE.get().unwrap().get_text().unwrap(), "embedded");
        let values = VALUES.get().unwrap();
        assert_eq!(values.get(0).get_n(), 37);
        assert_eq!(values.get(1).get_n(), 7);
        assert_eq!(values.get(1).get_text().unwrap(), "fallback");
        let mut message = capnp::message::Builder::new_default();
        let mut holder = message.init_root::<holder::Builder>();
        assert_eq!(holder.reborrow().get_bytes().unwrap(), &[0,255,128]);
        assert_eq!(holder.reborrow().get_empty().unwrap().get_n(), 7);
        let mut item = holder.reborrow().get_item().unwrap();
        assert_eq!(item.reborrow().get_n(), 37);
        item.set_n(91);
        item.set_text("mutated");
        let bytes = capnp::serialize::write_message_to_words(&message);
        let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
        let holder = decoded.get_root::<holder::Reader>().unwrap();
        let item = holder.get_item().unwrap();
        assert_eq!(item.get_n(), 91);
        assert_eq!(item.get_text().unwrap(), "mutated");
        let raw = capnp::any_struct::Reader::from_reader(item);
        assert_eq!(raw.get_data_section()[8], 99);
        assert_eq!(raw.get_pointer_section().get(1).get_as::<capnp::data::Reader>().unwrap(), b"unknown");
        assert_eq!(VALUE.get().unwrap().get_n(), 37);
    }
}
"#).unwrap();
    run(
        command("cargo")
            .args(["test", "--offline", "--manifest-path"])
            .arg(project.path().join("Cargo.toml"))
            .env(
                "CARGO_TARGET_DIR",
                root().join("target/schema-compiler-acceptance"),
            ),
        &root().join("target/verification/schema-compiler/embeds-rust.log"),
        0,
    )
    .unwrap();
}

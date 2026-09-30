use super::*;

#[test]
fn identifier_targets_and_ranges_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases = Vec::new();
    for prefix in ["", "\u{feff}# 🦀\r\n"] {
        for ty in [
            "Text",
            "S",
            ".S",
            "S(Text).N",
            "S(Text).P",
            "S.P",
            "Alias",
            "List(S)",
            "S.ListAlias",
        ] {
            for depth in 0..3 {
                cases.push(format!("{prefix}@0xabcdefabcdefabcd; struct S(T) {{ using P = T; using ListAlias = List(Text); struct N {{}} }} using Alias = S(Data); struct Use {{ x @0 : {}{ty}{}; }}", "(".repeat(depth), ")".repeat(depth)));
            }
        }
    }
    cases.extend([
        "@0xabcdefabcdefabcd; struct S { struct N {} } struct Use { x @0 :((S)).N; }",
        "@0xabcdefabcdefabcd; using Text = Data; struct S { x @0 :Text; }",
        "@0xabcdefabcdefabcd; const n :UInt32 = 4; const alias :UInt32 = ((.n));",
        "@0xabcdefabcdefabcd; annotation a (*) :Void; $((a)); struct S $a {}",
        "@0xabcdefabcdefabcd; struct S(T) { annotation a (*) :Void; } $(S(Text)).a;",
        "@0xabcdefabcdefabcd; struct G(T) { using P = T; annotation a(*) :P; } $G(Text).a(\"x\");",
    ].map(str::to_owned));
    for source in &cases {
        fs::write(directory.path().join("identifiers.capnp"), source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "identifiers.capnp"])
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "{source}\n{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = capnp_compiler::compile("identifiers.capnp", source)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        eprintln!("identifier case: {source}");
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    fs::write(root().join("target/verification/schema-compiler/identifiers.txt"), format!("{} additional accepted identifier-reference requests match pinned C++ after sorting and deduplication\n", cases.len())).unwrap();
}

#[test]
fn identifier_imports_and_multiple_requested_files_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let sources = [
        ("main.capnp", "@0xabcdefabcdefabcd; using T = import \"types.capnp\"; struct S { x @0 :T.Alias; n @1 :UInt32 = (import \"values.capnp\").n; }"),
        ("types.capnp", "@0xbbbbbbbbbbbbbbbb; using Alias = (import \"nested.capnp\").Item;"),
        ("nested.capnp", "@0xcccccccccccccccc; struct Item { t @0 :Text; }"),
        ("values.capnp", "@0xdddddddddddddddd; const n :UInt32 = 5;"),
    ];
    let mut parser = capnp_compiler::SchemaParser::new();
    for (path, source) in sources {
        fs::write(directory.path().join(path), source).unwrap();
        parser.add_source(path, source).unwrap();
    }
    for requested in [
        &["main.capnp"][..],
        &["main.capnp", "types.capnp"][..],
        &["values.capnp", "types.capnp", "main.capnp", "nested.capnp"][..],
    ] {
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-"])
            .args(requested)
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = parser.parse(requested).unwrap();
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
}

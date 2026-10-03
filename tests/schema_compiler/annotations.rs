use super::*;

#[test]
fn annotations_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases: Vec<String> = [
        "annotation a() :Void;",
        "annotation a(*) :Void; $a; struct S $a { f @0 :Bool $a; g :group $a { x @1 :Bool; } u :union $a { n @2 :Void; s @3 :Text; } } enum E $a { a @0 $a; } const x :Bool = true $a; annotation b(file) :Void $a;",
        "annotation a @0x9123456789abcdef (file) :Void; $a; $a(void);",
        "annotation a(struct) :Text; struct S $a(\"first\") $a(\"second\") {}",
        "annotation a(file) :Text; $(a)(\"text\");",
        "annotation a(file) :Text; $(a(\"text\"));",
        "annotation a(file) :Text; $((a))(\"text\");",
        "annotation a(file) :Text; $a((\"text\",));",
        "annotation a(file) :Text; $a(\"a\" \"b\");",
        "annotation a(file) :Void; $a; using b = .a; $b; $ .b;",
        "struct Scope { annotation a(file) :Text; const value :Text = \"nested\"; } $Scope.a(Scope.value);",
        "annotation a(struct) :Text; struct S $a(S.s) { const s :Text = \"nested\"; }",
        "annotation a(struct) :Void; struct S $a(\"inner\") { annotation a(struct) :Text; }",
        "annotation a(group) :Text; struct S { const x :Text = \"struct\"; g :group $a(S.x) { n @0 :UInt32; } }",
        "annotation a(struct) :S; struct S $a() { x @0 :UInt32 = 7; }",
        "annotation a(field) :S; struct S { x @0 :UInt32 $a(); }",
        "annotation a(file) :S; struct S { x @0 :UInt32 = 9; t @1 :Text; } $a(x = 4, t = \"x\"); $a((x = 8)); $a(3);",
        "annotation a(file) :List(S); struct S { x @0 :UInt32; } $a([(), (x = 8)]);",
        "annotation a(file) :List(List(UInt16)); $a([[1, 2], [], [3]]);",
        "annotation a(file) :AnyPointer; struct S { x @0 :UInt32; } const x :S = (x = 9); $a(.x);",
        "annotation a(file) :AnyPointer; const x :List(Text) = [\"one\",\"two\"]; $a(.x);",
        "annotation a(file) :S; struct S { g :group { union { a @0 :Void; b @1 :Text; } } } $a(g = (b = \"x\"), g = ());",
        "annotation a(annotation) :Void; annotation b(annotation) :Text $a; annotation c(*) :Bool $b(\"tag\");",
        "annotation a(const) :UInt32; const x :UInt32 = 1 $a(.y); const y :UInt32 = 2;",
        "annotation a(field) :Void; struct S { using a = .a; x @0 :Bool $a; }",
        "annotation a(enumerant, enum) :UInt16; enum E $a(1) { last @1 $a(2); first @0 $a(3); }",
    ].into_iter().map(str::to_owned).collect();
    cases.push(
        include_str!("../../crates/capnp-compiler/examples/annotations.capnp")
            .split_once(';')
            .unwrap()
            .1
            .to_owned(),
    );
    let targets = [
        "file",
        "const",
        "enum",
        "enumerant",
        "struct",
        "field",
        "union",
        "group",
        "interface",
        "method",
        "param",
        "annotation",
    ];
    for target in targets {
        cases.push(format!("annotation a({target},) :Void;"));
    }
    for (ty, val) in [
        ("Void", "void"),
        ("Bool", "true"),
        ("Int8", "-128"),
        ("Int16", "32767"),
        ("Int32", "-2147483648"),
        ("Int64", "-9223372036854775808"),
        ("UInt8", "255"),
        ("UInt16", "65535"),
        ("UInt32", "4294967295"),
        ("UInt64", "18446744073709551615"),
        ("Float32", "0.1"),
        ("Float64", "-0.0"),
        ("Text", "\"hé🦀\""),
        ("Data", "0x\"00ff\""),
        ("List(Bool)", "[true, false]"),
        ("List(Text)", "[\"\", \"x\"]"),
    ] {
        cases.push(format!(
            "annotation a(*) :{ty}; $a({val}); struct S $a({val}) {{ x @0 :Bool $a({val}); }}"
        ));
    }
    cases.push(
        "enum E { zero @0; one @1; } annotation a(*) :E; $a(one); struct S $a(zero) {}".into(),
    );
    for (index, body) in cases.iter().enumerate() {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "case {index}: {source}\n{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = capnp_compiler::compile("test.capnp", &source)
            .unwrap_or_else(|e| panic!("case {index}: {source}\n{e}"));
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    let invalid = [
        "annotation a(missing) :Void;", "annotation a(File) :Void;", "annotation a(file, file) :Void;",
        "annotation a(*, file) :Void;", "annotation a(file, *) :Void;", "annotation a(*, *) :Void;",
        "annotation A(file) :Void;", "annotation a_b(file) :Void;", "annotation a @5 (file) :Void;",
        "annotation a(file) :Text; $a;", "annotation a(file) :Void; $a();",
        "annotation a(file) :Bool; $a(1);", "annotation a(file) :UInt8; $a(256);",
        "annotation a(file) :Void; $a $a;", "annotation a(struct) :Void; $a;",
        "annotation a(file) :Void; struct S $a {}", "annotation a(field) :Void; struct S { g :group $a { x @0 :Bool; } }",
        "annotation a(group) :Void; struct S { u :union $a { x @0 :Bool; y @1 :Text; } }",
        "annotation a(union) :Void; struct S { union $a { x @0 :Bool; y @1 :Text; } }",
        "annotation a(annotation) :Void $a;", "annotation a(annotation) :Void $b; annotation b(annotation) :Void $a;",
        "annotation a(const) :UInt32; const x :UInt32 = 1 $a(.x);",
        "annotation a(annotation, const) :UInt32 $b(.x); annotation b(annotation) :UInt32; const x :UInt32 = 2 $a(3);",
        "annotation a(file) :Missing;", "annotation a(file) :a;", "annotation a(file) :Void; struct S { x @0 :a; }",
        "const a :Void = void; $a;", "struct A {} $A;", "$Text;", "$missing;",
        "annotation a(file) :Void; const x :UInt32 = .a;",
        "annotation a(file) :AnyPointer; $a([]);",
        "annotation a(file) :List(UInt8); $a([256]);",
        "annotation a(file) :Text; using A = .a;", "annotation a(struct) :Text; struct S $a(.s) { const s :Text = \"nested\"; }",
        "annotation a(file) :Void; struct S { $a; }",
        "annotation a(file) :Void; struct S { g :group { annotation b(field) :Void; x @0 :Bool; } }",
    ];
    for body in invalid {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(!reference.status.success(), "C++ accepted {body}");
        assert!(
            capnp_compiler::compile("test.capnp", &source).is_err(),
            "Rust accepted {body}"
        );
    }
    fs::create_dir_all(root().join("target/verification/schema-compiler")).unwrap();
    fs::write(root().join("target/verification/schema-compiler/annotations.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted annotation schemas match all known request fields and canonical annotation/default/constant bytes\n{} shared rejections\n", cases.len(), invalid.len())).unwrap();
}

#[test]
fn annotation_imports_and_dependencies_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        (
            "leaf.capnp",
            r#"@0xbbbbbbbbbbbbbbbb;
            annotation label(*) :Text;
            annotation meta(annotation) :Void;
            annotation item(file, struct) :Item $meta;
            struct Item { n @0 :UInt32 = 42; name @1 :Text = "default"; }
            const value :Item = (n = 7, name = "imported");
            const text :Text = "external";
            const annotated :Text = "tagged" $label("tag");
        "#,
        ),
        (
            "mid.capnp",
            r#"@0xcccccccccccccccc;
            using item = import "leaf.capnp".item;
            annotation label(file, struct) :Text $meta;
            using meta = import "leaf.capnp".meta;
            const text :Text = import "leaf.capnp".text;
            annotation broken(file) :import "missing.capnp".Type;
        "#,
        ),
        (
            "cycle.capnp",
            r#"@0xdddddddddddddddd;
            annotation tag(file) :Text;
            const text :Text = import "main.capnp".text;
        "#,
        ),
    ] {
        fs::write(directory.path().join(name), source).unwrap();
    }
    fs::write(
        directory.path().join("rust.capnp"),
        include_str!("../../vendor/capnpc/rust.capnp"),
    )
    .unwrap();
    fs::write(
        directory.path().join("c++.capnp"),
        include_str!("../../vendor/capnproto/c++/src/capnp/c++.capnp"),
    )
    .unwrap();
    let cases = [
        "$import \"leaf.capnp\".label(\"x\");",
        "$(import \"leaf.capnp\").label(\"x\");",
        "using L = import \"leaf.capnp\"; $L.label(L.text);",
        "using label = import \"leaf.capnp\".label; $label(\"x\");",
        "using import \"leaf.capnp\".label; $label(\"x\");",
        "$import \"mid.capnp\".label(import \"leaf.capnp\".text);",
        "$import \"mid.capnp\".item(n = 7);",
        "annotation label(file) :Text; $label(import \"mid.capnp\".text);",
        "annotation label(file) :Text; $label(import \"leaf.capnp\".annotated);",
        "annotation item(file) :import \"leaf.capnp\".Item; $item(n = 7);",
        "annotation item(file) :List(import \"leaf.capnp\".Item); $item([(), (n = 7)]);",
        "annotation item(file) :AnyPointer; $item(import \"leaf.capnp\".value);",
        "using L = import \"leaf.capnp\"; struct S $L.item(L.value) { x @0 :UInt32 $L.label(\"field\"); }",
        "const text :Text = \"from main\"; $import \"cycle.capnp\".tag(import \"cycle.capnp\".text);",
        "using R = import \"rust.capnp\"; $R.parentModule(\"container\"); struct S $R.name(\"Message\") { x @0 :Text $R.option $R.name(\"label\"); }",
        "using R = import \"rust.capnp\"; enum E $R.name(\"State\") { a @0 $R.name(\"ready\"); }",
        "using R = import \"rust.capnp\"; struct S { g :group $R.name(\"metadata\") { x @0 :Bool; } u :union $R.name(\"choice\") { a @1 :Void; b @2 :Text; } }",
        "using Cxx = import \"c++.capnp\"; $Cxx.namespace(\"example\"); $Cxx.allowCancellation; struct S {}",
    ];
    let mut frontend = capnp_compiler::FileCompiler::new();
    frontend.src_prefix(directory.path());
    for source in cases {
        fs::write(
            directory.path().join("main.capnp"),
            format!("@0xabcdefabcdefabcd; {source}"),
        )
        .unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "main.capnp"])
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
        let rust = frontend
            .compile(&[directory.path().join("main.capnp")])
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    for names in [
        ["main.capnp", "rust.capnp"],
        ["rust.capnp", "main.capnp"],
        ["main.capnp", "leaf.capnp"],
        ["main.capnp", "c++.capnp"],
    ] {
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-"])
            .args(names)
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
        let rust = frontend
            .compile(&names.map(|n| directory.path().join(n)))
            .unwrap();
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    let invalid = [
        "$import \"mid.capnp\".broken();",
        "$import \"leaf.capnp\".item(n = -1);",
        "$import \"leaf.capnp\".missing;",
        "$import \"leaf.capnp\".text(\"x\");",
        "using L = import \"leaf.capnp\"; struct S { x @0 :Bool $L.item(); }",
        "using R = import \"rust.capnp\"; struct S $R.option {}",
        "using R = import \"rust.capnp\"; $R.parentModule;",
        "using R = import \"rust.capnp\"; struct S { x @0 :Bool $R.name(1); }",
    ];
    for source in invalid {
        fs::write(
            directory.path().join("main.capnp"),
            format!("@0xabcdefabcdefabcd; {source}"),
        )
        .unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "main.capnp"])
            .output()
            .unwrap();
        assert!(!reference.status.success(), "C++ accepted {source}");
        assert!(
            frontend
                .compile(&[directory.path().join("main.capnp")])
                .is_err(),
            "Rust accepted {source}"
        );
    }
    fs::create_dir_all(root().join("target/verification/schema-compiler")).unwrap();
    fs::write(root().join("target/verification/schema-compiler/annotation-imports.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted annotation import graphs match all known request fields and canonical annotation/default/constant bytes\n{} shared rejections\n", cases.len() + 4, invalid.len())).unwrap();
}

#[test]
fn rust_annotations_generate_renamed_optional_fields_and_reflection() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let mut parser = capnp_compiler::SchemaParser::new();
    parser
        .add_source(
            "annotated.capnp",
            r#"
        @0xea1029b843d7fc65;
        using Rust = import "rust.capnp";
        $Rust.parentModule("container");
        annotation label @0xf1a0235c49bd786e (*) :Text;
        enum Status $Rust.name("State") {
            unknown @0 $Rust.name("idle");
            active @1 $Rust.name("ready") $label("available");
        }
        struct Record $Rust.name("Message") $label("record") {
            raw @0 :Text = "default" $Rust.name("title") $Rust.option;
            state @1 :Status = active;
            extra :group $Rust.name("metadata") {
                present @2 :Bool $Rust.name("enabled");
            }
            selection :union $Rust.name("choice") {
                unset @3 :Void $Rust.name("empty");
                content @4 :Data $Rust.name("bytes");
            }
        }
    "#,
        )
        .unwrap();
    parser
        .add_source("rust.capnp", include_str!("../../vendor/capnpc/rust.capnp"))
        .unwrap();
    let request = parser.parse(&["annotated.capnp"]).unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    assert!(!project.path().join("src/rust_capnp.rs").exists());
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-annotation-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    fs::write(project.path().join("src/lib.rs"), r#"
#![deny(unreachable_pub)]
pub mod container {
    pub mod annotated_capnp { include!("annotated_capnp.rs"); }
}
#[test]
fn round_trip_and_reflection() {
    use container::annotated_capnp::{label, message, State};
    let mut wire = capnp::message::Builder::new_default();
    {
        let mut root = wire.init_root::<message::Builder>();
        assert!(root.reborrow().get_title().unwrap().is_none());
        assert_eq!(root.reborrow().get_state().unwrap(), State::Ready);
        root.set_title("named");
        root.reborrow().get_metadata().set_enabled(true);
        root.reborrow().get_choice().set_bytes(&[0, 255]);
    }
    let bytes = capnp::serialize::write_message_to_words(&wire);
    let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let root = decoded.get_root::<message::Reader>().unwrap();
    assert_eq!(root.get_title().unwrap().unwrap(), "named");
    assert!(root.get_metadata().get_enabled());
    assert!(matches!(root.get_choice().which().unwrap(), message::choice::Which::Bytes(Ok([0, 255]))));
    // Names in schema reflection remain the original schema names.
    let schema = <message::Owned as capnp::introspect::Introspect>::introspect().as_struct_schema().unwrap();
    assert_eq!(schema.get_field_by_name("raw").unwrap().get_proto().get_name().unwrap(), "raw");
    assert!(schema.find_field_by_name("title").unwrap().is_none());
    for field in schema.get_fields().unwrap() {
        let name = field.get_proto().get_name().unwrap().to_str().unwrap();
        assert_eq!(schema.get_field_by_name(name).unwrap().get_index(), field.get_index());
    }
    let choice = <message::choice::Owned as capnp::introspect::Introspect>::introspect().as_struct_schema().unwrap();
    assert!(choice.find_field_by_name("bytes").unwrap().is_none());
    assert_eq!(choice.get_field_by_name("content").unwrap().get_index(), 1);
    let annotation = schema.get_annotations().unwrap().get(1);
    assert_eq!(annotation.get_id(), label::ID);
    assert!(matches!(annotation.get_value().unwrap(), capnp::dynamic_value::Reader::Text(t) if t == "record"));
    let mut other = capnp::message::Builder::new_default();
    let root = other.init_root::<message::Builder>().into_reader();
    assert!(root.get_title().unwrap().is_none());
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
        &root().join("target/verification/schema-compiler/annotations-rust.log"),
        0,
    )
    .unwrap();
}

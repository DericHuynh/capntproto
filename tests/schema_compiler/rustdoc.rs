use super::*;

fn generate(bytes: &[u8], directory: &std::path::Path, facade: bool) -> capnp::Result<()> {
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(directory)
        .field_api_values(facade)
        .field_api_projections(facade)
        .run(bytes)
}

#[test]
fn schema_documentation_renders_on_generated_rust_items_and_preserves_doctests() {
    let project = tempfile::tempdir().unwrap();
    let sources = project.path().join("schemas");
    fs::create_dir(&sources).unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let source =
        include_str!("../../crates/capnp-compiler/examples/rustdoc.capnp").replace('\n', "\r\n");
    fs::write(sources.join("docs.capnp"), source).unwrap();
    fs::write(sources.join("dep.capnp"), "@0xbbbbbbbbbbbbbbbb; # DEP-FILE-DOC\nstruct Item { # DEP-ITEM-DOC\nx @0 :UInt32; # DEP-FIELD-DOC\n}\n").unwrap();
    let cpp_includes = root().join("vendor/capnproto/c++/src");
    let rust_includes = root().join("vendor/capnpc");
    let request = capnp_compiler::FileCompiler::new()
        .src_prefix(&sources)
        .import_path(&cpp_includes)
        .import_path(&rust_includes)
        .compile(&[sources.join("docs.capnp"), sources.join("dep.capnp")])
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&request);
    let reference = command(
        cpp::build(&["capnp_tool"])
            .unwrap()
            .join("c++/src/capnp/capnp"),
    )
    .current_dir(&sources)
    .args(["compile", "-o-", "-I"])
    .arg(&cpp_includes)
    .arg("-I")
    .arg(&rust_includes)
    .args(["docs.capnp", "dep.capnp"])
    .output()
    .unwrap();
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let mut modules = String::new();
    for (name, facade) in [("legacy", false), ("facade", true)] {
        let output = project.path().join("src").join(name);
        generate(&bytes, &output, facade).unwrap();
        let cpp_output = project.path().join(format!("cpp-{name}"));
        generate(&reference.stdout, &cpp_output, facade).unwrap();
        for file in ["docs_capnp.rs", "dep_capnp.rs"] {
            let rust = fs::read_to_string(output.join(file)).unwrap();
            let cpp = fs::read_to_string(cpp_output.join(file)).unwrap();
            assert_eq!(
                rust.lines().skip(6).collect::<Vec<_>>(),
                cpp.lines().skip(6).collect::<Vec<_>>(),
                "{name}/{file}"
            );
        }
        modules.push_str(&format!("pub mod {name} {{ pub mod docs_capnp {{ include!(\"{name}/docs_capnp.rs\"); }} pub mod dep_capnp {{ include!(\"{name}/dep_capnp.rs\"); }} }}\n"));
    }
    // Generated paths include their parent module, just like CompilerCommand's option.
    // Generate again with that scope, after comparing both compiler inputs above.
    for (name, facade) in [("legacy", false), ("facade", true)] {
        capnpc::codegen::CodeGenerationCommand::new()
            .default_parent_module(vec![name.into()])
            .output_directory(project.path().join("src").join(name))
            .field_api_values(facade)
            .field_api_projections(facade)
            .run(bytes.as_slice())
            .unwrap();
    }
    fs::write(project.path().join("src/lib.rs"), modules).unwrap();
    fs::write(project.path().join("Cargo.toml"), format!("[package]\nname = \"rust-schema-docs\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    let target = root().join("target/schema-compiler-acceptance");
    let evidence = root().join("target/verification/schema-compiler/rustdoc");
    fs::create_dir_all(&evidence).unwrap();
    for args in [&["doc", "--no-deps"][..], &["test", "--doc"][..]] {
        run(
            command("cargo")
                .args(args)
                .args(["--offline", "--manifest-path"])
                .arg(project.path().join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .env("RUSTDOCFLAGS", "-D warnings"),
            &evidence.join(if args[0] == "doc" {
                "html.log"
            } else {
                "doctests.log"
            }),
            0,
        )
        .unwrap();
    }
    let docs = target.join("doc/rust_schema_docs");
    let has_doc = |file: &str, anchor: Option<&str>, expected: &str| {
        let page = fs::read_to_string(docs.join(file)).unwrap();
        let section = if let Some(anchor) = anchor {
            let (_, after) = page
                .split_once(&format!("id=\"{anchor}\""))
                .unwrap_or_else(|| panic!("missing {anchor} in {file}"));
            after.split("id=\"").next().unwrap()
        } else {
            &page
        };
        assert!(
            section.contains(expected),
            "missing {expected} in {file} at {anchor:?}"
        );
    };
    for mode in ["legacy", "facade"] {
        let file_page = fs::read_to_string(docs.join(format!(
            "{mode}/docs_capnp/schema_documentation_1/index.html"
        )))
        .unwrap();
        for expected in [
            "this is not Rust",
            "also not Rust",
            "literal ```` fence",
            "href=\"https://example.com/schema\"",
            "href=\"https://example.com/nested_(value)\"",
            "href=\"https://example.com/existing\"",
            "<code>http://code.example</code>",
        ] {
            assert!(
                file_page.contains(expected),
                "missing {expected} in file docs"
            );
        }
        for (file, marker) in [
            ("schema_documentation_1/index.html", "FILE-DOC"),
            ("schema_documentation/index.html", "COLLISION-DOC"),
            ("renamed_record/index.html", "RECORD-DOC"),
            ("renamed_record/struct.Reader.html", "RECORD-DOC"),
            ("renamed_record/struct.Builder.html", "RECORD-DOC"),
            ("renamed_record/struct.Owned.html", "RECORD-DOC"),
            ("renamed_record/metadata/index.html", "GROUP-DOC"),
            ("renamed_record/nested/index.html", "NESTED-DOC"),
            ("box_/index.html", "GENERIC-DOC"),
            ("enum.State.html", "STATE-DOC"),
            ("service/index.html", "SERVICE-DOC"),
            ("service/struct.Client.html", "SERVICE-DOC"),
            ("service/trait.Server.html", "SERVICE-DOC"),
            ("constant.LABEL.html", "LABEL-DOC"),
            ("static.LABELS.html", "LABELS-DOC"),
            ("note/index.html", "NOTE-DOC"),
        ] {
            has_doc(&format!("{mode}/docs_capnp/{file}"), None, marker);
        }
        for (file, anchor, marker) in [
            (
                "renamed_record/struct.Reader.html",
                "method.get_first",
                "FIRST-DOC",
            ),
            (
                "renamed_record/struct.Reader.html",
                "method.get_renamed_later",
                "LATER-DOC",
            ),
            (
                "renamed_record/struct.Reader.html",
                "method.has_renamed_later",
                "LATER-DOC",
            ),
            (
                "renamed_record/struct.Builder.html",
                "method.set_first",
                "FIRST-DOC",
            ),
            (
                "renamed_record/struct.Builder.html",
                "method.init_renamed_later",
                "LATER-DOC",
            ),
            (
                "renamed_record/struct.Pipeline.html",
                "method.get_imported",
                "IMPORT-DOC",
            ),
            ("renamed_record/enum.Which.html", "variant.None", "NONE-DOC"),
            ("renamed_record/enum.Which.html", "variant.Text", "TEXT-DOC"),
            ("enum.State.html", "variant.Unknown", "UNKNOWN-DOC"),
            ("enum.State.html", "variant.Available", "READY-DOC"),
            (
                "service/struct.Client.html",
                "method.get_request",
                "GET-DOC",
            ),
            ("service/trait.Server.html", "method.get", "GET-DOC"),
            (
                "service/struct.Client.html",
                "method.send_request",
                "SEND-DOC",
            ),
            ("service/trait.Server.html", "method.send", "SEND-DOC"),
        ] {
            has_doc(&format!("{mode}/docs_capnp/{file}"), Some(anchor), marker);
        }
        has_doc(
            &format!("{mode}/dep_capnp/item/index.html"),
            None,
            "DEP-ITEM-DOC",
        );
    }
    for (file, anchor, marker) in [
        ("api/struct.RenamedRecord.html", "method.first", "FIRST-DOC"),
        (
            "api/struct.RenamedRecord.html",
            "method.into_renamed_later",
            "LATER-DOC",
        ),
        (
            "api/struct.RenamedRecord.html",
            "associatedconstant.RENAMED_LATER",
            "LATER-DOC",
        ),
        (
            "api/enum.RenamedRecordUnionTag.html",
            "variant.Text",
            "TEXT-DOC",
        ),
        (
            "api/enum.RenamedRecordUnionRef.html",
            "variant.Text",
            "TEXT-DOC",
        ),
        (
            "api/enum.RenamedRecordUnionValue.html",
            "variant.Text",
            "TEXT-DOC",
        ),
        (
            "api/struct.RenamedRecordValue.html",
            "structfield.first",
            "FIRST-DOC",
        ),
        (
            "api/struct.RenamedRecordView.html",
            "structfield.first",
            "FIRST-DOC",
        ),
        ("service/struct.Client.html", "method.get_call", "GET-DOC"),
        ("service/struct.Client.html", "method.send_call", "SEND-DOC"),
    ] {
        has_doc(&format!("facade/docs_capnp/{file}"), Some(anchor), marker);
    }
}

#[test]
fn absent_partial_and_malformed_documentation_is_handled_without_panicking() {
    let output = tempfile::tempdir().unwrap();
    let source = "@0xabcdefabcdefabcd; struct S { a @0 :Text; b @1 :UInt32; }";
    for count in [0, 1] {
        let mut request = capnp_compiler::compile("test.capnp", source).unwrap();
        let mut root = request
            .get_root::<code_generator_request::Builder<'_>>()
            .unwrap();
        let id = root.reborrow().get_nodes().unwrap().get(1).get_id();
        let mut info = root.init_source_info(count);
        if count > 0 {
            let mut node = info.reborrow().get(0);
            node.set_id(id);
            node.set_doc_comment("PARTIAL-NODE-DOC");
            node.init_members(1)
                .get(0)
                .set_doc_comment("PARTIAL-FIELD-DOC");
        }
        let bytes = capnp::serialize::write_message_to_words(&request);
        for facade in [false, true] {
            generate(&bytes, output.path(), facade).unwrap();
            let code = fs::read_to_string(output.path().join("test_capnp.rs")).unwrap();
            assert_eq!(code.contains("PARTIAL-NODE-DOC"), count > 0);
            assert_eq!(code.contains("PARTIAL-FIELD-DOC"), count > 0);
        }
    }
    for duplicate in [false, true] {
        let mut request = capnp_compiler::compile("test.capnp", source).unwrap();
        let mut info = request
            .get_root::<code_generator_request::Builder<'_>>()
            .unwrap()
            .init_source_info(if duplicate { 2 } else { 1 });
        info.reborrow().get(0).set_id(0xabcdefabcdefabcd);
        if duplicate {
            info.get(1).set_id(0xabcdefabcdefabcd);
        } else {
            info.get(0).set_doc_comment(capnp::text::Reader(&[0xff]));
        }
        let bytes = capnp::serialize::write_message_to_words(&request);
        let error = generate(&bytes, output.path(), true).unwrap_err();
        assert!(
            error.to_string().contains(if duplicate {
                "duplicate source-info"
            } else {
                "utf-8"
            }),
            "{error}"
        );
    }
}

//! Compare the Rust textual frontend with the pinned C++ compiler, then compile
//! and execute Rust bindings generated from the Rust frontend's request.
use capnp::{message, schema_capnp::code_generator_request, schema_loader::SchemaLoader};
use reproto_test_support::verification::{command, cpp, root, run};
use std::{collections::BTreeMap, fs};

const SAMPLE: &str = include_str!("../crates/capnp-compiler/examples/message.capnp");

#[path = "schema_compiler/annotations.rs"]
mod annotations;
#[path = "schema_compiler/discovery.rs"]
mod discovery;
#[path = "schema_compiler/embeds.rs"]
mod embeds;
#[path = "schema_compiler/file_ids.rs"]
mod file_ids;
#[path = "schema_compiler/generics.rs"]
mod generics;
#[path = "schema_compiler/grammar.rs"]
mod grammar;
#[path = "schema_compiler/identifiers.rs"]
mod identifiers;
#[path = "schema_compiler/interfaces.rs"]
mod interfaces;
#[path = "schema_compiler/parsed.rs"]
mod parsed;
#[path = "schema_compiler/provider.rs"]
mod provider;
#[path = "schema_compiler/rustdoc.rs"]
mod rustdoc;
#[path = "schema_compiler/session.rs"]
mod session;
#[path = "schema_compiler/source_info.rs"]
mod source_info;
#[path = "schema_compiler/upstream.rs"]
mod upstream;

fn compare_annotations(
    actual: capnp::struct_list::Reader<'_, capnp::schema_capnp::annotation::Owned>,
    expected: capnp::struct_list::Reader<'_, capnp::schema_capnp::annotation::Owned>,
) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(
            capnp::any_struct::Reader::from_reader(actual)
                .canonicalize()
                .unwrap(),
            capnp::any_struct::Reader::from_reader(expected)
                .canonicalize()
                .unwrap(),
            "annotation wire value mismatch"
        );
    }
}

fn nodes(request: code_generator_request::Reader<'_>) -> BTreeMap<u64, String> {
    request
        .get_nodes()
        .unwrap()
        .iter()
        .map(|node| {
            // Pinned C++ preloads StreamResult from its older compiled schema,
            // whose Node range is zero even though SourceInfo has the range
            // from the source it just parsed. Compare that authoritative range
            // without erasing the Rust frontend's useful location metadata.
            if node.get_id() == 0x995f_9a33_77c0_b16e
                && node.get_start_byte() == 0
                && node.get_end_byte() == 0
            {
                let info = request
                    .get_source_info()
                    .unwrap()
                    .iter()
                    .find(|i| i.get_id() == node.get_id())
                    .unwrap();
                let mut copy = message::Builder::new_default();
                copy.set_root::<capnp::schema_capnp::node::Owned>(node)
                    .unwrap();
                let mut out = copy
                    .get_root::<capnp::schema_capnp::node::Builder<'_>>()
                    .unwrap();
                out.set_start_byte(info.get_start_byte());
                out.set_end_byte(info.get_end_byte());
                return (
                    node.get_id(),
                    format!(
                        "{:?}",
                        copy.get_root_as_reader::<capnp::schema_capnp::node::Reader<'_>>()
                            .unwrap()
                    ),
                );
            }
            (node.get_id(), format!("{node:?}"))
        })
        .collect()
}

fn compare_requests(
    rust: code_generator_request::Reader<'_>,
    reference: code_generator_request::Reader<'_>,
) {
    SchemaLoader::default().load_request(rust).unwrap();
    compare_request_fields(rust, reference);
}

fn compare_request_fields(
    rust: code_generator_request::Reader<'_>,
    reference: code_generator_request::Reader<'_>,
) {
    assert_eq!(nodes(rust), nodes(reference), "schema node mismatch");
    let infos = |request: code_generator_request::Reader<'_>| -> BTreeMap<_, _> {
        request
            .get_source_info()
            .unwrap()
            .iter()
            .map(|info| (info.get_id(), format!("{info:?}")))
            .collect()
    };
    assert_eq!(infos(rust), infos(reference), "source-info mismatch");
    let expected_info: BTreeMap<_, _> = reference
        .get_source_info()
        .unwrap()
        .iter()
        .map(|i| (i.get_id(), i))
        .collect();
    for info in rust.get_source_info().unwrap() {
        assert_eq!(
            capnp::any_struct::Reader::from_reader(info)
                .canonicalize()
                .unwrap(),
            capnp::any_struct::Reader::from_reader(expected_info[&info.get_id()])
                .canonicalize()
                .unwrap(),
            "source-info wire mismatch for {}",
            info.get_id(),
        );
    }
    let reference_nodes: BTreeMap<_, _> = reference
        .get_nodes()
        .unwrap()
        .iter()
        .map(|n| (n.get_id(), n))
        .collect();
    for node in rust.get_nodes().unwrap() {
        let expected = reference_nodes[&node.get_id()];
        compare_annotations(
            node.get_annotations().unwrap(),
            expected.get_annotations().unwrap(),
        );
        if let (
            capnp::schema_capnp::node::Enum(actual),
            capnp::schema_capnp::node::Enum(expected),
        ) = (node.which().unwrap(), expected.which().unwrap())
        {
            for (actual, expected) in actual
                .get_enumerants()
                .unwrap()
                .iter()
                .zip(expected.get_enumerants().unwrap())
            {
                compare_annotations(
                    actual.get_annotations().unwrap(),
                    expected.get_annotations().unwrap(),
                );
            }
        }
        if let (
            capnp::schema_capnp::node::Interface(actual),
            capnp::schema_capnp::node::Interface(expected),
        ) = (node.which().unwrap(), expected.which().unwrap())
        {
            for (actual, expected) in actual
                .get_methods()
                .unwrap()
                .iter()
                .zip(expected.get_methods().unwrap())
            {
                compare_annotations(
                    actual.get_annotations().unwrap(),
                    expected.get_annotations().unwrap(),
                );
            }
        }
        let bytes = |n| {
            capnp::Word::words_to_bytes(
                &capnp::any_struct::Reader::from_reader(n)
                    .canonicalize()
                    .unwrap(),
            )
            .to_vec()
        };
        // Debug compares all known fields, including source positions;
        // canonical values additionally
        // inspect the contents/nullness of opaque default pointers.
        if let (
            capnp::schema_capnp::node::Const(actual),
            capnp::schema_capnp::node::Const(expected),
        ) = (node.which().unwrap(), expected.which().unwrap())
        {
            assert_eq!(
                bytes(actual.get_value().unwrap()),
                bytes(expected.get_value().unwrap()),
                "constant value mismatch"
            );
        }
        if let (
            capnp::schema_capnp::node::Struct(actual),
            capnp::schema_capnp::node::Struct(expected),
        ) = (node.which().unwrap(), expected.which().unwrap())
        {
            for (actual, expected) in actual
                .get_fields()
                .unwrap()
                .iter()
                .zip(expected.get_fields().unwrap())
            {
                compare_annotations(
                    actual.get_annotations().unwrap(),
                    expected.get_annotations().unwrap(),
                );
                let capnp::schema_capnp::field::Slot(actual) = actual.which().unwrap() else {
                    continue; // Group type IDs and discriminants are compared above.
                };
                let capnp::schema_capnp::field::Slot(expected) = expected.which().unwrap() else {
                    panic!()
                };
                assert_eq!(
                    bytes(actual.get_default_value().unwrap()),
                    bytes(expected.get_default_value().unwrap()),
                    "default value mismatch"
                );
            }
        }
    }
    let files = rust.get_requested_files().unwrap();
    let expected_files = reference.get_requested_files().unwrap();
    assert_eq!(files.len(), expected_files.len());
    for (file, expected) in files.iter().zip(expected_files) {
        assert_eq!(file.get_id(), expected.get_id());
        assert_eq!(
            file.get_filename().unwrap(),
            expected.get_filename().unwrap()
        );
        assert_eq!(
            format!("{:?}", file.get_imports().unwrap()),
            format!("{:?}", expected.get_imports().unwrap())
        );
        assert!(file.has_file_source_info());
        assert!(file.get_file_source_info().unwrap().has_identifiers());
        // C++ reports resolutions during traversal, sometimes repeatedly.
        // The Rust frontend emits the same references once in source order.
        let identifiers = |file: code_generator_request::requested_file::Reader<'_>| {
            file.get_file_source_info()
                .unwrap()
                .get_identifiers()
                .unwrap()
                .iter()
                .map(|identifier| {
                    (
                        identifier.get_start_byte(),
                        identifier.get_end_byte(),
                        format!("{:?}", identifier),
                    )
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            identifiers(file),
            identifiers(expected),
            "identifier references in {:?}",
            file.get_filename().unwrap()
        );
    }
}

#[test]
fn supported_schemas_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases = vec![SAMPLE.to_owned()];
    let header = "@0xabcdefabcdefabcd;";
    for body in [
        "",
        "struct Empty {}",
        "enum Empty {}",
        "enum E { last @2; first @0; middle @1; } struct S { e @0 :E = middle; }",
        "struct A { b @0 :B; struct C { a @0 :A; } } struct B { c @0 :A.C; }",
        "struct S { struct Text {} value @0 :Text; }",
        "struct A { struct T {} struct B { struct T {} t @0 :T; outer @1 :A.T; } }",
        "struct S { x @0 :Text = \"hé🦀\\t\\r\\n\\\"\\\\\"; }",
        "struct S { a @0 :UInt32 = 0777; b @1 :Int64 = -0x8000000000000000; c @2 :Float32 = inf; d @3 :Float64 = -inf; e @4 :Float32 = nan; f @5 :Float64 = -0.0; }",
        "struct S { a @0 :List(Void); b @1 :List(Bool); c @2 :List(Data); d @3 :List(S); }",
    ] { cases.push(format!("{header}{body}")); }
    // All pairs of data widths and pointer slots, interleaved with a bool and
    // declared in reverse code order. This independently exercises hole reuse.
    let types = [
        "Void", "Bool", "UInt8", "UInt16", "UInt32", "UInt64", "Text", "Data",
    ];
    for left in types {
        for right in types {
            cases.push(format!(
                "{header} struct S {{ d @3 :{right}; c @2 :Bool; b @1 :{left}; a @0 :Bool; }}"
            ));
        }
    }
    // Deterministic longer layouts catch fragmentation beyond a single hole.
    let mut state = 0x7835a123u32;
    for _ in 0..32 {
        let mut source = format!("{header} struct S {{");
        for i in 0..64 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            source.push_str(&format!(
                "f{i} @{i} :{};",
                types[(state >> 16) as usize % types.len()]
            ));
        }
        source.push('}');
        cases.push(source);
    }
    let logs = root().join("target/verification/schema-compiler");
    fs::create_dir_all(&logs).unwrap();
    for (index, source) in cases.iter().enumerate() {
        let filename = ["test.capnp", "dir.with.dot/hé.capnp", "noExtension"][index % 3];
        fs::create_dir_all(directory.path().join("dir.with.dot")).unwrap();
        fs::write(directory.path().join(filename), source).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", filename])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "case {index}: {source}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reference =
            capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
                .unwrap();
        let reference = reference
            .get_root::<code_generator_request::Reader<'_>>()
            .unwrap();
        let rust = capnp_compiler::compile(filename, source).unwrap();
        let rust = rust
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        compare_requests(rust, reference);
    }
    let invalid = [
        "struct S { a @1 :Bool; }",
        "struct S { a @0 :Bool; b @0 :Bool; }",
        "struct S { a @0 :UInt8 = 256; }",
        "struct S { a @0 :Int8 = -129; }",
        "struct S { a @0 :Missing; }",
        "struct S { a @0 :Bool = 1; }",
        "struct S {} struct S {}",
        "struct S @0xabcdefabcdefabcd {}",
        "struct S { a @0 :List(AnyPointer); }",
    ];
    for body in invalid {
        let source = format!("{header}{body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "C++ accepted {body}");
        assert!(
            capnp_compiler::compile("test.capnp", &source).is_err(),
            "Rust accepted {body}"
        );
    }
    fs::write(logs.join("comparison.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} successful schemas: all known node fields, canonical default values and known requested-file fields match\n{} shared rejection cases\nNode/member ranges, SourceInfo/comments and sorted unique identifier references included; C++ compiler version excluded\n",
        cases.len(), invalid.len())).unwrap();
}

#[test]
fn rust_request_generates_compilable_bindings_and_round_trips() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let request = capnp_compiler::compile("message.capnp", SAMPLE).unwrap();
    let bytes = capnp::serialize::write_message_to_words(&request);
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(bytes.as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-schema-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    fs::write(
        project.path().join("src/lib.rs"),
        r#"
pub mod message_capnp;
#[test]
fn round_trip() {
    use message_capnp::{message, scalars};
    let mut wire = capnp::message::Builder::new_default();
    {
        let mut root = wire.init_root::<message::Builder>();
        assert_eq!(root.reborrow().get_sequence(), 42);
        assert_eq!(root.reborrow().get_title().unwrap(), "hello\nworld");
        assert_eq!(root.reborrow().get_state().unwrap(), message::State::Ready);
        assert!(root.reborrow().get_flag());
        root.set_sequence(900);
        root.set_title("from Rust schema compiler");
        root.set_flag(false);
        let mut samples = root.reborrow().init_samples(3);
        samples.set(0, -10); samples.set(1, 20); samples.set(2, i32::MAX);
        root.init_child().set_state(message::State::Ready);
    }
    let bytes = capnp::serialize::write_message_to_words(&wire);
    let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let root = decoded.get_root::<message::Reader>().unwrap();
    assert_eq!(root.get_sequence(), 900);
    assert_eq!(root.get_title().unwrap(), "from Rust schema compiler");
    assert!(!root.get_flag());
    assert_eq!(root.get_samples().unwrap().iter().collect::<Vec<_>>(), [-10, 20, i32::MAX]);
    assert_eq!(root.get_child().unwrap().get_state().unwrap(), message::State::Ready);
    let mut wire = capnp::message::Builder::new_default();
    let root = wire.init_root::<scalars::Builder>().into_reader();
    assert_eq!(root.get_i8(), i8::MIN);
    assert_eq!(root.get_i64(), i64::MIN);
    assert_eq!(root.get_u64(), u64::MAX);
    assert_eq!(root.get_f32(), 125.0);
}

"#,
    )
    .unwrap();
    run(
        command("cargo")
            .args(["test", "--offline", "--manifest-path"])
            .arg(project.path().join("Cargo.toml"))
            .env(
                "CARGO_TARGET_DIR",
                root().join("target/schema-compiler-acceptance"),
            ),
        &root().join("target/verification/schema-compiler/generated-rust.log"),
        0,
    )
    .unwrap();
}

#[test]
fn import_and_alias_graphs_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let header = "@0xabcdefabcdefabcd;";
    let dependencies = [
        ("dep.capnp", "@0xbbbbbbbbbbbbbbbb; using Number = UInt16; using Alias = S; enum E { zero @0; one @1; } struct S { n @0 :Number; struct Inner { n @0 :UInt8; } } struct Unused {}"),
        ("reexport.capnp", "@0xcccccccccccccccc; using X = import \"dep.capnp\"; using Renamed = X.Alias; using Values = List(X.E);"),
        ("include/public.capnp", "@0xdddddddddddddddd; struct Item {}"),
        ("include/sub/alias.capnp", "@0xeeeeeeeeeeeeeeee; using Item = import \"../public.capnp\".Item;"),
        ("groups.capnp", "@0xaaaabbbbccccddde; struct S { union { a @0 :Void; b :group { text @1 :Text; next @2 :S; } } } struct Unused {}"),
        ("lazy.capnp", "@0xaaaabbbbccccdddd; using Broken = import \"missing.capnp\"; struct S {} struct Unused { x @0 :import \"also-missing.capnp\".T; }"),
    ];
    let cases = [
        "using Dep = import \"dep.capnp\"; struct S { x @0 :Dep.S; }",
        "struct S { x @0 :import \"dep.capnp\".S; }",
        "using import \"dep.capnp\".S; struct Main { x @0 :S; }",
        "using Dep = import \"dep.capnp\"; using Dep.S; struct Main { x @0 :S; }",
        "using R = import \"reexport.capnp\"; struct Main { x @0 :R.Renamed; values @1 :R.Values; }",
        "using R = import \"/public.capnp\"; struct Main { x @0 :R.Item; }",
        "struct Main { x @0 :import \"dep.capnp\".S.Inner; }",
        "using Number = UInt64; using Dep = import \"dep.capnp\"; struct S { x @0 :Dep.Number; }",
        "using Dep = import \"dep.capnp\";", // Imported declarations are not emitted merely for an alias.
        "using Unused = import \"dep.capnp\".S; struct Empty {}",
        "using Number = UInt32; using Sequence = List; using Numbers = Sequence(Number); struct S { x @0 :Numbers; }",
        "using Number = UInt64; struct S { using Number = UInt8; a @0 :Number; b @1 : .Number; }",
        "using Third = Second; using Second = First; using First = UInt32; struct S { x @0 :Third; }",
        "struct A { using X = UInt32; } struct S { using A.X; x @0 :X; }",
        "struct S { using Dep = import \"dep.capnp\"; struct Child { x @0 :Dep.S; } }",
        "struct S { a @0 :import \"dep.capnp\".S; b @1 :import \"./dep.capnp\".S; c @2 :import \"sub/../dep.capnp\".S; }",
        "struct S { a @0 :import \"dep.capnp\".S; b @1 :import \"dep.capnp\".E; }",
        "using Dep = import \"dep.capnp\"; struct S { using Scope = Dep; a @0 :Scope.Alias; }",
        "using Dep = (import \"dep.capnp\"); struct S { a @0 :(Dep.S); b @1 :(List)(Dep.E,); }",
        "using (import \"dep.capnp\".S); struct Main { x @0 :S; }",
        "using Number = (UInt32,); struct S { x @0 :List((Number)); }",
        "using A = import \"/sub/alias.capnp\"; struct S { a @0 :A.Item; }",
        "using Lazy = import \"lazy.capnp\";",
        "using Lazy = import \"lazy.capnp\"; struct S { x @0 :Lazy.S; }",
        "using G = import \"groups.capnp\"; struct Main { value @0 :G.S; }",
        "struct Main { union { value :group { x @0 :import \"groups.capnp\".S; } empty @1 :Void; } }",
    ];
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("include/sub")).unwrap();
    fs::create_dir(directory.path().join("sub")).unwrap();
    for (path, source) in dependencies {
        fs::write(directory.path().join(path), source).unwrap();
    }
    let mut frontend = capnp_compiler::FileCompiler::new();
    frontend
        .src_prefix(directory.path())
        .import_path(directory.path().join("include"));
    for (index, body) in cases.iter().enumerate() {
        // Exercise file IDs before, between and after other declarations.
        let source = if index % 2 == 0 {
            format!("{header}{body}")
        } else {
            format!("{body}{header}")
        };
        fs::write(directory.path().join("main.capnp"), &source).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "-Iinclude", "main.capnp"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "case {index}: {source}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reference =
            capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
                .unwrap();
        let rust = frontend
            .compile(&[directory.path().join("main.capnp")])
            .unwrap();
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    fs::write(
        directory.path().join("main.capnp"),
        include_str!("../crates/capnp-compiler/examples/imports/main.capnp"),
    )
    .unwrap();
    fs::write(
        directory.path().join("common.capnp"),
        include_str!("../crates/capnp-compiler/examples/imports/common.capnp"),
    )
    .unwrap();
    for names in [
        &["main.capnp"][..],
        &["main.capnp", "common.capnp"][..],
        &["common.capnp", "main.capnp"][..],
    ] {
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-"])
            .args(names)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reference =
            capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
                .unwrap();
        let rust = frontend
            .compile(
                &names
                    .iter()
                    .map(|name| directory.path().join(name))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    let bad = [
        "using Dep = import \"missing.capnp\";",
        "using A = B; using B = A;",
        "using A = List(A);",
        "using A = UInt32; struct A {}",
        "using UInt32;",
        "using A = import \"dep.capnp\".Missing;",
        "struct S { x @0 :import \"dep.capnp\"; }",
        "using A = import \"reexport.capnp\"; struct S { x @0 :A.X.Missing; }",
        "using A = List(UInt32, UInt64);",
        "using A = List();",
        "using A = import \"dep.capnp\"; struct S { x @0 : .Missing; }",
        "using A = import \"lazy.capnp\".Broken;",
    ];
    for body in bad {
        fs::write(
            directory.path().join("main.capnp"),
            format!("{header}{body}"),
        )
        .unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "main.capnp"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "C++ accepted {body}");
        assert!(
            frontend
                .compile(&[directory.path().join("main.capnp")])
                .is_err(),
            "Rust accepted {body}"
        );
    }
    fs::write(root().join("target/verification/schema-compiler/imports.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} positive import/alias graphs; {} shared rejection cases\nAll known request fields and canonical default values match; identifier order/repetitions normalized; C++ compiler version excluded.\n", cases.len() + 3, bad.len())).unwrap();
}

#[test]
fn imported_rust_bindings_compile_and_round_trip_without_cpp() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let mut parser = capnp_compiler::SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            include_str!("../crates/capnp-compiler/examples/imports/main.capnp"),
        )
        .unwrap();
    parser
        .add_source(
            "common.capnp",
            include_str!("../crates/capnp-compiler/examples/imports/common.capnp"),
        )
        .unwrap();
    let request = parser.parse(&["main.capnp", "common.capnp"]).unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-import-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    fs::write(
        project.path().join("src/lib.rs"),
        r#"
pub mod main_capnp;
pub mod common_capnp;
#[test]
fn round_trip() {
    let mut message = capnp::message::Builder::new_default();
    let mut root = message.init_root::<main_capnp::message::Builder>();
    assert_eq!(root.reborrow().get_state().unwrap(), common_capnp::State::Ready);
    let mut record = root.reborrow().init_record();
    record.set_id(987);
    record.set_state(common_capnp::State::Ready);
    record.init_owner().init_record().set_id(654);
    root.init_batch(1).get(0).set_id(321);
    let bytes = capnp::serialize::write_message_to_words(&message);
    let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let root = decoded.get_root::<main_capnp::message::Reader>().unwrap();
    assert_eq!(root.get_record().unwrap().get_id(), 987);
    assert_eq!(root.get_record().unwrap().get_owner().unwrap().get_record().unwrap().get_id(), 654);
    assert_eq!(root.get_batch().unwrap().get(0).get_id(), 321);
}
"#,
    )
    .unwrap();
    run(
        command("cargo")
            .args(["test", "--offline", "--manifest-path"])
            .arg(project.path().join("Cargo.toml"))
            .env(
                "CARGO_TARGET_DIR",
                root().join("target/schema-compiler-acceptance"),
            ),
        &root().join("target/verification/schema-compiler/imported-rust.log"),
        0,
    )
    .unwrap();
}

#[test]
fn union_and_group_layouts_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases: Vec<String> = [
        "struct S { union { b @1 :Text; a @0 :Void; } }",
        "struct S { a @0 :UInt8; choice :union { b @1 :Bool; c @2 :UInt64; } }",
        "struct S { later :group { c @2 :Text; } first :group { a @0 :Bool; b @1 :UInt32; } }",
        "struct S { g :group { h :group { x @0 :UInt16; } y @2 :Text; } z @1 :Bool; }",
        "struct S { union { a :group { x @0 :Text; y @2 :Text; } b :group { x @1 :Data; y @3 :Data; } } }",
        "struct S { union { a :union { x @0 :Void; y @2 :Void; } b @1 :Void; } }",
        "struct S { union { a :group { union { x @0 :Void; y @2 :Void; } } b @1 :Void; } }",
        "struct S { g :group { union { a @2 :Text; b @0 :UInt64; } c @1 :Bool; } d @3 :UInt16; }",
        "struct S { old @0 :UInt64; choice @1! :union { a @2 :Bool; b @3 :Text; } }",
        "struct S { choice @1! :union { a @0 :UInt8; b @2 :UInt64; } }",
        "struct S { g @0! :union { h @1! :union { a @2 :Void; b @3 :Text; } c @4 :UInt32; } }",
        "struct S { struct T { union { a @0 :Bool; b @1 :Text; } } group :group { value @0 :T; } }",
        "enum E { a @0; b @1; } struct S { union { a @0 :E = b; b @1 :Text = \"hello\"; c @2 :Int32 = -5; d @3 :S; e @4 :List(S); } }",
        "struct S { a :union { first @2 :Bool; second @0 :UInt16; } b :union { first @3 :Text; second @1 :Void; } }",
    ].into_iter().map(str::to_owned).collect();
    let types = [
        "Void", "Bool", "UInt8", "UInt16", "UInt32", "UInt64", "Text", "Data",
    ];
    for left in types {
        for right in types {
            cases.push(format!("struct S {{ outside @0 :Bool; union {{ b @3 :{right}; a @1 :{left}; }} between @2 :UInt8; after @4 :Bool; }}"));
            cases.push(format!("struct S {{ union {{ b :group {{ x @3 :{right}; y @1 :{left}; }} a :group {{ x @0 :{right}; y @4 :{left}; }} }} outside @2 :Bool; }}"));
        }
    }
    // Interleaving groups by ordinal exercises growing shared locations, holes,
    // pointer reuse and alternatives which first appear late in source order.
    let mut state = 0x719452abu32;
    for _ in 0..64 {
        let mut source = String::from("struct S { union {");
        for group in 0..4 {
            source.push_str(&format!("g{group} :group {{"));
            for field in 0..16 {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                source.push_str(&format!(
                    "f{field} @{} :{};",
                    field * 4 + group,
                    types[(state >> 16) as usize % types.len()]
                ));
            }
            source.push('}');
        }
        source.push_str("} }");
        cases.push(source);
    }
    // Exercise the unmodified layout fixtures maintained by the reference project.
    cases.push(
        include_str!("../crates/capnp-compiler/examples/choices.capnp")
            .split_once(';')
            .unwrap()
            .1
            .to_owned(),
    );
    let upstream =
        fs::read_to_string(root().join("vendor/capnproto/c++/src/capnp/test.capnp")).unwrap();
    for name in [
        "TestUnion",
        "TestUnnamedUnion",
        "TestUnionInUnion",
        "TestGroups",
        "TestInterleavedGroups",
    ] {
        let tail = &upstream[upstream.find(&format!("struct {name} {{")).unwrap()..];
        cases.push(tail[..tail.find("\n}").unwrap() + 2].to_owned());
    }
    for (index, body) in cases.iter().enumerate() {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "case {index}: {body}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reference =
            capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
                .unwrap();
        let rust = capnp_compiler::compile("test.capnp", &source)
            .unwrap_or_else(|e| panic!("case {index}: {body}\n{e}"));
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    let invalid = [
        "struct S { union {} }",
        "struct S { union { a @0 :Void; } }",
        "struct S { g :group {} }",
        "struct S { g :union { a @0 :Void; } }",
        "struct S { union { a @0 :Void; b @1 :Void; } union { c @2 :Void; d @3 :Void; } }",
        "struct S { a @0 :Bool; union { a @1 :Void; b @2 :Void; } }",
        "struct S { g :group { a @0 :Bool; } b @0 :Bool; }",
        "struct S { g :group { a @1 :Bool; } }",
        "struct S { union { union { a @0 :Void; b @1 :Void; } c @2 :Void; } }",
        "struct S { g @0 :group { a @1 :Void; } }",
        "struct S { g @0 :union { a @1 :Void; b @2 :Void; } }",
        "struct S { g @2! :union { a @0 :Void; b @1 :Void; } }",
        "struct S { g :group { struct T {} a @0 :Void; } }",
        "struct S { union { using T = Bool; a @0 :Void; b @1 :Void; } }",
        "struct S { g :group { a @0 :Void; } b @1 :S.g; }",
    ];
    for body in invalid {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "C++ accepted {body}");
        assert!(
            capnp_compiler::compile("test.capnp", &source).is_err(),
            "Rust accepted {body}"
        );
    }
    let logs = root().join("target/verification/schema-compiler");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("unions.txt"), format!("reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} successful union/group schemas: all known node fields, canonical defaults and requested-file fields match\n{} shared rejection cases\n", cases.len(), invalid.len())).unwrap();
}

#[test]
fn union_group_bindings_switch_alternatives_and_preserve_other_fields() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let request = capnp_compiler::compile(
        "choices.capnp",
        include_str!("../crates/capnp-compiler/examples/choices.capnp"),
    )
    .unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-union-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    fs::write(project.path().join("src/lib.rs"), r#"
pub mod choices_capnp;
#[test]
fn round_trip() {
    use choices_capnp::message::{self, Which};
    let mut wire = capnp::message::Builder::new_default();
    {
        let mut root = wire.init_root::<message::Builder>();
        assert!(matches!(root.reborrow().which().unwrap(), Which::Empty(())));
        root.set_sequence(901);
        let mut metadata = root.reborrow().get_metadata();
        assert!(metadata.reborrow().get_enabled());
        assert_eq!(metadata.reborrow().get_title().unwrap(), "default");
        metadata.set_title("keep this");
        let mut reading = root.reborrow().init_reading();
        reading.set_value(12.5);
        reading.set_unit("ms");
        match root.reborrow().which().unwrap() {
            Which::Reading(mut reading) => {
                assert_eq!(reading.reborrow().get_value(), 12.5);
                assert_eq!(reading.reborrow().get_unit().unwrap(), "ms");
            }
            _ => panic!("expected reading"),
        }
        let mut error = root.reborrow().init_error();
        assert_eq!(error.reborrow().get_code(), 0);
        assert_eq!(error.reborrow().get_text().unwrap(), "");
        error.set_code(404);
        error.set_text("missing");
        let mut nested = root.reborrow().init_nested();
        assert!(matches!(nested.reborrow().which().unwrap(), message::nested::Which::None(())));
        nested.set_some(u64::MAX);
    }
    let bytes = capnp::serialize::write_message_to_words(&wire);
    let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let root = decoded.get_root::<message::Reader>().unwrap();
    assert_eq!(root.get_sequence(), 901);
    assert!(root.get_metadata().get_enabled());
    assert_eq!(root.get_metadata().get_title().unwrap(), "keep this");
    match root.which().unwrap() {
        Which::Nested(nested) => assert!(matches!(nested.which().unwrap(), message::nested::Which::Some(u64::MAX))),
        _ => panic!("expected nested"),
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
        &root().join("target/verification/schema-compiler/unions-rust.log"),
        0,
    )
    .unwrap();
}

#[test]
fn nested_union_permutations_match_cpp_including_historical_rejections() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let types = [
        "Void", "Bool", "UInt8", "UInt16", "UInt32", "UInt64", "Text",
    ];
    let mut state = 0x3e56a78bu32;
    let mut next = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 8) as usize
    };
    let mut accepted = 0;
    let mut rejected = 0;
    for case in 0..128 {
        let mut ordinals: Vec<_> = (0..12).collect();
        for i in (1..12).rev() {
            ordinals.swap(i, next() % (i + 1));
        }
        let fields: Vec<_> = ordinals
            .into_iter()
            .enumerate()
            .map(|(i, ordinal)| format!("f{i} @{ordinal} :{};", types[next() % types.len()]))
            .collect();
        let source = format!(
            "@0xabcdefabcdefabcd; struct S {{ union {{ a :group {{ union {{ x :group {{ {} }} y :group {{ {} }} }} {} }} b :group {{ {} }} }} }}",
            fields[..3].concat(), fields[3..6].concat(), fields[6..8].concat(), fields[8..].concat());
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .env_remove("CAPNP_IGNORE_ISSUE_344")
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        let rust = capnp_compiler::compile("test.capnp", &source);
        if reference.status.success() {
            let reference = capnp::serialize::read_message(
                reference.stdout.as_slice(),
                message::ReaderOptions::new(),
            )
            .unwrap();
            let rust = rust.unwrap_or_else(|e| panic!("case {case}: {source}\n{e}"));
            compare_requests(
                rust.get_root_as_reader().unwrap(),
                reference.get_root().unwrap(),
            );
            accepted += 1;
        } else {
            let stderr = String::from_utf8_lossy(&reference.stderr);
            assert!(
                stderr.contains("issues/344"),
                "case {case}: {source}\n{stderr}"
            );
            let error = rust
                .err()
                .unwrap_or_else(|| panic!("Rust accepted case {case}: {source}"));
            assert!(error.message.contains("issue #344"), "{error}");
            rejected += 1;
        }
    }
    assert!(accepted > 0 && rejected > 0);
    fs::write(root().join("target/verification/schema-compiler/nested-unions.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\nseed: 0x3e56a78b\n{accepted} accepted layouts match all known request fields and canonical default values\n{rejected} historical issue #344 rejections agree\n")).unwrap();
}

#[test]
fn constants_and_data_defaults_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases: Vec<String> = [
        "const n :UInt32 = .later; const later :UInt32 = 42;",
        "const n :UInt32 = 42; using renamed = .n; const result :UInt16 = .renamed;",
        "struct S { const n :UInt32 = 42; struct T { const m :UInt64 = S.n; } x @0 :UInt32 = S.T.m; }",
        "const voidValue :Void = void; const truth :Bool = true; const other :Bool = .truth; struct S { x @0 :Void = .voidValue; y @1 :Bool = .other; }",
        "const x :Data = \"\"; struct S { x @0 :Data = .x; y @1 :Data; }",
        "const x :Data = \"hé🦀\"; const y :Data = .x; struct S { x @0 :Data = .y; }",
        "const x :Data = 0x\"01 aF\n00\tFF \"; struct S { x @0 :Data = .x; }",
        "const x :Text = \"hé\" # comment\n \"🦀\"; struct S { x @0 :Text = .x; y @1 :Text = \"\"; z @2 :Text; }",
        "const x :UInt32 = (42,); const y :UInt64 = (.x); struct S { x @0 :UInt64 = (.y,); }",
        "const x :UInt8 = 3; struct S { group :group { n @0 :UInt32 = .x; } union { a @1 :UInt64 = .x; b @2 :Bool; } }",
        "const x :Float32 = 1e40; const y :Float64 = -1e400; struct S { x @0 :Float32 = .x; y @1 :Float64 = .y; }",
        "const x :Float32 = nan; const y :Float64 = .x; const z :Float32 = -inf; struct S { x @0 :Float32 = inf; }",
        "const x :Float32 = -0.0; const y :Float64 = .x; const z :Float32 = -0;",
        "enum E { a @0; b @1; } const x :E = b; const y :UInt16 = .x;",
        "enum E { true @0; false @1; inf @2; nan @3; void @4; } const x :E = void;",
        "const n :UInt32 = 42; struct S { const n :UInt16 = 12; x @0 :UInt32 = .n; y @1 :UInt16 = S.n; }",
        "const true :Bool = false; const x :Bool = true; const y :Bool = .true;",
    ].into_iter().map(str::to_owned).collect();
    cases.push(
        include_str!("../crates/capnp-compiler/examples/constants.capnp")
            .split_once(';')
            .unwrap()
            .1
            .to_owned(),
    );
    let mut invalid: Vec<String> = [
        "const x :UInt32 = .x;",
        "const x :UInt32 = .y; const y :UInt32 = .x;",
        "const x :UInt32 = 1; const y :UInt32 = x;",
        "const x :UInt32 = .missing;",
        "const x :UInt32 = 1; struct S { x @0 :x; }",
        "const x :UInt32 = UInt32;",
        "const X :UInt32 = 1;",
        "const x :UInt32 = 1; const x :UInt32 = 2;",
        "const x @0xabcdefabcdefabcd :UInt32 = 1;",
        "const x @3 :UInt32 = 1;",
        "struct S { x @0 :Bool; const x :UInt32 = 2; }",
        "struct S { const x :UInt32 = 2; x @0 :Bool; }",
        "struct S { using x = .x; x @0 :Bool; } const x :UInt32 = 1;",
        "struct S { g :group { const x :UInt32 = 1; n @0 :UInt32; } }",
        "struct S { union { const x :UInt32 = 1; a @0 :Void; b @1 :Void; } }",
        "const x :UInt32 = 1; using X = .x;",
        "using x = UInt32;",
        "const x :Data = 0x\"\";",
        "const x :Data = 0x\"a\";",
        "const x :Data = 0x\"a b\";",
        "const x :Data = 0x\"gg\";",
        "const x :Text = 0x\"aa\";",
        "const x :Text = \"text\"; const y :Data = .x;",
        "const x :Data = \"data\"; const y :Text = .x;",
        "const x :UInt32 = 1.0;",
        "const x :Float64 = -18446744073709551615;",
        "const x :Bool = 1;",
        "const x :Void = false;",
        "enum E { a @0; } const x :E = E.a;",
        "enum E { a @0; } const x :E = a; const y :E = .x;",
        "enum E { a @0; } const x :E = 0;",
        "const x :UInt32;",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let types = [
        "Int8", "Int16", "Int32", "Int64", "UInt8", "UInt16", "UInt32", "UInt64", "Float32",
        "Float64",
    ];
    for from in types {
        for to in types {
            let source = format!("const input :{from} = 7; const output :{to} = .input; struct S {{ x @0 :{to} = .output; }}");
            if from.starts_with("Float") && !to.starts_with("Float") {
                invalid.push(source);
            } else {
                cases.push(source);
            }
        }
    }
    for (ty, min, max) in [
        ("Int8", i8::MIN as i128, i8::MAX as i128),
        ("Int16", i16::MIN as i128, i16::MAX as i128),
        ("Int32", i32::MIN as i128, i32::MAX as i128),
        ("Int64", i64::MIN as i128, i64::MAX as i128),
        ("UInt8", 0, u8::MAX as i128),
        ("UInt16", 0, u16::MAX as i128),
        ("UInt32", 0, u32::MAX as i128),
        ("UInt64", 0, u64::MAX as i128),
    ] {
        for value in [min, max] {
            cases.push(format!(
                "const x :{ty} = {value}; struct S {{ x @0 :{ty} = .x; }}"
            ));
        }
        for value in [min - 1, max + 1] {
            // Rust deliberately rejects lexer overflow; the reference wraps
            // literals larger than UInt64 before checking the destination type.
            if value <= u64::MAX as i128 {
                invalid.push(format!("const x :{ty} = {value};"));
            }
        }
    }
    // Large integer-to-float casts and Float32 -> Float64 references must keep
    // C++'s rounding, rather than re-evaluating the source at destination precision.
    for value in [
        0,
        16_777_217,
        9_007_199_254_740_993,
        9_223_372_586_610_589_697,
        u64::MAX,
    ] {
        cases.push(format!("const n :UInt64 = {value}; const x :Float32 = .n; const y :Float64 = .x; struct S {{ x @0 :Float32 = {value}; y @1 :Float64 = .n; }}"));
    }
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
    for body in &invalid {
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
    // Preserve checked integer parsing even where the pinned lexer wraps.
    let overflow = "@0xabcdefabcdefabcd; const overflow :UInt64 = 18446744073709551616;";
    fs::write(directory.path().join("test.capnp"), overflow).unwrap();
    let reference = command(&compiler)
        .current_dir(directory.path())
        .args(["compile", "-o-", "test.capnp"])
        .output()
        .unwrap();
    assert!(reference.status.success());
    let reference =
        capnp::serialize::read_message(reference.stdout.as_slice(), message::ReaderOptions::new())
            .unwrap();
    let request = reference
        .get_root::<code_generator_request::Reader<'_>>()
        .unwrap();
    let constant = request
        .get_nodes()
        .unwrap()
        .iter()
        .find_map(|n| {
            if let capnp::schema_capnp::node::Const(c) = n.which().unwrap() {
                Some(c)
            } else {
                None
            }
        })
        .unwrap();
    assert!(matches!(
        constant.get_value().unwrap().which().unwrap(),
        capnp::schema_capnp::value::Uint64(0)
    ));
    assert!(capnp_compiler::compile("test.capnp", overflow)
        .err()
        .unwrap()
        .message
        .contains("out of range"));
    fs::write(root().join("target/verification/schema-compiler/constants.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} successful constant/Data schemas match all known node/request fields and canonical constant/default bytes\n{} shared rejection cases\nIntentional divergence verified: integer literal 2^64 is rejected in Rust; pinned C++ wraps to zero\n", cases.len(), invalid.len())).unwrap();
}

#[test]
fn imported_constants_are_inlined_and_resolved_lazily_like_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("leaf.capnp", "@0xbbbbbbbbbbbbbbbb; const number :UInt32 = 42; const text :Text = \"hé🦀\"; const data :Data = 0x\"00FF\"; enum E { zero @0; one @1; } const en :E = one; struct Scope { const number :UInt16 = 7; }"),
        ("mid.capnp", "@0xcccccccccccccccc; using Missing = import \"missing.capnp\"; using renamed = import \"leaf.capnp\".number; const answer :UInt64 = .renamed; const broken :UInt32 = Missing.value;"),
        ("cycle.capnp", "@0xdddddddddddddddd; using Main = import \"main.capnp\"; const answer :UInt32 = Main.answer;"),
        ("enum.capnp", "@0xeeeeeeeeeeeeeeee; enum E { zero @0; one @1; }"),
    ] { fs::write(directory.path().join(name), source).unwrap(); }
    let cases = [
        "const x :UInt32 = import \"mid.capnp\".answer;",
        "const x :UInt32 = import \"leaf.capnp\".number; using L = import \"leaf.capnp\"; struct S { x @0 :L.Scope; }",
        "using M = import \"mid.capnp\"; struct S { x @0 :UInt32 = M.answer; }",
        "using n = import \"leaf.capnp\".number; const answer :UInt32 = .n;",
        "using import \"leaf.capnp\".number; const answer :UInt32 = .number;",
        "using L = import \"leaf.capnp\"; using L.Scope.number; const answer :UInt32 = .number;",
        "const n :UInt32 = (import \"leaf.capnp\").Scope.number;",
        "using L = import \"leaf.capnp\"; struct S { const n :UInt16 = (L.Scope).number; x @0 :UInt32 = S.n; }",
        "using L = import \"leaf.capnp\"; struct S { t @0 :Text = L.text; d @1 :Data = L.data; }",
        "const answer :UInt32 = 8; const x :UInt32 = import \"cycle.capnp\".answer;",
        "using E = import \"enum.capnp\".E; const en :E = one;",
        "using L = import \"leaf.capnp\"; const en :L.E = one; struct S { raw @0 :UInt16 = L.en; }",
        "using L = import \"leaf.capnp\"; const answer :UInt32 = L.number; struct S { x @0 :L.Scope; }",
    ];
    let mut frontend = capnp_compiler::FileCompiler::new();
    frontend.src_prefix(directory.path());
    for (index, source) in cases.iter().enumerate() {
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
            "case {index}: {source}\n{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = frontend
            .compile(&[directory.path().join("main.capnp")])
            .unwrap_or_else(|e| panic!("case {index}: {source}\n{e}"));
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    for names in [["main.capnp", "leaf.capnp"], ["leaf.capnp", "main.capnp"]] {
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
        "const answer :UInt32 = import \"cycle.capnp\".answer;",
        "const answer :UInt32 = import \"mid.capnp\".broken;",
        "const answer :UInt32 = import \"leaf.capnp\".absent;",
        "using L = import \"leaf.capnp\"; const x :L.E = L.en;",
        "using L = import \"leaf.capnp\"; const x :UInt32 = L;",
        "using L = import \"leaf.capnp\"; const x :UInt32 = L.Scope;",
        "using L = import \"leaf.capnp\"; const x :L.number = 1;",
        "using n = import \"leaf.capnp\".number; const x :UInt32 = n;",
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
    fs::write(root().join("target/verification/schema-compiler/constant-imports.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} successful constant import graphs match all known node/request fields and canonical constant/default bytes\n{} shared rejection cases\n", cases.len() + 2, invalid.len())).unwrap();
}

#[test]
fn constant_bindings_and_imported_data_defaults_compile_and_round_trip() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let mut parser = capnp_compiler::SchemaParser::new();
    parser
        .add_source(
            "constants.capnp",
            format!(
                "{}\nconst external :Data = import \"external.capnp\".bytes;",
                include_str!("../crates/capnp-compiler/examples/constants.capnp")
            ),
        )
        .unwrap();
    parser
        .add_source(
            "external.capnp",
            "@0xbbbbbbbbbbbbbbbb; const bytes :Data = 0x\"0080FF\";",
        )
        .unwrap();
    let request = parser.parse(&["constants.capnp"]).unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    assert!(!project.path().join("src/external_capnp.rs").exists());
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-constant-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    fs::write(
        project.path().join("src/lib.rs"),
        r#"
pub mod constants_capnp;
#[test]
fn round_trip() {
    use constants_capnp::{self as constants, settings, State};
    assert_eq!(constants::LIMIT, 64);
    assert_eq!(constants::LABEL, "Rust frontend");
    assert_eq!(constants::SIGNATURE, [0, 255, 126, 128]);
    assert_eq!(constants::EXTERNAL, [0, 128, 255]);
    assert_eq!(constants::PREFERRED, State::Ready);
    assert_eq!(settings::MAX_COUNT, 64);
    let mut wire = capnp::message::Builder::new_default();
    {
        let mut root = wire.init_root::<settings::Builder>();
        assert_eq!(root.reborrow().get_count(), settings::MAX_COUNT);
        assert_eq!(root.reborrow().get_name().unwrap(), constants::LABEL);
        assert_eq!(root.reborrow().get_signature().unwrap(), constants::SIGNATURE);
        assert_eq!(root.reborrow().get_fraction(), f64::from(constants::FRACTION));
        assert_eq!(root.reborrow().get_state().unwrap(), constants::PREFERRED);
        root.set_count(65);
        root.set_signature(&[4, 5, 6]);
        root.set_state(State::Unknown);
    }
    let bytes = capnp::serialize::write_message_to_words(&wire);
    let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let root = decoded.get_root::<settings::Reader>().unwrap();
    assert_eq!(root.get_count(), 65);
    assert_eq!(root.get_signature().unwrap(), [4, 5, 6]);
    assert_eq!(root.get_state().unwrap(), State::Unknown);
    assert_eq!(root.get_name().unwrap(), constants::LABEL);
    assert_eq!(root.get_fraction(), f64::from(constants::FRACTION));
}
"#,
    )
    .unwrap();
    run(
        command("cargo")
            .args(["test", "--offline", "--manifest-path"])
            .arg(project.path().join("Cargo.toml"))
            .env(
                "CARGO_TARGET_DIR",
                root().join("target/schema-compiler-acceptance"),
            ),
        &root().join("target/verification/schema-compiler/constants-rust.log"),
        0,
    )
    .unwrap();
}

#[test]
fn composite_values_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases: Vec<String> = [
        "struct S { x @0 :UInt32 = 7; t @1 :Text = \"default\"; } const x :S = ();",
        "struct S { x @0 :UInt32 = 7; t @1 :Text = \"default\"; } const x :S = (x = 3, x = 4);",
        "struct S { union { x @0 :UInt64; y @1 :UInt8; } } const x :S = (x = 999999, y = 1);",
        "struct S { union { x @0 :Text; y @1 :Void; } } const x :S = (x = \"hello\", y = void);",
        "struct S { n @0 :S = (n = ()); }",
        "struct S { n @0 :S = .x; } const x :S = ();",
        "struct S { x @0 :UInt32 = 7; } const x :S = 5; const xs :List(S) = [0, 5, (), (x = 9), .x,];",
        "struct S { x @0 :Text = \"default\"; } const x :S = \"hello\";",
        "struct S { x @0 :Data; } const bytes :Data = 0x\"ff\"; const x :S = .bytes;",
        "struct S { g :group { x @0 :UInt32; } } const x :S = (g = 5);",
        "struct S { g :group { x @0 :UInt8; y @1 :UInt32; } } const x :S = (g = (x = 3), g = (y = 4));",
        "struct S { g :group { union { zero @0 :Void; text @1 :Text; n @2 :UInt64; } } } const x :S = (g = (text = \"retain\"), g = ());",
        "struct S { g :group { union { zero @0 :UInt8; n @1 :UInt64; } } } const x :S = (g = (n = 999999), g = ());",
        "struct S { union { a :group { n @0 :UInt32 = 42; x @1 :Text; } b :group { n @2 :UInt64; x @3 :Data; } } } const x :S = (b = (n = 999, x = 0x\"00ff\"), a = (n = 7));",
        "const x :List(UInt16) = [1,2]; const y :AnyPointer = .x; struct S { p @0 :AnyPointer = .x; }",
        "struct S { x @0 :UInt32; } const x :S = (); const y :AnyPointer = .x;",
        "const x :List(List(Void)) = [[], [void, void],];",
        "const x :List(List(UInt16)) = [[1,2], [], [3]]; const y :List(List(UInt16)) = .x;",
        "const x :List(Text) = [\"\", \"hé🦀\", \"a\" \"b\"]; const y :List(Data) = [\"\", 0x\"00ff\", \"text\"];",
        "enum E { a @0; b @1; } const x :List(E) = [a, b, a];",
        "struct Empty {} const x :Empty = (); const xs :List(Empty) = [(), (), ()];",
        "struct S { x @0 :UInt64 = 42; p @1 :Text = \"default\"; } const xs :List(S) = [(), (x = 42), (p = \"\"), (x = 9, p = \"x\")]; struct T { s @0 :S = (); xs @1 :List(S) = .xs; }",
        "struct S { x @0 :List(UInt16); } const x :List(UInt16) = [1,2]; const wrapped :S = .x;",
        "struct S { x @0 :S; } const a :S = (); const b :S = (x = .a); const c :S = (x = .b);",
    ].into_iter().map(str::to_owned).collect();
    cases.push(
        include_str!("../crates/capnp-compiler/examples/composites.capnp")
            .split_once(';')
            .unwrap()
            .1
            .to_owned(),
    );
    for (ty, values) in [
        ("Void", "void, void"),
        ("Bool", "true, false, true"),
        ("Int8", "-128, 0, 127"),
        ("Int16", "-32768, 9, 32767"),
        ("Int32", "-2147483648, 0, 2147483647"),
        ("Int64", "-9223372036854775808, 0, 9223372036854775807"),
        ("UInt8", "0, 255"),
        ("UInt16", "0, 65535"),
        ("UInt32", "0, 4294967295"),
        ("UInt64", "0, 18446744073709551615"),
        ("Float32", "-0.0, 0.1, inf, -inf, nan"),
        ("Float64", "-0.0, 0.1, inf, -inf, nan"),
    ] {
        cases.push(format!("const x :List({ty}) = [{values}]; const empty :List({ty}) = []; struct S {{ x @0 :List({ty}) = .x; empty @1 :List({ty}) = []; }}"));
    }
    let choices = [
        ("Void", "void", "void"),
        ("Bool", "true", "false"),
        ("Int8", "-7", "100"),
        ("UInt16", "43981", "1234"),
        ("Int32", "-1234", "-5678"),
        ("UInt64", "18446744073709551615", "1234567890"),
        ("Float32", "-0.0", "0.1"),
        ("Float64", "1.5", "-1.0"),
        ("Text", "\"default\"", "\"hé🦀\""),
        ("Data", "0x\"ff\"", "0x\"000102\""),
        ("E", "one", "zero"),
        ("T", "(x = 42)", "(x = 99)"),
        ("List(Int16)", "[1,2]", "[-1,5,8]"),
    ];
    let mut state = 0x49a35cb7u32;
    for _ in 0..32 {
        let mut source =
            String::from("enum E { zero @0; one @1; } struct T { x @0 :UInt16 = 8; } struct S {");
        let mut assignments = Vec::new();
        for i in 0..32 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let (ty, default, value) = choices[(state >> 16) as usize % choices.len()];
            source.push_str(&format!("f{i} @{i} :{ty} = {default};"));
            assignments.push(format!("f{i} = {value}"));
        }
        source.push_str(&format!(
            "}} const value :S = ({}); const values :List(S) = [(), .value];",
            assignments.join(",")
        ));
        cases.push(source);
    }
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
        "const x :List(UInt8) = [256];",
        "const x :List(UInt8) = [true];",
        "const x :Bool = [];",
        "const x :UInt32 = ();",
        "struct S {} const x :S = (missing = 1);",
        "struct S { x @0 :UInt8; } const x :S = (1,2);",
        "struct S { x @0 :UInt8; } const x :S = (x = 1, 2);",
        "struct S { x @0 :Data; } const x :S = \"foo\";",
        "struct S { x @0 :Data; } const x :S = 0x\"ff\";",
        "enum E { a @0; } struct S { x @0 :E; } const x :S = a;",
        "struct S { g :group { x @0 :UInt32; } } const x :S = 5;",
        "struct S { x @0 :List(UInt8); } const x :S = [1,2];",
        "const x :List(UInt16) = [1,2]; const y :List(UInt32) = .x;",
        "struct A { n @0 :UInt32; } struct B { n @0 :UInt32; } const x :A = (); const y :B = .x;",
        "const x :AnyPointer = [];",
        "const x :AnyPointer = ();",
        "const x :AnyPointer = \"x\";",
        "const x :List(UInt16) = [1,2]; const y :AnyPointer = .x; const z :AnyPointer = .y;",
        "struct S { x @0 :S; } const x :S = (x = .x);",
        "struct S { x @0 :S; } const x :S = (x = .y); const y :S = (x = .x);",
        "const x :List(UInt32) = [1,,2];",
        "struct S {} const x :S = (,);",
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
    fs::write(root().join("target/verification/schema-compiler/composites.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted composite schemas match all known request fields and canonical constant/default bytes\n{} shared rejections\n", cases.len(), invalid.len())).unwrap();
}

#[test]
fn imported_composites_and_erased_type_dependencies_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("leaf.capnp"),
        r#"
        @0xbbbbbbbbbbbbbbbb;
        enum E { zero @0; one @1; }
        struct Item {
            id @0 :UInt16 = 42;
            name @1 :Text = "default";
            state @2 :E = one;
            g :group { enabled @3 :Bool = true; }
            children @4 :List(Item);
        }
        const empty :Item = ();
        const item :Item = (id = 7, g = (enabled = false), children = [()]);
        const items :List(Item) = [.empty, .item];
        const nested :List(List(Item)) = [[], .items];
        const bytes :Data = 0x"00ff";
    "#,
    )
    .unwrap();
    fs::write(
        directory.path().join("mid.capnp"),
        r#"
        @0xcccccccccccccccc;
        using Item = import "leaf.capnp".Item;
        const item :Item = import "leaf.capnp".item;
        const items :List(Item) = import "leaf.capnp".items;
        const broken :UInt32 = import "missing.capnp".number;
    "#,
    )
    .unwrap();
    let cases = [
        "const p :AnyPointer = import \"leaf.capnp\".item;",
        "const p :AnyPointer = import \"leaf.capnp\".items;",
        "const p :AnyPointer = import \"leaf.capnp\".nested;",
        "struct S { p @0 :AnyPointer = import \"mid.capnp\".item; }",
        "struct S { p @0 :AnyPointer = import \"mid.capnp\".items; }",
        "using L = import \"leaf.capnp\"; const p :L.Item = L.item;",
        "using I = import \"mid.capnp\".Item; const p :List(I) = import \"mid.capnp\".items;",
        "using i = import \"mid.capnp\".item; const p :AnyPointer = .i;",
        "using L = import \"leaf.capnp\"; const p :L.Item = (state = zero, g = (), children = [L.item]);",
        "using L = import \"leaf.capnp\"; const id :UInt16 = 99; const p :L.Item = (id = .id, children = [()]);",
        "using L = import \"leaf.capnp\"; struct S { item @0 :L.Item = L.item; items @1 :List(L.Item) = L.items; }",
        "using L = import \"leaf.capnp\"; const p :List(List(L.Item)) = L.nested;",
        "struct S { data @0 :Data; } const p :S = import \"leaf.capnp\".bytes;",
        "struct S { p @0 :AnyPointer = import \"leaf.capnp\".item; } const p :S = ();",
        "const p :AnyPointer = import \"leaf.capnp\".empty;",
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
    for names in [["main.capnp", "leaf.capnp"], ["leaf.capnp", "main.capnp"]] {
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
        "struct Item {} const p :Item = import \"leaf.capnp\".item;",
        "struct Item {} const p :List(Item) = import \"leaf.capnp\".items;",
        "using L = import \"leaf.capnp\"; const p :L.Item = (absent = 1);",
        "using L = import \"leaf.capnp\"; const p :L.Item = (id = 65536);",
        "const p :AnyPointer = import \"mid.capnp\".broken;",
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
    fs::write(root().join("target/verification/schema-compiler/composite-imports.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted composite import graphs match all known request fields and canonical constant/default bytes\n{} shared rejections\n", cases.len() + 2, invalid.len())).unwrap();
}

#[test]
fn composite_constants_and_defaults_generate_bindings_and_round_trip() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let request = capnp_compiler::compile(
        "composites.capnp",
        include_str!("../crates/capnp-compiler/examples/composites.capnp"),
    )
    .unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-composite-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n", root().join("vendor/capnp"))).unwrap();
    fs::write(project.path().join("src/lib.rs"), r#"
#![deny(unreachable_pub)]
pub mod composites_capnp;
#[test]
fn round_trip() {
    use composites_capnp::{self as c, defaults, item, State};
    let empty = c::EMPTY.get().unwrap();
    assert_eq!(empty.get_count(), 42);
    assert_eq!(empty.get_name().unwrap(), "default");
    assert_eq!(empty.get_state().unwrap(), State::Ready);
    assert_eq!(empty.get_payloads().unwrap().get(0).unwrap(), [0, 255]);
    assert!(!empty.has_payloads());
    let item = c::ITEM.get().unwrap();
    assert_eq!(item.get_count(), 7);
    assert_eq!(item.get_name().unwrap(), "hé🦀");
    assert_eq!(item.get_state().unwrap(), State::Idle);
    assert!(!item.get_metadata().get_enabled());
    match item.get_metadata().which().unwrap() {
        item::metadata::Which::Label(label) => assert_eq!(label.unwrap(), "selected"),
        _ => panic!(),
    }
    assert_eq!(item.get_children().unwrap().get(0).get_count(), 42);
    assert_eq!(item.get_children().unwrap().get(1).get_count(), 9);
    assert_eq!(c::ITEMS.get().unwrap().get(1).get_count(), 7);
    assert_eq!(c::ROWS.get().unwrap().get(0).unwrap().get(0), -1);
    assert_eq!(c::ROWS.get().unwrap().get(1).unwrap().len(), 0);
    assert_eq!(c::ERASED.get().unwrap().get_as::<item::Reader>().unwrap().get_count(), 7);
    let mut wire = capnp::message::Builder::new_default();
    {
        let mut root = wire.init_root::<defaults::Builder>();
        assert!(!root.has_erased());
        assert_eq!(root.reborrow_as_reader().get_erased().get_as::<item::Reader>().unwrap().get_count(), 7);
        assert!(!root.has_erased());
        assert!(!root.has_item());
        assert_eq!(root.reborrow().get_item().unwrap().get_count(), 7);
        // Access through a builder materializes the default; mutation must not
        // alter either the constant or defaults read by a different message.
        root.reborrow().get_item().unwrap().set_count(100);
        root.reborrow().get_item().unwrap().get_metadata().set_absent(());
        root.reborrow().get_items().unwrap().get(0).set_name("changed");
        root.reborrow().get_rows().unwrap().get(0).unwrap().set(1, -9);
        let mut erased = root.reborrow().get_erased().get_as::<item::Builder>().unwrap();
        assert_eq!(erased.reborrow().get_count(), 7);
        erased.set_count(81);
        let rows = root.reborrow().get_erased_rows().get_as::<capnp::list_list::Builder<capnp::primitive_list::Owned<i16>>>().unwrap();
        assert_eq!(rows.into_reader().get(0).unwrap().get(0), -1);
    }
    let bytes = capnp::serialize::write_message_to_words(&wire);
    let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let root = decoded.get_root::<defaults::Reader>().unwrap();
    assert_eq!(root.get_item().unwrap().get_count(), 100);
    assert!(matches!(root.get_item().unwrap().get_metadata().which().unwrap(), item::metadata::Which::Absent(())));
    assert_eq!(root.get_items().unwrap().get(0).get_name().unwrap(), "changed");
    assert_eq!(root.get_items().unwrap().get(1).get_name().unwrap(), "hé🦀");
    assert_eq!(root.get_rows().unwrap().get(0).unwrap().get(1), -9);
    assert_eq!(root.get_erased().get_as::<item::Reader>().unwrap().get_count(), 81);
    assert_eq!(c::ITEM.get().unwrap().get_count(), 7);
    let mut other = capnp::message::Builder::new_default();
    let mut other = other.init_root::<defaults::Builder>();
    assert_eq!(other.reborrow_as_reader().get_item().unwrap().get_count(), 7);
    other.reborrow().get_erased().get_as::<item::Builder>().unwrap().set_count(91);
    assert_eq!(other.reborrow_as_reader().get_erased().get_as::<item::Reader>().unwrap().get_count(), 91);
    other.reborrow().get_erased().clear();
    assert!(!other.has_erased());
    assert_eq!(other.reborrow_as_reader().get_erased().get_as::<item::Reader>().unwrap().get_count(), 7);
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
        &root().join("target/verification/schema-compiler/composites-rust.log"),
        0,
    )
    .unwrap();
}

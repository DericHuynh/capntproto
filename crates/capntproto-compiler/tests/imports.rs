use capnp::schema_capnp::{code_generator_request, field, node, type_};
use capntproto_compiler::{FileCompiler, SchemaParser};
use std::fs;

const MAIN: &str = include_str!("../examples/imports/main.capnp");
const COMMON: &str = include_str!("../examples/imports/common.capnp");

fn root(
    message: &capnp::message::Builder<capnp::message::HeapAllocator>,
) -> code_generator_request::Reader<'_> {
    message.get_root_as_reader().unwrap()
}

fn find<'a>(request: code_generator_request::Reader<'a>, name: &str) -> node::Reader<'a> {
    request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == name)
        .unwrap()
}

fn slot<'a>(node: node::Reader<'a>, index: u32) -> field::slot::Reader<'a> {
    let node::Struct(s) = node.which().unwrap() else {
        panic!()
    };
    let field::Slot(s) = s.get_fields().unwrap().get(index).which().unwrap() else {
        panic!()
    };
    s
}

#[test]
fn cyclic_imports_and_cross_file_aliases_preserve_identity() {
    let mut parser = SchemaParser::new();
    parser
        .add_source("api/main.capnp", MAIN)
        .unwrap()
        .add_source("api/common.capnp", COMMON)
        .unwrap();
    let message = parser.parse(&["api/main.capnp"]).unwrap();
    let request = root(&message);
    capnp::schema_loader::SchemaLoader::default()
        .load_request(request)
        .unwrap();
    assert_eq!(request.get_nodes().unwrap().len(), 5);
    assert_eq!(request.get_requested_files().unwrap().len(), 1);
    let imports = request
        .get_requested_files()
        .unwrap()
        .get(0)
        .get_imports()
        .unwrap();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports.get(0).get_name().unwrap(), "common.capnp");
    assert_eq!(imports.get(0).get_id(), 0xafedbc9876543210);
    let record = find(request, "api/common.capnp:Record");
    let main = find(request, "api/main.capnp:Message");
    let type_::Struct(target) = slot(main, 0).get_type().unwrap().which().unwrap() else {
        panic!()
    };
    assert_eq!(target.get_type_id(), record.get_id());
    let type_::Struct(target) = slot(record, 2).get_type().unwrap().which().unwrap() else {
        panic!()
    };
    assert_eq!(target.get_type_id(), main.get_id());
    let all = parser
        .parse(&["api/main.capnp", "api/common.capnp", "api/./main.capnp"])
        .unwrap();
    assert_eq!(root(&all).get_requested_files().unwrap().len(), 2);
    assert_eq!(
        capnp::serialize::write_message_to_words(&message),
        capnp::serialize::write_message_to_words(&parser.parse(&["api/main.capnp"]).unwrap())
    );
}

#[test]
fn relative_absolute_direct_and_reexported_imports_use_lexical_scope() {
    let mut parser = SchemaParser::new();
    parser
        .import_path("first")
        .unwrap()
        .import_path("second")
        .unwrap()
        .import_path("shared")
        .unwrap();
    parser
        .add_source(
            "api/main.capnp",
            r#"
        using Local = import "../shared/types.capnp";
        using import "/public.capnp".Renamed;
        using Number = UInt32;
        using Sequence = List;
        using Numbers = Sequence(Number);
        @0xabcdefabcdefabcd;
        struct S {
          using Number = UInt16;
          a @0 :Number;
          b @1 : .Number;
          c @2 :Renamed;
          d @3 :import "../shared/./types.capnp".E;
          e @4 :Numbers;
        }
    "#,
        )
        .unwrap();
    parser
        .add_source(
            "first/public.capnp",
            "@0xaaaaaaaabbbbbbbb; using Renamed = import \"/types.capnp\".E;",
        )
        .unwrap();
    parser
        .add_source("second/public.capnp", "invalid shadowed source")
        .unwrap();
    parser
        .add_source(
            "shared/types.capnp",
            "@0xbbbbbbbbcccccccc; enum E { zero @0; one @1; } struct Unused {}",
        )
        .unwrap();
    let message = parser.parse(&["api/main.capnp"]).unwrap();
    let request = root(&message);
    let s = find(request, "api/main.capnp:S");
    assert!(matches!(
        slot(s, 0).get_type().unwrap().which().unwrap(),
        type_::Uint16(())
    ));
    assert!(matches!(
        slot(s, 1).get_type().unwrap().which().unwrap(),
        type_::Uint32(())
    ));
    let type_::Enum(a) = slot(s, 2).get_type().unwrap().which().unwrap() else {
        panic!()
    };
    let type_::Enum(b) = slot(s, 3).get_type().unwrap().which().unwrap() else {
        panic!()
    };
    assert_eq!(a.get_type_id(), b.get_type_id());
    assert!(request.get_nodes().unwrap().iter().all(|n| !n
        .get_display_name()
        .unwrap()
        .to_str()
        .unwrap()
        .ends_with("Unused")));
    let imports = request
        .get_requested_files()
        .unwrap()
        .get(0)
        .get_imports()
        .unwrap();
    assert_eq!(imports.len(), 3); // Distinct source spellings are preserved.
    assert_eq!(imports.get(0).get_id(), imports.get(1).get_id());
    assert_eq!(request.get_nodes().unwrap().len(), 4); // The reexport-only file is not a runtime dependency.
}

#[test]
fn alias_cycles_duplicates_bad_targets_and_import_errors_are_diagnostics() {
    for (source, expected) in [
        ("using A = B; using B = A;", "cyclic alias"),
        ("using A = List(A);", "cyclic alias"),
        ("using S = UInt32; struct S {}", "duplicate declaration"),
        ("struct S {} using S = UInt32;", "duplicate declaration"),
        (
            "using S = UInt32; using S = UInt16;",
            "duplicate declaration",
        ),
        ("using UInt32;", "another scope"),
        ("using A = Missing;", "unknown type"),
        ("using A = List();", "exactly one"),
        ("using A = List(UInt32, UInt64);", "exactly one"),
        ("using A = Text.Member;", "no nested"),
        ("struct S { x @0 :List; }", "exactly one"),
        ("struct S { x @0 :import \"dep.capnp\"; }", "file is not"),
        ("using A = import \"missing.capnp\";", "not found"),
        ("using A = import \"/dep.capnp\".Outside;", "not found"),
        ("using A = import \"../../outside.capnp\";", "escapes"),
        ("using A = import \"\";", "invalid import"),
        ("using A = import \"C:/dep.capnp\";", "invalid schema path"),
        ("@0xeeeeeeeeeeeeeeee;", "duplicate file ID"),
    ] {
        let mut parser = SchemaParser::new();
        parser
            .add_source("main.capnp", format!("@0xabcdefabcdefabcd;\n{source}"))
            .unwrap();
        parser
            .add_source("dep.capnp", "@0xbbbbbbbbbbbbbbbb; struct S {}")
            .unwrap();
        let error = parser.parse(&["main.capnp"]).err().expect(source);
        assert!(error.message.contains(expected), "{source}: {error}");
        assert_eq!(error.filename, "main.capnp");
        assert_eq!(error.line, 2);
    }
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            "@0xabcdefabcdefabcd; using Dep = import \"dep.capnp\";",
        )
        .unwrap();
    parser
        .add_source("dep.capnp", "@0xbbbbbbbbbbbbbbbb;\nstruct S { broken @0 :")
        .unwrap();
    let error = parser.parse(&["main.capnp"]).err().unwrap();
    assert_eq!((error.filename.as_str(), error.line), ("dep.capnp", 2));
    assert!(parser.parse(&[]).is_err());
    assert!(parser.add_source("./main.capnp", "different").is_err());
}

#[test]
fn graph_ids_are_unique_and_failed_compilation_does_not_poison_the_parser() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "bad.capnp",
            "@0xabcdefabcdefabcd; using Dep = import \"dep.capnp\";",
        )
        .unwrap();
    parser
        .add_source("dep.capnp", "@0xabcdefabcdefabcd;")
        .unwrap();
    assert!(parser
        .parse(&["bad.capnp"])
        .err()
        .unwrap()
        .message
        .contains("duplicate schema ID"));
    parser
        .add_source("good.capnp", "@0xcccccccccccccccc; struct S {}")
        .unwrap();
    assert!(parser.parse(&["good.capnp"]).is_ok());
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "a.capnp",
            "@0xaaaaaaaaaaaaaaaa; struct S @0xcccccccccccccccc {}",
        )
        .unwrap();
    parser
        .add_source(
            "b.capnp",
            "@0xbbbbbbbbbbbbbbbb; struct T @0xcccccccccccccccc {}",
        )
        .unwrap();
    assert!(parser
        .parse(&["a.capnp", "b.capnp"])
        .err()
        .unwrap()
        .message
        .contains("duplicate schema ID"));
}

#[test]
fn imported_alias_cycles_and_root_escapes_fail_at_the_importing_source() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            "@0xaaaaaaaaaaaaaaaa; using A = import \"dep.capnp\".B;",
        )
        .unwrap();
    parser
        .add_source(
            "dep.capnp",
            "@0xbbbbbbbbbbbbbbbb; using B = import \"main.capnp\".A;",
        )
        .unwrap();
    assert!(parser
        .parse(&["main.capnp"])
        .err()
        .unwrap()
        .message
        .contains("cyclic alias"));
    let mut parser = SchemaParser::new();
    parser.import_path("include").unwrap();
    parser
        .add_source(
            "main.capnp",
            "@0xaaaaaaaaaaaaaaaa; using A = import \"/dep.capnp\".Outside;",
        )
        .unwrap();
    assert!(parser.parse(&["main.capnp"]).is_err());
    parser
        .add_source(
            "include/dep.capnp",
            "@0xbbbbbbbbbbbbbbbb; using Outside = import \"../outside.capnp\";",
        )
        .unwrap();
    parser
        .add_source("outside.capnp", "@0xcccccccccccccccc;")
        .unwrap();
    let error = parser.parse(&["main.capnp"]).err().unwrap();
    assert_eq!(error.filename, "dep.capnp");
    assert!(error.message.contains("escapes"));

    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("include")).unwrap();
    fs::write(
        directory.path().join("main.capnp"),
        "@0xaaaaaaaaaaaaaaaa; using Dep = import \"/dep.capnp\".Outside;",
    )
    .unwrap();
    fs::write(
        directory.path().join("include/dep.capnp"),
        "@0xbbbbbbbbbbbbbbbb; using Outside = import \"../outside.capnp\";",
    )
    .unwrap();
    fs::write(
        directory.path().join("outside.capnp"),
        "@0xcccccccccccccccc;",
    )
    .unwrap();
    let error = FileCompiler::new()
        .src_prefix(directory.path())
        .import_path(directory.path().join("include"))
        .compile(&[directory.path().join("main.capnp")])
        .err()
        .unwrap();
    assert!(error.message.contains("escapes"));
}

#[test]
fn missing_import_can_be_added_and_aggregate_source_size_is_bounded() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            "@0xaaaaaaaaaaaaaaaa; using Dep = import \"dep.capnp\"; struct S { t @0 :Dep.T; }",
        )
        .unwrap();
    assert!(parser.parse(&["main.capnp"]).is_err());
    parser
        .add_source("dep.capnp", "@0xbbbbbbbbbbbbbbbb; struct T {}")
        .unwrap();
    assert!(parser.parse(&["main.capnp"]).is_ok());
    let mut parser = SchemaParser::new();
    for i in 0..5 {
        let mut source = format!("@0x{:016x}; #", (1u64 << 63) + i);
        source.push_str(&" ".repeat(4 * 1024 * 1024 - source.len()));
        parser.add_source(&format!("{i}.capnp"), source).unwrap();
    }
    assert!(parser
        .parse(&["0.capnp", "1.capnp", "2.capnp", "3.capnp", "4.capnp"])
        .err()
        .unwrap()
        .message
        .contains("total imported source/embed limit"));
}

#[test]
fn unused_transitive_imports_are_lazy_and_become_required_when_referenced() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            "@0xaaaaaaaaaaaaaaaa; using Dep = import \"dep.capnp\"; struct S { x @0 :Dep.T; }",
        )
        .unwrap();
    parser
        .add_source(
            "dep.capnp",
            "@0xbbbbbbbbbbbbbbbb; using Unused = import \"missing.capnp\"; struct T {}",
        )
        .unwrap();
    assert!(parser.parse(&["main.capnp"]).is_ok());
    assert!(parser
        .parse(&["main.capnp", "dep.capnp"])
        .err()
        .unwrap()
        .message
        .contains("not found"));
    parser
        .add_source(
            "uses.capnp",
            "@0xcccccccccccccccc; using Needed = import \"dep.capnp\".Unused;",
        )
        .unwrap();
    let error = parser.parse(&["uses.capnp"]).err().unwrap();
    assert_eq!(error.filename, "dep.capnp");
    assert!(error.message.contains("missing.capnp"));
}

#[test]
fn import_graph_and_alias_expansion_are_bounded() {
    let mut parser = SchemaParser::new();
    for i in 0..257 {
        let id = (1u64 << 63) + i;
        let import = if i == 256 {
            String::from("struct S {}")
        } else {
            format!("struct S {{ next @0 :import \"{}.capnp\".S; }}", i + 1)
        };
        parser
            .add_source(&format!("{i}.capnp"), format!("@0x{id:016x}; {import}"))
            .unwrap();
    }
    assert!(parser
        .parse(&["0.capnp"])
        .err()
        .unwrap()
        .message
        .contains("file limit"));
    let mut source = String::from("@0xabcdefabcdefabcd; using A0 = UInt32;");
    for i in 1..70 {
        source.push_str(&format!("using A{i} = List(A{});", i - 1));
    }
    assert!(capntproto_compiler::compile("deep.capnp", &source)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
}

#[test]
fn filesystem_and_cli_resolve_imports_without_cpp() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("main.capnp"), MAIN).unwrap();
    fs::write(directory.path().join("common.capnp"), COMMON).unwrap();
    let mut compiler = FileCompiler::new();
    compiler.src_prefix(directory.path());
    let paths = [
        directory.path().join("main.capnp"),
        directory.path().join("common.capnp"),
    ];
    let expected = compiler.compile(&paths).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_capntproto-compile"))
        .arg("--src-prefix")
        .arg(directory.path())
        .args(&paths)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        capnp::serialize::write_message_to_words(&expected)
    );
    fs::create_dir(directory.path().join("include")).unwrap();
    fs::write(
        directory.path().join("include/types.capnp"),
        "@0xbbbbbbbbbbbbbbbb; struct T {}",
    )
    .unwrap();
    fs::write(
        directory.path().join("absolute.capnp"),
        "@0xaaaaaaaaaaaaaaaa; struct S { t @0 :import \"/types.capnp\".T; }",
    )
    .unwrap();
    assert!(compiler
        .compile(&[directory.path().join("absolute.capnp")])
        .is_err());
    compiler.import_path(directory.path().join("include"));
    assert!(compiler
        .compile(&[directory.path().join("absolute.capnp")])
        .is_ok());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_capntproto-compile"))
        .current_dir(directory.path())
        .args(["-Iinclude", "--src-prefix=.", "absolute.capnp"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_capntproto-compile"))
        .arg("--src-prefix")
        .arg(directory.path())
        .arg(directory.path().join("missing.capnp"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_aliases_share_file_identity() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("dep.capnp"),
        "@0xbbbbbbbbbbbbbbbb; struct T {}",
    )
    .unwrap();
    std::os::unix::fs::symlink("dep.capnp", directory.path().join("alias.capnp")).unwrap();
    fs::write(directory.path().join("main.capnp"), "@0xaaaaaaaaaaaaaaaa; struct S { a @0 :import \"dep.capnp\".T; b @1 :import \"alias.capnp\".T; }").unwrap();
    let message = FileCompiler::new()
        .src_prefix(directory.path())
        .compile(&[directory.path().join("main.capnp")])
        .unwrap();
    let request = root(&message);
    assert_eq!(request.get_nodes().unwrap().len(), 4);
    let imports = request
        .get_requested_files()
        .unwrap()
        .get(0)
        .get_imports()
        .unwrap();
    assert_eq!(imports.get(0).get_id(), imports.get(1).get_id());
}

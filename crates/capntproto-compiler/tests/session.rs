use capnp::{
    message,
    schema_loader::{
        dynamic::{self, Value},
        Limits, Type,
    },
};
use capntproto_compiler::{FileCompiler, ParseError, SchemaParser, SchemaSession};
use std::fs;

const MAIN_ID: u64 = 0xaaaaaaaaaaaaaaaa;
const IMPORT_ID: u64 = 0xbbbbbbbbbbbbbbbb;
const MAIN: &str = "@0xaaaaaaaaaaaaaaaa; using I = import \"types.capnp\"; using Alias = Root; struct Root { item @0 :I.Used; }";
const TYPES: &str = r#"@0xbbbbbbbbbbbbbbbb;
struct Used {}
struct Good { # Loaded later.
  using Alias = Child;
  value @0 :Text = "late";
  struct Child { value @0 :UInt32 = 31; }
}
struct Broken { value @0 :Missing; }
using UnusedAlias = import "missing.capnp".Missing;
const limit :UInt32 = 42;
"#;

fn parser() -> SchemaParser {
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", MAIN).unwrap();
    parser.add_source("types.capnp", TYPES).unwrap();
    parser
}

fn bytes(session: &SchemaSession<'_>) -> Vec<u8> {
    let mut copy = message::Builder::new_default();
    copy.set_root(session.schemas().request()).unwrap();
    capnp::serialize::write_message_to_words(&copy)
}

#[test]
fn lazy_direct_lookup_compiles_only_the_selected_declaration_and_dependencies() {
    let parser = parser();
    let mut session = parser.parse_session(&["main.capnp"]).unwrap();
    let initial = bytes(&session);
    assert_eq!(session.schemas().loader().len(), 4);
    assert!(session
        .schemas()
        .get(IMPORT_ID)
        .unwrap()
        .find_nested("Good")
        .is_err());
    for missing in ["Missing", "Root.Child", "", "root", "Root\0"] {
        assert!(session.find_nested(MAIN_ID, missing).unwrap().is_none());
    }
    assert!(session.find_nested(IMPORT_ID, "UnusedAlias").is_err());
    let root_id = session
        .schemas()
        .get_file("main.capnp")
        .unwrap()
        .get_nested("Root")
        .unwrap()
        .schema()
        .id();
    assert_eq!(
        session.get_nested(MAIN_ID, "Alias").unwrap().schema().id(),
        root_id
    );
    assert_eq!(bytes(&session), initial);
    let good = session.get_nested(IMPORT_ID, "Good").unwrap();
    let good_id = good.schema().id();
    assert_eq!(
        good.source_info().unwrap().get_doc_comment().unwrap(),
        "Loaded later.\n"
    );
    assert!(good.find_nested("Child").is_err());
    assert_eq!(session.schemas().loader().len(), 5);
    let cached = bytes(&session);
    assert_eq!(session.load(good_id).unwrap().schema().id(), good_id);
    assert_eq!(bytes(&session), cached);
    let child_id = session.get_nested(good_id, "Alias").unwrap().schema().id();
    let children = session.get_all_nested(good_id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].schema().id(), child_id);
    let mut value = message::Builder::new_default();
    let value = dynamic::Builder::init(value.init_root(), children[0].schema()).unwrap();
    assert!(matches!(
        value.as_reader().get_named("value").unwrap(),
        Value::UInt32(31)
    ));
    let constant = session.get_nested(IMPORT_ID, "limit").unwrap();
    assert!(matches!(
        constant.schema().get_proto().which().unwrap(),
        capnp::schema_capnp::node::Const(_)
    ));
    assert_eq!(
        session
            .schemas()
            .requested_files()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["main.capnp"]
    );
    assert!(session
        .schemas()
        .get(IMPORT_ID)
        .unwrap()
        .find_nested("Broken")
        .is_err());
    let snapshot = session.into_schemas();
    drop(parser);
    assert!(snapshot.get(good_id).unwrap().get_nested("Child").is_ok());
}

#[test]
fn failed_single_and_batch_extensions_preserve_the_entire_snapshot() {
    let parser = parser();
    let mut session = parser.parse_session(&["main.capnp"]).unwrap();
    let before = bytes(&session);
    for _ in 0..2 {
        assert!(matches!(
            session.get_nested(IMPORT_ID, "Broken"),
            Err(ParseError::Compile(_))
        ));
        assert!(matches!(
            session.get_all_nested(IMPORT_ID),
            Err(ParseError::Compile(_))
        ));
        assert!(session.load(0).is_err());
        assert!(session.find_nested(0, "Unknown").is_err());
        assert!(session.get_nested(IMPORT_ID, "Missing").is_err());
        assert_eq!(bytes(&session), before);
        assert_eq!(session.schemas().loader().len(), 4);
    }
    assert!(session.get_nested(IMPORT_ID, "Good").is_ok());
}

#[test]
fn loader_limits_are_cumulative_and_rejection_does_not_commit_compiled_nodes() {
    let parser = parser();
    let mut session = parser
        .parse_session_with_limits(
            &["main.capnp"],
            Limits {
                nodes: 5,
                ..Limits::default()
            },
        )
        .unwrap();
    let good_id = session.get_nested(IMPORT_ID, "Good").unwrap().schema().id();
    let before = bytes(&session);
    assert!(matches!(
        session.get_nested(good_id, "Child"),
        Err(ParseError::Load(_))
    ));
    assert_eq!(bytes(&session), before);
    assert_eq!(session.schemas().loader().len(), 5);
    assert!(session.load(good_id).is_ok());
    assert!(session
        .schemas()
        .get(good_id)
        .unwrap()
        .find_nested("Child")
        .is_err());
}

#[test]
fn lazy_imports_preserve_brands_methods_and_implicit_source_info() {
    let mut parser = parser();
    parser
        .add_source(
            "generic.capnp",
            r#"@0xcccccccccccccccc;
      using I = import "types.capnp";
      struct Entry { used @0 :I.Used; }
      struct Box(T) { value @0 :T; }
      interface Service(T) { call @0 (box :Box(T)) -> (value :T); }
      struct Later { box @0 :Box(Text); service @1 :Service(Text); }
    "#,
        )
        .unwrap();
    parser
        .add_source(
            "root.capnp",
            "@0xdddddddddddddddd; using G = import \"generic.capnp\"; struct R { x @0 :G.Entry; }",
        )
        .unwrap();
    let mut session = parser.parse_session(&["root.capnp"]).unwrap();
    let later = session
        .get_nested(0xcccccccccccccccc, "Later")
        .unwrap()
        .schema();
    let Type::Struct(boxed) = later.field("box").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert!(matches!(
        boxed.field("value").unwrap().get_type().unwrap(),
        Type::Text
    ));
    let Type::Interface(service) = later.field("service").unwrap().get_type().unwrap() else {
        panic!()
    };
    let method = service.method("call").unwrap();
    assert!(matches!(
        method
            .results()
            .unwrap()
            .field("value")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Text
    ));
    let implicit = [
        method.params().unwrap().id(),
        method.results().unwrap().id(),
    ];
    for id in implicit {
        assert!(session.schemas().source_info(id).is_some());
    }
    assert!(session
        .schemas()
        .get(IMPORT_ID)
        .unwrap()
        .find_nested("Broken")
        .is_err());
}

#[test]
fn disk_discovery_and_embeds_roll_back_and_retry_after_source_errors() {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.capnp");
    let types = directory.path().join("types.capnp");
    let late = directory.path().join("late.capnp");
    let blob = directory.path().join("blob.txt");
    fs::write(&main, MAIN).unwrap();
    fs::write(&types, "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :import \"late.capnp\".Item; label @1 :Text = embed \"blob.txt\"; }").unwrap();
    let mut compiler = FileCompiler::new();
    compiler.src_prefix(directory.path());
    let mut session = compiler.parse_session(&[&main]).unwrap();
    drop(compiler);
    let before = bytes(&session);
    let dependencies = session.schemas().dependencies().to_vec();
    assert!(session.get_nested(IMPORT_ID, "Later").is_err());
    // The failed extension has already parsed the new import before finding
    // this missing type. A retry must not retain that provisional source.
    fs::write(&late, "@0xcccccccccccccccc; struct Item { x @0 :Missing; }").unwrap();
    fs::write(&blob, "first").unwrap();
    assert!(session.get_nested(IMPORT_ID, "Later").is_err());
    assert_eq!(bytes(&session), before);
    assert_eq!(session.schemas().dependencies(), dependencies);
    fs::write(
        &late,
        "@0xdddddddddddddddd; struct Item { x @0 :UInt32 = 9; }",
    )
    .unwrap();
    fs::write(&blob, "second").unwrap();
    // Previously read sources are cached; successful lookup must survive these
    // deletions and read only the newly needed import/embed.
    fs::remove_file(&main).unwrap();
    fs::remove_file(&types).unwrap();
    let later = session.get_nested(IMPORT_ID, "Later").unwrap().schema();
    let mut message = message::Builder::new_default();
    let value = dynamic::Builder::init(message.init_root(), later).unwrap();
    assert!(
        matches!(value.as_reader().get_named("label").unwrap(), Value::Text(t) if t == "second")
    );
    assert!(session.schemas().get(0xcccccccccccccccc).is_err());
    assert!(session.schemas().get(0xdddddddddddddddd).is_ok());
    assert!(session
        .schemas()
        .dependencies()
        .contains(&late.canonicalize().unwrap()));
    assert!(session
        .schemas()
        .dependencies()
        .contains(&blob.canonicalize().unwrap()));
}

#[test]
fn successful_disk_inputs_stay_cached_and_loader_failures_discard_new_inputs() {
    let directory = tempfile::tempdir().unwrap();
    // Count physical inputs independently of temporary-directory aliases.
    let directory_path = directory.path().canonicalize().unwrap();
    let main = directory_path.join("main.capnp");
    let types = directory_path.join("types.capnp");
    let late = directory_path.join("late.capnp");
    fs::write(&main, MAIN).unwrap();
    fs::write(&types, "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :import \"late.capnp\".Item; } struct Last { value @0 :import \"late.capnp\".Item; }").unwrap();
    let mut compiler = FileCompiler::new();
    compiler.src_prefix(&directory_path);
    let mut session = compiler
        .parse_session_with_limits(
            &[&main],
            Limits {
                nodes: 8,
                ..Limits::default()
            },
        )
        .unwrap();
    // Both interfaces can be emitted, but the runtime rejects cyclic inheritance.
    fs::write(
        &late,
        "@0xcccccccccccccccc; interface Item extends(B) {} interface B extends(Item) {}",
    )
    .unwrap();
    let before = bytes(&session);
    assert!(matches!(
        session.get_nested(IMPORT_ID, "Later"),
        Err(ParseError::Load(_))
    ));
    assert_eq!(bytes(&session), before);
    assert_eq!(session.schemas().dependencies().len(), 2);
    fs::write(
        &late,
        "@0xdddddddddddddddd; struct Item { x @0 :UInt32 = 9; }",
    )
    .unwrap();
    session.get_nested(IMPORT_ID, "Later").unwrap();
    fs::write(&late, "invalid changed source").unwrap();
    session.get_nested(IMPORT_ID, "Last").unwrap();
    assert!(session.schemas().get(0xdddddddddddddddd).is_ok());
    assert_eq!(session.schemas().loader().len(), 8);
}

#[test]
fn lazy_aliases_resolve_imports_and_declarations_while_erasing_generic_bindings() {
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", MAIN).unwrap();
    parser
        .add_source(
            "types.capnp",
            r#"@0xbbbbbbbbbbbbbbbb;
      struct Used {}
      struct Box(T) { value @0 :T; using Parameter = T; }
      using TextBox = Box(Text);
      using Numbers = List(UInt32);
      using Number = UInt32;
      using Cycle = Other;
      using Other = Cycle;
      using Remote = import "remote.capnp";
      using Alias = Remote.Item;
      using limit = Remote.limit;
      using note = Remote.note;
      using Enum = Remote.E;
      using Interface = Remote.Service;
    "#,
        )
        .unwrap();
    parser.add_source("remote.capnp", "@0xcccccccccccccccc; struct Item {} const limit :UInt32 = 7; annotation note(*) :Text; enum E { a @0; } interface Service {}").unwrap();
    let mut session = parser.parse_session(&["main.capnp"]).unwrap();
    assert!(session.schemas().get(0xcccccccccccccccc).is_err());
    let remote_id = session
        .get_nested(IMPORT_ID, "Remote")
        .unwrap()
        .schema()
        .id();
    assert_eq!(remote_id, 0xcccccccccccccccc);
    for (alias, name) in [
        ("Alias", "Item"),
        ("limit", "limit"),
        ("note", "note"),
        ("Enum", "E"),
        ("Interface", "Service"),
    ] {
        let id = session.get_nested(IMPORT_ID, alias).unwrap().schema().id();
        assert_eq!(
            session.get_nested(remote_id, name).unwrap().schema().id(),
            id
        );
    }
    let boxed = session.get_nested(IMPORT_ID, "TextBox").unwrap().schema();
    let box_id = boxed.id();
    assert!(matches!(
        boxed.field("value").unwrap().get_type().unwrap(),
        Type::AnyPointer(_)
    ));
    let before = bytes(&session);
    assert!(session.find_nested(box_id, "Parameter").unwrap().is_none());
    for name in ["Numbers", "Number", "Cycle"] {
        assert!(matches!(
            session.get_nested(IMPORT_ID, name),
            Err(ParseError::Compile(_))
        ));
        assert_eq!(bytes(&session), before);
    }
    assert_eq!(session.schemas().requested_files().len(), 1);
}

#[test]
fn well_formed_numeric_type_errors_remain_lazy() {
    for expression in ["256", "1.5", "18446744073709551616", "0x10000000000000000"] {
        let mut parser = capntproto_compiler::SchemaParser::new();
        parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { x @0 :D.Used; }").unwrap();
        parser
            .add_source(
                "types.capnp",
                format!(
                    "@0xbbbbbbbbbbbbbbbb; struct Used {{}} const hidden :UInt8 = {expression};"
                ),
            )
            .unwrap();
        let mut session = parser.parse_session(&["main.capnp"]).unwrap();
        let before = bytes(&session);
        assert!(matches!(
            session.get_nested(0xbbbbbbbbbbbbbbbb, "hidden"),
            Err(capntproto_compiler::ParseError::Compile(_))
        ));
        assert_eq!(bytes(&session), before);
    }
}

#[test]
fn newly_discovered_numeric_syntax_errors_roll_back_and_allow_retry() {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.capnp");
    let types = directory.path().join("types.capnp");
    let late = directory.path().join("late.capnp");
    fs::write(&main, MAIN).unwrap();
    fs::write(&types, "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :import \"late.capnp\".Item; }").unwrap();
    // This file is not needed initially, so even lexical errors are deferred
    // until discovery. Once opened, its unused declarations must also lex.
    fs::write(
        &late,
        "@0xcccccccccccccccc; struct Item {} const hidden :Float64 = 1ee2;",
    )
    .unwrap();
    let mut compiler = FileCompiler::new();
    compiler.src_prefix(directory.path());
    let mut session = compiler.parse_session(&[&main]).unwrap();
    let before = bytes(&session);
    let dependencies = session.schemas().dependencies().to_vec();
    let Err(ParseError::Compile(error)) = session.get_nested(IMPORT_ID, "Later") else {
        panic!("unused malformed literal must reject the discovered source")
    };
    assert_eq!(error.filename, "late.capnp");
    assert_eq!(error.message, "invalid numeric literal");
    assert_eq!(bytes(&session), before);
    assert_eq!(session.schemas().dependencies(), dependencies);
    fs::write(
        &late,
        "@0xdddddddddddddddd; struct Item {} const hidden :Float64 = 1e2;",
    )
    .unwrap();
    session.get_nested(IMPORT_ID, "Later").unwrap();
    assert!(session.schemas().get(0xcccccccccccccccc).is_err());
    assert!(session.schemas().get(0xdddddddddddddddd).is_ok());
    assert!(session
        .schemas()
        .dependencies()
        .contains(&late.canonicalize().unwrap()));
}

use capnp::{
    message,
    schema_capnp::code_generator_request,
    schema_loader::{dynamic, Limits},
};
use capnp_compiler::{
    ParseError, SchemaSession, SourceCompiler, SourceFile, SourceProvider, MAX_SOURCE_BYTES,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{self, Read},
    rc::Rc,
};

const MAIN: u64 = 0xaaaaaaaaaaaaaaaa;
const TYPES: u64 = 0xbbbbbbbbbbbbbbbb;

#[derive(Default)]
struct Provider {
    routes: BTreeMap<(Option<String>, String), SourceFile>,
    bytes: RefCell<BTreeMap<String, Vec<u8>>>,
    resolves: RefCell<Vec<(Option<String>, String)>>,
    opens: RefCell<Vec<String>>,
}

impl Provider {
    fn file(&mut self, identity: &str, bytes: impl AsRef<[u8]>) {
        self.bytes
            .get_mut()
            .insert(identity.into(), bytes.as_ref().into());
    }
    fn route(&mut self, from: Option<&str>, path: &str, identity: &str, filename: &str) {
        self.routes.insert(
            (from.map(Into::into), path.into()),
            SourceFile {
                identity: identity.into(),
                filename: filename.into(),
            },
        );
    }
}

impl SourceProvider for Provider {
    fn resolve(&self, from: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
        let key = (from.map(Into::into), path.into());
        self.resolves.borrow_mut().push(key.clone());
        Ok(self.routes.get(&key).cloned())
    }
    fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
        self.opens.borrow_mut().push(identity.into());
        let bytes = self
            .bytes
            .borrow()
            .get(identity)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "provider input absent"))?;
        Ok(Box::new(io::Cursor::new(bytes)))
    }
}

fn fixture() -> Provider {
    let mut provider = Provider::default();
    provider.file(
        "repo:main",
        br#"@0xaaaaaaaaaaaaaaaa;
using A = import "pkg:types";
using B = import "/alias//../types";
struct Root { a @0 :A.Item; b @1 :B.Item; }
const blob :Data = embed "asset:blob";
const again :Data = embed "asset:alias";
"#,
    );
    provider.file(
        "repo:types",
        br#"@0xbbbbbbbbbbbbbbbb;
using M = import "root:alias";
struct Item { root @0 :M.Root; }
struct Later { dep @0 :import "late:schema".Dep; }
using Unused = import "never:open";
"#,
    );
    provider.file("repo:blob", [0, 255, 42]);
    provider.file(
        "repo:late",
        b"@0xcccccccccccccccc; struct Dep { value @0 :UInt32 = 19; }",
    );
    provider.route(None, "entry", "repo:main", "main.capnp");
    provider.route(None, "alias", "repo:main", "ignored-alias.capnp");
    provider.route(Some("repo:main"), "pkg:types", "repo:types", "types.capnp");
    provider.route(
        Some("repo:main"),
        "/alias//../types",
        "repo:types",
        "ignored.capnp",
    );
    provider.route(
        Some("repo:types"),
        "root:alias",
        "repo:main",
        "ignored-root.capnp",
    );
    provider.route(Some("repo:types"), "late:schema", "repo:late", "late.capnp");
    provider.route(Some("repo:main"), "asset:blob", "repo:blob", "blob.bin");
    provider.route(Some("repo:main"), "asset:alias", "repo:blob", "alias.bin");
    provider
}

fn snapshot(session: &SchemaSession<'_>) -> Vec<u8> {
    let mut copy = message::Builder::new_default();
    copy.set_root(session.schemas().request()).unwrap();
    capnp::serialize::write_message_to_words(&copy)
}

#[test]
fn opaque_paths_aliases_cycles_and_binary_embeds_use_only_the_provider() {
    let provider = fixture();
    let parsed = SourceCompiler::new(&provider)
        .parse_schemas(&["entry", "alias"])
        .unwrap();
    assert_eq!(
        parsed
            .requested_files()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["main.capnp"]
    );
    assert!(parsed.dependencies().is_empty());
    let request = parsed.request();
    let imports = request
        .get_requested_files()
        .unwrap()
        .get(0)
        .get_imports()
        .unwrap();
    assert_eq!(imports.len(), 2);
    assert_eq!(imports.get(0).get_name().unwrap(), "/alias//../types");
    assert_eq!(imports.get(1).get_name().unwrap(), "pkg:types");
    assert_eq!(imports.get(0).get_id(), TYPES);
    assert_eq!(imports.get(1).get_id(), TYPES);
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:main")
            .count(),
        1
    );
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:types")
            .count(),
        1
    );
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:blob")
            .count(),
        1
    );
    assert_eq!(provider.opens.borrow().len(), 3);
    assert!(provider
        .resolves
        .borrow()
        .iter()
        .any(|(from, path)| from.as_deref() == Some("repo:types") && path == "root:alias"));
    assert!(!provider
        .resolves
        .borrow()
        .iter()
        .any(|(_, path)| path == "never:open" || path == "late:schema"));
    for name in ["blob", "again"] {
        let constant = parsed.get(MAIN).unwrap().get_nested(name).unwrap();
        let capnp::schema_capnp::node::Const(constant) =
            constant.schema().get_proto().which().unwrap()
        else {
            panic!()
        };
        let capnp::schema_capnp::value::Data(bytes) =
            constant.get_value().unwrap().which().unwrap()
        else {
            panic!()
        };
        assert_eq!(bytes.unwrap(), [0, 255, 42]);
    }
    drop(provider);
    assert!(parsed.get(MAIN).unwrap().get_nested("Root").is_ok());
}

#[test]
fn lazy_failures_discard_discovered_inputs_and_successful_inputs_stay_cached() {
    let provider = fixture();
    let mut session = SourceCompiler::new(&provider)
        .parse_session(&["entry"])
        .unwrap();
    let original = snapshot(&session);
    let calls = provider.resolves.borrow().len();
    assert!(session.find_nested(TYPES, "Missing").unwrap().is_none());
    assert_eq!(provider.resolves.borrow().len(), calls);
    // Existing inputs are cached; a failed extension's newly discovered input is not.
    provider.bytes.borrow_mut().remove("repo:main");
    provider.bytes.borrow_mut().remove("repo:types");
    provider.bytes.borrow_mut().insert(
        "repo:late".into(),
        b"@0xcccccccccccccccc; struct Dep { value @0 :Missing; }".to_vec(),
    );
    assert!(matches!(
        session.get_nested(TYPES, "Later"),
        Err(ParseError::Compile(_))
    ));
    assert_eq!(snapshot(&session), original);
    provider.bytes.borrow_mut().insert(
        "repo:late".into(),
        b"@0xcccccccccccccccc; struct Dep { value @0 :UInt32 = 19; }".to_vec(),
    );
    assert!(session.get_nested(TYPES, "Later").is_ok());
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:late")
            .count(),
        2
    );
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:types")
            .count(),
        1
    );
    let parsed = session.into_schemas();
    drop(provider);
    let dep = parsed
        .get(0xcccccccccccccccc)
        .unwrap()
        .get_nested("Dep")
        .unwrap();
    let mut message = message::Builder::new_default();
    let value = dynamic::Builder::init(message.init_root(), dep.schema()).unwrap();
    assert!(matches!(
        value.as_reader().get_named("value").unwrap(),
        dynamic::Value::UInt32(19)
    ));
}

#[test]
fn runtime_validation_failure_preserves_provider_session_and_snapshot() {
    let provider = fixture();
    let compiler = SourceCompiler::new(&provider);
    let initial = compiler.parse_schemas(&["entry"]).unwrap().loader().len();
    let mut session = compiler
        .parse_session_with_limits(
            &["entry"],
            Limits {
                nodes: initial,
                ..Limits::default()
            },
        )
        .unwrap();
    let before = snapshot(&session);
    for _ in 0..2 {
        assert!(matches!(
            session.get_nested(TYPES, "Later"),
            Err(ParseError::Load(_))
        ));
        assert_eq!(snapshot(&session), before);
    }
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:late")
            .count(),
        2
    );
    assert!(matches!(
        compiler.parse_schemas_with_limits(
            &["entry"],
            Limits {
                nodes: 1,
                ..Limits::default()
            }
        ),
        Err(ParseError::Load(_))
    ));
}

#[test]
fn fresh_compilations_read_changes_and_do_not_cache_failed_inputs() {
    let provider = fixture();
    let compiler = SourceCompiler::new(&provider);
    compiler.compile(&["entry"]).unwrap();
    provider
        .bytes
        .borrow_mut()
        .insert("repo:main".into(), vec![255]);
    let error = compiler.compile(&["entry"]).err().unwrap();
    assert_eq!(error.filename, "main.capnp");
    assert!(error.message.contains("UTF-8"));
    provider.bytes.borrow_mut().insert(
        "repo:main".into(),
        b"@0xaaaaaaaaaaaaaaaa; struct Changed {}".to_vec(),
    );
    assert!(compiler
        .parse_schemas(&["entry"])
        .unwrap()
        .get(MAIN)
        .unwrap()
        .get_nested("Changed")
        .is_ok());
    assert_eq!(
        provider
            .opens
            .borrow()
            .iter()
            .filter(|id| *id == "repo:main")
            .count(),
        3
    );
}

#[test]
fn absent_files_do_not_fall_back_to_disk_and_errors_keep_expression_locations() {
    let mut provider = fixture();
    assert!(SourceCompiler::new(&provider)
        .compile(&["Cargo.toml"])
        .err()
        .unwrap()
        .message
        .contains("not found"));
    assert!(provider.opens.borrow().is_empty());
    provider
        .routes
        .remove(&(Some("repo:main".into()), "pkg:types".into()));
    let error = SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap();
    assert_eq!(error.filename, "main.capnp");
    assert_eq!(error.line, 2);
    assert!(error.end > error.start);
    assert!(error.message.contains("pkg:types"));
    let provider = fixture();
    provider.bytes.borrow_mut().remove("repo:blob");
    let error = SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap();
    assert_eq!(error.filename, "main.capnp");
    assert_eq!(error.line, 5);
    assert!(error.message.contains("provider input absent"));
}

struct Unlimited {
    read: Rc<Cell<usize>>,
    file: SourceFile,
    resolve_error: bool,
    read_error: bool,
}

impl SourceProvider for Unlimited {
    fn resolve(&self, _: Option<&str>, _: &str) -> io::Result<Option<SourceFile>> {
        if self.resolve_error {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "denied by provider",
            ));
        }
        Ok(Some(self.file.clone()))
    }
    fn open(&self, _: &str) -> io::Result<Box<dyn Read + '_>> {
        struct Stream<'a>(&'a Unlimited);
        impl Read for Stream<'_> {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                if self.0.read_error && self.0.read.get() != 0 {
                    return Err(io::Error::other("interrupted content"));
                }
                let len = output.len().min(7);
                output[..len].fill(b' ');
                self.0.read.set(self.0.read.get() + len);
                Ok(len)
            }
        }
        Ok(Box::new(Stream(self)))
    }
}

fn unlimited() -> Unlimited {
    Unlimited {
        read: Rc::new(Cell::new(0)),
        file: SourceFile {
            identity: "opaque:key".into(),
            filename: "input.capnp".into(),
        },
        resolve_error: false,
        read_error: false,
    }
}

#[test]
fn provider_reads_are_bounded_and_partial_read_errors_are_not_hidden() {
    let mut provider = unlimited();
    let error = SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap();
    assert!(error.message.contains("4 MiB"));
    assert_eq!(provider.read.get(), MAX_SOURCE_BYTES + 1);
    provider.read.set(0);
    provider.read_error = true;
    let error = SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap();
    assert!(error.message.contains("interrupted content"));
    assert_eq!(error.filename, "entry");
    assert_eq!(provider.read.get(), 7);
    provider.read.set(0);
    provider.resolve_error = true;
    assert!(SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap()
        .message
        .contains("denied by provider"));
    assert_eq!(provider.read.get(), 0);
}

#[test]
fn invalid_provider_metadata_is_rejected_before_opening() {
    for invalid in [
        String::new(),
        "bad\0name".into(),
        "bad\nname".into(),
        "x".repeat(4097),
    ] {
        for identity in [true, false] {
            let mut provider = unlimited();
            if identity {
                provider.file.identity = invalid.clone();
            } else {
                provider.file.filename = invalid.clone();
            }
            assert!(SourceCompiler::new(&provider)
                .compile(&["entry"])
                .err()
                .unwrap()
                .message
                .contains("invalid provider"));
            assert_eq!(provider.read.get(), 0);
        }
    }
}

#[test]
fn compile_request_entrypoint_and_empty_requests_follow_existing_contracts() {
    let provider = fixture();
    let request = SourceCompiler::new(&provider).compile(&["entry"]).unwrap();
    assert_eq!(
        request
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap()
            .get_requested_files()
            .unwrap()
            .len(),
        1
    );
    let error = SourceCompiler::new(&provider).compile(&[]).err().unwrap();
    assert!(error.message.contains("at least one requested file"));
}

#[test]
fn distinct_provider_identities_do_not_silently_merge_duplicate_schema_ids() {
    let mut provider = fixture();
    let main_bytes = provider.bytes.borrow()["repo:main"].clone();
    provider.file("other:main", main_bytes);
    provider.route(None, "other", "other:main", "other.capnp");
    let error = SourceCompiler::new(&provider)
        .compile(&["entry", "other"])
        .err()
        .unwrap();
    assert_eq!(error.filename, "other.capnp");
    assert!(error.message.contains("duplicate schema ID"));
    assert_eq!(*provider.opens.borrow(), ["repo:main", "other:main"]);
}

#[test]
fn custom_embeds_keep_per_file_and_combined_source_budgets() {
    let mut provider = fixture();
    provider.file("repo:blob", vec![0; MAX_SOURCE_BYTES + 1]);
    let error = SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap();
    assert!(error.message.contains("embedded file exceeds 4 MiB"));

    // Four maximum-sized embeds plus their source exceed the shared 16 MiB
    // input budget even though each individual input is within its own limit.
    let mut provider = Provider::default();
    let mut source = String::from("@0xaaaaaaaaaaaaaaaa;\n");
    for index in 0..4 {
        let key = format!("blob{index}");
        source.push_str(&format!("const data{index} :Data = embed \"{key}\";\n"));
        provider.file(&key, vec![0; MAX_SOURCE_BYTES]);
        provider.route(Some("main"), &key, &key, &key);
    }
    provider.file("main", source);
    provider.route(None, "entry", "main", "main.capnp");
    let error = SourceCompiler::new(&provider)
        .compile(&["entry"])
        .err()
        .unwrap();
    assert!(
        error
            .message
            .contains("total source/embed size limit exceeded"),
        "{error}"
    );
}

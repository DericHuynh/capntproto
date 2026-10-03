use capnp::schema_loader::{
    dynamic::{self, Value},
    Limits,
};
use capnp_compiler::{
    CachedSchema, CachedSchemas, ConcurrentSchemaParser, FileCompiler, ParseError, SchemaParser,
    SourceFile, SourceProvider,
};
use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read},
    sync::{Arc, Barrier, Mutex},
    thread,
};

const MAIN: u64 = 0xaaaaaaaaaaaaaaaa;
const TYPES: u64 = 0xbbbbbbbbbbbbbbbb;
const LATE: u64 = 0xcccccccccccccccc;

#[derive(Default)]
struct Inputs {
    files: BTreeMap<String, Vec<u8>>,
    opens: BTreeMap<String, usize>,
    callbacks: Vec<thread::ThreadId>,
}
struct Provider(Arc<Mutex<Inputs>>);
impl SourceProvider for Provider {
    fn resolve(&self, _: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
        let path = if path == "entry" { "main.capnp" } else { path };
        let mut state = self.0.lock().unwrap();
        state.callbacks.push(thread::current().id());
        Ok(state.files.contains_key(path).then(|| SourceFile {
            identity: path.into(),
            filename: path.into(),
        }))
    }
    fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
        let mut state = self.0.lock().unwrap();
        state.callbacks.push(thread::current().id());
        *state.opens.entry(identity.into()).or_default() += 1;
        let bytes = state.files[identity].clone();
        Ok(Box::new(Cursor::new(bytes)))
    }
}
fn inputs(files: &[(&str, &str)]) -> Arc<Mutex<Inputs>> {
    Arc::new(Mutex::new(Inputs {
        files: files
            .iter()
            .map(|(name, text)| ((*name).into(), text.as_bytes().to_vec()))
            .collect(),
        ..Default::default()
    }))
}

#[test]
fn concurrent_lookups_share_reads_compilation_and_immutable_snapshots() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<ConcurrentSchemaParser>();
    send_sync::<CachedSchemas>();
    send_sync::<CachedSchema>();
    let state = inputs(&[
        ("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }"),
        ("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { item @0 :import \"late.capnp\".Item; label @1 :Text = embed \"label.txt\"; } using Alias = Later;"),
        ("late.capnp", "@0xcccccccccccccccc; struct Item {}"),
        ("label.txt", "cached"),
    ]);
    let cache =
        ConcurrentSchemaParser::from_provider(Provider(state.clone()), &["main.capnp", "entry"])
            .unwrap();
    let original = cache.schemas().unwrap();
    assert_eq!(original.requested_files().len(), 1);
    assert_eq!(original.get_file("main.capnp").unwrap().id(), MAIN);
    let barrier = Arc::new(Barrier::new(32));
    let threads: Vec<_> = (0..32).map(|_| {
        let cache = cache.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            let handle = cache.get_nested(TYPES, "Later").unwrap();
            let local = handle.schemas().materialize().unwrap();
            let mut message = capnp::message::Builder::new_default();
            let value = dynamic::Builder::init(message.init_root(), local.get(handle.id()).unwrap().schema()).unwrap();
            assert!(matches!(value.as_reader().get_named("label").unwrap(), Value::Text(text) if text == "cached"));
            handle
        })
    }).collect();
    let handles: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    for handle in &handles {
        assert_eq!(handle.id(), handles[0].id());
        assert_eq!(handle.schemas().revision(), 1);
        assert!(std::ptr::eq(
            handle.schemas().serialized_request(),
            handles[0].schemas().serialized_request()
        ));
    }
    assert_eq!(original.revision(), 0);
    assert!(original
        .materialize()
        .unwrap()
        .get(handles[0].id())
        .is_err());
    let alias = cache.get_nested(TYPES, "Alias").unwrap();
    let alias_again = cache.get_nested(TYPES, "Alias").unwrap();
    assert_eq!(alias.id(), handles[0].id());
    assert_eq!(alias.schemas().revision(), alias_again.schemas().revision());
    assert!(std::ptr::eq(
        alias.schemas().serialized_request(),
        alias_again.schemas().serialized_request()
    ));
    {
        let mut state = state.lock().unwrap();
        assert_eq!(state.opens.len(), 4);
        assert!(state.opens.values().all(|&count| count == 1));
        assert!(state
            .callbacks
            .iter()
            .all(|id| *id == state.callbacks[0] && *id != thread::current().id()));
        state.files.clear();
    }
    assert_eq!(cache.load(alias.id()).unwrap().id(), alias.id());
    drop(cache);
    assert!(handles[0]
        .schemas()
        .materialize()
        .unwrap()
        .get(LATE)
        .is_ok());
}

#[test]
fn concurrent_extensions_merge_and_failed_batches_do_not_publish_partial_state() {
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }").unwrap();
    parser.add_source("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Used {} struct A {} struct B {} struct C {} struct Broken { value @0 :Missing; }").unwrap();
    let cache = parser.into_concurrent(&["main.capnp"]).unwrap();
    let threads: Vec<_> = ["A", "B", "Broken", "A", "B", "Broken"]
        .into_iter()
        .map(|name| {
            let cache = cache.clone();
            thread::spawn(move || (name, cache.get_nested(TYPES, name)))
        })
        .collect();
    for task in threads {
        let (name, result) = task.join().unwrap();
        assert_eq!(result.is_ok(), name != "Broken");
    }
    let before = cache.schemas().unwrap();
    assert_eq!(before.revision(), 2);
    let local = before.materialize().unwrap();
    let types = local.get(TYPES).unwrap();
    assert!(types.get_nested("A").is_ok());
    assert!(types.get_nested("B").is_ok());
    assert!(types.get_nested("C").is_err());
    assert!(cache.get_all_nested(TYPES).is_err());
    assert_eq!(cache.schemas().unwrap().revision(), before.revision());
    assert_eq!(
        cache.schemas().unwrap().serialized_request(),
        before.serialized_request()
    );
    assert!(cache.find_nested(TYPES, "Missing").unwrap().is_none());
    assert!(cache.load(1).is_err());
    assert!(cache.find_nested(1, "Missing").is_err());
    assert_eq!(cache.schemas().unwrap().revision(), 2);
    let all = cache.get_all_nested(MAIN).unwrap();
    assert_eq!(all.len(), 1);
}

#[test]
fn disk_cache_rolls_back_discovery_and_retries_corrected_files() {
    let directory = tempfile::tempdir().unwrap();
    // This test counts physical inputs; a symlinked temporary root would also
    // retain the requested main-file alias as an intentional dependency.
    let directory_path = directory.path().canonicalize().unwrap();
    let main = directory_path.join("main.capnp");
    let types = directory_path.join("types.capnp");
    let late = directory_path.join("late.capnp");
    std::fs::write(
        &main,
        "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }",
    )
    .unwrap();
    std::fs::write(&types, "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :import \"late.capnp\".Item; }").unwrap();
    let mut compiler = FileCompiler::new();
    compiler.src_prefix(&directory_path);
    let cache = compiler.into_concurrent(&[&main]).unwrap();
    let before = cache.schemas().unwrap();
    for text in [
        "@0xcccccccccccccccc; struct Item { x @0 :Missing; }",
        "@0xcccccccccccccccc; struct Item {} const hidden :Float64 = 1ee2;",
    ] {
        std::fs::write(&late, text).unwrap();
        assert!(matches!(
            cache.get_nested(TYPES, "Later"),
            Err(ParseError::Compile(_))
        ));
        assert_eq!(
            cache.schemas().unwrap().serialized_request(),
            before.serialized_request()
        );
        assert_eq!(
            cache.schemas().unwrap().dependencies(),
            before.dependencies()
        );
    }
    std::fs::write(&late, "@0xdddddddddddddddd; struct Item {}").unwrap();
    std::fs::remove_file(main).unwrap();
    std::fs::remove_file(types).unwrap();
    let handle = cache.get_nested(TYPES, "Later").unwrap();
    assert_eq!(handle.schemas().revision(), 1);
    assert!(handle
        .schemas()
        .dependencies()
        .contains(&late.canonicalize().unwrap()));
    assert!(handle.schemas().materialize().unwrap().get(LATE).is_err());
    assert!(handle
        .schemas()
        .materialize()
        .unwrap()
        .get(0xdddddddddddddddd)
        .is_ok());
    assert_eq!(before.dependencies().len(), 2);
}

#[test]
fn cache_loader_limits_reject_extensions_atomically() {
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }").unwrap();
    parser
        .add_source(
            "types.capnp",
            "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later {}",
        )
        .unwrap();
    let cache = parser
        .into_concurrent_with_limits(
            &["main.capnp"],
            Limits {
                nodes: 4,
                ..Limits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        cache.get_nested(TYPES, "Later"),
        Err(ParseError::Load(_))
    ));
    assert_eq!(cache.schemas().unwrap().revision(), 0);
    assert_eq!(
        cache
            .schemas()
            .unwrap()
            .materialize()
            .unwrap()
            .get_all_loaded()
            .count(),
        4
    );
}

#[test]
fn optional_ids_are_stable_within_a_cache_and_fresh_in_another() {
    let make = || {
        let mut parser = SchemaParser::new();
        parser.set_file_ids_required(false);
        parser
            .add_source("config.capnp", "struct Config { value @0 :UInt32; }")
            .unwrap();
        parser.into_concurrent(&["config.capnp"]).unwrap()
    };
    let cache = make();
    let file = cache.schemas().unwrap().get_file("config.capnp").unwrap();
    let expected = cache.get_nested(file.id(), "Config").unwrap().id();
    let other = cache.clone();
    assert_eq!(
        thread::spawn(move || other.get_nested(file.id(), "Config").unwrap().id())
            .join()
            .unwrap(),
        expected
    );
    assert_ne!(
        make()
            .schemas()
            .unwrap()
            .get_file("config.capnp")
            .unwrap()
            .id(),
        cache
            .schemas()
            .unwrap()
            .get_file("config.capnp")
            .unwrap()
            .id()
    );
}

#[test]
fn initialization_errors_are_returned_and_workers_are_joined_on_last_drop() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct OwnedProvider {
        dropped: Arc<AtomicBool>,
        reads: std::cell::Cell<u32>,
    }
    impl SourceProvider for OwnedProvider {
        fn resolve(&self, _: Option<&str>, _: &str) -> io::Result<Option<SourceFile>> {
            Ok(Some(SourceFile {
                identity: "s".into(),
                filename: "s.capnp".into(),
            }))
        }
        fn open(&self, _: &str) -> io::Result<Box<dyn Read + '_>> {
            self.reads.set(self.reads.get() + 1);
            Ok(Box::new(Cursor::new(b"@0xaaaaaaaaaaaaaaaa; struct S {}")))
        }
    }
    impl Drop for OwnedProvider {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let cache = ConcurrentSchemaParser::from_provider(
        OwnedProvider {
            dropped: dropped.clone(),
            reads: 0.into(),
        },
        &["s"],
    )
    .unwrap();
    let clone = cache.clone();
    drop(cache);
    assert!(!dropped.load(Ordering::SeqCst));
    drop(clone);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(SchemaParser::new()
        .into_concurrent(&["missing.capnp"])
        .is_err());
}

#[test]
fn provider_reentry_is_rejected_without_deadlocking() {
    struct Reentrant {
        client: Arc<Mutex<Option<ConcurrentSchemaParser>>>,
        rejected: Arc<std::sync::atomic::AtomicBool>,
    }
    impl SourceProvider for Reentrant {
        fn resolve(&self, _: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
            Ok(Some(SourceFile {
                identity: path.into(),
                filename: path.into(),
            }))
        }
        fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
            let source = match identity {
                "main.capnp" => "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }",
                "types.capnp" => "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :import \"late.capnp\".Item; }",
                "late.capnp" => {
                    let client = self.client.lock().unwrap().clone().unwrap();
                    assert!(client.schemas().err().unwrap().to_string().contains("reenter"));
                    self.rejected.store(true, std::sync::atomic::Ordering::SeqCst);
                    "@0xcccccccccccccccc; struct Item {}"
                }
                _ => unreachable!(),
            };
            Ok(Box::new(Cursor::new(source.as_bytes())))
        }
    }
    let client = Arc::new(Mutex::new(None));
    let rejected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cache = ConcurrentSchemaParser::from_provider(
        Reentrant {
            client: client.clone(),
            rejected: rejected.clone(),
        },
        &["main.capnp"],
    )
    .unwrap();
    *client.lock().unwrap() = Some(cache.clone());
    let (done, result) = std::sync::mpsc::channel();
    let worker = cache.clone();
    thread::spawn(move || {
        done.send(worker.get_nested(TYPES, "Later").map(|handle| handle.id()))
            .unwrap()
    });
    result
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .unwrap();
    assert!(rejected.load(std::sync::atomic::Ordering::SeqCst));
    // Break the deliberately installed callback/client ownership cycle.
    client.lock().unwrap().take();
}

#[test]
fn panicking_provider_stops_worker_and_unblocks_waiting_clients() {
    struct Panicking;
    impl SourceProvider for Panicking {
        fn resolve(&self, _: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
            Ok(Some(SourceFile {
                identity: path.into(),
                filename: path.into(),
            }))
        }
        fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
            let source = match identity {
                "main.capnp" => "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }",
                "types.capnp" => "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :import \"late.capnp\".Item; }",
                _ => panic!("injected provider panic"),
            };
            Ok(Box::new(Cursor::new(source.as_bytes())))
        }
    }
    let cache = ConcurrentSchemaParser::from_provider(Panicking, &["main.capnp"]).unwrap();
    let retained = cache.schemas().unwrap();
    let barrier = Arc::new(Barrier::new(24));
    let (done, results) = std::sync::mpsc::channel();
    for _ in 0..24 {
        let cache = cache.clone();
        let barrier = barrier.clone();
        let done = done.clone();
        thread::spawn(move || {
            barrier.wait();
            done.send(cache.get_nested(TYPES, "Later").err().unwrap().to_string())
                .unwrap();
        });
    }
    for _ in 0..24 {
        assert!(results
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .contains("worker stopped"));
    }
    assert!(cache.schemas().is_err());
    drop(cache);
    assert!(retained.materialize().unwrap().get(MAIN).is_ok());
}

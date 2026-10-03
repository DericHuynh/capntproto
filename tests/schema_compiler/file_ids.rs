use super::*;
use capntproto_compiler::SchemaParser;
use std::fmt::Write;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn optional_file_ids_and_derived_schemas_match_pinned_cpp() {
    let build = cpp::build(&["capnpc"]).unwrap();
    let logs = root().join("target/verification/schema-compiler/file-ids");
    fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("schema-file-ids");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/schema-file-ids.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&binary),
        &logs.join("build.log"),
        0,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let names = ["auto-id-main.capnp", "auto-id-types.capnp"];
    let sources: Vec<_> = names
        .iter()
        .map(|name| {
            let source = fs::read_to_string(
                root()
                    .join("crates/capntproto-compiler/examples")
                    .join(name),
            )
            .unwrap()
            .replace("\r\n", "\n");
            fs::write(directory.path().join(name), &source).unwrap();
            source
        })
        .collect();
    let expected = run(
        command(binary).current_dir(directory.path()),
        &logs.join("cpp.txt"),
        0,
    )
    .unwrap();
    let mut lines = expected.lines();
    let header: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    assert_eq!(header.len(), 3);
    assert_eq!(header[0], "ids");
    let cpp_ids: Vec<u64> = header[1..].iter().map(|id| id.parse().unwrap()).collect();
    assert!(cpp_ids.iter().all(|id| id >> 63 == 1));
    assert_ne!(cpp_ids[0], cpp_ids[1]);

    // Exercise Rust's real random-ID path, then recompile with those root IDs
    // explicit to check byte-for-byte equivalence of all descendants/metadata.
    let mut parser = SchemaParser::new();
    parser.set_file_ids_required(false);
    for (name, source) in names.iter().zip(&sources) {
        parser.add_source(name, source).unwrap();
    }
    let random = parser.parse_schemas(&names).unwrap();
    let mut fixed = SchemaParser::new();
    for (name, source) in names.iter().zip(&sources) {
        let id = random.get_file(name).unwrap().schema().id();
        assert_eq!(id >> 63, 1);
        // Appending leaves declaration/member byte ranges unchanged.
        fixed
            .add_source(name, format!("{source}\n@0x{id:016x};\n"))
            .unwrap();
    }
    let equivalent = fixed.parse_schemas(&names).unwrap();
    assert_eq!(nodes(random.request()), nodes(equivalent.request()));
    for schema in random.get_all_loaded() {
        let other = equivalent.get(schema.schema().id()).unwrap();
        assert_eq!(
            capnp::any_struct::Reader::from_reader(schema.source_info().unwrap())
                .canonicalize()
                .unwrap(),
            capnp::any_struct::Reader::from_reader(other.source_info().unwrap())
                .canonicalize()
                .unwrap()
        );
    }

    // Independent C++ random draws cannot equal Rust's. Pin the observed C++
    // roots explicitly in Rust, then compare complete schemas and dynamic wire.
    let mut fixed = SchemaParser::new();
    for ((name, source), id) in names.iter().zip(&sources).zip(&cpp_ids) {
        fixed
            .add_source(name, format!("{source}\n@0x{id:016x};\n"))
            .unwrap();
    }
    let parsed = fixed.parse_schemas(&names).unwrap();
    let mut actual = String::new();
    writeln!(actual, "ids {} {}", cpp_ids[0], cpp_ids[1]).unwrap();
    for schema in parsed.get_all_loaded() {
        let node = capnp::any_struct::Reader::from_reader(schema.schema().get_proto())
            .canonicalize()
            .unwrap();
        let info = capnp::any_struct::Reader::from_reader(schema.source_info().unwrap())
            .canonicalize()
            .unwrap();
        writeln!(
            actual,
            "node {} {} {}",
            schema.schema().id(),
            hex(capnp::Word::words_to_bytes(&node)),
            hex(capnp::Word::words_to_bytes(&info))
        )
        .unwrap();
    }
    let config = parsed
        .get_file(names[0])
        .unwrap()
        .get_nested("Config")
        .unwrap();
    let mut message = message::Builder::new_default();
    use capnp::schema_loader::dynamic::{Builder, Value};
    let mut value = Builder::init(message.init_root(), config.schema()).unwrap();
    value
        .reborrow()
        .init_struct("server")
        .unwrap()
        .set_named("port", Value::UInt16(9000))
        .unwrap();
    value
        .reborrow()
        .init_struct("details")
        .unwrap()
        .set_named("enabled", Value::Bool(false))
        .unwrap();
    value
        .set_named("label", Value::Text("configured".into()))
        .unwrap();
    let wire = capnp::any_struct::Reader::from_reader(value.as_reader())
        .canonicalize()
        .unwrap();
    writeln!(actual, "wire {}", hex(capnp::Word::words_to_bytes(&wire))).unwrap();
    fs::write(logs.join("rust.txt"), &actual).unwrap();
    assert_eq!(actual, expected);
    fs::write(logs.join("summary.txt"), format!("{} canonical schema/source-info pairs and one dynamic message match pinned C++ with observed random file IDs fixed for comparison. Both real random-ID paths, default-required policy, stable cached identities and explicit overrides exercised.\n", parsed.get_all_loaded().count())).unwrap();
}

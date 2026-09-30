use super::*;
use std::fmt::Write;

const MAIN_ID: u64 = 0xaabd19620168fc40;
const TYPES_ID: u64 = 0xbabd19620168fc40;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn dump(schemas: &capnp_compiler::ParsedSchemas, output: &mut String, step: usize, result: u64) {
    writeln!(output, "stage {step} {result}").unwrap();
    for schema in schemas.get_all_loaded() {
        let id = schema.schema().id();
        let node = capnp::any_struct::Reader::from_reader(schema.schema().get_proto())
            .canonicalize()
            .unwrap();
        let info = capnp::any_struct::Reader::from_reader(schema.source_info().unwrap())
            .canonicalize()
            .unwrap();
        writeln!(
            output,
            "node {} {} {}",
            id,
            hex(capnp::Word::words_to_bytes(&node)),
            hex(capnp::Word::words_to_bytes(&info))
        )
        .unwrap();
    }
}

#[test]
fn lazy_session_declaration_closures_match_cpp_compiler_loader() {
    let build = cpp::build(&["capnpc"]).unwrap();
    let logs = root().join("target/verification/schema-compiler/session");
    fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("schema-session");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/schema-session.c++",
                "vendor/capnproto/c++/src/capnp/compiler/module-loader.c++",
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
    for file in [
        "session-main.capnp",
        "session-types.capnp",
        "session-late.capnp",
        "reflection.txt",
    ] {
        let source =
            fs::read_to_string(root().join("crates/capnp-compiler/examples").join(file)).unwrap();
        fs::write(directory.path().join(file), source.replace("\r\n", "\n")).unwrap();
    }
    let mut compiler = capnp_compiler::FileCompiler::new();
    compiler.src_prefix(directory.path());
    let mut session = compiler
        .parse_session(&[directory.path().join("session-main.capnp")])
        .unwrap();
    let mut actual = String::new();
    dump(session.schemas(), &mut actual, 0, MAIN_ID);
    let cache = compiler
        .into_concurrent(&[directory.path().join("session-main.capnp")])
        .unwrap();
    let mut cached = String::new();
    dump(
        &cache.schemas().unwrap().materialize().unwrap(),
        &mut cached,
        0,
        MAIN_ID,
    );
    let box_id = session
        .schemas()
        .get(TYPES_ID)
        .unwrap()
        .schema()
        .get_proto()
        .get_nested_nodes()
        .unwrap()
        .iter()
        .find(|node| node.get_name().unwrap() == "Box")
        .unwrap()
        .get_id();
    let queries = [
        (MAIN_ID, "Alias"),
        (TYPES_ID, "Used"),
        (TYPES_ID, "Box"),
        (box_id, "Alias"),
        (box_id, "*"),
        (TYPES_ID, "Later"),
        (TYPES_ID, "TextBox"),
        (TYPES_ID, "Remote"),
        (TYPES_ID, "limitAlias"),
        (TYPES_ID, "noteAlias"),
        (box_id, "Parameter"),
        (TYPES_ID, "Service"),
        (TYPES_ID, "limit"),
        (TYPES_ID, "note"),
        (TYPES_ID, "Alias"),
        (TYPES_ID, "Missing"),
        (TYPES_ID, "Later"),
    ];
    let mut inputs = String::new();
    for (step, (parent, name)) in queries.iter().enumerate() {
        writeln!(inputs, "{parent} {name}").unwrap();
        let result = if *name == "*" {
            session.get_all_nested(*parent).unwrap();
            *parent
        } else {
            session
                .find_nested(*parent, name)
                .unwrap()
                .map_or(0, |schema| schema.schema().id())
        };
        dump(session.schemas(), &mut actual, step + 1, result);
        let cached_result = if *name == "*" {
            cache.get_all_nested(*parent).unwrap();
            *parent
        } else {
            cache
                .find_nested(*parent, name)
                .unwrap()
                .map_or(0, |schema| schema.id())
        };
        dump(
            &cache.schemas().unwrap().materialize().unwrap(),
            &mut cached,
            step + 1,
            cached_result,
        );
    }
    assert_eq!(session.schemas().requested_files().len(), 1);
    let input_path = logs.join("queries.txt");
    fs::write(&input_path, inputs).unwrap();
    fs::write(logs.join("rust.txt"), &actual).unwrap();
    fs::write(logs.join("cache.txt"), &cached).unwrap();
    let expected = run(
        command(binary)
            .arg(input_path)
            .current_dir(directory.path()),
        &logs.join("cpp.txt"),
        0,
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(cached, expected);
    fs::write(logs.join("summary.txt"), format!("{} snapshots from both exclusive sessions and concurrent caches match pinned C++ Compiler lookup/lazy loading plus dependency/parent completion, including declaration aliases\n", queries.len() + 1)).unwrap();
}

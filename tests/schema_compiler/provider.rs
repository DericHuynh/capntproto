use super::*;
use capnp_compiler::{ParsedSchema, SourceCompiler, SourceFile, SourceProvider};
use std::{
    fmt::Write,
    io::{self, Read},
};

struct Provider {
    main: Vec<u8>,
    types: Vec<u8>,
}

impl SourceProvider for Provider {
    fn resolve(&self, from: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
        let file = match (from, path) {
            (None, "entry" | "alias") | (Some("main"), "self") | (Some("types"), "root:alias") => {
                ("main", "main.capnp")
            }
            (Some("main"), "pkg:types" | "/alias//../types") => ("types", "types.capnp"),
            (Some("main"), "asset:blob") => ("blob", "blob.bin"),
            (_, "never:open") => panic!("unused import must remain lazy"),
            _ => return Ok(None),
        };
        Ok(Some(SourceFile {
            identity: file.0.into(),
            filename: file.1.into(),
        }))
    }
    fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
        let bytes: &[u8] = match identity {
            "main" => &self.main,
            "types" => &self.types,
            "blob" => &[0, 255, 42],
            _ => return Err(io::ErrorKind::NotFound.into()),
        };
        Ok(Box::new(bytes))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn dump(schema: ParsedSchema<'_>, output: &mut String) {
    let node = capnp::any_struct::Reader::from_reader(schema.schema().get_proto())
        .canonicalize()
        .unwrap();
    let info = capnp::any_struct::Reader::from_reader(schema.source_info().unwrap())
        .canonicalize()
        .unwrap();
    writeln!(
        output,
        "node {} {} {}",
        schema.schema().id(),
        hex(capnp::Word::words_to_bytes(&node)),
        hex(capnp::Word::words_to_bytes(&info))
    )
    .unwrap();
}

#[test]
fn custom_source_identity_aliases_embeds_and_lazy_lookup_match_cpp_schema_files() {
    let build = cpp::build(&["capnpc"]).unwrap();
    let logs = root().join("target/verification/schema-compiler/provider");
    fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("schema-provider");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/schema-provider.c++",
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
    let sources: Vec<_> = ["main", "types"]
        .iter()
        .map(|key| {
            let name = format!("provider-{key}.capnp");
            let text =
                fs::read_to_string(root().join("crates/capnp-compiler/examples").join(&name))
                    .unwrap()
                    .replace("\r\n", "\n");
            fs::write(directory.path().join(name), &text).unwrap();
            text.into_bytes()
        })
        .collect();
    let provider = Provider {
        main: sources[0].clone(),
        types: sources[1].clone(),
    };
    let mut session = SourceCompiler::new(&provider)
        .parse_session(&["entry", "alias"])
        .unwrap();
    assert_eq!(session.schemas().requested_files().len(), 1);
    let mut actual = String::new();
    let main = 0xaaaaaaaaaaaaaaaa;
    let types = 0xbbbbbbbbbbbbbbbb;
    dump(session.schemas().get(main).unwrap(), &mut actual);
    dump(session.get_nested(main, "Root").unwrap(), &mut actual);
    dump(session.get_nested(main, "blob").unwrap(), &mut actual);
    assert_eq!(session.get_nested(main, "A").unwrap().schema().id(), types);
    assert_eq!(session.get_nested(main, "B").unwrap().schema().id(), types);
    dump(session.schemas().get(types).unwrap(), &mut actual);
    dump(session.get_nested(types, "Item").unwrap(), &mut actual);
    let later = session.get_nested(types, "Later").unwrap();
    let later_id = later.schema().id();
    dump(later, &mut actual);
    let root = session.get_nested(main, "Root").unwrap();
    let mut message = message::Builder::new_default();
    use capnp::schema_loader::dynamic::{Builder, Value};
    let mut value = Builder::init(message.init_root(), root.schema()).unwrap();
    let Value::Data(data) = value.as_reader().get_named("payload").unwrap() else {
        panic!()
    };
    assert_eq!(data, [0, 255, 42]);
    value
        .reborrow()
        .init_struct("a")
        .unwrap()
        .set_named("value", Value::UInt32(71))
        .unwrap();
    let wire = capnp::any_struct::Reader::from_reader(value.as_reader())
        .canonicalize()
        .unwrap();
    writeln!(actual, "wire {}", hex(capnp::Word::words_to_bytes(&wire))).unwrap();
    fs::write(logs.join("rust.txt"), &actual).unwrap();
    let expected = run(
        command(binary).arg(directory.path()),
        &logs.join("cpp.txt"),
        0,
    )
    .unwrap();
    assert_eq!(actual.lines().count(), 7);
    assert_eq!(expected.lines().count(), 7);
    let mut absent_info = Vec::new();
    for (actual, expected) in actual.lines().zip(expected.lines()) {
        let left: Vec<_> = actual.split_whitespace().collect();
        let right: Vec<_> = expected.split_whitespace().collect();
        match (left.as_slice(), right.as_slice()) {
            (["node", id, node, _], ["node", expected_id, expected_node, "-"]) => {
                // C++ lazy lookup omits SourceInfo for the imported file and
                // lazily loaded sibling. Their complete schema bytes still match.
                assert_eq!((id, node), (expected_id, expected_node));
                absent_info.push(id.parse::<u64>().unwrap());
            }
            _ => assert_eq!(actual, expected),
        }
    }
    assert_eq!(absent_info, [types, later_id]);
    fs::write(logs.join("summary.txt"), "6 schema nodes, 4 available source-info records and 1 dynamic wire message match pinned C++ SchemaFile callbacks; C++ omits source-info for the imported file and lazy sibling. Schema identities, cycles, aliases, lazy lookup and binary embeds exercised.\n").unwrap();
}

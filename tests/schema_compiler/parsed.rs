use super::*;
use capnp::schema_loader::{
    dynamic::{self, Value},
    Kind,
};
use std::fmt::Write;

fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".into();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn parsed_schema_navigation_metadata_and_dynamic_wire_match_cpp() {
    let build = cpp::build(&["capnpc"]).unwrap();
    let logs = root().join("target/verification/schema-compiler/parsed");
    fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("schema-parser");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/schema-parser.c++",
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
    for name in [
        "reflection.capnp",
        "reflection-common.capnp",
        "reflection.txt",
    ] {
        // Match the native fixture independently of checkout line endings.
        let source = fs::read_to_string(
            root()
                .join("crates/capntproto-compiler/examples")
                .join(name),
        )
        .unwrap();
        fs::write(directory.path().join(name), source.replace("\r\n", "\n")).unwrap();
    }
    let expected = run(
        command(&binary).current_dir(directory.path()),
        &logs.join("cpp.txt"),
        0,
    )
    .unwrap();
    let mut compiler = capntproto_compiler::FileCompiler::new();
    compiler.src_prefix(directory.path());
    let parsed = compiler
        .parse_schemas(&[
            directory.path().join("reflection.capnp"),
            directory.path().join("reflection-common.capnp"),
        ])
        .unwrap();
    // Source files and the compiler need not remain alive during reflection.
    drop(compiler);
    drop(directory);
    let mut actual = String::new();
    for schema in parsed.get_all_loaded() {
        let schema_id = schema.schema().id();
        let proto = schema.schema().get_proto();
        let info = schema.source_info().unwrap();
        let kind = match schema.schema().kind() {
            Kind::File => 0,
            Kind::Struct => 1,
            Kind::Enum => 2,
            Kind::Interface => 3,
            Kind::Const => 4,
            Kind::Annotation => 5,
        };
        writeln!(
            actual,
            "node {} {} {} {} {} {} {} {}",
            schema_id,
            kind,
            hex(proto.get_display_name().unwrap().as_bytes()),
            proto.get_start_byte(),
            proto.get_end_byte(),
            info.get_start_byte(),
            info.get_end_byte(),
            hex(info.get_doc_comment().unwrap().as_bytes())
        )
        .unwrap();
        for child in proto.get_nested_nodes().unwrap() {
            writeln!(
                actual,
                "child {} {} {}",
                schema_id,
                child.get_id(),
                hex(child.get_name().unwrap().as_bytes())
            )
            .unwrap();
        }
        for (index, member) in info.get_members().unwrap().iter().enumerate() {
            writeln!(
                actual,
                "member {} {} {} {} {}",
                schema_id,
                index,
                member.get_start_byte(),
                member.get_end_byte(),
                hex(member.get_doc_comment().unwrap().as_bytes())
            )
            .unwrap();
        }
    }
    let file = parsed.get_file("reflection.capnp").unwrap();
    let common = parsed.get_file("reflection-common.capnp").unwrap();
    for parent in [
        file.clone(),
        common.clone(),
        file.get_nested("Record").unwrap(),
        common.get_nested("Box").unwrap(),
    ] {
        assert!(parent.find_nested("Missing").unwrap().is_none());
        for child in parent.get_all_nested().unwrap() {
            let name = child.schema().short_display_name().unwrap();
            let name = name.to_str().unwrap();
            assert!(parent
                .get_nested(name)
                .unwrap()
                .schema()
                .equals(&child.schema()));
            assert!(parent
                .find_nested(name)
                .unwrap()
                .unwrap()
                .schema()
                .equals(&child.schema()));
            writeln!(
                actual,
                "lookup {} {} {}",
                parent.schema().id(),
                hex(name.as_bytes()),
                child.schema().id()
            )
            .unwrap();
        }
    }
    let mut message = message::Builder::new_default();
    let mut value = dynamic::Builder::init(
        message.init_root(),
        file.get_nested("Record").unwrap().schema(),
    )
    .unwrap();
    value.set_named("count", Value::UInt32(99)).unwrap();
    value
        .reborrow()
        .init_struct("box")
        .unwrap()
        .set_named("value", Value::Text("boxed".into()))
        .unwrap();
    value
        .reborrow()
        .init_struct("details")
        .unwrap()
        .set_named("active", Value::Bool(false))
        .unwrap();
    value
        .reborrow()
        .init_struct("child")
        .unwrap()
        .set_named("label", Value::Text("child".into()))
        .unwrap();
    value
        .reborrow()
        .init_list("entries", 2)
        .unwrap()
        .reborrow()
        .get_struct(1)
        .unwrap()
        .set_named("value", Value::Int16(123))
        .unwrap();
    let canonical = capnp::any_struct::Reader::from_reader(value.as_reader())
        .canonicalize()
        .unwrap();
    writeln!(
        actual,
        "wire {}",
        hex(capnp::Word::words_to_bytes(&canonical))
    )
    .unwrap();
    fs::write(logs.join("rust.txt"), &actual).unwrap();
    assert_eq!(actual, expected);
    fs::write(logs.join("summary.txt"), format!("{} parsed declarations: IDs, kinds, ranges, comments, direct lookups and canonical dynamic wire match pinned C++ SchemaParser\n", parsed.get_all_loaded().count())).unwrap();
}

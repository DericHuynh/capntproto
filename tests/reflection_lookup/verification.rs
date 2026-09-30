use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};
use std::collections::BTreeSet;
fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/reflection-lookup-cpp");
    let tmp = tempfile::tempdir().unwrap();
    run(
        command(bin.join("capnp")).args([
            "compile",
            &format!(
                "-o{}:{}",
                bin.join("capnpc-c++").display(),
                tmp.path().display()
            ),
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/reflection-lookup.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("reflection-lookup");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/reflection-lookup.c++")
            .arg(tmp.path().join("reflection-lookup.capnp.c++"))
            .arg(bin.join("libcapnpc.a"))
            .arg(bin.join("libcapnp-rpc.a"))
            .arg(bin.join("libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj-async.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&exe),
        &logs.join("compile.log"),
        0,
    )
    .unwrap();
    let input = logs.join("cases.txt");
    std::fs::write(&input, inputs).unwrap();
    let output = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    let mut actual = output.lines();
    let mut expected = expected.lines();
    for case in inputs.lines() {
        assert_eq!(actual.next(), expected.next(), "{case}");
    }
    assert!(actual.next().is_none() && expected.next().is_none());
    eprintln!(
        "{} pinned C++ reflection observations",
        inputs.lines().count()
    );
}
fn compiled_query(schema: InterfaceSchema, op: u64, needle: u64) -> u64 {
    if op == 0 {
        match schema.find_method_by_name(if needle == 1 { "match" } else { "missing" }) {
            Ok(Some(m)) => m.get_containing_interface().get_proto().get_id(),
            Ok(None) => 0,
            Err(_) => 9,
        }
    } else {
        let id = match needle {
            0 => u64::MAX,
            1 => fixture::root::Client::<capnp::text::Owned>::schema()
                .get_proto()
                .get_id(),
            2 => fixture::empty::Client::schema().get_proto().get_id(),
            3 => schema.get_proto().get_id(),
            _ => panic!(),
        };
        match schema.find_superclass(id) {
            Ok(Some(s)) => s.get_proto().get_id(),
            Ok(None) => 0,
            Err(_) => 9,
        }
    }
}
fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
#[test]
fn tlc_inheritance_queries_and_cpp_reflection_reference() {
    const MODEL: &str = "verification/SchemaLookup.tla";
    const CONFIG: &str = include_str!("../../verification/SchemaLookup.cfg");
    let paths = exploration::traces(MODEL, "schema-lookup", CONFIG).unwrap();
    let mut cases = BTreeSet::new();
    let mut inputs = String::new();
    let mut expected = String::new();
    for path in paths {
        let state = path.last().unwrap();
        if state["status"] != 2 {
            continue;
        }
        let (g, o, q) = (state["graph"], state["op"], state["needle"]);
        if !cases.insert((g, o, q)) {
            continue;
        }
        let observed = query(&graph_loader(g), o, q);
        assert_eq!(observed, state["result"], "{path:?}");
        inputs += &format!("Q {g} {o} {q}\n");
        expected += &format!("{observed}\n");
    }
    assert_eq!(cases.len(), 66);
    for (index, schema) in [
        fixture::diamond::Client::<capnp::text::Owned>::schema(),
        fixture::override_::Client::<capnp::text::Owned>::schema(),
        fixture::limit63::Client::schema(),
        fixture::limit64::Client::schema(),
        fixture::limit65::Client::schema(),
    ]
    .into_iter()
    .enumerate()
    {
        for op in 0..2 {
            for needle in 0..if op == 0 { 2 } else { 4 } {
                inputs += &format!("C {index} {op} {needle}\n");
                expected += &format!("{}\n", compiled_query(schema, op, needle));
            }
        }
    }
    for (index, name) in [
        "zulu", "alpha", "middle", "", "missing", "Alpha", "alpha\0", "☃",
    ]
    .into_iter()
    .enumerate()
    {
        inputs += &format!("E {index}\n");
        expected += &format!(
            "{}\n",
            enum_schema()
                .find_enumerant_by_name(name)
                .unwrap()
                .map_or(0, |e| u64::from(e.get_ordinal()) + 1)
        );
    }
    for (index, name) in ["", "path:Outer.Inner", "p:é.Name"].into_iter().enumerate() {
        for prefix in 0..=name.len() {
            let mut message = interface_node(900, &[], false);
            let mut n: node::Builder = message.get_root().unwrap();
            n.set_display_name(name);
            n.set_display_name_prefix_length(prefix as u32);
            let mut loader = SchemaLoader::default();
            let schema = loader.load(message.get_root_as_reader().unwrap()).unwrap();
            inputs += &format!("N {index} {prefix}\n");
            expected += &format!(
                "{} {}\n",
                hex(schema.short_display_name().unwrap().as_bytes()),
                hex(schema.unqualified_name().unwrap().as_bytes())
            );
        }
    }
    compare_cpp(&inputs, &expected);
    exploration::controls(
        MODEL,
        "schema-lookup",
        CONFIG,
        &[
            ("reverseBranches", "LookupResult"),
            ("skipSelf", "LookupResult"),
            ("wrongOwner", "LookupResult"),
            ("swallowLimit", "LookupResult"),
            ("earlyLimit", "ExactBudget"),
        ],
        None,
    )
    .unwrap();
}

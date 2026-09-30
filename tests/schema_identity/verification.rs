use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/schema-identity-cpp");
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
    let exe = tmp.path().join("schema-identity");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/schema-identity.c++")
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
        "{} pinned C++ member-identity scenarios",
        inputs.lines().count()
    );
}

#[test]
fn tlc_cache_traces_and_cpp_member_identity_reference() {
    const MODEL: &str = "verification/SchemaMemberIdentity.tla";
    const CONFIG: &str = include_str!("../../verification/SchemaMemberIdentity.cfg");
    let paths = exploration::traces(MODEL, "schema-member-identity", CONFIG).unwrap();
    let a = loader();
    let b = a.clone();
    let catalog = Catalog {
        a: &a,
        b: &b,
        foreign: foreign_cache(),
    };
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    for compiled in [false, true] {
        for path in &paths {
            let mut cache = CollisionMap::default();
            let mut selected = 1;
            inputs += &format!("T {}", u8::from(compiled));
            for state in path {
                let event = state["event"];
                inputs += &format!(" {event}");
                let result = match event {
                    1..=9 => {
                        selected = event;
                        false
                    }
                    10 => cache.insert(catalog.key(selected, compiled), 73).is_none(),
                    11 => cache.contains_key(&catalog.key(selected, compiled)),
                    12 => cache.remove(&catalog.key(selected, compiled)).is_some(),
                    _ => panic!("unexpected state {state:?}"),
                };
                assert_eq!(selected, state["selected"]);
                let mut mask = 0;
                for index in 1..=9 {
                    if cache.contains_key(&catalog.key(index, compiled)) {
                        mask |= 1 << (canonical(index) - 1);
                    }
                }
                assert_eq!(
                    (u64::from(result), cache.len() as u64, mask),
                    (state["result"], state["size"], state["mask"]),
                    "compiled={compiled} path={path:?}"
                );
                expected += &format!("{} {} {mask},", u8::from(result), cache.len());
                observations += 1;
            }
            inputs.push('\n');
            expected.push('\n');
        }
        for i in 1..=9 {
            for j in 1..=9 {
                inputs += &format!("P {} {i} {j}\n", u8::from(compiled));
                expected += &format!(
                    "{}\n",
                    u8::from(catalog.key(i, compiled) == catalog.key(j, compiled))
                );
            }
        }
    }
    let id = cache::<capnp::text::Owned>().get_proto().get_id();
    for i in 0..7 {
        for j in 0..7 {
            inputs += &format!("B {i} {j}\n");
            expected += &format!(
                "{}\n",
                u8::from(
                    bound(&a, id, i).identity().unwrap() == bound(&a, id, j).identity().unwrap()
                )
            );
        }
    }
    compare_cpp(&inputs, &expected);
    eprintln!("{observations} Rust/C++ cache observations");
    exploration::controls(
        MODEL,
        "schema-member-identity",
        CONFIG,
        &[
            ("dropOwner", "CacheIdentity"),
            ("dropBrand", "CacheIdentity"),
            ("dropIndex", "CacheIdentity"),
            ("wrongInheritedOwner", "CacheIdentity"),
            ("splitAlias", "CacheIdentity"),
        ],
        None,
    )
    .unwrap();
}

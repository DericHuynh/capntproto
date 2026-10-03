use super::super::*;
use capntproto_test_support::verification::{command, cpp, exploration, root, run};
use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

#[derive(Default)]
struct CollisionHasher;
impl Hasher for CollisionHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _: &[u8]) {}
}

fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/enum-brand-cpp");
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
            "schemas/enum-brand.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("enum-brand");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/enum-brand.c++")
            .arg(tmp.path().join("enum-brand.capnp.c++"))
            .arg(bin.join("libcapnpc.a"))
            .arg(bin.join("libcapnp.a"))
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
    eprintln!("{} pinned C++ enum-scope scenarios", inputs.lines().count());
}
#[test]
fn tlc_enum_scope_cache_and_assignments_match_cpp() {
    const MODEL: &str = "verification/EnumScopeIdentity.tla";
    const CONFIG: &str = include_str!("../../verification/EnumScopeIdentity.cfg");
    let paths = exploration::traces(MODEL, "enum-scope-identity", CONFIG).unwrap();
    let loader = loader();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    for path in paths {
        let mode = path[0]["mode"];
        let mut cache: HashMap<_, _, BuildHasherDefault<CollisionHasher>> = HashMap::default();
        let mut selected = 0;
        let mut message = message::Builder::new_default();
        message.init_root::<fixture::scope::Builder<capnp::text::Owned>>();
        inputs += &format!("T {mode}");
        for state in &path {
            let event = state["event"];
            inputs += &format!(" {event}");
            let result = match event {
                1..=5 => {
                    selected = event - 1;
                    false
                }
                6 => cache.insert(key(&loader, mode, selected), 1).is_none(),
                7 => cache.contains_key(&key(&loader, mode, selected)),
                8 | 9 => write(
                    &mut message,
                    &loader,
                    mode,
                    selected,
                    if event == 8 { 1 } else { u16::MAX },
                ),
                10 | 11 => {
                    assert_eq!(event, 10 + mode);
                    false
                }
                _ => panic!("{state:?}"),
            };
            let mut mask = 0;
            for index in 0..5 {
                if cache.contains_key(&key(&loader, mode, index)) {
                    mask |= 1 << canonical(mode, index);
                }
            }
            let value = read(&message);
            assert_eq!(selected, state["selected"]);
            assert_eq!(
                (
                    u64::from(result),
                    cache.len() as u64,
                    mask,
                    u64::from(value)
                ),
                (
                    state["result"],
                    state["size"],
                    state["mask"],
                    state["value"]
                ),
                "{path:?}"
            );
            expected += &format!("{} {} {mask} {value},", u8::from(result), cache.len());
            observations += 1;
        }
        inputs.push('\n');
        expected.push('\n');
    }
    for mode in 0..=1 {
        for i in 0..5 {
            for j in 0..5 {
                inputs += &format!("P {mode} {i} {j}\n");
                expected += &format!(
                    "{}\n",
                    u8::from(key(&loader, mode, i) == key(&loader, mode, j))
                );
            }
        }
    }
    compare_cpp(&inputs, &expected);
    eprintln!("{observations} Rust/C++ enum-scope state observations");
    exploration::controls(
        MODEL,
        "enum-scope-identity",
        CONFIG,
        &[
            ("eraseLoadedBrand", "CacheIdentity"),
            ("retainCompiledBrand", "CacheIdentity"),
            ("loseListBrand", "CacheIdentity"),
            ("mergeEnumTypes", "CacheIdentity"),
            ("acceptWrongBrand", "OperationResult"),
            ("truncateUnknown", "StoredOrdinal"),
        ],
        None,
    )
    .unwrap();
}

use super::*;

#[path = "../../crates/capnp-compiler/tests/corpus/discovery.rs"]
mod corpus;

#[test]
fn semantic_import_discovery_matches_cpp_without_renaming() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    for &(name, source) in corpus::SOURCES {
        fs::write(directory.path().join(name), source).unwrap();
    }
    let logs = root().join("target/verification/schema-compiler/discovery");
    fs::create_dir_all(&logs).unwrap();
    let mut failures = Vec::new();
    for case in corpus::cases() {
        fs::write(directory.path().join("src/main.capnp"), case.source()).unwrap();
        fs::write(logs.join(format!("{}.capnp", case.name)), case.source()).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "-Isrc"])
            .args(case.requested())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        let reference =
            capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
                .unwrap();
        let expected = reference
            .get_root::<code_generator_request::Reader<'_>>()
            .unwrap();
        let leaf = expected
            .get_nodes()
            .unwrap()
            .iter()
            .find(|node| node.get_id() == corpus::LEAF_ID)
            .unwrap();
        assert_eq!(
            leaf.get_display_name().unwrap(),
            case.leaf_name,
            "{}",
            case.name
        );
        let rust = capnp_compiler::FileCompiler::new()
            .src_prefix(directory.path())
            .import_path(directory.path().join("src"))
            .compile(
                &case
                    .requested()
                    .iter()
                    .map(|name| directory.path().join(name))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let actual = rust
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        fs::write(
            logs.join(format!("{}.rust.txt", case.name)),
            format!("{actual:?}"),
        )
        .unwrap();
        fs::write(
            logs.join(format!("{}.cpp.txt", case.name)),
            format!("{expected:?}"),
        )
        .unwrap();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compare_requests(actual, expected)
        }))
        .is_err()
        {
            failures.push(case.name);
        }
    }
    assert!(failures.is_empty(), "request mismatches: {failures:?}");
    fs::write(logs.join("summary.txt"), format!("{} import-discovery cases match pinned C++ known request fields, canonical values/source info and normalized identifiers, without display-name adjustments. Compiler version excluded.\n", corpus::cases().len())).unwrap();
}

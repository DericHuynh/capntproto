use super::*;

#[test]
fn every_pinned_upstream_schema_matches_cpp() {
    fn schemas(directory: &std::path::Path, result: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            let path = entry.path();
            // Ekam's provider links mirror the tree; visit actual sources once.
            if kind.is_dir() {
                schemas(&path, result);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "capnp") {
                result.push(path);
            }
        }
    }
    let root = root();
    let tree = root.join("vendor/capnproto/c++");
    let standard = tree.join("src");
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let mut files = Vec::new();
    schemas(&tree, &mut files);
    files.sort();
    assert_eq!(
        files.len(),
        22,
        "update the qualified corpus when the reference changes"
    );
    let logs = root.join("target/verification/schema-compiler/upstream");
    fs::create_dir_all(&logs).unwrap();
    let mut failures = Vec::new();
    for file in &files {
        let name = file.strip_prefix(&tree).unwrap();
        // Match normal compiler invocation: src is the standard include root;
        // sample schemas live in their separate source root.
        let prefix = if file.starts_with(&standard) {
            standard.clone()
        } else {
            tree.join("samples")
        };
        let requested = file.strip_prefix(&prefix).unwrap();
        let mut frontend = capnp_compiler::FileCompiler::new();
        frontend.src_prefix(&prefix).import_path(&standard);
        let output = command(&compiler)
            .current_dir(&prefix)
            .args(["compile", "-o-", "-I"])
            .arg(&standard)
            .arg(requested)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            name.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let rust = frontend
            .compile(&[file])
            .unwrap_or_else(|e| panic!("{}: {e}", name.display()));
        let reference =
            capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
                .unwrap();
        let actual = rust
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        let expected = reference
            .get_root::<code_generator_request::Reader<'_>>()
            .unwrap();
        let label = name.to_str().unwrap().replace(['/', '\\'], "_");
        fs::write(
            logs.join(format!("{label}.rust.txt")),
            format!("{actual:?}"),
        )
        .unwrap();
        fs::write(
            logs.join(format!("{label}.cpp.txt")),
            format!("{expected:?}"),
        )
        .unwrap();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compare_requests(actual, expected)
        }))
        .is_err()
        {
            failures.push(name.display().to_string());
        }
    }
    assert!(failures.is_empty(), "request mismatches: {failures:?}");
    fs::write(logs.join("summary.txt"), format!("{} unmodified pinned upstream schemas: known request fields, canonical values/source info and normalized identifiers agree with C++. Compiler version excluded.\n", files.len())).unwrap();
}

#[test]
fn mixed_import_roots_match_first_discovered_display_names() {
    let tree = root().join("vendor/capnproto/c++");
    let standard = tree.join("src");
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let output = command(&compiler)
        .current_dir(&tree)
        .args(["compile", "-o-", "-I"])
        .arg(&standard)
        .arg("src/capnp/test-import2.capnp")
        .output()
        .unwrap();
    assert!(output.status.success());
    let reference =
        capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
            .unwrap();
    let rust = capnp_compiler::FileCompiler::new()
        .src_prefix(&tree)
        .import_path(&standard)
        .compile(&[standard.join("capnp/test-import2.capnp")])
        .unwrap();
    compare_requests(
        rust.get_root_as_reader().unwrap(),
        reference.get_root().unwrap(),
    );
}

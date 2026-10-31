use super::*;

#[test]
fn native_builder_matches_manual_codegen_with_imports_and_options() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("schema");
    let output = directory.path().join("generated");
    let expected = directory.path().join("manual");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::create_dir_all(&expected).unwrap();
    std::fs::write(
        source.join("common.capnp"),
        "@0x91ba7f8d25c9e601; struct Item { value @0 :UInt32; }",
    )
    .unwrap();
    let file = source.join("service.capnp");
    std::fs::write(&file, "@0x91ba7f8d25c9e602; using Common = import \"common.capnp\"; interface Service { read @0 () -> (item :Common.Item); }").unwrap();
    let raw = directory.path().join("request.bin");
    let mut command = CompilerCommand::new();
    command
        .src_prefix(&source)
        .import_path(&source)
        .file(&file)
        .output_path(&output)
        .capnp_root("::runtime")
        .field_api_values(true)
        .field_api_projections(true)
        .structured_replies(true)
        .default_parent_module(vec!["bindings".into()])
        .raw_code_generator_request_path(&raw)
        .run_native()
        .unwrap();
    let compiled = capntproto_compiler::FileCompiler::new()
        .src_prefix(&source)
        .import_path(&source)
        .compile(&[&file])
        .unwrap();
    codegen::CodeGenerationCommand::new()
        .output_directory(&expected)
        .capnp_root("::runtime")
        .field_api_values(true)
        .field_api_projections(true)
        .structured_replies(true)
        .default_parent_module(vec!["bindings".into()])
        .run(capnp::serialize::write_message_to_words(&compiled).as_slice())
        .unwrap();
    let generated = std::fs::read_to_string(output.join("service_capnp.rs")).unwrap();
    assert_eq!(
        generated,
        std::fs::read_to_string(expected.join("service_capnp.rs")).unwrap()
    );
    assert!(generated.contains("::runtime"));
    assert!(!std::fs::read(&raw).unwrap().is_empty());
    // Reusing the builder does not consume files or leave compiler state behind.
    command.run_native().unwrap();
    assert_eq!(
        generated,
        std::fs::read_to_string(output.join("service_capnp.rs")).unwrap()
    );
}

#[test]
fn native_frontend_finds_bundled_standard_imports_unless_disabled() {
    assert_eq!(
        std::fs::read_to_string(capntproto_compiler::standard_imports().join("capnp/rust.capnp"))
            .unwrap(),
        include_str!("../rust.capnp"),
    );
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("annotated.capnp");
    std::fs::write(&file, "@0x91ba7f8d25c9e603; using Rust = import \"/capnp/rust.capnp\"; struct Item $Rust.name(\"Renamed\") { value @0 :UInt32; }").unwrap();
    let mut command = CompilerCommand::new();
    command
        .src_prefix(directory.path())
        .file(&file)
        .output_path(directory.path());
    command.run_native().unwrap();
    assert!(directory.path().join("annotated_capnp.rs").exists());
    let error = command.no_standard_import().run_native().unwrap_err();
    assert!(error.to_string().contains("rust.capnp"), "{error}");
}

#[test]
fn native_frontend_reports_invalid_schema_and_missing_files() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("invalid.capnp");
    std::fs::write(
        &file,
        "@0x91ba7f8d25c9e604; struct Broken { value @0 :MissingType; }",
    )
    .unwrap();
    let error = CompilerCommand::new()
        .src_prefix(directory.path())
        .file(&file)
        .output_path(directory.path())
        .run_native()
        .unwrap_err();
    assert!(error.to_string().contains("invalid.capnp"), "{error}");
    assert!(!directory.path().join("invalid_capnp.rs").exists());
    let error = CompilerCommand::new()
        .src_prefix(directory.path())
        .file(directory.path().join("missing.capnp"))
        .output_path(directory.path())
        .run_native()
        .unwrap_err();
    assert!(error.to_string().contains("missing.capnp"), "{error}");
}

#[test]
fn native_frontend_rejects_multiple_source_prefixes() {
    let directory = tempfile::tempdir().unwrap();
    let error = CompilerCommand::new()
        .src_prefix(directory.path())
        .src_prefix(directory.path())
        .output_path(directory.path())
        .run_native()
        .unwrap_err();
    assert!(error.to_string().contains("one src_prefix"), "{error}");
}

#[test]
fn native_frontend_forwards_bounded_rpc_option() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("bounded.capnp");
    std::fs::write(
        &file,
        "@0x91ba7f8d25c9e605; interface Service { read @0 () -> (value :UInt32); }",
    )
    .unwrap();
    CompilerCommand::new()
        .src_prefix(directory.path())
        .file(&file)
        .output_path(directory.path())
        .bounded_rpc(true)
        .run_native()
        .unwrap();
    let generated = std::fs::read_to_string(directory.path().join("bounded_capnp.rs")).unwrap();
    assert!(generated.contains("::capnp::wire_capability"));
    assert!(!generated.contains("capability::Promise"));
}

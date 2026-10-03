#[cfg(any(feature = "native", feature = "storage", feature = "services"))]
fn main() {
    println!("cargo:rerun-if-env-changed=CAPNP_INCLUDE_DIR");
    let mut compiler = capntproto_compiler::FileCompiler::new();
    if let Some(directory) = std::env::var_os("CAPNP_INCLUDE_DIR") {
        println!(
            "cargo:rerun-if-changed={}",
            std::path::Path::new(&directory).display()
        );
        compiler.import_path(directory);
    }
    compiler
        .src_prefix("schemas")
        .import_path("schemas/imports");
    let schemas: &[(&str, bool)] = &[
        ("store", cfg!(feature = "storage")),
        ("persistence", cfg!(feature = "storage")),
        ("native-provisioning", cfg!(feature = "native")),
        ("native-discovery", cfg!(feature = "native")),
        ("bulk", cfg!(feature = "services")),
        ("realtime", cfg!(feature = "services")),
        ("schema-exchange", cfg!(feature = "services")),
    ];
    let files: Vec<_> = schemas
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(schema, _)| format!("schemas/{schema}.capnp"))
        .collect();
    let compiled = compiler
        .compile_with_dependencies(&files)
        .expect("compile enabled service schemas in Rust");
    for dependency in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    let bytes = capnp::serialize::write_message_to_words(&compiled.message);
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
        .run(bytes.as_slice())
        .expect("generate service bindings");
}
#[cfg(not(any(feature = "native", feature = "storage", feature = "services")))]
fn main() {}

#[cfg(any(feature = "native", feature = "storage", feature = "services"))]
fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
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
    if cfg!(feature = "services") {
        let compiled = capntproto_compiler::FileCompiler::new()
            .src_prefix("examples")
            .compile_with_dependencies(&["examples/rpc_examples.capnp"])
            .expect("compile RPC example schema");
        for dependency in compiled.dependencies {
            println!("cargo:rerun-if-changed={}", dependency.display());
        }
        capnpc::codegen::CodeGenerationCommand::new()
            .capnp_root("::capntproto::capnp")
            .output_directory(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
            .run(capnp::serialize::write_message_to_words(&compiled.message).as_slice())
            .expect("generate RPC example bindings");
    }
}
#[cfg(not(any(feature = "native", feature = "storage", feature = "services")))]
fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
}

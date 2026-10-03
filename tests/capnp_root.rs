//! Compile and execute bindings in downstream crates whose runtime is renamed
//! or re-exported. A default-path control lives in a separate crate so it cannot
//! hide stray `::capnp` references in the override cases.
use capntproto_test_support::verification::{command, root, run};
use std::fs;

#[test]
fn runtime_paths_work_with_both_frontends() {
    let schemas = root().join("tests/capnp_root");
    let includes = root().join("vendor/capnproto/c++/src");
    let files = [schemas.join("roots.capnp"), schemas.join("leaf.capnp")];
    let request = capntproto_compiler::FileCompiler::new()
        .src_prefix(&schemas)
        .import_path(&includes)
        .compile(&files)
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&request);

    for (package, dependency, paths) in [
        ("default", "capnp", vec![("default", None)]),
        (
            "overrides",
            "runtime",
            vec![
                ("renamed", Some("::runtime")),
                ("reexport", Some("crate::wire")),
                ("bootstrap", Some("crate")),
            ],
        ),
    ] {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("src")).unwrap();
        fs::write(project.path().join("Cargo.toml"), format!(
            "[package]\nname = \"runtime-path-{package}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\n{dependency} = {{ package = \"capntproto-core\", path = {:?} }}\n",
            root().join("crates/capntproto-core"),
        )).unwrap();
        let mut source = format!(
            "use {dependency} as test_runtime;\n{}\npub mod generated {{\n",
            include_str!("capnp_root/checks.rs"),
        );
        let mut checks = String::new();
        for (path_name, runtime_path) in paths {
            for facade in [false, true] {
                for frontend in ["rust", "cpp"] {
                    let mode = if facade { "facade" } else { "legacy" };
                    let name = format!("{frontend}_{path_name}_{mode}");
                    let output = project.path().join("src").join(&name);
                    let parent = vec!["generated".into(), name.clone()];
                    if frontend == "rust" {
                        let mut generator = capnpc::codegen::CodeGenerationCommand::new();
                        generator
                            .output_directory(&output)
                            .default_parent_module(parent)
                            .field_api_values(facade)
                            .field_api_projections(facade);
                        if let Some(path) = runtime_path {
                            generator.capnp_root(path);
                        }
                        generator.run(bytes.as_slice()).unwrap();
                    } else {
                        // Exercise the ordinary build-script entry point too.
                        let mut compiler = capnpc::CompilerCommand::new();
                        compiler
                            .src_prefix(&schemas)
                            .import_path(&includes)
                            .file(&files[0])
                            .file(&files[1])
                            .output_path(&output)
                            .default_parent_module(parent)
                            .field_api_values(facade)
                            .field_api_projections(facade);
                        if let Some(path) = runtime_path {
                            compiler.capnp_root(path);
                        }
                        compiler.run().unwrap();
                    }
                    source.push_str(&format!(
                        "pub mod {name} {{ pub mod roots_capnp {{ include!(\"{name}/roots_capnp.rs\"); }} pub mod leaf_capnp {{ include!(\"{name}/leaf_capnp.rs\"); }} }}\n",
                    ));
                    checks.push_str(&format!("check_legacy!(generated::{name});\n"));
                    if facade {
                        checks.push_str(&format!("check_facade!(generated::{name});\n"));
                    }
                }
            }
        }
        source.push_str(&format!(
            "}}\nfn main() -> test_runtime::Result<()> {{\n{checks}Ok(())\n}}\n"
        ));
        fs::write(project.path().join("src/main.rs"), source).unwrap();
        run(
            command("cargo")
                .args(["run", "--offline", "--manifest-path"])
                .arg(project.path().join("Cargo.toml"))
                .arg("--target-dir")
                .arg(root().join("target/capnp-root-acceptance")),
            &root().join(format!("target/verification/capnp-root/{package}.log")),
            0,
        )
        .unwrap();
    }
}

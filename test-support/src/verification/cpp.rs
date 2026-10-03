//! Build and compare the pinned C++ reference using native test orchestration.
use super::*;

pub fn build(targets: &[&str]) -> Result<PathBuf> {
    let root = root();
    let destination = root.join("target/cpp-reference");
    let _lock = lock(&root.join("target/verification/cpp.lock"))?;
    let logs = root.join("target/verification/cpp");
    let provenance: serde_json::Value = serde_json::from_slice(&fs::read(
        root.join("vendor/provenance/capnproto-sources.json"),
    )?)?;
    assert_eq!(
        provenance["revision"],
        "0de72d8d8cec6b69edaa29de51d3bd490341f9c2"
    );
    for (name, expected) in provenance["sources"]
        .as_object()
        .ok_or("missing reference sources")?
    {
        assert_eq!(
            sha256(fs::read(root.join("vendor/capnproto").join(name))?),
            expected.as_str().unwrap(),
            "reference source drift: {name}"
        );
    }
    run(
        command("cmake")
            .args(["-S", "vendor/capnproto", "-B"])
            .arg(&destination)
            .args([
                "-G",
                "Ninja",
                "-DBUILD_TESTING=ON",
                "-DWITH_OPENSSL=OFF",
                "-DWITH_ZLIB=OFF",
                "-DWITH_FIBERS=OFF",
                "-DCMAKE_BUILD_TYPE=Release",
            ]),
        &logs.join("configure.log"),
        0,
    )?;
    run(
        command("cmake")
            .arg("--build")
            .arg(&destination)
            .args(["--parallel", "4", "--target"])
            .args(targets),
        &logs.join("build.log"),
        0,
    )?;
    Ok(destination)
}
pub fn loader() -> Result<String> {
    if let Ok(path) = std::env::var("CAPNTPROTO_CPP_SCHEMA_LOADER") {
        return Ok(path);
    }
    let build = build(&["capnpc"])?;
    let output = root().join("target/verification/cpp/schema-loader");
    let _lock = lock(&root().join("target/verification/cpp-loader.lock"))?;
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/schema-loader.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&output),
        &output.with_extension("log"),
        0,
    )?;
    Ok(output.to_string_lossy().into_owned())
}
pub fn reference() -> Result<()> {
    let build = build(&[
        "capnp_tool",
        "capnpc_cpp",
        "capnp-rpc",
        "capnp-heavy-tests",
        "capnp-tests",
    ])?;
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/cpp-reference");
    for (binary, selection, count, name) in [
        (
            "capnp-tests",
            "orphan-test.c++:341-627",
            12,
            "dynamic-orphans",
        ),
        ("capnp-tests", "orphan-test.c++:628-674", 1, "allocation"),
        ("capnp-tests", "orphan-test.c++:827-919", 3, "upgrade-null"),
        (
            "capnp-tests",
            "orphan-test.c++:920-1028",
            3,
            "external-data",
        ),
        ("capnp-tests", "orphan-test.c++:1029-1586", 20, "resize"),
        ("capnp-tests", "orphan-test.c++:1587-1758", 5, "concat"),
        (
            "capnp-heavy-tests",
            "dynamic-test.c++:714-731",
            1,
            "group-copy",
        ),
        (
            "capnp-heavy-tests",
            "rpc-test.c++:2267-2700",
            6,
            "native-handoff",
        ),
        ("capnp-heavy-tests", "rpc-test.c++:2188-2266", 2, "idle"),
        (
            "capnp-heavy-tests",
            "rpc-test.c++:1800-1877",
            2,
            "disconnect-errors",
        ),
        (
            "capnp-heavy-tests",
            "rpc-test.c++:1966-1990",
            1,
            "disconnect-details",
        ),
    ] {
        let output = run(
            command(bin.join(binary)).arg(format!("--filter={selection}")),
            &logs.join(format!("{name}.log")),
            0,
        )?;
        assert!(
            output.contains(&format!("{count} test(s) passed")),
            "{output}"
        );
    }
    let tmp = tempfile::tempdir()?;
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
            "schemas/runtime-test.capnp",
            "schemas/cancellation-policy.capnp",
            "schemas/dynamic-test.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )?;
    let mut cmd = command("g++");
    cmd.args(["-std=c++23", "-DCAPNTPROTO_PINNED_RUNTIME"])
        .arg(format!("-I{}", tmp.path().display()))
        .args([
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/static-cancellation.c++",
        ]);
    for name in ["runtime-test", "cancellation-policy", "dynamic-test"] {
        cmd.arg(tmp.path().join(format!("{name}.capnp.c++")));
    }
    cmd.arg(bin.join("libcapnp-rpc.a"))
        .arg(bin.join("libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(tmp.path().join("reference"));
    run(&mut cmd, &logs.join("compile.log"), 0)?;
    run(
        &mut command(tmp.path().join("reference")),
        &logs.join("reference.log"),
        0,
    )?;
    Ok(())
}

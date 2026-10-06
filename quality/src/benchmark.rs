use crate::{Result, Runner};
use capntproto_test_support::verification::{self as v, root};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::Path};
pub const PROTOCOLS: [&str; 4] = ["native", "capnp-cpp", "grpc", "websocket"];
pub const PAYLOADS: [usize; 4] = [0, 64, 1024, 65536];
const REPETITIONS: usize = 5;
pub const MEASUREMENT_VERSION: u32 = 2;
// capnp-reference is C++; its pinned sources/compiler are recorded separately.
const RUST_BINARIES: [&str; 7] = [
    "native",
    "grpc",
    "websocket",
    "capnp_cpp",
    "driver",
    "hot_paths",
    "gungraun-runner",
];

fn audit_bundle(r: &mut Runner, bundle: &Path) -> Result<Value> {
    let inventory = r.directory.join("auditable.json");
    r.run(
        "auditable-metadata",
        v::command("cargo")
            .args([
                "run",
                "--locked",
                "-p",
                "capntproto-dev",
                "--",
                "check-auditable",
            ])
            .arg("--output")
            .arg(&inventory)
            .args(RUST_BINARIES.map(|name| bundle.join(name))),
    )?;
    Ok(serde_json::from_slice(&fs::read(inventory)?)?)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub measurement_version: u32,
    pub protocol: String,
    pub payload_bytes: usize,
    pub warmup: u64,
    pub iterations: u64,
    pub elapsed_ns: u64,
    pub latency_ns: Vec<u64>,
}
pub fn validate(
    trial: &Trial,
    protocol: &str,
    bytes: usize,
    warmup: u64,
    iterations: u64,
) -> Result<()> {
    if trial.measurement_version != MEASUREMENT_VERSION {
        return Err("unsupported benchmark measurement version; rerun with matched C++ validation and cleanup".into());
    }
    if iterations == 0
        || trial.protocol != protocol
        || trial.payload_bytes != bytes
        || trial.warmup != warmup
        || trial.iterations != iterations
        || trial.latency_ns.len() as u64 != iterations
        || trial.elapsed_ns == 0
        || trial.latency_ns.contains(&0)
        || trial
            .latency_ns
            .iter()
            .map(|&x| u128::from(x))
            .sum::<u128>()
            > u128::from(trial.elapsed_ns)
    {
        return Err("benchmark output does not match the requested workload/timing bounds".into());
    }
    Ok(())
}
pub fn percentile(samples: &mut [u64], percentile: usize) -> u64 {
    samples.sort_unstable();
    samples[(samples.len() * percentile).div_ceil(100).saturating_sub(1)]
}
pub fn matrix(trials: &[Trial]) -> Result<Vec<Value>> {
    let mut rows = vec![];
    let first = trials.first().ok_or("no benchmark trials")?;
    if trials
        .iter()
        .any(|t| t.warmup != first.warmup || t.iterations != first.iterations)
    {
        return Err("benchmark cells have different sampling budgets".into());
    }
    for bytes in PAYLOADS {
        for protocol in PROTOCOLS {
            let cell: Vec<_> = trials
                .iter()
                .filter(|t| t.protocol == protocol && t.payload_bytes == bytes)
                .collect();
            if cell.len() != REPETITIONS {
                return Err(format!(
                    "missing/duplicate benchmark repetitions for {protocol}/{bytes}"
                )
                .into());
            }
            let first = cell[0];
            for trial in &cell {
                validate(trial, protocol, bytes, first.warmup, first.iterations)?;
            }
            let mut samples: Vec<_> = cell
                .iter()
                .flat_map(|t| t.latency_ns.iter().copied())
                .collect();
            let mut medians: Vec<_> = cell
                .iter()
                .map(|t| percentile(&mut t.latency_ns.clone(), 50))
                .collect();
            let p50 = percentile(&mut samples, 50);
            let p95 = percentile(&mut samples, 95);
            let p99 = percentile(&mut samples, 99);
            medians.sort_unstable();
            let elapsed: u128 = cell.iter().map(|t| u128::from(t.elapsed_ns)).sum();
            rows.push(json!({"protocol":protocol,"payload_bytes":bytes,"repetitions":cell.len(),"iterations_per_repetition":first.iterations,"p50_ns":p50,"p95_ns":p95,"p99_ns":p99,"median_range_ns":[medians[0],medians[REPETITIONS-1]],"sequential_requests_per_second":samples.len() as f64 * 1e9 / elapsed as f64}));
        }
    }
    if trials.len() != REPETITIONS * PROTOCOLS.len() * PAYLOADS.len() {
        return Err("unexpected benchmark workload rows".into());
    }
    Ok(rows)
}
/// Compile each Cargo bench target without executing any workload.
pub fn bundle(r: &mut Runner) -> Result<()> {
    let bundle = r.directory.join("bundle");
    fs::create_dir_all(&bundle)?;
    // Failed rebuilding or missing audit data must invalidate an older bundle.
    let manifest_path = bundle.join("manifest.json");
    if manifest_path.exists() {
        fs::remove_file(&manifest_path)?;
    }
    let output = r.cargo(
        "compile-instructions",
        &[
            "bench",
            "--locked",
            "--manifest-path",
            "benchmarks/instructions/Cargo.toml",
            "--bench",
            "hot_paths",
            "--no-run",
            "--message-format=json",
        ],
    )?;
    let artifact = output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|v| {
            v["reason"] == "compiler-artifact"
                && v["target"]["name"] == "hot_paths"
                && v["executable"].is_string()
        })
        .ok_or("Cargo did not identify the instruction benchmark")?;
    fs::copy(
        artifact["executable"].as_str().unwrap(),
        bundle.join("hot_paths"),
    )?;
    let runner = r.run(
        "locate-gungraun",
        v::command("which").arg("gungraun-runner"),
    )?;
    fs::copy(runner.trim(), bundle.join("gungraun-runner"))?;
    let version = r.run(
        "gungraun-version",
        v::command(bundle.join("gungraun-runner")).arg("--version"),
    )?;
    if version.trim() != "gungraun-runner 0.20.0" {
        return Err("instruction harness requires gungraun-runner 0.20.0".into());
    }
    for name in ["native", "grpc", "websocket", "capnp_cpp"] {
        let output = r.cargo(
            &format!("compile-{name}").replace('_', "-"),
            &[
                "bench",
                "--locked",
                "--manifest-path",
                "benchmarks/rpc/Cargo.toml",
                "--bench",
                name,
                "--no-run",
                "--message-format=json",
            ],
        )?;
        let executable = output
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|value| {
                value["reason"] == "compiler-artifact"
                    && value["target"]["name"] == name
                    && value["executable"].is_string()
            })
            .ok_or("Cargo did not identify benchmark executable")?;
        fs::copy(
            executable["executable"].as_str().unwrap(),
            bundle.join(name),
        )?;
    }
    r.cargo(
        "compile-driver",
        &[
            "build",
            "--locked",
            "--release",
            "--manifest-path",
            "benchmarks/rpc/Cargo.toml",
            "--bin",
            "capntproto-rpc-bench",
        ],
    )?;
    fs::copy(
        root().join("benchmarks/rpc/target/release/capntproto-rpc-bench"),
        bundle.join("driver"),
    )?;
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "g++".into());
    let reference = v::cpp::build(&["capnp_tool", "capnpc_cpp", "capnp-rpc"])?;
    let reference_bin = reference.join("c++/src/capnp");
    let cpp_dir = r.directory.join("cpp");
    fs::create_dir_all(&cpp_dir)?;
    r.run(
        "cpp-schema",
        v::command(reference_bin.join("capnp"))
            .arg("compile")
            .arg(format!(
                "-o{}:{}",
                reference_bin.join("capnpc-c++").display(),
                cpp_dir.display()
            ))
            .args(["--src-prefix=benchmarks/rpc", "benchmarks/rpc/echo.capnp"]),
    )?;
    let cpp = cpp_dir.join("capnp-bench");
    r.run(
        "cpp-compile",
        v::command(&cxx)
            .args([
                "-std=c++23",
                "-O3",
                "-DNDEBUG",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Ivendor/capnproto/c++/src",
            ])
            .arg(format!("-I{}", cpp_dir.display()))
            .arg("benchmarks/rpc/capnp.c++")
            .arg(cpp_dir.join("echo.capnp.c++"))
            .arg(reference_bin.join("libcapnp-rpc.a"))
            .arg(reference_bin.join("libcapnp.a"))
            .arg(reference.join("c++/src/kj/libkj-async.a"))
            .arg(reference.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&cpp),
    )?;
    fs::copy(cpp, bundle.join("capnp-reference"))?;
    let mut hashes = std::collections::BTreeMap::new();
    for name in [
        "native",
        "grpc",
        "websocket",
        "capnp_cpp",
        "capnp-reference",
        "driver",
        "hot_paths",
        "gungraun-runner",
    ] {
        hashes.insert(name, v::sha256(fs::read(bundle.join(name))?));
    }
    let auditable = audit_bundle(r, &bundle)?;
    let manifest = json!({"source_id":r.evidence.source_id, "compiler":r.evidence.compiler,
        "auditable":auditable,
        "cpp_compiler":r.run("cpp-compiler", v::command(&cxx).arg("--version"))?,
        "lockfile_sha256":v::sha256(fs::read(root().join("benchmarks/rpc/Cargo.lock"))?),
        "instructions_lockfile_sha256":v::sha256(fs::read(root().join("benchmarks/instructions/Cargo.lock"))?),
        "gungraun_version":"0.20.0",
        "cpp_revision":"0de72d8d8cec6b69edaa29de51d3bd490341f9c2", "binaries":hashes});
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    r.evidence.data = manifest;
    Ok(())
}
/// Validate downloaded measurements against the exact locally built bundle.
pub fn import(r: &mut Runner, bundle: &Path) -> Result<()> {
    let manifest: Value = serde_json::from_slice(&fs::read(bundle.join("manifest.json"))?)?;
    let mut environment: Value =
        serde_json::from_slice(&fs::read(r.directory.join("environment.json"))?)?;
    if manifest["source_id"] != r.evidence.source_id || environment["manifest"] != manifest {
        return Err("remote benchmark source/build identity mismatch".into());
    }
    if manifest["auditable"] != audit_bundle(r, bundle)? {
        return Err("benchmark dependency inventories differ from the audited bundle".into());
    }
    for (name, hash) in manifest["binaries"]
        .as_object()
        .ok_or("missing binary hashes")?
    {
        if hash != &v::sha256(fs::read(bundle.join(name))?)
            || environment["binary_sha256"][name] != *hash
        {
            return Err(format!("benchmark binary mismatch: {name}").into());
        }
    }
    environment["droplet"] = serde_json::from_slice(&fs::read(r.directory.join("droplet.json"))?)?;
    let raw = fs::read(r.directory.join("trials.json"))?;
    let trials: Vec<Trial> = serde_json::from_slice(&raw)?;
    if trials
        .iter()
        .any(|t| t.iterations != 1000 || t.warmup != 10000)
    {
        return Err("remote workload differs from fixed benchmark budget".into());
    }
    let rows = matrix(&trials)?;
    let instructions_raw = fs::read(r.directory.join("instructions.jsonl"))?;
    let instructions = crate::instructions::parse(&instructions_raw)?;
    if !environment["valgrind"]
        .as_str()
        .is_some_and(|s| s.starts_with("valgrind-"))
    {
        return Err("missing instruction profiler identity".into());
    }
    r.run(
        "verify-download",
        v::command("sha256sum").arg(r.directory.join("trials.json")),
    )?;
    r.evidence.data = json!({"rows":rows,"trials_sha256":v::sha256(raw),"environment":environment,
        "instructions":instructions,"instructions_sha256":v::sha256(instructions_raw)});
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_or_mixed_measurement_contracts_cannot_qualify() {
        let mut trials = Vec::new();
        for bytes in PAYLOADS {
            for protocol in PROTOCOLS {
                for _ in 0..REPETITIONS {
                    trials.push(Trial {
                        measurement_version: MEASUREMENT_VERSION,
                        protocol: protocol.into(),
                        payload_bytes: bytes,
                        warmup: 10,
                        iterations: 2,
                        elapsed_ns: 100,
                        latency_ns: vec![40, 40],
                    });
                }
            }
        }
        assert!(matrix(&trials).is_ok());
        let mut legacy = serde_json::to_value(&trials[0]).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("measurement_version");
        assert!(serde_json::from_value::<Trial>(legacy).is_err());
        for version in [0, 1, MEASUREMENT_VERSION + 1] {
            trials[7].measurement_version = version;
            assert!(matrix(&trials).is_err());
        }
    }
    #[test]
    fn incomplete_or_impossible_benchmark_measurements_fail() {
        assert!(matrix(&[]).is_err());
        let mut t = Trial {
            measurement_version: MEASUREMENT_VERSION,
            protocol: "native".into(),
            payload_bytes: 64,
            warmup: 10,
            iterations: 2,
            elapsed_ns: 100,
            latency_ns: vec![40, 40],
        };
        assert!(validate(&t, "native", 64, 10, 2).is_ok());
        t.latency_ns[1] = 70;
        assert!(validate(&t, "native", 64, 10, 2).is_err());
        t.latency_ns[1] = 0;
        assert!(validate(&t, "native", 64, 10, 2).is_err());
        assert_eq!(percentile(&mut [3, 1, 2, 4, 5], 95), 5);
    }
}

use crate::{Result, Runner};
use capntproto_test_support::verification::{self as v, root};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::Path};
#[path = "../../benchmarks/rpc/latency-summary.rs"]
mod latency_summary;
pub const PROTOCOLS: [&str; 6] = [
    "tcp",
    "capnp-cpp",
    "grpc",
    "websocket",
    "native",
    "capnp-rust",
];
pub const PAYLOADS: [usize; 4] = [0, 64, 1024, 65536];
const REPETITIONS: usize = 5;
pub const MEASUREMENT_VERSION: u32 = 2;
// capnp-reference is C++; its pinned sources/compiler are recorded separately.
const RUST_BINARIES: [&str; 9] = [
    "tcp",
    "native",
    "grpc",
    "websocket",
    "capnp_cpp",
    "driver",
    "hot_paths",
    "gungraun-runner",
    "capnp-rust",
];

/// Keep the comparison independent from our imported/modified runtime crates.
fn upstream_dependencies(metadata: &Value) -> Result<Value> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("missing upstream packages")?;
    let root = metadata["resolve"]["root"]
        .as_str()
        .ok_or("missing upstream root")?;
    for package in packages {
        if package["id"] == root && package["name"] == "capnp-rust-bench" {
            continue;
        }
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index"
            || package["name"]
                .as_str()
                .is_none_or(|name| name.starts_with("capntproto"))
        {
            return Err("upstream RPC baseline must use unmodified crates.io dependencies".into());
        }
    }
    let mut versions = serde_json::Map::new();
    for name in ["capnp", "capnp-rpc", "capnp-futures", "capnpc"] {
        let matching: Vec<_> = packages.iter().filter(|p| p["name"] == name).collect();
        if matching.len() != 1 || !matching[0]["version"].is_string() {
            return Err(format!("missing or ambiguous upstream dependency: {name}").into());
        }
        versions.insert(name.into(), matching[0]["version"].clone());
    }
    Ok(json!({"registry":"crates.io", "versions":versions}))
}

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
    pub security: Value,
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
    let security = &trial.security;
    let encrypted = if protocol == "native" {
        security["transport"] == "native" && security["authentication"] == "pinned-mutual-identity"
    } else {
        security["transport"] == "tls-tcp"
            && security["tls"] == "1.3"
            && security["authentication"] == "server-certificate"
            && security["early_data"] == false
            && security["server_name"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            && security["alpn"]
                == if protocol == "grpc" {
                    json!("h2")
                } else {
                    Value::Null
                }
    };
    if !encrypted || security["version"] != 1 || security["setup_timed"] != false {
        return Err("benchmark requires matching encrypted transport metadata; plaintext archives cannot qualify".into());
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
    if trials.len() != PROTOCOLS.len() * PAYLOADS.len() * REPETITIONS
        || trials.iter().any(|t| {
            !PROTOCOLS.contains(&t.protocol.as_str()) || !PAYLOADS.contains(&t.payload_bytes)
        })
    {
        return Err(
            "benchmark must contain exactly the declared protocols, payloads and repetitions"
                .into(),
        );
    }
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
                if trial.security != first.security {
                    return Err("mixed security contracts in benchmark cell".into());
                }
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
            let elapsed = cell.iter().map(|t| t.elapsed_ns).min().unwrap();
            let sample_trials: Vec<Vec<u64>> =
                cell.iter().map(|trial| trial.latency_ns.clone()).collect();
            let mut row = json!({"protocol":protocol,"security":first.security,"payload_bytes":bytes,"repetitions":cell.len(),"iterations_per_repetition":first.iterations,"p50_ns":p50,"p95_ns":p95,"p99_ns":p99,"median_range_ns":[medians[0],medians[REPETITIONS-1]],"throughput_estimator":"fastest_repetition","best_elapsed_ns":elapsed,"sequential_requests_per_second":first.iterations as f64 * 1e9 / elapsed as f64});
            row.as_object_mut().unwrap().extend(
                latency_summary::summarize(&sample_trials)
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Acceptance compares matched TLS capability RPC, at every payload size.
/// Keep raw distributions and tails in the report; the gate uses the minimum valid latency.
pub fn tcp_acceptance(rows: &[Value]) -> Result<()> {
    tcp_acceptance_for_driver(rows, "legacy")
}

pub fn tcp_acceptance_for_driver(rows: &[Value], driver: &str) -> Result<()> {
    let limit = match driver {
        "legacy" => 115,
        "portable" => 150,
        _ => return Err("missing or unknown measured TCP driver".into()),
    };
    for bytes in PAYLOADS {
        let latency = |protocol: &str| -> Result<u128> {
            let mut cells = rows
                .iter()
                .filter(|row| row["protocol"] == protocol && row["payload_bytes"] == bytes);
            let value = cells
                .next()
                .and_then(|row| row["min_ns"].as_u64())
                .filter(|&n| n > 0)
                .ok_or("missing TCP acceptance measurement")?;
            if cells.next().is_some() {
                return Err("duplicate TCP acceptance measurement".into());
            }
            Ok(u128::from(value))
        };
        let tcp = latency("tcp")?;
        let cpp = latency("capnp-cpp")?;
        if (driver == "legacy" || bytes <= 1024) && tcp * 100 > cpp * limit {
            return Err(format!(
                "{driver} TCP/C++ minimum exceeds {:.2}x at {bytes} bytes: {:.4}x",
                limit as f64 / 100.0,
                tcp as f64 / cpp as f64
            )
            .into());
        }
    }
    Ok(())
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
    for name in ["tcp", "native", "grpc", "websocket", "capnp_cpp"] {
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
    let metadata = r.run_stdout(
        "upstream-rpc-dependencies",
        v::command("cargo").args([
            "metadata",
            "--locked",
            "--format-version=1",
            "--manifest-path",
            "benchmarks/rpc-upstream/Cargo.toml",
        ]),
    )?;
    let upstream = upstream_dependencies(&serde_json::from_str(&metadata)?)?;
    let output = r.cargo(
        "compile-upstream-rpc",
        &[
            "build",
            "--locked",
            "--release",
            "--manifest-path",
            "benchmarks/rpc-upstream/Cargo.toml",
            "--bin",
            "capnp-rust",
            "--message-format=json",
        ],
    )?;
    let executable = output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|v| {
            v["reason"] == "compiler-artifact"
                && v["target"]["name"] == "capnp-rust"
                && v["executable"].is_string()
        })
        .ok_or("Cargo did not identify the upstream RPC benchmark")?;
    fs::copy(
        executable["executable"].as_str().unwrap(),
        bundle.join("capnp-rust"),
    )?;
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "g++".into());
    let reference = v::cpp::build(&["capnp_tool", "capnpc_cpp", "capnp-rpc", "kj-tls"])?;
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
            .arg("benchmarks/rpc/workflows.c++")
            .arg("benchmarks/rpc/topology.c++")
            .arg(cpp_dir.join("echo.capnp.c++"))
            .arg(reference_bin.join("libcapnp-rpc.a"))
            .arg(reference_bin.join("libcapnp.a"))
            .arg(reference.join("c++/src/kj/libkj-tls.a"))
            .arg(reference.join("c++/src/kj/libkj-async.a"))
            .arg(reference.join("c++/src/kj/libkj.a"))
            .args(["-lssl", "-lcrypto", "-pthread", "-o"])
            .arg(&cpp),
    )?;
    fs::copy(cpp, bundle.join("capnp-reference"))?;
    // All transported peers share one short-lived benchmark identity; no plaintext fallback.
    let certificates = bundle.join("certificates");
    if certificates.exists() {
        fs::remove_dir_all(&certificates)?;
    }
    r.run(
        "benchmark-certificates",
        v::command("python3")
            .arg("benchmarks/rpc/generate-certificates.py")
            .arg(&certificates),
    )?;
    fs::copy(
        root().join("benchmarks/rpc/generate-certificates.py"),
        bundle.join("generate-certificates.py"),
    )?;
    for script in [
        "secure-runner.py",
        "secure-profile.py",
        "topology-runner.py",
        "ciphertext-counts.py",
        "latency_stats.py",
        "workflows.json",
        "topologies.json",
    ] {
        fs::copy(
            root().join("benchmarks/rpc").join(script),
            bundle.join(script),
        )?;
    }
    let mut hashes = std::collections::BTreeMap::new();
    for name in [
        "tcp",
        "native",
        "grpc",
        "websocket",
        "capnp_cpp",
        "capnp-reference",
        "capnp-rust",
        "driver",
        "hot_paths",
        "gungraun-runner",
    ] {
        hashes.insert(name, v::sha256(fs::read(bundle.join(name))?));
    }
    let auditable = audit_bundle(r, &bundle)?;
    let manifest = json!({"source_id":r.evidence.source_id, "compiler":r.evidence.compiler,
        "auditable":auditable,
        "security_fixture":serde_json::from_slice::<Value>(&fs::read(certificates.join("manifest.json"))?)?,
        "cpp_compiler":r.run("cpp-compiler", v::command(&cxx).arg("--version"))?,
        "lockfile_sha256":v::sha256(fs::read(root().join("benchmarks/rpc/Cargo.lock"))?),
        "upstream_rpc":upstream,
        "upstream_lockfile_sha256":v::sha256(fs::read(root().join("benchmarks/rpc-upstream/Cargo.lock"))?),
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
pub(crate) fn test_security(protocol: &str) -> Value {
    if protocol == "native" {
        json!({"version":1,"transport":"native","authentication":"pinned-mutual-identity","setup_timed":false})
    } else {
        json!({"version":1,"transport":"tls-tcp","tls":"1.3","authentication":"server-certificate","server_name":"localhost","early_data":false,"setup_timed":false,"alpn":if protocol == "grpc" { json!("h2") } else { Value::Null }})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upstream_baseline_rejects_owned_path_and_git_dependencies() {
        let mut metadata = json!({"resolve":{"root":"bench"},"packages":[
            {"id":"bench","name":"capnp-rust-bench","source":null}
        ]});
        for name in ["capnp", "capnp-rpc", "capnp-futures", "capnpc"] {
            metadata["packages"].as_array_mut().unwrap().push(json!({
                "id":name, "name":name, "version":"0.27.0",
                "source":"registry+https://github.com/rust-lang/crates.io-index"
            }));
        }
        assert_eq!(
            upstream_dependencies(&metadata).unwrap()["versions"]["capnp-rpc"],
            "0.27.0"
        );
        for source in [Value::Null, json!("git+https://example.com/fork")] {
            let mut modified = metadata.clone();
            modified["packages"][2]["source"] = source;
            assert!(upstream_dependencies(&modified).is_err());
        }
        let mut owned = metadata.clone();
        owned["packages"][2]["name"] = json!("capntproto-rpc");
        assert!(upstream_dependencies(&owned).is_err());
        let mut missing = metadata.clone();
        missing["packages"].as_array_mut().unwrap().pop();
        assert!(upstream_dependencies(&missing).is_err());
        let duplicate = metadata["packages"][2].clone();
        metadata["packages"].as_array_mut().unwrap().push(duplicate);
        assert!(upstream_dependencies(&metadata).is_err());
    }
    #[test]
    fn tcp_target_requires_every_payload_and_accepts_exact_boundary() {
        let mut rows = Vec::new();
        for bytes in PAYLOADS {
            rows.push(json!({"protocol":"tcp","payload_bytes":bytes,"min_ns":115}));
            rows.push(json!({"protocol":"capnp-cpp","payload_bytes":bytes,"min_ns":100}));
        }
        assert!(tcp_acceptance(&rows).is_ok());
        rows[0]["min_ns"] = json!(116);
        assert!(tcp_acceptance(&rows).is_err());
        rows[0]["min_ns"] = json!(115);
        rows.push(rows[0].clone());
        assert!(tcp_acceptance(&rows).is_err());
        rows.pop();
        rows.pop();
        assert!(tcp_acceptance(&rows).is_err());
    }
    #[test]
    fn portable_target_limits_small_calls_and_retains_large_diagnostics() {
        let mut rows = Vec::new();
        for bytes in PAYLOADS {
            rows.push(json!({"protocol":"tcp","payload_bytes":bytes,"min_ns":if bytes <= 1024 { 150 } else { 900 }}));
            rows.push(json!({"protocol":"capnp-cpp","payload_bytes":bytes,"min_ns":100}));
        }
        assert!(tcp_acceptance_for_driver(&rows, "portable").is_ok());
        assert!(tcp_acceptance_for_driver(&rows, "legacy").is_err());
        assert!(tcp_acceptance_for_driver(&rows, "unknown").is_err());
        rows[0]["min_ns"] = json!(151);
        assert!(tcp_acceptance_for_driver(&rows, "portable").is_err());
        rows[0]["min_ns"] = json!(150);
        rows.pop();
        assert!(tcp_acceptance_for_driver(&rows, "portable").is_err());
    }
    #[test]
    fn legacy_or_mixed_measurement_contracts_cannot_qualify() {
        let mut trials = Vec::new();
        for bytes in PAYLOADS {
            for protocol in PROTOCOLS {
                for _ in 0..REPETITIONS {
                    trials.push(Trial {
                        measurement_version: MEASUREMENT_VERSION,
                        security: test_security(protocol),
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
        assert_eq!(matrix(&trials).unwrap().len(), 24);
        let summary = matrix(&trials).unwrap();
        assert_eq!(summary[0]["mean_ns"], 40.0);
        assert_eq!(summary[0]["repetition_mean_ns"], json!(vec![40.0; 5]));
        assert_eq!(summary[0]["repetition_mean_stddev_ns"], 0.0);
        let without_upstream: Vec<_> = trials
            .iter()
            .filter(|t| t.protocol != "capnp-rust")
            .cloned()
            .collect();
        assert!(matrix(&without_upstream).is_err());
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
            security: test_security("native"),
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

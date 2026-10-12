//! Runs precompiled clients and servers; never invokes a compiler on the benchmark host.
use crate::Result;
mod placement;
pub mod workflows;
use placement::Placement;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
const PROTOCOLS: [&str; 5] = ["tcp", "capnp-cpp", "grpc", "websocket", "native"];
const PAYLOADS: [usize; 4] = [0, 64, 1024, 65536];

pub fn clock_reads() -> Value {
    const READS: u32 = 100_000;
    for _ in 0..10_000 {
        std::hint::black_box(Instant::now());
    }
    let samples: Vec<_> = (0..5)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..READS {
                std::hint::black_box(Instant::now());
            }
            start.elapsed().as_nanos() as f64 / f64::from(READS)
        })
        .collect();
    json!({"clock":"std::time::Instant", "reads_per_sample":READS,
        "ns_per_read_including_loop":samples})
}

fn clock_diagnostics(placement: &Placement) -> Result<Value> {
    let exe = std::env::current_exe()?;
    let mut diagnostics = Vec::new();
    for cpu in [placement.server_cpu, placement.client_cpu] {
        let result = Placement::command(&exe, Some(cpu))
            .arg("clock-reads")
            .output()?;
        if !result.status.success() {
            return Err("clock diagnostic failed".into());
        }
        let mut sample: Value = serde_json::from_slice(&result.stdout)?;
        sample["cpu"] = json!(cpu);
        diagnostics.push(sample);
    }
    Ok(json!(diagnostics))
}
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn trial(exe: &Path, protocol: &str, bytes: usize, placement: Option<&Placement>) -> Result<Value> {
    let mut server = Server(
        Placement::command(exe, placement.map(|p| p.server_cpu))
            .arg("server")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let stdout = server.0.stdout.take().ok_or("missing readiness pipe")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    let ready: Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(15))??)?;
    let address = ready["address"].as_str().ok_or("missing server address")?;
    let parsed: std::net::SocketAddr = address.parse()?;
    if !parsed.ip().is_loopback() || parsed.port() == 0 {
        return Err("server must use loopback".into());
    }
    let mut client = Placement::command(exe, placement.map(|p| p.client_cpu));
    client.args(["measure", address]);
    if protocol != "capnp-cpp" {
        client.arg(serde_json::to_string(&ready["public"])?);
    }
    // Every implementation has a ten-second per-call deadline.
    let output = client
        .args([bytes.to_string(), "10000".into(), "1000".into()])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "{protocol} client failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let value: Value = serde_json::from_slice(&output.stdout)?;
    validate_trial(&value, protocol, bytes)?;
    Ok(value)
}

fn validate_trial(value: &Value, protocol: &str, bytes: usize) -> Result<()> {
    if value["measurement_version"] != crate::MEASUREMENT_VERSION
        || value["protocol"] != protocol
        || value["payload_bytes"] != bytes
        || value["warmup"] != 10000
        || value["iterations"] != 1000
    {
        return Err("unexpected trial identity".into());
    }
    let samples = value["latency_ns"]
        .as_array()
        .ok_or("missing latency samples")?;
    let mut total = 0u128;
    for sample in samples {
        total += u128::from(
            sample
                .as_u64()
                .filter(|&n| n > 0)
                .ok_or("invalid latency sample")?,
        );
    }
    if samples.len() != 1000
        || total > u128::from(value["elapsed_ns"].as_u64().ok_or("missing elapsed time")?)
    {
        return Err("invalid trial timing bounds".into());
    }
    Ok(())
}

/// A focused release comparison, using the same workload as the full bundle.
/// Unlike an individual bench, this alternates peers on fixed physical cores.
pub fn compare_tcp(tcp: &Path, cpp: &Path, destination: &Path) -> Result<()> {
    if cfg!(debug_assertions) {
        return Err("TCP comparisons require a release driver".into());
    }
    fs::create_dir(destination)?;
    let placement = Placement::discover()?;
    let digest = |path: &Path| -> Result<String> {
        let value = output(
            "sha256sum",
            &[path.to_str().ok_or("invalid executable path")?],
        )?;
        Ok(value
            .split_whitespace()
            .next()
            .ok_or("missing executable digest")?
            .into())
    };
    fs::write(
        destination.join("environment.json"),
        serde_json::to_vec_pretty(&json!({
            "measurement_version": crate::MEASUREMENT_VERSION,
            "tcp_binary": tcp, "cpp_binary": cpp,
            "tcp_driver": crate::tcp_driver()?,
            "tcp_sha256": digest(tcp)?, "cpp_sha256": digest(cpp)?,
            "driver_sha256": digest(&std::env::current_exe()?)?,
            "cpu_placement": placement, "cpu": output("lscpu", &[])?,
            "clock_reads": clock_diagnostics(&placement)?,
            "scope": "Matched plaintext TCP, separate processes and physical cores, 10000 warmups and 1000 retained calls per trial, five alternating repetitions. Portable gate: pooled p50 <= 1.5x for 0/64/1024 bytes; 65536 bytes and tails reported separately. Legacy gate: <= 1.15x at every payload."
        }))?,
    )?;
    let mut trials = Vec::new();
    for repetition in 0..5 {
        for bytes in PAYLOADS {
            let peers = if repetition % 2 == 0 {
                [(tcp, "tcp"), (cpp, "capnp-cpp")]
            } else {
                [(cpp, "capnp-cpp"), (tcp, "tcp")]
            };
            for (exe, protocol) in peers {
                eprintln!("{protocol}: {bytes} bytes, repetition {}", repetition + 1);
                trials.push(trial(exe, protocol, bytes, Some(&placement))?);
                fs::write(
                    destination.join("trials.json"),
                    serde_json::to_vec_pretty(&trials)?,
                )?;
            }
        }
    }
    let report = tcp_summary_for_driver(&trials, crate::tcp_driver()?)?;
    fs::write(
        destination.join("summary.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report["passed"] != true {
        return Err("TCP/C++ p50 target not met; see summary.json and retained trials".into());
    }
    Ok(())
}

#[cfg(test)]
fn tcp_summary(trials: &[Value]) -> Result<Value> {
    tcp_summary_for_driver(trials, "legacy")
}
fn tcp_summary_for_driver(trials: &[Value], driver: &str) -> Result<Value> {
    let limit = match driver {
        "legacy" => 115u128,
        "portable" => 150,
        _ => return Err("unknown TCP driver".into()),
    };
    if trials.len() != PAYLOADS.len() * 2 * 5 {
        return Err("TCP comparison requires five repetitions of every peer/payload pair".into());
    }
    let mut rows = Vec::new();
    let mut passed = true;
    for bytes in PAYLOADS {
        let percentiles = |protocol: &str| -> Result<[u64; 3]> {
            let cell: Vec<_> = trials
                .iter()
                .filter(|t| t["protocol"] == protocol && t["payload_bytes"] == bytes)
                .collect();
            if cell.len() != 5 {
                return Err("missing or duplicate TCP comparison cells".into());
            }
            for trial in &cell {
                validate_trial(trial, protocol, bytes)?;
            }
            let mut samples: Vec<u64> = cell
                .into_iter()
                .flat_map(|t| {
                    t["latency_ns"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_u64().unwrap())
                })
                .collect();
            samples.sort_unstable();
            Ok([50, 95, 99].map(|p| samples[(samples.len() * p).div_ceil(100) - 1]))
        };
        let tcp = percentiles("tcp")?;
        let cpp = percentiles("capnp-cpp")?;
        let within = u128::from(tcp[0]) * 100 <= u128::from(cpp[0]) * limit;
        let required = driver == "legacy" || bytes <= 1024;
        passed &= !required || within;
        rows.push(
            json!({"payload_bytes":bytes,"tcp_p50_p95_p99_ns":tcp,"cpp_p50_p95_p99_ns":cpp,
            "p50_ratio":tcp[0] as f64 / cpp[0] as f64,"passed":within,"required":required}),
        );
    }
    Ok(json!({"passed":passed,"tcp_driver":driver,"limit":limit as f64 / 100.0,"rows":rows}))
}
fn target(directory: &Path, protocol: &str) -> PathBuf {
    directory.join(if protocol == "capnp-cpp" {
        "capnp_cpp"
    } else {
        protocol
    })
}
pub fn individual(protocol: &str) -> Result<()> {
    let exe = std::env::current_exe()?;
    for bytes in PAYLOADS {
        for _ in 0..5 {
            println!("{}", trial(&exe, protocol, bytes, None)?);
        }
    }
    Ok(())
}
pub fn cpp() -> Result<()> {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|a| a != "--bench")
        .collect();
    if args.is_empty() {
        return individual("capnp-cpp");
    }
    let reference = std::env::var_os("CAPNTPROTO_CPP_BENCH")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?.with_file_name("capnp-reference"));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(Command::new(reference).args(args).exec().into())
    }
    #[cfg(not(unix))]
    {
        let status = Command::new(reference).args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err("C++ benchmark failed".into())
        }
    }
}
fn output(program: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(program).args(args).output()?;
    if !result.status.success() {
        return Err(format!("{program} failed").into());
    }
    Ok(String::from_utf8(result.stdout)?)
}
pub fn compare(directory: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    let placement = Placement::discover()?;
    let manifest: Value = serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
    let mut hashes = serde_json::Map::new();
    for (name, expected) in manifest["binaries"].as_object().ok_or("missing binaries")? {
        if ![
            "tcp",
            "native",
            "grpc",
            "websocket",
            "capnp_cpp",
            "capnp-reference",
            "driver",
            "hot_paths",
            "gungraun-runner",
        ]
        .contains(&name.as_str())
        {
            return Err("unexpected artifact".into());
        }
        let sum = output(
            "sha256sum",
            &[directory.join(name).to_str().ok_or("invalid binary path")?],
        )?;
        let hash = sum.split_whitespace().next().ok_or("missing digest")?;
        if expected != hash {
            return Err("uploaded binary digest mismatch".into());
        }
        hashes.insert(name.clone(), json!(hash));
    }
    let environment = json!({"measurement_version":crate::MEASUREMENT_VERSION,"manifest":manifest, "binary_sha256":hashes,
        "tcp_driver":crate::tcp_driver()?,
        "valgrind":output("valgrind", &["--version"])? ,
        "uname":output("uname", &["-a"])? , "cpu":output("lscpu", &[])?,
        "governor":fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").ok(),
        "clocksource":fs::read_to_string("/sys/devices/system/clocksource/clocksource0/current_clocksource").ok(),
        "available_clocksources":fs::read_to_string("/sys/devices/system/clocksource/clocksource0/available_clocksource").ok(),
        "clock_reads":clock_diagnostics(&placement)?,
        "cpu_placement":placement,
        "scope":"Linux IPv4 loopback; sequential echo with bulk payload validation, response cleanup and ten-second per-call deadlines inside each timed roundtrip; fixed separate physical cores for server/client across all protocols; 10000 warmups/1000 samples; 5 repetitions; Native QUIC v1 / TLS 1.3 with pinned peer authentication, other protocols plaintext"});
    fs::write(
        destination.join("environment.json"),
        serde_json::to_vec_pretty(&environment)?,
    )?;
    let mut trials = vec![];
    for repetition in 0..5 {
        for bytes in PAYLOADS {
            for index in 0..PROTOCOLS.len() {
                let protocol = PROTOCOLS[(index + repetition) % PROTOCOLS.len()];
                eprintln!("{protocol}: {bytes} bytes, repetition {}", repetition + 1);
                trials.push(trial(
                    &target(directory, protocol),
                    protocol,
                    bytes,
                    Some(&placement),
                )?);
                fs::write(
                    destination.join("trials.json"),
                    serde_json::to_vec_pretty(&trials)?,
                )?;
            }
        }
    }
    // Explicit directories suppress cargo metadata on the runtime-only host.
    let instructions = Command::new(directory.join("hot_paths"))
        .args(["--bench", "--output-format=json"])
        .arg(format!("--workspace-root={}", directory.display()))
        .arg(format!("--home={}", destination.join("gungraun").display()))
        .env(
            "PATH",
            format!("{}:{}", directory.display(), std::env::var("PATH")?),
        )
        .stdout(fs::File::create(destination.join("instructions.jsonl"))?)
        .stderr(Stdio::inherit())
        .status()?;
    if !instructions.success() {
        return Err("instruction benchmarks failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trials(tcp: u64, cpp: u64) -> Vec<Value> {
        let mut trials = Vec::new();
        for bytes in PAYLOADS {
            for (protocol, latency) in [("tcp", tcp), ("capnp-cpp", cpp)] {
                for _ in 0..5 {
                    trials.push(json!({
                        "measurement_version": crate::MEASUREMENT_VERSION,
                        "protocol": protocol, "payload_bytes": bytes,
                        "warmup": 10000, "iterations": 1000,
                        "elapsed_ns": latency * 1000, "latency_ns": vec![latency; 1000],
                    }));
                }
            }
        }
        trials
    }

    #[test]
    fn portable_target_gates_all_small_calls_and_reports_large_calls() {
        let mut cases = trials(150, 100);
        for trial in cases
            .iter_mut()
            .filter(|t| t["protocol"] == "tcp" && t["payload_bytes"] == 65536)
        {
            trial["latency_ns"] = json!(vec![900; 1000]);
            trial["elapsed_ns"] = json!(900000);
        }
        let report = tcp_summary_for_driver(&cases, "portable").unwrap();
        assert_eq!(report["passed"], true);
        assert_eq!(report["rows"][3]["required"], false);
        assert_eq!(report["rows"][3]["p50_ratio"], 9.0);
        assert_eq!(
            tcp_summary_for_driver(&trials(151, 100), "portable").unwrap()["passed"],
            false
        );
        assert!(tcp_summary_for_driver(&cases, "unknown").is_err());
        cases.pop();
        assert!(tcp_summary_for_driver(&cases, "portable").is_err());
    }

    #[test]
    fn tcp_comparison_checks_the_exact_limit_and_retains_slow_samples() {
        let mut trials = trials(115, 100);
        for trial in trials.iter_mut().filter(|t| t["protocol"] == "tcp") {
            for sample in &mut trial["latency_ns"].as_array_mut().unwrap()[900..] {
                *sample = json!(10000);
            }
            trial["elapsed_ns"] = json!(115 * 900 + 10000 * 100);
        }
        let report = tcp_summary(&trials).unwrap();
        assert_eq!(report["passed"], true);
        assert_eq!(report["rows"].as_array().unwrap().len(), PAYLOADS.len());
        for row in report["rows"].as_array().unwrap() {
            assert_eq!(row["tcp_p50_p95_p99_ns"], json!([115, 10000, 10000]));
            assert_eq!(row["cpp_p50_p95_p99_ns"], json!([100, 100, 100]));
        }
        assert_eq!(
            tcp_summary(&self::trials(116, 100)).unwrap()["passed"],
            false
        );
        // One failing payload must fail the entire comparison.
        for trial in trials
            .iter_mut()
            .filter(|t| t["protocol"] == "tcp" && t["payload_bytes"] == 65536)
        {
            trial["latency_ns"] = json!(vec![116; 1000]);
            trial["elapsed_ns"] = json!(116000);
        }
        assert_eq!(tcp_summary(&trials).unwrap()["passed"], false);
    }

    #[test]
    fn tcp_comparison_rejects_incomplete_mixed_or_invalid_trials() {
        let valid = trials(100, 100);
        assert!(tcp_summary(&valid[..valid.len() - 1]).is_err());
        let mut extra = valid.clone();
        extra.push(extra[0].clone());
        assert!(tcp_summary(&extra).is_err());
        let mut duplicate = valid.clone();
        duplicate[5] = duplicate[0].clone();
        assert!(tcp_summary(&duplicate).is_err());
        for (key, value) in [
            ("measurement_version", json!(0)),
            ("protocol", json!("native")),
            ("payload_bytes", json!(17)),
            ("warmup", json!(100)),
            ("iterations", json!(0)),
            ("elapsed_ns", json!(99999)),
            ("elapsed_ns", Value::Null),
            ("latency_ns", json!([])),
            ("latency_ns", Value::Null),
            ("latency_ns", json!(vec![0; 1000])),
            ("latency_ns", json!(vec![-1; 1000])),
        ] {
            let mut invalid = valid.clone();
            invalid[0][key] = value;
            assert!(tcp_summary(&invalid).is_err(), "accepted invalid {key}");
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_driver_cannot_publish_a_performance_comparison() {
        let error = compare_tcp(
            Path::new("missing-tcp"),
            Path::new("missing-cpp"),
            Path::new("."),
        )
        .unwrap_err();
        assert!(error.to_string().contains("release driver"));
    }

    #[test]
    fn clock_probe_records_all_samples_without_a_performance_threshold() {
        let report = super::clock_reads();
        assert_eq!(report["clock"], "std::time::Instant");
        assert_eq!(report["reads_per_sample"], 100_000);
        let samples = report["ns_per_read_including_loop"].as_array().unwrap();
        assert_eq!(samples.len(), 5);
        for sample in samples {
            let cost = sample.as_f64().unwrap();
            assert!(cost.is_finite() && cost > 0.0);
        }
    }
}

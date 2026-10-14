//! Reproducible whole-process instruction measurements. Never adjusts the workload
//! or removes checks to meet a requested ceiling. Optimized builds remain opt-in.
use anyhow::{bail, ensure, Context, Result};
use clap::Args as ClapArgs;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

const TRAINING_SIZES: [u32; 10] = [0, 1, 7, 8, 31, 64, 256, 1024, 4096, 65536];

#[derive(ClapArgs)]
pub struct Args {
    /// New directory for binaries, build logs, training data and raw profiles.
    #[arg(long, default_value = "target/quality/rpc-instructions")]
    pub output: PathBuf,
    /// Train both TCP peers on ten payload sizes before the measured build.
    #[arg(long)]
    pub pgo: bool,
    /// Optimize for this CPU. The resulting binary is not generally portable.
    #[arg(long)]
    pub native_cpu: bool,
    /// Link BoringSSL's C++ runtime statically (native GNU/Linux only).
    #[arg(long)]
    pub static_cpp: bool,
    /// Train and measure the legacy Rust driver as a matched control.
    #[arg(long)]
    pub legacy: bool,
    /// Fail after writing the report if client instructions exceed this ceiling.
    #[arg(long)]
    pub maximum: Option<u64>,
}

pub fn run(args: Args) -> Result<()> {
    ensure!(
        std::env::var_os("RUSTFLAGS").is_none()
            && std::env::var_os("CARGO_ENCODED_RUSTFLAGS").is_none(),
        "unset RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS so the recorded flags are complete"
    );
    ensure!(!args.output.exists(), "output directory already exists");
    fs::create_dir_all(&args.output)?;
    let output = args.output.canonicalize()?;
    let compiler = crate::process::text(Command::new("rustc").arg("-vV"))?;
    let valgrind = crate::process::text(Command::new("valgrind").arg("--version"))?;
    let mut flags = Vec::new();
    if args.native_cpu {
        flags.push("-Ctarget-cpu=native".to_owned());
    }
    if args.static_cpp {
        ensure!(
            cfg!(all(target_os = "linux", target_env = "gnu")),
            "static C++ runtime discovery requires native GNU/Linux"
        );
        let archive = PathBuf::from(crate::process::text(
            Command::new("c++").arg("-print-file-name=libstdc++.a"),
        )?);
        ensure!(
            archive.is_absolute() && archive.is_file(),
            "C++ compiler did not locate libstdc++.a"
        );
        flags.push(format!("-Lnative={}", archive.parent().unwrap().display()));
    }
    let mut training = Vec::new();
    if args.pgo {
        // Build-time instrumentation writes elsewhere; only the actual TCP peers
        // contribute to the merged profile, never build scripts or proc macros.
        let host = output.join("build-profiles");
        fs::create_dir(&host)?;
        let mut generate = flags.clone();
        generate.push(format!("-Cprofile-generate={}", host.display()));
        let instrumented = build(&output, "instrumented", &generate, args.static_cpp)?;
        let profiles = output.join("training");
        fs::create_dir(&profiles)?;
        let profile_pattern = profiles.join("%m-%p.profraw");
        for size in TRAINING_SIZES {
            eprintln!("Training TCP peers: {size} bytes");
            training.push(measure(
                &instrumented,
                &profiles,
                &format!("size-{size}"),
                size,
                Some(&profile_pattern),
                false,
                args.legacy,
            )?);
        }
        let sysroot = crate::process::text(Command::new("rustc").args(["--print", "sysroot"]))?;
        let host = compiler
            .lines()
            .find_map(|s| s.strip_prefix("host: "))
            .context("rustc host triple")?;
        let profdata = Path::new(&sysroot)
            .join("lib/rustlib")
            .join(host)
            .join("bin/llvm-profdata");
        ensure!(
            profdata.is_file(),
            "install llvm-tools-preview for the pinned toolchain"
        );
        let merged = output.join("training.profdata");
        let mut merge = Command::new(profdata);
        merge.arg("merge").arg("-o").arg(&merged);
        let mut raw = fs::read_dir(&profiles)?.collect::<std::io::Result<Vec<_>>>()?;
        raw.sort_by_key(|e| e.file_name());
        let raw: Vec<_> = raw
            .into_iter()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "profraw"))
            .collect();
        ensure!(!raw.is_empty(), "training produced no profiles");
        merge.args(raw);
        crate::process::checked(&mut merge, 120)?;
        flags.push(format!("-Cprofile-use={}", merged.display()));
    }
    let executable = build(&output, "measured", &flags, args.static_cpp)?;
    let measurement = measure(&executable, &output, "measured", 0, None, true, args.legacy)?;
    let count = instruction_count(&fs::read_to_string(
        output.join("measured-client.callgrind"),
    )?)?;
    let server_count = instruction_count(&fs::read_to_string(
        output.join("measured-server.callgrind"),
    )?)?;
    let report = json!({
        "client_instructions": count, "maximum": args.maximum,
        "server_instructions": server_count,
        "passes": args.maximum.is_none_or(|limit| count <= limit),
        "driver": if args.legacy { "legacy" } else { "portable" }, "protocol": "plaintext TCP",
        "measurement": "complete Callgrind Ir process, including startup/bootstrap/cleanup",
        "payload_bytes": 0, "warmups": 100, "retained_calls": 1000,
        "compiler": compiler, "rustflags": flags, "static_cpp": args.static_cpp,
        "valgrind": valgrind,
        "cpu": fs::read_to_string("/proc/cpuinfo").ok().and_then(|text| text.lines().find(|line| line.starts_with("model name")).map(str::to_owned)),
        "pgo": args.pgo, "native_cpu": args.native_cpu,
        "lto": "fat", "codegen_units": 1, "overflow_checks": true,
        "executable_sha256": crate::file_hash(&executable)?,
        "measurement_output": measurement, "training_outputs": training,
        "git_head": crate::process::text(Command::new("git").args(["rev-parse", "HEAD"]))?,
        "git_status": crate::process::text(Command::new("git").args(["status", "--short"]))?,
    });
    crate::write_json(output.join("report.json"), &report)?;
    println!(
        "Client instructions: {count}; server instructions: {server_count}; report: {}",
        output.join("report.json").display()
    );
    ensure!(
        report["passes"] == true,
        "client instruction ceiling exceeded"
    );
    Ok(())
}

fn build(output: &Path, label: &str, flags: &[String], static_cpp: bool) -> Result<PathBuf> {
    eprintln!("Building {label} release TCP benchmark");
    let mut cmd = Command::new("cargo");
    cmd.args([
        "bench",
        "--locked",
        "--manifest-path",
        "benchmarks/rpc/Cargo.toml",
        "--bench",
        "tcp",
        "--no-run",
        "--message-format=json",
    ])
    .env("CARGO_PROFILE_RELEASE_LTO", "fat")
    .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
    .env("CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS", "true")
    .env("CARGO_ENCODED_RUSTFLAGS", flags.join("\u{1f}"));
    if static_cpp {
        cmd.env("BORING_BSSL_RUST_CPPLIB", "static=stdc++");
    } else {
        cmd.env_remove("BORING_BSSL_RUST_CPPLIB");
    }
    let result = crate::process::run(&mut cmd, 1800, None, &Default::default())?;
    crate::write(output.join(format!("{label}-build.jsonl")), &result.stdout)?;
    crate::write(output.join(format!("{label}-build.log")), &result.stderr)?;
    ensure!(result.code == 0, "{label} build failed; see build log");
    let source = String::from_utf8(result.stdout)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|v| {
            (v["reason"] == "compiler-artifact" && v["target"]["name"] == "tcp")
                .then(|| v["executable"].as_str().map(PathBuf::from))
                .flatten()
        })
        .context("TCP executable missing from Cargo output")?;
    let executable = output.join(format!("{label}-tcp"));
    fs::copy(source, &executable)?;
    Ok(executable)
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn peer(
    executable: &Path,
    profile: Option<&Path>,
    callgrind: Option<&Path>,
    legacy: bool,
) -> Command {
    let mut command = if let Some(path) = callgrind {
        let mut cmd = Command::new("valgrind");
        cmd.arg("--tool=callgrind")
            .arg(format!("--callgrind-out-file={}", path.display()))
            .arg(executable);
        cmd
    } else {
        Command::new(executable)
    };
    command.env("CAPNTPROTO_BENCH_PORTABLE", if legacy { "0" } else { "1" });
    if let Some(profile) = profile {
        command.env("LLVM_PROFILE_FILE", profile);
    } else {
        command.env_remove("LLVM_PROFILE_FILE");
    }
    command
}

fn measure(
    executable: &Path,
    output: &Path,
    label: &str,
    size: u32,
    profile: Option<&Path>,
    callgrind: bool,
    legacy: bool,
) -> Result<Value> {
    let server_profile = output.join(format!("{label}-server.callgrind"));
    let ready = output.join(format!("{label}-ready.json"));
    let mut server = Server(
        peer(
            executable,
            profile,
            callgrind.then_some(server_profile.as_path()),
            legacy,
        )
        .arg("server")
        .stdin(Stdio::null())
        .stdout(File::create(&ready)?)
        .stderr(File::create(output.join(format!("{label}-server.log")))?)
        .spawn()?,
    );
    let start = Instant::now();
    let address = loop {
        if let Ok(value) = crate::read_json(&ready) {
            break crate::string(&value, "address")?.to_owned();
        }
        ensure!(
            server.0.try_wait()?.is_none(),
            "{label} server exited before readiness"
        );
        ensure!(
            start.elapsed() < Duration::from_secs(30),
            "{label} server readiness timed out"
        );
        sleep(Duration::from_millis(25));
    };
    let client_profile = output.join(format!("{label}-client.callgrind"));
    let result = crate::process::run(
        peer(
            executable,
            profile,
            callgrind.then_some(client_profile.as_path()),
            legacy,
        )
        .args([
            "measure",
            &address,
            "null",
            &size.to_string(),
            "100",
            "1000",
        ]),
        120,
        None,
        &Default::default(),
    )?;
    crate::write(output.join(format!("{label}-client.log")), &result.stderr)?;
    crate::write(output.join(format!("{label}-measure.json")), &result.stdout)?;
    ensure!(
        result.code == 0,
        "{label} measurement failed; see client log"
    );
    // Graceful EOF flushes both LLVM and Callgrind profiles. Never silently
    // accept a killed training peer or its incomplete instrumentation data.
    let start = Instant::now();
    loop {
        if let Some(status) = server.0.try_wait()? {
            ensure!(status.success(), "{label} server failed: {status}");
            break;
        }
        ensure!(
            start.elapsed() < Duration::from_secs(30),
            "{label} server failed to stop after EOF"
        );
        sleep(Duration::from_millis(25));
    }
    let value: Value = serde_json::from_slice(&result.stdout)?;
    ensure!(
        value["protocol"] == "tcp"
            && value["payload_bytes"] == size
            && value["warmup"] == 100
            && value["iterations"] == 1000
            && value["latency_ns"]
                .as_array()
                .is_some_and(|samples| samples.len() == 1000),
        "{label} did not execute the required workload"
    );
    Ok(value)
}

fn instruction_count(profile: &str) -> Result<u64> {
    let events = profile
        .lines()
        .find_map(|s| s.strip_prefix("events: "))
        .context("missing Callgrind events")?;
    let index = events
        .split_whitespace()
        .position(|s| s == "Ir")
        .context("missing Callgrind Ir event")?;
    // totals includes all parts of the process, rather than a selected function.
    let counts = profile
        .lines()
        .find_map(|s| s.strip_prefix("totals: "))
        .context("missing Callgrind totals")?;
    let count: u64 = counts
        .split_whitespace()
        .nth(index)
        .context("missing instruction total")?
        .parse()?;
    if count == 0 {
        bail!("empty instruction profile");
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_ir_without_assuming_event_order_or_using_partial_summary() {
        assert_eq!(
            instruction_count("events: Dr Ir Dw\nsummary: 1 9 2\ntotals: 20 21000000 3\n").unwrap(),
            21_000_000
        );
        for invalid in [
            "events: Ir\n",
            "events: Dr\ntotals: 10\n",
            "events: Ir\ntotals: 0\n",
        ] {
            assert!(instruction_count(invalid).is_err());
        }
    }
}

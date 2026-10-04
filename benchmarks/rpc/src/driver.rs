//! Runs precompiled clients and servers; never invokes a compiler on the benchmark host.
use crate::Result;
mod placement;
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
const PROTOCOLS: [&str; 4] = ["native", "capnp-cpp", "grpc", "websocket"];
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
    // Each Rust request has a timeout; the enclosing remote command bounds C++ too.
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
    if value["protocol"] != protocol
        || value["payload_bytes"] != bytes
        || value["warmup"] != 10000
        || value["iterations"] != 1000
    {
        return Err("unexpected trial identity".into());
    }
    Ok(value)
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
    let environment = json!({"manifest":manifest, "binary_sha256":hashes,
        "valgrind":output("valgrind", &["--version"])? ,
        "uname":output("uname", &["-a"])? , "cpu":output("lscpu", &[])?,
        "governor":fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").ok(),
        "clocksource":fs::read_to_string("/sys/devices/system/clocksource/clocksource0/current_clocksource").ok(),
        "available_clocksources":fs::read_to_string("/sys/devices/system/clocksource/clocksource0/available_clocksource").ok(),
        "clock_reads":clock_diagnostics(&placement)?,
        "cpu_placement":placement,
        "scope":"Linux IPv4 loopback; sequential validated echo; fixed separate physical cores for server/client across all protocols; 10000 warmups/1000 samples; 5 repetitions; Native QUIC v1 / TLS 1.3 with pinned peer authentication, other protocols plaintext"});
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

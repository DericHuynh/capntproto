//! Cross-host workflow runner. SSH is orchestration only: all measured RPCs use
//! a direct TCP connection to the other host's private address.
use super::*;
use crate::workflows::support::{supported, IMPLEMENTATIONS};
const PORT: u16 = 46001;

fn binary(implementation: &str) -> &str {
    match implementation {
        "capnp-cpp" => "capnp-reference",
        "capnp-rust" => "capnp-rust",
        _ => "driver",
    }
}

fn address(value: &str, smoke: bool) -> Result<std::net::Ipv4Addr> {
    let ip: std::net::Ipv4Addr = value.parse()?;
    if !ip.is_private() && !(smoke && ip.is_loopback()) {
        return Err("workflow host must have a private IPv4 address".into());
    }
    Ok(ip)
}
fn manifest(bundle: &Path) -> Result<Value> {
    let manifest: Value = serde_json::from_slice(&fs::read(bundle.join("manifest.json"))?)?;
    for name in ["driver", "capnp-reference", "capnp-rust"] {
        let actual = output(
            "sha256sum",
            &[bundle.join(name).to_str().ok_or("invalid binary path")?],
        )?;
        if manifest["binaries"][name] != actual.split_whitespace().next().ok_or("missing digest")? {
            return Err(format!("workflow executable digest mismatch: {name}").into());
        }
    }
    Ok(manifest)
}
fn environment(bundle: &Path, placement: &Placement, role: &str) -> Result<Value> {
    Ok(
        json!({"workflow_version":crate::workflows::VERSION, "role":role,
        "manifest":manifest(bundle)?, "hostname":output("hostname", &[])?,
        "boot_id":fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
        "cpu":output("lscpu", &[])?, "cpu_placement":placement,
        "worker_cpu":cpu(placement), "clock_ticks_per_second":output("getconf", &["CLK_TCK"])?,
        "clocksource":fs::read_to_string("/sys/devices/system/clocksource/clocksource0/current_clocksource").ok(),
        "clock_reads":clock_diagnostics(placement)?, "uname":output("uname", &["-a"])?,
        "network_before":fs::read_to_string("/proc/net/dev")?}),
    )
}
fn cpu(placement: &Placement) -> usize {
    placement.server_cpu
}
fn worker(bundle: &Path, index: usize, placement: &Placement) -> Command {
    let mut command = Placement::command(
        &bundle.join(binary(IMPLEMENTATIONS[index])),
        Some(cpu(placement)),
    );
    command.env(
        "CAPNTPROTO_BENCH_PORTABLE",
        if index == 1 { "1" } else { "0" },
    );
    command
}
pub fn host(bundle: &Path, ip: &str, out: &Path, smoke: bool) -> Result<()> {
    if cfg!(debug_assertions) && !smoke {
        return Err("remote workflow servers require release binaries".into());
    }
    let ip = address(ip, smoke)?;
    fs::create_dir_all(out)?;
    if out.join("server-ready.json").exists() || out.join("stop").exists() {
        return Err("workflow host output must be fresh".into());
    }
    let placement = Placement::discover()?;
    let mut env = environment(bundle, &placement, "server")?;
    env["bind_address"] = json!(ip.to_string());
    fs::write(
        out.join("server-environment.json"),
        serde_json::to_vec_pretty(&env)?,
    )?;
    let mut servers = Vec::new();
    for (index, implementation) in IMPLEMENTATIONS.iter().enumerate() {
        let mut command = worker(bundle, index, &placement);
        if matches!(*implementation, "grpc" | "websocket") {
            command.args(["comparison-server", implementation]);
        } else {
            command.arg("workflow-server");
        }
        let mut server = Server(
            command
                .arg(format!("{ip}:{}", PORT + index as u16))
                .stdout(Stdio::piped())
                .stderr(fs::File::create(
                    out.join(format!("{implementation}-server.log")),
                )?)
                .spawn()?,
        );
        let stdout = server.0.stdout.take().ok_or("missing readiness pipe")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = tx.send(BufReader::new(stdout).read_line(&mut line).map(|_| line));
        });
        let ready: Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(15))??)?;
        if index == 2 && ready["ready"] != true {
            return Err("C++ server did not become ready".into());
        }
        if index != 2 && ready["address"] != format!("{ip}:{}", PORT + index as u16) {
            return Err("Rust server bound the wrong address".into());
        }
        servers.push(server);
    }
    fs::write(
        out.join("server-ready.json"),
        serde_json::to_vec_pretty(
            &json!({"address":ip.to_string(),"implementations":IMPLEMENTATIONS,"first_port":PORT}),
        )?,
    )?;
    let deadline = Instant::now() + Duration::from_secs(1800);
    while !out.join("stop").exists() {
        if Instant::now() > deadline {
            return Err("workflow server lifetime exceeded".into());
        }
        for server in &mut servers {
            if server.0.try_wait()?.is_some() {
                return Err("workflow server exited during the campaign".into());
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let mut resources = Vec::new();
    for (server, implementation) in servers.iter().zip(IMPLEMENTATIONS) {
        resources.push(json!({"implementation":implementation,"proc_stat":fs::read_to_string(format!("/proc/{}/stat", server.0.id()))?,"proc_status":fs::read_to_string(format!("/proc/{}/status", server.0.id()))?}));
    }
    env["network_after"] = json!(fs::read_to_string("/proc/net/dev")?);
    env["server_resources"] = json!(resources);
    fs::write(
        out.join("server-environment.json"),
        serde_json::to_vec_pretty(&env)?,
    )?;
    // Drop kills and reaps every worker before confirming the server has stopped.
    drop(servers);
    fs::write(out.join("server-stopped"), "ok\n")?;
    Ok(())
}

pub fn client(bundle: &Path, ip: &str, out: &Path, smoke: bool) -> Result<()> {
    let ip = address(ip, smoke)?;
    fs::create_dir_all(out)?;
    if out.join("workflow-trials.json").exists() {
        return Err("workflow client output must be fresh".into());
    }
    if cfg!(debug_assertions) && !smoke {
        return Err("remote workflows require release binaries".into());
    }
    let placement = Placement::discover()?;
    let mut env = environment(bundle, &placement, "client")?;
    env["server_address"] = json!(ip.to_string());
    env["network_path"] = json!(output("ip", &["route", "get", &ip.to_string()])?);
    env["scope"] = json!(if smoke {
        "local smoke validation; not performance evidence"
    } else {
        "two hosts, direct private IPv4 TCP; closed-loop workflows, no one-way timing"
    });
    env["smoke"] = json!(smoke);
    fs::write(
        out.join("client-environment.json"),
        serde_json::to_vec_pretty(&env)?,
    )?;
    let (warmup, iterations, repetitions) = if smoke { (2, 5, 1) } else { (250, 1000, 5) };
    let mut trials = Vec::new();
    for repetition in 0..repetitions {
        for case in crate::workflows::cases() {
            for step in 0..IMPLEMENTATIONS.len() {
                let index = (step + repetition) % IMPLEMENTATIONS.len();
                let implementation = IMPLEMENTATIONS[index];
                let name = case["name"].as_str().unwrap();
                if !supported(implementation, name) {
                    continue;
                }
                let prefix = format!("{repetition}-{name}-{implementation}");
                eprintln!(
                    "remote workflow {name}, {implementation}, repetition {}",
                    repetition + 1
                );
                // Bound even a wedged executor. /usr/bin/time keeps independent
                // CPU/RSS evidence; stdout contains only the validated trial.
                let mut command = Command::new("timeout");
                command.args(["--kill-after=5s", "180s"]);
                if Path::new("/usr/bin/time").is_file() {
                    command
                        .args(["/usr/bin/time", "-v", "-o"])
                        .arg(out.join(format!("{prefix}.resources.txt")));
                } else if smoke {
                    fs::write(
                        out.join(format!("{prefix}.resources.txt")),
                        "GNU time is unavailable; local smoke only, no CPU/RSS measurement.\n",
                    )?;
                } else {
                    return Err("remote performance runs require /usr/bin/time".into());
                }
                command
                    .args(["taskset", "--cpu-list", &cpu(&placement).to_string()])
                    .arg(bundle.join(binary(implementation)));
                if matches!(implementation, "grpc" | "websocket") {
                    command.args(["comparison-measure", implementation]);
                } else {
                    command.arg("workflow-measure");
                }
                command
                    .args([
                        &format!("{ip}:{}", PORT + index as u16),
                        name,
                        &warmup.to_string(),
                        &iterations.to_string(),
                    ])
                    .env(
                        "CAPNTPROTO_BENCH_PORTABLE",
                        if index == 1 { "1" } else { "0" },
                    )
                    .stderr(fs::File::create(out.join(format!("{prefix}.stderr.log")))?);
                let result = command.output()?;
                fs::write(out.join(format!("{prefix}.json")), &result.stdout)?;
                if !result.status.success() {
                    return Err(
                        format!("workflow {prefix} failed; see stderr and resource log").into(),
                    );
                }
                let mut trial: Value = serde_json::from_slice(&result.stdout)?;
                validate(&trial, &case, implementation, warmup, iterations)?;
                trial["repetition"] = json!(repetition);
                trials.push(trial);
                fs::write(
                    out.join("workflow-trials.json"),
                    serde_json::to_vec_pretty(&trials)?,
                )?;
            }
        }
    }
    env["network_after"] = json!(fs::read_to_string("/proc/net/dev")?);
    fs::write(
        out.join("client-environment.json"),
        serde_json::to_vec_pretty(&env)?,
    )?;
    Ok(())
}
fn validate(
    value: &Value,
    case: &Value,
    implementation: &str,
    warmup: u64,
    iterations: u64,
) -> Result<()> {
    if value["security"] != crate::transport_metadata(implementation)?
        || value["workflow_version"] != crate::workflows::VERSION
        || value["implementation"] != implementation
        || value["case"] != *case
        || value["warmup"] != warmup
        || value["iterations"] != iterations
    {
        return Err("workflow identity mismatch".into());
    }
    let samples = value["latency_ns"]
        .as_array()
        .ok_or("missing workflow samples")?;
    let mut sum = 0u128;
    for sample in samples {
        sum += u128::from(
            sample
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or("invalid workflow sample")?,
        );
    }
    if samples.len() as u64 != iterations
        || sum > u128::from(value["elapsed_ns"].as_u64().ok_or("missing elapsed time")?)
    {
        return Err("workflow timing bounds mismatch".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_loopback_evidence_wrong_workload_and_partial_samples() {
        assert!(address("127.0.0.1", false).is_err());
        assert!(address("0.0.0.0", false).is_err());
        assert!(address("8.8.8.8", false).is_err());
        assert!(address("10.0.0.1", false).is_ok());
        let case = crate::workflows::cases().remove(0);
        let valid = json!({"workflow_version":crate::workflows::VERSION,"implementation":"legacy","security":crate::transport_metadata("legacy").unwrap(),"case":case,"warmup":250,"iterations":2,"elapsed_ns":20,"latency_ns":[10,10]});
        assert!(validate(&valid, &case, "legacy", 250, 2).is_ok());
        for (key, replacement) in [
            ("latency_ns", json!([10])),
            ("elapsed_ns", json!(19)),
            ("implementation", json!("portable")),
            ("warmup", json!(0)),
            ("workflow_version", json!(1)),
            ("security", json!(null)),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = replacement;
            assert!(
                validate(&invalid, &case, "legacy", 250, 2).is_err(),
                "{key}"
            );
        }
    }
}

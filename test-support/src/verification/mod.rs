//! Native Cargo test support for bounded TLC checks and Rust trace replays.
//!
//! The checked-in regression corpus is bound to canonical TLC state/edge graphs.
//! A changed graph fails closed: a stale corpus cannot certify a changed model.
//! TLC is cached by model/configuration/tool content; Rust replays always execute.
use flate2::read::GzDecoder;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};

mod measurement;
pub mod nextest;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub groups: Vec<Group>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub report: String,
    pub checks: Vec<Check>,
    pub inputs: BTreeMap<String, Input>,
    pub scope: String,
    pub limits: serde_json::Value,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub module: String,
    pub config: String,
    pub exit: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphDigest>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub file: String,
    pub sha256: String,
    pub cases: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphDigest {
    pub states: usize,
    pub edges: usize,
    pub sha256: String,
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .into()
}
pub fn catalog() -> &'static Catalog {
    static VALUE: OnceLock<Catalog> = OnceLock::new();
    VALUE.get_or_init(|| {
        let mut value: Catalog =
            serde_json::from_str(include_str!("../../verification/models.json")).unwrap();
        assert_eq!(value.version, 1);
        // Standalone configurations remain the source of truth. Discover new
        // configurations too; keeping a second frozen list would miss coverage.
        for group in &mut value.groups {
            if group.id == "protocol_reference" || group.id == "runtime_reference" {
                group.checks.clear();
                let mut paths: Vec<_> = fs::read_dir(root().join("verification/configs"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|e| e == "cfg"))
                    .collect();
                paths.sort();
                for path in paths {
                    let name = path.file_stem().unwrap().to_str().unwrap();
                    if group.id == "runtime_reference" && !runtime_reference(name) {
                        continue;
                    }
                    let config = fs::read_to_string(&path).unwrap();
                    let annotation = |name: &str| {
                        config.lines().find_map(|line| {
                            line.split_once(name).map(|(_, v)| v.trim().to_owned())
                        })
                    };
                    let module = annotation("@module:").unwrap_or_else(|| "CapnpRpc".into());
                    let violation = annotation("@violation:");
                    group.checks.push(Check {
                        id: path.file_stem().unwrap().to_str().unwrap().into(),
                        module: format!("verification/{module}.tla"),
                        config: config.clone(),
                        exit: if violation.is_some() { 12 } else { 0 },
                        message: violation
                            .map(|v| format!("Invariant {v} is violated"))
                            .unwrap_or_else(|| {
                                "Model checking completed. No error has been found.".into()
                            }),
                        graph: None,
                    });
                }
                if group.id == "protocol_reference" {
                    if let Ok(selected) = std::env::var("CAPNTPROTO_TLC_CASES") {
                        let names: BTreeSet<_> = selected.split(',').map(str::trim).collect();
                        for name in &names {
                            assert!(
                                group.checks.iter().any(|c| c.id == *name),
                                "unknown TLC configuration {name}"
                            );
                        }
                        group.checks.retain(|c| names.contains(c.id.as_str()));
                    }
                }
            } else if group.id == "wire_model" || group.id == "guard_model" {
                for check in &mut group.checks {
                    check.config = fs::read_to_string(
                        root()
                            .join("verification")
                            .join(format!("{}.cfg", check.id)),
                    )
                    .unwrap();
                }
            }
        }
        let mut ids = BTreeSet::new();
        let mut inputs = BTreeSet::new();
        for g in &value.groups {
            assert!(ids.insert(&g.id), "duplicate group {}", g.id);
            assert!(!g.checks.is_empty(), "empty group {}", g.id);
            for key in g.inputs.keys() {
                assert!(inputs.insert(key), "duplicate input {key}");
            }
        }
        value
    })
}

fn runtime_reference(name: &str) -> bool {
    ["Bulk", "Realtime", "Schema"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || [
            "Join",
            "JoinThreeShares",
            "JoinDifferentHost",
            "JoinDifferentObject",
            "JoinOpaque",
            "JoinLive",
            "JoinUnequalLive",
            "BugJoinHostOnly",
            "WitnessJoin",
            "WitnessJoinUnequal",
            "WitnessJoinCancel",
            "TwoPartyJoinLocal",
            "TwoPartyJoinRemote",
            "TwoPartyJoinUnequal",
            "WitnessTwoPartyJoin",
            "ThirdPartyTailCall",
            "WitnessThirdPartyAnswer",
        ]
        .contains(&name)
}
pub fn sha256(bytes: impl AsRef<[u8]>) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes.as_ref())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Decode every DOT state and edge, independent of TLC's randomized fingerprints.
/// State values remain text so sets, records and sequences also compare exactly.
pub fn graph_digest(text: &str) -> Result<GraphDigest> {
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    let mut initial = BTreeSet::new();
    for line in text.lines() {
        let Some((id, rest)) = line.split_once(' ') else {
            continue;
        };
        if id.parse::<i64>().is_err() {
            continue;
        }
        if let Some(target) = rest.strip_prefix("-> ") {
            let target = target.split_whitespace().next().ok_or("missing target")?;
            target.parse::<i64>()?;
            edges.push((id.to_owned(), target.to_owned()));
        } else if let Some(label) = rest.strip_prefix("[label=\"") {
            let end = label.find('"').ok_or("unterminated node label")?;
            let mut fields: Vec<_> = label[..end].split("\\n").map(str::trim).collect();
            fields.sort_unstable();
            let state = fields.join("\n");
            if line.contains("style = filled") {
                initial.insert(state.clone());
            }
            if let Some(old) = nodes.insert(id.to_owned(), state.clone()) {
                if old != state {
                    return Err("conflicting DOT node definitions".into());
                }
            }
        } else {
            return Err(format!("invalid DOT line: {line}").into());
        }
    }
    if nodes.is_empty() {
        return Err("empty TLC graph".into());
    }
    let states: Vec<_> = nodes
        .values()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let index: BTreeMap<_, _> = states.iter().enumerate().map(|(i, v)| (v, i)).collect();
    let mut transitions = BTreeSet::new();
    for (a, b) in edges {
        let a = nodes.get(&a).ok_or("edge references missing source")?;
        let b = nodes.get(&b).ok_or("edge references missing target")?;
        transitions.insert([index[a], index[b]]);
    }
    let edges: Vec<_> = transitions.into_iter().collect();
    let initial: Vec<_> = initial.into_iter().collect();
    let sha256 = sha256(serde_json::to_vec(&(&states, &edges, initial))?);
    Ok(GraphDigest {
        states: states.len(),
        edges: edges.len(),
        sha256,
    })
}

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(root());
    // Nextest exports its profile to tests. A nested workspace must choose its
    // own profile, not inherit the outer CI report's temporary `evidence` profile.
    cmd.env_remove("NEXTEST_PROFILE");
    if std::env::var_os("CAPNTPROTO_COVERAGE_CHILDREN_NATIVE").is_some() {
        // Coverage of this test process is retained. Nested compiler-contract,
        // Miri, sanitizer and mutation commands must use their own toolchains
        // and profiles, rather than mixing incompatible LLVM profile formats.
        for key in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTDOCFLAGS",
            "LLVM_PROFILE_FILE",
            "CARGO_TARGET_DIR",
            "RUSTUP_TOOLCHAIN",
            "CAPNTPROTO_COVERAGE_CHILDREN_NATIVE",
        ] {
            cmd.env_remove(key);
        }
    }
    cmd
}
/// Execute without a shell, capturing a durable combined log and enforcing a timeout.
pub fn run(cmd: &mut Command, log: &Path, expected: i32) -> Result<String> {
    run_with_timeout(cmd, log, expected, check_timeout()?)
}

/// Like `run`, but return only stdout for JSON and other structured output.
/// Stderr remains available in `log`, including on failure.
pub fn run_stdout(cmd: &mut Command, log: &Path, expected: i32) -> Result<String> {
    run_stdout_with_timeout(
        cmd,
        &log.with_extension("stdout"),
        log,
        expected,
        check_timeout()?,
    )
}

fn check_timeout() -> Result<Duration> {
    Ok(Duration::from_secs(
        std::env::var("CAPNTPROTO_CHECK_TIMEOUT")
            .ok()
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(600),
    ))
}

pub fn run_with_timeout(
    cmd: &mut Command,
    log: &Path,
    expected: i32,
    timeout: Duration,
) -> Result<String> {
    run_with_outputs(cmd, log, None, expected, timeout)
}

/// Capture machine-readable stdout separately from diagnostic stderr while
/// retaining the same exit-status, deadline and process-tree cancellation checks.
pub fn run_stdout_with_timeout(
    cmd: &mut Command,
    stdout: &Path,
    log: &Path,
    expected: i32,
    timeout: Duration,
) -> Result<String> {
    if stdout == log {
        return Err("stdout and diagnostic paths must differ".into());
    }
    run_with_outputs(cmd, log, Some(stdout), expected, timeout)
}

fn run_with_outputs(
    cmd: &mut Command,
    log: &Path,
    stdout: Option<&Path>,
    expected: i32,
    timeout: Duration,
) -> Result<String> {
    // Every nested verification driver forwards termination to its own group.
    // Handling SIGTERM here lets cancellation propagate through Cargo/test/TLC
    // chains even though each command has an isolated process group.
    static CANCELLED: OnceLock<std::io::Result<Arc<AtomicBool>>> = OnceLock::new();
    let cancelled = CANCELLED
        .get_or_init(|| {
            let flag = Arc::new(AtomicBool::new(false));
            for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
                signal_hook::flag::register(signal, flag.clone())?;
            }
            Ok(flag)
        })
        .as_ref()
        .map_err(|e| format!("install cancellation handlers: {e}"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("verification cancelled".into());
    }
    if let Some(parent) = log.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(log)?;
    let output = if let Some(path) = stdout {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        File::create(path)?
    } else {
        file.try_clone()?
    };
    cmd.stdout(output).stderr(Stdio::from(file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| format!("launch {cmd:?}: {e}"))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let interrupted = cancelled.load(Ordering::Relaxed);
        if interrupted || start.elapsed() > timeout {
            #[cfg(unix)]
            {
                let groups = descendant_groups(child.id());
                for group in &groups {
                    let _ = Command::new("kill")
                        .args(["-TERM", "--", &format!("-{group}")])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                // Let each parent reap its stopped descendants and finish its
                // own cleanup (including LLVM's atexit profile flush) before
                // escalating that parent's group. A single shared 500ms sleep
                // races every nested driver's own 500ms grace period.
                let cleanup_deadline = Instant::now() + Duration::from_secs(2);
                for group in groups.iter().rev() {
                    let grace = (Instant::now() + Duration::from_millis(500)).min(cleanup_deadline);
                    loop {
                        // Reap the direct child so an exited group leader does
                        // not look alive merely because it is a zombie.
                        let _ = child.try_wait();
                        if !Command::new("kill")
                            .args(["-0", "--", &format!("-{group}")])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status()
                            .is_ok_and(|status| status.success())
                        {
                            break;
                        }
                        if Instant::now() >= grace {
                            let _ = Command::new("kill")
                                .args(["-KILL", "--", &format!("-{group}")])
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            let reason = if interrupted {
                "cancelled"
            } else {
                "timed out"
            };
            return Err(format!("{reason}: {cmd:?}; inspect {}", log.display()).into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = fs::read_to_string(log)?;
    if status.code() != Some(expected) {
        return Err(format!(
            "{cmd:?}: expected {expected}, got {status}; inspect {}\n{}",
            log.display(),
            output
                .chars()
                .rev()
                .take(3000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
        .into());
    }
    if let Some(path) = stdout {
        Ok(fs::read_to_string(path)?)
    } else {
        Ok(output)
    }
}
#[cfg(unix)]
fn descendant_groups(pid: u32) -> Vec<u32> {
    let mut groups = vec![pid];
    // Linux children may themselves have started isolated process groups.
    // Snapshot these before TERM reparents them; KILL must reach even a child
    // that ignores termination. Other Unix platforms still stop the main group.
    #[cfg(target_os = "linux")]
    {
        let mut pending = vec![pid];
        while let Some(parent) = pending.pop() {
            let Ok(tasks) = fs::read_dir(format!("/proc/{parent}/task")) else {
                continue;
            };
            for task in tasks.flatten() {
                let Ok(children) = fs::read_to_string(task.path().join("children")) else {
                    continue;
                };
                for child in children
                    .split_whitespace()
                    .filter_map(|p| p.parse::<u32>().ok())
                {
                    pending.push(child);
                    if let Ok(stat) = fs::read_to_string(format!("/proc/{child}/stat")) {
                        if stat
                            .rsplit_once(") ")
                            .and_then(|(_, fields)| fields.split_whitespace().nth(2))
                            .and_then(|p| p.parse::<u32>().ok())
                            == Some(child)
                        {
                            groups.push(child);
                        }
                    }
                }
            }
        }
    }
    // Discovery is parent-before-child. Numeric PID sorting loses that order
    // when PID allocation wraps, and siblings do not need an ordering.
    let mut seen = BTreeSet::new();
    groups.retain(|group| seen.insert(*group));
    groups
}
fn lock(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}
fn tools() -> Result<(PathBuf, PathBuf)> {
    let java = std::env::var_os("JAVA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if Path::new("/tmp/capntproto-jre17/bin/java").exists() {
                "/tmp/capntproto-jre17/bin/java".into()
            } else {
                "java".into()
            }
        });
    let jar = std::env::var_os("TLA2TOOLS_JAR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let local = root().join("target/tools/tla2tools.jar");
            if local.is_file() {
                local
            } else {
                "/tmp/capntproto-tla2tools.jar".into()
            }
        });
    if !jar.is_file() {
        return Err("TLC jar missing: set TLA2TOOLS_JAR to tla2tools.jar".into());
    }
    Ok((java, jar))
}
fn fingerprint(group: &Group, java: &Path, jar: &Path) -> Result<String> {
    let mut files = BTreeMap::new();
    for dir in [
        root().join("verification"),
        root().join("verification/configs"),
        root().join("verification/audit"),
        root().join("verification/audit/configs"),
    ] {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|v| v == "tla" || v == "cfg") {
                files.insert(
                    path.strip_prefix(root())?.to_owned(),
                    sha256(fs::read(path)?),
                );
            }
        }
    }
    let version = Command::new(java).arg("-version").output()?;
    if !version.status.success() {
        return Err("java -version failed".into());
    }
    Ok(sha256(serde_json::to_vec(&(
        group,
        files,
        sha256(fs::read(jar)?),
        version.stdout,
        version.stderr,
        include_str!("mod.rs"),
        std::env::var("CAPNTPROTO_TLC_SESSION").ok(),
    ))?))
}

pub(super) fn require_model_lane() -> Result<()> {
    if std::env::var("CAPNTPROTO_CI_LANE").as_deref() == Ok("cargo") {
        return Err("TLC check reached the Cargo partition: name its test with tlc so the model job owns it".into());
    }
    Ok(())
}

pub fn verify(group: &Group) -> Result<()> {
    require_model_lane()?;
    let dir = root().join("target/verification").join(&group.report);
    let _group_lock = lock(&dir.join("check.lock"))?;
    let (java, jar) = tools()?;
    let before = fingerprint(group, &java, &jar)?;
    let stamp = dir.join("checked.json");
    if std::env::var_os("CAPNTPROTO_TLC_FRESH").is_none() {
        if let Ok(bytes) = fs::read(&stamp) {
            if let Ok((key, logs)) =
                serde_json::from_slice::<(String, BTreeMap<String, String>)>(&bytes)
            {
                if key == before
                    && !logs.is_empty()
                    && logs
                        .iter()
                        .all(|(p, h)| fs::read(dir.join(p)).is_ok_and(|b| sha256(b) == *h))
                {
                    return Ok(());
                }
            }
        }
    }
    // Serialize JVMs across libtest processes; parallel Rust replay remains enabled.
    let _jvm_lock = lock(&root().join("target/verification/tlc.lock"))?;
    let mut logs = BTreeMap::new();
    for check in &group.checks {
        eprintln!("TLC {} / {}", group.report, check.id);
        let destination = dir.join(&check.id);
        fs::create_dir_all(&destination)?;
        let cfg = destination.join("model.cfg");
        fs::write(&cfg, &check.config)?;
        let states = tempfile::tempdir_in(&destination)?;
        let mut cmd = command(&java);
        cmd.arg(format!(
            "-DTLA-Library={}",
            root().join("verification").display()
        ))
        .args(["-XX:+UseParallelGC", "-Xmx1g", "-cp"])
        .arg(std::env::join_paths([
            jar.clone(),
            root().join("verification"),
        ])?)
        .args(["tlc2.TLC", "-workers", "2", "-fp", "0", "-config"])
        .arg(&cfg)
        .arg("-metadir")
        .arg(states.path());
        let dot = destination.join("graph.dot");
        if check.graph.is_some() {
            cmd.args(["-dump", "dot"]).arg(&dot);
        }
        cmd.arg(&check.module);
        let log = destination.join("tlc.log");
        let measurement =
            measurement::Measurement::start(&check.module, &check.config, check.exit, &log)?;
        let output = run(&mut cmd, &log, check.exit)?;
        if !output.contains(&check.message) {
            return Err(format!(
                "missing expected result {:?} in {}",
                check.message,
                log.display()
            )
            .into());
        }
        if let Some(expected) = &check.graph {
            let actual = graph_digest(&fs::read_to_string(&dot)?)?;
            if &actual != expected {
                return Err(format!("TLC graph changed for {}/{}: expected {expected:?}, got {actual:?}; update the reviewed trace corpus together with the model", group.report, check.id).into());
            }
        }
        measurement.finish()?;
        logs.insert(
            log.strip_prefix(&dir)?.to_string_lossy().into_owned(),
            sha256(output),
        );
    }
    if fingerprint(group, &java, &jar)? != before {
        return Err("model inputs changed during checking".into());
    }
    fs::write(&stamp, serde_json::to_vec_pretty(&(before, logs))?)?;
    Ok(())
}

/// Resolve a trace corpus only after its TLC checks pass. Explicit caller inputs
/// are supported for reproductions; the default path never reads old reports.
pub fn input(name: &str) -> std::result::Result<String, String> {
    if let Ok(value) = std::env::var(name) {
        return Ok(value);
    }
    let result = (|| -> Result<String> {
        let group = catalog()
            .groups
            .iter()
            .find(|g| g.inputs.contains_key(name))
            .ok_or_else(|| format!("unknown verification input {name}"))?;
        verify(group)?;
        let input = &group.inputs[name];
        let source = root().join("test-support/verification").join(&input.file);
        let bytes = fs::read(source)?;
        if sha256(&bytes) != input.sha256 {
            return Err(format!("trace corpus checksum mismatch: {name}").into());
        }
        let mut plain = Vec::new();
        GzDecoder::new(bytes.as_slice()).read_to_end(&mut plain)?;
        let value: serde_json::Value = serde_json::from_slice(&plain)?;
        let cases = value
            .as_array()
            .or_else(|| value.get("cases").and_then(|v| v.as_array()))
            .ok_or("invalid trace corpus")?;
        if cases.len() != input.cases || cases.is_empty() {
            return Err("trace corpus case count mismatch".into());
        }
        let destination = root()
            .join("target/verification/inputs")
            .join(format!("{}.json", input.sha256));
        fs::create_dir_all(destination.parent().unwrap())?;
        // Same input can serve several binaries. Write then atomically publish.
        let mut temporary = tempfile::NamedTempFile::new_in(destination.parent().unwrap())?;
        std::io::Write::write_all(&mut temporary, &plain)?;
        temporary.persist(&destination)?;
        Ok(destination.to_string_lossy().into_owned())
    })();
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn structured_output_keeps_warnings_out_of_json_and_preserves_failure_diagnostics() {
        let directory = tempfile::tempdir().unwrap();
        let stdout = directory.path().join("export.json");
        let log = directory.path().join("export.log");
        let script = "printf 'warning: mismatched data\\n' >&2; printf '{\"data\":[]}'";
        let output = run_stdout(Command::new("sh").args(["-c", script]), &log, 0).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            serde_json::json!({"data": []})
        );
        assert_eq!(
            fs::read_to_string(log.with_extension("stdout")).unwrap(),
            output
        );
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "warning: mismatched data\n"
        );

        let error = run_stdout_with_timeout(
            Command::new("sh").args([
                "-c",
                "printf 'partial output'; printf 'export failed\\n' >&2; exit 7",
            ]),
            &stdout,
            &log,
            0,
            Duration::from_secs(5),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("export failed"));
        assert_eq!(fs::read_to_string(&stdout).unwrap(), "partial output");
        assert!(run_stdout_with_timeout(
            &mut Command::new("sh"),
            &log,
            &log,
            0,
            Duration::from_secs(5)
        )
        .is_err());
    }

    #[test]
    fn repository_layout_preserves_verification_and_distribution_inputs() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("source.tar.gz");
        distribution::build(&root(), &archive, false).unwrap();
        let unpacked = distribution::unpack(&archive, &work.path().join("unpacked")).unwrap();
        let sources = distribution::sources(&unpacked).unwrap();
        for path in [
            "verification/CapnpNetwork.tla",
            "verification/configs/NetworkBasic.cfg",
            "verification/audit/CapnpConformance.tla",
            "verification/audit/configs/WireReleaseEffect.cfg",
            "research/EAE-Reconstruction.zip",
            "research/baseline/CapnpRpc.tla",
            "research/reports/storage-benchmark/comparison.json",
            "research/reports/eae-integration/verification.json",
            "scripts/generate_network_configs.py",
            "scripts/generate_feature_configs.py",
            "scripts/generate_realtime_configs.py",
            "docs/wiki/Protocol-Models.md",
            "docs/wiki/Home.md",
            "docs/wiki/_Sidebar.md",
            "docs/documentation-review.json",
            "scripts/wiki.py",
            "scripts/tests/test_wiki.py",
            "vendor/capnproto/CMakeLists.txt",
            "vendor/capnproto/c++/src/capnp/rpc.c++",
            "vendor/capnproto/LICENSE",
            "crates/capntproto-core/src/lib.rs",
            "crates/capntproto-core/LICENSE",
            "crates/capntproto-rpc/src/lib.rs",
            "crates/capntproto-rpc/LICENSE",
            "crates/capntproto-futures/src/lib.rs",
            "crates/capntproto-futures/LICENSE",
            "crates/capntproto-codegen/src/lib.rs",
            "crates/capntproto-codegen/LICENSE",
            "schemas/imports/capnp/schema.capnp",
            "schemas/imports/capnp/c++.capnp",
            "schemas/imports/capnp/stream.capnp",
        ] {
            assert!(sources.contains(Path::new(path)), "unbundled input {path}");
        }
        assert!(!sources.iter().any(|path| {
            path.starts_with("research/baseline/pre-apalache")
                || path.starts_with("research/baseline/apalache-abandoned")
                || path.starts_with("target")
        }));
        for group in &catalog().groups {
            for check in &group.checks {
                assert!(
                    sources.contains(Path::new(&check.module)),
                    "missing model input for {}/{}: {}",
                    group.id,
                    check.id,
                    check.module
                );
            }
        }
        assert_eq!(
            sha256(fs::read(unpacked.join("research/EAE-Reconstruction.zip")).unwrap()),
            "55872653058ee56813a3417df242760b81dc002e53500ca13c7c696335cf7f5b"
        );
        // Exercise the qualified archive layout with test-only evidence in the
        // temporary extraction. Never certify or change this workspace's reports.
        let evidence = unpacked.join("target/release-qualification");
        fs::create_dir_all(&evidence).unwrap();
        fs::write(
            unpacked.join("research/reports/storage-benchmark/local-run.log"),
            "ignored local run fixture\n",
        )
        .unwrap();
        fs::write(
            evidence.join("cargo-qualification.json"),
            serde_json::to_vec(&serde_json::json!({
                "sources": distribution::verification_hashes(&unpacked).unwrap(),
                "scope": "archive layout test fixture"
            }))
            .unwrap(),
        )
        .unwrap();
        for name in ["cargo-tests.log", "cargo-doctests.log", "cargo-clippy.log"] {
            fs::write(evidence.join(name), "archive layout test fixture\n").unwrap();
        }
        assert_eq!(sources, distribution::sources(&unpacked).unwrap());
        let qualified = work.path().join("qualified.tar.gz");
        distribution::build(&unpacked, &qualified, true).unwrap();
        let checked = distribution::unpack(&qualified, &work.path().join("qualified")).unwrap();
        assert_eq!(
            fs::read(evidence.join("cargo-qualification.json")).unwrap(),
            fs::read(checked.join("target/release-qualification/cargo-qualification.json"))
                .unwrap()
        );
        fs::write(
            unpacked.join("verification/CapnpNetwork.tla"),
            "changed test fixture",
        )
        .unwrap();
        assert!(distribution::build(&unpacked, &qualified, true)
            .unwrap_err()
            .to_string()
            .contains("source qualification is stale"));
    }

    #[test]
    fn graph_fingerprint_ignores_node_ids_and_declaration_order() {
        let a = "1 [label=\"/\\ x = 0\\n/\\ y = TRUE\",style = filled]\n1 -> 2 [label=\"\"];\n2 [label=\"/\\ y = FALSE\\n/\\ x = 1\"]";
        let b = "-8 [label=\"/\\ x = 1\\n/\\ y = FALSE\"]\n99 -> -8 [label=\"\"];\n99 [label=\"/\\ y = TRUE\\n/\\ x = 0\",style = filled]";
        assert_eq!(graph_digest(a).unwrap(), graph_digest(b).unwrap());
        assert_ne!(
            graph_digest(a).unwrap(),
            graph_digest(&b.replace("TRUE", "FALSE")).unwrap()
        );
        assert!(graph_digest("1 -> 2 [label=\"\"]").is_err());
        assert!(graph_digest("1 [label=\"x\"]\n1 -> 2 [label=\"\"]").is_err());
    }
}

pub mod cpp;
pub mod distribution;
pub mod exploration;

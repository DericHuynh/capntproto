//! Source-bound quality evidence. No shell evaluation and no synthetic results.
#![forbid(unsafe_code)]
pub mod benchmark;
mod charts;
pub mod coverage;
pub mod instructions;
pub mod lanes;
pub mod report;
use capntproto_test_support::verification::{self as v, root};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn source_id() -> Result<String> {
    let files: BTreeMap<_, _> = v::distribution::verification_hashes(&root())?
        .into_iter()
        .filter(|(p, _)| {
            !p.starts_with("research/reports") && p != Path::new("quality/coverage-baseline.json")
        })
        .collect();
    Ok(v::sha256(serde_json::to_vec(&files)?))
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    pub command: Vec<String>,
    pub seconds: f64,
    pub passed: bool,
    pub log_sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub format: u32,
    pub lane: String,
    pub source_id: String,
    pub passed: bool,
    pub error: Option<String>,
    pub compiler: String,
    pub steps: Vec<Step>,
    pub data: Value,
}
pub struct Runner {
    pub directory: PathBuf,
    pub evidence: Evidence,
}
impl Runner {
    pub fn new(lane: &str, directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory.join("logs"))?;
        // Immediately replace a previous success with an explicit incomplete run.
        let compiler = Command::new("rustc").args(["-vV"]).output()?;
        if !compiler.status.success() {
            return Err("rustc identity failed".into());
        }
        let runner = Self {
            directory: directory.canonicalize()?,
            evidence: Evidence {
                format: 1,
                lane: lane.into(),
                source_id: source_id()?,
                passed: false,
                error: Some("run incomplete".into()),
                compiler: String::from_utf8(compiler.stdout)?,
                steps: vec![],
                data: json!({}),
            },
        };
        runner.save()?;
        Ok(runner)
    }
    pub fn run(&mut self, name: &str, cmd: &mut Command) -> Result<String> {
        self.run_output(name, cmd, false)
    }
    /// Machine-readable output must not include warnings written to stderr.
    pub fn run_stdout(&mut self, name: &str, cmd: &mut Command) -> Result<String> {
        self.run_output(name, cmd, true)
    }
    fn run_output(
        &mut self,
        name: &str,
        cmd: &mut Command,
        separate_stdout: bool,
    ) -> Result<String> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("invalid log name".into());
        }
        let log = self.directory.join("logs").join(format!("{name}.log"));
        let command = std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        eprintln!("quality {}: {name}", self.evidence.lane);
        let start = Instant::now();
        let timeout = Duration::from_secs(7200);
        let result = if separate_stdout {
            v::run_stdout_with_timeout(cmd, &log.with_extension("stdout"), &log, 0, timeout)
        } else {
            v::run_with_timeout(cmd, &log, 0, timeout)
        };
        self.evidence.steps.push(Step {
            name: name.into(),
            command,
            seconds: start.elapsed().as_secs_f64(),
            passed: result.is_ok(),
            log_sha256: v::sha256(fs::read(&log).unwrap_or_default()),
        });
        self.save()?;
        eprintln!(
            "quality {}: {name} {} ({:.1}s)",
            self.evidence.lane,
            if result.is_ok() { "passed" } else { "failed" },
            start.elapsed().as_secs_f64()
        );
        result
    }
    pub fn cargo(&mut self, name: &str, args: &[&str]) -> Result<String> {
        let mut cmd = v::command("cargo");
        let args = if args.first().is_some_and(|arg| arg.starts_with('+')) {
            cmd.arg(args[0]);
            &args[1..]
        } else {
            args
        };
        self.run(name, cmd.arg("auditable").args(args))
    }
    /// Give each invocation its own JUnit destination, including failures. A
    /// previous run or a nested nextest process cannot supply its evidence.
    pub fn nextest(&mut self, name: &str, cmd: &mut Command) -> Result<String> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("invalid log name".into());
        }
        let junit = self.directory.join("logs").join(format!("{name}.xml"));
        if junit.exists() {
            fs::remove_file(&junit)?;
        }
        // A repeated invocation must invalidate both the file and its recorded
        // digest before starting. Failure before nextest writes a report must
        // never retain a previous invocation's evidence.
        if let Some(reports) = self.evidence.data["test_reports"].as_object_mut() {
            reports.remove(name);
        }
        self.save()?;
        let config = tempfile::Builder::new()
            .prefix("nextest-")
            .suffix(".toml")
            .tempfile_in(&self.directory)?;
        let base = fs::read_to_string(root().join(".config/nextest.toml"))?;
        fs::write(
            config.path(),
            format!(
                "{base}\n[profile.evidence]\ninherits = \"ci\"\n[profile.evidence.junit]\npath = {}\n",
                serde_json::to_string(&junit)?,
            ),
        )?;
        cmd.args(["--profile", "evidence", "--config-file"])
            .arg(config.path());
        let result = self.run(name, cmd);
        if junit.exists() {
            self.evidence.data["test_reports"][name] = json!({
                "file": format!("{name}.xml"),
                "sha256": v::sha256(fs::read(junit)?),
            });
            self.save()?;
        } else if result.is_ok() {
            return Err("successful nextest command produced no JUnit report".into());
        }
        result
    }
    pub fn save(&self) -> Result<()> {
        fs::write(
            self.directory.join("evidence.json"),
            serde_json::to_vec_pretty(&self.evidence)?,
        )?;
        Ok(())
    }
    pub fn finish(&mut self, result: Result<()>) -> Result<()> {
        let result = result.and_then(|()| {
            if source_id()? == self.evidence.source_id {
                Ok(())
            } else {
                Err("sources changed during qualification".into())
            }
        });
        self.evidence.passed = result.is_ok();
        self.evidence.error = result.as_ref().err().map(ToString::to_string);
        self.save()?;
        result
    }
}

pub fn qualification(r: &mut Runner) -> Result<()> {
    let tests = r.nextest(
        "workspace-tests",
        v::command("cargo").args([
            "auditable",
            "nextest",
            "run",
            "--locked",
            "--workspace",
            "--no-fail-fast",
        ]),
    );
    let docs = r.cargo(
        "workspace-doctests",
        &["test", "--locked", "--workspace", "--doc"],
    );
    tests?;
    docs?;
    r.evidence.data["scope"] = "Default-feature workspace tests, including bounded models, Miri, mutations, fuzz controls, native/C++ checks and doctests".into();
    Ok(())
}

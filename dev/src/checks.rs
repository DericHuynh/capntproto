use crate::{file_hash, hash, process, read_json, remove, root, write, write_json};
use anyhow::{ensure, Context, Result};
use clap::Args;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Args)]
pub struct AuditArgs {
    #[arg(long)]
    pub output: PathBuf,
    #[arg(required = true)]
    pub binaries: Vec<PathBuf>,
}
pub fn audit(a: &AuditArgs) -> Result<()> {
    remove(&a.output)?;
    let mut values = BTreeMap::new();
    for binary in &a.binaries {
        let name = binary
            .file_name()
            .context("binary has no name")?
            .to_string_lossy()
            .into_owned();
        ensure!(!values.contains_key(&name), "duplicate binary name: {name}");
        let metadata: Value = serde_json::from_slice(&process::checked(
            Command::new("rust-audit-info").arg(binary),
            120,
        )?)?;
        ensure!(
            metadata["packages"]
                .as_array()
                .is_some_and(|a| a.iter().any(|p| p["root"] == true)),
            "{}: missing root dependency inventory",
            binary.display()
        );
        values.insert(
            name,
            json!({"sha256":file_hash(binary)?,"metadata":metadata}),
        );
    }
    write_json(&a.output, &values)?;
    println!("Verified embedded inventories in {} binaries", values.len());
    Ok(())
}
pub fn repository(root: &Path) -> Result<()> {
    let git =
        |args: &[&str]| process::checked(Command::new("git").arg("-C").arg(root).args(args), 120);
    let mut errors = Vec::new();
    for p in git(&[
        "ls-files",
        "--cached",
        "--ignored",
        "--exclude-standard",
        "-z",
    ])?
    .split(|b| *b == 0)
    .filter(|p| !p.is_empty())
    {
        errors.push(format!(
            "Ignored file is tracked: {}",
            String::from_utf8_lossy(p)
        ));
    }
    let mut blobs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in git(&["ls-files", "--stage", "-z"])?
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
    {
        let (meta, path) = entry.split_at(
            entry
                .iter()
                .position(|b| *b == b'\t')
                .context("invalid git index entry")?,
        );
        let meta = std::str::from_utf8(meta)?
            .split_whitespace()
            .collect::<Vec<_>>();
        ensure!(meta.len() == 3, "invalid git stage");
        let path = String::from_utf8_lossy(&path[1..]).into_owned();
        if meta[2] != "0" {
            errors.push(format!("Unmerged path: {path}"));
        } else if meta[0] != "160000" {
            blobs.entry(meta[1].into()).or_default().push(path);
        }
    }
    // Batch input uses a file, never a shell or interpolated path.
    if !blobs.is_empty() {
        use std::io::Write;
        let mut input = tempfile::tempfile()?;
        for oid in blobs.keys() {
            writeln!(input, "{oid}")?;
        }
        use std::io::Seek;
        input.rewind()?;
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ])
            .stdin(input)
            .output()?;
        ensure!(output.status.success(), "git cat-file failed");
        for line in std::str::from_utf8(&output.stdout)?.lines() {
            let p = line.split_whitespace().collect::<Vec<_>>();
            ensure!(p.len() == 3, "invalid object metadata");
            if p[1] != "blob" {
                errors.push(format!("Expected blob: {}", p[0]));
            } else if p[2].parse::<u64>()? > 50 * 1024 * 1024 {
                errors.push(format!("Blob exceeds 50 MiB: {:?}", blobs[p[0]]));
            }
        }
    }
    ensure!(
        errors.is_empty(),
        "Repository check failed:\n{}",
        errors.join("\n")
    );
    println!("Repository check passed");
    Ok(())
}
pub fn nextest(args: &[String]) -> Result<i32> {
    nextest_at(&root(), args, |command| {
        Ok(command.status()?.code().unwrap_or(1))
    })
}
fn nextest_at(
    root: &Path,
    args: &[String],
    execute: impl FnOnce(&mut Command) -> Result<i32>,
) -> Result<i32> {
    let split = args
        .iter()
        .position(|a| a == "--")
        .context("separate command prefix and test selection with --")?;
    ensure!(split > 0, "missing command prefix");
    let reports = root.join("target/nextest/reports");
    fs::create_dir_all(&reports)?;
    let name = &hash(args.join("\0"))[..16];
    let report = reports.join(format!("{name}.xml"));
    remove(&report)?;
    let config = fs::read_to_string(root.join(".config/nextest.toml"))?
        + &format!(
            "\n[profile.evidence]\ninherits = \"ci\"\n[profile.evidence.junit]\npath = {}\n",
            serde_json::to_string(&report)?
        );
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("nextest.toml");
    write(&path, config)?;
    let mut command = Command::new(&args[0]);
    command
        .args(&args[1..split])
        .args(["nextest", "run", "--config-file"])
        .arg(path)
        .args(["--profile", "evidence"])
        .args(&args[split + 1..])
        .current_dir(root)
        .env_remove("NEXTEST_PROFILE");
    write(
        reports.join(format!("{name}.command.txt")),
        format!("{command:?}\n"),
    )?;
    let code = execute(&mut command)?;
    ensure!(
        code != 0 || report.is_file(),
        "successful nextest run produced no JUnit report"
    );
    Ok(code)
}
pub fn unsafe_inventory(text: &str, root: &Path) -> Result<Value> {
    let mut seen = BTreeSet::new();
    for line in text.lines().filter(|l| l.starts_with('{')) {
        let e: Value = serde_json::from_str(line)?;
        if e["reason"] != "compiler-message" {
            continue;
        }
        let code = e["message"]["code"]["code"].as_str().unwrap_or("");
        let code = if code == "E0133" {
            "unsafe_op_in_unsafe_fn"
        } else {
            code
        };
        if ![
            "clippy::undocumented_unsafe_blocks",
            "clippy::missing_safety_doc",
            "unsafe_op_in_unsafe_fn",
        ]
        .contains(&code)
        {
            continue;
        }
        let spans = e["message"]["spans"]
            .as_array()
            .context("diagnostic spans missing")?;
        let mut primary = false;
        for span in spans.iter().filter(|s| s["is_primary"] == true) {
            primary = true;
            let path = root
                .join(crate::string(span, "file_name")?)
                .canonicalize()?;
            let rel = path
                .strip_prefix(root.canonicalize()?)?
                .to_string_lossy()
                .replace('\\', "/");
            seen.insert((
                rel,
                code.to_owned(),
                crate::number(span, "line_start")?,
                crate::number(span, "column_start")?,
            ));
        }
        ensure!(primary, "unsafe diagnostic has no primary span");
    }
    let mut files = json!({});
    for (path, code, _, _) in seen {
        if files.get(&path).is_none() {
            files[&path] = json!({"sha256":file_hash(root.join(&path))?,"diagnostics":{}});
        }
        let n = files[&path]["diagnostics"][&code].as_u64().unwrap_or(0);
        files[&path]["diagnostics"][&code] = json!(n + 1);
    }
    Ok(files)
}
fn compiler_errors(output: &[u8]) -> String {
    output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(|event| {
            event["reason"] == "compiler-message" && event["message"]["level"] == "error"
        })
        .filter_map(|event| {
            event["message"]["rendered"]
                .as_str()
                .or_else(|| event["message"]["message"].as_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn unsafe_code() -> Result<()> {
    let args = [
        "clippy",
        "--locked",
        "--workspace",
        "--all-targets",
        "--message-format=json",
        "--",
        "-D",
        "warnings",
        "--force-warn=clippy::undocumented_unsafe_blocks",
        "--force-warn=clippy::missing_safety_doc",
        "--force-warn=unsafe_op_in_unsafe_fn",
    ];
    let output = process::run(
        Command::new("cargo").args(args),
        1800,
        None,
        &Default::default(),
    )?;
    let directory = root().join("target/quality/unsafe");
    write(directory.join("clippy.jsonl"), &output.stdout)?;
    ensure!(
        output.code == 0,
        "Clippy failed:\n{}\n{}",
        compiler_errors(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = unsafe_inventory(std::str::from_utf8(&output.stdout)?, &root())?;
    write_json(directory.join("inventory.json"), &actual)?;
    let baseline = read_json(root().join("quality/unsafe-baseline.json"))?;
    validate_unsafe(&actual, &baseline)?;
    println!("Unsafe documentation gate passed");
    Ok(())
}
fn validate_unsafe(actual: &Value, baseline: &Value) -> Result<()> {
    ensure!(
        baseline["files"]
            .as_object()
            .context("invalid unsafe baseline")?
            .keys()
            .all(|p| p.starts_with("crates/capntproto-core/src/")),
        "unsafe exceptions may only cover core runtime"
    );
    ensure!(*actual==baseline["files"],"Unsafe documentation debt changed; inspect target/quality/unsafe/inventory.json and document/review changes");

    Ok(())
}
pub fn workflows() -> Result<()> {
    let mut all = BTreeMap::new();
    for f in fs::read_dir(root().join(".github/workflows"))? {
        let p = f?.path();
        if p.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            let yaml: Value = serde_saphyr::from_str(&fs::read_to_string(&p)?)?;
            all.insert(p.file_stem().unwrap().to_string_lossy().into_owned(), yaml);
        }
    }
    workflow_policy(&all)?;
    println!("Workflow triggers and dependencies passed");
    Ok(())
}
fn workflow_policy(all: &BTreeMap<String, Value>) -> Result<()> {
    let mut automatic = Vec::new();
    for (name, workflow) in all {
        if workflow["on"].get("push").is_some() || workflow["on"].get("pull_request").is_some() {
            automatic.push(name.as_str());
            ensure!(
                workflow["on"]["push"]["branches"] == json!(["main"])
                    && workflow["on"]["pull_request"].is_null(),
                "{name}: automatic checks must cover every PR and main"
            );
        }
    }
    ensure!(
        automatic == ["ci", "quality"],
        "CI and coverage must be the only automatic entrypoints"
    );
    let jobs = &all["ci"]["jobs"];
    let required = [
        "workflows",
        "documentation",
        "cargo",
        "examples",
        "models",
        "fuzz",
        "extended",
        "performance",
    ];
    ensure!(
        jobs["report"]["if"] == "always()" && jobs["report"]["needs"] == json!(required),
        "CI must require every verification lane"
    );
    ensure!(
        keys(jobs)?
            == required
                .into_iter()
                .map(str::to_owned)
                .chain(["report".into()])
                .collect(),
        "untracked or missing CI gate"
    );
    for (job, file) in [
        ("workflows", "ci-workflows"),
        ("documentation", "ci-docs"),
        ("cargo", "verification-tests"),
        ("models", "verification-models"),
        ("fuzz", "verification-fuzz"),
        ("extended", "verification-extended"),
        ("performance", "performance"),
    ] {
        ensure!(
            jobs[job]["uses"] == format!("./.github/workflows/{file}.yml")
                && all[file]["on"].get("workflow_call").is_some(),
            "{job}: reusable workflow is missing"
        );
    }
    let quality = &all["quality"]["jobs"];
    ensure!(
        keys(quality)? == BTreeSet::from(["coverage".into()])
            && quality["coverage"].get("if").is_none()
            && quality["coverage"].get("continue-on-error").is_none(),
        "coverage must be independent and required for its own badge"
    );
    let coverage = quality["coverage"]["steps"]
        .as_array()
        .context("coverage steps")?;
    ensure!(
        coverage.iter().any(|step| step["run"]
            == "cargo run --locked -p capntproto-quality -- coverage target/quality/coverage"
            && step.get("if").is_none()
            && step.get("continue-on-error").is_none()),
        "coverage must execute its instrumented checks"
    );
    let performance = &all["performance"];
    ensure!(
        performance["permissions"]["contents"] == "read"
            && performance.to_string().find("secrets.").is_none(),
        "instruction profiles need no cloud credentials or write access"
    );
    ensure!(
        keys(&performance["jobs"])? == BTreeSet::from(["profile".into()]),
        "only the legacy instruction profiler is supported"
    );
    ensure!(
        performance["jobs"]["profile"]["env"]["CAPNTPROTO_VALGRIND_INCLUDE"] == "/usr/include",
        "measured-region headers are required"
    );
    ensure!(
        performance["jobs"]["profile"]["steps"]
            .as_array()
            .context("profile steps")?
            .iter()
            .any(|step| step["run"]
                .as_str()
                .is_some_and(|run| run.contains("runtime-runner --profile"))),
        "benchmarks must run the repeated instruction profiler"
    );
    if let Some(release) = all.get("release") {
        ensure!(
            keys(&release["on"])? == BTreeSet::from(["workflow_dispatch".into()])
                && release["on"]["workflow_dispatch"]["inputs"]["publish"]["default"] == false
                && release["on"]["workflow_dispatch"]["inputs"]["bootstrap"]["default"] == false
                && release["jobs"]["release"]["if"] == "github.ref == 'refs/heads/main'"
                && release["concurrency"]["cancel-in-progress"] == false,
            "publication must be explicit and use main"
        );
        for name in [
            "Require passing CI for the exact release commit",
            "Publish the verified package set",
        ] {
            ensure!(
                release["jobs"]["release"]["steps"]
                    .as_array()
                    .context("release steps")?
                    .iter()
                    .any(|step| step["name"] == name && step["if"] == "inputs.publish"),
                "publication must require passing CI and explicit input"
            );
        }
    }
    Ok(())
}

fn keys(v: &Value) -> Result<BTreeSet<String>> {
    Ok(v.as_object()
        .context("expected map")?
        .keys()
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests;

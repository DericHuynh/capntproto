use crate::{file_hash, hash, process, read_json, remove, root, write, write_json};
use anyhow::{bail, ensure, Context, Result};
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
        "Clippy failed: {}",
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
    let mut automatic = Vec::new();
    for (name, w) in &all {
        if w["on"].get("push").is_some() || w["on"].get("pull_request").is_some() {
            automatic.push(name.as_str());
        }
        if let Some(push) = w["on"].get("push") {
            ensure!(
                push["branches"] == json!(["main"]) && w["on"].get("pull_request").is_some(),
                "{name}: overlapping push triggers"
            );
            ensure!(
                push.as_object()
                    .unwrap()
                    .keys()
                    .all(|k| ["branches", "paths"].contains(&k.as_str())),
                "invalid push filter"
            );
        }
    }
    ensure!(automatic == ["ci"], "CI must have one automatic entrypoint");
    let ci = &all["ci"];
    ensure!(
        ci["on"]["pull_request"].is_null(),
        "PR checks cannot be path filtered"
    );
    let jobs = &ci["jobs"];
    for (job, file) in [("workflows", "ci-workflows"), ("documentation", "ci-docs")] {
        ensure!(
            jobs[job]["uses"] == format!("./.github/workflows/{file}.yml")
                && all[file]["on"].get("workflow_call").is_some(),
            "reusable workflow dependency changed"
        );
    }
    ensure!(
        jobs["platforms"]["needs"] == "workflows" && jobs["report"]["if"] == "always()",
        "CI dependency gate changed"
    );
    ensure!(
        jobs["report"]["needs"] == json!(["workflows", "documentation", "platforms"]),
        "required CI result must depend on every gate"
    );
    let mut groups = BTreeSet::new();
    for name in ["ci", "ci-workflows", "ci-docs"] {
        let w = &all[name];
        ensure!(
            w["concurrency"]["cancel-in-progress"] == true,
            "PR checks must cancel stale runs"
        );
        for pr in [1, 2] {
            let key = concurrency(
                w,
                "pull_request",
                &format!("refs/pull/{pr}/merge"),
                Some(pr),
                ci["name"].as_str().unwrap(),
            )?;
            ensure!(groups.insert(key), "reusable workflow cancels caller");
        }
        let mut events = BTreeSet::new();
        for event in w["on"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|e| !["pull_request", "workflow_call"].contains(&e.as_str()))
        {
            ensure!(
                events.insert(concurrency(
                    w,
                    event,
                    "refs/heads/main",
                    None,
                    crate::string(w, "name")?
                )?),
                "event groups overlap"
            );
        }
    }
    for name in [
        "verification-tests",
        "verification-models",
        "verification-fuzz",
        "verification-extended",
    ] {
        ensure!(
            keys(&all[name]["on"])?
                == BTreeSet::from(["schedule".into(), "workflow_dispatch".into()]),
            "expensive checks must not run on push/PR"
        );
    }
    ensure!(
        keys(&all["performance"]["on"])? == BTreeSet::from(["workflow_dispatch".into()]),
        "benchmarks must remain manual"
    );
    let mut groups = BTreeSet::new();
    for name in ["performance", "maintenance-benchmarks", "reports"] {
        let w = &all[name];
        ensure!(
            w["concurrency"]["cancel-in-progress"] == false,
            "resource workflows must not cancel"
        );
        let key = concurrency(
            w,
            "workflow_dispatch",
            "refs/heads/main",
            None,
            crate::string(w, "name")?,
        )?;
        ensure!(
            key == concurrency(
                w,
                "schedule",
                "refs/heads/other",
                None,
                crate::string(w, "name")?
            )? && groups.insert(key),
            "resource groups overlap"
        );
    }
    let publishers = all
        .values()
        .filter(|w| w["on"].get("workflow_run").is_some())
        .collect::<Vec<_>>();
    ensure!(publishers.len() == 1, "expected one report publisher");
    let events = &publishers[0]["on"];
    ensure!(
        keys(events)? == BTreeSet::from(["workflow_run".into(), "workflow_dispatch".into()])
            && events["workflow_run"]["types"] == json!(["completed"]),
        "report trigger changed"
    );
    let names = [
        "verification-tests",
        "verification-models",
        "verification-fuzz",
        "performance",
    ]
    .into_iter()
    .map(|n| {
        all[n]["name"]
            .as_str()
            .unwrap()
            .chars()
            .flat_map(|c| {
                if r"\*+?![]".contains(c) {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect::<String>()
    })
    .collect::<BTreeSet<_>>();
    ensure!(
        events["workflow_run"]["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            == names,
        "publisher must name exact producers"
    );
    println!("Workflow triggers and dependencies passed");
    Ok(())
}
fn keys(v: &Value) -> Result<BTreeSet<String>> {
    Ok(v.as_object()
        .context("expected map")?
        .keys()
        .cloned()
        .collect())
}
fn concurrency(
    w: &Value,
    event: &str,
    reference: &str,
    pr: Option<u32>,
    name: &str,
) -> Result<String> {
    let regex = regex::Regex::new(r"\$\{\{\s*(.*?)\s*\}\}")?;
    let group = crate::string(&w["concurrency"], "group")?;
    let mut error = false;
    let result = regex
        .replace_all(group, |m: &regex::Captures| match &m[1] {
            "github.workflow" => name.into(),
            "github.event_name" => event.into(),
            "github.ref" => reference.into(),
            "github.event.pull_request.number || github.ref" => {
                pr.map(|p| p.to_string()).unwrap_or(reference.into())
            }
            _ => {
                error = true;
                String::new()
            }
        })
        .to_lowercase();
    if error {
        bail!("unsupported concurrency expression");
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

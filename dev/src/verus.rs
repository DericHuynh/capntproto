//! Source-bound deductive and temporal proofs. No deployed crate depends on Verus.
use crate::process::{self, Cancellation};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const VERSION: &str = "0.2026.10.04.426d8b0";
const COMMIT: &str = "426d8b01e7ffb910a36c15e1f870ff985bb61eef";
const BUNDLE_SHA: &str = "624018662868563a7bec0dd07be5e4dd0c3d61124b4b55105765884bf2d4d1a0";
const TLA_COMMIT: &str = "e7eda3f24b60fbbf4396e623a06da9ec32512c0e";
const SOURCES: &[&str] = &[
    "verification/verus/main.rs",
    "crates/capntproto-rpc-engine/src/question.rs",
    "crates/capntproto-rpc-engine/src/question/verified.rs",
    "crates/capntproto-rpc-engine/src/handoff.rs",
    "crates/capntproto-rpc-engine/src/handoff/verified.rs",
];

#[derive(clap::Subcommand)]
pub enum Args {
    /// Install the checksum-pinned Linux x86-64 release and its separate Rust toolchain.
    Install,
    /// Verify the temporal library, production source, and all deliberate faults.
    Verify {
        #[arg(long, default_value = "target/verification/verus")]
        output: PathBuf,
    },
}
pub fn run(args: Args) -> Result<()> {
    let cancel = Cancellation::install()?;
    match args {
        Args::Install => install(&crate::root(), &cancel),
        Args::Verify { output } => verify(&crate::root(), &output, &cancel),
    }
}
fn install(root: &Path, cancel: &Cancellation) -> Result<()> {
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "pinned Verus bundle requires Linux x86-64"
    );
    let dest = root.join("target/tools/verus");
    fs::create_dir_all(&dest)?;
    let archive = dest.join("release.zip");
    if !archive.exists() || crate::file_hash(&archive)? != BUNDLE_SHA {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()?;
        let url = format!("https://github.com/verus-lang/verus/releases/download/release/{VERSION}/verus-{VERSION}-x86-linux.zip");
        eprintln!("Downloading pinned Verus {VERSION}");
        let mut response = client.get(url).send()?.error_for_status()?;
        let staged = dest.join("release.download");
        response.copy_to(&mut fs::File::create(&staged)?)?;
        ensure!(
            crate::file_hash(&staged)? == BUNDLE_SHA,
            "Verus bundle checksum mismatch"
        );
        fs::rename(staged, &archive)?;
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let path = entry
            .enclosed_name()
            .context("archive path escapes tool directory")?;
        ensure!(
            entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "archive contains symlink"
        );
        for ancestor in dest.join(path).ancestors() {
            ensure!(!ancestor.is_symlink(), "tool destination contains symlink");
        }
    }
    zip.extract(&dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["verus", "rust_verify", "z3", "cargo-verus"] {
            fs::set_permissions(
                dest.join("verus-x86-linux").join(name),
                fs::Permissions::from_mode(0o755),
            )?;
        }
    }
    process::live(
        Command::new("rustup").args(["toolchain", "install", "1.98.1", "--profile", "minimal"]),
        600,
        cancel,
    )?;
    Ok(())
}

fn functions(report: &Value) -> Result<BTreeMap<String, Value>> {
    let modules = report["times-ms"]["smt"]["smt-run-module-times"]
        .as_array()
        .context("missing SMT execution details")?;
    let mut result = BTreeMap::new();
    for module in modules {
        for row in module["function-breakdown"]
            .as_array()
            .context("missing function results")?
        {
            let name = crate::string(row, "function")?.to_owned();
            ensure!(
                result.insert(name, row.clone()).is_none(),
                "duplicate function result"
            );
        }
    }
    Ok(result)
}
fn validate(
    report: &Value,
    code: i32,
    count: usize,
    expected: Option<&BTreeSet<String>>,
    negative: Option<&str>,
) -> Result<()> {
    let summary = &report["verification-results"];
    ensure!(
        report["verus"]["version"] == VERSION && report["verus"]["commit"] == COMMIT,
        "unexpected Verus version"
    );
    ensure!(
        summary["encountered-vir-error"] == false && summary["is-verifying-entire-crate"] == true,
        "incomplete verification or invalid Rust/Verus program"
    );
    let rows = functions(report)?;
    ensure!(
        rows.len() == count,
        "missing or unexpected verified functions"
    );
    if let Some(expected) = expected {
        ensure!(
            rows.keys().cloned().collect::<BTreeSet<_>>() == *expected,
            "proof inventory differs from plan"
        );
    }
    if let Some(name) = negative {
        ensure!(
            code != 0
                && summary["success"] == false
                && crate::number(summary, "errors")? > 0
                && rows.get(name).is_some_and(|v| v["success"] == false),
            "intended proof did not reject mutation: {name}"
        );
        // Timeouts and solver failures must not masquerade as counterexamples.
        ensure!(
            rows.values().all(|v| v["success"].is_boolean()),
            "incomplete mutation results"
        );
    } else {
        ensure!(
            code == 0
                && summary["success"] == true
                && summary["encountered-error"] == false
                && summary["errors"] == 0
                && summary["verified"] == count
                && rows.values().all(|v| v["success"] == true),
            "Verus proof failed"
        );
    }
    Ok(())
}
fn control_source(root: &Path, control: &Value) -> Result<(PathBuf, String)> {
    let source = crate::string(control, "source")?;
    ensure!(
        SOURCES.contains(&source),
        "mutation source is outside verified inventory"
    );
    let before = fs::read_to_string(root.join(source))?;
    let from = crate::string(control, "from")?;
    ensure!(
        !from.is_empty() && before.matches(from).count() == 1,
        "mutation must match exactly one source site"
    );
    let to = crate::string(control, "to")?;
    ensure!(from != to, "mutation does not change source");
    Ok((PathBuf::from(source), before.replacen(from, to, 1)))
}
fn plan(root: &Path) -> Result<(Value, BTreeSet<String>)> {
    let plan = crate::read_json(root.join("verification/verus/plan.json"))?;
    ensure!(plan["format"] == 1, "unsupported Verus proof plan");
    let functions = plan["functions"]
        .as_array()
        .context("missing proof inventory")?;
    let expected: BTreeSet<_> = functions
        .iter()
        .map(|v| v.as_str().context("invalid function").map(str::to_owned))
        .collect::<Result<_>>()?;
    ensure!(
        !expected.is_empty() && expected.len() == functions.len(),
        "empty/duplicate proof inventory"
    );
    let mut names = BTreeSet::new();
    let controls = plan["controls"].as_array().context("missing controls")?;
    ensure!(!controls.is_empty(), "empty controls");
    for control in controls {
        let name = crate::string(control, "name")?;
        ensure!(
            !name.is_empty()
                && name.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
                && names.insert(name),
            "invalid/duplicate control name"
        );
        ensure!(
            expected.contains(crate::string(control, "function")?),
            "control lacks a positive proof"
        );
        control_source(root, control)?;
    }
    Ok((plan, expected))
}
fn inputs(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut inputs = BTreeMap::new();
    for name in SOURCES.iter().copied().chain([
        "verification/verus/plan.json",
        "dev/src/verus.rs",
        "dev/src/verus/tests.rs",
        "dev/src/main.rs",
        "dev/src/lib.rs",
        "dev/src/process.rs",
        "Cargo.lock",
        "dev/Cargo.toml",
        "crates/capntproto-rpc-engine/Cargo.toml",
        ".github/workflows/verification-models.yml",
    ]) {
        inputs.insert(name.into(), crate::file_hash(root.join(name))?);
    }
    for entry in walkdir::WalkDir::new(root.join("vendor/verus-tla/src")) {
        let entry = entry?;
        if entry.file_type().is_file() {
            inputs.insert(
                entry
                    .path()
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .into_owned(),
                crate::file_hash(entry.path())?,
            );
        }
    }
    Ok(inputs)
}
struct Verifier<'a> {
    binary: PathBuf,
    output: &'a Path,
    cancel: &'a Cancellation,
}
impl Verifier<'_> {
    fn invoke(&self, root: &Path, name: &str, library: bool) -> Result<(i32, Value)> {
        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(root).args([
            "--edition=2021",
            "--no-cheating",
            "--num-threads",
            "2",
            "--output-json",
            "--time",
        ]);
        for key in [
            "RUSTUP_TOOLCHAIN",
            "VERUS_Z3_PATH",
            "VERUS_SINGULAR_PATH",
            "RUSTFLAGS",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
        ] {
            cmd.env_remove(key);
        }
        let rlib = self.output.join("libverus_temporal_logic.rlib");
        let vir = self.output.join("temporal.vir");
        if library {
            cmd.args([
                "--crate-type=lib",
                "--crate-name",
                "verus_temporal_logic",
                "--compile",
                "--export",
            ])
            .arg(&vir)
            .arg("-o")
            .arg(&rlib)
            .arg("vendor/verus-tla/src/lib.rs");
        } else {
            cmd.args([
                "--crate-name",
                "capntproto_proofs",
                "--cfg",
                "capntproto_verus",
                "--extern",
            ])
            .arg(format!("verus_temporal_logic={}", rlib.display()))
            .arg("--import")
            .arg(format!("verus_temporal_logic={}", vir.display()))
            .arg("verification/verus/main.rs");
        }
        eprintln!("Verus: {name}");
        let result = process::run(&mut cmd, 600, None, self.cancel)?;
        crate::write(self.output.join(format!("{name}.log")), &result.stderr)?;
        crate::write(self.output.join(format!("{name}.json")), &result.stdout)?;
        let report = serde_json::from_slice(&result.stdout)
            .with_context(|| format!("Verus {name} produced no readable report; see {name}.log"))?;
        if result.code != 0 {
            let diagnostics = String::from_utf8_lossy(&result.stderr);
            // A negative control must fail a logical obligation, not just fail.
            ensure!(
                !diagnostics.contains("resource limit")
                    && !diagnostics.contains("timed out")
                    && !diagnostics.contains("internal error")
                    && !diagnostics.contains("panicked at"),
                "verifier failed without a logical counterexample"
            );
            ensure!(
                diagnostics.contains("postcondition not satisfied")
                    || diagnostics.contains("precondition not satisfied")
                    || diagnostics.contains("assertion failed"),
                "no failed logical obligation; see {name}.log"
            );
        }
        Ok((result.code, report))
    }
}
fn verify(root: &Path, output: &Path, cancel: &Cancellation) -> Result<()> {
    fs::create_dir_all(output)?;
    let output = output.canonicalize()?;
    let mut evidence = json!({"format":1,"passed":false,"error":"run incomplete","controls":[]});
    crate::write_json(output.join("evidence.json"), &evidence)?;
    crate::write(
        output.join("README.md"),
        "# Verus and verus-tla\n\nRun incomplete.\n",
    )?;
    crate::remove(output.join("proofs.svg"))?;
    let result = (|| -> Result<()> {
        let (plan, expected) = plan(root)?;
        let source = inputs(root)?;
        evidence["sources"] = json!(source);
        evidence["source_id"] = crate::hash(serde_json::to_vec(&source)?).into();
        let tla = root.join("vendor/verus-tla");
        ensure!(
            process::text(
                Command::new("git")
                    .arg("-C")
                    .arg(&tla)
                    .args(["rev-parse", "HEAD"])
            )? == TLA_COMMIT,
            "initialize pinned vendor/verus-tla submodule"
        );
        ensure!(
            process::text(
                Command::new("git")
                    .arg("-C")
                    .arg(&tla)
                    .args(["status", "--porcelain"])
            )?
            .is_empty(),
            "verus-tla submodule is modified"
        );
        evidence["verus_tla_commit"] = TLA_COMMIT.into();
        let verifier = Verifier {
            binary: root.join("target/tools/verus/verus-x86-linux/verus"),
            output: &output,
            cancel,
        };
        evidence["binary_sha256"] = crate::file_hash(&verifier.binary)
            .context("run `cargo run --locked -p capntproto-dev -- verus install` first")?
            .into();
        let tool_dir = verifier.binary.parent().unwrap();
        for file in ["rust_verify", "z3", "libvstd.rlib", "vstd.vir"] {
            evidence["tool_hashes"][file] = crate::file_hash(tool_dir.join(file))?.into();
        }
        crate::write_json(output.join("evidence.json"), &evidence)?;
        let (code, library) = verifier.invoke(root, "temporal-library", true)?;
        validate(
            &library,
            code,
            crate::number(&plan, "library_verified")? as usize,
            None,
            None,
        )?;
        evidence["library"] = library["verification-results"].clone();
        crate::write_json(output.join("evidence.json"), &evidence)?;
        let (code, report) = verifier.invoke(root, "proofs", false)?;
        validate(&report, code, expected.len(), Some(&expected), None)?;
        graph(&report, &output)?;
        evidence["proofs"] = report["verification-results"].clone();
        crate::write_json(output.join("evidence.json"), &evidence)?;
        for control in plan["controls"].as_array().unwrap() {
            let copy = tempfile::tempdir()?;
            for source in SOURCES {
                crate::write(copy.path().join(source), fs::read(root.join(source))?)?;
            }
            let (path, mutated) = control_source(root, control)?;
            crate::write(copy.path().join(path), mutated)?;
            let name = crate::string(control, "name")?;
            let (code, report) = verifier.invoke(copy.path(), name, false)?;
            validate(
                &report,
                code,
                expected.len(),
                Some(&expected),
                Some(crate::string(control, "function")?),
            )?;
            evidence["controls"]
                .as_array_mut()
                .unwrap()
                .push(json!({"name":name,"detected":true}));
            crate::write_json(output.join("evidence.json"), &evidence)?;
        }
        ensure!(
            source == inputs(root)?,
            "proof inputs changed during verification"
        );
        let mut hashes = BTreeMap::new();
        for name in ["temporal-library", "proofs"].into_iter().chain(
            plan["controls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["name"].as_str().unwrap()),
        ) {
            for ext in ["json", "log"] {
                let file = format!("{name}.{ext}");
                hashes.insert(file.clone(), crate::file_hash(output.join(file))?);
            }
        }
        evidence["reports"] = json!(hashes);
        evidence["passed"] = true.into();
        evidence["error"] = Value::Null;
        Ok(())
    })();
    if let Err(error) = &result {
        evidence["error"] = format!("{error:#}").into();
    }
    crate::write_json(output.join("evidence.json"), &evidence)?;
    let status = if result.is_ok() { "PASS" } else { "FAIL" };
    let details = if result.is_ok() {
        "All production contracts, temporal lemmas, and negative controls passed.".to_owned()
    } else {
        evidence["error"].as_str().unwrap().to_owned()
    };
    crate::write(output.join("README.md"), format!("# Verus and verus-tla: {status}\n\n{details}\n\nSee [production proofs](proofs.json), [temporal library](temporal-library.json) and [source-bound evidence](evidence.json). Each JSON report includes per-function SMT timings. Control failures are expected and their logs identify the violated obligation.\n\n![SMT time per production contract and temporal lemma](proofs.svg)\n\nScope: local handoff and outgoing-question policies, full-width counters and unbounded traces. Progress assumes fair draining/lifting or Return processing/local release. Authentication, network delivery, output acceptance, table generations and constructor/Default wrappers are outside this Verus proof; Kani retains the wrapper and ledger checks.\n"))?;
    result
}

fn graph(report: &Value, output: &Path) -> Result<()> {
    let rows = functions(report)?;
    let max = rows
        .values()
        .map(|r| crate::number(r, "time-micros"))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(1)
        .max(1);
    let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{}\" role=\"img\"><title>Verus SMT time by function</title><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g font-family=\"monospace\" font-size=\"12\" fill=\"#172332\"><text x=\"12\" y=\"20\">SMT microseconds per verified function (not source coverage)</text>", 45 + rows.len()*30);
    for (i, (name, row)) in rows.iter().enumerate() {
        let label = html_escape::encode_text(name.trim_start_matches("capntproto_proofs::"));
        let us = crate::number(row, "time-micros")?;
        svg += &format!("<text x=\"12\" y=\"{}\">{label}</text><rect x=\"550\" y=\"{}\" width=\"{}\" height=\"16\" fill=\"#267c61\"/><text x=\"920\" y=\"{}\">{us}</text>", 45+i*30, 33+i*30, 350u128*u128::from(us)/u128::from(max), 45+i*30);
    }
    svg += "</g></svg>\n";
    crate::write(output.join("proofs.svg"), svg)
}

#[cfg(test)]
mod tests;

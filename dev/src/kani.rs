//! Pinned, source-bound Kani proofs and isolated production-code negative controls.
use crate::process::{self, Cancellation};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "target/verification/kani")]
    pub output: PathBuf,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=16))]
    pub jobs: u8,
}

fn inputs(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for package in ["capntproto-rpc-engine", "capntproto-core"] {
        for entry in walkdir::WalkDir::new(root.join("crates").join(package)) {
            let entry = entry?;
            if entry.file_type().is_file() {
                result.insert(
                    entry
                        .path()
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .into_owned(),
                    crate::file_hash(entry.path())?,
                );
            }
        }
    }
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "verification/kani.json",
        "dev/src/kani.rs",
    ] {
        result.insert(name.into(), crate::file_hash(root.join(name))?);
    }
    Ok(result)
}

fn names(value: &Value) -> Result<BTreeSet<String>> {
    value
        .as_array()
        .context("missing harness inventory")?
        .iter()
        .map(|v| Ok(v.as_str().context("invalid harness name")?.to_owned()))
        .collect()
}

/// Reject empty/partial success, missing proofs, incompatible tools, and controls
/// that merely fail to compile or time out instead of finding the intended bug.
fn validate(value: &Value, expected: &BTreeSet<String>, negative: Option<&str>) -> Result<()> {
    ensure!(!expected.is_empty(), "empty Kani inventory");
    ensure!(
        value["metadata"]["version"] == "1.0"
            && value["tools"]["kani"] == "0.68.0"
            && value["metadata"]["target"] == "x86_64-unknown-linux-gnu",
        "unexpected Kani report/toolchain"
    );
    let summary = &value["verification_results"]["summary"];
    ensure!(
        summary["status"] == "completed"
            && summary["executed"] == expected.len()
            && summary["total_harnesses"] == expected.len(),
        "incomplete Kani execution"
    );
    let rows = value["verification_results"]["results"]
        .as_array()
        .context("missing proof results")?;
    let actual: BTreeSet<_> = rows
        .iter()
        .map(|r| crate::string(r, "harness_id").map(str::to_owned))
        .collect::<Result<_>>()?;
    ensure!(
        actual == *expected && rows.len() == expected.len(),
        "missing, duplicate or unexpected proof"
    );
    let metadata = value["harness_metadata"]
        .as_array()
        .context("missing harness metadata")?;
    let declared: BTreeSet<_> = metadata
        .iter()
        .map(|r| crate::string(r, "pretty_name").map(str::to_owned))
        .collect::<Result<_>>()?;
    ensure!(
        declared == *expected
            && metadata.len() == expected.len()
            && metadata
                .iter()
                .all(|r| r["attributes"]["should_panic"] == false),
        "unexpected proof attributes/inventory"
    );
    match negative {
        None => {
            ensure!(
                summary["failed"] == 0 && summary["successful"] == expected.len(),
                "Kani proof failed"
            );
            for row in rows {
                let checks = row["checks"].as_array().context("missing proof checks")?;
                ensure!(
                    row["status"] == "Success"
                        && !checks.is_empty()
                        && checks
                            .iter()
                            .all(|c| c["status"] == "Success" || c["status"] == "Unreachable"),
                    "failed or incomplete proof checks"
                );
            }
        }
        Some(message) => {
            ensure!(
                expected.len() == 1 && summary["failed"] == 1 && summary["successful"] == 0,
                "mutation was not detected"
            );
            let checks = rows[0]["checks"]
                .as_array()
                .context("mutation produced no checks")?;
            ensure!(
                rows[0]["status"] == "Failure"
                    && checks.iter().any(|c| c["status"] == "Failure"
                        && c["category"] == "assertion"
                        && c["description"]
                            .as_str()
                            .is_some_and(|s| s.contains(message))),
                "intended mutation assertion was not detected"
            );
        }
    }
    Ok(())
}

fn invoke(
    root: &Path,
    output: &Path,
    name: &str,
    manifest: Option<&Path>,
    harness: Option<&str>,
    jobs: u8,
    cancel: &Cancellation,
) -> Result<(i32, Value)> {
    let path = output.join(format!("{name}.json"));
    crate::remove(&path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([
            "kani",
            "--no-default-features",
            "--output-format",
            "terse",
            "-Z",
            "unstable-options",
            "--harness-timeout",
            "180s",
            "--export-json",
        ])
        .arg(&path)
        .args(["-j", &jobs.to_string()])
        .env("CARGO_NET_OFFLINE", "true");
    for key in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "KANIFLAGS",
    ] {
        command.env_remove(key);
    }
    if let Some(manifest) = manifest {
        command.arg("--manifest-path").arg(manifest);
    } else {
        command.args(["-p", "capntproto-rpc-engine", "-p", "capntproto-core"]);
    }
    if let Some(harness) = harness {
        command.args(["--harness", harness, "--exact"]);
    }
    eprintln!("Kani: {name}");
    let result = process::run(
        &mut command,
        1800,
        Some(&output.join(format!("{name}.log"))),
        cancel,
    )?;
    let data = crate::read_json(&path).with_context(|| {
        format!(
            "Kani {name} exited with code {} without readable results at {}; see {}",
            result.code,
            path.display(),
            output.join(format!("{name}.log")).display()
        )
    })?;
    Ok((result.code, data))
}

pub fn run(args: Args) -> Result<()> {
    let mut cancel = None;
    run_using(
        &crate::root(),
        args,
        || process::text(Command::new("cargo").args(["kani", "--version"])),
        |root, output, name, manifest, harness, jobs| {
            if cancel.is_none() {
                cancel = Some(Cancellation::install()?);
            }
            invoke(
                root,
                output,
                name,
                manifest,
                harness,
                jobs,
                cancel.as_ref().unwrap(),
            )
        },
    )
}

/// Catch invalid plans and stale mutation anchors before the expensive verifier
/// campaign. Each mutation is still applied and rechecked in an isolated copy.
fn preflight_controls(root: &Path, plan: &Value, expected: &BTreeSet<String>) -> Result<()> {
    let mut names = BTreeSet::new();
    for control in plan["controls"]
        .as_array()
        .context("missing negative controls")?
    {
        let name = crate::string(control, "name")?;
        ensure!(
            !name.is_empty() && name.bytes().all(|c| c.is_ascii_lowercase() || c == b'-'),
            "invalid control name"
        );
        ensure!(names.insert(name), "duplicate control name");
        let package = control
            .get("package")
            .map(|p| p.as_str().context("invalid mutation package"))
            .transpose()?
            .unwrap_or("capntproto-rpc-engine");
        ensure!(
            matches!(package, "capntproto-rpc-engine" | "capntproto-core"),
            "unsupported mutation package"
        );
        let source = crate::string(control, "source")?;
        ensure!(
            source.starts_with("src/")
                && Path::new(source)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "invalid mutation source"
        );
        let before = fs::read_to_string(root.join("crates").join(package).join(source))?;
        let from = crate::string(control, "from")?;
        ensure!(
            !from.is_empty() && before.matches(from).count() == 1,
            "mutation must match exactly one production site"
        );
        crate::string(control, "to")?;
        ensure!(
            expected.contains(crate::string(control, "harness")?),
            "control has no positive proof"
        );
        ensure!(
            !crate::string(control, "expected_check")?.is_empty(),
            "missing expected mutation assertion"
        );
    }
    Ok(())
}

fn run_using<V, F>(root: &Path, args: Args, version: V, mut verify: F) -> Result<()>
where
    V: FnOnce() -> Result<String>,
    F: FnMut(&Path, &Path, &str, Option<&Path>, Option<&str>, u8) -> Result<(i32, Value)>,
{
    fs::create_dir_all(&args.output)?;
    let output = args.output.canonicalize()?;
    let source = inputs(root)?;
    let source_id = crate::hash(serde_json::to_vec(&source)?);
    let mut evidence = json!({"format":1,"source_id":source_id,"sources":source,"passed":false,"error":"run incomplete","controls":[]});
    crate::write_json(output.join("evidence.json"), &evidence)?;
    crate::write(
        output.join("README.md"),
        "# Kani verification\n\nRun incomplete.\n",
    )?;
    crate::remove(output.join("proofs.svg"))?;
    let result = (|| -> Result<()> {
        let plan = crate::read_json(root.join("verification/kani.json"))?;
        ensure!(
            plan["format"] == 1 && plan["kani_version"] == "0.68.0",
            "unsupported proof plan"
        );
        let expected = names(&plan["harnesses"])?;
        ensure!(
            expected.len() == plan["harnesses"].as_array().unwrap().len(),
            "duplicate proof in inventory"
        );
        preflight_controls(root, &plan, &expected)?;
        let version = version()?;
        ensure!(
            version.lines().next() == Some("Kani Rust Verifier 0.68.0 (cargo plugin)"),
            "install pinned kani-verifier 0.68.0"
        );
        let (code, data) = verify(root, &output, "proofs", None, None, args.jobs)?;
        ensure!(
            code == 0,
            "production proofs failed; see proofs.log and proofs.json"
        );
        validate(&data, &expected, None)?;
        evidence["tools"] = data["tools"].clone();
        evidence["summary"] = data["verification_results"]["summary"].clone();
        evidence["proofs_sha256"] = crate::file_hash(output.join("proofs.json"))?.into();
        for control in plan["controls"]
            .as_array()
            .context("missing negative controls")?
        {
            let name = crate::string(control, "name")?;
            ensure!(
                !name.is_empty() && name.bytes().all(|c| c.is_ascii_lowercase() || c == b'-'),
                "invalid control name"
            );
            let copy = tempfile::tempdir()?;
            let package = control
                .get("package")
                .map(|p| p.as_str().context("invalid mutation package"))
                .transpose()?
                .unwrap_or("capntproto-rpc-engine");
            ensure!(
                matches!(package, "capntproto-rpc-engine" | "capntproto-core"),
                "unsupported mutation package"
            );
            let package_root = root.join("crates").join(package);
            for entry in walkdir::WalkDir::new(&package_root) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    crate::write(
                        copy.path().join(entry.path().strip_prefix(&package_root)?),
                        fs::read(entry.path())?,
                    )?;
                }
            }
            let source = crate::string(control, "source")?;
            ensure!(
                source.starts_with("src/")
                    && Path::new(source)
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_))),
                "invalid mutation source"
            );
            let path = copy.path().join(source);
            let before = fs::read_to_string(&path)?;
            let from = crate::string(control, "from")?;
            ensure!(
                !from.is_empty() && before.matches(from).count() == 1,
                "mutation must match exactly one production site"
            );
            crate::write(
                &path,
                before.replacen(from, crate::string(control, "to")?, 1),
            )?;
            let harness = crate::string(control, "harness")?;
            ensure!(expected.contains(harness), "control has no positive proof");
            let (code, mutated) = verify(
                root,
                &output,
                name,
                Some(&copy.path().join("Cargo.toml")),
                Some(harness),
                1,
            )?;
            ensure!(code != 0, "mutation unexpectedly passed");
            validate(
                &mutated,
                &BTreeSet::from([harness.to_owned()]),
                Some(crate::string(control, "expected_check")?),
            )?;
            evidence["controls"].as_array_mut().unwrap().push(json!({"name":name,"detected":true,"json_sha256":crate::file_hash(output.join(format!("{name}.json")))?}));
        }
        Ok(())
    })();
    let result = result.and_then(|()| {
        ensure!(
            inputs(root)? == source,
            "proof inputs changed during verification"
        );
        Ok(())
    });
    evidence["passed"] = result.is_ok().into();
    evidence["error"] = result.as_ref().err().map(|e| e.to_string()).into();
    crate::write_json(output.join("evidence.json"), &evidence)?;
    let status = if result.is_ok() {
        "Passed"
    } else {
        "Failed / incomplete"
    };
    let failures = result.as_ref().err().map(|e| e.to_string()).unwrap_or_else(|| "No unexpected failures. All required production mutations triggered their specified assertions.".into());
    crate::write(output.join("FAILED-PROOFS.md"), format!("# Failed proofs\n\n{failures}\n\n[Production log](proofs.log) · [Full checks and counterexamples](proofs.json). Mutation logs and JSON are retained beside this report.\n"))?;
    crate::write(output.join("README.md"), format!("# Kani verification\n\nResult: **{status}**. Source fingerprint: `{source_id}`.\n\n[Failed proofs](FAILED-PROOFS.md) · [Raw checks](proofs.json) · [Source/tool identities and controls](evidence.json).\n\n![Per-harness verification time](proofs.svg)\n\nScope: production portable-engine code and the core fixed arena. Growing-storage arithmetic is checked for every usize input; allocator behavior and relocated ownership are covered by native tests and Miri, not claimed as Kani proofs. Borrowed reply-slot checks cover arbitrary u8 values, producer cancellation, receiver closure before and after delivery, once-only recovery and storage reuse. Tracked reply proofs additionally cover duplicate sends, live receiver ownership, abandonment and explicit shutdown recovery; waker callbacks are covered by native reentry tests. Handoff induction covers all valid u64 states. Dense-table induction uses three slots. Sparse proofs use at most two slots. Question histories are eight operations. Scoped questions additionally retain wire IDs through late Return and rejected Finish output; a one-slot proof rejects premature reuse. Incoming answers retain a pipeline owner across Finish and Return, with cancellation policy checked independently of transport; a two-slot proof checks early-Finish dispatch and safe wire-ID reuse while an old pipeline retains its owner. Incoming call preflight checks a new answer together with its parent pipeline dependency; arbitrary u32 reference counts and u64 generations cannot partially commit or wrap. Implicit export-release batches validate every full-width count before applying any release and retain retired owners for explicit cleanup. Atomic import admission covers two repeated descriptors and every valid full-width local/remote count and generation, preserving the whole prefix on failure. Wire-reference transitions cover all u32 counts, failed admission/release, terminal exports and explicit cleanup acceptance. Bounded import ownership covers a one-slot lifecycle with arbitrary u64 generations and full-width count admission; scope separation is a compiler contract. Export proofs cover a one-slot publication/abort lifecycle, call leases across final peer release, generation exhaustion and full-width staged grants. Message proofs cover one two-byte buffer and one send with arbitrary generations, plus completion over up to four bytes with arbitrary reader and completion counts. They preserve buffers through queue acceptance and partial writes, and reject stale completions. Two-buffer proofs retain arbitrary request and mutable result bytes while independently sending and recycling output with the production borrow cells. An owning-writer proof preserves arbitrary initialized bytes across independent buffer use and explicit publication. Descriptor matching covers two slots and up to three full-width wire IDs, plus arbitrary u32 count admission and sticky rejection. A two-frame FIFO proof preserves the oldest unsent suffix through partial writes. Promise admission and resolution preserve arbitrary positive wire counts; accepted Resolve commits exactly once and rejected output remains retryable. A one-slot embargo proof covers every u32 starting ID, cancellation, rejection, exhaustion and stale echoes after storage reuse. Reflected export and answer descriptor batches check two repeated local references against every u32 count; the answer proof also checks joint insertion at every valid u64 generation. One-slot pipeline reflection observes noFinishNeeded while references remain live. Same-connection tail adoption covers every u32 dependency count: rejection preserves state, acceptance consumes the redirect once and retains its producer through Finish and Return. Three-slot graph proofs preserve frozen remote endpoints, reflected loopback barriers, and every positive u32 local reference count. A two-slot imported-promise proof checks all u64 generation pairs, preserving owner and storage on rejection and separating the promise from its immutable endpoint. A two-task dispatcher proof starts from a reachable reused-slot state with two distinct owners, then checks output rejection, FIFO selection and stale cancellation. A one-slot borrowed-dispatch proof checks identity continuity across scopes from every u64 sequence permitting two admissions. Terminal proofs transfer retained ledger owners once, prevent endpoint extraction before queued tasks drain, and preserve independent readers when failed output discards a partial send. A one-byte buffered-output proof covers Pending, Interrupted, failure and successful flush, requiring flush completion before readiness and accepting attached resources only once. The RPC frame decoder, full driver teardown, publication transaction and complete async coordinator are outside these proofs. Fixed-arena proofs use aligned byte storage, one to four words and two full-width reservation attempts, then reset/drop, plus unpublished-suffix rollback and consuming publication that preserves its initialized prefix; they do not prove wire construction or Word-to-byte conversion. No distributed liveness, arbitrary-capacity proof or complete no-alloc RPC claim. Native allocation counters, compile-fail contracts, Miri and bare-metal linking remain separate checks.\n"))?;
    if result.is_ok() {
        let data = crate::read_json(output.join("proofs.json"))?;
        let rows = data["verification_results"]["results"].as_array().unwrap();
        let max = rows
            .iter()
            .filter_map(|r| r["duration_ms"].as_u64())
            .max()
            .unwrap_or(1)
            .max(1);
        let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"{}\" role=\"img\"><title>Kani verification time by harness</title><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g font-family=\"monospace\" font-size=\"11\" fill=\"#172332\"><text x=\"12\" y=\"20\">Successful production proofs — milliseconds (not source coverage)</text>", 45 + rows.len()*32);
        for (i, row) in rows.iter().enumerate() {
            let ms = crate::number(row, "duration_ms")?;
            let name = crate::string(row, "harness_id")?;
            // Names were checked against the source-controlled inventory above.
            ensure!(
                name.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b':'),
                "unsafe chart label"
            );
            svg += &format!("<text x=\"12\" y=\"{}\">{name}</text><rect x=\"610\" y=\"{}\" width=\"{}\" height=\"16\" fill=\"#267c61\"/><text x=\"875\" y=\"{}\">{ms}</text>",45+i*32,33+i*32,250*ms/max,45+i*32);
        }
        svg += "</g></svg>\n";
        crate::write(output.join("proofs.svg"), svg)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(root: &Path, controls: bool) {
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            "dev/src/kani.rs",
            "crates/capntproto-rpc-engine/Cargo.toml",
            "crates/capntproto-core/Cargo.toml",
        ] {
            crate::write(root.join(file), "report lifecycle fixture\n").unwrap();
        }
        crate::write(
            root.join("crates/capntproto-rpc-engine/src/lib.rs"),
            "VALID_GUARD\n",
        )
        .unwrap();
        crate::write_json(root.join("verification/kani.json"), &json!({"format":1,"kani_version":"0.68.0","harnesses":["proof"],"controls":if controls {json!([{"name":"broken-guard","source":"src/lib.rs","from":"VALID_GUARD","to":"BROKEN_GUARD","harness":"proof","expected_check":"expected property"}])} else {json!([])}})).unwrap();
    }

    fn version() -> Result<String> {
        Ok("Kani Rust Verifier 0.68.0 (cargo plugin)".into())
    }

    #[test]
    fn production_mutations_match_current_source_before_verification() {
        let root = crate::root();
        let plan = crate::read_json(root.join("verification/kani.json")).unwrap();
        preflight_controls(&root, &plan, &names(&plan["harnesses"]).unwrap()).unwrap();
    }

    #[test]
    fn invalid_mutation_plans_fail_before_running_the_verifier() {
        for (field, value, expected_error) in [
            (
                "source",
                "crates/capntproto-rpc-engine/src/lib.rs",
                "invalid mutation source",
            ),
            ("source", "src/../Cargo.toml", "invalid mutation source"),
            (
                "from",
                "REMOVED_GUARD",
                "mutation must match exactly one production site",
            ),
            ("harness", "missing", "control has no positive proof"),
            ("expected_check", "", "missing expected mutation assertion"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            fixture(root, true);
            let path = root.join("verification/kani.json");
            let mut plan = crate::read_json(&path).unwrap();
            plan["controls"][0][field] = value.into();
            crate::write_json(path, &plan).unwrap();
            let output = root.join("report");
            let result = run_using(
                root,
                Args {
                    output: output.clone(),
                    jobs: 1,
                },
                version,
                |_, _, _, _, _, _| panic!("invalid plan must not run verifier"),
            );
            assert_eq!(result.unwrap_err().to_string(), expected_error);
            assert_eq!(
                crate::read_json(output.join("evidence.json")).unwrap()["passed"],
                false
            );
        }
    }

    // Fixtures exercise report orchestration only; actual proofs are the separate
    // pinned Kani gate. No fixture output leaves these temporary directories.
    #[test]
    fn reporting_keeps_mutations_isolated_and_invalidates_stale_success() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root, true);
        let output = root.join("report");
        run_using(
            root,
            Args {
                output: output.clone(),
                jobs: 2,
            },
            version,
            |_, out, name, manifest, harness, _| {
                let mut data = report();
                let negative = manifest.is_some();
                if let Some(manifest) = manifest {
                    assert_eq!(harness, Some("proof"));
                    assert_eq!(
                        fs::read_to_string(manifest.parent().unwrap().join("src/lib.rs")).unwrap(),
                        "BROKEN_GUARD\n"
                    );
                    data["verification_results"]["summary"]["failed"] = 1.into();
                    data["verification_results"]["summary"]["successful"] = 0.into();
                    data["verification_results"]["results"][0]["status"] = "Failure".into();
                    data["verification_results"]["results"][0]["checks"][0]["status"] =
                        "Failure".into();
                }
                crate::write_json(out.join(format!("{name}.json")), &data)?;
                Ok((i32::from(negative), data))
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("crates/capntproto-rpc-engine/src/lib.rs")).unwrap(),
            "VALID_GUARD\n"
        );
        let evidence = crate::read_json(output.join("evidence.json")).unwrap();
        assert_eq!(evidence["passed"], true);
        assert_eq!(evidence["controls"][0]["detected"], true);
        assert!(output.join("proofs.svg").exists());
        assert!(run_using(
            root,
            Args {
                output: output.clone(),
                jobs: 1
            },
            || Ok("wrong version".into()),
            |_, _, _, _, _, _| panic!("wrong tool must not execute")
        )
        .is_err());
        assert_eq!(
            crate::read_json(output.join("evidence.json")).unwrap()["passed"],
            false
        );
        assert!(!output.join("proofs.svg").exists());
        assert!(fs::read_to_string(output.join("FAILED-PROOFS.md"))
            .unwrap()
            .contains("install pinned"));
    }

    #[test]
    fn source_changes_and_missing_exports_cannot_certify_a_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root, false);
        let output = root.join("report");
        let result = run_using(
            root,
            Args {
                output: output.clone(),
                jobs: 1,
            },
            version,
            |_, out, name, _, _, _| {
                crate::write(root.join("Cargo.lock"), "changed while verifier ran")?;
                let data = report();
                crate::write_json(out.join(format!("{name}.json")), &data)?;
                Ok((0, data))
            },
        );
        assert!(result.unwrap_err().to_string().contains("inputs changed"));
        assert_eq!(
            crate::read_json(output.join("evidence.json")).unwrap()["passed"],
            false
        );
        assert!(run_using(
            root,
            Args {
                output: output.clone(),
                jobs: 1
            },
            version,
            |_, _, _, _, _, _| anyhow::bail!("verifier produced no export")
        )
        .is_err());
        assert!(fs::read_to_string(output.join("FAILED-PROOFS.md"))
            .unwrap()
            .contains("no export"));
    }
    fn report() -> Value {
        json!({"metadata":{"version":"1.0","target":"x86_64-unknown-linux-gnu"},"tools":{"kani":"0.68.0"},
          "harness_metadata":[{"pretty_name":"proof","attributes":{"should_panic":false}}],
          "verification_results":{"summary":{"status":"completed","executed":1,"total_harnesses":1,"successful":1,"failed":0},
          "results":[{"harness_id":"proof","status":"Success","duration_ms":1,"checks":[{"status":"Success","category":"assertion","description":"expected property"}]}]}})
    }
    #[test]
    fn reports_reject_missing_partial_or_false_success() {
        let expected = BTreeSet::from(["proof".into()]);
        assert!(validate(&report(), &expected, None).is_ok());
        for (pointer, bad) in [
            ("/verification_results/summary/executed", json!(0)),
            ("/verification_results/results/0/status", json!("Timeout")),
            ("/harness_metadata/0/attributes/should_panic", json!(true)),
            ("/verification_results/results/0/checks", json!([])),
            ("/tools/kani", json!("other")),
        ] {
            let mut data = report();
            *data.pointer_mut(pointer).unwrap() = bad;
            assert!(validate(&data, &expected, None).is_err());
        }
        assert!(validate(&report(), &BTreeSet::new(), None).is_err());
        assert!(validate(&report(), &BTreeSet::from(["missing".into()]), None).is_err());
    }
    #[test]
    fn negative_controls_require_the_named_assertion() {
        let expected = BTreeSet::from(["proof".into()]);
        let mut data = report();
        assert!(validate(&data, &expected, Some("expected property")).is_err());
        data["verification_results"]["summary"]["failed"] = 1.into();
        data["verification_results"]["summary"]["successful"] = 0.into();
        data["verification_results"]["results"][0]["status"] = "Failure".into();
        data["verification_results"]["results"][0]["checks"][0]["status"] = "Failure".into();
        assert!(validate(&data, &expected, Some("expected property")).is_ok());
        assert!(validate(&data, &expected, Some("different property")).is_err());
        assert!(validate(&data, &expected, None).is_err());
    }
}

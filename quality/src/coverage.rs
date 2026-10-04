use crate::{Result, Runner};
use capntproto_test_support::verification::{self as v, root};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};
pub const NIGHTLY: &str = "nightly-2026-03-05";
pub const SCOPE: &str = "first-party-owned-crates-v3";
const FLAGS: &str = "-C instrument-coverage -C link-dead-code -C opt-level=1 -C debug-assertions=yes -C overflow-checks=yes -Z coverage-options=branch";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub count: u64,
    pub covered: u64,
}
impl Metric {
    pub fn percent(&self) -> Option<f64> {
        (self.count > 0).then(|| self.covered as f64 * 100.0 / self.count as f64)
    }
    pub fn add(&mut self, other: &Self) {
        self.count += other.count;
        self.covered += other.covered;
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub lines: Metric,
    pub regions: Metric,
    pub functions: Metric,
    pub branches: Metric,
}
impl Metrics {
    pub fn add(&mut self, other: &Self) {
        self.lines.add(&other.lines);
        self.regions.add(&other.regions);
        self.functions.add(&other.functions);
        self.branches.add(&other.branches);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileCoverage {
    pub sha256: String,
    pub group: String,
    pub status: String,
    pub metrics: Option<Metrics>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub files: BTreeMap<String, FileCoverage>,
    pub totals: BTreeMap<String, Metrics>,
    pub flags: String,
    pub compiler: String,
    pub baseline: String,
    #[serde(default)]
    pub scope: String,
}

/// Explicit source ownership boundary, shared by every export and regression gate.
fn owned_source(path: &Path) -> bool {
    if path.extension().is_none_or(|ext| ext != "rs")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_capnp.rs"))
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || path
            .iter()
            .any(|part| matches!(part.to_str(), Some("target" | "vendor" | ".git")))
    {
        return false;
    }
    path == Path::new("build.rs")
        || [
            "src",
            "crates/capntproto-core",
            "crates/capntproto-rpc",
            "crates/capntproto-futures",
            "crates/capntproto-codegen",
            "crates/capntproto-compiler",
            "crates/capntproto-compat",
            "test-support",
            "quality",
            "tests",
            "examples",
            "fuzz",
            "benchmarks",
            "verification/miri",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn sources() -> Result<BTreeMap<PathBuf, String>> {
    Ok(v::distribution::source_hashes(&root())?
        .into_iter()
        .filter(|(path, _)| owned_source(path))
        .collect())
}

fn metric(value: &Value) -> Result<Metric> {
    let count = value["count"].as_u64().ok_or("missing coverage count")?;
    let covered = value["covered"].as_u64().ok_or("missing covered count")?;
    if covered > count {
        return Err("covered count exceeds executable count".into());
    }
    Ok(Metric { count, covered })
}
pub fn parse_export(value: &Value, base: &Path) -> Result<BTreeMap<String, Metrics>> {
    if value["type"] != "llvm.coverage.json.export" {
        return Err("not an LLVM coverage export".into());
    }
    let mut files = BTreeMap::new();
    for data in value["data"].as_array().ok_or("missing coverage data")? {
        for file in data["files"].as_array().ok_or("missing file inventory")? {
            let filename = Path::new(
                file["filename"]
                    .as_str()
                    .ok_or("missing coverage filename")?,
            );
            let Ok(relative) = filename.strip_prefix(base) else {
                continue;
            };
            if !owned_source(relative) {
                continue;
            }
            let summary = &file["summary"];
            let metrics = Metrics {
                lines: metric(&summary["lines"])?,
                regions: metric(&summary["regions"])?,
                functions: metric(&summary["functions"])?,
                branches: metric(&summary["branches"])?,
            };
            let name = relative.to_str().ok_or("non-UTF8 source path")?.to_owned();
            if files.insert(name, metrics).is_some() {
                return Err("duplicate file in merged LLVM export".into());
            }
        }
    }
    if files.is_empty() {
        return Err("coverage contains no repository files".into());
    }
    Ok(files)
}
fn group(path: &str) -> &str {
    if path.starts_with("src/") {
        "runtime"
    } else if path.starts_with("crates/capntproto-compiler/") {
        "schema-compiler"
    } else if path.starts_with("crates/capntproto-compat/") {
        "optional-compatibility"
    } else if path.starts_with("crates/capntproto-core/") {
        "wire-runtime"
    } else if path.starts_with("crates/capntproto-rpc/") {
        "capability-rpc"
    } else if path.starts_with("crates/capntproto-futures/") {
        "async-framing"
    } else if path.starts_with("crates/capntproto-codegen/") {
        "rust-codegen"
    } else if path.starts_with("tests/") || path.starts_with("test-support/") {
        "tests-and-verification"
    } else {
        "tools-fuzz-examples-benchmarks"
    }
}
pub fn regression(
    current: &BTreeMap<String, FileCoverage>,
    baseline: &BTreeMap<String, FileCoverage>,
) -> Result<()> {
    let mut failures = vec![];
    let old_totals = totals(baseline);
    let new_totals = totals(current);
    let mut pairs = Vec::new();
    for (name, old) in baseline {
        let Some(new) = current.get(name) else {
            continue;
        }; // Deleted source is not executable code.
        let Some(old) = &old.metrics else {
            continue;
        };
        let Some(new) = &new.metrics else {
            failures.push(format!("{name}: lost instrumentation"));
            continue;
        };
        pairs.push((name.as_str(), old, new));
    }
    for (name, old) in &old_totals {
        if let Some(new) = new_totals.get(name) {
            pairs.push((name.as_str(), old, new));
        }
    }
    for (name, old, new) in pairs {
        for (kind, old, new) in [
            ("lines", &old.lines, &new.lines),
            ("regions", &old.regions, &new.regions),
            ("functions", &old.functions, &new.functions),
            ("branches", &old.branches, &new.branches),
        ] {
            if old.count > 0
                && (new.count == 0
                    || u128::from(new.covered) * u128::from(old.count)
                        < u128::from(old.covered) * u128::from(new.count))
            {
                failures.push(format!(
                    "{name}: {kind} regressed {}/{} -> {}/{}",
                    old.covered, old.count, new.covered, new.count
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}
pub fn totals(files: &BTreeMap<String, FileCoverage>) -> BTreeMap<String, Metrics> {
    let mut totals: BTreeMap<String, Metrics> = BTreeMap::new();
    for file in files.values() {
        if let Some(metrics) = &file.metrics {
            totals.entry(file.group.clone()).or_default().add(metrics);
        }
    }
    totals
}
pub fn inventory(measured: &BTreeMap<String, Metrics>) -> Result<BTreeMap<String, FileCoverage>> {
    let mut files = BTreeMap::new();
    for (path, sha256) in sources()? {
        let name = path.to_string_lossy().into_owned();
        let metrics = measured.get(&name).cloned();
        let status = if metrics.is_some() {
            "measured (including zero-hit code)"
        } else {
            "no executable LLVM mapping in these platform/feature/test builds; not counted as covered"
        };
        files.insert(
            name.clone(),
            FileCoverage {
                sha256,
                group: group(&name).into(),
                status: status.into(),
                metrics,
            },
        );
    }
    Ok(files)
}
fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if entry.file_name() != "incremental" && entry.file_name() != ".fingerprint" {
                visit(&entry.path(), files)?;
            }
        } else if entry.file_type()?.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}
fn instrumented(args: &[&str], build: &Path, profiles: &Path) -> Command {
    // quiche emits an unhashed libquiche.rlib alongside its cdylib/staticlib.
    // Sharing targets across workspaces can overwrite it with a build using a
    // different Tokio feature set while Cargo retains the earlier fingerprint.
    let workspace = args
        .windows(2)
        .find(|pair| pair[0] == "--manifest-path")
        .map(|pair| pair[1].split('/').next().unwrap())
        .unwrap_or("root");
    let mut cmd = v::command("cargo");
    cmd.arg(format!("+{NIGHTLY}"))
        .arg("auditable")
        .args(args)
        .env("CARGO_TARGET_DIR", build.join(workspace))
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .env("RUSTFLAGS", FLAGS)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("LLVM_PROFILE_FILE", profiles.join("%p-%m.profraw"))
        .env("CAPNTPROTO_COVERAGE_CHILDREN_NATIVE", "1");
    cmd
}
/// Build and execute the independent C++ reference suites without coverage instrumentation.
/// The cache is bound to the complete C++ inputs, compiler and collector code.
pub fn cpp(r: &mut Runner) -> Result<()> {
    let clang_binary =
        std::env::var("CAPNTPROTO_REFERENCE_CXX").unwrap_or_else(|_| "clang++".into());
    let clang = r.run("clang", v::command(&clang_binary).arg("--version"))?;
    let inputs: BTreeMap<_, _> = v::distribution::source_hashes(&root())?
        .into_iter()
        .filter(|(path, _)| path.starts_with("vendor/capnproto"))
        .collect();
    let key = v::sha256(serde_json::to_vec(&json!([
        inputs,
        clang,
        clang_binary,
        "native-O1-debug-fibers-clean-shutdown-v2"
    ]))?);
    let cpp = root().join("target/quality-build/cpp").join(key);
    fs::create_dir_all(&cpp)?;
    let cpp = cpp.canonicalize()?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cpp.join("reference.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    r.run(
        "cpp-configure",
        v::command("cmake")
            .args(["-S", "vendor/capnproto", "-B"])
            .arg(&cpp)
            .args([
                "-G",
                "Ninja",
                "-DBUILD_TESTING=ON",
                "-DWITH_OPENSSL=OFF",
                "-DWITH_ZLIB=OFF",
                "-DWITH_FIBERS=ON",
                "-DCMAKE_BUILD_TYPE=Debug",
                "-DCMAKE_CXX_FLAGS_DEBUG=-O1 -g0 -DKJ_DEBUG=1",
            ])
            .arg(format!("-DCMAKE_CXX_COMPILER={clang_binary}")),
    )?;
    r.run(
        "cpp-build",
        v::command("cmake")
            .arg("--build")
            .arg(&cpp)
            .args([
                "--parallel",
                "2",
                "--target",
                "capnp-tests",
                "capnp-heavy-tests",
                "kj-tests",
                "kj-heavy-tests",
            ])
            .env("KJ_CLEAN_SHUTDOWN", "1"),
    )?;
    for (name, package) in [
        ("capnp-tests", "capnp"),
        ("capnp-heavy-tests", "capnp"),
        ("kj-tests", "kj"),
        ("kj-heavy-tests", "kj"),
    ] {
        r.run(
            name,
            v::command(cpp.join(format!("c++/src/{package}/{name}"))).env("KJ_CLEAN_SHUTDOWN", "1"),
        )?;
    }
    Ok(())
}
pub fn collect(r: &mut Runner) -> Result<()> {
    let working = root().join("target/quality-build");
    fs::create_dir_all(&working)?;
    // Reuse only artifacts and counters for identical source/configuration and
    // toolchain inputs. Changed sources get a separate directory, so removed
    // tests and stale mappings cannot contribute to a new revision's coverage.
    let build = working.join(&r.evidence.source_id);
    fs::create_dir_all(&build)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(build.join("coverage.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|e| format!("coverage for this source is already running: {e}"))?;
    let profile_directory = tempfile::Builder::new()
        .prefix("profiles-")
        .tempdir_in(&build)?;
    let profiles = profile_directory.path().to_path_buf();
    let compiler = r.run(
        "coverage-compiler",
        v::command("rustc").args([&format!("+{NIGHTLY}"), "-vV"]),
    )?;
    let sysroot = r.run(
        "sysroot",
        v::command("rustc").args([&format!("+{NIGHTLY}"), "--print", "sysroot"]),
    )?;
    let llvm = Path::new(sysroot.trim()).join("lib/rustlib/x86_64-unknown-linux-gnu/bin");
    // Instrument the Cargo partition once. Models, fuzz campaigns and extended
    // memory/mutation controls have independent jobs and reports.
    let exclusions = crate::lanes::CARGO_SKIPS
        .iter()
        .map(|name| format!("test({name})"))
        .collect::<Vec<_>>()
        .join(" | ");
    let filter = format!("not ({exclusions})");
    let tests = r.nextest(
        "workspace-tests",
        instrumented(
            &[
                "nextest",
                "run",
                "--locked",
                "--ignore-rust-version",
                "--workspace",
                "--no-fail-fast",
                "-E",
                &filter,
            ],
            &build,
            &profiles,
        )
        .env("CAPNTPROTO_CI_LANE", "cargo")
        .env("CAPNTPROTO_FULL_COVERAGE_BUILD", &build)
        .env("CAPNTPROTO_FULL_COVERAGE_PROFILES", &profiles)
        .env("CAPNTPROTO_FULL_COVERAGE_FLAGS", FLAGS),
    );
    // Nextest does not run rustdoc tests. Keep their instrumentation and result
    // stream separate, and still execute them after a unit/integration failure.
    let docs = r.run(
        "workspace-doctests",
        instrumented(
            &[
                "test",
                "--locked",
                "--ignore-rust-version",
                "--workspace",
                "--doc",
                "--no-fail-fast",
                "--",
                "-Z",
                "unstable-options",
                "--format=json",
            ],
            &build,
            &profiles,
        )
        .env(
            "RUSTDOCFLAGS",
            format!(
                "{FLAGS} -Z unstable-options --persist-doctests {}",
                build.join("doctests").display(),
            ),
        ),
    );
    tests?;
    docs?;

    let mut objects = vec![];
    let mut paths = vec![];
    visit(&build, &mut paths)?;
    for path in paths {
        // ELF coverage objects include build-script executables. Archives and
        // generated source files cannot silently substitute for runnable code.
        let metadata = fs::metadata(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        let mut header = [0; 4];
        if fs::File::open(&path)?.read(&mut header)? != 4 || &header != b"\x7fELF" {
            continue;
        }
        // Read the ELF section table instead of scanning gigabytes of code in
        // the unoptimized orchestration binary. A string in code is not a map.
        let sections = v::command("readelf")
            .args(["--sections", "--wide"])
            .arg(&path)
            .output()?;
        if !sections.status.success() {
            return Err(format!("cannot inspect ELF sections: {}", path.display()).into());
        }
        if String::from_utf8(sections.stdout)?
            .split_whitespace()
            .any(|name| name == "__llvm_covmap")
        {
            objects.push(path);
        }
    }
    objects.sort();
    objects.dedup();
    if objects.is_empty() {
        return Err("no instrumented objects discovered".into());
    }
    let profile_paths: Vec<_> = fs::read_dir(&profiles)?
        .filter_map(|p| match p {
            Ok(p) if p.path().extension().is_some_and(|e| e == "profraw") => Some(Ok(p.path())),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<std::io::Result<_>>()?;
    if profile_paths.is_empty() {
        return Err("no profiles produced".into());
    }
    let profile_list = build.join("profiles.txt");
    fs::write(
        &profile_list,
        profile_paths
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect::<String>(),
    )?;
    let profdata = build.join("merged.profdata");
    r.run(
        "merge",
        v::command(llvm.join("llvm-profdata"))
            .args(["merge", "-sparse", "--failure-mode=any", "-f"])
            .arg(profile_list)
            .arg("-o")
            .arg(&profdata),
    )?;
    // LLVM's -sources allowlist applies to JSON, LCOV and HTML alike. Exact
    // owned paths also exclude generated OUT_DIR files and checkout-local vendors.
    let source_paths: Vec<_> = sources()?
        .into_keys()
        .map(|path| root().join(path))
        .collect();
    if source_paths.is_empty() {
        return Err("no first-party Rust sources discovered".into());
    }
    let command = |mode: &str| {
        let mut c = v::command(llvm.join("llvm-cov"));
        c.arg(mode)
            .arg(format!("-instr-profile={}", profdata.display()))
            .arg("-ignore-filename-regex=/\\.cargo/|/rustc/");
        for object in &objects {
            c.arg("-object").arg(object);
        }
        c.arg("-sources").args(&source_paths);
        c
    };
    let export = r.run_stdout(
        "llvm-export",
        command("export")
            .arg("-skip-functions")
            .arg("-skip-expansions"),
    )?;
    fs::write(r.directory.join("coverage.json"), &export)?;
    let measured = parse_export(&serde_json::from_str(&export)?, &root())?;
    let lcov = r.run_stdout("lcov", command("export").arg("-format=lcov"))?;
    fs::write(r.directory.join("coverage.lcov"), lcov)?;
    r.run(
        "html",
        command("show")
            .arg("-format=html")
            .arg(format!(
                "-output-dir={}",
                r.directory.join("html").display()
            ))
            .args(["-show-branches=count", "-show-line-counts-or-regions"]),
    )?;
    let files = inventory(&measured)?;
    let totals = totals(&files);
    // Never certify the old three-file report or lose an entire maintained crate.
    for prefix in [
        "src/",
        "crates/capntproto-core/src/",
        "crates/capntproto-rpc/src/",
        "crates/capntproto-futures/src/",
        "crates/capntproto-codegen/src/",
        "crates/capntproto-compiler/src/",
        "crates/capntproto-compat/src/",
        "fuzz/src/",
        "quality/src/",
    ] {
        if !files.iter().any(|(p, f)| {
            p.starts_with(prefix) && f.metrics.as_ref().is_some_and(|m| m.lines.covered > 0)
        }) {
            return Err(format!("missing measured crate: {prefix}").into());
        }
    }
    let baseline_path = root().join("quality/coverage-baseline.json");
    let mut summary = Summary {
        files,
        totals,
        flags: FLAGS.into(),
        compiler,
        baseline: "not yet established; this run is the initial measurement".into(),
        scope: SCOPE.into(),
    };
    crate::report::write_files(&r.directory, &summary)?;
    fs::write(
        r.directory.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    if baseline_path.exists() {
        let old: Summary = serde_json::from_slice(&fs::read(&baseline_path)?)?;
        if old.scope != SCOPE || old.flags != summary.flags || old.compiler != summary.compiler {
            return Err(
                "coverage baseline scope/toolchain/options differ; requalification required".into(),
            );
        }
        regression(&summary.files, &old.files)?;
        summary.baseline = v::sha256(fs::read(baseline_path)?);
    }
    fs::write(
        r.directory.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    r.evidence.data = json!({"test_reports":r.evidence.data["test_reports"],"summary":summary,"objects":objects.len(),"profiles":profile_paths.len(),"llvm_json_sha256":v::sha256(export),"build":build,"scope":"First-party Rust line/region/function/branch coverage only; vendored, generated and external sources excluded; zero-hit mappings retained and unmapped owned files explicit; no MC/DC claim"});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_excludes_dependencies_generated_files_and_path_escapes() {
        for path in [
            "src/lib.rs",
            "build.rs",
            "crates/capntproto-compiler/src/lib.rs",
            "crates/capntproto-compat/src/json.rs",
            "crates/capntproto-core/src/lib.rs",
            "crates/capntproto-rpc/src/lib.rs",
            "crates/capntproto-futures/src/lib.rs",
            "crates/capntproto-codegen/src/lib.rs",
            "quality/src/coverage.rs",
            "tests/storage.rs",
            "verification/miri/tests/ownership.rs",
        ] {
            assert!(owned_source(Path::new(path)), "{path}");
        }
        for path in [
            "crates/capntproto-core/src/schema_capnp.rs",
            "crates/capntproto-rpc/src/rpc_capnp.rs",
            "vendor/quiche/quiche/src/lib.rs",
            "vendor/capnproto/c++/src/capnp/rpc.c++",
            "target/debug/build/schema.rs",
            "crates/capntproto-compiler/target/debug/build/schema.rs",
            "src/../vendor/a.rs",
            "src-extra/a.rs",
            "/src/lib.rs",
            "src/schema.capnp",
            "crates/external/src/lib.rs",
        ] {
            assert!(!owned_source(Path::new(path)), "{path}");
        }
        assert_eq!(
            group("crates/capntproto-compat/src/json.rs"),
            "optional-compatibility"
        );
    }

    #[test]
    fn export_keeps_zero_hit_owned_code_and_ignores_other_sources() {
        let counts = json!({"count": 3, "covered": 0});
        let summary = json!({"lines":counts,"regions":counts,"functions":counts,"branches":counts});
        let exported = json!({"type":"llvm.coverage.json.export", "data":[{"files":[
            {"filename":"/repo/src/lib.rs", "summary":summary},
            {"filename":"/repo/crates/capntproto-core/src/lib.rs", "summary":summary},
            {"filename":"/repo/target/debug/build/generated.rs", "summary":{}},
            {"filename":"/registry/dependency/src/lib.rs", "summary":{}}
        ]}]});
        let files = parse_export(&exported, Path::new("/repo")).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            files["src/lib.rs"].lines,
            Metric {
                count: 3,
                covered: 0
            }
        );
    }

    #[test]
    fn missing_and_inconsistent_coverage_cannot_pass() {
        assert!(parse_export(&json!({}), Path::new("/repo")).is_err());
        assert!(metric(&json!({"count":1,"covered":2})).is_err());
        let mut old = BTreeMap::new();
        old.insert(
            "src/a.rs".into(),
            FileCoverage {
                sha256: "x".into(),
                group: "runtime".into(),
                status: "measured".into(),
                metrics: Some(Metrics {
                    lines: Metric {
                        count: 10,
                        covered: 9,
                    },
                    ..Metrics::default()
                }),
            },
        );
        let mut new = old.clone();
        new.get_mut("src/a.rs")
            .unwrap()
            .metrics
            .as_mut()
            .unwrap()
            .lines
            .covered = 8;
        assert!(regression(&new, &old).is_err());
        new.get_mut("src/a.rs").unwrap().metrics = None;
        assert!(regression(&new, &old).is_err());
        assert!(regression(&old, &old).is_ok());
        let mut additional = old.clone();
        let mut uncovered = old["src/a.rs"].clone();
        uncovered.metrics.as_mut().unwrap().lines.covered = 0;
        additional.insert("src/new.rs".into(), uncovered);
        // Existing files have not changed, but new uncovered code lowers the group.
        assert!(regression(&additional, &old).is_err());
        assert_eq!(Metric::default().percent(), None);
    }
}

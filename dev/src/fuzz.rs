//! Bounded AFL++ runs, retaining failures and byte-for-byte corpus filenames.
use anyhow::{ensure, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};
#[derive(Parser)]
pub struct Args {
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub binaries: PathBuf,
    #[arg(long)]
    pub corpus: PathBuf,
    #[arg(long,default_value_t=120,value_parser=clap::value_parser!(u64).range(1..=3600))]
    pub seconds: u64,
}
pub fn statistics(text: &str) -> Result<Value> {
    let fields: BTreeMap<_, _> = text
        .lines()
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let mut result = json!({});
    for name in [
        "execs_done",
        "corpus_count",
        "saved_crashes",
        "saved_hangs",
        "edges_found",
    ] {
        let value = fields
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("missing AFL counter {name}"))?
            .parse::<u64>()?;
        result[name] = json!(value);
    }
    ensure!(
        result["execs_done"].as_u64().unwrap() > 0 && result["edges_found"].as_u64().unwrap() > 0,
        "AFL produced no instrumented executions"
    );
    for name in ["stability", "bitmap_cvg"] {
        let value = fields
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("missing AFL percentage {name}"))?
            .trim_end_matches('%')
            .parse::<f64>()?;
        ensure!(
            value.is_finite() && (0.0..=100.0).contains(&value),
            "invalid AFL percentage"
        );
        result[name] = json!(value);
    }
    Ok(result)
}
pub fn validate(target: &str, code: i32, stats: &Value, log: &str) -> Result<()> {
    ensure!(
        target != "rpc_lifecycle" || log.contains("Using IJON feature."),
        "AFL did not report IJON support for the RPC target"
    );
    ensure!(
        code == 0 && stats["saved_crashes"] == 0 && stats["saved_hangs"] == 0,
        "AFL failed or saved crash/hang inputs; inspect the retained corpus"
    );
    Ok(())
}
pub fn run(args: Args) -> Result<()> {
    let cancel = crate::process::Cancellation::install()?;
    run_with(
        args,
        &cancel,
        || crate::process::text(Command::new("cargo").args(["afl", "--version"])),
        |command, seconds, log| {
            Ok(crate::process::run(
                Command::new(&command[0]).args(&command[1..]).envs([
                    ("AFL_NO_UI", "1"),
                    ("AFL_SKIP_CPUFREQ", "1"),
                    ("AFL_NO_AFFINITY", "1"),
                    ("AFL_FUZZER_LOOPCOUNT", "1000"),
                ]),
                seconds,
                Some(log),
                &cancel,
            )?
            .code)
        },
    )
}
fn run_with(
    args: Args,
    cancel: &crate::process::Cancellation,
    version: impl FnOnce() -> Result<String>,
    mut execute: impl FnMut(&[String], u64, &Path) -> Result<i32>,
) -> Result<()> {
    let output = std::path::absolute(args.output)?;
    let binaries = std::path::absolute(args.binaries)?;
    let corpus = std::path::absolute(args.corpus)?;
    let path = output.join("afl.json");
    let mut report = json!({"format":1,"engine":"afl++","cargo_afl":"0.18.2","afl_version":"4.40c","cmplog":false,"seconds_per_target":args.seconds,"campaigns":[],"passed":false});
    crate::write_json(&path, &report)?;
    let version = version()?;
    ensure!(
        version == "cargo-afl 0.18.2 (AFL++ version 4.40c)",
        "unexpected AFL tool: {version}"
    );
    for target in [
        "capnp_framing",
        "capnp_pointers",
        "capnp_schema",
        "rpc_lifecycle",
    ] {
        let findings = output.join(target);
        ensure!(
            !findings.exists(),
            "campaign output already exists: {}",
            findings.display()
        );
        let command = vec![
            "cargo".into(),
            "afl".into(),
            "fuzz".into(),
            "-i".into(),
            corpus.join(target).display().to_string(),
            "-o".into(),
            findings.display().to_string(),
            "-V".into(),
            args.seconds.to_string(),
            "-s".into(),
            "1".into(),
            "-G".into(),
            "4096".into(),
            "-t".into(),
            "1000".into(),
            "-m".into(),
            "none".into(),
            "-c".into(),
            "-".into(),
            "--".into(),
            binaries.join(format!("afl_{target}")).display().to_string(),
        ];
        let log = output.join(format!("afl-{target}.log"));
        let started = Instant::now();
        println!("AFL++ {target}: {}s mutation budget", args.seconds);
        let mut campaign = json!({"target":target,"passed":false,"statistics":null,"error":null,"command":command});
        let result = (|| -> Result<()> {
            let code = execute(&command, args.seconds + 300, &log)?;
            campaign["exit_code"] = json!(code);
            let stats = statistics(&std::fs::read_to_string(
                findings.join("default/fuzzer_stats"),
            )?)?;
            campaign["statistics"] = stats.clone();
            validate(
                target,
                code,
                &stats,
                &String::from_utf8_lossy(&std::fs::read(&log)?),
            )
        })();
        match result {
            Ok(()) => campaign["passed"] = json!(true),
            Err(e) => campaign["error"] = json!(e.to_string()),
        }
        campaign["seconds"] = json!(started.elapsed().as_secs_f64());
        if log.exists() {
            campaign["log_sha256"] = json!(crate::file_hash(&log)?);
        }
        println!(
            "AFL++ {target}: {}",
            if campaign["statistics"].is_null() {
                &campaign["error"]
            } else {
                &campaign["statistics"]
            }
        );
        report["campaigns"].as_array_mut().unwrap().push(campaign);
        crate::write_json(&path, &report)?;
        ensure!(!cancel.cancelled(), "campaign interrupted");
    }
    report["passed"] = json!(report["campaigns"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["passed"] == true));
    crate::write_json(&path, &report)?;
    ensure!(
        report["passed"] == true,
        "AFL campaigns failed; see {}",
        path.display()
    );
    Ok(())
}
pub fn package(root: &Path) -> Result<PathBuf> {
    let output = root.join("target/fuzz-report.tar.gz");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(&output)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    archive.follow_symlinks(false);
    for relative in [
        "target/quality/fuzz",
        "target/verification/native-fuzz",
        "fuzz/artifacts",
    ] {
        let path = root.join(relative);
        if path.exists() {
            archive.append_dir_all(relative, path)?;
        }
    }
    archive.into_inner()?.finish()?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_campaigns_remain_failures() {
        let stats=statistics("execs_done : 100\ncorpus_count : 2\nsaved_crashes : 0\nsaved_hangs : 0\nedges_found : 30\nstability : 99.1%\nbitmap_cvg : 20%\n").unwrap();
        assert!(validate("rpc_lifecycle", 0, &stats, "Using IJON feature.").is_ok());
        assert!(validate("rpc_lifecycle", 0, &stats, "").is_err());
        assert!(validate("rpc_lifecycle", 1, &stats, "Using IJON feature.").is_err());
        for counter in ["saved_crashes", "saved_hangs"] {
            let mut s = stats.clone();
            s[counter] = json!(1);
            assert!(validate("capnp_framing", 0, &s, "").is_err());
        }
    }
    #[test]
    fn invalid_statistics_fail() {
        for value in ["0", "-1", "nan"] {
            assert!(statistics(&format!("execs_done : {value}\ncorpus_count : 1\nsaved_crashes : 0\nsaved_hangs : 0\nedges_found : 0\nstability : 100%\nbitmap_cvg : 0%\n")).is_err());
        }
    }
    #[test]
    fn archive_preserves_afl_names() {
        let dir = tempfile::tempdir().unwrap();
        let files = BTreeMap::from([
            (
                "target/quality/fuzz/default/queue/id:000000,time:0,execs:0,orig:seed-1",
                "seed",
            ),
            (
                "target/quality/fuzz/default/crashes/id:000001,sig:06",
                "crash",
            ),
            (
                "target/quality/fuzz/default/hangs/id:000002,time:42",
                "hang",
            ),
            ("target/quality/fuzz/FAILED-TESTS.md", "diagnostics"),
            ("target/verification/native-fuzz/check/campaign.log", "log"),
            ("fuzz/artifacts/native_packet/crash-input", "corpus"),
        ]);
        for (name, contents) in &files {
            crate::write(dir.path().join(name), contents).unwrap();
        }
        let path = package(dir.path()).unwrap();
        let decoder = flate2::read::GzDecoder::new(std::fs::File::open(&path).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let mut found = BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.header().entry_type().is_file() {
                use std::io::Read;
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                found.insert(entry.path().unwrap().to_string_lossy().into_owned(), text);
            }
        }
        assert_eq!(
            found,
            files
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        );
        for name in files.keys() {
            std::fs::remove_file(dir.path().join(name)).unwrap();
        }
        package(dir.path()).unwrap();
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
            std::fs::File::open(path).unwrap(),
        ));
        assert!(archive
            .entries()
            .unwrap()
            .all(|e| !e.unwrap().header().entry_type().is_file()));
    }

    #[test]
    fn campaign_reports_keep_findings_missing_stats_and_process_failures() {
        for failure in [
            "none", "crashes", "hangs", "ijon", "exit", "stats", "timeout",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path().join("output");
            let result = run_with(
                Args {
                    output: output.clone(),
                    binaries: dir.path().join("bin"),
                    corpus: dir.path().join("corpus"),
                    seconds: 1,
                },
                &crate::process::Cancellation::default(),
                || Ok("cargo-afl 0.18.2 (AFL++ version 4.40c)".into()),
                |command, seconds, log| {
                    assert_eq!(seconds, 301);
                    let dest =
                        Path::new(&command[command.iter().position(|a| a == "-o").unwrap() + 1]);
                    crate::write(
                        log,
                        if failure == "ijon" {
                            "ordinary instrumentation"
                        } else {
                            "Using IJON feature."
                        },
                    )?;
                    if failure == "timeout" {
                        anyhow::bail!("fake deadline");
                    }
                    if failure != "stats" {
                        crate::write(dest.join("default/fuzzer_stats"), format!("execs_done : 100\ncorpus_count : 2\nsaved_crashes : {}\nsaved_hangs : {}\nedges_found : 30\nstability : 99.1%\nbitmap_cvg : 20%\n", u8::from(failure == "crashes"), u8::from(failure == "hangs")))?;
                    }
                    Ok(i32::from(failure == "exit"))
                },
            );
            assert_eq!(result.is_ok(), failure == "none", "{failure}");
            let report = crate::read_json(output.join("afl.json")).unwrap();
            assert_eq!(report["passed"], result.is_ok());
            let campaigns = report["campaigns"].as_array().unwrap();
            assert_eq!(campaigns.len(), 4);
            assert!(campaigns.iter().all(|c| c["log_sha256"].as_str().is_some()));
            if result.is_err() {
                assert!(campaigns.iter().any(|c| c["error"].as_str().is_some()));
            }
        }
    }
}

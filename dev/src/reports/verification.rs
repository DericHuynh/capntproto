use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
pub fn chart(
    name: &str,
    title: &str,
    unit: &str,
    note: &str,
    panels: Vec<(&str, Vec<(String, Value)>)>,
) -> Value {
    json!({"name":name,"title":title,"unit":unit,"note":note,"panels":panels.into_iter().map(|(title,values)|json!({"title":title,"bars":values.into_iter().map(|(label,value)|json!({"label":label,"value":value})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
pub fn models(data: &Value, directory: &Path) -> Result<Vec<Value>> {
    let records = data["models"].as_array().cloned().unwrap_or_default();
    if records.is_empty() {
        return Ok(vec![]);
    }
    let mut positive = 0;
    let mut controls = 0;
    let mut failed = 0;
    let mut states = BTreeMap::<String, [u64; 2]>::new();
    let pattern = regex::Regex::new(r"([\d,]+) states generated, ([\d,]+) distinct states found")?;
    for record in records {
        let id = crate::string(&record, "id")?;
        ensure!(
            super::data::matches(r"^[a-f0-9]{64}$", id),
            "invalid model identity"
        );
        let passed = record["passed"].as_bool().context("invalid model result")?;
        if passed {
            if crate::number(&record, "expected_exit")? != 0 {
                controls += 1;
            } else {
                positive += 1;
            }
        } else {
            failed += 1;
        }
        if record["log_sha256"].is_null() {
            ensure!(!passed, "passing TLC result without a log");
            continue;
        }
        let log = std::fs::read(directory.join("models").join(format!("{id}.log")))?;
        ensure!(
            json!(crate::hash(&log)) == record["log_sha256"],
            "TLC log hash mismatch"
        );
        if let Some(m) = pattern.captures_iter(std::str::from_utf8(&log)?).last() {
            let module = Path::new(crate::string(&record, "module")?)
                .file_stem()
                .context("invalid model module")?
                .to_string_lossy()
                .into_owned();
            let counts = states.entry(module).or_default();
            for index in 0..2 {
                counts[index] = counts[index]
                    .checked_add(m[index + 1].replace(',', "").parse::<u64>()?)
                    .context("TLC state count overflow")?;
            }
        }
    }
    let mut result=vec![chart("tla-outcomes","TLA+ checks and expected counterexamples","Checks","Unique module/configuration/expected-exit checks. Expected invariant violations are successful controls.",vec![("Observed outcomes",vec![("Invariant/liveness checks".into(),json!(positive)),("Expected counterexamples".into(),json!(controls)),("Failed / incomplete checks".into(),json!(failed))])])];
    if !states.is_empty() {
        let mut sorted: Vec<_> = states.iter().collect();
        sorted.sort_by(|a, b| b.1[1].cmp(&a.1[1]).then_with(|| a.0.cmp(b.0)));
        let mut panels = Vec::new();
        for (index, title) in ["Generated states", "Distinct states"]
            .into_iter()
            .enumerate()
        {
            let mut values: Vec<_> = sorted
                .iter()
                .take(15)
                .map(|(name, counts)| ((*name).clone(), json!(counts[index])))
                .collect();
            if sorted.len() > 15 {
                let mut others = 0_u64;
                for (_, counts) in sorted.iter().skip(15) {
                    others = others
                        .checked_add(counts[index])
                        .context("TLC count overflow")?;
                }
                values.push(("Other models".into(), json!(others)));
            }
            panels.push((title, values));
        }
        result.push(chart("tla-states","TLA+ explored states by model","States","Counts sum independent bounded configurations; they are not globally distinct states or a proof beyond those bounds.",panels));
    }
    Ok(result)
}
pub fn fuzz(data: &Value) -> Result<Vec<Value>> {
    let (mut runs, mut coverage, mut findings) = (Vec::new(), Vec::new(), Vec::new());
    let pattern = regex::Regex::new(r"\bcov:\s*(\d+)")?;
    if let Some(campaigns) = data["libfuzzer"]["campaigns"].as_object() {
        for (target, campaign) in campaigns {
            let label = format!("libFuzzer / {target}");
            runs.push((label.clone(), campaign["runs"].clone()));
            if let Some(m) = pattern.captures(crate::string(campaign, "coverage_feedback")?) {
                coverage.push((label.clone(), json!(m[1].parse::<u64>()?)));
            }
            findings.push((label, json!(0)));
        }
    }
    let (mut afl_runs, mut afl_coverage, mut afl_findings) = (Vec::new(), Vec::new(), Vec::new());
    for campaign in data["afl"]["campaigns"].as_array().into_iter().flatten() {
        let stats = &campaign["statistics"];
        if stats.is_null() {
            continue;
        }
        let label = format!("AFL++ / {}", crate::string(campaign, "target")?);
        afl_runs.push((label.clone(), stats["execs_done"].clone()));
        afl_coverage.push((label.clone(), stats["edges_found"].clone()));
        afl_findings.push((format!("{label} crashes"), stats["saved_crashes"].clone()));
        afl_findings.push((format!("{label} hangs"), stats["saved_hangs"].clone()));
    }
    let mut result = Vec::new();
    for(name,title,unit,note,left,right)in[("fuzz-executions","Fuzzing executions by engine","Executions","Bounded campaigns including seed calibration; execution counts are not comparable performance benchmarks.",runs,afl_runs),("fuzz-coverage","Fuzzer feedback by engine","Feedback entries","Engine-local counters, not LLVM source coverage. Do not compare counts across engines or builds.",coverage,afl_coverage),("fuzz-findings","Saved fuzzing findings","Inputs","Saved crashes/hangs are findings requiring triage, not confirmed unique bugs. Missing results remain unknown.",findings,afl_findings)]{
let mut panels=Vec::new();
if !left.is_empty(){panels.push(("libFuzzer + ASan",left));}
if !right.is_empty(){panels.push(("AFL++ + RPC IJON",right));}
if !panels.is_empty(){result.push(chart(name,title,unit,note,panels));}}
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_graphs_bind_logs_and_separate_expected_counterexamples() {
        let dir = tempfile::tempdir().unwrap();
        let log = b"1,024 states generated, 512 distinct states found, 0 states left on queue.\n";
        let mut records = Vec::new();
        for (index, exit) in [0, 12].into_iter().enumerate() {
            let id = index.to_string().repeat(64);
            crate::write(dir.path().join(format!("models/{id}.log")), log).unwrap();
            records.push(json!({"id":id,"expected_exit":exit,"passed":true,"module":"verification/Example.tla","log_sha256":crate::hash(log)}));
        }
        let evidence = json!({"models":records});
        let charts = models(&evidence, dir.path()).unwrap();
        let bars = charts[0]["panels"][0]["bars"].as_array().unwrap();
        assert_eq!(
            bars.iter().map(|b| b["value"].clone()).collect::<Vec<_>>(),
            [json!(1), json!(1), json!(0)]
        );
        assert_eq!(charts[1]["panels"][1]["bars"][0]["value"], 1024);
        crate::write(
            dir.path().join(format!("models/{}.log", "0".repeat(64))),
            "tampered",
        )
        .unwrap();
        assert!(models(&evidence, dir.path())
            .unwrap_err()
            .to_string()
            .contains("hash mismatch"));
    }

    #[test]
    fn missing_fuzz_engines_are_unknown_and_findings_are_visible() {
        let charts = fuzz(&json!({"afl":{"campaigns":[{"target":"rpc_lifecycle","statistics":{"execs_done":100,"edges_found":20,"saved_crashes":2,"saved_hangs":1}}]}})).unwrap();
        assert_eq!(charts[0]["panels"].as_array().unwrap().len(), 1);
        let bars = charts[2]["panels"][0]["bars"].as_array().unwrap();
        assert_eq!(
            bars.iter().map(|b| b["value"].clone()).collect::<Vec<_>>(),
            [json!(2), json!(1)]
        );
        assert!(fuzz(&json!({})).unwrap().is_empty());
    }
}

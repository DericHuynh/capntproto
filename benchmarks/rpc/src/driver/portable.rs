//! Matched portable/legacy/C++ loopback controls, separate from VPC workflows.
use super::*;

const DRIVERS: [&str; 3] = ["portable", "legacy", "capnp-cpp"];

pub fn compare(directory: &Path, destination: &Path) -> Result<()> {
    if cfg!(debug_assertions) {
        return Err("portable comparisons require a release driver".into());
    }
    fs::create_dir(destination)?;
    let placement = Placement::discover()?;
    let manifest: Value = serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
    let mut hashes = serde_json::Map::new();
    for name in ["tcp", "capnp_cpp", "capnp-reference", "driver"] {
        let digest = output(
            "sha256sum",
            &[directory.join(name).to_str().ok_or("invalid binary path")?],
        )?;
        let digest = digest.split_whitespace().next().ok_or("missing digest")?;
        if manifest["binaries"][name] != digest {
            return Err("portable comparison binary digest mismatch".into());
        }
        hashes.insert(name.to_owned(), json!(digest));
    }
    fs::write(
        destination.join("environment.json"),
        serde_json::to_vec_pretty(&json!({
            "measurement_version":crate::MEASUREMENT_VERSION, "manifest":manifest,
            "binary_sha256":hashes, "cpu_placement":placement,
            "cpu":output("lscpu", &[])?, "clock_reads":clock_diagnostics(&placement)?,
            "scope":"Plaintext TCP loopback; separate processes and physical cores. Five rotating repetitions, 10000 warmups and 1000 retained calls per driver/payload. Portable and legacy selected explicitly for both peers."
        }))?,
    )?;
    let mut trials = Vec::new();
    for repetition in 0..5 {
        for bytes in PAYLOADS {
            for offset in 0..DRIVERS.len() {
                let driver = DRIVERS[(repetition + offset) % DRIVERS.len()];
                let protocol = if driver == "capnp-cpp" {
                    "capnp-cpp"
                } else {
                    "tcp"
                };
                eprintln!("{driver}: {bytes} bytes, repetition {}", repetition + 1);
                let mut value = trial_for_driver(
                    &target(directory, protocol),
                    protocol,
                    bytes,
                    Some(&placement),
                    1000,
                    (driver != "capnp-cpp").then_some(driver),
                )?;
                value["driver"] = json!(driver);
                value["repetition"] = json!(repetition);
                trials.push(value);
                fs::write(
                    destination.join("trials.json"),
                    serde_json::to_vec_pretty(&trials)?,
                )?;
            }
        }
    }
    let report = summarize(&trials)?;
    fs::write(
        destination.join("summary.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    let mut readme = String::from("# Portable loopback target\n\nPlaintext TCP, separate processes and physical cores; five rotating repetitions, 10,000 warmups and 1,000 retained calls per driver/payload. All samples and executable/host identities are retained in trials.json and environment.json. This is separate from the two-host VPC workflows.\n\nTarget: portable pooled p50 ≤ 1.05 × min(legacy, C++) for every payload. Advisory while gap #11 remains open; tails remain in summary.json.\n\n| Bytes | Portable p50 (ns) | Legacy p50 (ns) | C++ p50 (ns) | Faster reference | Ratio | Meets target |\n|---:|---:|---:|---:|---|---:|---|\n");
    for row in report["rows"].as_array().unwrap() {
        readme += &format!(
            "| {} | {} | {} | {} | {} | {:.3}× | {} |\n",
            row["payload_bytes"],
            row["portable_p50_p95_p99_ns"][0],
            row["legacy_p50_p95_p99_ns"][0],
            row["cpp_p50_p95_p99_ns"][0],
            row["reference"].as_str().unwrap(),
            row["ratio"].as_f64().unwrap(),
            row["meets_target"]
        );
    }
    fs::write(destination.join("README.md"), readme)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    // Preserve every miss while this tracked gap is being optimized. Invalid
    // sampling/identity fails; a valid slower result remains reviewable evidence.
    Ok(())
}

fn summarize(trials: &[Value]) -> Result<Value> {
    if trials.len() != 5 * PAYLOADS.len() * DRIVERS.len() {
        return Err("portable comparison requires every driver/payload/repetition".into());
    }
    let mut rows = Vec::new();
    let mut passed = true;
    for bytes in PAYLOADS {
        let mut measured = Vec::new();
        for driver in DRIVERS {
            let protocol = if driver == "capnp-cpp" {
                "capnp-cpp"
            } else {
                "tcp"
            };
            let cell: Vec<_> = trials
                .iter()
                .filter(|t| t["driver"] == driver && t["payload_bytes"] == bytes)
                .collect();
            if cell.len() != 5 {
                return Err("missing portable comparison cell".into());
            }
            for repetition in 0..5 {
                if cell
                    .iter()
                    .filter(|t| t["repetition"] == repetition)
                    .count()
                    != 1
                {
                    return Err("missing or duplicate portable repetition".into());
                }
            }
            let mut samples = Vec::new();
            for trial in cell {
                validate_trial(trial, protocol, bytes)?;
                samples.extend(
                    trial["latency_ns"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap()),
                );
            }
            samples.sort_unstable();
            measured.push([50, 95, 99].map(|p| samples[(samples.len() * p).div_ceil(100) - 1]));
        }
        let reference = if measured[1][0] <= measured[2][0] {
            1
        } else {
            2
        };
        let within = u128::from(measured[0][0]) * 100 <= u128::from(measured[reference][0]) * 105;
        passed &= within;
        rows.push(json!({"payload_bytes":bytes, "portable_p50_p95_p99_ns":measured[0],
            "legacy_p50_p95_p99_ns":measured[1], "cpp_p50_p95_p99_ns":measured[2],
            "reference":DRIVERS[reference], "ratio":measured[0][0] as f64 / measured[reference][0] as f64,
            "meets_target":within}));
    }
    Ok(
        json!({"metric":"pooled p50 per payload", "maximum_ratio":1.05, "advisory":true,
        "reference_rule":"min(legacy, capnp-cpp) in the same campaign", "all_payloads_pass":passed, "rows":rows}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn trials(portable: u64, legacy: u64, cpp: u64) -> Vec<Value> {
        let mut values = Vec::new();
        for bytes in PAYLOADS {
            for (driver, latency) in DRIVERS.into_iter().zip([portable, legacy, cpp]) {
                for repetition in 0..5 {
                    values.push(json!({"measurement_version":crate::MEASUREMENT_VERSION,
                        "protocol":if driver == "capnp-cpp" {"capnp-cpp"} else {"tcp"},
                        "payload_bytes":bytes, "driver":driver, "repetition":repetition,
                        "warmup":10000, "iterations":1000, "elapsed_ns":latency*1000,
                        "latency_ns":vec![latency;1000]}));
                }
            }
        }
        values
    }
    #[test]
    fn target_switches_reference_and_keeps_every_payload_and_tail() {
        for (legacy, cpp, reference) in [(100, 200, "legacy"), (200, 100, "capnp-cpp")] {
            for (portable, expected) in [(99, true), (105, true), (106, false)] {
                let result = summarize(&trials(portable, legacy, cpp)).unwrap();
                assert_eq!(result["all_payloads_pass"], expected);
                for row in result["rows"].as_array().unwrap() {
                    assert_eq!(row["reference"], reference);
                    assert_eq!(row["meets_target"], expected);
                    assert_eq!(
                        row["portable_p50_p95_p99_ns"],
                        json!([portable, portable, portable])
                    );
                }
            }
        }
    }
    #[test]
    fn partial_duplicate_or_misidentified_trials_cannot_qualify() {
        let valid = trials(100, 100, 100);
        assert!(summarize(&valid[..valid.len() - 1]).is_err());
        for (field, value) in [
            ("driver", json!("unknown")),
            ("protocol", json!("native")),
            ("repetition", json!(1)),
            ("warmup", json!(0)),
            ("latency_ns", json!([100])),
        ] {
            let mut bad = valid.clone();
            bad[0][field] = value;
            assert!(summarize(&bad).is_err(), "{field}");
        }
        let mut one_miss = valid;
        for value in &mut one_miss {
            if value["driver"] == "portable" && value["payload_bytes"] == 65536 {
                value["latency_ns"] = json!(vec![106; 1000]);
                value["elapsed_ns"] = json!(106000);
            }
        }
        assert_eq!(summarize(&one_miss).unwrap()["all_payloads_pass"], false);
    }
}

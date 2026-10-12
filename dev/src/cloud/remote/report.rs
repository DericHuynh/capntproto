use super::*;
#[path = "../../../../benchmarks/rpc/workflow-support.rs"]
mod support;
use support::{supported, IMPLEMENTATIONS, VERSION};
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("../../../../benchmarks/rpc/workflows.json"))
        .expect("workflow catalog")
}
fn matrix(trials: &[Value]) -> Result<Vec<Value>> {
    let cases = cases();
    ensure!(
        trials.len()
            == cases
                .iter()
                .map(|case| IMPLEMENTATIONS
                    .iter()
                    .filter(|implementation| supported(
                        implementation,
                        case["name"].as_str().unwrap()
                    ))
                    .count())
                .sum::<usize>()
                * 5,
        "incomplete remote workflow matrix"
    );
    let mut seen = BTreeSet::new();
    for trial in trials {
        ensure!(
            trial["workflow_version"] == VERSION,
            "unknown workflow version"
        );
        let case = cases
            .iter()
            .find(|case| **case == trial["case"])
            .context("unexpected workflow shape")?;
        let implementation = crate::string(trial, "implementation")?;
        let repetition = crate::number(trial, "repetition")?;
        ensure!(
            supported(implementation, crate::string(case, "name")?) && repetition < 5,
            "unexpected implementation/repetition"
        );
        ensure!(
            seen.insert((
                crate::string(case, "name")?.to_owned(),
                implementation.to_owned(),
                repetition
            )),
            "duplicate workflow trial"
        );
        ensure!(
            trial["warmup"] == 250 && trial["iterations"] == 1000,
            "unexpected workflow sampling budget"
        );
        let samples = trial["latency_ns"]
            .as_array()
            .context("missing latency samples")?;
        ensure!(samples.len() == 1000, "missing workflow samples");
        let mut total = 0u128;
        for sample in samples {
            let n = sample.as_u64().context("non-integer latency")?;
            ensure!(n > 0, "zero latency");
            total += u128::from(n);
        }
        ensure!(
            total <= u128::from(crate::number(trial, "elapsed_ns")?),
            "impossible workflow timing bounds"
        );
    }
    let mut rows = Vec::new();
    for case in cases {
        for implementation in IMPLEMENTATIONS {
            if !supported(implementation, crate::string(&case, "name")?) {
                continue;
            }
            let cell: Vec<_> = trials
                .iter()
                .filter(|t| t["case"] == case && t["implementation"] == implementation)
                .collect();
            ensure!(cell.len() == 5, "missing implementation");
            let mut samples: Vec<_> = cell
                .iter()
                .flat_map(|t| {
                    t["latency_ns"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_u64().unwrap())
                })
                .collect();
            samples.sort_unstable();
            let elapsed: u128 = cell
                .iter()
                .map(|t| u128::from(t["elapsed_ns"].as_u64().unwrap()))
                .sum();
            let throughput = samples.len() as f64 * 1e9 / elapsed as f64;
            rows.push(json!({"case":case,"implementation":implementation,"samples":samples.len(),"p50_ns":samples[2499],"p95_ns":samples[4749],"p99_ns":samples[4949],"workflows_per_second":throughput,"rpcs_per_second":throughput*case["rpcs_per_iteration"].as_u64().unwrap() as f64}));
        }
    }
    for index in 0..rows.len() {
        let cpp = rows
            .iter()
            .find(|r| r["case"] == rows[index]["case"] && r["implementation"] == "capnp-cpp")
            .unwrap()["p50_ns"]
            .as_u64()
            .unwrap();
        rows[index]["p50_ratio_to_cpp"] =
            json!(rows[index]["p50_ns"].as_u64().unwrap() as f64 / cpp as f64);
    }
    Ok(rows)
}
fn validate_hosts(
    topology: &Value,
    client: &Value,
    server: &Value,
    manifest: &Value,
) -> Result<()> {
    ensure!(
        topology["topology"] == "two-droplet-vpc",
        "not remote topology"
    );
    let c = &topology["client"];
    let s = &topology["server"];
    ensure!(
        crate::number(c, "id")? != crate::number(s, "id")?,
        "same droplet on both sides"
    );
    for key in ["region", "size", "vpc_uuid"] {
        ensure!(
            !crate::string(c, key)?.is_empty() && c[key] == s[key],
            "peer placement mismatch: {key}"
        );
    }
    let ca: Ipv4Addr = crate::string(c, "private_address")?.parse()?;
    let sa: Ipv4Addr = crate::string(s, "private_address")?.parse()?;
    ensure!(
        ca.is_private() && sa.is_private() && ca != sa,
        "remote peers must have distinct private addresses"
    );
    ensure!(
        client["server_address"] == sa.to_string() && server["bind_address"] == sa.to_string(),
        "measured address does not match receipt"
    );
    ensure!(
        client["role"] == "client"
            && server["role"] == "server"
            && client["workflow_version"] == VERSION
            && server["workflow_version"] == VERSION,
        "wrong host roles/version"
    );
    ensure!(
        client["smoke"] == false,
        "smoke run is not performance evidence"
    );
    ensure!(
        !crate::string(client, "boot_id")?.trim().is_empty()
            && !crate::string(server, "boot_id")?.trim().is_empty()
            && client["boot_id"] != server["boot_id"],
        "both peers ran on the same host"
    );
    ensure!(
        client["manifest"] == *manifest && server["manifest"] == *manifest,
        "remote peers did not measure this bundle"
    );
    ensure!(
        server["server_resources"].as_array().is_some_and(|r| {
            r.len() == IMPLEMENTATIONS.len()
                && IMPLEMENTATIONS.iter().all(|i| {
                    r.iter()
                        .filter(|entry| entry["implementation"] == *i)
                        .count()
                        == 1
                })
        }),
        "missing server resource evidence"
    );
    Ok(())
}
pub(super) fn generate(output: &Path, bundle: &Path) -> Result<()> {
    let topology = crate::read_json(output.join("topology.json"))?;
    let client = crate::read_json(output.join("client/client-environment.json"))?;
    let server = crate::read_json(output.join("server/server-environment.json"))?;
    let manifest = crate::read_json(bundle.join("manifest.json"))?;
    validate_hosts(&topology, &client, &server, &manifest)?;
    ensure!(
        fs::read_to_string(output.join("server/server-stopped"))? == "ok\n",
        "server completion not confirmed"
    );
    let trials = crate::read_json(output.join("client/workflow-trials.json"))?;
    let rows = matrix(trials.as_array().context("missing workflow matrix")?)?;
    crate::write_json(
        output.join("summary.json"),
        &json!({"workflow_version":VERSION,"topology":topology,"validated":true,"acceptance":"descriptive remote workflow baseline; no loopback ratio gate applied","rows":rows,"unsupported":[{"case":"pipelined-read","implementation":"grpc","reason":"no native capability promise pipelining"},{"case":"pipelined-read","implementation":"websocket","reason":"no native capability promise pipelining"}]}),
    )?;
    let mut text=String::from("# Remote RPC workflows\n\nTwo dedicated droplets, same region/plan/VPC, direct plaintext TCP. Five rotating repetitions, 250 warmups and 1,000 retained workflows per cell. Timings use only the client monotonic clock; validation, response cleanup and ten-second workflow deadlines are included. Setup/compilation is excluded.\n\nA fan-out sample covers completion of all 32 concurrent calls; it is **not** individual request latency. Throughput is closed-loop, not an open-loop saturation or SLO test. RPC/s counts the two calls in dependent/stateful/callback workflows.\n\n| Workflow | Implementation | p50 µs | p95 µs | p99 µs | Workflows/s | RPC/s | p50 / C++ |\n|---|---|---:|---:|---:|---:|---:|---:|\n");
    for row in &rows {
        text += &format!(
            "| {} | {} | {:.2} | {:.2} | {:.2} | {:.0} | {:.0} | {:.3}× |\n",
            row["case"]["name"].as_str().unwrap(),
            row["implementation"].as_str().unwrap(),
            row["p50_ns"].as_u64().unwrap() as f64 / 1000.0,
            row["p95_ns"].as_u64().unwrap() as f64 / 1000.0,
            row["p99_ns"].as_u64().unwrap() as f64 / 1000.0,
            row["workflows_per_second"].as_f64().unwrap(),
            row["rpcs_per_second"].as_f64().unwrap(),
            row["p50_ratio_to_cpp"].as_f64().unwrap()
        );
    }
    text+="\n| Unsupported workflow | Implementation | Reason |\n|---|---|---|\n| pipelined-read | grpc | No native capability promise pipelining |\n| pipelined-read | websocket | No native capability promise pipelining |\n\ngRPC uses protobuf unary calls over HTTP/2; its callback uses a pre-established bidirectional stream. WebSockets use binary protobuf application messages, with 32 outstanding messages for fan-out. Both use application IDs for state and four acknowledged duplex messages per callback (request, notification, acknowledgement, completion). Dependent reads consume temporary state; reusable state is released outside timing. These IDs do not supply capability security. There is no TLS, compression, JSON or base64 in this comparison. RPC/s counts logical application calls, including callbacks, not HTTP/2 streams.\n\n![Remote workflow p50 ratios](p50-ratios.svg)\n\nRaw distributions, per-trial client CPU/RSS logs and both host environments are retained in the adjacent client/server directories. Server CPU counters include setup and campaign idle time; they are not per-request CPU costs. Legacy loopback controls are reported separately in the parent directory.\n";
    crate::write(output.join("README.md"), text)?;
    let maximum = rows
        .iter()
        .map(|r| r["p50_ratio_to_cpp"].as_f64().unwrap())
        .fold(1.0, f64::max)
        * 1.1;
    let height = 100 + rows.len() * 23;
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1080\" height=\"{height}\" viewBox=\"0 0 1080 {height}\" role=\"img\" aria-label=\"Remote workflow median latency divided by C++ median\"><rect width=\"1080\" height=\"{height}\" fill=\"white\"/><g font-family=\"sans-serif\" font-size=\"13\" fill=\"#182230\"><text x=\"20\" y=\"28\" font-size=\"20\">Remote workflow p50 / C++ (lower is faster)</text><text x=\"20\" y=\"52\">Promise-pipelined reads: gRPC and WebSocket unsupported; see table.</text>");
    for (i, row) in rows.iter().enumerate() {
        let y = 80 + i * 23;
        let implementation = row["implementation"].as_str().unwrap();
        let index = IMPLEMENTATIONS
            .iter()
            .position(|name| *name == implementation)
            .unwrap();
        svg += &format!(
            "<text x=\"20\" y=\"{}\">{} / {implementation}</text>",
            y + 12,
            row["case"]["name"].as_str().unwrap()
        );
        let ratio = row["p50_ratio_to_cpp"].as_f64().unwrap();
        let width = ratio / maximum * 640.0;
        svg+=&format!("<rect x=\"260\" y=\"{y}\" width=\"{width:.2}\" height=\"15\" fill=\"{}\"/><text x=\"{:.2}\" y=\"{}\">{ratio:.3}×</text>",["#2878b5","#d07815","#6b7280","#168247","#8756b4"][index],265.0+width,y+12);
    }
    svg += "</g></svg>";
    crate::write(output.join("p50-ratios.svg"), svg)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn resources() -> Value {
        json!(IMPLEMENTATIONS.map(|implementation| json!({"implementation":implementation})))
    }
    fn trials() -> Vec<Value> {
        let mut trials = Vec::new();
        for case in cases() {
            for implementation in IMPLEMENTATIONS {
                if !supported(implementation, case["name"].as_str().unwrap()) {
                    continue;
                }
                for repetition in 0..5 {
                    trials.push(json!({"workflow_version":VERSION,"case":case,"implementation":implementation,"repetition":repetition,"warmup":250,"iterations":1000,"latency_ns":vec![10;1000],"elapsed_ns":10000}));
                }
            }
        }
        trials
    }
    #[test]
    fn full_matrix_and_bad_evidence() {
        let valid = trials();
        let rows = matrix(&valid).unwrap();
        assert_eq!(rows.len(), 58);
        for (name, bytes) in [
            ("bulk-256k", 262144),
            ("bulk-1m", 1048576),
            ("bulk-4m", 4194304),
        ] {
            for implementation in IMPLEMENTATIONS {
                let row = rows
                    .iter()
                    .find(|row| {
                        row["case"]["name"] == name && row["implementation"] == implementation
                    })
                    .unwrap();
                assert_eq!(row["case"]["payload_bytes"], bytes);
            }
            let missing: Vec<_> = valid
                .iter()
                .filter(|trial| trial["case"]["name"] != name)
                .cloned()
                .collect();
            assert!(matrix(&missing).is_err(), "missing large payload {name}");
        }
        let fanout = rows
            .iter()
            .find(|r| r["case"]["name"] == "fanout-32")
            .unwrap();
        assert_eq!(
            fanout["rpcs_per_second"].as_f64().unwrap(),
            32.0 * fanout["workflows_per_second"].as_f64().unwrap()
        );
        assert!(matrix(&valid[..valid.len() - 1]).is_err());
        let mut duplicate = valid.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(matrix(&duplicate).is_err());
        let mut unsupported = valid.clone();
        let cell = unsupported
            .iter_mut()
            .find(|trial| trial["case"]["name"] == "pipelined-read")
            .unwrap();
        cell["implementation"] = json!("grpc");
        assert!(matrix(&unsupported).is_err());
        for (key, value) in [
            ("warmup", json!(2)),
            ("iterations", json!(5)),
            ("elapsed_ns", json!(1)),
            ("latency_ns", json!([1])),
            ("implementation", json!("unknown")),
            ("case", json!({"name":"unary-empty"})),
        ] {
            let mut broken = valid.clone();
            broken[0][key] = value;
            assert!(matrix(&broken).is_err(), "{key}");
        }
    }
    #[test]
    fn refuses_same_host_smoke_mismatched_bundle_and_placement() {
        let manifest = json!({"binaries":{"driver":"digest"}});
        let s = json!({"id":1,"region":"nyc3","size":"c-4","vpc_uuid":"vpc","private_address":"10.0.0.1"});
        let mut c = s.clone();
        c["id"] = json!(2);
        c["private_address"] = json!("10.0.0.2");
        let topology = json!({"topology":"two-droplet-vpc","server":s,"client":c});
        let client = json!({"role":"client","workflow_version":VERSION,"smoke":false,"server_address":"10.0.0.1","boot_id":"client","manifest":manifest});
        let server = json!({"role":"server","workflow_version":VERSION,"bind_address":"10.0.0.1","boot_id":"server","manifest":manifest,"server_resources":resources()});
        validate_hosts(&topology, &client, &server, &manifest).unwrap();
        let mut missing_server = server.clone();
        missing_server["server_resources"][4] = missing_server["server_resources"][0].clone();
        assert!(validate_hosts(&topology, &client, &missing_server, &manifest).is_err());
        for (key, value) in [
            ("boot_id", json!("server")),
            ("smoke", json!(true)),
            ("manifest", json!({})),
            ("server_address", json!("127.0.0.1")),
        ] {
            let mut broken = client.clone();
            broken[key] = value;
            assert!(validate_hosts(&topology, &broken, &server, &manifest).is_err());
        }
        for key in ["id", "private_address", "region", "size", "vpc_uuid"] {
            let mut broken = topology.clone();
            broken["client"][key] = if key == "id" || key == "private_address" {
                broken["server"][key].clone()
            } else {
                json!("other")
            };
            assert!(
                validate_hosts(&broken, &client, &server, &manifest).is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn generates_report_and_graph_only_after_full_validation() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("report");
        let bundle = dir.path().join("bundle");
        let manifest = json!({"binaries":{"driver":"synthetic fixture"}});
        let server_receipt = json!({"id":1,"region":"nyc3","size":"c-4","vpc_uuid":"fixture-vpc","private_address":"10.0.0.1"});
        let mut client_receipt = server_receipt.clone();
        client_receipt["id"] = json!(2);
        client_receipt["private_address"] = json!("10.0.0.2");
        crate::write_json(bundle.join("manifest.json"), &manifest).unwrap();
        crate::write_json(
            output.join("topology.json"),
            &json!({"topology":"two-droplet-vpc","server":server_receipt,"client":client_receipt}),
        )
        .unwrap();
        crate::write_json(output.join("client/client-environment.json"), &json!({"role":"client","workflow_version":VERSION,"smoke":false,"server_address":"10.0.0.1","boot_id":"client-fixture","manifest":manifest})).unwrap();
        crate::write_json(output.join("server/server-environment.json"), &json!({"role":"server","workflow_version":VERSION,"bind_address":"10.0.0.1","boot_id":"server-fixture","manifest":manifest,"server_resources":resources()})).unwrap();
        crate::write(output.join("server/server-stopped"), "ok\n").unwrap();
        crate::write_json(output.join("client/workflow-trials.json"), &trials()).unwrap();
        generate(&output, &bundle).unwrap();
        let summary = crate::read_json(output.join("summary.json")).unwrap();
        assert_eq!(summary["rows"].as_array().unwrap().len(), 58);
        let svg = fs::read_to_string(output.join("p50-ratios.svg")).unwrap();
        let parsed = roxmltree::Document::parse(&svg).unwrap();
        assert_eq!(
            parsed
                .descendants()
                .filter(|node| node.has_tag_name("rect"))
                .count(),
            59
        );
        let markdown = fs::read_to_string(output.join("README.md")).unwrap();
        for case in cases() {
            assert!(markdown.contains(case["name"].as_str().unwrap()));
        }
        // A new output directory cannot publish a partial result.
        crate::remove(output.join("summary.json")).unwrap();
        crate::write_json(output.join("client/workflow-trials.json"), &json!([])).unwrap();
        assert!(generate(&output, &bundle).is_err());
        assert!(!output.join("summary.json").exists());
    }
}

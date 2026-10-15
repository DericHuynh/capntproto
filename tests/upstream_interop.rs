//! Independent upstream peers, in both roles, on ordinary Cap'n Proto TCP framing.
//! No Native envelope or private protocol extension is enabled here.
use capntproto_test_support::verification::{self as v, command, root, run};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
const CASES: &[&str] = &[
    "bootstrap-payloads",
    "pipeline-reflection-embargo",
    "tail-to-callback",
    "bidirectional-tail-calls",
    "promise-resolution-rejection",
    "exceptions-unknown-methods",
    "finish-cancellation",
    "capability-release",
    "streaming",
    "connection-survives-errors",
    "graceful-shutdown",
];

fn build(output: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let metadata = v::run_stdout(
        command("cargo").args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
            "verification/upstream-rpc/Cargo.toml",
        ]),
        &output.join("rust-metadata.log"),
        0,
    )
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    for package in packages {
        if package["name"] != "capntproto-upstream-interop" {
            assert_eq!(
                package["source"], "registry+https://github.com/rust-lang/crates.io-index",
                "independent peer must not link a local or git-patched dependency: {}",
                package["name"]
            );
        }
    }
    for (name, version) in [
        ("capnp", "0.27.2"),
        ("capnp-rpc", "0.27.0"),
        ("capnpc", "0.27.0"),
    ] {
        let matches: Vec<_> = packages.iter().filter(|p| p["name"] == name).collect();
        assert_eq!(matches.len(), 1, "one independent {name} implementation");
        assert_eq!(matches[0]["version"], version);
    }
    let cpp = v::cpp::build(&["capnp_tool", "capnpc_cpp", "capnp-rpc"]).unwrap();
    let bin = cpp.join("c++/src/capnp");
    run(
        command(bin.join("capnp")).args([
            "compile",
            &format!(
                "-o{}:{}",
                bin.join("capnpc-c++").display(),
                output.display()
            ),
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/interop.capnp",
        ]),
        &output.join("cpp-schema.log"),
        0,
    )
    .unwrap();
    let executable = output.join("cpp-peer");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", output.display()))
            .arg("tests/cpp/interop-peer.c++")
            .arg(output.join("interop.capnp.c++"))
            .arg(bin.join("libcapnp-rpc.a"))
            .arg(bin.join("libcapnp.a"))
            .arg(cpp.join("c++/src/kj/libkj-async.a"))
            .arg(cpp.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&executable),
        &output.join("cpp-build.log"),
        0,
    )
    .unwrap();
    run(
        command("cargo").args([
            "build",
            "--locked",
            "--no-default-features",
            "--example",
            "rpc_interop",
            "--example",
            "bounded_cpp_interop",
            "--target-dir",
            "target/interop-build",
        ]),
        &output.join("ours-build.log"),
        0,
    )
    .unwrap();
    run(
        command("cargo")
            .args([
                "build",
                "--locked",
                "--manifest-path",
                "verification/upstream-rpc/Cargo.toml",
                "--target-dir",
                "verification/upstream-rpc/target",
            ])
            .env("CAPNP", bin.join("capnp")),
        &output.join("rust-build.log"),
        0,
    )
    .unwrap();
    (
        root().join("target/interop-build/debug/examples/rpc_interop"),
        executable,
        root().join("verification/upstream-rpc/target/debug/capntproto-upstream-interop"),
    )
}

fn row(
    client: &Path,
    client_driver: &str,
    server: &Path,
    server_driver: &str,
    output: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let mut server = Process(
        Command::new(server)
            .args([server_driver, "server"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(output.join("server.log")).map_err(|e| e.to_string())?)
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let stdout = server.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    // The server must announce readiness, not rely on a guessed sleep. Continue
    // draining stdout so a diagnostic cannot fill the pipe and deadlock it.
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut port = String::new();
        let _ = tx.send(reader.read_line(&mut port).map(|_| port));
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
    let port: u16 = rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|e| format!("server readiness: {e}"))?
        .map_err(|e| e.to_string())?
        .trim()
        .parse()
        .map_err(|e| format!("invalid server port: {e}"))?;
    let log = output.join("client.log");
    let mut client = Process(
        Command::new(client)
            .args([client_driver, "client", &format!("127.0.0.1:{port}")])
            .stdin(Stdio::null())
            .stdout(fs::File::create(&log).map_err(|e| e.to_string())?)
            .stderr(fs::File::create(output.join("client-errors.log")).map_err(|e| e.to_string())?)
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = client.0.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err(format!(
                    "client {status}: {}",
                    fs::read_to_string(output.join("client-errors.log")).unwrap_or_default()
                ));
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "client timeout; {}",
                fs::read_to_string(&log).unwrap_or_default()
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let stdout = fs::read_to_string(log).map_err(|e| e.to_string())?;
    for case in CASES {
        if !stdout.lines().any(|line| line == format!("PASS {case}")) {
            return Err(format!("missing case {case}: {stdout}"));
        }
    }
    wait_for_server_exit(Duration::from_secs(10), || {
        match server.0.try_wait().map_err(|e| e.to_string())? {
            None => Ok(false),
            Some(status) if status.success() => Ok(true),
            Some(status) => Err(format!("server {status}")),
        }
    })?;
    reader.join().map_err(|_| "server reader panicked")?;
    Ok(())
}

fn wait_for_server_exit(
    timeout: Duration,
    mut exited: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if exited()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("server did not shut down after client EOF".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn server_shutdown_wait_handles_pending_timeout_and_poll_errors() {
    // Do not depend on the OS scheduler leaving the real peer alive long enough
    // to exercise the pending/timeout paths during the interoperability matrix.
    wait_for_server_exit(Duration::ZERO, || Ok(true)).unwrap();
    let mut polls = 0;
    wait_for_server_exit(Duration::from_secs(10), || {
        polls += 1;
        Ok(polls == 2)
    })
    .unwrap();
    assert_eq!(polls, 2);
    assert_eq!(
        wait_for_server_exit(Duration::ZERO, || Ok(false)).unwrap_err(),
        "server did not shut down after client EOF"
    );
    assert_eq!(
        wait_for_server_exit(Duration::ZERO, || Err("poll failure".into())).unwrap_err(),
        "poll failure"
    );
}

#[test]
fn cpp_and_upstream_rust_bidirectional_rpc() {
    let output = root().join("target/verification/upstream-interop");
    fs::create_dir_all(&output).unwrap();
    let _lock = v::lock(&output.join("matrix.lock")).unwrap();
    let (ours, cpp, rust) = build(&output);
    let peers = [
        (&ours, "legacy"),
        (&ours, "portable"),
        (&cpp, "cpp"),
        (&rust, "upstream-rust"),
    ];
    let mut results = Vec::new();
    let mut failures = Vec::new();
    let bounded = run(
        command(root().join("target/interop-build/debug/examples/bounded_cpp_interop"))
            .arg(&rust)
            .arg("bounded-server"),
        &output.join("bounded-upstream-rust.log"),
        0,
    )
    .map_err(|error| error.to_string());
    if let Err(error) = &bounded {
        failures.push(format!("bounded-to-upstream-rust: {error}"));
    }
    let mut pairs: Vec<_> = peers
        .iter()
        .flat_map(|client| peers.iter().map(move |server| (*client, *server)))
        .filter(|(client, server)| client.1 != server.1)
        .collect();
    // Qualify the explicit optimization against C++, while ordinary streams
    // retain interoperability with upstream Rust without application settings.
    pairs.extend([
        ((&ours, "legacy-transfer"), (&cpp, "cpp")),
        ((&cpp, "cpp"), (&ours, "legacy-transfer")),
    ]);
    let mut markdown = String::from("# Independent RPC interoperability\n\nC++ `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`; capnp `0.27.2`, capnp-rpc/capnpc `0.27.0`. Ordinary TCP framing, no Native extensions.\n\n| Client | Server | Result | Completed scenarios |\n| --- | --- | --- | --- |\n");
    for ((client, client_driver), (server, server_driver)) in pairs {
        let name = format!("{client_driver}-to-{server_driver}");
        let result = row(
            client,
            client_driver,
            server,
            server_driver,
            &output.join(&name),
        );
        let stdout = fs::read_to_string(output.join(&name).join("client.log")).unwrap_or_default();
        let completed: Vec<_> = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("PASS "))
            .collect();
        // These are independent upstream/upstream controls, never a
        // waiver for capntproto. Keep actual failures visible in the report.
        // Accept only the reproduced 0.27.0 tail-transfer signatures; any
        // other failure (and every failure involving our code) fails CI.
        let limitation = match (client_driver, server_driver, &result) {
            ("upstream-rust", "cpp", Err(error)) => {
                error.contains("E-order lost during reflection")
                    && completed == ["bootstrap-payloads"]
            }
            ("cpp", "upstream-rust", Err(error)) => {
                error.contains("Peer disconnected")
                    && completed
                        == [
                            "bootstrap-payloads",
                            "pipeline-reflection-embargo",
                            "tail-to-callback",
                        ]
                    && fs::read_to_string(output.join(&name).join("server.log"))
                        .unwrap_or_default()
                        .contains("Error: Elapsed(())")
            }
            _ => false,
        };
        println!("{name}: {result:?}");
        if let Err(error) = &result {
            if !limitation {
                failures.push(format!("{name}: {error}"));
            }
        }
        let status = if limitation {
            "upstream limitation (failed)"
        } else if result.is_ok() {
            "passed"
        } else {
            "FAILED"
        };
        markdown.push_str(&format!(
            "| {client_driver} | {server_driver} | {status} | {}/{} |\n",
            completed.len(),
            CASES.len()
        ));
        results.push(serde_json::json!({"client":client_driver,"server":server_driver,"passed":result.is_ok(),"upstream_limitation":limitation,"error":result.err(),"required_cases":CASES,"completed_cases":completed}));
    }
    markdown.push_str("\nThe two upstream-only controls expose unsupported optimized tail transfer in capnp-rpc 0.27.0. They are reported as failures, never as successful interoperability. Every capntproto row must pass all scenarios. See docs/wiki/Interoperability.md for scope and reproduction.\n");
    markdown.push_str(&format!("\nAllocation-free caller-buffered client against upstream Rust: **{}**. Its allocation counter must remain zero through RPC teardown and complete message-buffer recovery.\n", if bounded.is_ok() { "passed" } else { "FAILED" }));
    fs::write(output.join("README.md"), markdown).unwrap();
    fs::write(output.join("matrix.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "cpp_revision":"0de72d8d8cec6b69edaa29de51d3bd490341f9c2", "capnp":"0.27.2", "capnp_rpc":"0.27.0", "capnpc":"0.27.0", "rows": results,
        "bounded_upstream_rust": {"passed": bounded.is_ok(), "error": bounded.err()}
    })).unwrap()).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

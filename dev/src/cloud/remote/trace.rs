//! Diagnostic ordering on the same peers, after the acceptance measurements.
use super::*;

const TRACE: &str = "strace -f -qq -ttt -T -yy -xx -v -s 4096 -e trace=read,write,readv,writev,sendto,recvfrom,sendmsg,recvmsg,epoll_wait,epoll_pwait,epoll_ctl";
const CASES: [&str; 5] = [
    "unary-empty",
    "dependent-read",
    "pipelined-read",
    "callback",
    "bulk-64k",
];
const VARIANTS: [(&str, &str, &str); 3] = [
    ("portable", "driver", "1"),
    ("legacy", "driver", "0"),
    ("cpp", "capnp-reference", "0"),
];

pub(super) fn run(
    commands: &impl Commands,
    server: &Host,
    client: &Host,
    output: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    fs::create_dir_all(output)?;
    crate::write_json(
        output.join("scope.json"),
        &json!({
            "scope": "Post-measurement syscall diagnostics; traced latencies are not acceptance evidence",
            "warmup": 2, "iterations": 20, "cases": CASES,
            "variants": ["portable", "legacy", "cpp"],
            "server": server.receipt, "client": client.receipt,
            "trace_flags": TRACE, "server_cpu": 1, "client_cpu": 2,
        }),
    )?;
    for (role, host) in [("server", server), ("client", client)] {
        fs::create_dir_all(output.join(role).join("io-trace"))?;
        commands.ssh(&host.options, &host.public.to_string(),
            "DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y strace && mkdir -p /root/results/io-trace && strace --version > /root/results/io-trace/version.txt", 120, cancel)?;
        commands.download(
            &host.options,
            &host.public.to_string(),
            &output.join(role),
            "io-trace/version.txt",
            30,
            cancel,
        )?;
    }
    for case in CASES {
        for (label, binary, portable) in VARIANTS {
            let prefix = format!("{case}-{label}");
            let path = format!("/root/results/io-trace/{prefix}");
            let address = format!("{}:46001", server.private);
            commands.ssh(&server.options, &server.public.to_string(), &format!(
                "nohup sh -c 'CAPNTPROTO_BENCH_PORTABLE={portable} timeout --kill-after=5s 60s {TRACE} -o {path}-server.strace taskset -c 1 /root/bench/{binary} workflow-server {address} & traced=$!; echo \"$traced\" > {path}-server.pid; wait \"$traced\"; touch {path}-server.done' > {path}-server.ready 2> {path}-server.log < /dev/null &"
            ), 30, cancel)?;
            let result = (|| -> Result<()> {
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    ensure!(
                        !cancel.cancelled() && Instant::now() < deadline,
                        "diagnostic server readiness failed: {prefix}"
                    );
                    let mut args = server.options.clone();
                    args.extend([
                        format!("root@{}", server.public),
                        format!("test -s {path}-server.ready"),
                    ]);
                    if commands.ready(&args, cancel) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                commands.ssh(&client.options, &client.public.to_string(), &format!(
                    "CAPNTPROTO_BENCH_PORTABLE={portable} timeout --kill-after=5s 30s {TRACE} -o {path}-client.strace taskset -c 2 /root/bench/{binary} workflow-measure {address} {case} 2 20 > {path}-client.json 2> {path}-client.log"
                ), 40, cancel)
            })();
            // Stop and retrieve independently of the measurement result. The
            // outer campaign also unconditionally destroys both owned hosts.
            let cleanup = Cancellation::default();
            let stopped = commands.ssh(&server.options, &server.public.to_string(), &format!(
                "kill -TERM \"$(cat {path}-server.pid)\" 2>/dev/null || true; timeout 10s sh -c 'until test -f {path}-server.done; do sleep 0.1; done'"
            ), 15, &cleanup);
            let mut downloads = Ok(());
            for (role, host, suffixes) in [
                ("server", server, ["strace", "log", "ready"]),
                ("client", client, ["strace", "log", "json"]),
            ] {
                for suffix in suffixes {
                    let file = format!("io-trace/{prefix}-{role}.{suffix}");
                    if let Err(error) = commands.download(
                        &host.options,
                        &host.public.to_string(),
                        &output.join(role),
                        &file,
                        30,
                        &cleanup,
                    ) {
                        eprintln!("Could not retrieve {file}: {error}");
                        downloads = Err(error);
                    }
                }
            }
            result?;
            stopped?;
            downloads?;
        }
    }
    Ok(())
}

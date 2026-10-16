//! Low-overhead socket timing, isolated from the uninstrumented qualification.
use super::*;

const PROBE: &str = include_str!("timing/probe.c");
const CASES: [&str; 3] = ["unary-empty", "pipelined-read", "callback"];
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
    crate::write(output.join("analyze.py"), include_str!("timing/analyze.py"))?;
    crate::write_json(
        output.join("scope.json"),
        &json!({
            "scope": "Post-qualification paired wall/thread-CPU socket diagnostics; instrumentation adds overhead and never enters acceptance",
            "probe_source_sha256": crate::hash(PROBE),
            "warmup": 100, "iterations": 1000, "repetitions": 5,
            "cases": CASES, "variants": ["portable", "legacy", "cpp"],
            "server_cpu": 1, "client_cpu": 2,
            "event_format": "little-endian u64 count, then 56-byte QQQQqiIII records; capacity 131072",
            "pairing": "positive socket read completion to next write entry on the same thread and fd; discard first 210 and final interval",
            "server": server.receipt, "client": client.receipt,
        }),
    )?;
    for (role, host) in [("server", server), ("client", client)] {
        fs::create_dir_all(output.join(role).join("timing"))?;
        commands.ssh(&host.options, &host.public.to_string(), &format!(
            "set -eu\nDEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y gcc libc6-dev\nmkdir -p /root/results/timing\ncd /root/results/timing\ncat > probe.c <<'CAPNTPROTO_SOCKET_PROBE'\n{PROBE}CAPNTPROTO_SOCKET_PROBE\ngcc --version > build.log\ngcc -O2 -shared -fPIC -Wall -Wextra -Werror probe.c -o probe.so -ldl >> build.log 2>&1\nsha256sum probe.c probe.so /root/bench/driver /root/bench/capnp-reference > hashes.txt"
        ), 180, cancel)?;
        for file in ["probe.c", "probe.so", "hashes.txt", "build.log"] {
            commands.download(
                &host.options,
                &host.public.to_string(),
                &output.join(role),
                &format!("timing/{file}"),
                30,
                cancel,
            )?;
        }
    }
    for repetition in 0..5 {
        for case in CASES {
            for offset in 0..VARIANTS.len() {
                let (label, binary, portable) = VARIANTS[(repetition + offset) % VARIANTS.len()];
                let prefix = format!("{repetition}-{case}-{label}");
                let path = format!("/root/results/timing/{prefix}");
                let address = format!("{}:46002", server.private);
                commands.ssh(&server.options, &server.public.to_string(), &format!(
                    "nohup sh -c 'timeout --kill-after=5s 60s env CAPNTPROTO_BENCH_PORTABLE={portable} LD_PRELOAD=/root/results/timing/probe.so CAPNTPROTO_IO_TRACE={path}-server.bin taskset -c 1 /root/bench/{binary} workflow-server {address} & timed=$!; echo \"$timed\" > {path}-server.pid; wait \"$timed\"; touch {path}-server.done' > {path}-server.ready 2> {path}-server.log < /dev/null &"
                ), 30, cancel)?;
                let result = (|| -> Result<()> {
                    let deadline = Instant::now() + Duration::from_secs(30);
                    loop {
                        ensure!(
                            !cancel.cancelled() && Instant::now() < deadline,
                            "timing server readiness failed: {prefix}"
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
                        "timeout --kill-after=5s 30s env CAPNTPROTO_BENCH_PORTABLE={portable} LD_PRELOAD=/root/results/timing/probe.so CAPNTPROTO_IO_TRACE={path}-client.bin taskset -c 2 /root/bench/{binary} workflow-measure {address} {case} 100 1000 > {path}-client.json 2> {path}-client.log"
                    ), 40, cancel)
                })();
                // Always stop the server and salvage each artifact independently,
                // even if the client failed or the outer cancellation was set.
                let cleanup = Cancellation::default();
                let stopped = commands.ssh(&server.options, &server.public.to_string(), &format!(
                    "kill -TERM \"$(cat {path}-server.pid)\" 2>/dev/null || true; timeout 10s sh -c 'until test -f {path}-server.done; do sleep 0.1; done'"
                ), 15, &cleanup);
                let mut downloads = Ok(());
                for (role, host, suffixes) in [
                    ("server", server, ["bin", "log", "ready"]),
                    ("client", client, ["bin", "log", "json"]),
                ] {
                    for suffix in suffixes {
                        let file = format!("timing/{prefix}-{role}.{suffix}");
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
    }
    Ok(())
}

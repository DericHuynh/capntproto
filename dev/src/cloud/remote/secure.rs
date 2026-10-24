//! Encrypted loopback measurements on CI's dedicated droplet, never the CI runner.
use super::*;

pub(super) fn run(
    commands: &impl Commands,
    host: &Host,
    output: &Path,
    cancel: &Cancellation,
    allocations_only: bool,
) -> Result<()> {
    fs::create_dir_all(output)?;
    crate::write_json(output.join("host.json"), &host.receipt)?;
    commands.ssh(&host.options, &host.public.to_string(),
        "DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y python3 openssl strace && mkdir -p /root/results/secure && uname -a > /root/results/secure/kernel.txt && lscpu > /root/results/secure/cpu.txt && valgrind --version > /root/results/secure/valgrind.txt && sha256sum /root/bench/driver /root/bench/capnp-reference /root/bench/*.py /root/bench/*.json > /root/results/secure/hashes.txt", 180, cancel)?;
    // Tracing/profiling always follows every native repetition. Per-trial host
    // state, explicit TLS identity and all samples are saved by the same runner.
    let command = if allocations_only {
        r#"bash -o pipefail -c 'set -eu
{
cd /root/bench
baseline=()
if test -f driver-baseline; then baseline+=(--portable-baseline /root/bench/driver-baseline); fi
if test -f driver-legacy-baseline; then baseline+=(--legacy-baseline /root/bench/driver-legacy-baseline); fi
python3 secure-profile.py --rust /root/bench/driver --cpp /root/bench/capnp-reference "${baseline[@]}" --output /root/results/secure/profiles --allocation-only
} 2>&1 | tee /root/results/secure/runner.log'"#
    } else {
        r#"bash -o pipefail -c 'set -eu
{
cd /root/bench
baseline=()
if test -f driver-baseline; then baseline+=(--portable-baseline /root/bench/driver-baseline); fi
if test -f driver-legacy-baseline; then baseline+=(--legacy-baseline /root/bench/driver-legacy-baseline); fi
python3 secure-runner.py --rust /root/bench/driver --cpp /root/bench/capnp-reference "${baseline[@]}" --output /root/results/secure/native --repetitions 5 --warmup 100 --iterations 1000
for n in 10 100; do
    python3 secure-runner.py --rust /root/bench/driver --cpp /root/bench/capnp-reference "${baseline[@]}" --output /root/results/secure/trace-$n --repetitions 5 --warmup 2 --iterations "$n" --cases dependent-read,pipelined-read,fanout-32,concurrent-32,services-4-deep-16-clients-8 --trace
done
python3 secure-profile.py --rust /root/bench/driver --cpp /root/bench/capnp-reference "${baseline[@]}" --output /root/results/secure/profiles
} 2>&1 | tee /root/results/secure/runner.log'"#
    };
    let measured = commands.ssh(
        &host.options,
        &host.public.to_string(),
        command,
        // The complete 48-case DHAT matrix alone took 57 minutes on the
        // dedicated host. Leave room for the native and Callgrind phases.
        7200,
        cancel,
    );
    // Salvage partial output independently of failure/cancellation, before the
    // outer owner unconditionally destroys the hosts and their SSH keys.
    let cleanup = Cancellation::default();
    let packed = commands.ssh(
        &host.options,
        &host.public.to_string(),
        "tar -czf /root/results/secure.tar.gz -C /root/results secure",
        180,
        &cleanup,
    );
    let downloaded = commands.download(
        &host.options,
        &host.public.to_string(),
        output,
        "secure.tar.gz",
        180,
        &cleanup,
    );
    measured?;
    packed?;
    downloaded?;
    Ok(())
}

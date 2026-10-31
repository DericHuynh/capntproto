# Runtime and externally driven transport benchmarks

`runtime-bench` uses one actual Cap'n Proto schema/service for all profiles,
with explicit trust and TLS 1.3 on every connection. Both peers run in the same
single-thread runtime; counters therefore include both client and server. The
legacy and portable RPC implementations are selectable independently of IO.
Setup, certificate generation, warmup and teardown are outside the measured
region. The final stdout line is the JSON result; the RPC library may print an expected disconnect diagnostic first. Validation and driver errors fail the process.

Build from the repository root (the same release profile must be used for both
frozen and candidate):

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo build --locked --release --manifest-path benchmarks/runtimes/Cargo.toml
# Run once and reuse these exact certificates for every worker.
target/release/runtime-bench --init-certificates target/runtime-certificates
```

Cargo normally uses `benchmarks/runtimes/target` for this isolated package. Set
`CARGO_TARGET_DIR=target` to use the paths above, or adjust them accordingly.

| Option | Values |
| --- | --- |
| `--runtime` | `tokio` (direct TLS / facade WebTransport), `tokio-driven`, `monoio-epoll`, `monoio-uring` |
| `--transport` | `tls`, `webtransport` |
| `--driver` | `legacy`, `portable` |
| `--case` | `unary`, `large` (100KB), `dependent` (32 awaited capability hops), `pipelined` (32 promise-pipelined hops), `concurrent-32`, `deep-32-concurrent-32`, `callback` |
| `--iterations` / `--warmup` | Positive measured operations / unmeasured operations, defaults 32 / 4 |
| `--certificates` | Directory containing `server.der` and `server-key.der` |

Every workload verifies returned values, payload size/content and completion.
The pipelined cases retain all parent promises and check their completion.
io_uring initialization fails explicitly if unsupported; this profile never
silently falls back to epoll.

```sh
target/release/runtime-bench --runtime monoio-uring --transport tls \
  --driver portable --case dependent --iterations 32 --warmup 4 \
  --certificates target/runtime-certificates
```

Run **five separate processes per configuration**, retaining all JSON results.
Report the lowest observed value **per metric** as requested; these minima need
not come from the same run. Include mean and min/max to expose variation. Do not
interpret a latency difference as a CPU improvement.

The allocator counts successful allocation/reallocation operations and requested
bytes inside the measured region. TLS read/write counts are completed ciphertext
IO operations. WebTransport counts are completed **RPC stream** operations,
including partial acceptance, not UDP packets or kernel syscalls. The `io_level`
field records this distinction. Never compare those counts across transports.
All builds include the same atomic-counter overhead.

`cycles` uses Linux perf_event hardware counters, excluding kernel/hypervisor.
A denied/unavailable PMU returns null with the actual OS error; do not replace it
with wall time. `cpu_ns` is process CPU time, still subject to frequency/scheduler
variation. Collect instruction/branch counts separately under Callgrind:

```sh
# Headers are needed at build time; missing headers are explicitly reported by
# callgrind_markers=false, which invalidates a measured-region instruction run.
CAPNTPROTO_VALGRIND_INCLUDE=/usr/include cargo build --locked --release \
  --manifest-path benchmarks/runtimes/Cargo.toml
valgrind --tool=callgrind --instr-atstart=no --branch-sim=yes \
  --callgrind-out-file=target/runtime.callgrind \
  target/release/runtime-bench --runtime tokio-driven --transport tls \
  --driver portable --case pipelined --iterations 8 --warmup 4 \
  --certificates target/runtime-certificates
```

Use the dumped measured part (`runtime.callgrind.1`), its `events` and `summary`
fields, and divide counters by `iterations`. `Ir` is guest instructions; `Bcm`
and `Bim` are **simulated** conditional/indirect branch misses, not hardware PMU
measurements. Qualify io_uring support in the installed Valgrind before using
that runtime: require successful workload validation without unsupported-syscall
warnings. Valgrind 3.24.0 passed that check in this workspace. Keep native and
Callgrind measurements separate.

## Frozen comparison

`baseline.toml` pins the pre-change commit. Create a detached worktree at that
SHA, copy this benchmark package there, and build without its new optional
features (`--no-default-features --features webtransport`). The worker's
`cfg(feature="driven")` / `cfg(feature="monoio")` code then disappears; only
`--runtime tokio` exists on the frozen implementation. Remove the `driven`
feature's `capntproto-transport/tokio` mapping and the monoio feature's transport
mapping from the copied manifest, since those features did not exist at the
frozen commit. Seed its lockfile from this benchmark lockfile and verify shared
registry package versions match. Keep the benchmark source, certificates,
compiler, optimization profile, iterations and instrumentation identical.

Store binaries, raw JSON and Callgrind output under ignored `target` directories.
Do not replace the baseline with a later main branch while comparing changes.

## Workspace comparison (2026-10-31)

Frozen commit: `256241c2801c9b2651f19e335b71f1a0d3155386`. Rust 1.97.0, release/fat LTO, one runtime thread, identical service, certificates and shared registry versions. Each configuration ran five times natively and five times under Valgrind 3.24.0: **1,400 measurements**. Each pass warmed up twice. Native samples use 16 measured iterations and Callgrind samples use 4, except deep/concurrent uses 4 and 1 respectively. Values below are minimum guest instructions per complete workload, including both peers. Dependent/pipelined workloads contain 32 capability hops; deep/concurrent contains 32 concurrent chains of depth 32.

The TLS completion benchmark counts IO around native owned socket halves, matching the public adapters. It does not insert a generic split mutex. The correction was qualified with a separate worker build; each raw record identifies its binary SHA-256. Native allocation/IO results and simulated instruction/branch results remain separate.

### Portable RPC over TLS

| Case | Frozen Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 48,582 | 49,274 | 62,704 | 61,470 | 59,463 |
| dependent | 3,446,281 | 3,485,057 | 3,787,784 | 3,684,438 | 3,926,620 |
| pipelined | 3,785,524 | 3,797,628 | 3,235,813 | 3,230,418 | 3,239,580 |
| concurrent-32 | 1,047,902 | 1,053,885 | 815,228 | 813,738 | 812,242 |
| callback | 139,132 | 140,682 | 161,570 | 160,529 | 156,666 |
| deep-32-concurrent-32 | 423,404,705 | 423,868,911 | 196,279,525 | 310,155,521 | 304,998,166 |
| large | 1,791,923 | 1,792,796 | 1,819,203 | 1,816,145 | 1,818,157 |

### Portable RPC over WEBTRANSPORT

| Case | Frozen Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 88,841 | 89,362 | 97,325 | 92,937 | 95,403 |
| dependent | 5,248,190 | 5,275,446 | 5,580,922 | 5,373,347 | 5,854,277 |
| pipelined | 3,534,138 | 3,533,130 | 3,623,777 | 3,585,310 | 3,702,400 |
| concurrent-32 | 972,629 | 975,614 | 1,040,634 | 1,041,915 | 1,028,540 |
| callback | 213,460 | 214,893 | 227,700 | 220,194 | 224,423 |
| deep-32-concurrent-32 | 223,389,116 | 221,390,565 | 229,424,237 | 382,330,322 | 274,360,114 |
| large | 2,467,504 | 2,488,741 | 2,974,244 | 3,007,877 | 4,519,205 |

### Legacy RPC over TLS

| Case | Frozen Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 53,360 | 54,254 | 59,730 | 55,623 | 53,339 |
| dependent | 2,771,573 | 2,811,157 | 2,468,294 | 2,332,502 | 2,258,198 |
| pipelined | 2,516,528 | 2,537,940 | 1,458,704 | 1,449,052 | 1,446,814 |
| concurrent-32 | 1,087,978 | 1,098,986 | 608,998 | 595,198 | 603,462 |
| callback | 135,472 | 137,492 | 141,486 | 129,811 | 125,490 |
| deep-32-concurrent-32 | 87,032,018 | 87,766,257 | 54,879,347 | 54,744,789 | 54,883,697 |
| large | 1,689,038 | 1,689,614 | 1,728,344 | 1,722,825 | 1,724,225 |

### Legacy RPC over WEBTRANSPORT

| Case | Frozen Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 84,721 | 85,365 | 99,760 | 91,565 | 96,065 |
| dependent | 3,321,042 | 3,353,424 | 3,743,744 | 3,494,196 | 3,557,794 |
| pipelined | 2,232,220 | 2,236,943 | 2,294,428 | 2,280,101 | 2,301,924 |
| concurrent-32 | 867,617 | 873,288 | 950,046 | 944,862 | 972,533 |
| callback | 192,072 | 193,582 | 218,765 | 199,922 | 204,045 |
| deep-32-concurrent-32 | 189,749,629 | 185,439,367 | 457,504,614 | 236,135,441 | 79,760,635 |
| large | 2,428,361 | 2,425,597 | 2,924,764 | 2,939,898 | 4,408,646 |

### Spread and branch counts

Portable TLS `concurrent-32`, per workload. Each cell is **minimum / mean / min–max** across five runs; Bcm/Bim are simulated misses.

| Runtime | Ir | Bcm | Bim |
| --- | ---: | ---: | ---: |
| Frozen Tokio | 1,047,902.5 / 1,048,298.4 / 1,047,902.5–1,049,607.0 | 6,654.0 / 6,662.6 / 6,654.0–6,693.0 | 1,043.2 / 1,052.0 / 1,043.2–1,054.2 |
| tokio | 1,053,884.8 / 1,054,949.6 / 1,053,884.8–1,055,718.0 | 6,707.0 / 6,714.5 / 6,707.0–6,728.0 | 1,195.2 / 1,200.2 / 1,195.2–1,207.8 |
| tokio-driven | 815,227.8 / 815,227.8 / 815,227.8–815,227.8 | 5,740.0 / 5,740.0 / 5,740.0–5,740.0 | 792.0 / 792.0 / 792.0–792.0 |
| monoio-epoll | 813,737.8 / 813,737.8 / 813,737.8–813,737.8 | 5,498.5 / 5,498.5 / 5,498.5–5,498.5 | 631.0 / 631.0 / 631.0–631.0 |
| monoio-uring | 812,242.0 / 812,242.0 / 812,242.0–812,242.0 | 5,527.5 / 5,527.5 / 5,527.5–5,527.5 | 633.0 / 633.0 / 633.0–633.0 |

TLS batching is the main gain: portable concurrent-32 falls from 64 to 2 completed ciphertext writes and from 265 to 202.19 allocation operations per workload. Monoio io_uring reduces instructions by 22.5% there and 14.4% for pipelined calls. It increases instructions by 22.4% for unary and 13.9% for dependent calls; it is not uniformly faster. Driven Tokio is strongest for the deep concurrent TLS workload.

The new QUIC path has a remaining packet scheduling/fragmentation cost. In the large portable workload, monoio io_uring completes 87 RPC stream reads and about 803 allocations, versus 2 reads and 51 allocations in frozen Tokio. Its instruction count is 83.2% higher. The mature Tokio WebTransport implementation remains the default; the externally driven core is explicitly selectable. Batched UDP receive, fewer timer re-registrations and datagram buffer reuse are the next optimization targets.

Linux hardware cycle counters were unavailable (`perf_event_open` returned ENOENT); `cycles` is null. No elapsed-time value is substituted for hardware cycles. The retained JSON in ignored `target/runtime-comparison` contains every sample and minimum/mean/maximum/spread for Ir, Bcm, Bim, allocations, bytes and IO counts, plus process CPU time. These workspace numbers are not CI droplet results.

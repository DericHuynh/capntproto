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


## Monoio optimization comparison (2026-10-31)

Baseline: `36d2db1f794e3a8ad5bd283fd5991eda38ef7013`, the initial externally driven runtime branch. These tables compare the monoio implementation before and after the UDP ready path, shared receive storage, retained QUIC alarm and TLS pending-read guard. Both RPC drivers were tested with both monoio backends and both transports: **1,120 measurements**, five native and five Callgrind runs per configuration/build. Compiler, registry versions, certificates, benchmark source, instrumentation, profile, iteration counts and warmup match the initial comparison above. Minima are selected independently per metric; percentages use unrounded minima. These are local instruction/allocation measurements, not CI droplet latency results.

Instruction minima fall in 51 of 56 profiles. Allocation counts fall in 30 and remain unchanged in 26. The largest remaining instruction regression is portable deep/concurrent QUIC on epoll (+3.8%). Simulated branch misses remain mixed: the largest Bcm increase is legacy pipelined TLS on io_uring (+9.6%), and the largest Bim increase is portable deep/concurrent TLS on io_uring (+15.9%). These changes reduce allocation operations and most instruction counts; they do not establish uniform performance parity.

The ready UDP path removes Monoio sendmsg/recvmsg metadata allocations for immediately available epoll/kqueue IO. io_uring retains its native completion boundaries to preserve application/ACK scheduling and avoid amplifying Quinn unacknowledged-fragment lookup costs. Shared initialized receive slabs remove the payload copy into QUIC. The retained timer avoids repeated registration/cancellation; Monoio timers are in-place, so this is a CPU optimization rather than removal of a timer heap allocation. TLS skips record processing while a pending receive owns the ciphertext buffer. The existing cooperative work limit also polls pending sends, preventing a receive flood from starving transmission while retaining receive batching.

### Portable RPC over TLS

| Case | Epoll Ir before → after | Δ Bcm | Δ Bim | io_uring Ir before → after | Δ Bcm | Δ Bim |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary | 61,470 → 56,993 | -2.0% | +3.6% | 59,463 → 58,089 | -3.4% | +3.7% |
| dependent | 3,684,446 → 3,518,746 | -3.8% | +10.7% | 3,926,627 → 3,801,892 | -5.7% | +9.1% |
| pipelined | 3,230,025 → 3,224,523 | -0.1% | +10.1% | 3,238,306 → 3,235,854 | -0.1% | +10.1% |
| concurrent-32 | 813,737 → 809,332 | +3.6% | -17.1% | 812,039 → 810,600 | +3.8% | -16.2% |
| callback | 160,527 → 152,010 | -2.4% | -0.4% | 156,678 → 154,226 | -3.8% | +3.0% |
| large | 1,816,122 → 1,811,659 | -0.6% | +13.5% | 1,818,113 → 1,816,739 | -2.6% | +13.8% |
| deep-32-concurrent-32 | 310,141,465 → 310,118,218 | -1.0% | +13.7% | 304,865,389 → 304,845,606 | -0.6% | +15.9% |

### Legacy RPC over TLS

| Case | Epoll Ir before → after | Δ Bcm | Δ Bim | io_uring Ir before → after | Δ Bcm | Δ Bim |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary | 55,623 → 55,074 | -3.9% | -16.2% | 53,339 → 52,801 | -2.5% | -16.2% |
| dependent | 2,332,501 → 2,314,362 | +2.3% | -17.0% | 2,255,981 → 2,238,306 | +3.1% | -15.2% |
| pipelined | 1,449,014 → 1,448,446 | +8.0% | -7.9% | 1,446,819 → 1,446,311 | +9.6% | -8.1% |
| concurrent-32 | 595,198 → 594,648 | -3.3% | -10.0% | 603,558 → 603,022 | -2.7% | -9.1% |
| callback | 129,811 → 128,712 | -1.6% | -7.8% | 125,490 → 124,418 | -0.9% | -6.8% |
| large | 1,722,530 → 1,721,995 | -0.0% | -9.9% | 1,724,361 → 1,723,858 | +1.2% | -10.7% |
| deep-32-concurrent-32 | 54,769,545 → 54,751,383 | +5.7% | -4.6% | 54,933,861 → 54,978,116 | +2.8% | -4.4% |

### Portable RPC over WEBTRANSPORT

| Case | Epoll Ir before → after | Δ Bcm | Δ Bim | io_uring Ir before → after | Δ Bcm | Δ Bim |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary | 96,140 → 90,482 | +0.6% | -18.8% | 95,673 → 93,309 | +0.4% | -13.8% |
| dependent | 5,379,684 → 5,199,982 | -0.2% | -13.5% | 5,842,852 → 5,672,895 | -2.1% | -11.0% |
| pipelined | 3,585,255 → 3,565,172 | -1.4% | +8.3% | 3,703,412 → 3,682,969 | -1.7% | +3.0% |
| concurrent-32 | 1,042,632 → 1,024,534 | -1.5% | -20.9% | 1,028,595 → 1,014,794 | -0.7% | -13.2% |
| callback | 220,144 → 215,469 | +1.8% | -10.6% | 224,468 → 219,431 | -0.3% | -8.6% |
| large | 3,012,056 → 2,985,984 | -24.7% | -17.5% | 4,518,722 → 4,503,888 | -8.3% | -15.6% |
| deep-32-concurrent-32 | 349,722,103 → 362,918,114 | +0.1% | +15.8% | 273,677,604 → 274,565,985 | -0.4% | +8.4% |

### Legacy RPC over WEBTRANSPORT

| Case | Epoll Ir before → after | Δ Bcm | Δ Bim | io_uring Ir before → after | Δ Bcm | Δ Bim |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary | 92,810 → 88,885 | +3.5% | -13.0% | 96,085 → 92,922 | +1.9% | -10.6% |
| dependent | 3,493,893 → 3,363,477 | +8.3% | -6.1% | 3,554,142 → 3,431,918 | +6.6% | -7.0% |
| pipelined | 2,278,644 → 2,246,592 | +0.6% | -9.0% | 2,301,440 → 2,243,866 | -2.9% | -7.9% |
| concurrent-32 | 944,636 → 930,254 | -7.1% | -12.2% | 972,045 → 957,579 | -3.0% | -5.6% |
| callback | 199,836 → 192,997 | +2.8% | -8.1% | 204,160 → 198,836 | +1.5% | -6.3% |
| large | 2,935,694 → 2,939,382 | -23.7% | -15.2% | 4,404,795 → 4,423,038 | -7.8% | -15.8% |
| deep-32-concurrent-32 | 232,046,818 → 196,660,884 | -4.9% | -10.7% | 79,702,006 → 79,501,869 | -1.3% | -4.4% |

### Native QUIC allocations and RPC stream reads

Minimum allocation operations and completed RPC stream reads per workload. These are not UDP packet or syscall counts.

**Portable RPC**

| Case | Epoll allocations | Epoll reads | io_uring allocations | io_uring reads |
| --- | ---: | ---: | ---: | ---: |
| unary | 21.00 → 13.00 | 2.00 → 2.00 | 21.00 → 17.00 | 2.00 → 2.00 |
| dependent | 1,972.88 → 1,326.25 | 66.19 → 66.06 | 1,968.62 → 1,709.38 | 97.50 → 97.44 |
| pipelined | 2,334.50 → 2,255.19 | 2.06 → 2.06 | 2,337.12 → 2,306.25 | 14.00 → 14.00 |
| concurrent-32 | 336.19 → 280.31 | 2.00 → 2.00 | 329.38 → 311.38 | 8.00 → 8.00 |
| callback | 66.00 → 50.00 | 4.00 → 4.00 | 66.00 → 58.00 | 4.00 → 4.00 |
| large | 570.56 → 38.19 | 2.69 → 2.69 | 802.88 → 546.12 | 87.00 → 87.00 |
| deep-32-concurrent-32 | 74,259.50 → 72,283.50 | 9.25 → 9.75 | 74,710.50 → 73,985.25 | 332.00 → 332.25 |

**Legacy RPC**

| Case | Epoll allocations | Epoll reads | io_uring allocations | io_uring reads |
| --- | ---: | ---: | ---: | ---: |
| unary | 23.00 → 15.00 | 2.00 → 2.00 | 23.00 → 19.00 | 2.00 → 2.00 |
| dependent | 1,908.88 → 1,643.38 | 66.00 → 66.00 | 1,910.25 → 1,777.62 | 66.00 → 66.00 |
| pipelined | 1,821.06 → 1,747.38 | 2.00 → 2.00 | 1,832.62 → 1,805.69 | 12.00 → 12.00 |
| concurrent-32 | 406.00 → 350.12 | 2.06 → 2.00 | 409.19 → 391.31 | 8.00 → 8.00 |
| callback | 73.00 → 57.00 | 4.00 → 4.00 | 73.00 → 65.00 | 4.00 → 4.00 |
| large | 568.56 → 38.19 | 3.25 → 3.25 | 801.62 → 545.25 | 87.00 → 87.00 |
| deep-32-concurrent-32 | 62,168.75 → 60,080.50 | 8.25 → 8.25 | 60,170.25 → 59,448.50 | 331.25 → 331.25 |

### Repetition spread

Portable RPC, io_uring, guest instructions per workload. Each entry is **minimum / mean / min–max** over five separate processes.

| Transport / case | Before | After |
| --- | ---: | ---: |
| tls / unary | 59,463.2 / 59,463.3 / 59,463.2–59,463.8 | 58,089.2 / 58,089.3 / 58,089.2–58,089.8 |
| tls / pipelined | 3,238,306.5 / 3,238,809.8 / 3,238,306.5–3,240,318.8 | 3,235,853.5 / 3,236,581.0 / 3,235,853.5–3,239,150.0 |
| webtransport / large | 4,518,721.8 / 4,529,503.7 / 4,518,721.8–4,567,368.0 | 4,503,887.5 / 4,524,943.5 / 4,503,887.5–4,554,872.2 |
| webtransport / deep-32-concurrent-32 | 273,677,604.0 / 274,339,665.0 / 273,677,604.0–274,902,985.0 | 274,565,985.0 / 274,838,080.6 / 274,565,985.0–274,971,104.0 |

Receive storage starts at 128 KiB per endpoint rather than 64 KiB; a retained packet can pin a slab. Fewer allocation operations therefore do not guarantee fewer requested bytes. The readiness fast paths remove userspace operation metadata without batching UDP syscalls. io_uring retains native completion boundaries. Whole-process syscall traces (including setup, warmup and teardown) are retained beside the measured-region data; userspace instruction gains do not establish a reduction in kernel CPU. UDP segmentation/receive offload remains an optimization opportunity.

Validation: 32 secure RPC integration tests and 17 transport unit tests pass, including epoll zero-allocation ready UDP operations and io_uring completion ownership, empty/dual-stack datagrams, completion fallback, live packet ownership, timer preemption/PTO and receive-flood send progress. Transport and benchmark Clippy pass with warnings denied; monoio client/server examples and the no-std Embassy target compile. Native monoio transport dependencies contain no Tokio.

Hardware cycles remain unavailable (`perf_event_open`: ENOENT). `target/monoio-optimization/samples.jsonl` retains every sample and binary SHA-256; `summary.json` contains minimum/mean/maximum/spread for Ir, Bcm, Bim, allocations, bytes, IO and process CPU time. Raw Callgrind profiles are compressed alongside those ignored artifacts.

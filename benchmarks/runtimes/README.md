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

For allocation ownership stacks, opt into DHAT **ad-hoc** events in a separate
worker build. Each successful measured-region allocation/reallocation records
one event weighted by its requested bytes. Setup, warmup and teardown produce
no events. This instrumentation is compiled out by default:

```sh
CAPNTPROTO_ALLOCATION_TRACE=1 CAPNTPROTO_VALGRIND_INCLUDE=/usr/include \
  cargo build --locked --release --manifest-path benchmarks/runtimes/Cargo.toml
valgrind --tool=dhat --mode=ad-hoc --num-callers=32 \
  --dhat-out-file=target/runtime-allocations.json \
  target/release/runtime-bench --runtime monoio-epoll --transport tls \
  --driver portable --case pipelined --iterations 4 --warmup 2 \
  --certificates target/runtime-certificates
```

In the DHAT JSON, sum `pps[].tbk` for allocation operations and `pps[].tb` for
requested bytes; require exact agreement with the worker's `sample.blocks` and
`sample.bytes`. Group stacks by the RPC/transport owner, leaving workload task
allocations visible. Rebuild with `CAPNTPROTO_ALLOCATION_TRACE` unset before
native timing or Callgrind instruction comparisons; keep the worker binaries
and their SHA-256 hashes separate.

`wall_ns` measures elapsed time around only the validated RPC workload loop,
after warmup and before disconnect/teardown. Divide by `iterations` for average
time per workload; throughput is `iterations * 1e9 / wall_ns` workloads/second.
A workload may include multiple RPCs: concurrent-32 contains 32 reads, and the
deep case contains 32 concurrent chains of 32 capability hops plus their reads.
Batch averages and five-run spread are not per-RPC tail latency percentiles.
The stream counter forwards vectored writes to the underlying transport.

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

## Native elapsed-time comparison (2026-10-31)

Actual native runs, without Valgrind: **1,260 samples / 252 configurations**, five independent processes per configuration. The original Tokio reference is `256241c2801c9b2651f19e335b71f1a0d3155386`; the pre-optimization runtime branch is `36d2db1f794e3a8ad5bd283fd5991eda38ef7013`; current implementation code is `5bc79a117cc4b075e1ff680e2f1304f2970cacc4`. Every worker uses the same updated benchmark sources, including measured-region elapsed time and vectored-write forwarding, with matching shared registry versions/checksums. Earlier instruction tables used the earlier harness and a separate measurement run.

AMD EPYC 9V74 virtual machine, Linux x86-64, Rust 1.97.0, release/fat LTO, one runtime thread. Both loopback peers share that runtime. Every process is pinned to CPU 2; runs are sequential, with no concurrent build or profiler. Builds/runtimes are interleaved across repetitions. These are local workspace results, not CI droplet or WAN measurements. TLS 1.3 is enabled throughout; WebTransport uses QUIC/HTTP3. Connection setup, warmup and teardown are outside the measured interval. Every workload validates values and payload contents.

| Case | Measured iterations | Warmup iterations |
| --- | ---: | ---: |
| unary | 16,384 | 64 |
| dependent | 256 | 8 |
| pipelined | 512 | 8 |
| concurrent-32 | 1,024 | 16 |
| callback | 4,096 | 32 |
| large | 1,024 | 16 |
| deep-32-concurrent-32 | 16 | 2 |

Each timing cell below is **minimum / mean / spread**, in **µs per complete workload**. Spread is maximum minus minimum across the five process-level batch averages. These are not individual-RPC p95/p99 latencies. Lower elapsed time is better; no instruction-count estimate is substituted for elapsed time.

### Native portable RPC over TCP/TLS

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 13.06 / 13.59 / 1.26 | 12.94 / 13.59 / 1.79 | 15.01 / 16.46 / 5.89 | 13.87 / 14.66 / 2.63 | 14.53 / 15.09 / 1.39 |
| dependent | 1,035.24 / 1,061.09 / 56.68 | 1,016.47 / 1,045.13 / 62.10 | 999.37 / 1,026.43 / 70.71 | 945.38 / 988.17 / 150.08 | 1,051.83 / 1,060.07 / 26.91 |
| pipelined | 783.23 / 849.88 / 126.90 | 794.58 / 824.28 / 92.32 | 441.32 / 475.73 / 100.81 | 437.51 / 475.12 / 70.10 | 455.04 / 483.37 / 70.37 |
| concurrent-32 | 257.26 / 273.01 / 30.53 | 256.22 / 273.49 / 38.08 | 99.51 / 106.55 / 18.02 | 111.03 / 116.55 / 10.49 | 96.90 / 109.59 / 23.40 |
| callback | 42.74 / 45.56 / 9.45 | 43.73 / 46.33 / 6.89 | 40.63 / 43.53 / 6.48 | 39.50 / 40.01 / 1.01 | 40.82 / 41.60 / 1.35 |
| large | 134.54 / 138.73 / 9.60 | 129.61 / 137.21 / 13.71 | 128.49 / 132.61 / 7.95 | 129.20 / 143.41 / 48.80 | 132.36 / 136.34 / 8.70 |
| deep-32-concurrent-32 | 38,952.44 / 42,446.81 / 6,116.22 | 42,197.43 / 44,184.70 / 3,446.63 | 22,389.05 / 23,176.99 / 2,288.28 | 31,060.81 / 32,652.82 / 3,219.04 | 32,585.74 / 33,051.98 / 1,847.62 |

### Native legacy RPC over TCP/TLS

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 19.39 / 19.73 / 0.81 | 19.52 / 19.95 / 0.87 | 13.55 / 14.16 / 1.38 | 12.66 / 12.78 / 0.44 | 12.60 / 12.84 / 0.50 |
| dependent | 1,000.71 / 1,035.31 / 74.80 | 1,016.93 / 1,042.05 / 53.15 | 517.00 / 556.34 / 75.12 | 483.05 / 534.55 / 81.35 | 493.95 / 539.38 / 68.88 |
| pipelined | 798.07 / 849.14 / 111.71 | 814.57 / 868.35 / 108.64 | 159.24 / 174.46 / 32.95 | 162.43 / 170.17 / 18.57 | 175.95 / 186.07 / 19.98 |
| concurrent-32 | 370.56 / 388.28 / 46.19 | 376.10 / 385.67 / 25.24 | 68.51 / 76.71 / 16.82 | 68.08 / 74.31 / 16.62 | 72.04 / 77.73 / 17.25 |
| callback | 49.43 / 50.83 / 3.75 | 46.28 / 50.67 / 7.71 | 32.49 / 33.28 / 1.95 | 28.68 / 31.26 / 4.74 | 29.39 / 31.49 / 3.92 |
| large | 128.34 / 130.07 / 3.94 | 127.88 / 132.52 / 10.43 | 124.81 / 129.43 / 8.84 | 123.17 / 129.27 / 14.73 | 126.23 / 129.87 / 12.60 |
| deep-32-concurrent-32 | 23,492.05 / 26,192.27 / 5,237.98 | 24,099.12 / 26,674.05 / 6,481.23 | 6,026.62 / 6,259.20 / 410.08 | 6,060.99 / 6,182.91 / 417.50 | 6,381.46 / 6,984.54 / 1,208.73 |

### Native portable RPC over QUIC/WebTransport

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 18.39 / 18.56 / 0.31 | 18.23 / 18.94 / 1.17 | 18.90 / 19.42 / 0.90 | 18.08 / 18.71 / 1.39 | 18.41 / 19.02 / 1.14 |
| dependent | 1,419.91 / 1,462.35 / 101.91 | 1,396.19 / 1,450.15 / 83.14 | 1,330.89 / 1,406.27 / 242.68 | 1,251.69 / 1,349.94 / 245.82 | 1,394.83 / 1,478.67 / 125.79 |
| pipelined | 523.24 / 549.81 / 71.50 | 521.89 / 550.86 / 68.83 | 537.26 / 576.05 / 69.93 | 503.43 / 551.70 / 100.37 | 655.95 / 702.19 / 92.36 |
| concurrent-32 | 123.87 / 139.60 / 37.61 | 131.64 / 135.63 / 11.04 | 150.33 / 156.67 / 14.34 | 150.04 / 155.85 / 14.43 | 158.16 / 172.99 / 43.97 |
| callback | 53.35 / 58.33 / 8.58 | 57.05 / 60.08 / 7.66 | 50.50 / 55.86 / 8.98 | 52.36 / 54.67 / 5.48 | 53.72 / 56.72 / 6.03 |
| large | 242.79 / 259.64 / 42.05 | 244.30 / 256.53 / 24.26 | 496.72 / 513.23 / 30.56 | 490.77 / 511.33 / 53.55 | 784.79 / 813.84 / 56.39 |
| deep-32-concurrent-32 | 24,940.89 / 25,470.16 / 860.54 | 25,314.00 / 26,108.61 / 1,959.40 | 26,451.48 / 28,069.46 / 5,045.84 | 37,973.69 / 39,064.49 / 2,818.72 | 39,903.34 / 40,784.10 / 2,487.09 |

### Native legacy RPC over QUIC/WebTransport

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 16.71 / 18.29 / 4.33 | 16.42 / 18.06 / 3.53 | 17.65 / 19.04 / 2.49 | 16.18 / 17.76 / 3.14 | 16.01 / 17.82 / 2.89 |
| dependent | 655.55 / 691.97 / 71.16 | 659.40 / 685.20 / 55.37 | 674.57 / 698.03 / 50.84 | 646.55 / 679.17 / 49.29 | 649.87 / 687.27 / 86.89 |
| pipelined | 237.75 / 248.58 / 24.66 | 248.61 / 257.67 / 15.60 | 262.88 / 271.72 / 17.37 | 255.35 / 265.38 / 18.67 | 304.24 / 320.93 / 35.78 |
| concurrent-32 | 102.25 / 106.47 / 6.65 | 104.13 / 107.50 / 9.60 | 126.29 / 131.88 / 16.20 | 125.10 / 130.33 / 15.22 | 129.51 / 135.82 / 12.07 |
| callback | 38.52 / 40.25 / 6.27 | 38.83 / 39.90 / 3.09 | 39.94 / 43.28 / 6.97 | 36.34 / 37.51 / 2.67 | 37.53 / 38.65 / 3.67 |
| large | 236.59 / 243.86 / 15.47 | 235.73 / 247.32 / 22.66 | 468.56 / 484.51 / 45.15 | 468.29 / 478.08 / 29.09 | 766.61 / 779.93 / 19.29 |
| deep-32-concurrent-32 | 16,831.37 / 17,650.27 / 1,902.58 | 16,816.05 / 17,777.43 / 2,255.45 | 23,532.13 / 23,763.03 / 623.14 | 16,961.60 / 17,682.20 / 1,447.52 | 9,555.78 / 10,401.42 / 1,524.54 |

### Observed pre/post timing differences

Change in minimum elapsed time from `36d2db1` to `5bc79a1`, with the same timed harness. Negative values indicate less elapsed time. The direct Tokio path is also shown to expose run variation in a largely unchanged reference. Consult the absolute timing spread above and the full sample data when interpreting small differences.

Minimum elapsed time improves in 31 of the 56 monoio comparisons and rises in 25. The 100 KB TLS pre/post differences are +29–33% on monoio, but the direct Tokio control also shifts +23–28% in that case. That discrepancy needs isolation before attributing the full change to a particular optimization. Allocation reductions alone do not establish an elapsed-time improvement.

**Portable / tls**

| Case | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: |
| unary | -0.8% | +3.5% | -5.7% | -2.3% |
| dependent | -2.2% | -0.1% | -0.4% | -1.4% |
| pipelined | +1.5% | -1.0% | 0.0% | +4.3% |
| concurrent-32 | +2.3% | -3.2% | +9.5% | -7.7% |
| callback | +5.4% | -2.5% | -2.5% | +5.2% |
| large | +22.8% | +32.1% | +32.8% | +32.5% |
| deep-32-concurrent-32 | +2.8% | +3.1% | +1.0% | +5.9% |

**Legacy / tls**

| Case | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: |
| unary | 0.0% | -3.2% | -0.5% | -0.2% |
| dependent | +2.4% | -1.1% | -0.1% | +0.2% |
| pipelined | +2.3% | -2.3% | +2.6% | +5.0% |
| concurrent-32 | +1.9% | -6.3% | +0.6% | +4.5% |
| callback | -6.0% | +0.6% | -1.4% | +1.3% |
| large | +27.6% | +32.0% | +30.9% | +29.3% |
| deep-32-concurrent-32 | -2.8% | +1.9% | -0.3% | -1.2% |

**Portable / webtransport**

| Case | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: |
| unary | -1.5% | -3.1% | -2.3% | -1.1% |
| dependent | +0.9% | -2.3% | -1.6% | -4.7% |
| pipelined | -1.6% | -1.7% | -1.0% | +5.2% |
| concurrent-32 | -0.6% | +0.3% | +0.6% | -3.0% |
| callback | +7.5% | -10.7% | -2.3% | -2.5% |
| large | +13.6% | +9.9% | +3.2% | +1.9% |
| deep-32-concurrent-32 | -0.2% | +3.3% | +4.3% | +6.9% |

**Legacy / webtransport**

| Case | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: |
| unary | -1.7% | -1.6% | -4.1% | -8.5% |
| dependent | -0.4% | -5.8% | -2.4% | -3.3% |
| pipelined | +0.6% | -5.9% | -4.3% | +1.7% |
| concurrent-32 | +0.3% | -3.0% | -2.1% | -3.3% |
| callback | +3.4% | -3.8% | -2.3% | -2.1% |
| large | +15.0% | +11.1% | +3.6% | +2.5% |
| deep-32-concurrent-32 | -10.4% | +4.1% | -6.0% | -2.5% |

### Throughput

Best observed **workloads/second**, derived from the minimum batch time. A concurrent-32 workload contains 32 independent reads; a deep workload contains 32 simultaneous chains, each with 32 capability hops and a final read. Rates across different workload types are not equivalent service-request rates.

**Portable / tls**

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 76,547.8 | 77,290.8 | 66,625.9 | 72,122.8 | 68,813.9 |
| dependent | 966.0 | 983.8 | 1,000.6 | 1,057.8 | 950.7 |
| pipelined | 1,276.8 | 1,258.5 | 2,265.9 | 2,285.6 | 2,197.6 |
| concurrent-32 | 3,887.1 | 3,902.9 | 10,049.1 | 9,006.4 | 10,320.0 |
| callback | 23,396.8 | 22,866.1 | 24,610.9 | 25,313.3 | 24,495.5 |
| large | 7,432.5 | 7,715.6 | 7,782.9 | 7,739.8 | 7,555.0 |
| deep-32-concurrent-32 | 25.7 | 23.7 | 44.7 | 32.2 | 30.7 |

**Legacy / tls**

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 51,564.3 | 51,217.1 | 73,801.4 | 79,018.3 | 79,391.7 |
| dependent | 999.3 | 983.3 | 1,934.2 | 2,070.2 | 2,024.5 |
| pipelined | 1,253.0 | 1,227.6 | 6,279.6 | 6,156.4 | 5,683.4 |
| concurrent-32 | 2,698.6 | 2,658.9 | 14,596.8 | 14,688.0 | 13,881.6 |
| callback | 20,229.8 | 21,609.3 | 30,783.1 | 34,866.0 | 34,030.9 |
| large | 7,791.7 | 7,820.0 | 8,012.3 | 8,118.8 | 7,921.9 |
| deep-32-concurrent-32 | 42.6 | 41.5 | 165.9 | 165.0 | 156.7 |

**Portable / webtransport**

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 54,378.1 | 54,867.7 | 52,900.9 | 55,304.9 | 54,315.7 |
| dependent | 704.3 | 716.2 | 751.4 | 798.9 | 716.9 |
| pipelined | 1,911.2 | 1,916.1 | 1,861.3 | 1,986.4 | 1,524.5 |
| concurrent-32 | 8,073.2 | 7,596.5 | 6,652.1 | 6,665.1 | 6,322.9 |
| callback | 18,742.6 | 17,527.9 | 19,801.1 | 19,100.2 | 18,614.7 |
| large | 4,118.7 | 4,093.4 | 2,013.2 | 2,037.6 | 1,274.2 |
| deep-32-concurrent-32 | 40.1 | 39.5 | 37.8 | 26.3 | 25.1 |

**Legacy / webtransport**

| Case | Original Tokio | Current Tokio | Driven Tokio | Monoio epoll | Monoio io_uring |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary | 59,854.3 | 60,913.7 | 56,673.1 | 61,801.7 | 62,479.3 |
| dependent | 1,525.4 | 1,516.5 | 1,482.4 | 1,546.7 | 1,538.8 |
| pipelined | 4,206.0 | 4,022.4 | 3,804.1 | 3,916.2 | 3,286.9 |
| concurrent-32 | 9,780.4 | 9,603.8 | 7,918.4 | 7,993.8 | 7,721.6 |
| callback | 25,957.9 | 25,754.7 | 25,035.3 | 27,515.8 | 26,645.2 |
| large | 4,226.8 | 4,242.2 | 2,134.2 | 2,135.4 | 1,304.5 |
| deep-32-concurrent-32 | 59.4 | 59.5 | 42.5 | 59.0 | 104.6 |

Among current runtimes, the minimum-time winner across the 28 transport/driver/workload combinations is: Current Tokio: 7, Driven Tokio: 5, Monoio epoll: 12, Monoio io_uring: 4. Counts give each fixture equal weight and are not an aggregate application speedup.

The ignored `target/runtime-native-comparison` directory contains `samples.jsonl` (every native sample), `summary.json` and `summary.csv` (all 252 configurations with minimum/mean/maximum/spread/standard deviation), plus `metadata.json` (CPU, compiler, affinity, inputs and source/binary hashes). Wall time, process CPU time, allocations, requested bytes, read/write completions and throughput are retained separately. Hardware cycles are unavailable on this VM and remain null. Embassy hardware and browser WASM are outside this native timing matrix.

## RPC allocation optimization (2026-10-31)

Baseline: `ce5b40b18c57c450d267aec0a78dcd865cc3da18`. Same Rust 1.97.0 release/fat-LTO profile, service, certificates and CPU 2 affinity. Five separate processes per configuration, with baseline/candidate order alternated. **1,120 native samples** cover both RPC drivers, four runtimes, both transports and all seven cases. Setup/warmup/teardown are excluded. Native measurements use dependent/pipelined 128 iterations with 8 warmups, concurrent-32 256/16, deep 4/2, unary 1024/32, callback 128/8 and large 32/4.

Two RPC changes account for the savings: concrete native projected requests use direct completion instead of generic local queued-pipeline/channel/parameter/result adapters, and completed calls recycle only cleared request metadata. Generic enqueue shares the connection shutdown receiver. Cleared metadata retention is bounded by the existing concurrency limit and released on close; live replies/pipelines and application owners are never cached. Public APIs and the bounded no-std/no-alloc implementation are unchanged.

Allocation operations per complete workload, **minimum / mean / spread (max − min)** over five runs. Both peers are counted; each dependent/pipelined workload contains 32 capability hops. Monoio epoll over TLS:

| Case | Before | After | Minimum change |
| --- | ---: | ---: | ---: |
| dependent | 1,096.000 / 1,096.000 / 0.000 | 1,063.000 / 1,063.000 / 0.000 | -3.01% |
| pipelined | 2,106.148 / 2,106.155 / 0.008 | 1,234.148 / 1,234.153 / 0.008 | -41.40% |
| concurrent-32 | 202.000 / 202.000 / 0.000 | 170.000 / 170.000 / 0.000 | -15.84% |
| deep-32-concurrent-32 | 68,389.750 / 68,415.500 / 56.000 | 40,498.000 / 40,529.600 / 51.500 | -40.78% |

Requested allocation bytes, with the same **minimum / mean / spread** convention:

| Case | Before | After | Minimum change |
| --- | ---: | ---: | ---: |
| dependent | 117,424.000 / 117,424.000 / 0.000 | 105,808.000 / 105,808.000 / 0.000 | -9.89% |
| pipelined | 267,679.930 / 267,725.630 / 57.125 | 141,942.930 / 141,977.205 / 57.125 | -46.97% |
| concurrent-32 | 34,816.000 / 34,816.000 / 0.000 | 23,552.000 / 23,552.000 / 0.000 | -32.35% |
| deep-32-concurrent-32 | 10,611,714.250 / 10,619,496.600 / 26,567.750 | 6,633,948.250 / 6,637,428.850 / 5,698.250 | -37.48% |

Minimum allocation operations, **before → after**, across every affected runtime/transport profile:

| Transport | Runtime | Dependent | Pipelined | Concurrent-32 | Deep-32-concurrent-32 |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | tokio | 1,128.00 → 1,095.00 | 2,233.90 → 1,361.91 | 264.00 → 232.00 | 73,148.50 → 45,283.75 |
| tls | tokio-driven | 1,096.00 → 1,063.00 | 2,106.15 → 1,234.15 | 202.00 → 170.00 | 68,296.00 → 40,562.25 |
| tls | monoio-epoll | 1,096.00 → 1,063.00 | 2,106.15 → 1,234.15 | 202.00 → 170.00 | 68,389.75 → 40,498.00 |
| tls | monoio-uring | 1,096.00 → 1,063.00 | 2,107.20 → 1,235.20 | 202.00 → 170.00 | 68,613.25 → 40,705.75 |
| webtransport | tokio | 1,454.22 → 1,421.22 | 2,259.47 → 1,388.08 | 284.12 → 252.12 | 71,893.75 → 44,084.50 |
| webtransport | tokio-driven | 1,194.50 → 1,161.48 | 2,249.61 → 1,377.62 | 276.18 → 244.17 | 72,044.25 → 44,268.25 |
| webtransport | monoio-epoll | 1,326.50 → 1,293.50 | 2,253.69 → 1,381.80 | 280.26 → 248.20 | 72,196.00 → 44,553.00 |
| webtransport | monoio-uring | 1,712.34 → 1,679.38 | 2,305.87 → 1,433.96 | 311.32 → 279.36 | 73,983.50 → 46,083.75 |

Legacy reference in the candidate worker, using the same monoio epoll/TLS fixtures. Each metric is independently minimized:

| Case | Legacy blocks | Portable blocks | Legacy requested bytes | Portable requested bytes |
| --- | ---: | ---: | ---: | ---: |
| dependent | 1,189.000 | 1,063.000 | 93,840.000 | 105,808.000 |
| pipelined | 1,466.000 | 1,234.148 | 183,112.000 | 141,942.930 |
| concurrent-32 | 206.000 | 170.000 | 36,864.000 | 23,552.000 |
| deep-32-concurrent-32 | 52,208.750 | 40,498.000 | 9,099,728.000 | 6,633,948.250 |

Five separate Callgrind passes per build/case (40 samples), using the DHAT-disabled workers, monoio epoll/TLS, two warmups and four measured iterations (concurrent: 8; deep: 1). Minimum per-workload counters below come from the dumped measured part. Branch misses are simulated:

| Case | Ir before → after | Ir change | Bcm change | Bim change |
| --- | ---: | ---: | ---: | ---: |
| dependent | 3,517,630.500 → 3,526,459.500 | +0.25% | +7.39% | -3.95% |
| pipelined | 3,224,521.750 → 2,767,705.500 | -14.17% | -10.83% | -14.18% |
| concurrent-32 | 809,189.250 → 805,802.125 | -0.42% | +1.51% | +28.24% |
| deep-32-concurrent-32 | 310,117,413.000 → 297,694,905.000 | -4.01% | -9.83% | -15.72% |

This is an allocation improvement, not a uniform CPU or branch-miss improvement. Dependent instructions rise 0.25%; its Bcm and concurrent Bcm/Bim are higher. Miss changes occur in unchanged reader, scheduler and TLS routines as well as edited paths, so the trace does not isolate their cause. Hardware cycles are unavailable (`perf_event_open`: ENOENT); no cycle or latency speedup is claimed. TLS read/write counts and bytes are unchanged in dependent, pipelined and concurrent-32; WebTransport/deep batching and partial stream operations vary.

Raw native JSON samples, mean/range summaries and binary SHA-256 metadata: `target/rpc-allocation-optimization/native/`. Instruction profiles and five-run summaries: `target/rpc-allocation-optimization/instructions/`. DHAT profiles in `target/rpc-allocation-optimization/` separately verify allocation ownership and exact counter agreement. These generated artifacts stay out of source control.

Eight candidate DHAT traces (both drivers, four affected cases) match their allocation counters exactly, as do the baseline traces. Native pipeline adapters disappear from the candidate stacks. Remaining owners include immutable reader arenas, capability receipt/binding storage, admission credits/permits and TLS plaintext buffers. The concurrent fixture also allocates 32 FuturesOrdered task nodes; the workload is unchanged.

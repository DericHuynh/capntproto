# Legacy RPC / Tokio / rustls profiling

One worker exercises the supported stack, including both loopback peers. Eight
fixtures cover unary, a large reply, dependent and pipelined reads, concurrent-32,
fanout-128, callbacks and depth-32/concurrency-32. TLS authentication and warmup
finish before measured-region counters start; shutdown is excluded. Every reply
is checked. Concurrent fixtures count a complete batch, not a single leaf call.

```sh
# Install Valgrind and its headers first.
export CARGO_TARGET_DIR=target
CAPNTPROTO_VALGRIND_INCLUDE=/usr/include cargo build --locked --release --manifest-path benchmarks/runtimes/Cargo.toml
cargo build --locked --release -p capntproto-dev --bin runtime-runner
target/release/runtime-bench --init-certificates target/benchmark-certificates
target/release/runtime-runner --profile --worker target/release/runtime-bench \
  --certificates target/benchmark-certificates --output target/legacy-profiles
```

`--profile` launches five fresh worker processes per fixture. It records measured
instructions (Ir), conditional/indirect branch misses (Bcm/Bim), allocation
counts/bytes, reads/writes and wire bytes. Reports retain independent minima,
means, sample standard deviations and maxima, plus raw Callgrind traces, worker
identity, compiler, profiler and certificate hashes. Callgrind simulates branch
prediction; it does not report native CPU cycles. Setup, credentials and workload
budgets must match when comparing frozen binaries.

`runtime-runner --smoke` runs every fixture once for correctness and produces no
qualified performance result. The worker no longer accepts driver, transport or
runtime selectors. Historical multi-implementation results remain archival data.

## C++ ownership comparison and local profiles

The RPC implementation follows these paths in the pinned C++ reference:

- `capability.c++`, `LocalCallContext` / `LocalPipeline`: share one immutable
  response owner instead of cloning boxed results hooks and retaining a second
  mutable pipeline owner.
- `capability.c++`, `LocalClient`: its server keeps a direct hook. The Rust
  registry now keeps the first generated interface view inline, removing the
  inner TypeId hash table. Extra views retain canonicalization and revocation
  checks, including replacement while an older owner is still alive.
- `capability.c++`, `LocalCallContext::directTailCall`: adopt the tail response
  instead of copying its payload. The 64 KiB regression checks pointer identity
  and less than 1 KiB of allocation during completion. Adoption without extra
  lifetime context reuses the supplied response owner without allocation.
- `kj/async.c++`, `TaskSet`: track actual tasks. Empty Rust task sets no longer
  install a never-completing sentinel, and can accept work after becoming empty.

Before/after: the frozen legacy-only worker and the simplified implementation,
Rust 1.97.0, fat LTO, one codegen unit, release overflow checks, Valgrind 3.24.0.
The same certificates and fixtures were used. Each cell has five fresh processes,
16 measured iterations after eight warmups (two measured iterations for the deep
concurrent case). Counts include both peers and exclude setup and shutdown.
Concurrent cases report a complete batch, not one leaf call.

| Case | Allocations before → after | Bytes before → after | Ir mean ± sample SD | Ir change | Bcm change | Bim change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary | 7 → 7 | 744 → 744 | 42,946.93 ± 0.10 | -0.033% | +3.61% | +15.59% |
| large | 18 → 18 | 201,048 → 201,048 | 1,706,807.32 ± 150.19 | +0.000% | +4.84% | +5.35% |
| dependent | 1,189 → 1,093 | 93,840 → 87,056 | 1,870,400.24 ± 301.27 | -1.970% | -9.39% | +5.14% |
| pipelined | 1,466 → 1,370 | 183,112 → 176,328 | 1,363,521.88 ± 519.35 | -3.440% | -9.57% | +21.98% |
| concurrent-32 | 206 → 206 | 36,864 → 36,864 | 569,538.72 ± 0.67 | -0.003% | -2.36% | +65.50% |
| fanout-128 | 1,042 → 1,042 | 436,480 → 436,480 | 2,715,895.92 ± 24.35 | -0.022% | -2.18% | +78.90% |
| deep-32-concurrent-32 | 52,213 → 49,141 | 8,783,408 → 8,566,320 | 51,913,572.30 ± 37,481.82 | -3.760% | -1.47% | +28.83% |
| callback | 39 → 38 | 3,612 → 3,432 | 104,830.69 ± 0.27 | -0.860% | -0.45% | +10.08% |

Allocation and counter changes compare independent minima. Read/write counts and
wire-byte minima are unchanged in all eight fixtures. The ordinary benchmarks
do not exercise the local 64 KiB tail-response regression described above.

Incremental qualification, using the same five-run fixtures:

| Change | Dependent allocations | Pipelined allocations | Deep concurrent allocations |
| --- | ---: | ---: | ---: |
| Frozen legacy-only | 1,189 | 1,466 | 52,213 |
| Shared response/pipeline owner | 1,125 | 1,402 | 50,165 |
| Inline first interface registration | 1,093 | 1,370 | 49,141 |
| Empty task set and tail adoption | 1,093 | 1,370 | 49,141 |

These are allocation and guest-instruction improvements, not a universal speedup
claim. Simulated indirect branch misses increase in several fixtures, including
concurrent/fanout workloads whose executed instruction counts barely change.
The modified build also executes fewer indirect branches in dependent/pipelined
work. Actual cycle counters are unavailable in this workspace; native branch
prediction and CPU-cycle performance remain unqualified.

Worker SHA-256, frozen: `3083e863397418f6eccaaf1d3554c6d8d68cd6f326b668c7bf6f4a779e45650b`; modified: `63ab1bd0a585a679b1497395700d41662b3b52a32e6f884f61274c1fd862d7bd`.

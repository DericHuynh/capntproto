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

## Local instruction comparison

Frozen main `af2eeeb` versus the legacy-only build, Rust 1.97.0, fat LTO,
one codegen unit, release overflow checks, Valgrind 3.24.0. The same certificates,
fixtures and measured-region markers were used on both binaries. Five fresh
processes per cell; 16 measured iterations after eight warmups (two measured
iterations for the deep concurrent case). Counts include both peers. This is
instruction simulation and allocation evidence, not a network latency claim.

| Case | Ir minimum | Ir mean ± SD | Ir change | Bcm change | Bim change | Allocations | Allocated bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| unary | 42,961.00 | 42,961.19 ± 0.23 | -7.96% | -8.62% | -21.49% | 7 | 744 |
| large | 1,706,671.88 | 1,706,812.38 ± 79.38 | -0.25% | -10.40% | -12.79% | 18 | 201,048 |
| dependent | 1,907,816.12 | 1,907,853.11 ± 41.77 | -6.43% | -2.68% | -19.64% | 1,189 | 93,840 |
| pipelined | 1,411,390.12 | 1,412,167.24 ± 785.03 | -2.06% | +0.89% | -18.33% | 1,466 | 183,112 |
| concurrent-32 | 569,556.31 | 569,557.40 ± 0.84 | -3.12% | -3.80% | -33.60% | 206 | 36,864 |
| fanout-128 | 2,716,461.94 | 2,716,476.09 ± 30.91 | -2.49% | +1.55% | -26.90% | 1,042 | 436,480 |
| deep-32-concurrent-32 | 53,894,232.00 | 53,911,940.80 ± 10,171.94 | -1.59% | +5.56% | -19.42% | 52,213 | 8,783,408 |
| callback | 105,739.81 | 105,740.49 ± 0.62 | -7.45% | -8.51% | -23.29% | 39 | 3,612 |

Changes compare independent minima. Allocation minima are unchanged in all eight
fixtures. Conditional branch misses rose in pipelined, fanout and deep concurrent
work; indirect misses and instruction counts fell throughout. The separate TLS
first-write regression verifies zero allocations for a single effective iovec;
the workload table measures warmed connections. CI retains the full raw traces
and statistics in its `legacy-tokio-profiles` artifact.

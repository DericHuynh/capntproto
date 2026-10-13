# Upstream Rust RPC benchmark

`capnp-rust` is an independent plaintext TCP baseline built with published
crates.io packages: `capnp` **0.27.2**, `capnp-rpc` **0.27.0**, and the
`capnpc` **0.27.0** code generator. Its committed lockfile also selects upstream
`capnp-futures` 0.27.0. It has no dependency on our imported or modified crates.

The IO arrangement follows the upstream
[calculator example](https://github.com/capnproto/capnproto-rust/blob/master/capnp-rpc/examples/calculator/client.rs):
Tokio TCP with `TCP_NODELAY`, futures buffered readers/writers, upstream
`twoparty::VatNetwork` and `RpcSystem`. All Rust peers use a current-thread Tokio
runtime and local tasks. Build from the repository root to use the same release
configuration (full LTO, one codegen unit, overflow checks) as the other peers.

The [schema](../rpc/echo.capnp), [timing loop](../rpc/measurement.rs) and
[capability workloads](../rpc/capability-workflows.rs) are shared source, compiled
separately against each implementation's crates. Setup and warmup are excluded;
full response validation, response cleanup and per-call deadlines are included.
Workflows include capability handoff, promise pipelining, callbacks, 32-call
fan-out and payloads through 4 MiB. No compression or TLS is enabled.

```sh
# Requires the capnp schema compiler on PATH.
cargo build --locked --release --manifest-path benchmarks/rpc-upstream/Cargo.toml

# Produce the auditable CI bundle, including all comparison peers.
cargo run --locked -p capntproto-quality -- benchmark-build target/benchmark-build

# Validate all six peers and C++ interoperability without cloud credentials.
cargo run --locked -p capntproto-dev -- digitalocean-bench smoke \
  --bundle target/benchmark-build/bundle --output target/workflow-smoke \
  --state target/workflow-smoke-state
```

The binary supports `server`, `measure ADDRESS PUBLIC_JSON BYTES WARMUP ITERATIONS`,
`workflow-server ADDRESS`, and `workflow-measure ADDRESS CASE WARMUP ITERATIONS`.
`PUBLIC_JSON` must be `null` for this plaintext peer. The orchestration driver
supplies these arguments; measurements identify themselves as `capnp-rust`.

CI compiles and packages this executable alongside legacy/portable, C++, gRPC
and WebSocket peers. Loopback also includes our encrypted QUIC baseline. The
bundle records upstream crate versions and its lockfile, executable and dependency
inventory hashes. Its build rejects path/git substitutions or owned runtime
dependencies. Reports require the upstream cells and calculate latency ratios
as upstream Rust / C++ for the same payload or workflow (lower is faster).
Upstream results are descriptive and do not change our own TCP acceptance gate.

The [benchmark guide](../../docs/wiki/Quality-and-Benchmarks.md) describes
sampling budgets, the two-droplet topology and retained evidence. Local smoke
checks validate behavior only; they are not remote performance evidence.

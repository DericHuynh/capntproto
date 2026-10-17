# Portable RPC runtime comparison

Tokio is the active runtime for portable-driver optimization and the default
build of this harness. Compio is retained as an opt-in reproduction experiment;
it is not a production dependency or a planned runtime migration. The directory
name remains unchanged so existing reports and archived commands stay traceable.

This runtime comparison runs the production portable driver with Compio 0.19.2
or Tokio's current-thread executor. It shares the generated services, operations,
payload validation, response lifetimes, admission limits and ten-second deadlines
with `../rpc`. Only the host's timeout function is injected into that fixture.
With `--features compio-experiment`, the same executable contains the Tokio and
both Compio controls. This is a
benchmark host; the project's TLS, QUIC, Unix descriptor passing and application
services have not been migrated to Compio.

`CAPNTPROTO_BENCH_RUNTIME` selects:

| Value | Executor and socket path |
| --- | --- |
| `tokio` | Current-thread runtime, LocalSet and the production vectored Tokio adapter |
| `compio-poll` | Compio local executor and `PollFd`, preserving borrowed reads and vectored writes |
| `compio-completion` | Compio local executor and its buffered completion-to-futures adapters |

Compio's completion compatibility adapters own staging buffers and copy at the
borrowed `futures-io` boundary. They are an integration baseline, not a native
completion transport. The readiness path explicitly makes sockets nonblocking,
including when the underlying Compio driver is io_uring. Both paths sample the
live socket send-buffer size for the existing streaming window policy.

`CAPNTPROTO_COMPIO_DRIVER=auto|poll|io-uring` selects the underlying Compio driver.
Every Compio process reports the actual driver on stderr. An explicit io_uring
request fails if unavailable; it does not silently substitute polling. The local
October 17 environment rejects `io_uring_setup` with EPERM, so its measurements
use the Poll backend and cannot establish io_uring performance.

Build the Tokio host from the repository root, retaining its release settings:

```sh
cargo build --locked --release --manifest-path benchmarks/rpc-compio/Cargo.toml
benchmarks/rpc-compio/target/release/capntproto-rpc-runtime-bench runtime-info
```

Reproduce the historical Compio comparison explicitly:

```sh
cargo build --locked --release --features compio-experiment \
  --manifest-path benchmarks/rpc-compio/Cargo.toml
CAPNTPROTO_BENCH_RUNTIME=compio-poll CAPNTPROTO_COMPIO_DRIVER=poll \
  benchmarks/rpc-compio/target/release/capntproto-rpc-runtime-bench runtime-info
CAPNTPROTO_BENCH_RUNTIME=compio-poll CAPNTPROTO_COMPIO_DRIVER=poll \
  taskset -c 1 benchmarks/rpc-compio/target/release/capntproto-rpc-runtime-bench \
  workflow-server 127.0.0.1:49114
```

In a second terminal:

```sh
CAPNTPROTO_BENCH_RUNTIME=compio-poll CAPNTPROTO_COMPIO_DRIVER=poll \
  taskset -c 2 benchmarks/rpc-compio/target/release/capntproto-rpc-runtime-bench \
  workflow-measure 127.0.0.1:49114 pipelined-read 100 1000
```

Use matching selectors on both peers for timing, and mix selectors or C++ peers
for interoperability. Rotate five repetitions, preserve every sample and keep
instrumented timing separate. `CARGO_TARGET_DIR` can reuse an existing benchmark
build cache; adjust the executable path accordingly. Runtime tests cover canceled
pending reads, vectored byte ordering, partial progress and half-close behavior.

The [integration report](../../research/reports/portable-performance/2026-10-17-integration/README.md)
records the source and executable hashes, measured backend, full comparisons and
remaining architecture work. The dedicated sixteen-cell 5% target is separate
from these local TCP diagnostics.

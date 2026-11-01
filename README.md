# Cap’nt Proto

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)
[![Rust 1.97](https://img.shields.io/badge/rust-1.97-orange.svg)](rust-toolchain.toml)

Rust schemas, serialization, capability RPC and durable storage. RPC uses one
implementation: the legacy driver on **Tokio, with rustls TLS 1.3 over TCP**.
This is an experimental developer preview, independent of upstream Cap’n Proto.

## Start with a service

The examples have separate client/server programs and real Cap’n Proto schemas:

- [Greeting](examples/configurations/README.md): a session capability, a pipelined call, TLS and mutual TLS.
- [Key/value](examples/README.md): shared state and a deep concurrent directory client.
- [Distributed checkout](examples/checkout/README.md): order, inventory and payment services with durable recovery, cancellation and concurrent history checks.
- [Messages](examples/message_basic.rs): serialization without a network connection.

```sh
cargo run --manifest-path examples/configurations/Cargo.toml --bin example-credentials -- /tmp/example-pki
# Terminal 1
cargo run --manifest-path examples/configurations/Cargo.toml --bin rpc-server -- \
  --cert /tmp/example-pki/server.pem --key /tmp/example-pki/server-key.pem
# Terminal 2
cargo run --manifest-path examples/configurations/Cargo.toml --bin rpc-client -- \
  --ca /tmp/example-pki/ca.pem world
```

`capntproto` re-exports `capnp`, `capnp_rpc` and `capnp_futures`. Generate schemas
with `capntproto-codegen`; its native Rust compiler needs no external schema
compiler. Use `rpc::tcp::tls` to authenticate a socket before installing RPC.
`rpc::Connection` owns a running connection. The lower-level
`capnp_rpc::twoparty::TwoPartyClient` can be polled alongside an application in a
Tokio `LocalSet`. Configuration accepts an explicit `TlsClock` for certificate
validation. The low-level byte stream and Unix descriptor adapters use the same
RPC driver; they provide no additional protocol implementation.

## Allocation-conscious IO

The write path retains message envelopes, framing tables and TLS ciphertext
storage, preserves vectored writes, and batches related protocol output. rustls’s
Unbuffered API encrypts into caller-provided output storage. A single nonempty
write slice bypasses plaintext staging, including its first write. Fixed message
builders, borrowed readers and caller-owned framing scratch remain available for
allocation-free operations.

RPC connections and TLS handshakes allocate. rustls 0.23 also allocates received
plaintext records; the complete network stack is not allocation-free. See the
[transport API](crates/capntproto-transport/README.md) and
[benchmark fixtures](benchmarks/runtimes/README.md) for precise boundaries.

## Build and verify

Use Rust 1.97.0 and a platform C toolchain for rustls’s cryptography provider.
Default features are `tls`, `services` and `storage`. The smaller network build is:

```sh
cargo build --locked --no-default-features --features tls
cargo test --locked -p capntproto-rpc -p capntproto-futures -p capntproto-transport
cargo test --locked --test secure_rpc
cargo run --locked -p capntproto-dev -- examples
```

The full test suite additionally uses pinned C++ Cap’n Proto, CMake, a C++
compiler and Java for independent interoperability and model checks:

```sh
git submodule update --init --depth 1 -- vendor/capnproto vendor/verus-tla
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
```

CI runs tests, examples, fuzzing, formal checks and instruction profiles. LLVM
coverage runs separately with its own badge. Each performance fixture is repeated
five times, retaining raw counters, independent minima, means and sample standard
deviations. Instructions and simulated branch misses are measured separately
from noisy network latency.

Portable RPC, bounded RPC generation, Monoio, Embassy, embedded TLS, QUIC and
WebTransport backends have been removed. Earlier architecture and benchmark
reports in `docs/` describe historical versions and are not the current API.

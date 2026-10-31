# Runtime and protocol configurations

A real greeting service with separate client/server binaries and a shared
[Cap'n Proto schema](../webtransport/schema/greeting.capnp). The first request
creates a session; its echo call is pipelined before awaiting the session reply.

```sh
cargo run --manifest-path examples/configurations/Cargo.toml --bin example-credentials -- /tmp/example-pki
cargo run --manifest-path examples/configurations/Cargo.toml --bin rpc-server -- --help
cargo run --manifest-path examples/configurations/Cargo.toml --bin rpc-client -- --help
```

Use the generated server certificate/key and CA with the TLS defaults; CLI help
lists the exact paths/arguments. `--transport tcp` explicitly selects plaintext.
TLS is TCP with TLS authentication; WebTransport is HTTP/3 over QUIC with TLS.
The example's transport selector makes configuration combinations easy to test;
the library exposes these through separate modules.

The server uses Tokio. Clients select Tokio or monoio (Linux/macOS), and legacy
or bounded portable RPC independently. The monoio example explicitly uses its
readiness driver; it does not silently fall back from io_uring. Optional features
`monoio` and `webtransport` can be disabled independently. Browser connections
use the companion WASI Preview 3 frontend.

`tests/matrix.rs` checks all four server/client driver pairings for Tokio TCP,
TLS, mutual TLS and WebTransport, and monoio TLS, mutual TLS and WebTransport.
Additional tests reject missing client identities and prevent credential
regeneration from overwriting an existing directory. Credentials are for local
development, expire after seven days and are never committed.

```sh
cargo test --locked --manifest-path examples/configurations/Cargo.toml
cargo run --locked -p capntproto-dev -- examples
```

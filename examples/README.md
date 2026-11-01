# Runnable services

All network examples use legacy RPC, Tokio and rustls TLS 1.3. Clients authenticate
servers against an explicit CA. Each application separates client and server and
includes a shared Cap’n Proto schema.

- [Greeting and mutual TLS](configurations/README.md).
- [Distributed checkout](checkout/README.md): order, inventory and payment services,
  SQLite durability, cancellation and concurrent history checks.
- [Key/value service](kv/key_value.capnp): shared namespaces and a deep concurrent client.
- [Message round trip](message_basic.rs): serialization without a connection.

Generate development credentials, then run the key/value service in separate
terminals:

```sh
cargo run --manifest-path examples/configurations/Cargo.toml --bin example-credentials -- /tmp/example-pki
# Terminal 1
cargo run --manifest-path examples/kv/Cargo.toml --bin server -- \
  --cert /tmp/example-pki/server.pem --key /tmp/example-pki/server-key.pem
# Terminal 2
cargo run --manifest-path examples/kv/Cargo.toml --bin client -- --ca /tmp/example-pki/ca.pem put greeting hello
cargo run --manifest-path examples/kv/Cargo.toml --bin client -- --ca /tmp/example-pki/ca.pem get greeting
cargo run --manifest-path examples/kv/Cargo.toml --bin deep-client -- --ca /tmp/example-pki/ca.pem --depth 32 --concurrent 32
```

The server retains values across independent client connections until exit. The
deep client pipelines directory capabilities and verifies concurrent writes and
reads. `--help` lists application arguments. No runtime or transport selection is
needed. Credentials generated here are only for local development.

The common integration entry point runs the service, TLS/mTLS, key/value and
distributed checkout suites in both local development and CI:

```sh
cargo run --locked -p capntproto-dev -- examples
```

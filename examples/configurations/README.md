# Greeting service over Tokio and rustls

Separate client/server executables use a shared [schema](../service/schema/greeting.capnp)
and [service implementation](../service/src/lib.rs). A greeting creates a session
capability; the client pipelines its echo before awaiting the first reply.

```sh
cargo run --manifest-path examples/configurations/Cargo.toml --bin example-credentials -- /tmp/example-pki
# Terminal 1
cargo run --manifest-path examples/configurations/Cargo.toml --bin rpc-server -- \
  --cert /tmp/example-pki/server.pem --key /tmp/example-pki/server-key.pem
# Terminal 2
cargo run --manifest-path examples/configurations/Cargo.toml --bin rpc-client -- \
  --ca /tmp/example-pki/ca.pem world
```

For mutual TLS, add `--client-ca /tmp/example-pki/ca.pem` to the server and
`--cert /tmp/example-pki/client.pem --key /tmp/example-pki/client-key.pem` to
the client. The credentials are for local development and expire after seven
days. Regeneration cannot overwrite an existing directory.

`tests/matrix.rs` runs the actual greeting protocol over TCP with TLS and mutual
TLS and rejects clients without a required identity. All connections use the
legacy RPC implementation and Tokio; there are no runtime/driver selectors.

```sh
cargo test --locked --manifest-path examples/configurations/Cargo.toml
cargo run --locked -p capntproto-dev -- examples
```

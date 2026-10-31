# Embedded TLS telemetry service

The schema in `schema/telemetry.capnp` defines a sensor reading and its
acknowledgment. `publish` is a no-std, no-alloc client over embedded-tls: the
application supplies its IO, CSPRNG, TLS clock, CA trust and storage. It validates
the server and acknowledgment sequence, then closes TLS. This is a framed
Cap'n Proto message service, rather than hosted capability RPC.

With `embassy-net`, `publish_on_embassy` opens an Embassy TCP socket using borrowed
TCP buffers. The application must drive the Embassy network stack Runner and
apply its operation deadline. Supply a real Unix-time `TlsClock`; do not replace
certificate validation with an unchecked clock. Storage contains the fixed TLS
record, certificate and message buffers (approximately 40 KiB).

The independent host server and client make the device operation testable:

```sh
cargo run --manifest-path examples/configurations/Cargo.toml --bin example-credentials -- /tmp/example-pki
# Terminal 1: keep the telemetry service running.
cargo run --manifest-path examples/embedded-tls/Cargo.toml --features host --bin telemetry-server -- \
  127.0.0.1:4434 /tmp/example-pki/server.pem /tmp/example-pki/server-key.pem
# Terminal 2: run the device client against that service.
cargo run --manifest-path examples/embedded-tls/Cargo.toml --features host --bin telemetry-client -- \
  127.0.0.1:4434 /tmp/example-pki/ca.pem localhost
cargo test --manifest-path examples/embedded-tls/Cargo.toml --features host
```

The client prints `sequence=1 accepted=true`. Reuse an existing development PKI
directory instead of regenerating it; the credential tool refuses to overwrite it.

The shared integration runner also checks `thumbv7em-none-eabi`, with and without
Embassy networking. Tests use real TLS with fragmented IO, valid and invalid
certificate clocks/names/trust, rejected temperatures, acknowledgment validation,
and zero-allocation message encoding. They do not claim execution on hardware.

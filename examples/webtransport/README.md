# WebTransport greeting service

This service uses [greeting.capnp](schema/greeting.capnp) on both client and
server. `Greeting.open(prefix)` returns a per-client `Session` capability;
`Session.echo(value)` returns the prefixed value. The client pipelines the echo
before receiving the result of `open`. Both programs use the portable driver
with explicit storage bounds.

## Native client and server

Generate a short-lived P-256 development certificate (OpenSSL is needed only
for this setup):

```sh
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
  -days 14 -subj /CN=localhost \
  -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' \
  -addext 'basicConstraints=critical,CA:FALSE' \
  -addext 'extendedKeyUsage=serverAuth' \
  -keyout /tmp/greeting-key.pem -out /tmp/greeting-cert.pem
cargo run --locked --manifest-path examples/webtransport/Cargo.toml --bin server -- \
  127.0.0.1:4433 /tmp/greeting-cert.pem /tmp/greeting-key.pem http://localhost:8080
```

In another terminal:

```sh
cargo run --locked --manifest-path examples/webtransport/Cargo.toml --bin client -- \
  https://127.0.0.1:4433/rpc /tmp/greeting-cert.pem 'Hello, ' world
```

The result is `Hello, world`. The server authorizes `/rpc` and the configured
browser origin. Native clients do not send a browser origin. Production services
should add application authentication before granting sensitive capabilities.
TLS uses explicit trust roots and `TlsClock`; these executables opt into
`SystemClock`. No early data or certificate bypass is enabled.

## Browser WASM client

Install `wasm32-unknown-unknown` and a `wasm-bindgen-cli` version matching the
`wasm-bindgen` package in this example's lockfile. Then build and generate JS:

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --manifest-path examples/webtransport/Cargo.toml \
  --no-default-features --features browser --target wasm32-unknown-unknown --lib
wasm-bindgen --target web --out-dir examples/webtransport/pkg \
  examples/webtransport/target/wasm32-unknown-unknown/debug/capntproto_webtransport_example.wasm
openssl x509 -in /tmp/greeting-cert.pem -outform DER | openssl dgst -sha256
```

Serve this directory at `http://localhost:8080` using a static HTTP server, open
[browser.html](browser.html), and paste the SHA-256 hex digest. With a publicly
trusted certificate, leave the digest empty. Browser certificate pins are
subject to the browser's restrictions (including a maximum 14-day certificate
lifetime and supported key types). The development certificate above is P-256
and valid for 14 days. Use a browser that supports HTTP/3 WebTransport.

The browser owns QUIC, TLS, certificate checks and time. Rustls and embedded-tls
are not linked into this browser client. Its exported `call` function opens a
WebTransport session and stream, performs the same pipelined RPC, and shuts it
down. The native server uses rustls over QUIC; embedded-tls is a separate TCP
client backend documented in [capntproto-transport](../../crates/capntproto-transport/README.md).

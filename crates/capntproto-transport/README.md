# Capntproto transports

WebTransport streams for native Rust and browser WASM, clock-controlled rustls
streams, and an allocator-free embedded-tls client. The
[greeting service](../../examples/webtransport/README.md) has separate client and
server programs, a Cap'n Proto schema, and a browser client.

| Feature | Backend | IO and requirements |
| --- | --- | --- |
| `webtransport` | HTTP/3 over Quinn/rustls on native; browser WebTransport on WASM | `futures::AsyncRead + AsyncWrite`; native endpoints need Tokio |
| `rustls` | Existing rustls Unbuffered stream adapter | Tokio IO traits, TLS 1.3, explicit trust and clock |
| `embedded-tls` | embedded-tls TLS 1.3 client | `embedded-io-async`; no std or allocator; caller supplies IO, RNG, buffers, root and clock |
| `embedded-rpc` | Scoped adapter for the embedded client | Bounded allocating plaintext pipes to a hosted futures IO application |

`rpc::driver` and `rpc::network` adapt futures IO to the existing legacy or
portable RPC drivers. Neither backend is replaced. The hosted RPC driver uses
allocation; the separate bounded RPC API and isolated embedded TLS backend can
be built without std or an allocator.

## Verification time

Library-managed certificate verification requires `TlsClock`:

```rust
struct Clock;
impl capntproto_transport::TlsClock for Clock {
    fn now(&self) -> Option<u64> {
        // Read authenticated Unix seconds from the application's time source.
        Some(1_800_000_000)
    }
}
```

Returning `None` fails verification. Native rustls configurations take
`Arc<dyn TlsClock + Send + Sync>`; `SystemClock` is an explicit native opt-in.
Embedded configurations borrow `&dyn TlsClock`. No insecure verification or
implicit system trust store is selected. TCP rustls configuration helpers now
require this clock argument as well.

Browser WebTransport uses the browser's TLS implementation, trust policy and
clock. Browser clients can optionally supply SHA-256 certificate pins subject to
the browser's certificate lifetime and key restrictions. They cannot replace
browser verification with rustls or embedded-tls.

## Embedded TLS over TCP or custom IO

Use `default-features = false, features = ["embedded-tls"]` for the isolated
backend. `embedded::connect(io, config, rng, buffers)` authenticates a server
before returning a TLS connection implementing embedded IO. Supply separate read
and write record buffers (18 KiB each accommodates normal TLS records), plus
certificate scratch storage (2 KiB accommodates a typical P-256 leaf). Larger
certificates need more scratch space; insufficient storage returns an error.
Certificate scratch is borrowed only during setup; record buffers remain
borrowed for the connection lifetime.

This profile uses AES-128-GCM/SHA-256 and P-256 ECDSA certificates. Webpki checks
chain signatures, server usage, validity, and DNS/IP identity using a RustCrypto
P-256 verifier. Unsupported algorithms fail verification. Client certificates,
TLS servers, and QUIC are not implemented by this embedded backend. It offers
the RPC ALPN, but upstream embedded-tls does not expose the selected ALPN for an
independent post-handshake check.

The root crate's `rpc::tcp::embedded::connect` includes DNS, TCP, and TLS in one
setup deadline. For hosted RPC, `embedded::with_futures_io` consumes the TLS
connection and drives it concurrently with a borrowing application closure.
Each plaintext pipe has an explicit capacity. Successful completion drains
accepted writes and sends TLS close-notify. Cancellation destroys the connection
instead of allowing reuse after a partially processed record.

## WebTransport ownership

One reliable bidirectional WebTransport stream carries one RPC connection.
Streams retain their session and native endpoint; dropping the client, server,
or session handle leaves active streams usable. Dropping the last owner closes
the session. Closing a stream's writer leaves its reader and sibling streams
usable. The browser adapter retains pending JS reads/writes across polls and
canceled operations; writes apply backpressure rather than spawning a task for
each chunk.

Native `Server::incoming` separates admission from TLS/HTTP setup. `listen`
bounds concurrent setup and its deadline, including the first RPC stream.
Authorize path/origin in its callback before exposing bootstrap capabilities.
HTTP origin is an authorization input, not a client identity. Applications that
need client authentication must implement it in their service. Dropping a
listener cancels pending setup; established connections belong to the server
driver. Poll that driver concurrently.

## Build checks

```sh
cargo check --locked -p capntproto-transport --no-default-features \
  --features embedded-tls --target thumbv7em-none-eabi --lib
cargo check --locked -p capntproto --no-default-features \
  --features webtransport,tls,embedded-tls --target wasm32-unknown-unknown --lib
cargo build --locked --manifest-path examples/webtransport/Cargo.toml \
  --no-default-features --features browser --target wasm32-unknown-unknown --lib
```

The browser bindings use stable web-sys stream APIs; no `web_sys_unstable_apis`
flag is required. Compiling rustls for WASM additionally requires a Clang C
compiler for ring. Browser WebTransport alone does not compile a Rust TLS engine.

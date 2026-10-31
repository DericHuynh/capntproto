# Capntproto transports

WebTransport streams for native Rust and browser WASM, clock-controlled rustls
streams, and an allocator-free embedded-tls client. The
[greeting service](../../examples/webtransport/README.md) has separate client and
server programs, a Cap'n Proto schema, and a browser client.

| Feature | Backend | IO and requirements |
| --- | --- | --- |
| `webtransport` | HTTP/3 over Quinn/rustls on native; browser WebTransport on WASM | futures IO; native default facade uses Tokio |
| `webtransport-core` | Runtime-independent native QUIC/HTTP3 | Caller-owned UDP completion halves and timer |
| `monoio` | Completion-based TCP/TLS; QUIC with `webtransport-core` | io_uring or epoll on Linux, kqueue on macOS; no Tokio runtime |
| `embassy-net` | Embassy TCP with embedded TLS | No std/alloc; caller-owned network stack and storage |
| `rustls` | Shared Unbuffered record engine and owned IO driver | TLS 1.3, explicit trust and clock; direct stream additionally requires `tokio` |
| `embedded-tls` | embedded-tls TLS 1.3 client | `embedded-io-async`; no std or allocator; caller supplies IO, RNG, buffers, root and clock |
| `embedded-rpc` | Scoped adapter for the embedded client | Bounded allocating plaintext pipes to a hosted futures IO application |

`rpc::driver` and `rpc::network` adapt futures IO to the existing legacy or
portable RPC drivers. Neither backend is replaced. The hosted RPC driver uses
allocation; the separate bounded RPC API and isolated embedded TLS backend can
be built without std or an allocator.

## Runtime ownership

The common IO traits in `io` return buffers with each completion. They have no
`Send`, `'static`, allocator or executor requirement. Read and write halves are
separate so a blocked receive cannot block transmission. `Timer` supplies
monotonic deadlines; `TlsClock` supplies Unix time for certificate validation.

| API | Who polls transport IO? | Allocation / runtime |
| --- | --- | --- |
| `rustls::TlsStream` | Its Tokio read/write polls, as before | Native allocating rustls; direct readiness path retained |
| `rustls::RecordLayer` | Caller supplies ciphertext input/output and drives progress | Native allocating rustls; no socket, task or executor |
| `rustls::{client, server}` | Caller polls the returned driver alongside `Stream` | Owned completion buffers; no runtime or background task selected |
| `tokio::tls::{client_driven, server_driven}` | Caller owns the driver | Tokio socket adapter for the same completion engine |
| `monoio::tls::{client, server}` | Caller owns the driver | Monoio io_uring or epoll; no Tokio runtime |
| `webtransport::driven::{client, server}` | Caller owns UDP completion halves, timer and driver | Native QUIC/HTTP3 core; std/alloc, no Tokio dependency |
| `webtransport::{Client, Server}` | Existing Quinn/wtransport tasks on native | Established Tokio implementation retained; browser-owned on WASM |
| `tokio::webtransport::{Client, Server}` | One local endpoint task spawned by the new facade | Tokio `LocalSet`; same connect/accept/stream model |
| `monoio::webtransport::{Client, Server}` | One endpoint task spawned by the facade | Monoio; `connect_to` accepts externally resolved addresses |
| `embassy::connect` | Caller polls TCP/TLS futures and the Embassy network Runner | Borrowed socket/TLS storage; no std or allocator |

For example, after constructing a monoio TCP socket and explicit TLS config:

```rust,ignore
use capntproto_transport::{driven::DriverExt, monoio::tls};
let (mut stream, transport) = tls::client(socket, config, server_name)?;
let result = transport.run_until(async {
    stream.ready().await?; // Authenticate before exposing RPC capabilities.
    let rpc = capntproto_transport::rpc::driver(stream, None, side, options)?;
    // Drive rpc alongside the application, or use the existing RPC helpers.
    application(rpc).await
}).await??;
```

Factories do not spawn or perform IO. The returned future retains outstanding
completion operations across unrelated wakeups; there are no per-operation
boxed futures or plaintext relay pipes. Apply a runtime timeout around the
whole scope when a deadline is required. Successful scope completion drops the
application frame before requesting shutdown and draining local close output.
Complete RPC/stream shutdown inside the application before returning. Dropping
an unfinished scope cancels it; dropping a TLS stream is cancellation, not a
promise to deliver unfinished calls or buffered writes. Dropping the driver
wakes surviving handles with an error.

The explicitly driven native HTTP/3 core uses `quinn-proto` and `wtransport-proto`, without the
high-level Tokio-based wtransport endpoint. It supports one authorized session
per connection and bidirectional RPC streams. It bounds concurrent connections,
pending protocol streams and header/capsule bytes through `driven::Limits`.
Custom native UDP readers use `driven::ReceiveBuffer` (`bytes::BytesMut`);
writers still return `Vec<u8>`. The core splits received packets directly into
QUIC rather than copying their payloads. It reuses initialized 128 KiB receive
storage when packet owners release it, allocating another slab if a live packet
still retains the old one. This doubles the initial receive storage from 64 KiB.
Monoio preserves the initialized buffer length across short receives.

Epoll/kqueue UDP adapters first try a nonblocking syscall with stack-only
metadata, avoiding boxed sendmsg/recvmsg metadata for ready packets.
io_uring retains Monoio's send/receive completion boundaries: synchronous draining
delayed ACK/application processing and magnified Quinn's linear search through
tiny unacknowledged write fragments in deep concurrent workloads. A would-block
result uses Monoio's normal owned completion operation; descriptor blocking
flags remain unchanged. The endpoint
retains its outstanding QUIC alarm across packet and application wakeups,
replacing it when an earlier deadline is required. The driver guarantees send
progress and yields at its existing cooperative work limit, preserving receive
batching without letting a continuously ready receive queue starve transmission.

The readiness fast path removes operation metadata without batching UDP
syscalls. Compare kernel CPU separately
from userspace instruction counts when selecting a runtime for a deployment.
Runtime UDP adapters currently send individual datagrams; UDP segmentation and
receive offload are not implemented. Browser WebTransport remains browser-owned
and builds for `wasm32-unknown-unknown`.

Use the transport crate directly with `default-features = false` and features
`monoio` (TLS), or `monoio,webtransport-core` (QUIC), to avoid a Tokio dependency.
The root crate retains Tokio dependencies for its existing hosted APIs. Monoio
WebTransport's `connect(url)` uses the host's blocking resolver; use
`connect_to(peer, url)` for externally resolved addresses. The explicit core
always takes a resolved address.

## Embassy and bounded RPC

`embassy-net` supplies `embassy::connect`, borrowing TCP receive/transmit buffers,
TLS record/certificate buffers, RNG and `TlsClock`. The application still owns
and polls the Embassy network Runner. Socket setup deadlines and DNS belong to
the application. The embedded TLS crypto/client limitations below still apply;
this profile does not implement QUIC or a TLS server.

`embassy::{receive, send_pending}` connect embedded async IO to the existing
bounded RPC engine/wire APIs, reexported as `embassy::{engine, wire}`. Frames use
the shared Cap'n Proto framing validation. A `SendCursor` retains completed
write progress if a send future is canceled; `FrameReceiver` retains completed
read progress. Resume only if the underlying IO supports cancellation. Canceling
inside embedded-tls requires discarding the session. These adapters cooperate
after bounded work and perform no allocation. They do not replace the bounded
RPC state machine or make the hosted `TwoPartyClient` allocator-free. See the
[bounded RPC examples](../../verification/portable/src/driver.rs) for storage and
protocol driving.

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
  --features embassy-net --target thumbv7em-none-eabi --lib
cargo check --locked -p capntproto --no-default-features \
  --features webtransport,tls,embedded-tls --target wasm32-unknown-unknown --lib
cargo build --locked --manifest-path examples/webtransport/Cargo.toml \
  --no-default-features --features browser --target wasm32-unknown-unknown --lib
```

The browser bindings use stable web-sys stream APIs; no `web_sys_unstable_apis`
flag is required. Compiling rustls for WASM additionally requires a Clang C
compiler for ring. Browser WebTransport alone does not compile a Rust TLS engine.

Runtime comparisons against the frozen implementation live in
[benchmarks/runtimes](../../benchmarks/runtimes/README.md).

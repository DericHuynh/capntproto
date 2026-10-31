# TCP and QUIC

Select the network transport independently of certificate configuration:

| Transport | API | Security |
| --- | --- | --- |
| TCP | `capntproto::rpc::tcp` | Plain TCP, or TLS 1.3 through `rpc::tcp::tls` |
| QUIC | `capntproto::rpc::quic` | TLS 1.3 is part of QUIC; it cannot be disabled |
| WebTransport | `capntproto::rpc::webtransport` | HTTP/3 over QUIC; rustls on native, browser TLS on WASM |

`rpc::security` owns shared certificate material, PEM loading and the rustls
configuration types. TLS is a security layer for TCP, not a third transport.
The [key/value service](../../examples/README.md) demonstrates independent client
and server processes using the same schema on either transport.

TLS/TCP uses vendored rustls 0.23.46's Unbuffered API through the shared
`tls::TlsStream<S>` Tokio adapter, including native multiparty TCP/TLS. Both
maintained benchmark drivers use this same adapter. The independent published
upstream RPC and framework-managed gRPC comparators retain tokio-rustls.
The raw QUIC backend uses quiche with
Cargo-managed BoringSSL, including standard two-party RPC and the authenticated
[native multiparty profile](Native-Transports.md). The unmodified quiche 0.30.0 registry package and checksum are pinned in Cargo.lock.

Defaults enable `native`, services and storage, with TCP as the default route
backend. `rpc::tcp::connect` and `rpc::tcp::listen` select standard plaintext TCP;
authenticated native directory routes retain mutual TLS. `quic`
both enable the single QUIC engine and native sessions. For TLS/TCP alone use
`default-features = false, features = ["tls"]`. Plain TCP needs no feature.
QUIC builds require a C/C++ compiler, CMake and libclang.

## Record storage and receive APIs

`tls::TlsStream<S>` owns `UnbufferedClientConnection` or
`UnbufferedServerConnection`, retains ciphertext storage between records, and
passes complete plaintext records directly to the caller's read buffer. Partial
application reads keep their remaining plaintext in reusable storage. Receive
storage is 82 KiB; the normal transmit buffer is 129 KiB for a maximum 128 KiB
plaintext batch. Configured smaller TLS fragments can require a larger retained
transmit buffer. Receive offsets avoid shifting ciphertext after every record;
compaction happens only when socket refill needs room. This trades retained
per-connection memory for fewer receive-buffer allocations.

All production QUIC stream receives use `Connection::stream_recv_buf()`. The
ordinary RPC stream passes Tokio's `ReadBuf` directly as `BufMut`, so the unused
destination is not zeroed before reception. Native stream bridges already use
bounded `BytesMut` destinations. Quiche 0.30 still copies from stream reassembly
into `BufMut`; this is not ownership transfer of received packet storage.
The [local rustls delta](../../vendor/rustls/README.capntproto.md) writes new
Unbuffered application records directly into the retained transmit buffer with
the ring TLS 1.3 provider. It backports upstream PR #3127 and removes the temporary ciphertext allocation and
copy by using the same AEAD operation in caller-owned storage. Plaintext still
enters that storage once; received plaintext and queued handshake/control
records retain upstream allocation and copy behavior. Ring and AWS-LC now use the same caller-buffer encryption contract for TLS 1.2
and TLS 1.3. Custom crypto providers must adapt to its changed `encrypt` signature.
Neither transport provides end-to-end zero-copy IO.

`TlsStream::connect(io, config, name)` and `TlsStream::accept(io, config)` expose
the generic transport adapter for callers with their own deadline and ALPN
policy. Ordinary RPC applications should use the timed `tls::connect` and
`tls::accept` helpers below. Early data is unsupported. The adapter preserves
partial IO across `Pending`, processes post-handshake records and key updates,
and reports an underlying EOF without TLS `close_notify` as `UnexpectedEof`.

## Certificates and trust

`security::Identity` takes a leaf-first chain of `CertificateDer<'static>` values
and its matching `PrivateKeyDer<'static>`. These are the standard rustls types;
`security::rustls` re-exports rustls for configuration. Use `security::Identity::from_pem_files(cert, key)` for PEM identities,
`security::roots_from_pem_file(ca)` for TCP trust, and
`security::certificates_from_pem_file(ca)` for QUIC trust. DER construction is
also available.

Build configurations once and share their `Arc`s:

```rust
use capntproto::rpc::{security, tcp::tls};

// RootCertStore values contain the application's trusted CA certificates.
let clock = std::sync::Arc::new(capntproto::rpc::SystemClock);
let client = tls::client_config(server_roots, Some(client_identity), clock.clone())?;
let server = tls::server_config(server_identity, Some(client_roots), clock)?;
```

TLS configurations require a caller-supplied `TlsClock`, which returns Unix
seconds or `None` when time is unavailable. Missing time fails certificate
verification. `SystemClock` explicitly opts into the native host clock; embedded
applications can supply their own authenticated time source.

Passing `Some(client_roots)` makes client authentication mandatory, including
certificate-chain, validity-period and client-auth key-usage checks. An absent
or untrusted client certificate fails the server handshake. Pass `None` on both
sides for server-authenticated TLS without client certificates. An empty root
store is not a request to disable verification. Clients always validate the
server chain and the requested DNS name or IP address.

Helpers use an explicit ring provider, TLS 1.3 only, and the private application
ALPN `capntproto-rpc/1`. Rustls and raw QUIC adapters reject missing or different ALPN values. This
ALPN carries ordinary two-party Cap'n Proto framing; it is not HTTP/3 or an
upstream-assigned Cap'n Proto protocol identifier. Helpers disable early data;
QUIC setup awaits the full handshake before exposing RPC streams.

Certificate trust is distinct from authorization. To grant different bootstrap
capabilities to different peers, call `tls::accept` or `quic::accept`, inspect
the authenticated chain, apply your authorization policy, and only then build
the RPC driver. TLS exposes it through `stream.get_ref().1.peer_certificates()`;
QUIC exposes `stream.peer_certificates()`. A server using server-only TLS has no
authenticated client certificate. `listen` shares one bootstrap with every
peer accepted by its certificate policy.

## TCP and TLS connections

`tcp::connect(address, timeout)` dials a plaintext peer and returns a `TcpStream`.
All connectors return streams; `tcp::driver`, `tcp::tls::driver` and
`quic::driver` adapt those streams to RPC drivers with matching arguments. It and the TCP listener disable Nagle's
algorithm for RPC latency. The network adapter retains live socket-informed
flow control. The TCP setup deadline includes DNS and connection establishment.

For TLS, connect first, then construct the RPC driver:

```rust
use capntproto::capnp_rpc::rpc_twoparty_capnp::Side;
use capntproto::rpc::tcp::tls;
use std::time::Duration;

let stream = tls::connect(
    "127.0.0.1:7443",
    "rpc.example.com".try_into()?,
    client_config,
    Duration::from_secs(10),
).await?;
let connection = capntproto::rpc::Connection::spawn(
    tls::driver(stream, None, Side::Client, Default::default()),
);
let remote: my_schema::service::Client = connection.bootstrap();
```

Run RPC drivers inside a Tokio `LocalSet`. TLS's setup deadline includes DNS,
TCP connect and handshake. `tls::accept(socket, server_config, timeout)` applies
a handshake deadline to an accepted TCP socket. The TLS network captures its
TCP send-buffer size at admission for streaming flow control.

`tls::listen(&server, &listener, config, reader_options, accept_options,
on_rejected)` performs bounded concurrent handshakes for a `TwoPartyServer`.
Poll its `ServerDriver` concurrently. `AcceptOptions` defaults to 64 pending
connections with a ten-second setup deadline. Invalid peers are reported to
the callback and do not stop the listener; TCP accept failures are returned.
Canceling the listener drops pending handshakes but leaves established sessions
with their ServerDriver. Dropping that driver cancels its owned sessions.

## Standard QUIC connections

Create endpoints inside a Tokio `LocalSet`. QUIC configuration takes DER root
certificates and `security::Identity` values directly. TCP's rustls configuration
objects are specific to TCP and are not passed to the QUIC engine.

```rust
use capntproto::rpc::quic::{self, Endpoint, Version};
use std::time::Duration;

let server = Endpoint::server(
    quic::server_config(server_identity, Some(client_ca_certificates))?,
    "127.0.0.1:7443".parse()?,
)?;
let mut client = Endpoint::client("0.0.0.0:0".parse()?)?;
client.set_default_client_config(quic::client_config_for_version(
    server_ca_certificates, Some(client_identity), Version::V1,
)?);
let stream = quic::connect(
    &client, server_address, "rpc.example.com", Duration::from_secs(10),
).await?;
// quic::driver(stream, bootstrap, side, reader_options) returns the RPC driver.
```

`client_config(roots, identity)` defaults to v1, the version supported by upstream
Quiche 0.30.0. Explicit `Version::V2` requests return `Unsupported`; there is no
silent downgrade. Servers accept v1 and `stream.version()` reports that version.
Clients verify the certificate chain and DNS name or IP subject alternative name;
IP literals are checked before exposing the authenticated stream and are not
sent as SNI. Roots are explicit; system trust is not loaded implicitly.
Supplying client roots makes authentication mandatory.
`stream.peer_certificates()` exposes the verified peer chain.

Sessions use full TLS handshakes with no resumption or
application 0-RTT. `ServerConfig::require_retry(true)` enables stateless Retry
before allocating handshake state; tokens expire after ten seconds and bind
the source address, wire version, original destination CID and Retry CID.

The first client-initiated bidirectional stream carries ordinary Cap'n Proto
framing. Its first write makes it visible to the server; start the client RPC
driver before waiting for server stream acceptance. Unidirectional streams and
datagrams are disabled for this two-party profile. Use native sessions for
multiparty discovery, introductions, NAT rendezvous, authorized datagrams,
explicit validated path migration and CID rotation.

`quic::accept(incoming, timeout)` bounds both TLS and arrival of the first RPC
stream. `quic::listen` performs concurrent bounded setup with the same
`capntproto::rpc::AcceptOptions` as TCP/TLS. Canceling the listener stops new admission and
cancels pending setup while established streams retain their packet router.
The endpoint also bounds its incoming queue, route count and per-route packets.

Application reads and writes access Quiche's stream buffers directly on the
`LocalSet`. There is no additional duplex byte queue or task relaying application
data. Quiche retains flow-control and congestion backpressure. The packet task
owns UDP transmission, recovery, pacing and bounded bursts; it wakes only parked
operations that can make progress. Connection borrows never cross a socket or
pacing await. Authenticated receive bytes remain drainable after the packet task
terminates.

Write shutdown queues all buffered bytes and FIN into Quiche; the receive
direction stays open. It is local write completion, not a peer receipt or method
completion. Applications requiring delivery confirmation must use a response or
the Native session shutdown protocol. Dropping a stream with an open writer cancels its driver immediately. After
successful write shutdown, drop cancels reading while Quiche retains pending
transmission until stream collection or connection close/idle timeout. `Endpoint::wait_idle()` waits for route
release.

## WebTransport and embedded TLS

Feature `webtransport` exposes HTTP/3 sessions and futures IO streams on native
and `wasm32-unknown-unknown` targets. Native endpoints use Quinn/rustls with
explicit roots and `TlsClock`; browser endpoints use browser-managed TLS, trust
and time. A reliable bidirectional stream carries an ordinary RPC connection.
The [WebTransport greeting service](../../examples/webtransport/README.md) has
separate native server/client programs, a schema, and a browser WASM client.
Authorize session paths and browser origins before granting capabilities.

Feature `embedded-tls` adds `rpc::tcp::embedded`: a TLS 1.3 TCP client with caller
entropy, trust, clock and record/certificate buffers. Its isolated
[transport backend](../../crates/capntproto-transport/README.md) builds without
std or allocation and accepts any embedded async IO. Its current crypto profile
uses AES-128-GCM/SHA-256 and P-256 ECDSA certificates. Hosted RPC uses a scoped,
bounded plaintext bridge; that bridge allocates. This is a TCP implementation;
embedded-tls is not a QUIC TLS provider. Browser WebTransport does not depend on
either Rust TLS implementation.

## Verification

```sh
cargo nextest run --locked --test tcp_rpc --test secure_rpc
cargo nextest run --locked --no-default-features --features tls --test secure_rpc
cargo nextest run --locked --no-default-features --features quic --test secure_rpc
cargo test --locked --no-default-features --features tls,webtransport,embedded-tls --test secure_rpc
cargo nextest run --locked --lib transport::backend_tests
cargo nextest run --locked --lib rpc::quic::retry
cargo nextest run --locked --lib transport:: -- --skip tlc
```

Loopback tests exercise TLS/mTLS, callbacks, pipelined capabilities, large
messages, shutdown, listener isolation and cancellation. Negative cases cover
untrusted issuers, wrong hostnames, expired certificates, wrong key usage,
missing client certificates and ALPN mismatch. Retry tokens are checked against
mutation, expiry and reuse with another address, CID or version. Unsupported v2 configuration is also tested.

Independent interoperability uses s2n-quic 1.88.0 in both client/server roles for
v1, with and without Retry. Each case exchanges 256 KiB in each direction;
the independent peer rotates keys that quiche must process. The test uses the
upstream Rustls/AWS-LC cipher implementation with a lower packet-use limit to
trigger rotation during a short transfer; cipher operations are unchanged.
The dev crate enables s2n-quic only with `--features quic-interop`; it is not a
production transport dependency. Packet events verify v1, actual Retry behavior,
no early data, and at least one 1-RTT key update.

```sh
cargo build --locked --example quic-interop --no-default-features --features quic
cargo run --locked -p capntproto-dev --features quic-interop -- check-quic-interop
```

These tests do not establish HTTP/3 support or production qualification.

References: [quiche configuration](https://docs.rs/quiche/latest/quiche/struct.Config.html),
[RFC 9369](https://www.rfc-editor.org/rfc/rfc9369.html),
[RFC 9368](https://www.rfc-editor.org/rfc/rfc9368.html).

# TCP, TLS and standard QUIC

The public two-party RPC adapters support three ordinary network transports:

| API | Wire protocol | Authentication |
| --- | --- | --- |
| `capntproto::rpc::tcp` | Cap'n Proto RPC on TCP | Plaintext; no peer authentication |
| `capntproto::rpc::tls` | Cap'n Proto RPC on TLS 1.3/TCP | Server certificates, optional required client certificates |
| `capntproto::rpc::quic` | Cap'n Proto RPC on standard QUIC v1 | TLS 1.3 server certificates, optional required client certificates |

TLS/TCP uses Cargo-managed rustls and tokio-rustls. All QUIC uses quiche with
Cargo-managed BoringSSL, including standard two-party RPC and the authenticated
[native multiparty profile](Native-Transports.md). The unmodified quiche 0.30.0 registry package and checksum are pinned in Cargo.lock.

Defaults enable `quic-quiche`, services and storage. `quic` and `quic-quiche`
both enable the single QUIC engine and native sessions. For TLS/TCP alone use
`default-features = false, features = ["tls"]`. Plain TCP needs no feature.
QUIC builds require a C/C++ compiler, CMake and libclang.

## Certificates and trust

`tls::Identity` takes a leaf-first chain of `CertificateDer<'static>` values
and its matching `PrivateKeyDer<'static>`. These are the standard rustls types;
`tls::rustls` re-exports rustls for configuration. Keys and certificates can be
loaded from DER, or from PEM using `rustls::pki_types::pem::PemObject`.

Build configurations once and share their `Arc`s:

```rust
use capntproto::rpc::tls;

// RootCertStore values contain the application's trusted CA certificates.
let client = tls::client_config(server_roots, Some(client_identity))?;
let server = tls::server_config(server_identity, Some(client_roots))?;
```

Passing `Some(client_roots)` makes client authentication mandatory, including
certificate-chain, validity-period and client-auth key-usage checks. An absent
or untrusted client certificate fails the server handshake. Pass `None` on both
sides for server-authenticated TLS without client certificates. An empty root
store is not a request to disable verification. Clients always validate the
server chain and the requested DNS name or IP address.

Helpers use an explicit ring provider, TLS 1.3 only, and the private application
ALPN `capntproto-rpc/1`. Adapters reject missing or different ALPN values. This
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

`tcp::connect(address, bootstrap, reader_options)` dials a plaintext peer and
returns a `TwoPartyClient` driver. It and the TCP listener disable Nagle's
algorithm for RPC latency. The network adapter retains live socket-informed
flow control. Wrap a plaintext connect future in `tokio::time::timeout` when
the caller needs a DNS/connect deadline.

For TLS, connect first, then construct the RPC driver:

```rust
use capnp_rpc::rpc_twoparty_capnp::Side;
use capntproto::rpc::tls;
use std::time::Duration;

let stream = tls::connect(
    "127.0.0.1:7443",
    "rpc.example.com".try_into()?,
    client_config,
    Duration::from_secs(10),
).await?;
let mut rpc = tls::client(stream, None, Side::Client, Default::default());
let remote: my_schema::service::Client = rpc.bootstrap();
let driver = tokio::task::spawn_local(rpc);
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
certificates and `tls::Identity` values directly. TCP's rustls configuration
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
// quic::client(stream, bootstrap, side, reader_options) returns the RPC driver.
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
`tls::AcceptOptions` as TCP/TLS. Canceling the listener stops new admission and
cancels pending setup while established streams retain their packet router.
The endpoint also bounds its incoming queue, route count and per-route packets.

Write shutdown queues all buffered bytes and FIN into Quiche; the receive
direction stays open. It is local write completion, not a peer receipt or method
completion. Applications requiring delivery confirmation must use a response or
the Native session shutdown protocol. Dropping a stream with an open writer cancels its driver immediately. After
successful write shutdown, drop cancels reading while Quiche retains pending
transmission until stream collection or connection close/idle timeout. `Endpoint::wait_idle()` waits for route
release.

## Verification

```sh
cargo test --locked --test tcp_rpc --test secure_rpc
cargo test --locked --no-default-features --features tls --test secure_rpc
cargo test --locked --no-default-features --features quic --test secure_rpc
cargo test --locked --lib transport::backend_tests
cargo test --locked --lib rpc::quic::retry
cargo test --locked --lib transport:: -- --skip tlc
```

Loopback tests exercise TLS/mTLS, callbacks, pipelined capabilities, large
messages, shutdown, listener isolation and cancellation. Negative cases cover
untrusted issuers, wrong hostnames, expired certificates, wrong key usage,
missing client certificates and ALPN mismatch. Retry tokens are checked against
mutation, expiry and reuse with another address, CID or version. Unsupported v2 configuration is also tested.

Independent interoperability uses aioquic 1.3.0 in both client/server roles for
v1, with and without Retry. Each case exchanges 256 KiB in each direction;
the independent peer initiates key updates that quiche must process.

```sh
python3 -m venv target/quic-interop-venv
target/quic-interop-venv/bin/pip install -r scripts/requirements-quic-interop.txt
cargo build --locked --example quic-interop --no-default-features --features quic
target/quic-interop-venv/bin/python scripts/check_quic_interop.py
```

These tests do not establish HTTP/3 support or production qualification.

References: [quiche configuration](https://docs.rs/quiche/latest/quiche/struct.Config.html),
[RFC 9369](https://www.rfc-editor.org/rfc/rfc9369.html),
[RFC 9368](https://www.rfc-editor.org/rfc/rfc9368.html).

# Tokio / rustls transport

TLS 1.3 streams over Tokio IO, using rustls’s Unbuffered API. There are no backend
or runtime feature selectors. `rustls::client_config` and `server_config` accept
explicit trust roots, identity and a caller-supplied `TlsClock`. `SystemClock`
is available for ordinary hosts. Client certificate verification is mandatory
when server configuration receives client roots. RPC uses the `capnp-rpc` ALPN;
early data is disabled.

`rustls::TlsStream::connect` and `accept` authenticate before returning. The stream
implements Tokio `AsyncRead` and `AsyncWrite`, including vectored writes.
`tokio::FuturesIo` preserves scatter/gather writes when adapting it to futures IO.
The main crate’s `rpc::tcp::tls` module adds setup deadlines, ALPN checks,
TCP_NODELAY, bounded concurrent handshakes and the legacy RPC adapter.

Ciphertext buffers are retained between operations. Encryption writes directly
into the output buffer. A write with exactly one nonempty iovec reads that slice
directly, without allocating or copying into plaintext staging. Multiple slices
retain batching through reusable staging storage. Backpressure preserves pending
ciphertext; accepted plaintext is not encrypted twice. Closing flushes accepted
output before sending close_notify. Dropping a stream cancels outstanding work.

This is an allocating hosted stack: handshakes and received rustls plaintext
records allocate. Tests cover first-write allocation counts, all supported TLS
cipher suites, key updates, authentication, custom clocks, truncation, corrupted
records, backpressure and shutdown.

```sh
cargo test -p capntproto-transport
cargo test --test secure_rpc
```

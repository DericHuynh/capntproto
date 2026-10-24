# Local rustls 0.23.46 delta

This directory contains the checksum-verified crates.io package, with all upstream
licenses retained. `UPSTREAM.json` records the original package checksum, upstream
commit and hashes of every original file; `LOCAL.patch` contains the five-file delta. The upstream formatting configuration
is copied from that same commit. The original README is unchanged.

The local change removes an owned ciphertext allocation and its copy from the
Unbuffered application-data write path:

- `MessageEncrypter::encrypt_into` writes an encoded record into caller storage.
  Its default implementation uses the original owned-record path, preserving other
  providers and TLS 1.2. The existing buffered encrypter remains unchanged.
- The ring TLS 1.3 override copies plaintext into the caller's record payload,
  authenticates the original five-byte TLS 1.3 header, encrypts in place using
  ring's existing AEAD, and stores its separate tag directly into the record.
- The record layer shares sequence advancement and exhaustion checks between both
  output paths. `CommonState` keeps its complete size preflight, fragmentation,
  queued-control ordering and key-refresh logic, then writes new application
  records directly into the caller buffer.
- `OutboundChunks::copy_to_slice` handles selected ranges of scatter/gather input
  without allocating. The direct override is ring-only; the default is available
  to every provider.

Short output buffers leave output unchanged. The existing Unbuffered API's size
preflight handles retries before encrypting application fragments; any queued key-update control record retains its original ordering and state.
Provider encryption failures retain the record layer's existing terminal/panic
behavior; a consumed sequence number is never reused.

This removes the application-record ciphertext copy, not every TLS copy. Plaintext
still enters the output buffer once. Buffered handshake/key-update/alert records
still use their existing queue and copy, and decrypted records still allocate.
The root crate uses a direct path dependency; root and maintained RPC benchmark
patches unify transitive rustls users. The independent upstream benchmark keeps
registry rustls. No algorithm, authentication rule or protocol version changed.

Validation lives in `tests/secure_rpc/rustls_direct.rs`: byte-for-byte comparison
with the unchanged owned encrypter and zero-allocation assertions for AES-128-GCM,
AES-256-GCM and ChaCha20-Poly1305; empty/max-size/fragmented/ranged payloads; multiple
content types and high sequence bits; short-buffer retries and untouched tails;
default-provider fallback; and real buffered/unbuffered interoperability with
forced automatic key updates in both directions for all three ciphers.
Existing secure RPC tests cover fragmentation, tiny fragments, cancellation,
backpressure, key updates, corruption, truncation, shutdown, identities and ALPN.

The published crate excludes upstream test-data fixtures; this is a package
vendor, not the complete upstream test workspace. The repository's integration
and differential tests are the validation entry points for this local delta.

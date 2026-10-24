# Unbuffered TLS and QUIC receive APIs — 24 October 2026

Implementation: [bd60556f1d48fd38355fa0c17f4f304cec444bf8](https://github.com/DericHuynh/capntproto/commit/bd60556f1d48fd38355fa0c17f4f304cec444bf8).
Dedicated qualification: [40046022437](https://github.com/DericHuynh/capntproto/actions/runs/40046022437), workflow source `ceb883122ce17bfa7a0fd866ff49ba176b373296`.
Frozen legacy control: `867fb7863db47e559de521f90b743769fd390a68`, immediately before this implementation.

**Performance is not yet qualified.** The campaign first waits for the preceding
QUIC routing qualification (40031849761) to acquire the dedicated measurement
slot. This prevents replacing GitHub's single pending measurement job. It then
runs the existing full secure campaign on CI droplets: five native repetitions
with mean/spread, paired DHAT allocation profiles, Callgrind and syscall traces.
Both maintained Rust drivers use the new shared transports; C++ remains the
supported TLS reference. C++ QUIC is still unsupported. No local timing or
allocation benchmarks were run.

## Subsequent local measurements

The user requested local benchmarks after this implementation. The
[local comparison](../2026-10-24-local-unbuffered/README.md) records 405 main timing
trials, a larger 25-trial bulk repeat and 200 syscall diagnostic trials. It finds
lower TLS socket counts, including concurrent-32 read parity, but regressions in
TLS bulk latency. Dedicated-host qualification and allocation parity remain open.

## Changes

All production quiche stream receive calls now use `Connection::stream_recv_buf()`.
Ordinary RPC passes Tokio's `ReadBuf` directly, preserving its filled prefix and
initializing/advancing only bytes actually received. This removes the previous
`initialize_unfilled()` pass. Native stream bridges already used bounded
`BytesMut` destinations; bulk/control receives now consistently name the BufMut
API. Quiche 0.30.0's scalar `stream_recv()` already forwarded to `stream_recv_buf()`;
simply renaming the remaining fixed-slice calls does not remove a copy.

Ordinary RPC TLS, native TCP/TLS and maintained legacy/portable fixtures now use
one Tokio adapter owning rustls `UnbufferedClientConnection` or
`UnbufferedServerConnection`. It retains ciphertext storage, advances input
offsets instead of shifting every record, and encrypts bounded 128 KiB plaintext
batches into retained output storage. Normal input/output storage is 82/129 KiB
per connection; small configured TLS fragments can enlarge output storage.
Partial application reads and records encountered while making write progress
use reusable plaintext spill storage. Writers do not read more application
ciphertext from the socket to make room. Certificate/name verification, mandatory
mTLS, ALPN policy, deadlines, post-handshake key updates, backpressure and
close_notify remain active.

`tls::TlsStream<S>` is now a project-owned type, rather than a tokio-rustls enum
re-export. Its generic constructors accept the supplied verification policy;
ordinary RPC uses the existing timed, ALPN-validating helpers. `get_ref()` exposes
the IO and immutable authenticated `rustls::CommonState`. No buffered fallback or
compatibility wrapper is used in the first-party transport. tokio-rustls remains
only as a root dev dependency for interoperability tests and in independent
upstream/framework-managed benchmark comparators. Benchmark certificate, ALPN,
resumption and early-data policy is shared with those comparators.

## Remaining upstream allocation/copy limits

The [preceding measured dispatch report](../2026-10-24-dispatch-qualification/README.md)
attributed 70.4 versus 33.6 TLS bulk-1m allocations, and 98.0444 versus 38.7556
TLS bulk-4m allocations, to rustls deframer growth in legacy versus portable.
The new adapter bypasses that buffered deframer storage; the pending campaign
must establish the resulting allocation and latency changes.

This is not an end-to-end zero-copy implementation. In the pinned sources:

- quiche 0.30.0 copies reassembled stream bytes into the caller's `BufMut`.
- rustls 0.23.46 `ReadTraffic::next_record()` owns an allocated plaintext chunk;
  its incoming slice is reserved for future in-place decryption support.
- rustls 0.23.46 `CommonState::write_fragments()` allocates/encodes each encrypted
  record and copies it into the supplied output slice.

Changing API names does not remove those upstream record allocations. The
existing four bulk allocation gaps and read-count gaps remain open until the
new CI evidence is reviewed.

## Correctness validation

- 27 secure RPC tests pass, including both drivers, certificate rejection,
  mTLS, pipeline/large-message behavior and QUIC shutdown/backpressure.
- Five new Unbuffered tests cover independent concurrent readers/writers over
  257-byte transport buffers, buffered IO that requires flush, partial records,
  32-byte TLS fragments, half-close, buffered-peer interoperability in both
  directions, repeated key updates through a 37-byte transport, truncated EOF
  and invalid ciphertext. Post-handshake tickets are drained concurrently with
  the other side's handshake rather than blocked behind a joint handshake barrier.
- The QUIC half-close test now uses uninitialized destination storage with an
  existing prefix and verifies that only returned bytes become initialized.
- 35 selected identity, native transport, multiparty and shutdown integration
  tests pass. Native TLS/backend tests in the root unit suite also pass.
- TLS-only compilation, root all-target Clippy with warnings denied, maintained
  benchmark all-target compilation and independent upstream fixture compilation
  pass. The upstream generated schema emits its existing unused-code warnings.

The broad root unit run had 296 passed, four ignored and one storage property
failure: `storage::components::tests::mixed_edits_publications_retention_and_recovery_match_materialized_values`
received `EWOULDBLOCK` reopening a store at `src/storage/components/tests.rs:598`.
Its saved seed `c4f246129d9a031fbf9fb0d991dd490c21a55d5c3684d1efc69263ffbdfcc0ed`
passes in isolation both with the original temporary directory and with a
separate build-cache temporary directory. The parallel-suite failure is recorded,
not represented as a clean full-suite pass. No storage implementation was changed.

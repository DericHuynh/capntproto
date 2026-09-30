# Experimental Capn't Proto implementation

This workspace now contains a Rust implementation of a Noise-backed transport,
capability object storage, and an explicit three-party introduction protocol.
It is an experimental protocol profile, **not a complete port of the Cap’n Proto
C++ implementation or a wire-compatible replacement for standard QUIC**.

See [ARCHITECTURE.md](ARCHITECTURE.md) for feature boundaries, lifecycle ownership
and targeted verification. Test schema bindings are in the `reproto-test-support`
dev package. [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md) separates release
criteria from the [feature/research backlog](ROADMAP.md).

## Build and run

Requirements: Rust/Cargo, the C++ `capnp` schema compiler, and Java 17
with TLC for model checking; [TESTING.md](TESTING.md) lists the complete test
toolchain. Cargo.lock pins Rust dependencies. The supplied
quiche checkout is modified locally; preserve it or apply the archived patch to
the upstream revision in `vendor/provenance/quiche-revision.json`.

```sh
cargo test --tests
cargo clippy --all-targets --no-deps -- -D warnings
cargo run --example noise_store -- /tmp/reproto-example.rp
cargo test --test tooling cpp_decodes_durable_payloads -- --exact
JAVA=/tmp/reproto-jre17/bin/java \
TLA2TOOLS_JAR=/tmp/reproto-tla2tools.jar cargo test --test protocol_models guard_model -- --exact
```

The example uses real localhost UDP sockets. Repeating it increments the stored
revision. Do not point it at a file in another format. The interoperability check
uses a temporary directory and decodes Rust-written payloads with the C++ schema
compiler. It checks serialization compatibility, not C++ RPC interoperability.

## Rust modules and source relationship

| Module | Implemented behavior |
| --- | --- |
| `transport` | Pinned Noise identity configuration; UDP driver; bounded ordered IO bridge on stream 0; timer and pacing integration |
| fork `vendor/quiche/quiche/src/tls/noise.rs` | Noise IK / IKpsk2 handshake replacing the TLS backend |
| fork `vendor/quiche/quiche/src/crypto/rust_crypto.rs` | Ring packet/header protection and Rust HKDF; no BoringSSL |
| `rpc`, `vendor/capnp-rpc` | Forked Rust RPC engine with native handoff and membranes; two-party convenience adapter |
| `vendor/capnp-rpc` third-party answers | Authenticated level-3 tail return adoption, self-adoption, shared invocation ownership and direct result capabilities; [scope and checks](ANSWER_ADOPTION.md) |
| `vendor/capnp-rpc::Joiner` | Authenticated multiparty secret-share Join with direct Noise acquisition ([profile](MULTIPARTY_JOIN.md)); standard bilateral capability Join, transparent relay forwarding across systems, Finish retention and cancellation; [scope and checks](JOIN.md) |
| `vendor/capnp::dynamic_orphan` | Reflected disown/adopt, independent allocation/copy, scoped views, fieldwise/contiguous typed groups and field transfer, typed release, immutable external/mmap data, lossless concatenation, tail reclamation and growth in place; message/context/type checks and capability ownership |
| `noise_rpc` | Multiparty Noise network, identity-bound rendezvous, on-demand pinned routes and connector cancellation |
| `authority` | Object-bound rights, attenuation, holder identity, shared lineage revocation |
| `handoff`, `semantics` | Provide/accept lifecycle, old-route drain barrier, duplicate acceptance, revocation |
| `introduction` | RPC ticket issuance and forwarding; automatic recipient-to-owner Noise connection and acceptance |
| `schema_exchange` | Read-only catalog capability, immutable revision publication, bounded parallel retrieval and dependency-closed bundles with transactional runtime loading |
| `capnp::schema_loader` | Owned validated schemas, typed stubs, compatible evolution, generic/implicit method reflection, native registration, dynamic messages and capability clients/servers |
| `noise_listener` | Shared UDP socket, bounded single-use pinned reservations, expiry/cancellation, retired IDs and native directory integration |
| `noise_provisioning` | Recipient-scoped provisioning capabilities, fresh PSKs/reservations, introducer relay, host readiness and automatic native dialing |
| `noise_arbitration` | Opt-in crossed-dial arbitration, epoch-bound selection/ack/commit, stable pending endpoints, winner datagrams and bounded cancellation |
| `realtime_datagram` | Session-local capability tokens, bounded fragmented Noise snapshots, atomic batch admission, reliable status/cancel/close, identical retries and driver/capability revocation |
| `realtime` | Snapshot capability, deadline guards, latest-per-key publication, bounded receipts, explicit cancellation and close |
| `bulk` | Per-transfer capability, strict payload credit, ordered chunks, sticky failure, cancellation and atomic in-memory publication |
| `orm` | Generic object capability, staged writes, publication, coalescing callbacks, durable pull history, limits, schema checks |
| `durable_bulk` | Resumable file upload, durable chunk journals, typed validation and atomic ORM/receipt publication |
| `storage` | Whole-entry append-only revisions, checksums, fsync, recovery, mapped snapshots |
| `bin/model` | Finite exploration of production revision/handoff transitions |

The upstream C++ clone remains pinned at
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. Its `rpc.c++`, `capability.c++`,
`membrane.c++`, and `rpc.capnp` are behavioral references, not linked runtime
libraries. Serialization, two-party RPC, promises, and reference tables reuse the
existing Rust `capnp`/`capnp-rpc` implementation rather than creating a second
translation of that existing port. New Rust modules implement the added profile.

The [RPC runtime port](RUNTIME_PORT.md) now includes a vendored engine with
standard wire Provide/Accept/ThirdPartyHosted support, multiple connections,
capability membranes, and real C++ interoperability checks. Full C++ RPC API
parity remains unfinished; [CPP_PARITY.md](CPP_PARITY.md) tracks the concrete
source-audited gaps. [Capability directory discovery](NOISE_DEPLOYMENT.md)
supplies dynamic endpoint lookup, bounded control-route failover, owned publication renewal and named identity
replacement; managed advertisements coordinate mapping changes, publication and
provider retirement independently of authenticated session identity. General multiparty
secret-share Join is implemented for the [Noise profile](MULTIPARTY_JOIN.md). Authenticated [third-party answer adoption](ANSWER_ADOPTION.md) is
implemented for the caller, relay and callee, with authenticated active-pipeline
Join fences on native Noise. Shared-socket listener reservations
are implemented in [NOISE_LISTENER.md](NOISE_LISTENER.md). Multiparty native RPC supports
preinstalled authenticated sessions and on-demand routes through a configured
`Connector`. `DirectoryConnector` resolves an allowlist of pinned peers to
dedicated endpoints or single-use shared-listener reservations. That guide
records the exact boundary.
`ProvisioningConnector` obtains fresh recipient-bound reservations and PSKs from
delegated provider capabilities. Its explicit introducer relay preserves the
existing control path during native handoff; see [NOISE_PROVISIONING.md](NOISE_PROVISIONING.md).
`Network::with_arbitration` adds coordinated session selection before RPC
publication, preserving pending capability tables and closing losing sessions.
Both peers opt in; see [NOISE_ARBITRATION.md](NOISE_ARBITRATION.md).

## Transport profile

The default backend in the quiche fork is Noise. The root crate explicitly
selects it with default features disabled. Standard TLS source remains in the
checkout for reference, but is excluded from this build. Noise cannot be combined
with the BoringSSL, crypto-bypassing fuzzing, or TLS C-FFI features.

* Experimental version: `0xff525001`, not an assigned interoperable QUIC version.
* Application profile: exactly `reproto/1`.
* Bootstrap: `Noise_IK_25519_ChaChaPoly_BLAKE3`, with both peer public keys pinned.
* Introduced connection: `Noise_IKpsk2_25519_ChaChaPoly_BLAKE3` with a fresh PSK.
* Prologue: `Capn't Proto-Noise-1/ff525001/reproto/1` followed by application context.
* Each handshake message is a big-endian u16 length plus Noise bytes, carried in
  Initial CRYPTO stream data. Transport parameters are Noise handshake payloads.
* Noise Split outputs seed quiche's directional packet protection key derivation.
  Packet AEAD, header protection, packet numbers, and key updates retain the
  quiche packet machinery. This is a custom binding, not the standard Noise
  transport-message encoding or TLS-secured QUIC.
* No 0-RTT application execution, TLS resumption, certificate fallback, or
  disabling key pinning. First-flight application data is never dispatched.

Both Cargo manifests pin upstream Snow at
`8ac60f51cfe3e010c84f0a454cc575ad9204fa12`, which provides the
[`use-blake3` feature](https://github.com/mcginty/snow/blob/8ac60f51cfe3e010c84f0a454cc575ad9204fa12/Cargo.toml).
Snow defaults are disabled; the selected features are `std`, `use-curve25519`,
`use-chacha20poly1305`, `use-blake3`, `use-getrandom` and `risky-raw-split`.
The BLAKE3 implementation comes from upstream Snow; no local hash implementation
or resolver override is used. BLAKE3 is an extension beyond Noise revision 34's
listed hash suites. It supplies the Noise transcript hash and handshake
HMAC/HKDF; the existing quiche packet-protection HKDF remains separate.
SHA-256-profile peers cannot complete this handshake, so both ends must upgrade.
The optional introduction PSK selects IKpsk2 with the same BLAKE3 hash.
`cargo test --test tooling pinned_noise_profile -- --exact` checks the resolved features, hash
digest, backend transcript binding and packet handshake regressions. The main
runtime checker also runs it. Dependency provenance is in
`vendor/provenance/snow-revision.json`.

Quiche supplies stream retransmission, flow control, congestion control, pacing,
packet duplicate handling, and DATAGRAM support. Tests exercise both streams and
datagrams, invalid keys/PSKs/contexts, ciphertext mutation, duplicate packets,
and loss of either handshake flight. These are implementation tests, not a
cryptographic proof or exhaustive testing of the fork's recovery behavior.

The async RPC adapters use one ordered stream per peer connection. Dedicated
sessions use a socket per peer; the shared listener routes multiple authenticated
sessions on one UDP socket.
`noise_rpc::Network` supports native multiparty handoff over preinstalled pinned
sessions, with per-vat authenticated rendezvous. Its session constructors bind
a `Capn't Proto native RPC v1\0` context prefix and exchange a stream-opening byte
before exposing RPC IO; either vat can then send the first call.
It retains 16 KiB staging buffers and a 64 KiB application bridge; transport
connection/stream windows are 2 MiB/1 MiB. Slow application reads do not block
packet processing. The introduction service allocates a socket for each offered
connection and limits active offers/sessions to 64 per introducer. Handshake
acceptance and idle timeouts are 10 seconds. Shared-socket routing, CID rotation,
STUN-assisted NAT rendezvous and validated client migration are implemented in
[the deployment profile](NOISE_DEPLOYMENT.md). [Local scheduling controls](NOISE_SCHEDULING.md)
bound packet bursts and optionally pace datagram admission per session. Full
ICE/TURN traversal, a portable server daemon, adaptive/per-capability scheduling
and deployment performance qualification remain.

## Introductions and authority

The owner hosts an `Introducer(T)` capability scoped to a parent grant and a
specific delegated rights set. A broker invokes `provide(recipientKey)`. The
owner creates a random introduction ID and PSK, binds the ID, keys, object,
generation, and rights into a SHA-256 context, and returns a ticket containing a
concrete socket address. The broker forwards that ticket through the recipient's
`IntroductionReceiver(T)` capability over another authenticated connection.

The recipient connects directly to the owner using the ticket's pinned target
key, PSK, and context. A `Handoff(T).accept(id)` bootstrap validates the introduction
and returns the restricted object capability. The recipient helper owns its
network tasks and cancels them on drop or failed/canceled connection setup.

For manually coordinated fences, call `handoff::serve::<T>(introduction, object,
owner, socket).await` inside a Tokio LocalSet and retain the returned `Serving`.
It validates the owner/object binding, authenticates the pinned recipient with
the introduction's PSK/context, and creates the private RPC bootstrap on that
session's stream. `Serving::wait()` waits for disconnection; dropping either the
owner or its waiting future cancels RPC and transport. Introduction key/context
fields are private. Revocation or replacement during authentication prevents
publication; replacement after authentication cannot authorize either an old or
new introduction ID on the captured bootstrap. The raw-byte authentication API
and public `Acceptor` constructor are removed in this unreleased 0.x API.
Both ticket endpoints now use the authenticated native context prefix and stream
preface described above. Ticket schemas and capability methods are unchanged.

The lower-level handoff state permits acceptance before old-route completion,
but withholds the direct capability until the pending count reaches zero.
`Introduction::state()` returns a snapshot with `phase()`, `pending()`,
`drained()`, `accepted()` and `is_revoked()` observations. Its named phases are
`Proxying`, `Offered`, `Embargoed` and `Direct`; fields and deserialization are
private/unavailable. Enqueue reserves cumulative drain-counter capacity and
returns false without changing state if no slot remains. Existing pending calls
can still drain after revocation. These are unreleased 0.x API changes.
Duplicate acceptance returns authority to the same logical grant; it does not
mint a new independent grant. The returned capability supports normal promise
pipelining. Revoking a parent invalidates its derived grants in the owning
process; revoking one child does not revoke siblings.

The high-level `provide` API **requires the caller's old-route completion fence**.
It does not transparently discover in-flight calls on arbitrary connections.
Tests cover an explicitly tracked outstanding operation and a fully drained
three-party path. Tickets contain secrets and must travel on authorized encrypted
channels. Grants, acceptance history, and PSKs are memory-resident, not durable
cross-restart replay state. Failed introduced connections require a fresh offer;
the original proxy capability remains available to the application. There is no
claim of seamless transparent migration of arbitrary existing RPC traffic.

## ORM contract

The schema is `schemas/store.capnp`:

```capnp
interface Object(T) {
  get @0 () -> (value :T, revision :UInt64);
  put @1 (expectedHead :UInt64, value :T) -> (revision :UInt64);
  publish @2 (revision :UInt64, expectedPublished :UInt64) -> (revision :UInt64);
  subscribe @3 (after :UInt64, observer :Observer(T))
      -> (subscription :Subscription);
  history @4 () -> (history :History(T), floor :UInt64, published :UInt64);
}
interface History(T) {
  next @0 (after :UInt64) -> (result :HistoryResult(T));
  cancel @1 () -> ();
}
```

`put` durably stages an entire new entry using compare-and-swap on the head.
`publish` durably changes the visible revision using a separate compare-and-swap;
publication must advance and reference an existing staged revision. `get` returns
only the published entry. A successful result follows fsync; a failed or lost
response does not prove the operation did not persist. There is no automatic
retry of mutations or promise of exactly-once external effects.

Subscriptions send published snapshots, initially the latest revision newer than
`after`. Slow observers receive coalesced updates with revision gaps, not a
complete durable event log. There is one callback in flight per subscription,
and at most 64 subscriptions per ObjectState. Cancellation/drop stops future
callback scheduling; a callback already delivered may already have run.

`history` exposes every retained publication through a pull capability. The
consumer persists its own cursor after processing each returned event and
supplies it to `next`; retries and reconnects do not advance it. Reads at the
current publication wait, canceled or revoked reads terminate, and expired
history returns an explicit gap. `Retention::History` preserves these events
across compaction and restart. Pull and push capabilities share the per-state
subscription limit. See [ORM_HISTORY.md](ORM_HISTORY.md) for the schema, cursor
scope, at-least-once processing contract and TLC/Rust trace coverage.

The schema is parameterized. This backend accepts generated struct types with a
stable schema ID, persists that ID, rejects type mismatches, and rejects live
capabilities embedded in stored values. The [persistence realm](PERSISTENCE.md) supplies explicit owner-sealed durable
references and typed object reconstruction; live client hooks are never stored. Method rights are checked on every call;
subscription authorization is rechecked before each callback.

## Storage format version 4

All integers are little-endian u64 unless stated otherwise. All record starts
and payloads are aligned to 8 bytes. One process holds an exclusive advisory
data-file lock and stable adjacent path lock for the lifetime of Store and every
mapped snapshot. Other programs must honor both locks and must not truncate or
rewrite the mapped file. The lock sidecar must not be unlinked or replaced.

File header (64 bytes): `RPROTO04`, version 4, checkpoint boundary, reserved
zero u64, then a SHA-256 checksum of the preceding 32 bytes. A new store has
an empty checkpoint ending at byte 64; older headers are rejected.
Each record is:

| Offset | Size | Meaning |
| --- | ---: | --- |
| 0 | 8 | `RPENTRY2` |
| 8 | 8 | Kind: 1 = staged entry, 2 = publication |
| 16 | 8 | Object ID |
| 24 | 8 | Revision |
| 32 | 8 | Payload length |
| 40 | 32 | SHA-256 of bytes 0..40 followed by unpadded payload |
| 72 | 8 | First 8 bytes of SHA-256 of header bytes 0..72; checked before trusting length |
| 80 | variable | Payload, followed by zero padding to 8-byte alignment |
| following payload/padding | 8 | `RPCOMMIT` |
| following magic | 8 | Total record length, including footer |

ORM payloads are schema ID followed by an unpacked Cap’n Proto framed message.
Publication records have empty payloads. Checksums detect corruption, not a
malicious writer. Storage is not encrypted at rest. Default limits are
16 MiB per payload and 256 MiB per file; host-selected `storage::Limits` permit
growth beyond these defaults. Reaching the configured capacity rejects writes.

Writes append records and fsync before advancing the in-memory index. Existing
mapped bytes are never modified. Readers retain an Arc-backed mapping and file
lock, while later revisions use a larger mapping. Opening a file scans the
validated prefix and removes an incomplete trailing record. The independent
header digest is checked before any length-based tail repair. Corrupt complete
headers fail without rewriting. Every successful reopen stabilizes the selected
file and containing directory before returning; either sync failure returns no
serving handle. An I/O failure quarantines the writer and new snapshot reads
until reopen/recovery. Existing snapshots remain
readable when Store is dropped and prevent a second writer from opening the file.

This is a mutable store through whole-entry replacement, not arbitrary in-place
mutation of serialized pointers. Explicit compaction atomically replaces a file
with a current-format compact or history-preserving checkpoint while preserving held mappings; see
[STORAGE_COMPACTION.md](STORAGE_COMPACTION.md) for retention, locks, configurable
quotas, the checkpoint format and verification. Multiple writers, cross-file
transactions, indexing beyond startup scanning, and recovery from arbitrary
filesystem/hardware corruption are not implemented.

## Verification evidence and boundaries

The additive [durable bulk service](DURABLE_BULK.md) uses single-store atomic
kind-7 batches to commit a typed ORM publication and its durable completion
receipt together. It replays checkpoints after reconnect/restart and validates
the complete payload before publishing. See that guide for the batch format,
retention rules, bounded file/entry limits and TLC/Rust verification.

`verification/Capn't Proto.tla` independently specifies revision and handoff behavior.
`cargo test --test protocol_models guard_model -- --exact` runs TLC, executes `reproto-model` against the
production `Revisions`/`HandoffState` methods, then compares every reachable state
and edge, including idle steps. At bound 2 they match: **324 states, 927 edges**.
TLC checks safety, stable revocation, no effects after revocation, and draining
under weak fairness. An intentionally missing embargo must violate `Embargo`.

`verification/StorageCrash.tla` explores **24 states** of write/persist/ack/crash
ordering and checks that acknowledged publication cannot roll back. Actual Rust
storage tests cut the appended byte sequence at every suffix length across a
replacement/publication, reopen it, and check the previous publication survives.
This models valid-prefix/torn-tail recovery and abstract durable writes, not all
possible reorderings or failures of real storage hardware.

Rust integration tests cover real UDP/Noise/Cap’n Proto calls, subscriptions,
rights denial, parent revocation, three-party ticket forwarding, a direct
introduced connection, and pipelined calls through acceptance. The C++ compiler
also decodes Rust-stored revisions. Reports and source hashes are under
`reports/reproto/`.

These checks are **not whole-program model checking of Rust**, a proof of the
Noise/quiche binding, full distributed Cap’n Proto conformance, or validation of
all existing TLA+ components as one implementation. The previously documented
conformance gaps remain. Production deployment requires additional protocol
review, operational limits, fuzzing, and interoperability work.

`StorageRecovery.tla` adds separate visible/durable append and pathname state,
uncertain writes, partial records, recovery barriers and a second crash. Its
734 states produce 1,082 concrete Store trace prefixes; three mutations omit
file sync, directory sync or independent framing validation. Test-only fault
injection exercises short writes, EINTR, ENOSPC/EIO and child-process death,
including atomic publication/receipt and realm revocation. Physical power loss
is simulated by restoring the last synced image, not certified on hardware.

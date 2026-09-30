# Cargo test suite

Planned additions and completion criteria are tracked in the
[correctness and testing roadmap](CORRECTNESS_ROADMAP.md).

All first-party test orchestration is Rust. Use:

```sh
cargo test --workspace
```

The suite includes ordinary runtime tests, capability/RPC trace replays, TLC
invariants and liveness properties, named mutation controls, generated API
compiler diagnostics, installed and pinned C++ comparisons, feature combinations,
an external consumer, isolated crash tests, Valgrind, and the private EAE probe.
It requires Rust 1.97, Java 17, the TLC 1.7.4 jar, Cap'n Proto development tools,
a C++23 compiler, CMake, Ninja, pkg-config, Valgrind and unzip. Set `JAVA` and
`TLA2TOOLS_JAR`, or install the jar at `target/tools/tla2tools.jar`.
On x86-64 Linux, the serialization/Noise fuzz gate additionally requires cargo-fuzz 0.13.1 and
nightly-2026-08-29; installation and bounded-run commands are below.
The guard quality gates also require cargo-mutants 27.1.0, cargo-llvm-cov 0.8.6
and the pinned toolchain's `llvm-tools-preview` component; commands are below.
First activate the [auditable Cargo setup](QUALITY.md#auditable-cargo-builds);
it pins cargo-auditable 0.7.6 and rust-audit-info 0.5.4 for builds and the binary
metadata regression in the tooling suite. Keep its PATH active when installing
the other Cargo tools and running the test commands below.

Useful selections:

```sh
cargo test --test protocol_models -- --list
cargo test --test protocol_models storage_recovery_model -- --exact
cargo test --lib storage::recovery_traces::replay_tlc_storage_recovery_traces -- --exact
cargo test --test tooling generated_api_compile_contracts -- --exact
cargo test --locked --test tooling nightly_rpc_try_contracts -- --exact
cargo test --locked --test tooling capnp_runtime_lints -- --exact
cargo test --locked --test capnp_root
cargo test --test tooling external_consumer_feature_matrix -- --exact
cargo test --test tooling eae_feasibility_probe -- --exact
cargo test --test release source_bundle_roundtrip -- --exact
cargo test --locked --test flow_control --test streaming
cargo test --locked --test flow_control --test tcp_rpc
cargo test --locked --test outgoing_queue --test output_completion
cargo test --locked --test buffered_input
cargo test --locked --lib unix_rpc::
cargo test --locked --test fd_capabilities
cargo test --locked --test capability_debug --test deferred_handoff
cargo test --locked --test field_presence --test schema_loader
cargo test --locked --test dynamic_conversion
cargo test --locked --test result_pipeline
cargo test --locked --test result_construction
cargo test --locked --test structural_equality
cargo test --locked --test any_struct
cargo test --locked --test any_list
cargo test --locked --test twoparty_facade
cargo test --locked --test unix_facade --test fd_capabilities
cargo test --locked --lib unix_rpc::
cargo test --locked --test membrane_copy --test membrane --test result_construction
cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc capability::Results
```

## Noise transport correctness

```sh
cargo test --locked --test tooling quiche_noise_regressions -- --exact --nocapture
cargo test --locked --test tooling quiche_noise_handshake_model -- --exact --nocapture
cargo test --locked --test tooling pinned_noise_profile -- --exact
cargo test --locked --test noise --test noise_multiparty --test noise_pipeline_migration
```

`quiche_noise_regressions` runs the fork's library tests with
`--no-default-features --features noise`. It checks the discovered test inventory,
runs **1,104 cases**, and requires zero failures, ignored cases and filtered cases.
All 59 formerly pending adaptations are included; their temporary exclusion
inventory has been removed. New library tests run automatically. Missing critical
test families or a shrinking inventory fail the gate. This is the complete library
suite for the Noise feature set, not a full quiche workspace, feature-matrix or
interoperability qualification. These results were checked on **2026-09-24**.

The fixtures saturate the amplification budget with a large authenticated Noise
response, use actual sent packet numbers for injected ACKs, and assert congestion
credit relative to the starting window. Handshake-stage tests cover key retirement,
close behavior, corrupted packet types and reordered application packets. PMTU
fixtures finish the handshake with 1,200-byte output buffers so the larger probes
remain queued for explicit delivery or loss. These inherited fixtures still use
wall-clock timers and real entropy. The simulator below supplies a separate
controlled environment for its packet and model-replay scenarios.

The backend excludes inherited certificate verification/accessors, TLS session
resumption, TLS ALPN negotiation, application 0-RTT and H3 0-RTT tests (20 cases
from the audited Noise inventory). Noise-specific tests instead assert rejection
of TLS configuration, missing identities, session input and early stream data.
H3 framing tests run over pinned `reproto/1` peers solely to retain parser/stream
regressions. Production ALPN and authentication requirements are unchanged.

`quiche_noise_handshake_model` runs fresh TLC exploration of
[NoiseHandshakeConfirmation.tla](../verification/NoiseHandshakeConfirmation.tla),
then supplies every graph edge-prefix trace to the fork's real-packet test driver.
The model currently has **242 states and 391 edge prefixes**. It observes both peers'
establishment, Initial-key retention/retirement, address verification, client
confirmation and application delivery. Seven negative model controls cover premature
retirement, missing server/client retirement, forged verification, verification of
the wrong address, unauthenticated delivery and duplicate delivery.

Bounds: one IK handshake, two address tuples, and at most one delayed responder
flight, forged confirmation, confirmation replay and Initial replay. HANDSHAKE_DONE
can arrive separately from the IK response; one post-IK stream message can confirm
the server's original address first. Replay must not deliver that message twice
within the connection; this does not model RPC retries across reconnects. The driver
splits coalesced datagrams into protected packets to match that scheduling unit.
Cryptography is abstracted in TLA+ and exercised through Snow in Rust; this does
not prove cryptographic security, timer fairness, coalescing behavior or arbitrary
network schedules. Native lost-flight tests separately exercise actual timeout
retransmission. The 32 KiB response regression checks that a tiny authenticated
request no longer leaves the server capped by its initial amplification budget.
TLC requires Java and permission for its local worker socket as described above.

### Checked Noise identities

```sh
cargo test --locked --test identity -- --nocapture
cargo test --locked --doc transport::identity::Identity
```

`Identity` constructors are checked against the independent Alice/Bob X25519
vectors in [RFC 7748 section 6.1](https://www.rfc-editor.org/rfc/rfc7748#section-6.1).
Tests cover mismatched pairs, equivalent masked scalar bits, immutable public
copies and public-only diagnostics. Generated and imported identities complete
real IK/IKpsk2 handshakes after the source identity is dropped; UDP tests check
that authenticated session handles name the proven peer. Three compile-fail
doctests prevent direct construction, field mutation and private-key access.

[NoiseIdentity.tla](../verification/NoiseIdentity.tla) bounds histories to four
operations, two valid key pairs, one invalid public label, one live identity and
one replaceable configuration. Fresh TLC exploration checks **75 states / 190
edge prefixes**; Rust replays every prefix, including **12 real handshakes**.
Replay compares constructor outcomes, live public identities, configuration
presence/labels and authenticated peer labels. Configured private keys remain
opaque; real protected stream transfer checks possession of the named key.
Five model faults must violate the named binding, ownership or outcome invariant:
accepting a mismatched pair, rejecting a valid pair, incorrect derivation,
mislabeling a configuration and losing its key when the source identity drops.

TLA+ abstracts cryptography; these are bounded API/ownership checks, not a
cryptographic proof, entropy assessment or memory-erasure test. The owned scalar
uses `Zeroizing`; a guard overwrites Snow's temporary derivation scalar before
release. This does not guarantee erasure of compiler/provider intermediate copies
or caller-owned copies. Existing configurations and sessions own separate key
state: dropping an `Identity` does not revoke them.

### Authenticated application handoff

```sh
cargo test --locked --test handoff_auth --test rpc -- --nocapture
cargo test --locked --doc handoff::
```

`handoff_auth` uses independent raw wire peers to challenge the new serving
boundary with incorrect recipient/host keys, wrong or missing PSKs, context/domain
mismatches and invalid/missing stream prefaces. A valid handshake without its
stream preface remains unpublished during a 250 ms observation window, then
cancellation must release the UDP socket. Other invalid peers must fail or remain
unpublished within the same bounded window. These are not proofs about arbitrary
network timing. Preflight tests reject wrong owners/objects, unprovided offers
and revoked grants without waiting for a peer. Native tests also cover replacement
or revocation while authentication is pending, both IDs after replacement on an
established connection, rights attenuation, and canceled acceptance/serving owners.
Three compile-fail doctests reject authentication assertions using raw bytes,
recipient-field mutation and access to the private bootstrap constructor.

[AuthenticatedHandoff.tla](../verification/AuthenticatedHandoff.tla) models one
provided introduction, one pending proxy call, one connection, up to two Accept
requests and four operations. Fresh TLC exploration checks **158 states / 236
edge prefixes**; every prefix replays through real UDP/Noise, the production
serving API and generated RPC calls. Replay compares publication, request results,
phase, pending count, acceptance count and parent-grant liveness. Repeated
successful requests must return the same retained capability identity. Connection
closure tears down both test endpoints. The abstract bad proof maps to a wrong
Noise context; separate cases above cover the other handshake bindings. Revocation,
replacement, wrong IDs and draining can interleave with acceptance within the bound.
Six model controls must detect false authentication, wrong-ID acceptance, revoked
or replaced authority, premature capability publication and duplicate acceptance.

The model abstracts cryptography and the socket driver, and makes no liveness or
durable restart/replay claim. `rpc` retains the larger three-party scenario with a
real old-route write/publication, delayed direct authority, pipelining, duplicate
capability identity and parent revocation. The high-level offer still requires an
explicit caller completion fence; it does not discover arbitrary in-flight calls.

### Immutable grant bindings and opaque route generations

```sh
cargo test --locked --test api_contracts -- --nocapture
cargo test --locked --lib noise_rpc::generation:: -- --nocapture
cargo test --locked --test authority --test handoff_auth --test durable_bulk --test persistence
cargo test --locked --test noise_multiparty --test noise_shutdown
```

`Grant::object()`, `generation()` and `holder()` return immutable binding values.
Delegation still checks the parent's live DELEGATE right and a subset of rights;
clones preserve revocation lineage. Trusted root issuance and authenticated-holder
binding remain the owning service's responsibility. The binding change does not
turn in-process Rust types into a security boundary against trusted root issuers.

`RouteObserver::generation()` returns opaque `RouteGeneration`. `get()` is an
explicit numeric projection for diagnostics and trace encoding. Generations are
nonzero and monotonically allocated within one network, including `u64::MAX`;
subsequent new routes fail without wrapping or recycling. Distinct networks start
independent counters, and stale route cleanup still uses owner identity rather
than numeric equality. Native tests cover exhaustion with direct and arbitrated
connectors, live-route reuse and rejection before another dial or route insertion.

[api_contracts.rs](../tests/api_contracts.rs) compiles a real external consumer:
one positive case and **15 required failures** with exact structured Rust
diagnostics. Cases reject writes to each grant binding, construction/conversion/
arithmetic/default/deserialization of route IDs, grant deserialization, sending
or sharing local grant owners across threads, sending route observers and ignoring
a returned grant under `deny(unused_must_use)`. Six compile-fail doctests provide
adjacent API examples. Cargo records source and lockfile hashes, compiler identity,
case sources and logs under `target/verification/api-contracts`.

[RouteGeneration.tla](../verification/RouteGeneration.tla) checks **39 states /
78 edge prefixes**, replayed against the production allocator seeded at
`u64::MAX - 2`. Two observer slots retain/release the last three identifiers.
Invariants require nonzero successes, allocation while capacity remains, permanent
exhaustion, distinct live values and no reuse after release. Four controls detect
zero allocation, early failure, wraparound and recycling. Weakly fair actions
eventually exhaust the finite supply. This checks the allocator boundary, not
arbitrary-width arithmetic, global uniqueness across networks, authentication or
all route installation schedules; existing lifecycle and replacement tests cover
the surrounding ownership behavior.

### RPC identifiers and table ownership

```sh
cargo test --locked -p capnp-rpc --lib
cargo test --locked --test rpc_tables -- --nocapture
cargo test --locked --test api_contracts rpc_identifier_and_table_compile_contracts -- --exact --nocapture
cargo test --locked --test runtime --test call_hints --test import_alias --test cancellation
cargo test --locked --test join --test multiparty_join --test answer_adoption --test tail_transfer
cargo test --locked --test deferred_handoff --test disconnect_cleanup --test noise_pipeline_migration
cargo test --locked --test tooling installed_cpp_rpc_interoperability -- --exact
```

[RPC IDs](../vendor/capnp-rpc/src/rpc_ids.rs) are distinct private types for local
questions, answers, imports, exports and embargoes. Decoding follows the local
endpoint's perspective: incoming Call/Finish IDs address answers, Return IDs
address questions, sender-owned capability descriptors address imports, and
Release/receiver-owned descriptors address exports. The schema continues to use
32-bit numbers, including zero. `RequestHook::tail_send()` remains a shared hook
boundary exposing a wire number; peer-owned loopback contexts are echoed opaquely.

[Tables](../vendor/capnp-rpc/src/rpc_tables.rs) require the matching key type and
local/peer allocation role. Only questions support high or adopted insertion.
Private allocator storage centralizes allocation and removal: removing a missing
entry is inert, duplicate removal cannot duplicate a free slot, and failed
adoption returns the attempted value without replacing the existing owner.
`remove()` transfers ownership so runtime export destruction can happen after
releasing the table borrow. Adoption reserves its slot before constructing the
cleanup owner. These types do not brand connections or prevent stale IDs from
reaching legitimately reused slots; runtime ownership and protocol lifetime rules
remain necessary. Exhaustion of the large local allocation spaces retains the
existing assertion policy and remains part of the broader numeric-contract audit.

The Cargo compiler gate checks **50 contracts**: one positive control and 49
required failures with exact diagnostic codes, including all 20 directed
interchanges among the five ID domains. It compiles the actual private module
files in a consumer with the same parent visibility as the runtime, avoiding a
new public testing API. Source/compiler/lockfile hashes and case logs are retained
under `target/verification/rpc-id-contracts`. The separate grant/generation gate
still checks its original 16 contracts.

[RpcIdTables.tla](../verification/RpcIdTables.tla) explores **1,041 states / 11,304
edge prefixes**. [Cargo replay](../tests/rpc_tables.rs) calls the actual production
table implementation for every prefix and compares operation results, occupied
IDs, retained values, domain isolation, adopted count and idleness. Bounds are
two low slots in each of the question/export domains, one adopted question, and
one high allocation. Four controls must detect live-slot reuse, cross-domain
mutation, sparse removal changing a low slot, and adopted-owner replacement.
This is a bounded safety check, without a liveness claim. Separate native tests
exercise high-counter wrap/collision skipping, both adopted-range endpoints,
invalid insertion, duplicate/missing removal and full-width peer IDs. Runtime
trace suites and C++ interoperability cover the surrounding wire interpretation;
the table model itself does not model RPC handlers or prove stale-ID safety.

### Bulk reservation ownership

```sh
cargo test --locked --test bulk_reservations -- --nocapture
cargo test --locked --test api_contracts bulk_reservation_compile_contracts -- --exact --nocapture
cargo test --locked --test bulk --test durable_bulk -- --nocapture
```

[CreditWindow](../src/bulk/credit.rs) returns opaque, immutable reservations bound
to their issuing window. Only an explicit sequence projection reaches the wire;
local settlement takes the retained handle and distinguishes first release from
duplicate settlement. Foreign or old-window handles fail without changing credit.
Dropping a handle leaves outstanding debt. The sender keeps reservations with
pending RPC replies and retains its separate first-error/publication checks.
The old raw-number acknowledgment API has been removed for this unreleased 0.x API.

[BulkReservation.tla](../verification/BulkReservation.tla) checks **1,749 states /
8,866 edge prefixes** against the production API. Bounds are two two-byte windows,
two issued sequences each and one retained handle per window. Replay compares
credit, issuance, outstanding byte/chunk debt, retained sequence/size metadata,
operation results and unchanged state on rejection. Five controls must fail for
foreign release, duplicate credit, credit release on drop, sequence-budget bypass
and backpressure charging. This is a bounded safety check, without a liveness or
remote-execution claim. Native tests cover moving/replacing windows and the full
65,536-chunk boundary. The original bulk component and actual RPC trace suites
continue to exercise delayed, reordered, duplicate and failed replies.

The compiler gate runs **16 external-consumer contracts**, preserving `Send` and
`Sync` while rejecting private-field access/mutation, cloning/copying, default or
numeric construction, deserialization, use after move, ignored reservation or
settlement results, implicit numeric decay and raw-number acknowledgment.
Artifacts with source/compiler/lockfile hashes and case logs are retained under
`target/verification/bulk-reservation-contracts`. Runtime identity checks still
decide whether a valid reservation belongs to a particular window; a reservation
does not itself prove a matching remote reply was observed.

### Checked bulk configuration

```sh
cargo test --locked --test bulk_config --test bulk --test durable_bulk -- --nocapture
cargo test --locked --test api_contracts bulk_config_compile_contracts -- --exact --nocapture
```

[Config](../src/bulk/config.rs) validates length, chunk size, byte window and
chunk count before issuing an immutable value. Checked construction and JSON/RPC
imports share the same boundary; only getters expose the limits. JSON retains
numeric fields but rejects invalid values, unknown/duplicate fields and missing
fields. The receiver accepts a checked value without another fallible validation
step. Durable journal decoding checks it before resuming or exposing progress.

[BulkConfigBoundary.tla](../verification/BulkConfigBoundary.tla) explores
**9,421 states / 70,275 edge prefixes**. Inputs are the 108 tuples with length
0..3 and chunk/window/count 0..2, plus seven samples at the production maxima.
Each uses direct construction, JSON import or the production sender's local RPC
describe boundary. Accepted small configurations then receive at most three
write/done/cancel calls; write sizes and sequence numbers range from 0..3.
Replay compares import/operation outcomes, status, historical byte/chunk counts,
staging, sticky failure and publication, including the complete payload. Large
valid limits are imported without allocating their declared payload. Capacity
uses ceiling division in TLC to avoid its integer multiplication limit and the
equivalent widened product in Rust.

Seven model controls must detect bypassed length/chunk/window/count/capacity
limits, oversized writes and premature completion. Native tests additionally
cover `u64`/`u32` extremes, exact capacity, malformed JSON, invalid advertisements
over framed two-party RPC, a valid transfer through the same path, and journal
rejection without changing storage. Existing bulk/reservation/durable trace suites
continue to check credit, ordering, cancellation, recovery and publication.

**13 external compiler contracts** check private construction/mutation, checked
constructor results, required checked input to the receiver, absence of default
or tuple conversion, immutable sender limits, `must_use`, cloning and `Send`/`Sync`.
Artifacts are under `target/verification/bulk-config-contracts` and
`target/verification/bulk-config-boundary`. This is bounded safety and import
validation evidence, not a liveness, resource-quota or unbounded protocol proof.
Configuration values confer no transfer authority.

### Discovery generations and captured bindings

```sh
cargo test --locked --lib noise_discovery:: -- --nocapture
cargo test --locked --test discovery_types --test noise_deployment --test noise_discovery_failover -- --nocapture
cargo test --locked --test api_contracts discovery_generation_and_resolved_compile_contracts -- --exact --nocapture
```

[DiscoveryGeneration](../src/noise_discovery/generation.rs) is a nonzero domain
shared by publication, renewal, advertisement status and decoded discovery
records. Administrative absence is `None`; replacement requires a typed expected
generation. Numeric wire encoding is unchanged. The issuing directory still
checks compare-and-replace and ownership; numeric imports are metadata, not
authority or an instance brand.

[Resolved](../src/noise_discovery/resolved.rs) keeps decoded binding coordinates,
provider, generation and conservative expiry immutable. Only the decoder can
issue one through the public API. Connection setup consumes it and rechecks
recipient/expiry before provisioning and expiry while setup is pending. Native
tests check authoritative rejection of zero wire generations, maximum `u64`
imports, isolated mutation of cloned proposals, foreign recipients, the exact
expiry boundary and setup already waiting on a provider. A real encrypted
two-host test retains a result across rotation, connects both old and new captured
bindings to their respective Noise identities, and executes RPC on each.
Retaining a result is not a revocation subscription: a low-level directory update
does not independently revoke an already delegated provider.

[DiscoveryGenerationBoundary.tla](../verification/DiscoveryGenerationBoundary.tla)
checks **393 states / 1,498 edge prefixes**. Generations 1..3 map to the last
three `u64` values. Bounds are one name/recipient, two host bindings, one renewal
owner, one initially captured result and three publish/renew/revoke/stop/close
operations. Each prefix drives the production directory, lease, RPC lookup and
decoder. Observations compare results, the allocation counter, live entry,
owner/status, host and generation, and preservation of the captured result.
Six fault controls reject wrap, counter reuse, stale publication/renewal,
successor deletion and captured-result relabeling. Native exhaustion tests also
check unchanged entry identity/deadline, allocation across the final values,
continued lookup and rejection of recreation after revocation. Existing
publication, discovery and failover models retain expiry/liveness coverage;
the new boundary model is a bounded safety check with time held fixed.

**30 external compiler contracts** protect construction, mutation, identifier
separation, absence semantics, explicit numeric boundaries, `must_use`, and
capability-local thread traits. Reports are under
`target/verification/discovery-type-contracts` and
`target/verification/discovery-generation-boundary`. These checks do not prove
unbounded histories, cryptographic security, durable generation allocation or
consistency across independent directory authorities.

### Checked realtime configuration

```sh
cargo test --locked --test realtime_config --test realtime --test realtime_datagram -- --nocapture
cargo test --locked --lib realtime -- --nocapture
cargo test --locked --test api_contracts realtime_config_compile_contracts -- --exact --nocapture
```

[Config](../src/realtime/config.rs) validates clock-domain byte length, key and
queue counts, sequence namespace, payload/waiter limits and the combined payload
budget before issuing an immutable value. Wire imports also reject invalid
UTF-8 before allocating the owned domain. `Receiver::new` consumes checked limits
and returns its pair directly; datagram binding and sender import still enforce
the smaller `MAX_SNAPSHOT_BYTES` bound. All `u64` skews remain valid configuration:
overflow or insufficient margin at use time prevents publication.

[RealtimeConfigBoundary.tla](../verification/RealtimeConfigBoundary.tla) checks
**15,108 states / 19,811 edge-prefix replays**. Inputs are 486 combinations of
keys/capacity/sequence/payload/waiter limits in 0..2 and skew in {0,2}, plus twelve
production-boundary samples. Local construction accepts UTF-8 strings; the wire
path additionally explores invalid UTF-8. Accepted small configurations receive
an offer, optionally a retained duplicate, and an apply. Offer sequence/size range
from 0..3 and key from 0..2; time is fixed at zero and deadline at two. Large
configurations exercise import and receiver creation without payload allocation.
The budget check uses division in TLC to avoid its 32-bit multiplication limit;
Rust widens the arithmetic.

Replay uses the real sender describe boundary and receiver, comparing acceptance,
error class, terminal outcomes, waiter counts, pending work and the published
snapshot's sequence, key, deadline and bytes. Eleven model fault controls weaken
domain/UTF-8, each numeric bound, the combined budget, admission, duplicate quota
or deadline checking and must violate the named invariant. These are model
sensitivity controls, not Rust mutation testing. Existing receiver, receipt and
fragmentation models cover longer histories and concurrent queued keys.

Native cases additionally cover maximum-width invalid numbers, an owned domain
independent of its source string, multibyte domains at and above 128 bytes, exact
64 MiB budgets, malicious advertisements over framed RPC, sender sequence
exhaustion and datagram import/binding limits on an authenticated Noise session.
The **16 external compiler contracts** check private construction/fields,
read-only domain/sender access, absence of default/Serde bypasses, checked
construction, infallible receiver creation, cloning, thread traits and required
result handling. Artifacts are under `target/verification/realtime-config-boundary`
and `target/verification/realtime-config-contracts`.

This is bounded import/admission safety, not a proof of clock synchronization,
capability authorization, liveness or total process memory use. The payload
budget excludes retained application snapshots, RPC/transport buffers and
metadata. Numerical wire fields and protocol semantics remain unchanged.

### Packet reservations and reentrant datagram admission

```sh
cargo test --locked --lib datagram -- --nocapture
cargo test --locked --test realtime_datagram -- --nocapture
cargo test --locked --test api_contracts datagram_batch_compile_contracts -- --exact --nocapture
```

The regression in [datagram_reentry_tests.rs](../src/transport/datagram_reentry_tests.rs)
first failed with two offers receiving sequence 1: Tokio's queue send invoked a
receiver waker synchronously, which submitted through a sender clone before the
outer offer advanced its counter. [Batch](../src/transport/datagram_batch.rs)
now binds a complete validated packet slice to reserved queue capacity. The sender
reserves, advances its sequence, then consumes the batch to enqueue. No state
borrow spans publication and no later counter assignment can overwrite a nested
offer or close. Rejected reservations consume no sequence or queue slots.

The move-only reservation borrows both queue and packets. Dropping it releases
capacity without sending; after commitment, existing permits allow submission
even if a wake callback closes the queue during publication. This does not
establish remote receipt or execution. Thirteen compiler contracts compile the
exact private module and reject construction/field access, mutation or dropping
borrowed inputs, lifetime escape, cloning/copying/defaulting, repeated send and
ignored reservations. The compiler does not itself enforce the sender's
reserve/commit/send order.

[DatagramReentry.tla](../verification/DatagramReentry.tla) explores **3,021 states /
3,192 edge prefixes**. There are 640 initial scenarios: a sequence limit of one
or two, zero through the limit already issued, zero through three queued
pressure packets, one/two outer fragments, four callback modes, local closure
and connection closure. The callback does nothing, offers two fragments, closes
the sender, or retries an earlier receipt. The model includes remaining reserved
capacity during the wake, counter visibility and nested issuance. Each scenario
can then retry, close-and-retry, drain-and-retry or drain-and-offer.

Rust replay uses the production sender, its clone, real Tokio queue permits and
a custom waker that runs the nested operation synchronously. It compares error
classes, issued/retained sequences, local/session closure, queue length, capacity
and the counter visible inside the callback. At explicit drains and every replay
endpoint it checks the ordered wire sequence IDs, including interleaved fragment
batches and unrelated queue pressure. Eight model fault controls cover late or
overwritten commitment, insufficient reservation, partial failed batches, burned
IDs, ignored closure, counter advancement on retry and altered retry identity.
They are model sensitivity controls; the pre-fix native regression separately
demonstrates an implementation failure.

Native regressions additionally check a compact nested offer during fragmented
submission, exhaustion, local and queue closure during publication, invisible
unsubmitted reservations and capacity recovery on drop. Existing batch and
fragment tests cover exact retry bytes, all 64 fragments and real Noise sessions.
Artifacts are under `target/verification/datagram-reentry` and
`target/verification/datagram-batch-contracts`. These are finite safety checks
under returning wake callbacks, not weak-memory, panic-recovery, arbitrary-depth
reentry, transport-delivery or liveness proofs.

### Realtime record and receipt lifecycle

```sh
cargo test --locked --lib realtime -- --nocapture
cargo test --locked --test realtime --test realtime_datagram -- --nocapture
cargo test --locked --test api_contracts realtime_snapshot_compile_contracts -- --exact --nocapture
```

[Record](../src/realtime/record.rs) separates queued work (required snapshot and
waiters), finished work (duplicate-detection metadata and first outcome), and
pre-offer canceled/closed tombstones (no payload identity). Admission inserts a
complete variant. Completion releases queued ownership and takes waiters for
notification after the receiver's borrow ends. Terminal records have no payload
or waiter fields. Matching retries preserve their outcome, conflicting known
metadata fails, and a cancellation tombstone cannot be revived by a later offer.

[Snapshot](../src/realtime/snapshot.rs) exposes immutable sequence, key, deadline
and byte-slice getters. Native tests retain owned/shared copies through newer
publication, receiver closure and owner drop. A longer history checks that
repeated apply/cancel/offer observations of an old `Applied` result cannot replace
the newer visible allocation. The deadline constrains publication, not the
lifetime of an already published value.

[RealtimeReceiptLifecycle.tla](../verification/RealtimeReceiptLifecycle.tla)
explores **12,761 states / 41,966 edge prefixes**. Bounds are one key, two
sequences, two payload identities, two retained receipt slots, a quota of two waiters,
and four operations. A transient third observer exercises quota rejection before
supersession and receipt dropping. An injected clock ranges from 0..2, with both
deadlines at 2; expiry and publication are explicit actions. Replay drives the
production receiver and receipt futures, comparing operation results, readiness,
terminal outcomes, waiter counts, high-water mark, pending index, record metadata,
payloads, visible values, time and closure. Model application counters describe
new publications, not repeated returns of the cached `Applied` result.

Seven model controls must detect changed-metadata acceptance, quota bypass,
tombstone revival, retained waiter charges, cancellation on receipt drop,
overwritten outcomes and duplicate application. Separate native regressions
observe payload allocation release through weak references, preserve work at
waiter-ID exhaustion, and poll actual user wakers that reenter the receiver's
state on notification. Both duplicate waiters wake once; dropping waits preserves
work. Existing realtime/datagram/fragmentation models and real Noise/RPC tests
exercise the surrounding adapters, including multi-key capacity and clock faults.

**14 external compiler contracts** reject construction, coordinate/payload
mutation, mutable byte access, default/deserialization, copying, cross-thread
use and ignored snapshots, while preserving owned clones and diagnostics.
Artifacts are under `target/verification/realtime-snapshot-contracts` and
`target/verification/realtime-receipt-lifecycle`. The new model provides bounded
safety evidence; its deterministic polling is not executor exploration, and it
does not establish liveness, hard deadlines, arbitrary histories or transport
delivery guarantees. Snapshots are values, not receiver-branded authority.

### Persistent descriptor bindings

```sh
cargo test --locked --test persistence_types --test persistence -- --nocapture
cargo test --locked --test api_contracts persistent_descriptor_compile_contracts -- --exact --nocapture
```

[Descriptor](../src/persistence/descriptor.rs) holds private `ObjectKind`,
`ObjectId`, `ObjectGeneration` and `Rights` values. Checked numeric construction
and deserialization cannot produce zero IDs or invalid rights. The factory
registry, ORM factory generation, grants and object-state cache retain the same
distinct types; Store addressing explicitly converts an authority ID to
`storage::ObjectKey` without converting it to a revision number.
Native tests cover all 32 valid rights masks, the full-width numeric boundary,
an independent JSON fixture with distinct IDs, malformed/duplicate/missing/extra
fields and stable typed factory/state bindings.

[PersistentFactoryBinding.tla](../verification/PersistentFactoryBinding.tla)
checks **293 states / 564 edge-prefix replays** using real Realm registration,
binding, standard Persistent.save, restoration and ORM get/put calls. Bounds are
two kinds, two objects, two generations, VIEW plus DELEGATE versus ALL, one bound
descriptor and at most one restore. Factories accept different generations so
dispatch errors cannot hide behind interchangeable factories. Every endpoint
compares registration, bindings, restoration outcome, observed object and write
permission. Five controls must detect registration bypass, wrong dispatch,
generation bypass, object substitution and rights expansion. Evidence is retained
under `target/verification/persistent-factory-binding`.

The compiler gate checks **42 external-consumer contracts**: a positive control,
all six directed ID swaps, private construction/mutation, numeric conversions,
defaults, arithmetic, typed factory and ORM arguments, separation from route
generations and ignored descriptors. Source/compiler/lockfile hashes and case
logs are retained under `target/verification/persistent-descriptor-contracts`.

These descriptors are metadata, not capability authority or realm-branded IDs.
Trusted hosts still bind the intended capability, and custom factories must check
generation and exact authority. The bounded model assumes a fixed authorized
owner and makes no liveness claim; existing persistence models separately check
owner sealing, revocation, asynchronous completion, expiry, restart and compaction.

### Shared object authority domains

```sh
cargo test --locked --test object_authority --test authority -- --nocapture
cargo test --locked --test api_contracts object_authority_compile_contracts -- --exact --nocapture
cargo test --locked --test durable_bulk --test persistence --test rpc --test handoff_auth -- --nocapture
cargo test --locked --test authority_schedules -- --nocapture
```

`authority::ObjectId` and `ObjectGeneration` are shared by grants and persistent
descriptors. `Grant::root` and `ObjectState::new` accept checked identifier values;
grant getters and delegation retain them. ORM object/store bindings are private:
typed `object()` and borrowed `store()` inspection cannot retarget the state.
Handoff contexts explicitly encode the original numeric values, and upload
journals serialize the shared types as numbers with checked decoding. Store
addressing and upload journals now use their separate key types below; revision
and cursor values remain numeric.

[ObjectAuthorityBinding.tla](../verification/ObjectAuthorityBinding.tla) checks
**177 states / 372 production edge-prefix replays**. Each replay initializes two
real stored ORM objects, issues one root grant, delegates VIEW or ALL, attempts
one object binding and makes one get/put pair. Revocation may occur before or
after each step. Replay compares all modeled binding and result fields, holder,
exact rights, both Store heads/publications and unchanged object/store bindings.
Five controls must detect object or generation substitution during delegation,
rights expansion, wrong-object admission and invocation after revocation.
Artifacts are under `target/verification/object-authority-binding`.

**17 external compiler contracts** cover typed grant/ORM arguments and getters,
immutable ORM binding fields/accessors and local `Send`/`Sync` restrictions;
artifacts are under `target/verification/object-authority-contracts`. Native
regressions cover `u64::MAX` bindings through delegation/persistence and invalid
or substituted upload-journal IDs without mutation. The existing rights,
revocation, persistence, handoff and Shuttle checks also use the typed APIs.

The model checks bounded safety with trusted root issuance and two object IDs
and generations. It does not add or verify an incarnation registry, cross-store
identity, authenticated holder provisioning or liveness. ORM generation values
remain host-managed metadata; persistent factories and resumed uploads enforce
their own generation checks.

### Storage keys and snapshot identity

```sh
cargo test --locked --test storage_keys --test storage --test history --test durable_bulk -- --nocapture
cargo test --locked --test api_contracts storage_key_and_snapshot_compile_contracts -- --exact --nocapture
cargo test --locked --lib storage:: -- --nocapture
```

`storage::ObjectKey` separates Store addresses from revision numbers across the
public API, batches, indexes, recovery and compaction. Every `u64` is valid,
including zero; `new()` imports a numeric key and `get()` explicitly encodes it.
An authority `ObjectId` converts with `ObjectKey::from(id)`. Upload APIs accept
`durable_bulk::JournalId`, whose `key()` supplies a Store address. These types
identify slots; they do not brand instances or confer authority. The on-disk
format and revision ordering checks are unchanged. Publication cursors now have
an additional owner-bound API, described below.

Snapshots retain private object/revision coordinates with their mapped bytes;
`object()` and `revision()` are read-only observations, and clones retain the
mapping and locks. Native regressions exercise zero, values above 32 bits and
`u64::MAX`, held snapshots after compaction/Store drop, reopen, valid zero/max
journals and rejected journal/target collision without mutation.

[StorageObjectKeys.tla](../verification/StorageObjectKeys.tla) explores **441
states / 1,567 edge-prefix replays** through real Store operations. Bounds are
keys zero and one, two writes total, one held snapshot, one compaction and one
reopen. Replay exercises both batch and single-entry records and compares heads,
publications, object enumeration and captured object/revision/data after every
step. Six controls must detect cross-key writes, snapshot relabeling, wrong
object/payload, damaged mapping after compaction and key substitution on recovery.
Artifacts live under `target/verification/storage-object-keys`.

**34 external compiler contracts** reject raw/mixed Store and journal arguments,
object-as-revision mistakes, snapshot coordinate mutation, implicit conversions,
arithmetic and defaults, while preserving `Send`/`Sync`. Source/compiler/lockfile
hashes and case logs are under `target/verification/storage-key-contracts`.
This is bounded safety evidence. Existing recovery/compaction models and fault
tests cover crash barriers and uncertain writes; the new model assumes successful
I/O. External mutation of mapped files still violates the mmap safety contract.

### Publication cursor ownership

```sh
cargo test --locked --test publication_cursor --test history --test storage_keys -- --nocapture
cargo test --locked --test api_contracts publication_cursor_compile_contracts -- --exact --nocapture
cargo test --locked --lib storage:: -- --nocapture
```

`Store::publication_cursor(object, after)` validates a numeric resume position;
`publication_after(&cursor)` checks the issuing Store and current retention and
writer health. `cursor.after()` returns a `Revision`; its `get()` supplies the
numeric checkpoint for explicit import. The cursor's fields are private. Clones retain the same binding
and position, and reads leave the input unchanged. Each `Publication` pairs a
snapshot with the next cursor; `into_parts()` transfers both. The numeric wire
and storage formats stay unchanged. After reopening, the host must explicitly
import a checkpoint again; a retained old cursor returns `ForeignCursor`.
Cursors do not retain file locks, reserve history or grant capability authority.

[PublicationCursor.tla](../verification/PublicationCursor.tla) explores **493
states / 3,218 edge-prefix replays** through actual Store APIs. Two objects have
three revisions each, with revision two always a draft. Object zero can publish
revision three; object one starts there. The graph covers imports from positions
zero through four, one retained cursor, a foreign Store, one compaction, one
reopen, retries and explicit advancement. Replay compares outcomes, owner epochs,
cursor coordinates, snapshot coordinates/bytes, next positions and history floors.
Seven model controls must detect draft imports, ignored owner/retention checks,
implicit advancement, substituted objects, skipped events and wrong next positions.
Artifacts are under `target/verification/publication-cursor`.

Native tests additionally cover empty histories followed by publication, zero
and full-width keys, `u64::MAX` resume rejection, Store moves, all retention modes,
cursor clones, lock release and retained snapshot bytes. Storage fault-injection
tests require both old cursors and new imports to reject a poisoned writer and
old cursors to reject the recovered instance. Existing RPC history tests retain
wire replay, cancellation, revocation and restart coverage.

**25 external compiler contracts** check construction/mutation restrictions,
wrong argument domains, serialization bypasses, paired result ownership,
`must_use`, and `Send`/`Sync`. Source/compiler/lockfile hashes and case logs are
under `target/verification/publication-cursor-contracts`. The model is bounded
safety evidence under successful I/O; it does not prove unbounded refinement,
liveness or authorization. Recovery faults remain covered by the storage fault
suite. Revision counters and durable checkpoints still need explicit host context.

### Revision domains and exhaustion

```sh
cargo test --locked --test revisions -- --nocapture
cargo test --locked --lib storage::revision_tests:: -- --nocapture
cargo test --locked --test api_contracts revision_domain_compile_contracts -- --exact --nocapture
cargo test --locked --test tooling rust_guard_graph_equals_tlc -- --exact --nocapture
```

`storage::Revision` (also exported by `semantics`) separates revision counters
from object keys, authority generations and sizes. Heads, publications, snapshots,
Store update expectations/results, publication cursor positions/history floors,
ORM notifications and durable-upload metadata use the same domain. All 64 bits
are preserved. `INITIAL` denotes zero/no entry; `MAX` is a usable final revision.
`new()` and `get()` define explicit numeric boundaries, including Serde's numeric
encoding. Checked `Revisions` state keeps publication <= head behind private
fields and exposes fallible stage/publish transitions; it cannot be deserialized.

[RevisionBoundary.tla](../verification/RevisionBoundary.tla) explores **965
states / 7,980 edge-prefix replays** against the actual Store and production
revision guards. The primary object starts at `u64::MAX - 2`; a second object
starts absent. Four operations include compare-and-set staging/publication,
two-entry batches, one history-preserving compaction and one reopen. Replay
checks results, both objects' counters, snapshot bytes and byte-for-byte disk
immutability after every rejected operation. Seven model fault controls detect
wrapping, stale stage/publication compares, future/rewound publication, partial
batch writes and revision reuse after maintenance. Artifacts live under
`target/verification/revision-boundary`.

Native tests exercise the final revision under all retention modes, publication
after exhaustion, empty-entry rejection, both batch orders and corrupt appended
single/batch records that try to reuse a revision after `MAX`. Numeric tests
exercise zero, 32-bit boundaries, final counters and malformed JSON, while checked
state tests cross valid/inverted bounds with stale, future and exhausted inputs.
**31 external compiler contracts** reject raw/mixed domains, field mutation,
unchecked arithmetic, defaults, state deserialization and ignored outcomes;
positive controls retain explicit encoding and `Send`/`Sync`. Artifacts are under
`target/verification/revision-domain-contracts`.

The model assumes successful I/O and uses a valid compacted fixture to reach
high counters. It supplies bounded safety evidence, not an arbitrary-size proof
or hardware durability/liveness guarantee. Existing recovery, history, bulk and
persistence models cover their separate failure and authorization conditions.
Revisions remain object-local metadata; a number does not prove entry existence,
publication or owner identity. The existing composed revision/handoff graph is
also checked for exact equality with TLC after the API migration.

### Typed transition state and counter limits

```sh
cargo test --locked --lib semantics::handoff::tests -- --nocapture
cargo test --locked --test authority
cargo test --locked --test tooling rust_guard_graph_equals_tlc -- --exact --nocapture
cargo test --locked --test handoff_auth --test noise_multiparty --test rpc
cargo test --locked --doc semantics::
```

`HandoffState` stores a typed phase with private counters and derives acceptance
from that phase. `NativeStreamGate` stores one explicit state and requires an
initiator/responder role. Five compile-fail cases reject handoff field mutation,
invalid direct-state construction, deserialization, pre-opened gate construction
and boolean role arguments. Native tests cover revocation in every handoff phase,
draining after revocation, incorrect-role preface operations, repeated
authentication notifications and permanent preface failure.

`rust_guard_graph_equals_tlc` compares every projected Rust state and transition
with `Capn't Proto.tla`: **324 states / 927 transitions**, including its safety/liveness
checks and embargo fault control. Model phase numbers are confined to verification
adapters. The existing authenticated-handoff and stream-opening replay drivers
also use the typed production APIs.

[HandoffCounterBoundary.tla](../verification/HandoffCounterBoundary.tla) checks
two scenarios with five operations per history. Proxy mode starts with two
cumulative drain slots remaining; direct mode starts with two invocation-counter
slots remaining. The Rust unit replay seeds private, valid states with the
relevant counter at `u64::MAX - 2`, then executes production transitions and
compares every result, phase, pending/drained/direct count and revocation flag.
Every rejected transition must leave the complete Rust state unchanged. Proxy
mode checks **428 states / 1,652 edge prefixes**; direct mode checks **169 states /
968 edge prefixes**. Five model controls detect admission without drain capacity,
lost drain accounting, premature embargo release, revoked acceptance and wrapping
direct counts. Explicit Rust regressions cover `u64::MAX` pending/direct counters
and successful handoff after the cumulative drain counter is exhausted.

The offset fixtures stand for long, reachable histories; the tests do not execute
`u64::MAX` calls or prove all counter bounds. These checks concern pure transition
policy and preserve the existing transport-authentication boundary. They do not
establish per-method RPC accounting, cryptographic security or complete runtime
refinement. Broader numeric/state audits remain on the roadmap.

### Guard mutation and coverage evidence

```sh
cargo install cargo-mutants --version 27.1.0 --locked
cargo install cargo-llvm-cov --version 0.8.6 --locked
rustup component add llvm-tools-preview
cargo test --locked --test guard_quality -- --nocapture
cargo test --locked --no-default-features --test authority -- --nocapture
```

[guard_quality.rs](../tests/guard_quality.rs) measures `src/authority.rs`,
`src/semantics/handoff.rs` and `src/semantics/stream.rs` using seven native tests
with default features disabled. TLC replay and Unix RPC tests are excluded from
this selection. [authority.rs](../tests/authority.rs) exhausts 256 rights bytes
and all 1,024 valid parent/child rights combinations using a set-based subset
oracle. Forty-eight revocation scenarios vary the watched node, revoked node and
timing before construction, before first poll or after notification registration.
They check actual wakeups, cancellation/replacement, ancestor propagation,
sibling isolation and idempotence. Handoff assertions cover both zero and
nonzero counters and draining two queued calls before lifting the embargo.

The mutation gate uses the release source inventory to make an isolated copy;
`--in-place` applies only to that copy. It requires a successful baseline with
the full selected test inventory, then matches every generated mutant against
its outcome. The 2026-09-25 refresh includes the typed immutable grant getters:
**98 generated mutations: 91 caught by native test failures,
seven unbuildable, zero missed, zero timeouts.** The seven unbuildable replacements
introduce `Default::default()` for `Rights`, `Grant`, `ObjectId`,
`ObjectGeneration` or `NativeStreamState`, which
intentionally lack `Default`. Their exact functions/replacements and compiler
diagnostics are checked separately; they are not counted as test kills. A build
failure or timeout cannot qualify as a caught assertion. The first run's 16
survivors motivated the additional tests above; production behavior was unchanged.
No mutant exclusions or equivalent-mutant allowances are used.

The coverage gate records JSON and HTML using production configuration
(`--no-cfg-coverage`), checks the native test inventory and extracts the three
production files. Handoff unit tests live separately in
[handoff_tests.rs](../src/semantics/handoff_tests.rs). Results on Rust 1.97.0:

| Production file | Executed lines | Executed regions |
|---|---:|---:|
| Authority | 73 / 73 (100%) | 86 / 86 (100%) |
| Handoff guard | 80 / 83 (96.39%) | 92 / 95 (96.84%) |
| Stream guard | 39 / 39 (100%) | 39 / 39 (100%) |

The uncovered handoff region is the defensive rejection when the cumulative
drain counter cannot increment. Public admission reserves that capacity, so
valid pending calls cannot reach the rejection. Its continued presence is not
evidence of an exercised error path. Line/region coverage does not establish
branch or MC/DC coverage; those are not instrumented. These figures describe
the selected tests and files, not the workspace or full runtime.

Evidence is written under `target/verification/rust-mutations/checked.json` and
`target/verification/guard-coverage/checked.json`. Each records the tool/compiler,
source hashes, test inventory, scope and artifact directory. Mutation artifacts
retain individual diffs, logs and build/test outcomes; coverage retains the raw
export, HTML and uncovered segment coordinates. Both gates check source hashes
after execution; mutation also checks that the disposable copy was restored.

[GrantRevocation.tla](../verification/GrantRevocation.tla) models one waiter's
lifecycle over a fixed root/branch/leaf/sibling grant tree. Fresh TLC exploration
is replayed through the actual Rust grants and futures:

| Watched node | States | Edge-prefix replays |
|---|---:|---:|
| Root | 1,197 | 6,424 |
| Branch | 1,188 | 6,286 |
| Leaf | 1,166 | 6,134 |
| Sibling | 1,188 | 6,286 |

The replay observes future readiness and every grant's liveness/GET authority
after each action. Four model faults detect ignoring ancestors, contaminating a
sibling branch, losing preexisting revocation and completing while live. Each
configuration checks eventual observation or cancellation under weakly fair
polling. Actual waker assertions are separate native tests: TLC does not model
Tokio internals, and fairness does not prove production executor progress.
Rights masks are exhaustively tested separately; this model abstracts rights,
authentication, arbitrary tree sizes and multiple simultaneous waiters.

## Local async schedule exploration

```sh
cargo test --locked --test schedules -- --nocapture
cargo test --locked --lib noise_shutdown::schedule_tests::replay_tlc_shutdown_waiter_lifecycle -- --exact --nocapture
cargo test --locked --lib noise_rpc::session::schedule_tests -- --nocapture
cargo test --locked --lib noise_rpc::session::output_tests -- --nocapture
cargo test --locked --lib noise_rpc::session::shutdown_tests -- --nocapture
cargo test --locked --test noise_shutdown --test output_completion -- --nocapture
cargo test --locked --no-default-features --test authority_schedules -- --nocapture
```

Shuttle **0.9.4** is pinned in the development support package; Cargo installs it
from the lockfile. The [shared harness](../test-support/src/schedules.rs) runs
production futures on Shuttle's [local async executor](https://docs.rs/shuttle/0.9.4/shuttle/future/index.html).
Grant `watch` and shutdown `Notify` primitives retain their real implementations.
Actors yield explicitly, and pending futures register Shuttle task wakers.
Synchronous polls/operations are indivisible at this boundary, matching the
component's `Rc`-based, single-thread use. This is not instrumentation inside
Tokio atomics, a weak-memory check, or execution of Tokio's own scheduler.
Route worker fixtures use the private executor boundary in
[session.rs](../src/noise_rpc/session.rs). Production retains Tokio abort handles;
the Shuttle adapter uses `futures::Abortable` to keep abort requests synchronous
and release captures on the next task poll. Shuttle's own abort handle would
insert a scheduling point inside cancellation. Spawn and explicit actor yields
are scheduling points; no actor adds workers to an owner concurrently with stop.

| Scenario | Sampled schedules | Distinct decision sequences | Assertions |
|---|---:|---:|---|
| Shutdown completion | 256 | 254 | Three waiters agree on the first acknowledgement, timeout error or driver-drop result; late observers see the same result. |
| Shutdown cancellation | 256 | 256 | Cancellation before/after registration, completion winning cancellation, replacement/surviving waiters, and distinct receipts for old/new controls. |
| Grant waiters | 256 | 255 | Two leaf waiters and a sibling waiter, cancellation/replacement, branch then root revocation, sibling isolation and denied authority after revocation. |
| Route owners | 256 | 256 | Pending dial failure versus stop/drop, cancellation before/after polling, retained observers, release of both owned workers and independent generation outcomes. |
| Output success | 256 | 256 | Real short writes and ordered serialization, rejection after termination, flush/close/disconnect ordering, three fence waiters with cancellation/replacement. |
| Output errors | 256 | 256 | Write, flush and close failures each race owner stop and starting a drain; local output errors versus canceled-driver results, first cause and drain ownership. |
| Shutdown fences | 256 | 256 | Flush, close and peer outcomes versus deadline readiness; healthy and all three error configurations with independent expected results. |
| Shutdown terminal causes | 256 | 256 | Each fence configuration versus cancellation and prior route failure; forced cancellation after receipt while local flush remains blocked. |

Each scenario uses seed `0x5250534348454431` and fails after 512 scheduling steps;
there is no skip-on-bound or wall-time truncation. Sampling checks that both
cancellation/completion outcomes occur, including cancellation before the first
poll and while registered. Shutdown completion checks all three winning causes.
Every generated schedule replays immediately and compares its complete task
decisions and semantic events. All spawned tasks are joined; lost wakeups cannot
pass by abandoning a blocked observer. The timeout case supplies a terminal
`TimedOut` error; it does not exercise clock expiration. The shutdown-composition
scenarios supply explicit deadline readiness; paused-time Tokio tests separately
exercise production timers before and after initial polling.

The [Cargo gate](../tests/schedules.rs) builds the eight fixtures with only the
`noise` feature enabled, then runs each in **two independent processes** and
requires byte-identical schedules/events. This produces 2,048 sampled executions
per process, each with an exact replay: **8,192 executions** across both processes.
It also replays one saved execution per case. Full decision sequences contain
some repeated samples; these are neither exhaustive schedules nor state counts.
The gate requires at least 200 distinct sequences per case.

Negative controls deliberately discard an executor waker after registering with
the real shutdown control or a route's terminal notification. The latter already
has a receipt but remains blocked on local flush when the route is canceled.
Shuttle must report a deadlock; replaying each saved failure must reproduce it.
These fixtures are ignored in ordinary library runs because their required
result is failure; the Cargo gate executes them explicitly. Per-case controls also truncate a valid decision sequence
or corrupt an expected event and require the specific replay failure. They are
harness sensitivity checks, not mutations of production source.

`target/verification/async-schedules/checked.json` records the source inputs,
compiler, binary hashes, seed, configuration, scenario names, distinct counts
and artifact directory. Each case retains versioned `runs.json` with its name,
per-run seed, task decisions and semantic events. Failed sampling retains its
partial events and Shuttle's native failure file. Replay inputs bind the case
name and reject unknown fields. For example, use the gate's standalone artifact:

```sh
REPROTO_SCHEDULE_REPLAY=/path/to/grant-waiters-single.json \
cargo test --locked --no-default-features --features noise --test authority_schedules -- --nocapture
```

Saved replay bypasses sampling and aggregate diversity checks, while retaining
every per-execution assertion and exact decision/event comparison. Normal runs
reject `SHUTTLE_RANDOM_SEED` overrides; the gate clears inherited Shuttle settings.

[ShutdownWaiters.tla](../verification/ShutdownWaiters.tla) complements schedule
sampling with **3,874 states / 24,752 edge-prefix replays**. Two waiters observe
one control and a third observes another control. The model includes creation,
polling, one cancellation/replacement, completion before registration and repeated
late completion. Rust replay compares future readiness, terminal receipts/error
kinds and every actual wake flag after each operation. Four controls reject
overwriting the first result, waking only one observer, cancelling another waiter
and completing the wrong generation. Weakly fair polling establishes eventual
observation or cancellation after completion. It abstracts notification internals,
arbitrary waiter counts, timer/IO progress and full route ownership.

[RouteTaskOwner.tla](../verification/RouteTaskOwner.tla) explores **482 states /
2,008 edge-prefix replays**. One old owner has a pending selection and a second
pending worker; after stop a replacement may begin before old captures are
released. The replay drives the actual owner and pending-selection code with a
manually polled executor, checking abort flags, task results, released captures,
observer terminal causes and replacement isolation after every transition.
Five model controls detect an owner retained by its observer, a missed worker,
first-cause overwrite, cancellation of the replacement and running canceled work.
Eventual release assumes weakly fair polling, not synchronous cleanup by stop.
The native Tokio test checks stop/drop before and after initial polling, release
of both workers with an observer retained, and replacement while cancellation
is still pending. The Shuttle test adds competing actors and exact replay.

The pending-owner fixture supplies pending/error selections and an inert sibling
worker. The new output fixtures instead drive the actual serializer/write queue,
two-party driver, shared output fence and route watcher. An `AsyncWrite` adapter
controls write/flush/close readiness and failures; input access fails the test.
The watcher uses an explicit producer-first `select!` order, avoiding hidden
randomness. Two ordered Bootstrap envelopes use short writes; a prebuilt message submitted
after termination must fail. Three fence waiters include cancellation/replacement.
The error scenario runs all three failure stages in each sampled execution and
requires both failure/cancellation winners and preserved drain ownership at each
stage. The fixture initializes only lifecycle policy, not an authenticated session.
The native Noise shutdown tests separately cover real queued-byte receipts and
crossed shutdown. Their backpressure payloads use valid Call envelopes so they
also pass through the RPC reader's envelope classification.

[RouteOutputFence.tla](../verification/RouteOutputFence.tla) checks the same
writer/fence boundary with a manually polled executor. Four configurations check
**1,843 states / 4,803 edge-prefix replays**: success (480/1,230), write failure
(425/1,143), flush failure (461/1,209), close failure (477/1,221).
Every transition compares write/flush/close progress, queue-termination and output
fence results, route status/cause, abort flags and retained connection handles.
Five controls require failures for premature publication, skipping flush, missed
cancellation, overwritten causes and a writer taking over an active drain.
Progress assumes weakly fair polling and either all I/O gates ready or owner stop;
it does not require connection handles to disappear for the output fence to resolve.
The direct bounded test also holds those handles until after fence completion.

`Handle::shutdown()` uses the production completion helper exercised by
[shutdown_tests.rs](../src/noise_rpc/session/shutdown_tests.rs). Queue flush,
output closure and peer outcome are observed in that order. A previously
committed route failure/cancellation wins even over a ready receipt; otherwise
a ready fence result wins simultaneous expiration. A deadline bounds any chain
still pending. A route terminal notification independently wakes blocked drains.
This fixes a reproduced canceled-route/successful-receipt disagreement and avoids
waiting for local I/O to wake a drain after cancellation. The real-writer fixture
also checks that an injected early receipt cannot bypass output close/failure.

[RouteShutdownCompletion.tla](../verification/RouteShutdownCompletion.tla) checks
**2,087 states / 8,368 edge-prefix replays**: healthy (510/2,056), flush failure
(545/2,167), close failure (522/2,089), peer failure (510/2,056). Replay compares
completion codes, structured route causes/status and first control outcomes after
every input/poll. Six fault controls reject skipped fences, ignored terminal
causes, premature deadline precedence and cause overwrite. Progress assumes
weakly fair polling and an available result/deadline/terminal transition.
Shuttle samples every mode, checks each completion against an independent oracle,
and joins all tasks. A forced blocked-cancellation case tests the actual wakeup;
the model assumes polling fairness rather than proving notification internals.
The native route-replacement regression both drops and resumes an old shutdown
future, preserving its cancellation cause and the replacement's route entry.

Authenticated installation, connector admission/arbitration, route-table
replacement under Shuttle, packet/transport driver scheduling, clock/socket
integration and schedule minimization remain on the roadmap. Production Tokio
spawning and abort behavior are unchanged.

### Shutdown receipt packets and peer close

```sh
cargo test --locked --lib transport::shutdown:: -- --nocapture
cargo test --locked --test noise_shutdown --test output_completion --test noise_arbitration --test noise_listener
```

[NoiseShutdownPacketFence.tla](../verification/NoiseShutdownPacketFence.tla)
checks **489 states / 737 graph-edge prefixes**: ordinary receipt (18/17), crossed
receipt (124/193), insufficient reciprocal-write credit (124/193), invalid nonce
(99/141), reset reciprocal stream (124/193). Every prefix replays through the real
`ShutdownDriver` over encrypted quiche streams in both IK and IKpsk2:
**1,474 native replays**. The fixture compares frame offsets, accepted
receipt/request state, delivered input bytes, complete
reply writes, close state and sticky control outcomes after every action. Every
delivered datagram is duplicated. Seven required model controls detect skipped FIN,
nonce binding failure, missing crossed fences, incomplete/reset reply writes,
unrelated close and overwritten failure. Weakly fair actions with valid receipts, sufficient
credit and a cooperative peer establish success; other configurations establish
termination, which may be an error.

The reproduced implementation bug accepted peer close after a crossed receipt
even without the reciprocal request. A second regression reproduced success after
resetting the queued reciprocal stream. Production completion now requires the
authenticated graceful application code and all local crossed-exchange fences;
the reply stream must still be valid in quiche. Only the final transport ACK may
be replaced by that graceful peer signal. Native regressions also reject
byte-count/role/version/length errors, missing FIN, stream
reset, wrong application/transport close codes, damaged packet tags and old
ciphertext after reconnect using the same static identities, addresses and CIDs.
Decryption rejection is observed through accepted-packet counters and unchanged
stream/protocol state: quiche's `recv()` may successfully consume a datagram while
silently discarding its unauthenticated packet. The outer transport driver is
tested with actual encrypted close state and paused Tokio deadline expiration.

Bounds are one locally drained 41-byte stream, one two-fragment receipt, one
reciprocal request for two bytes, fixed reply credit and one close/failure,
optionally resetting the reply immediately before close. The peer is scripted,
including malformed authenticated messages; received stream
bytes use an explicit test consumer instead of the runtime duplex bridge. Its
graceful close assertion is trusted only after checking the local fences. This
does not prove a malicious peer's delivery claim. Cryptography, entropy and the
packet engine's clock are real; replay is semantic, not byte-identical ciphertext
or deterministic whole-runtime scheduling. Saved fresh TLC graphs and controls
are under `target/verification/noise-shutdown-packet-fence`.

## Deterministic Noise packet simulation

```sh
cargo test --locked --test tooling quiche_noise_packet_simulation -- --exact --nocapture
cargo test --locked --test tooling quiche_noise_recovery_model -- --exact --nocapture
cargo test --locked --test tooling quiche_noise_lifecycle_model -- --exact --nocapture
```

The [simulator](../vendor/quiche/quiche/src/simulation/packet/mod.rs) drives real quiche
connections with a private test-only clock/entropy scope. Production keeps
`std::time::Instant` and secure randomness; no Cargo feature exposes deterministic
keys. The test clock covers packet processing, ACK delays, pacing, timeouts and
Cubic/BBR2 recovery initialization. Snow's test builder hook supplies seeded
ephemeral keys while retaining the actual DH, hashes, AEAD and packet protection.
The scope is thread-local, cannot move between threads, and restores normal
clock/entropy behavior on unwind.

Sixteen loss schedules combine seeds `0`, `1`, `42`, `3735928559`, both congestion
controllers, and IK/IKpsk2. Each drops the first initiator flight, first responder
flight and an application packet, then introduces corruption, duplication and
seeded reordering. Every received stream fragment must match the expected prefix;
FIN requires the complete payload and cannot be delivered twice. The request is
4,096 bytes and the response 1,536 bytes on one bidirectional stream. Receive
credit is 131,072 bytes per connection and 65,536 bytes per stream, active CID
limit is three, idle timeout 10 seconds, output buffers 1,350 bytes and propagation
delay 1 ms. Delivery also
respects each packet's pacing deadline. Runs have bounded events/queues and use
virtual time without sleeping.

Sixteen [lifecycle schedules](../vendor/quiche/quiche/src/simulation/packet/lifecycle.rs)
use the same seeds/controllers/patterns and actual quiche send addresses. A partition
discards the first alternate-path challenge and a timer-driven retry, then heals.
Corrupted response copies must leave path state unchanged. Both peers finish path
validation before the client migrates; blocking the original path still permits
a full request/response on the alternate path. The server connection is replaced,
then the client reconnects on that address with fresh Noise ephemeral keys and the
same identities/PSK. Both deliberately reuse their active source CIDs. The driver
checks that saved old STREAM packets still address the fresh receivers, replays
them in both directions, and requires unchanged application data and open sessions.
A second request/response uses different generation-specific bytes on stream zero.
There is no generation-based packet filtering in the delivery adapter.

The Cargo gate runs all **32 fixed schedules** and **128 generated property cases**
in two independent processes. Fixed schedules require identical serialized inputs,
packet bytes, read effects, timer deadlines and observations; generated cases
compare SHA-256 hashes of that complete record after exact in-process replay.
It retains per-case JSON files and a generated-input manifest under
`target/verification/noise-simulation/`, plus a
`checked.json` containing toolchain, fork metadata and the actual test executable's
SHA-256. Reproduction is qualified for that build/configuration, not all platforms.
Failed direct unit runs save their partial event log under the system temporary
directory's `reproto-noise-simulation/` directory. Filenames distinguish loss and
lifecycle scenarios. Replay a saved case from either family with:

```sh
REPROTO_NOISE_SIM_REPLAY=/absolute/path/to/case.json cargo test --locked \
  --manifest-path vendor/quiche/Cargo.toml -p quiche --no-default-features --features noise \
  --lib simulation::packet::seeded_packet_replay -- --exact --nocapture
```

Packet reports use format 3 and record the required completion generation. A
complete old stream cannot make a truncated reconnect scenario pass. TLC edge
prefixes remain explicitly partial reports with no terminal completion obligation.

The [property driver](../vendor/quiche/quiche/src/simulation/packet/property.rs) pins
Proptest 1.11.0 as a test-only dependency. It generates sixteen cases in each of
eight configurations: Cubic/BBR2, IK/IKpsk2, and fault windows before/after a real
confirmed handshake. Every case varies the entropy seed and up to twelve delivery
decisions. A decision chooses a current queue index modulo queue length, adds
0–3 ms to the packet's earliest arrival, then delivers, drops, corrupts, duplicates,
or delivers a corrupted copy followed by the original. At most two decisions may
destroy an original packet. Once the finite input ends, delivery is reliable and
real recovery timers are serviced. Unused decisions after completion are reported
as unapplied; treatment counts reflect executed decisions, not requested inputs.
The Cargo gate requires all five treatments in every configuration.

Each receive must match the independent expected payload prefix, FIN requires all
bytes exactly once, and both streams must complete with open authenticated peers
within the existing 400-round/4,096-event, 256-packet and ten-second virtual-time
bounds. These are bounded recovery assertions, not unconditional liveness under
arbitrary loss. Corruption currently flips the last datagram byte; this is not
arbitrary parser-input fuzzing. No cryptographic checks are disabled.

Proptest shrinks seeds, decision sequences, selectors and delays, up to 1,024
iterations. Queue-relative selectors avoid stale packet IDs after shrinking.
Failures save the reduced JSON case to the temporary `reproto-noise-properties/`
directory (override with `REPROTO_NOISE_PROPERTY_FAILURE_DIR`), and rerun it once
to retain the final packet log. A synthetic failing predicate checks that shrinking
reduces to two necessary decisions; those decisions must pass the actual transport
oracle in all eight configurations. This control tests the shrink machinery, not
Rust mutation coverage or global minimality.

Replay a property input through Cargo:

```sh
REPROTO_NOISE_PROPERTY_REPLAY=/absolute/path/to/case.json cargo test --locked \
  --manifest-path vendor/quiche/Cargo.toml -p quiche --no-default-features --features noise \
  --lib simulation::packet::property::generated_packet_properties -- --exact --nocapture
```

The Cargo gate exercises this disk entry point and rejects unsupported formats;
it retains a valid `target/verification/noise-simulation/property-replay.json`
example. `REPROTO_NOISE_PROPERTY_CASES` changes the standalone run's per-configuration
case count (1–4,096); the cross-process gate fixes it at sixteen. After fixing a
discovered bug, add the reduced case to
[property-corpus.json](../vendor/quiche/quiche/src/simulation/packet/property-corpus.json),
which runs through the same oracle and replay in ordinary library tests. Its current
two entries are seed cases, not claims of previously discovered protocol defects.

[NoiseHandshakeRecovery.tla](../verification/NoiseHandshakeRecovery.tla) explores
**17 states and 16 edge prefixes**, each replayed under both patterns/controllers
for **64 native traces**. Four model faults must violate authentication, Initial-key
retention or address-confirmation invariants. Liveness assumes eventual timer
service and delivery with at most one lost initiator and one lost responder flight.
Retry actions abstract bounded local PTO handling until CRYPTO is retransmitted;
an ACK-only Initial does not qualify. Final confirmation settles HANDSHAKE_DONE
recovery and client confirmation with no further loss. Every internal packet and
timer action still passes through the simulator. Flight actions are atomic in TLA+;
the separate seeded schedules exercise packet-level ordering and application loss.

[NoisePathLifecycle.tla](../verification/NoisePathLifecycle.tla) explores **44 states
and 43 edge prefixes**, producing **172 native replays** across both controllers
and Noise patterns. The observed projection covers establishment, connection
generations, old-generation alternate-path validation, active client address,
completed streams and blocked routes. All generated traces check actual Rust state
after every modeled action. Six model faults cover timeout/forgery validating a
path, missing migration, authentication or stream state surviving restart, and
stale ciphertext delivering data. Optional partition/forgery branches and replay
both before and after fresh stream completion are explored. Liveness assumes
eventual delivery/timer service after the bounded partition; flight/stream actions
settle bounded internal packet schedules rather than modeling each packet in TLC.

This scope excludes the RPC executor, handoff/token entropy, OS sockets, arbitrary
partition/restart histories, qlog wall-clock output and lifecycle-schedule
shrinking. Restart means replacing connection objects, not crashing an OS process
or its listener/router. Old Initial packets and replay before the fresh handshake
are outside the lifecycle model. It does not establish cryptographic security or
exactly-once RPC execution across reconnects. The production UDP and handoff tests remain
separate checks.

## Noise fuzzing with real cryptography

The [first-party fuzz package](../fuzz/Cargo.toml) has six libFuzzer targets and a
[shared native harness](../fuzz/src/lib.rs), covering serialization, schema loading
and RPC lifecycles as well as Noise.
It enables only quiche's `noise`
feature. The inherited `vendor/quiche/fuzz` package enables a crypto bypass and remains
a separate TLS/parser harness. Combining Noise with that bypass is a compile error;
the new Cargo gate requires that rejection.

On x86-64 Linux, install the pinned tools and run the complete bounded gate:

```sh
rustup toolchain install nightly-2026-08-29 --profile minimal
cargo install cargo-fuzz --version 0.13.1 --locked
cargo test --locked --test noise_fuzz -- --nocapture
```

For a longer serialization/schema/RPC campaign through the same gate, increase its budget
(values below 16,384 are rejected):

```sh
REPROTO_FUZZ_WIRE_RUNS=262144 cargo test --locked --test noise_fuzz -- --nocapture
```

The gate runs thirteen native tests and creates the following independent corpora:

| Target | Initial seeds | Minimum ASan trials |
| --- | ---: | ---: |
| `noise_packet` | 131 | 1,024 |
| `noise_stream` | 258 | 1,024 |
| `capnp_framing` | 1,478 | 16,384 |
| `capnp_pointers` | 1,754 | 16,384 |
| `capnp_schema` | 13,321 | 16,384 |
| `rpc_lifecycle` | 592 | 16,384 |

All six campaigns use coverage feedback,
a 4,096-byte input cap, ten-second per-input timeout, 1,024 MiB RSS limit and 128 MiB
allocation limit. `--no-cfg-fuzzing` keeps normal dependency behavior as well as
real packet authentication. A negative control requires tampered Short packets
to deliver no bytes, then requires their original ciphertext to deliver the full
message. Each campaign must finish its budget, report instrumented coverage and
produce no crash artifacts. This is a smoke budget, not a sustained fuzz campaign.
LibFuzzer can exceed the requested trial count while initializing/retesting a
large seed corpus. The gate requires at least the budget, cross-checks the final
execution statistics, and records both requested and actual counts.
Tool background: [cargo-fuzz setup](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html)
and [tutorial](https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html).

`noise_packet` selects IK/IKpsk2, Cubic/BBR2, the sender direction and pre/post-handshake
state. It obtains an actual protected datagram, then replaces it with raw bytes,
XORs selected bytes, truncates it or appends bytes. The candidate is delivered twice.
Malformed-input errors and connection closure are allowed; any application read
must match the sole legitimate message and cannot duplicate a completed stream.
Appending input can produce a datagram larger than the input cap (at most 5,443
bytes). The pre-handshake client-receive case first delivers the real initiator
flight so mutations reach the responder's flight.

`noise_stream` first establishes a real authenticated connection. It sends up to
4,094 payload bytes on stream zero, using writes of 16–271 bytes and reads of
1–256 bytes. Input flags reverse outgoing batches, duplicate datagrams or inject
bad-tag copies before originals. Reordering batches writes, and a maximum-size
seed must reverse multiple datagrams to verify that the scenario is exercised.
Reads must equal successive payload prefixes; FIN requires the full payload
once, and both peers must remain established and
open. Setup is bounded to 32 delivery rounds, send loops to 64 packets, and stream
transfer to 300 rounds. Original packets remain available: these checks do not
exercise timer-driven loss recovery. They also do not construct arbitrary
authenticated malformed frames; most byte mutations are rejected by protection.

`capnp_framing` interprets its first two bytes as the packed bit and maximum
fragment size minus one (1–256 bytes), followed by arbitrary stream bytes. It
compares allocating and caller-buffer readers with whole/fragmented `Read` and
`BufRead`, checking up to two consecutive messages, exact consumed bytes and
segment contents. For unpacked input, a separate checked-slice implementation
supplies the expected framing and checks both flat-slice readers too. Packed
input uses fragmentation/differential properties, with independently encoded
valid fixtures. The seeds include run boundaries, partial tables/bodies, empty
segments, 510/511-segment tables and concatenated messages. Scratch memory is
explicitly aligned and prefilled to expose stale bytes. No production writer
constructs these fixtures.

`capnp_pointers` uses a four-byte header: operation (`raw`, XOR, overwrite or
truncate, modulo four), fixture index, and a little-endian mutation offset. The
remaining bytes are a raw framed message or mutation bytes. The 23 independent
fixtures include structs, every list encoding, far/double-far pointers, cycles,
invalid sizes and capability indices. Fresh readers isolate traversal accounting
between sizing, schema-free views, structural equality, deep copy and
canonicalization. Successful comparisons must preserve reflexivity/copy contents;
canonical output must be canonical and idempotent. Reader errors are allowed for
malformed or over-budget inputs. Pointer walks inspect bounded prefixes (16
elements/fields, at most 64 visited pointers). Both serialization targets impose
a 512-word traversal/framing limit and nesting limit 16; framing rejects 512 or
more segments. No capability table is installed: extraction must fail even for
nested indices. This checks that bytes alone do not grant authority, not live
capability ownership or RPC dispatch.

The framing seed matrix found a real packed-reader mismatch: the four-byte input
`01 03 00 01` encodes four empty segments, with a zero run spanning two remaining
table words. The caller-buffer reader previously split that run and rejected the
message. It now reads the remaining table as one block, matching the allocating
reader. [Native regressions](../tests/serialization_reads.rs) cover fragmented
packed input, truncation and the next message; framing seed 1 retains the same
case. Deliberately wrong boundaries/segment contents are negative controls for
the independent oracle. Capability and cycle fixtures have explicit expected
outcomes in native tests.

`capnp_schema` uses a five-byte header: operation (`raw`, XOR, overwrite or
truncate, modulo four), fixture index, little-endian mutation offset, and limit
profile. Remaining bytes contain a framed `CodeGeneratorRequest` or mutation
bytes. Its [38 named fixtures](../fuzz/src/schema.rs) have explicit acceptance or
rejection expectations for layouts, names/orders, ordinals, unions, defaults,
dependencies, inheritance/group cycles, brands, future tags and nested list types.
The generated messages use schema builders; their semantic expectations do not
come from a round trip. The corpus also sweeps word-boundary corruptions and
truncations, every fixture in eight resource profiles, and maximum-size input.

Each attempt starts with a real struct definition and a typed dependency stub.
Fixtures begin with a valid upgrade before the candidate node. On any rejected
request, canonical snapshots must preserve every prior definition, the initial
stub and the total entry count; a subsequent valid upgrade must still succeed.
Successful requests undergo bounded reflection and default reads after the input
reader is dropped. Repeating the request must succeed and retain the selected
definitions. A positive control requires the snapshot oracle to detect an actual
upgrade and stub completion. Reflection errors for lazily resolved metadata are
allowed; fields and methods are inspected only up to the first 16 per definition.
This exercises a bounded transaction/replay sequence, not general stateful schema
evolution or arbitrary dynamic message contents.

Schema readers allow 32,768 traversal words and nesting 128. Loader profiles
select 2 or 16 nodes, 128 or 4,096 total words, and 64 or 1,024 words per node.
Positive controls require node, per-node-word and cumulative-word rejections.
Nested list fixtures distinguish the loader's type-depth boundary (63 nested
lists accepted, 64 rejected). These bounds are separate from the framing/pointer
targets' smaller reader limits.

The schema seed sweep exposed a reload inconsistency: an unknown field type
with an unknown default tag was accepted initially, then rejected by compatibility
checking. Matching future types now keep defaults opaque during replay, consistent
with initial validation. The six-byte input `01 23 98 02 00 ff` is retained as
schema seed 2 and an explicit native acceptance regression. A
[root loader test](../tests/schema_loader.rs) also checks repeated loads, dynamic
inspection as `Unknown`, rejection of a changed future type, and rejection of an
unknown default for a known type. Run it with
`cargo test --locked --test schema_loader future_type`.

`rpc_lifecycle` uses a structured command grammar over the actual two-party RPC
runtime. It creates a bootstrap and two separately tracked object capabilities,
then executes at most 32 commands. The first three bytes select an I/O profile,
terminal action (modulo eight), and command count (modulo 33). Each four-byte
command selects a kind (modulo nine), a state-relative object/pending-call index,
and a little-endian `u16` value. Missing command bytes are zero; remaining bytes
mutate only the terminal frame. Inputs are capped at 4,096 bytes.

| Command | Operation and oracle |
| --- | --- |
| 0 | Create a replacement after release, or re-export a live capability. |
| 1 | Call the selected capability; check the exact recipient, argument and returned value. |
| 2 | Retain a capability through a returned reference; require the same ID while its export is live. |
| 3 | Release zero through all outstanding references; check object destruction. |
| 4 | Start one of at most four concurrent calls gated by explicit oneshots. |
| 5 | Complete a selected call; check its value, returned capability and released answer ID. |
| 6 | Cancel a selected call; require one Canceled Return and destruction of its completion receiver. |
| 7 | Run a two-child promised-answer pipeline; check successful dispatch, early parent Finish, cancellation of one/both children, parent failure or an invalid capability-field transform. |
| 8 | Export a gated `senderPromise`; check successful/failed outgoing Resolve, cancellation of one queued call, release before use, release with queued calls, or release after canceling both calls. |

Commands 7 and 8 are bounded compound histories. Selector bit zero chooses the
object; `(selector / 2) % 6` chooses the history. They use questions 8–10 while
up to four ordinary pending calls remain active on questions 1–4. Each compound
history settles before the next command, so deletion/shrinking preserves its
setup; this is not arbitrary interleaving of pipeline or promise operations.
The two queued echo values are the command value plus 9 and 10. The wire oracle
requires exactly one expected Return per live child and rejects extra messages.

Pipelines cannot dispatch before their gated parent completes. A surviving child
keeps its parent running after Finish, with a Canceled parent Return and successful
child Returns; canceling both children and the parent must destroy the gate.
Failed parents and missing pointer fields must produce child exceptions without
dispatch. Successful results retain the expected object export. Exported promises
must emit exactly one Resolve for their own ID while exported, carrying either
the expected hosted capability or an exception. The original promise ID remains
callable after resolution, and its reference is released separately from any
capability carried by Resolve. Releasing the promise before resolution suppresses
Resolve; queued calls must still complete, while releasing it after canceling
all calls must destroy the resolver. Promise futures and their captured owners
must be gone when each compound history finishes.

An independent model tracks remote reference counts, object generations, expected
calls, pending tokens and completions. It compares those expectations with actual
server dispatch and destruction after each command. Pending calls must keep their
object alive after its last exported reference is released. Later results can
re-export it using an available wire ID; the model does not assume retired IDs are
globally unique. Question IDs are reused only after Return and Finish.

The terminal action selects orderly cleanup, over-release, an unknown export,
a duplicate active question ID, an unallocated ordinary Return ID, raw bytes,
a truncated Call, or XOR mutations of a valid Call. The four explicit protocol
faults must Abort without application dispatch. Truncated Calls must not dispatch.
Arbitrary terminal bytes may produce errors, closure or valid operations, so only
valid outgoing framing and complete teardown are required afterward. Every trial
must drop all service objects, pending application futures and completion receivers.

The fixture provides 1-, 7-, 64- or 4,096-byte read/write chunks to the production
two-party reader and outgoing queue. It drives the real RPC task for at most 64
polls per step; externally pending work uses explicit I/O wakeups and oneshots.
There are no OS sockets, sleeps or background executor threads; the queue's clock
is fixed. Input/output buffers are capped at 256 KiB, and reader limits are 4,096
traversal words and nesting 32. These are bounded fixture schedules, not general
executor exploration or a termination proof for every runtime input.

The 592 seeds cross eighteen lifecycle histories with all four I/O profiles and
eight endings, plus empty, mixed pipeline/promise replay, maximum-length command
and maximum-byte inputs, plus four raw duplicate-promise-token regressions.
A native control requires duplicate tokens to return errors without replacing
the fixture's original resolver; raw terminal seeds also check framing/teardown.
Saved seed 1 exercises all twelve pipeline/promise modes
while an ordinary call remains pending, then cancels that call.
Native tests require every command, ending and actual lifecycle effect, compare
repeated observations, and execute command deletion and byte truncation cases.
Negative controls must detect wrong reference ownership, dispatch history and
pending-call observations. Run only those controls with
`cargo test --locked --manifest-path fuzz/Cargo.toml --no-default-features --lib rpc_lifecycle::tests`.
The same inputs use the shared saved-input replay entry point and libFuzzer's
artifact/minimization flow. Six additional controls corrupt real observed output
and require rejection of wrong/duplicate child results, wrong parent completion,
wrong Resolve IDs/capabilities and success replacing a rejected promise. A
metamorphic test requires identical observations across all four fragmentation
profiles. This RPC fixture asserts no peer authentication; it is separate from
the real-cryptography Noise fixtures. Incoming Resolve, nested/chained promises,
general promise schedules, handoff and authenticated transport/RPC composition
remain outside its scope.

The Noise harness retains production OS entropy and wall-clock timing. Replaying an
input repeats its actions and assertions, but does not guarantee identical
ciphertext, coverage counts or reproduction of an entropy/timing-dependent failure.
The deterministic simulator above supplies the separate clock/entropy-controlled
boundary. ASan results cover executed, instrumented code; they do not establish
soundness of every unsafe abstraction or comprehensive coverage of native assembly,
the standard library, every schema/RPC operation, storage or three-party handoff.

Logs, isolated evolving corpora, seed hashes, executable hashes, Rust/schema
compiler identities
and source provenance (including the vendored decoder, RPC runtime, async framing
and schema generator) are retained under
`target/verification/noise-fuzz/`.
`checked.json` points to the latest completed run and records libFuzzer's `cov`/`ft`
feedback; these counts are **not a source coverage percentage**. The report's
format 3 records actual trial totals separately from requested budgets.
Cargo-fuzz 0.13.1 has no `--locked` option, so the gate checks the lockfile is
unchanged afterward.
The gate removes stale success before starting and rejects source changes during
qualification. The source archive includes the
package/lockfile and Rust seed matrices, and excludes
generated `fuzz/corpus`, `fuzz/artifacts` and build directories.

The 2026-09-26 extended run completed **526,336 trials** (262,144 per serialization
target and 1,024 per Noise target), with no crash artifacts after the packed-table
fix. The report records seven native tests, all four saved-seed replays and the
crypto-bypass rejection. This single seeded campaign does not complete the open
RPC/handoff or storage-recovery fuzzing roadmap items. The schema target added
after that campaign has its own bounded smoke run; the older campaign does not
qualify it.

The subsequent schema qualification completed **51,200 trials** (16,384 per
framing/pointer/schema target and 1,024 per Noise target), all ten native tests,
five saved-seed replays and the crypto-bypass rejection with no crash artifacts.
The 37 root schema-loader, schema-identity, reflection-lookup and field-presence
tests also passed, including their existing C++ comparisons and model replays.
These results qualify the documented bounds; they do not establish complete
malformed-data coverage.

The subsequent structured RPC qualification completed **67,584 trials**, including
**16,384 RPC lifecycle trials**, with no crash artifacts. All thirteen native
tests, six saved-seed replays, source/lockfile checks, formatting/lints and the
crypto-bypass rejection passed. This is a bounded seeded campaign, not full
RPC or handoff qualification.

The pipeline/exported-promise extension then passed **68,361 trials**, including
**16,384 RPC trials**, with no crash artifacts. The schema target executed
17,161 trials because corpus initialization exceeded its requested 16,384;
the other targets completed their requested budgets. All sixteen native tests,
six saved-input replays, source/lockfile checks, formatting/lints and crypto-bypass
rejection passed. This run used x86-64 Linux outside the process-tracing sandbox:
LeakSanitizer cannot complete its final check under that sandbox, and its settings
were preserved for the successful rerun. The compound-history bounds above remain
the qualification scope.

Replay a saved seed or crash through ordinary Cargo tests:

```sh
REPROTO_NOISE_FUZZ_TARGET=noise_packet \
REPROTO_NOISE_FUZZ_REPLAY=/absolute/path/to/input \
  cargo test --locked --manifest-path fuzz/Cargo.toml --no-default-features \
  --lib tests::replay_saved_fuzz_input -- --exact --nocapture
```

Use `noise_stream`, `capnp_framing`, `capnp_pointers`, `capnp_schema` or
`rpc_lifecycle` for their respective inputs.
The replay test accepts all six formats unchanged. Preserve a discovered failure as a versioned
fixture and add it to the shared native harness after fixing its cause; generated
artifact directories are intentionally excluded from distribution. Run or minimize
inputs with cargo-fuzz using the same nightly, sanitizer and `--no-cfg-fuzzing`
settings. No cryptographic review or long-running campaign is implied by this gate.

## Serialization and ownership under Miri

The [Cargo gate](../tests/memory_safety.rs) runs the isolated
[memory-check package](../verification/miri/Cargo.toml). It compiles the production
field API fixture with the vendored generator, without linking the root transport
or storage dependencies into interpreted tests. Prerequisites are the usual
`capnp` schema compiler and this pinned Rust toolchain:

```sh
rustup toolchain install nightly-2026-08-29 --profile minimal --component miri,rust-src
cargo +nightly-2026-08-29 miri setup
cargo test --locked --test memory_safety -- --nocapture
```

The gate requires **23 tests** in each run: nine ownership tests, six reused
vendored wire regressions, four orphan-type tests, two generated group/list tests
and two schema-loading tests. It executes both default aligned and `unaligned`
features natively (**46 executions**) and under Miri with strict provenance:
Stacked Borrows with seed 0 and Tree Borrows with seed 1 (**92 executions**).
The interpreted target is `x86_64-unknown-linux-gnu`. Isolation, borrow checking,
validity checking and leak detection remain enabled. A separate intentionally
invalid freed-pointer read must fail with Miri's use-after-free diagnostic;
ordinary test runs leave this negative control ignored.

Cases cover moving/extracting checked readers with inline segment providers,
far pointers and retained capabilities; shared traversal limits and lazy descendant
validation; cross-arena orphan rejection and capability release; data-orphan
shrink/grow with neighboring allocations; immutable external heap ownership;
staged failure, unwinding and publication; scratch arena reuse; deliberately
misaligned frames; and malformed/zero-sized wire cases. The inline ownership
regression exposed `Box` move retagging and lifetime-extended references protecting
an allocation during extraction. `CheckedStructReader` now uses a private `Rc`
and reference-free root metadata, creating arena borrows only for live views.

The expanded cases check retained contents and zeroed growth for void, bool,
signed/unsigned integers, floats and enums; nested pointer children and text
terminators across resize; capability-list truncation with an extracted client;
and struct-list fields unknown to the declared schema. Both ordinary and
one-word initial segments are exercised. Generated list upgrades must preserve
existing values and leave the source untouched; staged groups must preserve
sibling bits, text addresses and capabilities on success, error and unwinding.
Runtime-loaded schemas retain their copied definitions after input destruction,
isolate cloned loaders during compatible replacement, reject incompatible
batches and enforce node limits. Loaded orphan tests check failed adoption,
distinct loader authority, typed resizing and explicit native registration.

The gate compares native/Miri test inventories, runs package formatting and Clippy,
and writes compiler/interpreter/schema-compiler identities, flags, source hashes
and logs under `target/verification/memory-safety/`. `checked.json` identifies the
latest completed run (report format 2 includes suite names and counts). Source
hashes are taken before testing and must still match before publishing evidence.
The source archive includes this package and its lockfile;
generated build outputs are excluded.

For a focused rerun:

```sh
MIRIFLAGS='-Zmiri-strict-provenance -Zmiri-seed=0' \
  cargo +nightly-2026-08-29 miri test --locked \
  --manifest-path verification/miri/Cargo.toml --test ownership \
  checked_inline_segments_survive_owner_moves_and_extraction -- --exact
```

This is a bounded executed-path check, not a soundness proof. It does not cover
mmap or concurrent external file mutation, descriptor/FFI paths, networking,
cryptographic integration, RPC scheduling, every schema/orphan operation or other
platforms. The isolated package uses local RPC clients only to exercise capability
hook ownership. See [Miri's documented scope and flags](https://github.com/rust-lang/miri).
Existing TLC ownership replays independently check protocol/resource behavior;
they do not model Rust pointer provenance or aliasing.

## Runtime and serialization selections

`twoparty_facade` checks owned and scoped borrowed connections, real TCP listener
cancellation, drain timing, disconnect observers, reverse bootstrap and callable
capabilities, partial IO, large responses and failure isolation. Fresh
`verification/TwoPartyFacade.tla` traces compare five-action lifecycles against
Rust and pinned C++: one owned and one borrowed connection, one call per
connection, one drain observer. Socket/TLC execution needs loopback permission.
The model settles IO between actions; it does not enumerate executor schedules.

`flow_control` also replays fresh `verification/RpcSocketWindow.tla` traces
through Rust and pinned C++ two-party networks, comparing socket-hint query
counts and credit outcomes. Bounds: two streams with at most three/two sends,
first-call success/error acknowledgements, two hint changes and six actions.
Seven fault controls exercise query timing, shared fallback and resize behavior.
Native Linux tests change actual TCP/Unix `SO_SNDBUF` values, observe credit,
and check that outstanding controllers do not keep sockets alive. `tcp_rpc`
tests listener cancellation, capability callbacks, disconnect and drain on real
loopback TCP. The model abstracts kernel IO and settles each controller action.

`unix_facade` compares descriptor-aware owned/borrowed acceptance, per-message
FD limits, callable capabilities and retained OS authority with the pinned C++
facade using fresh `verification/UnixFacade.tla` traces. Limits vary independently
from zero to two; zero disables sending and receiving. Five-action scenarios
include at most two transfers of two capabilities each way. Native Linux/macOS
tests also cover a Unix listener, reverse bootstrap, partial ancillary reads and
blocked output cancellation. `cargo test --lib unix_rpc::` includes framing,
buffer ownership, queue metrics and descriptor cleanup regressions.

`any_struct` checks schema-free struct allocation, mutable sections, checked
writable schema casts, capability-preserving copies and canonicalization. It
runs a fresh `verification/AnyStruct.tla` exploration and compares every
edge-prefix replay with pinned C++ `AnyStruct` calls. Bounds are four actions,
zero to two data words/pointer slots, first/last byte writes and null/data/capability
pointers. Native tests additionally cover loaded views, virtual list elements,
malformed inputs, far pointers and reader limits. Run the standalone capnp
doctests to check that overlapping mutable section borrows fail to compile:
`cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc`.

`any_list` checks all physical list encodings, explicit allocation, checked casts,
capabilities, primitive virtual structs, loaded-schema erasure, reader limits
and rejection before mutation on count overflow. Fresh `verification/AnyList.tla`
traces cover twelve layouts, zero to two elements and four actions per scenario,
including pointer-list conversion and bit-cast checks. The C++ oracle separately
reproduces the pinned upstream double-offset bug; that action compares Rust's
repaired reader against a fresh C++ root reader. See the
[comparison boundary](CPP_PARITY.md#schema-free-list-implementation-checks-2026-09-23).

TLC configurations and compressed trace corpora live in
`test-support/verification`. The corpora preserve the existing bounded shortest
prefix scenarios; they are regression vectors, not a new exploration of the Rust
executor. The native runner compares fresh TLC state and transition graphs with
canonical graph digests recorded with each corpus. Different fingerprint IDs or
DOT declaration order do not affect comparison. A changed reachable graph fails
closed and requires review of its trace corpus. Corpus digests and case counts
are checked before replay. These are bounded conformance tests, not an unbounded
protocol or cryptographic proof.

`cargo test` runs trace replays directly; missing inputs no longer silently skip
them. Explicit `REPROTO_*_TRACES` overrides remain useful for reproducing a
particular case. Default tests read checked-in corpora, never historical reports.
The newer `RpcAnswerSetup` and `RpcRouteRecovery` tests instead generate fresh
edge-prefix traces from TLC graphs using Rust. Run them with
`cargo test --test answer_adoption replay_tlc_answer_setup_deadlines -- --exact`
and `cargo test --test noise_multiparty replay_tlc_failed_route_recovery -- --exact`.
Discovery, rendezvous and active-pipeline checks also use fresh graphs:
`cargo test --test noise_deployment --test noise_pipeline_migration`. The path
commitment gate is checked with `cargo test --lib transport::mobility::` and
STUN parsing/refresh with `cargo test --lib nat::`. Owned directory renewal uses
`cargo test --lib noise_discovery::publication::` for timer regressions and fresh
TLC trace replay.
Managed advertisement checks run with `cargo test --lib noise_discovery::advertisement::`;
the deployment integration suite exercises automatic outage/recovery and a
replacement UDP endpoint while authenticated RPC stays live.
Discovery reader failover runs with `cargo test --lib noise_discovery::readers::`
and `cargo test --test noise_discovery_failover`: fresh TLC graph replay, mutation
controls, deadline/cancellation regressions and lost control routes followed by
real Noise RPC setup. The integration checks also verify that authoritative
absence and provisioning failure do not consult another reader.
[NOISE_DEPLOYMENT.md](NOISE_DEPLOYMENT.md) lists bounds, mutation controls and the distinction between policy replay and
actual UDP/Noise/RPC integration.
Local scheduling uses `cargo test --lib transport::scheduling::` for fresh
TLC/Rust replay and clock regressions, and `cargo test --test noise_scheduling`
for RPC, migration, policy wakeup, shutdown and reconnect under datagram pacing.
[NOISE_SCHEDULING.md](NOISE_SCHEDULING.md) describes the bounds and counters.
Linux Unix input uses `cargo test --locked --lib unix_rpc::` for descriptor
ownership, truncation, cancellation and buffered frame boundaries, a pinned C++
comparison, and fresh `RpcBufferedFds` graphs replayed over real sockets.
Full-frame and partial-frame configurations include mutation controls and EOF
liveness checks; partial traces run at header/body byte splits. The existing
`fd_capabilities` integration suite covers capability attachments and retained
Call/Return behavior through this reader. [CPP_PARITY.md](CPP_PARITY.md) records
the bounds and counts.
Capability diagnostics use `cargo test --locked --test capability_debug`: pinned
C++ descriptions, fresh `RpcDebugInfo` traces, four mutation controls, and native
tests for observation without promise/policy polling, reconnect, calls or retained
authority. `deferred_handoff` additionally verifies that inspection does not send
Accept; persistence checks its own wrapper description. The TLA model verifies
bounded synchronous observation, with no additional liveness claim.
Dynamic presence uses `cargo test --locked --test field_presence --test schema_loader`:
2,081 observations against pinned C++, fresh `DynamicFieldPresence` traces through
compiled and loaded readers, and four mutation controls. Native tests cover
bitwise floating defaults, all scalar widths, pointer nullness, builders,
detached groups, schema validation, missing sections, unknown types/tags and
capability lifetime/polling. The model checks bounded storage observation with
stale union slots; it makes no liveness or arbitrary-payload proof claim.
Checked conversions use `cargo test --locked --test dynamic_conversion`: 1,723
defined pinned C++ observations, fresh `DynamicConversion` traces through compiled
and loaded conversion/assignment, and five mutation controls. Native cases cover
all numeric widths, 64-bit float boundaries, direct integer-to-f32 rounding,
enum names/ordinals, blobs, aggregate brands and retained capability lifetimes.
Two exact C++ float-to-integer boundaries permit undefined casts and are tested
as Rust rejections instead of used as differential reference cases. The model
bounds input representatives and target types; it does not prove all floating
point patterns or all setter failures transactional. [Details](CPP_PARITY.md).
Ignored-result completion uses `cargo test --locked --test ignore_result --test field_api_rpc`:
fresh `RpcIgnoreResult` exploration (30 states / 50 edge-prefix traces), 100
local/RPC Rust replays matching 320 pinned C++ observations, and six mutation
controls. Native cases cover typed and both dynamic APIs, promised clients,
membranes, protected/allowed cancellation, early result/pipeline publication,
capability cleanup, no response decoding, unpolled promise drop, and the generated
field-operation facade. The C++ harness drains deferred transport batching and
cancellation work via `WaitScope::poll()`. The model assumes executor draining
between actions; it does not exhaust arbitrary network or executor schedules.
Reflection lookup uses `cargo test --locked --test reflection_lookup`: fresh
`SchemaLookup` exploration (1,117 states / 1,116 edge-prefix traces), all 66
terminal queries replayed through loaded Rust and C++, 132 total C++ observations
including compiled lookups and display names, and five mutation controls.
Native tests cover exact inheritance limits, owner/brand retention, enum ordinals,
visited and unvisited malformed brands, cycle rejection and invalid display-name
prefixes. The compiled diamond fixture rejects unreachable dispatch arms.
The model checks bounded DFS and its visit budget; replay compares public query
outcomes, not private intermediate traversal states or the whole RPC protocol.
Reflection cache identity uses `cargo test --locked --test schema_identity`:
fresh `SchemaMemberIdentity` exploration (201 states / 649 edge-prefix traces),
3,608 compiled/loaded Rust and pinned C++ cache observations, 211 additional
identity comparisons and five mutation controls. Nine handles and three cache
operations cover aliasing, owner/brand/index distinctions, inherited methods and
enums. Maps deliberately force hash collisions; numerical hashes are never
compared across languages. Native tests check stateful/cyclic callbacks,
incomplete metadata, implicit arguments and the exact 128-visit boundary.
`cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc` includes two
compile-fail cases for loaded-key lifetimes.
These checks cover bounded reflection caching, not RPC protocol equivalence.
Enums in generic scopes use `cargo test --locked --test enum_brand`: fresh
`EnumScopeIdentity` exploration (298 states / 770 edge-prefix traces), 2,914
Rust/C++ state observations, 50 identity comparisons and six mutation controls.
The model distinguishes compiled erasure from loaded wire-brand retention,
including list aliases, rejected assignments and unknown ordinals. Bounds are
two modes, five handles, three actions and one non-union destination. Native
tests cover nested/symbolic scopes, malformed bindings, generated annotation
callbacks and field-API round trips. The fixture compiles both native bindings
and the field API with values/projections enabled.
Native list casting uses `cargo test --locked --test native_list`: fresh
`NativeListCast` exploration (299 states / 1,065 edge-prefix traces), 3,032
Rust/C++ observations and seven mutation controls. Eight casts, three
registration bits and three actions check kind/depth/ID compatibility, native
generic erasure, rejected casts and in-place writes. Native tests cover blobs,
packed bits, empty lists, loader isolation, strict dynamic assignment, interface
inheritance, hook purity, capability-table preservation and calls after dropping
the message/loader. Capnp doctests include two compile-fail checks for borrowed
readers and exclusive builders. Ownership and RPC scheduling are outside this
finite model, as are arbitrary schemas and pre-cast union/default materialization.
Native capability/pipeline transfers use `cargo test --locked --test native_rpc`:
fresh `NativeRpcCast` exploration (322 states / 401 edge-prefix traces), 1,481
Rust/C++ observations and nine mutation controls. Seven conversion cases,
compiled/loaded modes, registration presence and four actions check sharing,
transfer, failed-transfer recovery, dropping owners and calls through nested
pipeline paths. Calls drain between actions. Native tests exercise pending calls
locally and over RPC, membrane denial/revocation, loader disposal, hook tripwires,
null clients and direct/nested capability-list rejection. C++ also checks list
casts; capnp doctests reject reuse after consuming a loaded client. The finite
model does not exhaust pending RPC schedules or membrane policy interleavings.
Individual enum casting uses `cargo test --locked --test native_enum`: fresh
`NativeEnumCast` exploration (494 states / 917 edge-prefix traces), 3,402 matching
Rust/C++ observations and nine mutation controls. Seven source cases, four
ordinals and three actions check nominal IDs independent of registration,
brands and loader owners, source-version lookup and union writes. C++ is
compared to Rust's open enum representation. Native tests exhaust all 65,536
ordinals through open enums and check closed enums' safe unknown-value errors,
nested generated metadata, malformed kinds and unchanged rejected writes.
Member handles can outlive messages; a compile-fail doctest checks that they
cannot outlive loaders. These bounds do not prove general schema evolution.
Structural message comparison uses `cargo test --locked --test structural_equality`:
fresh `StructuralEquality` exploration (3,925 states / 3,924 edge-prefix traces),
2,592 Rust/C++ comparisons and eight model mutation controls. Thirty-six values
cover all ordered pairs in both directions, including zero/null padding,
physical list encodings and capability uncertainty followed by definite
differences. Another 181 C++ comparisons check primitive widths, floating bits,
single/double-far pointers, malformed inputs, reader limits and raw aggregate
helpers. Native tests check hook purity, authority lifetime, generated and both
dynamic schema APIs, unknown fields and bit boundaries. This is bounded
structural comparison, not capability identity/Join or a parser safety proof.
Exceptions are compared as errors, not by their text or exact traversal budget.
Standalone membrane copying uses `cargo test --locked --test membrane_copy`:
fresh `MembraneCopy` exploration (91 states / 140 edge-prefix traces), four
payload forms and 1,960 matching Rust/C++ observations across 560 scenarios.
Eight model fault controls check crossing direction, capability preservation,
reverse unwrapping, adoption, source ownership, revocation and authority release.
Native tests cover unknown fields, nested group capabilities, rejected foreign
adoption, promise polling, substitutions, malformed input and callback errors /
unwinding. The four-action model settles calls between actions and does not
exhaust pending RPC schedules or policy callbacks. Related regressions are
`membrane`, `result_construction`, `orphan_access` and `orphan_groups`.
Rust stages transformation after structural copying; inline groups retain its
known-field rules. [API boundaries](CPP_PARITY.md#membrane-copy-implementation-checks-2026-09-23).
TLC checks are cached using model sources, configuration, Java version, jar
content and runner code. Fresh graph exploration always executes TLC; negative
controls and liveness checks can use the cache. The cache also validates result-log hashes. Rust tests
always execute. Set `REPROTO_TLC_FRESH=1` to force TLC to run again. Logs and
expanded inputs are under `target/verification`; `REPROTO_CHECK_TIMEOUT` sets
the per-command timeout in seconds (default 600).

The crash test which forks is run in a separate Cargo test process with one test
thread, to avoid inheriting another test's file locks. Full isolated release
qualification is explicitly selected because it rebuilds the entire source
bundle, runs the suite and produces a qualified artifact:

```sh
cargo test --test release isolated_release_qualification -- --ignored --exact --nocapture
```

The source bundle round-trip checks reproducibility, file inventory, modes and
hashes. Isolated qualification records executable input hashes in
`target/release-qualification/cargo-qualification.json`. Markdown and reStructuredText updates
do not invalidate runtime qualification; the archive manifest still hashes every
file, including documentation. Historical Python-era reports remain historical
evidence and are not accepted as current qualification.

The repository sets `RUST_TEST_THREADS=1` in `.cargo/config.toml`. Subprocess
creation can briefly inherit file locks before exec; serial test execution keeps
storage close/reopen assertions isolated from TLC/compiler launches. Do not
increase libtest thread counts for suites that combine these operations.

The larger composed-network exploration is separate from the runtime gate's
reference configurations and is explicitly selected with:

```sh
REPROTO_CHECK_TIMEOUT=2400 cargo test --test protocol_models protocol_reference -- --ignored --exact
```

The composed wire-handler diagnostics run as ordinary tests:

```sh
cargo test --locked --test conformance -- --nocapture
cargo test --locked --test composed_wire_boundary -- --nocapture
```

All 12 conformance checks are active. The audit states explicit generated
Release-annotation and request-identity relations, checks them on the bounded
explicit-release workload, and varies annotations independently of wire fields.
[ComposedWireBoundary.tla](../verification/ComposedWireBoundary.tla) invokes the
actual `CapnpNetwork` handlers: **279 states / 1,092 edge-prefix replays**, two
exports with one/two references, and up to two incoming Release/Call actions.
Release covers zero, partial, final, unknown/retired export and over-release;
calls vary target, method and data while retaining deliberately different caller
method/data. Replay uses raw RPC messages, with no auxiliary ticket/op fields.
Export counts are witnessed by capability destruction and later Release
acceptance/Abort, not by reading private counters; dispatch is witnessed by the
server's target/method/data log and returned value. Calls settle and finish
between actions; model method names map to distinct test-server methods rather
than their composed application semantics. A separate native boundary case
sends `u32::MAX` as the release count.

Three model fault controls require `WireRelease`, `WireMethod` and `WireData`
failures when ticket-based accounting or paired caller values are restored.
Reports are under `target/verification/composed-wire-boundary`, including the
fresh graph/log and source-bound control reports; audit logs are under
`target/verification/conformance`. This is finite safety evidence without
liveness assumptions or a C++ execution comparison. The remaining auxiliary
state and whole-model refinement limits are recorded in [CONFORMANCE.md](CONFORMANCE.md).

### Generated RPC histories and shrinking

```sh
cargo test --locked --test composed_wire_boundary composed_wire_history:: -- --nocapture
# Re-run a retained minimal input against a fresh model graph and Rust runtime:
REPROTO_RPC_HISTORY_REPLAY=/path/to/saved.json cargo test --locked --test composed_wire_boundary composed_wire_history::generated_rpc_histories -- --exact --nocapture
cargo test --locked -p reproto-test-support verification::exploration::tests
```

The [history configuration](../verification/ComposedWireHistory.cfg) uses the
actual composed handlers with two exports, one to three references each and a
folded step counter. TLC checks **630 states / 12,040 edges**, including cycles;
folding the counter does not prove termination. The original two-action graph
and its three model fault controls remain active.

Pinned Proptest 1.11.0 generates **128 histories**, each containing zero to 64
prefix operations plus one to three terminal releases. Prefixes preserve both
exports so early abort cannot hide the remaining generated work. Endings vary
full release order, over-release and zero-count release of unknown/retired IDs.
The native fixture observes dispatch target/method/data, return values,
capability destruction and Abort. It reuses two question IDs after Return/Finish
and varies explicit yields before receiving replies and between actions. Calls
still settle before the next action; these yields are not exhaustive scheduling
or a substitute for Shuttle.

Every history replays immediately from serialized JSON. Two subprocesses run the
same seed and must produce identical histories and provenance: **512 native
executions**. Coverage assertions require long histories, repeated graph states,
partial release, each terminal mode and every target/method/data combination.
Successor ordering is canonical by state values, independent of DOT line order
and worker scheduling. Graph-reader controls reject absent edge endpoints,
unreachable/duplicate states, differing fields and non-scalar values.

Choices address the current state's legal successors, so deleting prior choices
does not create invalid operations during shrinking. A synthetic two-method
failure must shrink to two calls; its saved artifact must pass real RPC replay.
This tests the reduction/retention pipeline, not Rust mutation coverage. Replay
checks the graph digest, legal transitions, generated expected states, yield
sequence, format and completion suffix, rejecting changed or truncated inputs.

Artifacts are under `target/verification/composed-wire-history`: the fresh
`graph.dot`/`tlc.log`, `histories-0.json` and `histories-1.json`, worker logs and
minimal inputs under `failures/`. Reports include the generator seed, model
configuration, canonical graph digest, source/lockfile hashes, compiler and
executable hash. The synthetic shrink-control input also lives under `failures/`
and is not a discovered protocol bug. These are sampled sequential histories;
overlapping calls, promise resolution, capability-slot reuse, reconnect, Noise
and handoff are outside this check.

Historical model snapshots remain under `research/baseline/`, including the abandoned
Apalache experiment. They are provenance, not supported test instructions or
verified protocol evidence. Current verification uses TLC through Cargo.
Machine-readable historical reports and raw logs retain their original input
hashes; [CONFORMANCE.md](CONFORMANCE.md) links the relevant model audits.

### RPC stream progress and buffer ownership

```sh
cargo test --locked --lib transport::stream_tests -- --nocapture
cargo test --locked --lib transport::engine_tests
cargo test --locked --lib transport::datagram_owned_tests
cargo test --locked --lib unix_rpc::tests
cargo test --locked --lib storage::batch_tests
cargo test --locked --test serialization_reads --test bulk --test durable_bulk
cargo test --locked --test noise_shutdown --test noise_pipeline_migration --test noise_scheduling
```

`RpcStreamProgress.tla` explores **1,306 states** and Rust replays all **5,301
reachable edge-prefix traces** through the production send/receive stream types.
The model bounds each direction to two bytes while varying partial progress,
zero-byte send acceptance, canceled observation, ordinary FIN, graceful shutdown
selection, receive EOF and consumer half-close. Independent byte patterns check
preservation and ordering; controls for lost write progress, early FIN and early
half-close must violate their named invariants. Artifacts are under
`target/verification/rpc-stream-progress`. This is bounded stream safety evidence,
not a liveness or whole-protocol refinement claim.

Socket-free Noise packet tests use the production synchronous engine and small
flow-control windows to exercise preface gating, partial writes, bidirectional
payloads and EOF. A queued datagram racing a shutdown request is discarded
without failing reliable RPC. Existing live UDP, handoff, migration, scheduling
and receipt tests exercise the adapter. Engine/migration transitions receive
explicit runtime time; quiche recovery and runtime entropy are not yet a single
controlled simulation domain.

`NoiseCloseFlush.tla` explores **12 states / 14 edge-prefix traces**, replayed
against real Noise engine pairs. Every trace obtains an authenticated receipt,
queues CLOSE, then varies yields before and after sending its single packet.
Quiche's resulting draining state must retain the receipt until output reports
`Done`. Two named controls reject forgetting that receipt and claiming output
completion early. Artifacts are under `target/verification/noise-close-flush`.
This covers successful receipt/output ordering; IO errors, owner cancellation
and deadline failures remain covered by the existing shutdown tests.

Owned datagram tests check allocation identity across admission, backpressure,
invalid size and closed-queue rejection. Borrowed admission reserves first.
Unix writer tests send consecutive small/large/small frames with distinct
payloads and descriptors, complementing the existing forced-partial-write and
FD-framing TLC tests. These checks establish ownership and framing behavior;
they do not claim measured throughput or tail-latency improvements.

`serialization_reads` uses independent one-to-five-segment fixtures, read sizes
1–16, reused dirty buffers, back-to-back messages, and every nonempty truncated
prefix. It guards the no-allocation reader's segment-table short-read fix.
Storage batch tests cover fixed headers, variable payload offsets, every
truncation, duplicate objects, invalid revisions/flags, padding and size overflow.
The storage parser uses checked array views under the root Rust 1.97 requirement;
the vendored parser fix needs no newer language APIs.

## Shared transport clock

```sh
cargo test --locked --lib transport::clock_tests -- --nocapture
cargo test --locked --test tooling quiche_noise_runtime_clock_regressions -- --exact
```

The runtime enables quiche's `tokio-clock` feature. Its monotonic timestamps,
recovery, idle/path timers and `SendInfo.at` pacing deadlines use the same clock
as runtime datagram admission, migration and shutdown. The standalone fork uses
the OS clock by default; its private `cfg(test)` packet-simulation scope still
takes precedence. The clock feature enables neither Tokio `test-util` nor a
deterministic RNG in production. Create and drive each connection in the same
time domain; moving a live connection between independently paused runtimes is
unsupported.

Native tests advance virtual time by ten minutes before creating real Noise
peers, lose encrypted stream packets, fire recovery, check ordered delivery and
pacing timestamps, and exercise idle expiry, replacement isolation, datagram
credit, candidate-path expiry and shutdown. The production outer driver is also
polled with real UDP sockets and a duplex RPC bridge: idle expiry terminates it,
and a coincident shutdown deadline retains its own terminal cause.

`NoiseClockDomains.tla` explores **56 states / 99 edge-prefix traces** with two
connection generations, four times (five-second ticks), ten-second idle expiry,
and independent timer polling order. Every trace builds real authenticated
connections under paused Tokio time and compares close/timeout state after each
transition. Controls for a stalled wall clock, premature expiry and an inherited
replacement deadline must violate their named invariants. This finite model
covers timer dispatch and generation isolation, not transport liveness under
arbitrary loss or application-level exactly-once behavior.

Packet delivery in the engine fixtures is controlled in memory; the migration
and outer-driver tests still bind OS sockets. Runtime entropy remains secure and
uncontrolled, so these are reproducible semantic scenarios, not byte-identical
whole-runtime simulation. The packet IO adapter below extends the transport
driver tests; full socket/executor composition and identity/token entropy
injection remain separate roadmap items.

The tooling gate runs all **1,104** fork library regressions for each of `noise`
and `noise,tokio-clock`; both inventories must include the existing packet,
recovery and lifecycle simulation families, with no ignored or filtered cases.
The pinned-profile gate also checks that production dependency edges enable
`tokio-clock` without enabling Tokio's `test-util` feature.

## Runtime packet IO and semantic replay

```sh
cargo test --locked --lib transport::simulation -- --nocapture
cargo test --locked --test runtime_simulation
```

The private `cfg(test)` socket replaces only packet IO. Tests run the actual
Noise connection, async transport driver, native stream preface, bounded duplex
bridge and receipt protocol. Driver futures are polled only when their real
wakers signal readiness. The adapter models UDP truncation, canceled pending
operations, send readiness, send/receive errors, closure and address reuse; it
never binds an OS socket. The queues are bounded at 1,024 packets in tests.

IK and IKpsk2 scenarios lose the first handshake flight, inject data/ACK loss,
duplicates, corruption, delay and reordering, and check exact bidirectional bytes
through 11-byte application bridges and 42-byte stream windows. Crossed shutdown
checks the byte counts of authenticated receipts. Further tests blackhole a
finite partition, replay old encrypted packets into new sessions using the same
identities/addresses/CIDs, and ensure retired socket handles and terminal controls
cannot affect replacements. Error paths exercise actual blocked driver futures.

`NoiseSocketDriver.tla` explores **344 states / 790 edge-prefix traces**. Each
trace creates a real driver blocked on its first encrypted send, arms a real
virtual-time shutdown deadline, and races socket closure, timeout, IO failure,
unblocking, cancellation and polls. The tests compare terminal causes and packet
emission after every event. Shutdown expiry precedes simultaneous dedicated
socket closure or IO failure, and recorded terminal results remain sticky.
Dedicated closure is an IO error, separate from shared-listener shutdown (which
retains its outer-driver priority). Send failure is observed
on an attempted send; it cannot fail an unrelated idle receive wait. Four model
controls violate close handling, deadline precedence, terminal stability or
canceled-send suppression. The model is a finite safety check, not a general
liveness proof for network partitions.

`NoiseCandidateIo.tla` explores **44 states / 43 edge-prefix traces**, replayed
against established IK and IKpsk2 drivers (**86 runs**). Each starts with a blocked
candidate socket and uses real CID exchange, probe retransmission and encrypted
validation replies. Macro-steps cover established stream progress while blocked,
holding replies, corrupted proof, commitment, queued proof racing timeout or
cancellation, candidate receive failure, duplicate/late replies and cancellation
after commitment. Replay compares outcomes, old/new socket ownership and exact
stream bytes; terminal paths must still carry bidirectional traffic. Cancellation
is projected from dropping the operation and observing candidate retirement,
since a dropped future has no return value. Five model controls break progress,
authentication, blocked expiry, retirement or commitment stability. This finite
safety model does not establish liveness under arbitrary packet or task schedules.
Separate regressions check candidate send errors, retry on another path and
crossed receipt shutdown after migration. A blocked-send regression reproduced
the original production stall before the nonblocking probe fix. Run these with:

```sh
cargo test --locked --lib transport::simulation::tests::migration -- --nocapture
```

Retained TLC graphs and controls are under `target/verification/noise-candidate-io`.
These lifecycle traces complement the existing abstract `NoisePathMigration`
gate checks; they do not fabricate quiche validation state.

Shared-listener simulation runs the production listener receive loop, CID routing,
reservation accept and spawned transport drivers on a Tokio `LocalSet`, without
OS sockets. Its fixture authenticates two distinct clients and holds a third
unaccepted reservation. `NoiseListenerIo.tla` explores **213 states / 456 edge-prefix
traces**, replayed under IK and IKpsk2 (**912 runs**). Each starts with both shared
sends blocked. Macro-steps unblock sends, cancel one route, drain admission,
expire the pending reservation, inject transient/fatal receive errors, close the
listener or drop its last owner. After each step, replay checks actual driver
completion, listener counts/admission state and exact delivered stream bytes.
Retired reservations must fail to accept. Five failing model controls exercise
missing send progress, sibling cancellation, admission/expiry leaks and incomplete
shutdown. This is finite safety evidence with bounded delivery between actions;
it does not explore every Tokio task ordering or prove general network liveness.

Separate packet tests flood pending routes with malformed, oversized, unknown-CID
and STUN datagrams; substitute one established route's destination CID with
another's; and replay duplicate/stale ciphertext. They check isolation, failed
identity/PSK authentication and fresh reservations at reused client addresses.
An executor-level test verifies that readiness/error/closure wakes every blocked
shared sender and that canceled sends produce no output. Unlike the two-peer
fixture, these listener tests retain production bridge/window sizes. Run with:

```sh
cargo test --locked --lib transport::simulation::tests::listener -- --nocapture
```

TLC graphs, model configurations and negative-control logs are retained under
`target/verification/noise-listener-io`. The existing `RpcNoiseListener` registry
model remains complementary; the new traces obtain authentication through real
Noise handshakes rather than directly marking a route authenticated.

One-shot STUN discovery uses a private borrowed `DatagramIo` interface for both
real UDP and simulated sockets. `NatDiscoveryIo.tla` explores **40 states / 47
edge-prefix traces**, replayed with IPv4 and IPv6 observed addresses (**94 runs**).
The real discovery future starts blocked on a send. Actions unblock or fail the
send, retry, cancel, deliver a valid/invalid reply, expire the operation, or race
a reply with the deadline. Replay checks outcomes and emitted datagrams, including
transaction stability on retransmission and no output after completion. Four
failing model controls cover blocked expiry, foreign replies, deadline precedence
and late output. Native regressions reproduced a production bug where an awaited
send hid the timeout, and check that a reply to an earlier probe can finish an
operation while a retry is blocked. Run with:

```sh
cargo test --locked --lib transport::simulation::tests::nat -- --nocapture
```

The existing `NoiseMappingRefresh` model's **63 states / 122 edge-prefix traces**
also replay through the real shared-listener receive loop under IK and IKpsk2
(**244 runs**). Tests capture actual STUN transaction IDs from emitted packets,
encode responses independently, and advance virtual time while exchanging
encrypted stream bytes to keep established sessions alive. Mapping observations
drive real `MappedService` advertisements and capability-based directory lookups.
The model supplies the expected address/withdrawal state; additional native
oracles check recipient/host bindings, publication generation agreement, rejection
of retained provisioning capabilities after withdrawal/replacement, and stream
continuity. The model does not represent every publication or capability state.
A separate test covers shared-send backpressure, mapping timeout, old replies and
owners after replacement, and admission drain followed by listener closure.

```sh
cargo test --locked --lib transport::simulation::tests::listener::mapping -- --nocapture
```

Artifacts are retained under `target/verification/nat-discovery-io` and
`target/verification/noise-mapping-socket`. These tests inject routing hints;
they do not emulate NAT translation/filtering, prove endpoint reachability, or
exercise provisioning dials and authorized punching through simulated IO.

The property harness samples **32** ChaCha-seeded schedules with one to six
packet decisions (`0=deliver`, `1=drop`, `2=duplicate`, `3=corrupt`, `4=delay`).
It uses seed `0x5250534f434b4554`, bounds delivery at 4,000 one-millisecond turns
and shutdown at 2,000 turns, and allows Proptest to shrink failing inputs. Every
attempt retains its inputs and packet length/action/time log, including shrinking
attempts. Successful semantic outcomes are written to `runs.json`. For example:

```sh
REPROTO_RUNTIME_SIM_OUTPUT=target/verification/runtime-socket/example \
  cargo test --locked --lib transport::simulation::tests::generated_runtime_packet_schedules -- --exact
REPROTO_RUNTIME_SIM_REPLAY=target/verification/runtime-socket/example/runs.json \
  cargo test --locked --lib transport::simulation::tests::generated_runtime_packet_schedules -- --exact
```

By default each run gets a fresh retained directory under
`target/verification/runtime-socket`; `REPROTO_RUNTIME_SIM_OUTPUT` selects an
explicit directory. Failed runs clear prior success evidence, and replay can
read and rewrite the same saved path. The separate
Cargo qualification test samples in two independent processes, compares saved
inputs and actual delivered bytes/receipts, replays the saved cases, and requires
a corrupted expected receipt to fail. Its retained directory under
`target/verification/runtime-socket-qualification` includes source, compiler,
binary and saved-case hashes.

This qualifies **semantic outcomes**, not exact task schedules or ciphertext.
Noise ephemeral entropy and runtime nonce generation remain secure and
uncontrolled, and the production `select!` arbitration is unchanged. Arbitrary
combined listener/migration histories, route arbitration, provisioning dials,
authorized NAT rendezvous and routed capability handoff still require broader
composition tests. The cross-process
qualification above covers the generated two-peer scripts; shared-listener tests
replay bounded TLC actions and make no claim of exact task-schedule replay.


## Caller-owned async word scratch buffers

```sh
cargo test --locked --test async_scratch --test allocations --test serialization_reads
cargo test --locked --manifest-path vendor/capnp-futures/Cargo.toml
```

These also run through `cargo test --workspace`. The async scratch suite compares
168 framing/storage cases against the pinned C++ async reader and tests partial
reads, limits, I/O errors and cancellation. It needs the normal pinned C++ build
prerequisites. The allocation suite checks that a fitting 8 KiB payload allocates
only segment metadata; the capnp-futures doctest rejects an escaping scratch
borrow. See [the parity contract](CPP_PARITY.md#async-word-scratch-implementation-checks-2026-09-27)
for the standalone contract, and the adapter checks below for buffered input.

## Buffered and descriptor scratch storage

```sh
cargo test --locked -p reproto --test buffered_input --test allocations
cargo test --locked -p reproto --lib unix_rpc::
cargo test --locked -p reproto --doc
```

All run through `cargo test --workspace`; the second command applies to Linux/macOS.
Portable [scratch tests](../tests/buffered_input/scratch.rs) cover exact framing
capacity, multi-segment offsets, the 511-segment boundary, retained/shared storage,
owned conversion, every selected partial prefix, errors, EOF and reuse. The
[descriptor tests](../src/unix_rpc/scratch_tests.rs) check occupied slot rejection,
prefetched FD ownership, changing capacities, cancellation/retry/drop, CLOEXEC,
truncation and cleanup. Both word and FD views have compile-fail lifetime checks.

The pinned [C++ oracle](../tests/cpp/buffered-fds.c++) compares 1,008 new observations
across scratch sizes, short/retained classification, complete/direct reads,
descriptor budgets and split ancillary frames. Allocation counters require one
segment-index allocation for fitting buffered words, including a staged-FD case;
the insufficient-scratch control also allocates payload and Arc storage.

Platform smoke CI runs the five portable scratch tests; Linux/macOS run native
descriptor cases. Linux additionally checks prefetch and allocation contracts. Full workspace verification
includes the C++ comparisons and existing bounded TLC replays. See the
[ownership contract](CPP_PARITY.md#buffered-and-descriptor-scratch-checks-2026-09-28).

## Linux/macOS descriptor transport

The existing smoke jobs run these commands on Linux and macOS with the auditable
Cargo wrapper. All tests remain reachable through `cargo test --workspace`.

```sh
cargo test --locked -p reproto --lib unix_rpc:: -- --skip tlc --skip pinned_cpp
cargo test --locked -p reproto --test unix_facade --test fd_capabilities -- --skip tlc
cargo clippy --locked -p reproto --lib --test unix_facade --test fd_capabilities -- -D warnings
```

The [ancillary tests](../src/unix_rpc/ancillary_tests.rs) force explicit CLOEXEC
and full control-buffer receipt on Linux as well as macOS. They check zero/small
FD limits, excess-FD closure, cleanup after an injected configuration failure,
and truncated header lengths without reading beyond returned control bytes. An
isolated subprocess restores the default SIGPIPE handler and checks that writing
to a closed peer returns an error. The [frame-boundary test](../src/unix_rpc/buffered_tests.rs)
forces the macOS read policy on Linux: queued bare/FD/bare frames, a split header
and changing per-message ownership must keep each FD on its original frame.

Shared socket tests cover short writes, descriptor identity, close-on-exec,
limits, canceled partial reads, caller slots, owned/borrowed facade cancellation,
reverse bootstrap and callable FD-bearing capabilities. Linux-only tests retain
prefetch, buffered allocation and TLC/pinned C++ assertions. On macOS, exact-frame
reads use owned bodies and do not invoke the buffered-message classifier; the
caller FD slots and partial-read ownership rules still apply. Native macOS test
execution is pending; platform smoke success is required before qualification.

Local validation logs are under `target/macos-fds-*.log`. The direct cross-check
of the full crate stopped at `ring` because this Linux host has no Apple SDK
(`TargetConditionals.h`). A disposable crate under `target/macos-fds-check/`
checked byte-identical transport sources and unit tests, using the real vendored
runtime dependencies, with strict Clippy for `aarch64-apple-darwin`. This checks
Darwin types and platform branches; it does not execute macOS socket behavior.

## Rust schema-language frontend

```sh
cargo test --locked -p capnp-compiler
cargo test --locked -p reproto --test schema_compiler
```

Both run through `cargo test --workspace`. The standalone crate tests use no C++
tools and run in each platform smoke job. They cover requests accepted by the
schema loader, deterministic layout, invalid/unsupported syntax, diagnostics,
resource limits, truncated input, import paths/identity/cycles, aliases and the
CLI. The root integration suite compares 107 original schemas, 29 import/alias
graphs and 212 union/group schemas against pinned C++, with 36 shared
syntax/semantic rejections. A further 128 deterministic nested layouts produce
97 matching requests and 31 matching historical issue #344 rejections. Five
unmodified upstream layout fixtures are included. It also generates, compiles
and executes Rust bindings from single-file, cyclic multi-file and union/group
Rust-produced requests; the latter switches alternatives and preserves fields
outside the union. Constants add 123 accepted schemas and 63 shared rejections,
plus 15 import graphs and eight shared rejections. A fourth generated Rust test
checks public constants and imported Data defaults without imported bindings.
Composites add 69 accepted schemas and 22 shared rejections, plus 17 import graphs
and five rejections. A fifth generated Rust test reads, mutates and serializes
list/struct/group/union defaults, AnyPointer defaults and constants, while checking
that mutations leave constants and other messages unchanged. Native tests verify
decoded composite values, erased import dependencies, null versus empty pointers,
diagnostics and bounded expanded depth/work/storage. Annotations add 56 accepted
schemas and 37 shared rejections, plus 22 import graphs and eight rejections.
These include the real `rust.capnp` and `c++.capnp` annotation files. A sixth
generated Rust test checks renamed types/fields/variants/groups/unions, optional
getters, parent modules, serialization and reflection by original schema names.
Native annotation tests cover target flags, typed arguments, ordering, cycles,
imports, truncated input and shared expansion limits. Interfaces add 52 accepted
schemas and 38 shared rejections, plus 12 import/streaming graphs and six
rejections. A seventh generated Rust test executes inherited calls, explicit
struct signatures, defaults, pipelined capabilities and streaming uploads.
Native tests check stable method IDs, schema tags, import roots and bounded
syntax; generator regressions check cyclic, deep and exponential inheritance.
Generics add 56 accepted schemas and 28 shared rejections, 17 imported graphs,
four unmodified standard schemas and all 21 repository schemas. The eighth
generated acceptance crate checks generic defaults/groups, serialization,
inherited RPC, implicit method parameters and explicit nested signatures.
Native regressions check brands, alias isolation, annotation/default diagnostics,
truncated inputs and bounded expansion. A separate C++ comparison documents the
intentional rejection of lists of unbound parameters at every parameter index.
Strings add 65 accepted requests and 13 shared rejections; embeds add 30 accepted
requests and 16 shared rejections. The ninth generated acceptance crate checks
byte constants, embedded struct defaults, mutation, serialization and unknown
fields. Native regressions cover binary inputs, lazy files, path roots, malformed
messages, cycles and per-file/aggregate/work limits.
Documentation adds 52 accepted requests covering attachment, whitespace, BOMs,
Unicode, CRLF/CR and EOF, including method/parameter locations. Native regressions
cover imported ranges, member order, truncated syntax and expanded documentation
limits.
The focused corpus totals 1,119 matching accepted requests and 311 matching
rejections. Two accepted requests contain inheritance cycles that both runtime
loaders reject; their request fields are still compared. There are 74 native
integration tests, six doctests and 35 root integration tests. An AnyPointer
constant reference that asserts in pinned C++ produces a Rust source diagnostic.
Native tests cover group IDs, shared sizes, layout work, constant cycles and value
expansion limits. A separate test records the intentional rejection of oversized
integer literals that pinned C++ wraps.
Comparisons include all fields known to our schema bindings, node/member byte
ranges, documentation comments and canonical constant/default/annotation/source-info
bytes. The known C++ built-in StreamResult Node range is normalized using its
source-info. Per-file identifier references are compared as sorted, unique
byte-range/target entries; the C++ compiler version remains excluded. An additional
63 accepted requests cover parenthesized names, aliases, built-ins, imports and
multiple requested files. Native regressions cover empty tables, UTF-8 offsets,
retry isolation and synthetic streaming signatures. The regenerated schema
bindings also pass 35 schema-loader,
metadata, introspection and reflection tests. Logs are under `target/verification/schema-compiler/`.


### Generated Rustdoc and Rust build inputs

`cargo test --locked -p reproto --test schema_compiler rustdoc::` generates standard
bindings and field-API/native-value/projection bindings from both Rust and pinned
C++ requests. It compares generated bodies, compiles via `include!`, builds HTML
with warnings denied, checks documentation on individual items, and runs 13
explicit generated Rust doctests. Untagged schema examples stay non-Rust text.
Missing/partial metadata and malformed UTF-8/duplicate source-info have regressions.

`cargo test --locked -p capnp-compiler --test build_inputs` checks loaded schema
and embed dependencies, import search precedence, retry isolation, and symlink
spellings/canonical paths (the symlink case is Unix-only). Root and test-support
build scripts use this API and bundled standard imports. The vendored RPC crate
and downstream example still invoke the external compiler.

Evidence is under `target/verification/schema-compiler/rustdoc/`; rendered HTML
is under `target/schema-compiler-acceptance/doc/rust_schema_docs/`. All these
regressions remain included in `cargo test --workspace`.

### Parsed-schema runtime snapshots

`cargo test --locked -p capnp-compiler --test parsed` exercises owned snapshots,
declaration navigation, imported brands, group/method metadata, dynamic wire
roundtrips, dependency selection, compile/loader failure isolation, input tracking
and source deletion/recompilation. A compile-fail doctest prevents a parsed handle
from outliving its owner. Existing platform smoke jobs include these native tests.

`cargo test --locked -p reproto --test schema_compiler parsed::` builds the pinned
C++ `SchemaParser` oracle and compares all 15 declarations in the two-file
reflection fixture: IDs, kinds, source/member ranges, comments and direct nested
lookups. Both implementations build a dynamic message with branded pointers,
groups, a union and struct lists; canonical wire bytes must match exactly.
Defaults are inspected through readers to avoid builder getters materializing
pointer defaults. Evidence is under `target/verification/schema-compiler/parsed/`.
This immutable API does not resolve aliases or compile during navigation; the
session API below supports that work under an exclusive borrow. Both suites run
under `cargo test --workspace`.

### Lazy textual schema sessions

`cargo test --locked -p capnp-compiler --test session` checks incremental selection,
declaration aliases, imported/file/constant/annotation aliases, generic binding
erasure, implicit method metadata, dynamic defaults, atomic batch rejection and
cumulative loader limits. Disk tests delete cached sources, modify newly discovered
files after failed compilation/validation, and check that retries see corrected
inputs without retaining provisional schemas or dependencies. A compile-fail doctest
prevents extension while a schema handle is live.

`cargo test --locked -p reproto --test schema_compiler session::` compares 18 session
snapshots against the pinned C++ compiler's public `lookup()` and lazy schema loader,
followed by dependency/parent completion with `eagerlyCompile()`. Every selected node
and its source-info must have identical canonical bytes. It also verifies declaration
alias behavior through C++ `SchemaParser::findNested()`. Cases include aliases with
erased generic bindings, aliases of file scopes/constants/annotations, generic
parameters, imported defaults, embeds, groups, methods and unused invalid siblings.
It does not assert C++ failure atomicity or concurrent caching. Evidence is under
`target/verification/schema-compiler/session/`. Both suites are included in
`cargo test --workspace`; platform smoke jobs include the native tests.

### Custom source providers

`cargo test --locked -p capnp-compiler --test provider` covers callback-defined
identities, unmodified import strings, aliases, cycles, binary embeds, read limits,
diagnostic locations, lazy imports and absence without disk fallback. Failed
compilation/runtime validation must preserve the prior session snapshot; retries
must reopen provisional inputs, while successfully loaded inputs stay cached.
The crate's doctests include a minimal provider with a borrowed byte reader.

`cargo test --locked -p reproto --test schema_compiler provider::` compares six
canonical schema nodes, four available source-info records and one dynamic wire message with the pinned
C++ `SchemaFile` callback API. It checks repeated schema identity, file aliases,
recursive imports, unused imports, binary defaults and lazy nested lookup.
C++ omits source-info for the imported file and lazily loaded sibling; Rust
retains it, while their complete schema nodes still match.
Embed read counts are not compared: C++ can read an embed for each use while Rust
caches it by identity. Logs are under `target/verification/schema-compiler/provider/`.
Both suites run through `cargo test --workspace`, with native provider tests in
the existing Linux/macOS/Windows compiler smoke jobs.

### Optional schema file IDs

`cargo test --locked -p capnp-compiler --lib --test file_ids` checks strict defaults,
explicit overrides, high-bit generation, fresh identities, imports/cycles, provider
identity deduplication, dynamic defaults and policy capture across all three source
entry points. Compilation/runtime validation failures preserve committed session
IDs. Private entropy controls verify failure diagnostics, collision rejection and
that strict/explicit-ID inputs never request randomness. The CLI remains strict.

`cargo test --locked -p reproto --test schema_compiler file_ids::` exercises pinned
C++ `setFileIdsRequired(false)`: default rejection, cached/repeated identity,
fresh-parser IDs, import aliases and explicit declaration overrides. It compares
12 canonical schema/source-info pairs and a dynamic message. Independent random
draws are not expected to match: the oracle's generated root IDs are appended as
explicit IDs for Rust comparison, preserving declaration byte offsets. Separately,
Rust's actual optional-ID output is compared with the same source recompiled using
its generated IDs explicitly. Cases include groups, unions, methods, constants,
recursive imports and fixed-ID descendants. Evidence lives under
`target/verification/schema-compiler/file-ids/`.

These tests and the API example run through `cargo test --workspace`; native tests
remain in the existing compiler smoke jobs on Linux, macOS and Windows.

### Schema grammar corpus

`cargo test --locked -p capnp-compiler --test grammar` runs the shared
[295-case corpus](../crates/capnp-compiler/tests/corpus/grammar.rs) without C++.
It checks expected acceptance, runtime loading and valid diagnostic source spans.
Separate assertions resolve contextual generic parameters through branded fields
and method signatures, and exercise the leading-zero numeric rule through the
shared text-value parser used by compatibility codecs.

`cargo test --locked -p reproto --test schema_compiler grammar::` compares the
same corpus with pinned C++. All 205 accepted cases compare known schema-node
fields, source-info fields and canonical bytes, canonical constants/defaults,
requested files and normalized identifier references. The remaining 90 cases
require rejection by both compilers; diagnostic wording is not equated. C++'s
compiler-version field is excluded, as in the existing differential tests.
Sources, diagnostics, decoded requests and the summary are retained under
`target/verification/schema-compiler/grammar/`.

Cases cover contextual keywords and built-ins, duplicate/shadowed generic
parameters, annotation targets, declarations inside groups/unions/enums,
parenthesized types and values, trailing commas and numeric spellings. The
leading-zero float cases vary the integer prefix, sign, fraction and exponent.
Implicit system imports are outside this single-file corpus; import and streaming
coverage remains in the existing dedicated suites. This corpus does not certify
every grammar production or diagnostic. Both tests run through
`cargo test --workspace`; portable tests join the existing compiler smoke jobs on
Linux, macOS and Windows.

The same commands also run 108 lexical cases: all ASCII controls, space and DEL
in token spacing, quoted strings and hexadecimal binary literals, plus selected
Unicode separators. The pinned compiler agrees on 46 accepted requests and
62 rejections. Metadata, values and diagnostic spans use the same checks as the
grammar corpus; artifacts are under `target/verification/schema-compiler/lexical/`.
The standalone text-value parser also has a vertical-tab byte-preservation check.

The [numeric corpus](../crates/capnp-compiler/tests/corpus/numbers.rs) adds 33 valid
and 33 invalid expressions, each compiled as a requested declaration and as an
unused constant in a loaded import: **132 pinned C++ comparisons**. Accepted
requests compare the same metadata and canonical values as the grammar corpus.
Cases include radix syntax, signed limits, malformed exponents, punctuation,
Unicode suffixes, float overflow/underflow and negative zero. Incomplete
exponents cause an exception in pinned C++; only rejection is compared, not its
diagnostic behavior. Logs live under `target/verification/schema-compiler/numeric/`
and `numeric-imports/`.

Portable tests also run the expressions through the shared text-value parser.
`cargo test --locked -p capnp-compiler --test session` verifies that numeric
type/range failures stay lazy, newly discovered lexical failures preserve the
snapshot and dependency list, and corrected source can be loaded on retry.
These tests use the existing workspace command and platform smoke jobs.

### Full upstream corpus and concurrent parser cache

The [qualification report](SCHEMA_COMPILER_QUALIFICATION.md) records grammar
coverage and remaining differences. `cargo test --locked -p reproto --test
schema_compiler upstream::` compares all 22 real upstream schemas with pinned C++
under standard source roots. A second regression compares the mixed-root fixture
without changing display names or prefix lengths.

`cargo test --locked -p capnp-compiler --test discovery` runs the shared
[22-case import-order corpus](../crates/capnp-compiler/tests/corpus/discovery.rs)
in memory and through the concurrent cache, including lazy extensions and retained
snapshots. `cargo test --locked -p reproto --test schema_compiler discovery::`
compares those cases with pinned C++ using the filesystem frontend. Cases cover
declarations, aliases, nested groups, annotations, generic dependencies, RPC
signatures and default-evaluation phases. All usual request fields and canonical
values are compared without display-name adjustments. Inputs, decoded requests
and the summary live under `target/verification/schema-compiler/discovery/`.

`cargo test --locked -p capnp-compiler --test cache` exercises the worker-owned
cache, including actual concurrent callers, immutable generations, single reads,
alias reuse, atomic failure/retry, limits, optional IDs and lifecycle failures.
The `session::` root compiler test compares both cache and exclusive-session
snapshots against the same pinned C++ oracle. All tests join the existing
workspace command; portable tests join the platform compiler smoke jobs.

## Dynamic group initialization and clearing

`cargo test --locked -p reproto --test dynamic_groups` checks loaded group
initialization, clearing and non-clearing views against compiled Rust reflection
and the pinned C++ runtime. Sixteen traces compare 38 canonical wire observations,
including repeated resets, nested/default union groups, inactive pointer/data
storage, union activation and bound generic fields. C++ loads its own compiled
schema and the same serialized seed. Evidence is under
`target/verification/dynamic-groups/`.

Portable tests also assert typed defaults, writable returned views, retained
brands and sibling values. A separate regression checks orphan adoption's
stronger recursive cleanup of inactive storage. The existing three-platform
smoke job runs these cases while excluding the Linux C++ oracle. All cases
remain part of `cargo test --workspace`.

## Loaded mutable getters and union selection

`cargo test --locked -p reproto --test dynamic_getters` checks struct, group,
primitive/struct/nested/capability-list and generic getters. The portable and
pinned C++ comparisons cover 53 states, including 35 rejected accesses, checking
both results and canonical wire. Seeds include null pointers with schema defaults,
populated values, a different pointer kind retained behind another arm and unknown
union tags. Non-union fields remain usable with unknown tags. Rejected portable
cases also compare full serialized message bytes to detect materialization or
allocation during a failed read.

Separate native tests verify writable defaulted and branded views, explicit
group selection, and live capability ownership retained behind inactive arms.
Errors must identify inactive access before pointer interpretation; table entries
and hook identity remain unchanged until explicit clearing releases the owner.
The existing C++ group fixture has an optional result-reporting mode so errors
can be compared together with the remaining message. Evidence is under
`target/verification/dynamic-getters/`. The portable cases join Linux/macOS/Windows
smoke CI; all cases remain part of `cargo test --workspace`.

## Existing nested mutable lists

`cargo test --locked -p reproto --test dynamic_nested_lists` checks loaded
`ListBuilder::get_list(index)` against compiled Rust reflection and pinned C++.
The 73 comparisons include 22 rejections and compare success, returned lengths
and canonical bytes. They cover scalar/bit/enum/text/data/void/capability/struct
children, another level of nesting, null and explicitly empty children, sibling
preservation, out-of-bounds indices, non-list elements and incompatible bit-list
storage. Primitive and shorter struct lists upgrade to writable branded struct
views; larger physical layouts retain unknown data and pointer fields.

Portable tests also compare the full serialized message on compatible reads and
rejected access, verify retained values after upgrades, and check live capability
identity, table ownership and release after replacement. The compiled regression
panics with the previous getter. A compile-fail doctest checks exclusive parent/
child borrowing: `cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc schema_loader::dynamic::ListBuilder`.

C++ schema requests, seeds, per-case output and the summary stay under
`target/verification/dynamic-nested-lists/`. The three portable tests run in
Linux/macOS/Windows smoke CI. The complete suite is discovered by
`cargo test --workspace`; the C++ test runs on Linux.

## Loaded mutable text and data

`cargo test --locked -p reproto --test dynamic_blobs` compares 100 blob access
results, contents and canonical wire states with compiled Rust reflection and
pinned C++, including 25 rejected accesses. Cases cover null, populated and
explicitly empty fields/elements; nonempty and empty defaults; branded group
fields; inactive and unknown union selections; in-place edits; zeroed sized
initialization; non-byte storage; missing text termination; invalid UTF-8 bytes;
and out-of-bounds list indices.

Portable tests check the original backing address, unchanged serialized bytes
on compatible reads and errors, independently materialized defaults, and nullness
for empty defaults. Overflow/type/bounds failures preserve union selection and
capability-table ownership. Successful replacement releases the old hook and
maintains text termination. The empty-default regression fails with the previous
compiled getter. Parent/child borrowing and message lifetime are guarded by
compile-fail doctests, included through `tooling::standalone_crate_tests` in the
workspace suite. A focused command is
`cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc schema_loader::dynamic`.

The three portable tests join Linux/macOS/Windows smoke CI; the C++ test runs on
Linux. All are part of `cargo test --workspace`. C++ schema requests, seeds,
outputs and a summary remain under `target/verification/dynamic-blobs/`.
Oversized native requests are rejected before allocating; the oracle uses small
sizes only. Loaded initializer prevalidation is stronger than C++'s early union
selection on invalid initialization, so that failure-atomicity contract is checked
locally rather than claimed as C++ failure-state equivalence.

## Loaded untyped pointer fields

`cargo test --locked -p reproto --test dynamic_pointers` compares 85 pointer-view
and initialization results, physical layout observations and canonical wire
states with compiled Rust and pinned C++, including 21 rejected accesses. The
oracle uses dynamic field lookup followed by raw pointer, AnyStruct or AnyList
conversion. Seeds cover null/empty/populated pointers, all list encodings,
inline structs, wrong pointer kinds, ordinary and unknown union selections,
non-union fields, groups and a generic AnyPointer binding. Alternating fixtures
use tiny fixed segments to force far pointers. Writes retain unknown fields and
siblings; initializers cover zero/nonzero sizes and selection of inactive arms.

Portable tests also verify original backing addresses, unchanged full serialized
bytes on compatible reads and failures, and rejection of the wrong accessor for
a declared constraint or concrete generic binding. Invalid encodings/counts and
word-count overflow must preserve selection and live capability-table entries.
Positive struct/list views retain descendant capabilities; extracted clients
outlive replacement, and the server is released exactly once after its last owner.
Unconstrained raw access can inspect a capability pointer without extracting it.

The four portable tests join Linux/macOS/Windows smoke CI; the C++ comparison
runs on Linux. All are part of `cargo test --workspace`. Two compile-fail doctests
check message lifetime and exclusive parent/child borrowing through the existing
standalone-crate test driver. Run them with
`cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc schema_loader::dynamic::pointer`.
C++ schema requests, seeds, outputs and the summary stay under
`target/verification/dynamic-pointers/`.

The C++ oracle exercises supported operations with small allocations. Loaded
constraint checks and validation before union activation are checked locally;
C++ raw-pointer reflection has a broader access surface and can activate an arm
before an invalid initializer fails.

## Loaded schemas on schema-free views

`cargo test --locked -p reproto --test loaded_view_casts` checks 62 reader and
builder casts. All match compiled Rust's acceptance, values and canonical bytes;
41 compatible cases also match pinned C++. The other 21 are stricter Rust layout
rejections. The corpus covers short/full/oversized structs, defaults, generic
Text/Data bindings, empty and evolving struct lists, scalar/pointer projections,
packed bits, unknown enum ordinals, nested lists, far pointers and mutable edits.

Portable tests also check unchanged serialized messages on rejected casts,
backing addresses and unknown fields after mutation, unregistered schemas,
invalid or unresolved type metadata, lazy malformed-child rejection, nesting
limits and remaining traversal budgets. Opaque capability hooks detect any
unexpected cloning, identity checks, resolution or calls during casts. Explicit
extraction retains a reference across message/loader teardown; replacing the
field or element releases only the original table reference.

Pinned C++ has no mutable `AnyList::as<DynamicList>(schema)` overload. For those
compatible mutable cases, the oracle uses its owning pointer getter on storage
that already fits. It reads back through a fresh AnyList reader to avoid the
known C++ projected-builder reader-offset issue. Rust's stricter cast rejections
are checked against compiled Rust, not asserted as equivalent C++ failures.
Canonicalization wraps the raw root in a struct so both root kinds can be
compared without losing unknown content.

The [builder-erasure cases](../tests/loaded_view_casts/erasure.rs) exercise all 15
accepted mutable cast layouts through `into_any_struct()` / `into_any_list()` and
raw edits, comparing with compiled Rust. Thirteen also match C++ dynamic-to-raw
constructors and mutations. Two inline pointer projections stay native-only
because of the pinned C++ offset defect. Additional checks cover void/byte/word
encodings, empty and null nested lists, malformed children, whole-parent group
storage, reborrows, unknown fields and retained capabilities. Erasure output is
recorded as `erasure-*.out` with counts in `erasure-summary.txt` beside cast logs.

Three additional borrowing doctests cover message lifetime and exclusive mutable
struct/list reborrows. Run them with
`cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc schema_loader::dynamic::cast`.

The nine portable tests join Linux/macOS/Windows smoke CI; the C++ comparison
runs on Linux. All remain part of `cargo test --workspace`. Three compile-fail
doctests check loader/message lifetimes and exclusive mutable access:
`cargo test --locked --manifest-path vendor/capnp/Cargo.toml --doc get_as_loaded`.
C++ inputs, outputs and counts stay in `target/verification/loaded-view-casts/`.

## Optional compatibility codecs and adapters

`capnp-compat` is a workspace member: `cargo test --workspace` includes its codec,
byte-stream, HTTP, WebSocket and JSON-RPC tests. The focused command is
`cargo test --locked -p capnp-compat` using the existing auditable Cargo wrapper.
Linux C++ tests build the pinned reference, compare canonical wire and exact
compact/pretty codec output, and run loopback ByteStream, HTTP upload/echo,
WebSocket upgrade, CONNECT and JSON-RPC exchanges. Generated C++ bindings,
executables and logs stay under `target/verification/compat/`.

The codec oracle compares 152 basic accepted input/format combinations, 66 shared
numeric/type rejections, and 72 ordered-assignment combinations (30 failure states).
Its shared portable corpus covers direct integer-to-float
rounding around Float32 half-way points in decimal/hex/octal, both signs, integer
and floating-point zero, overflow/underflow, schema range/type errors and binary
whitespace. Portable tests separately assert exact float bits and reject positive
integers above UInt64 (a deliberate difference from C++ lexer wraparound).
The focused commands are `cargo test --locked -p capnp-compat --test codecs` and
`cargo test --locked -p capnp-compat --test cpp`; summaries are in
`target/verification/compat/codecs-summary.txt`.

The [wrapping corpus](../crates/capnp-compat/tests/common/text_wrapping.rs) adds
25 accepted values (checked in compact and pretty modes) and 23 rejections. Cases
cover structs/groups/unions, reordered declarations, defaults, branded fields,
list elements, numeric boundaries, Unicode and controls, plus rejection of
ambiguous literals and two-level wrapping. The native suite compares implicit
inputs with explicit equivalents and checks replacement into existing messages,
group defaults and non-UTF-8 bytes. The C++ fixture generates pinned bindings in
`target/verification/compat/codec-generated/` to exercise the public typed orphan
overload. Existing-root decoding remains restricted to struct expressions.

The [assignment corpus](../crates/capnp-compat/tests/common/text_assignments.rs)
adds 36 cases, each compared with C++ in compact and pretty modes. It covers
repeated scalar, pointer and list fields, nested assignments, successive union
selections, repeated group initialization, defaults and inactive union storage.
Pre-populated destinations check replacement across calls. Fifteen failing cases
compare the resulting canonical bytes and exact encodings, including failed slot
replacement, partial group mutation, unknown fields, syntax errors and failed
orphan decoding. Portable tests independently assert the expected logical values.
The C++ fixture captures expected decode exceptions before serializing the remaining
destination; input, seed, diagnostics and outputs use `assignment-*` artifact names.

The platform smoke jobs run `codecs`, `adapters` and `json_rpc` tests on Linux,
macOS and Windows. Native C++ comparisons remain in Linux full verification.
See the [crate contracts and limits](../crates/capnp-compat/README.md) for executor,
framing, cancellation and dependency boundaries.

## Generated runtime paths

`cargo test --locked -p reproto --test capnp_root` creates two temporary
downstream crates and compiles and runs 16 generated configurations. It uses
both the native Rust frontend and the installed `capnp` compiler through
`CompilerCommand`, then checks default, renamed, re-exported and bootstrap
runtime paths with and without the optional field API. Imports, nested lists,
defaults/constants, generics, groups and interfaces compile under each path;
roundtrips also exercise reflection, projections and native values.

The default control is isolated from the override crate, which has no direct
dependency named `capnp`. The fixture reproduced unresolved runtime paths
before the fix. Nested builds use the auditable Cargo PATH, offline dependency
resolution and a shared `target/capnp-root-acceptance` directory. Logs are under
`target/verification/capnp-root/`. This test is included in `cargo test --workspace`
and in the existing Linux/macOS/Windows smoke jobs.

## Optional nightly Result-to-Promise propagation

`cargo test --locked --test tooling nightly_rpc_try_contracts -- --exact` runs
the `rpc_try` feature's eight regression tests and two doctests twice: once with
default `std + alloc` and once with `no_std + alloc`. It also checks the feature
without allocation. Install `nightly-2026-08-29` and activate the existing
auditable Cargo PATH first. This focused driver is part of `cargo test --workspace`
and Linux platform smoke CI; it adds no platform or general feature matrix.

The checks cover immediate validation, error conversion and metadata ownership,
borrowed/non-Unpin output, lazy deferred work, cancellation and compile-time
rejection of direct `promise?`. The nightly feature permits `Result?` inside a
promise-returning function; asynchronous propagation remains `.await?`.
Logs are written under `target/verification/tooling/rpc-try/`. During full LLVM
verification, the driver uses the existing LLVM-matched coverage toolchain and
records its profiles alongside the other standalone crate tests. This driver
also runs strict Clippy for the runtime and its test targets with `rpc_try`.

`cargo test --locked --test tooling capnp_runtime_lints -- --exact` runs the
standalone runtime's library and test targets through Clippy with `-D warnings`
on the stable toolchain. This separate gate is necessary because workspace
Clippy with `--no-deps` does not lint excluded path dependencies. Platform CI
runs the stable command directly on Linux, macOS and Windows; Linux also runs
the nightly driver. Install the nightly `clippy` component for that driver.
Default lint logs are under `target/verification/tooling/capnp/clippy.log` and
optional-feature lint logs under `target/verification/tooling/rpc-try/clippy.log`.

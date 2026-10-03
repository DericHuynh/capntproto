> Historical development record, archived 2026-10-01. Commands, counts,
> feature status and security descriptions below belong to earlier snapshots.
> Use the [current guide](../wiki/Correctness.md) for supported behavior and commands.
> Unbundled local run artifacts are not current verification evidence.

# Correctness and testing roadmap

Transport migration note: older private-handshake simulation entries below describe
historical pre-TLS evidence. Current validation is documented in
[native transports](../wiki/Native-Transports.md) and [testing](../wiki/Testing.md).


Source audit: **2026-09-23**. Scope: the Rust Cap'n Proto runtime and generator,
the custom quiche/Native transport, capability handoff, and supporting storage.

This is a list of potentially useful engineering additions, not a requirement to
adopt every named tool. Choose one suitable tool where alternatives overlap.
The [release checklist](../wiki/Release-Acceptance.md) owns release requirements; the
[C++ parity checklist](../wiki/Cpp-Parity.md) owns implementation parity. No collection
of tools guarantees whole-system correctness. Each result must state its
properties, bounds, environment and assumptions.

`[x]` identifies an existing foundation within the stated scope. `[ ]` identifies
proposed or incomplete work, including extensions to an existing capability.
Existence of tests is not evidence of a fresh passing run. Use Rust/Cargo for
first-party test orchestration, including wrappers around TLC and external tools;
do not introduce Python test runners. Keep TLA+/TLC as the protocol model checker.

## Existing foundations to preserve

- [x] Lifetime-bound readers, builders and orphans, checked arena identity and
  consuming ownership operations. Sensitive reader lifetime handling is
  centralized in `CheckedStructReader`; descendants remain lazily validated.
- [x] Restricted construction of native `AuthenticatedSession` handles and
  explicit route lifecycle states with cancellation ownership.
- [x] Cargo unit/integration tests, doctests, custom compile-pass/compile-fail
  tests, formatting and Clippy gates.
- [x] Bounded TLA+ safety/liveness checks, model fault controls, graph-bound
  regression corpora and fresh graph-edge trace replay against Rust.
- [x] Pinned C++ differential comparisons and separate bilateral interoperability
  tests for behavior those peers implement.
- [x] Partial-IO, disconnect, cancellation and storage fault injection, including
  isolated process crashes; targeted Valgrind ownership checks.
- [x] Toolchain/dependency pins, source provenance, default-feature builds,
  downstream consumer checks and source-bundle validation.
- [x] Inherited QuickCheck serialization tests, selected in-memory Native packet
  tests and real UDP/Native/RPC integration tests.

Evidence and commands: [TESTING.md](../wiki/Testing.md),
[architecture](../wiki/Architecture.md), [tooling tests](../../tests/tooling.rs),
[trace harness](../../test-support/src/verification/exploration.rs).

## Recommended implementation order

| Priority | Work | Completion evidence |
|---|---|---|
| First | Adapt applicable quiche regression fixtures to Native | Transport tests reach their intended assertions under the supported Native feature set; exclusions are explicit and justified. |
| First | Establish a reproducible packet simulation boundary | Fixed seed and event log reproduce the same execution using controlled clocks, entropy and delivery. |
| First | Add structured property testing and Native-aware fuzz targets | Tests reach authenticated and handoff states, reject malformed inputs, retain minimized regressions and report their actual coverage. |
| First | Qualify unsafe boundaries with Miri and native memory checks | Maintained, bounded jobs cover the custom reader/orphan, mmap and descriptor paths with documented exclusions. |
| Next | Measure coverage and mutate Rust implementation code | Reports identify untested branches; tests detect selected implementation faults, not merely faulty model variants. |
| Next | Explore async cancellation and notification schedules | Reproducible schedule tests cover route installation, wakeups, shutdown and cleanup. |
| Parallel | Review cryptographic composition and strengthen authentication APIs | Explicit threat model, specialist findings and regressions; authentication evidence is carried by trusted handles. |
| Later/selective | Implementation proofs and wider platforms | Focused proofs or platform runs establish named properties without overstating their scope. |

Implementation started **2026-09-23**:

- [x] **Native peer fixtures and an inherited regression gate.** Shared transport
  fixtures now configure distinct, reciprocally pinned identities and `reproto/2`.
  Repeated certificate setup is consolidated without changing transport limits.
  `quiche_native_regressions` runs **1,104 passing cases** with real cryptography,
  including Cubic/BBR2, streams, datagrams, key updates, CIDs and migration.
  H3 framing tests use the custom transport as a test substrate; this does not
  establish HTTP/3 interoperability. Certificate, TLS session/0-RTT and TLS ALPN
  negotiation tests are excluded by backend configuration.
- [x] **Repair Native handshake completion.** The inherited tests exposed retained
  Initial keys and a server address that never left anti-amplification limits.
  Authenticated confirmation now retires Initial state and verifies only the
  original address tuple. Native regressions cover a 32 KiB server response,
  forged confirmation, replay and confirmation from a different address.
  `NativeHandshakeConfirmation` now checks **242 states**, replays **391 edge prefixes**
  through real protected packets, and rejects **seven model faults**. The extension
  covers post-IK stream data arriving before HANDSHAKE_DONE, with authentication
  and duplicate-delivery checks within that connection.
- [x] **Finish the remaining inherited adaptations (2026-09-24).** All **59 formerly
  pending cases** now run. Native-specific handshake stages, amplification-budget
  saturation, sent-packet ACK ranges, BBR window-relative assertions, PMTU probe
  scheduling and stream-frame ordering reach their intended checks. The obsolete
  exclusion inventory is removed. The Cargo gate runs the complete Native library
  inventory and rejects failures, ignored cases, filtered cases or missing critical
  test families. This qualifies that feature set, not the entire quiche workspace.
- [x] **Reproducible two-peer packet simulation (2026-09-24).** A private test
  scope controls quiche's protocol/recovery clock, packet randomness and Snow
  ephemeral keys. The simulator calls the real connection API and records inputs,
  protected datagrams, pacing deadlines, timeout effects and delivered stream bytes.
  **16 schedules** covering mutual TLS with optional admission secrets and Cubic/BBR2 reproduce byte-for-byte across
  two processes, including lost initiator/responder flights, stream loss, corruption,
  duplication and reordering. Artifacts include the seed, configuration, toolchain
  and executable hash; saved event sequences can be replayed through Cargo.
  `NativeHandshakeRecovery` checks **17 states**, **64 native replays** (16 edge
  prefixes in four configurations), four model fault controls and fair recovery.
  This is a bounded transport foundation. Runtime executor/socket integration,
  handoff entropy, broader network histories and lifecycle shrinking remain open.
- [x] **Bounded migration and restart simulation (2026-09-24).** Packets retain
  quiche's actual source/destination addresses. Sixteen additional schedules
  partition an alternate path through a probe retry, reject corrupted validation
  responses, migrate and transfer data with the original path blocked, then replace
  both endpoints and reconnect. Saved old STREAM ciphertext must remain harmless
  even when the new connections reuse the old active CIDs. Distinct generation
  payloads detect old bytes entering new streams. All **32 schedules** reproduce
  across independent processes. `NativePathLifecycle` checks **44 states**, **172
  native replays** (43 edge prefixes in four configurations), six model fault
  controls and bounded fair recovery. Replay covers stale packets both before and
  after fresh stream completion. These are connection-object restarts, not OS
  listener/process recovery or durable exactly-once RPC guarantees.
- [x] **Generated packet properties and bounded shrinking (2026-09-24).** Pinned
  Proptest generates **128 cases** across both controllers, both Native modes and
  fault windows starting before/after authentication. Up to twelve decisions
  select queued packets, delay/reorder, drop, corrupt or duplicate them, with at
  most two destructive losses followed by reliable recovery. The existing exact
  stream-prefix/FIN oracle checks every action; every successful event log replays.
  The Cargo gate checks each treatment was exercised in every configuration and
  compares full-trace hashes across independent processes. Reduced failures save
  standalone JSON inputs and final packet logs; a versioned seed corpus and a
  synthetic shrink control exercise retention and reduction. Replay now checks
  the required connection generation, so old completed streams cannot satisfy a
  truncated restart trace. This is bounded transport property testing, not yet
  runtime/handoff command generation or coverage-guided fuzzing.
- [x] **Native-aware fuzz smoke gate (2026-09-24).** Two first-party libFuzzer
  targets retain real mutual TLS with optional admission secrets cryptography and exercise both peer directions and
  congestion controllers. One mutates datagrams before/after authentication; the
  other varies authenticated stream data, write/read boundaries, reordering,
  duplicate delivery and corrupted copies. Ordinary Cargo tests generate **131
  packet seeds and 258 stream seeds**, including maximum-size inputs, and require
  rejection of bad authentication tags followed by successful original delivery.
  `cargo test --test native_fuzz` runs **1,024 AddressSanitizer trials per target**,
  checks the crypto-bypass build prohibition, exercises artifact replay and records
  toolchain, source/lockfile/binary hashes and coverage feedback. Source bundles
  include the harness and exclude generated corpus/artifacts. These harnesses
  retain production entropy/clocks; semantic input replay is not byte-exact packet
  replay. Longer campaigns, handoff/RPC fuzzing and broader memory checks remain open.
- [x] **Serialization fuzzing and packed-read regression (2026-09-26).**
  `capnp_framing` checks packed/unpacked messages across fragmented and contiguous
  I/O, allocating and caller-buffer readers, and consecutive messages. An
  independent framing oracle checks unpacked segment contents and exact message
  boundaries, including flat-slice readers. `capnp_pointers` mutates raw messages
  and independent fixtures for structs, every list encoding, far/double-far
  pointers, cycles, invalid sizes and capability indices. It exercises bounded
  traversal, schema-free views, copying, structural equality and canonicalization;
  serialized indices without a capability table cannot yield a client.
  **1,478 framing and 1,754 pointer seeds** run through ordinary Cargo tests and
  the same libFuzzer entry points. The Cargo gate adds **16,384 ASan trials per
  serialization target**, source/binary hashes and saved-input replay. The seed
  matrix exposed a packed no-allocation reader rejecting valid zero runs across
  segment lengths; the reader now consumes the complete remaining table block,
  with a four-byte fixture and fragmentation/truncation regression. This covers
  bounded serialization behavior; schema loading, RPC dispatch, bound capability
  lifecycle, handoff and storage-recovery fuzzing remain open. The extended Cargo
  campaign passed **262,144 trials per serialization target**, plus 1,024 per
  Native target (**526,336 total**), without crash artifacts. Its native regressions,
  artifact replay, crypto-bypass rejection, formatting/lints and feature matrix
  also passed. This is one bounded campaign, not an exhaustive safety claim.
- [x] **Malformed schema requests and reload regression (2026-09-26).**
  `capnp_schema` adds **38 named semantic cases** and **13,321 native seeds** for
  raw, mutated and truncated requests. Cases cover layout offsets, names/orders,
  ordinals, union tags, defaults, dependency kinds, interface/group cycles,
  generic brands, future discriminants and type-depth limits. Failed batches
  must preserve existing definitions and the initial typed stub, including when
  an earlier node staged an upgrade; a subsequent valid load must succeed.
  Accepted requests retain their selected versions on replay and support bounded
  reflection/default reads after the input reader is destroyed. The sweep found
  that future field types with unknown default tags loaded once but failed on
  replay. Compatibility now keeps those defaults opaque; differing future types
  and invalid defaults for known types still fail. The shared native/fuzz seed
  and root schema-loader regression retain this case. The Cargo gate includes
  **16,384 ASan trials** for the new target, saved-input replay and source hashes.
  Qualification passed **51,200 total trials** across all five targets, ten native
  harness tests and **37 root schema/reflection/presence tests** with no crash artifacts.
  This covers bounded request loading, not arbitrary schema histories, dynamic
  message mutations, RPC dispatch, handoff or storage recovery.
- [x] **Structured RPC lifecycle fuzz inputs (2026-09-26).** `rpc_lifecycle`
  establishes a real bootstrap and two exported objects, then interprets up to
  **32 commands** for creation/re-export, calls, reference retention/release,
  pending calls, completion and cancellation. State-relative selectors preserve
  valid setup as inputs shrink. An independent reference/pending-call model checks
  exact dispatch, return values, object destruction and question-ID reuse after
  each command. Four I/O fragmentation profiles exercise the production two-party
  byte-stream reader, queue and RPC dispatcher. Eight endings cover orderly
  cleanup, invalid releases, duplicate questions, invalid Returns and raw/mutated/
  truncated frames; every run checks capability and pending-call teardown.
  **202 seeds** share native execution, libFuzzer and saved-input replay. Native
  controls require every command/ending and actual lifecycle effects, reproducible
  observations, executable command deletion/truncation, and detection of wrong
  ownership, dispatch and pending-call observations. The Cargo gate includes
  **16,384 ASan trials** for this target and hashes the runtime, async framing and
  generator sources. Qualification passed **67,584 total ASan trials** across all
  six targets, thirteen native tests and six saved-input replays with no crash
  artifacts. The gate now records actual trial totals and accepts libFuzzer's
  seed-initialization overruns while still requiring its full requested budget.
  This is deterministic, in-memory RPC testing with no
  authentication claim; the separate Native targets retain real cryptography.
  Pipelining, promise resolution/Resolve messages, general schedule exploration,
  handoff and storage commands remain open.
- [x] **Bounded RPC pipeline and exported-promise fuzz histories (2026-09-26).**
  The shared `rpc_lifecycle` grammar now includes six promised-answer pipeline
  histories and six exported-promise histories. Real pending calls and queued
  children check early parent Finish, child cancellation, rejected parents,
  invalid capability-field transforms, successful/failed outgoing Resolve,
  post-resolution calls and separate promise/result reference ownership.
  Releasing an unresolved promise must preserve queued calls or destroy its
  resolver when no callers remain. **592 seeds** cross eighteen histories,
  four I/O profiles and eight endings; a mixed saved-input replay covers every
  new mode while another call remains pending. Native controls compare repeated
  and differently fragmented runs, preserve executable command deletion and
  truncation, and reject six corruptions of observed pipeline/Resolve output.
  A retained malformed-input regression requires duplicate application promise
  tokens to return errors without replacing the fixture's original resolver.
  Qualification passed **68,361 ASan trials** across six targets, including
  **16,384 RPC trials**, all sixteen native tests and six saved-input replays,
  with no crash artifacts. Formatting, lints, source/lockfile checks and the
  crypto-bypass rejection also passed. Sanitizer qualification ran outside the
  process-tracing sandbox so LeakSanitizer could complete its final check.
  These are bounded compound commands, each settled before the next command;
  incoming Resolve, nested/chained promises, arbitrary scheduling, authenticated
  composition, handoff and storage fuzz commands remain open.

Commands, assumptions and model bounds: [TESTING.md](Testing-History.md#native-transport-correctness).
The inherited [packet fuzz target](https://github.com/DericHuynh/capntproto/blob/7b0a5aafb6a58f6216979c7c9517739a5687428e/vendor/quiche/fuzz/src/packet_recv_server.rs) still
uses TLS configuration and a BoringSSL-specific randomness reset. The separate
[first-party fuzz package](../../fuzz/Cargo.toml) tests the Native transport with real
cryptography. Runtime-wide deterministic injection and the other roadmap items
remain open. Keep TLS 1.3 with pinned mutual authentication / TLS admission and the existing
security boundary intact while extending these checks.

## API and type design

- [x] **Opaque route generations (2026-09-25).** `RouteGeneration` replaces raw
  `u64` values throughout route allocation, task ownership and observation. Only
  the network allocates nonzero generations; callers explicitly project numbers
  for diagnostics. Allocation uses the final `u64` value and then fails with
  `Overloaded`, without wrapping, reusing IDs or starting another dial. Existing
  routes remain usable after exhaustion. The API disallows numeric construction,
  implicit conversion, arithmetic, default values and deserialization. Generations
  are local to one network; owner-identity checks still protect stale cleanup.
  `RouteGeneration.tla` checks **39 states / 78 Rust edge-prefix replays**, four
  fault controls and fair exhaustion over the last three IDs/two observers.
- [x] **Typed RPC identifiers and tables (2026-09-25).** Distinct private
  `QuestionId`, `AnswerId`, `ImportId`, `ExportId` and `EmbargoId` replace integer
  aliases throughout the runtime, including Join and third-party answer adoption.
  `LocalTable` and `PeerTable` enforce the key domain and allocation role. Wire
  conversion is explicit and follows the local endpoint's perspective; zero and
  all 32 wire bits are preserved. Sparse high/adopted insertion is restricted to
  questions. Private allocator storage prevents unchecked free-list mutation;
  absent removal cannot recycle a slot twice, and adoption cannot overwrite an
  existing owner. **50 compiler contracts** exercise the exact private production
  modules. `RpcIdTables.tla` checks **1,041 states / 11,304 production edge-prefix
  replays**, with four required model fault detections. Native tests also cover
  high-ID wrap and reserved-range boundaries. See [testing scope](Testing-History.md#rpc-identifiers-and-table-ownership).
- [x] **Immutable, typed persistent descriptors (2026-09-25).** Distinct nonzero
  `ObjectKind`, `ObjectId` and `ObjectGeneration` replace interchangeable numbers
  in `Descriptor`, the factory registry, ORM factory generation and state cache.
  Private fields preserve checked identifiers and `Rights` after construction;
  typed getters provide inspection. Numeric decoding rejects zero, invalid rights
  and missing/duplicate/unknown fields while retaining the existing ledger format.
  Grants now share these checked object domains end to end. **42 external compiler
  contracts** reject domain swaps, mutation, implicit conversion and ignored
  descriptors. Boundary tests and `PersistentFactoryBinding.tla` (**293 states /
  564 real bind/save/restore edge-prefix replays**, five model fault controls)
  check factory dispatch, generation, object selection and read/write attenuation.
  These are metadata types, not authority or realm brands; trusted binding and
  factory authorization remain required. See [testing scope](Testing-History.md#persistent-descriptor-bindings).
- [x] **Shared object authority domains (2026-09-25).** `authority::ObjectId` and
  `ObjectGeneration` now flow through root grants, delegation, persistence,
  ORM state, handoff context binding and durable-upload metadata without losing
  their types. ORM state keeps its object/store binding private and exposes typed
  inspection; callers cannot retarget a cached schema by replacing those fields.
  Storage and wire boundaries use explicit conversions. Zero IDs/generations
  cannot enter grants or ORM state; journal decoding checks the same types.
  **17 new compiler contracts** and `ObjectAuthorityBinding.tla` (**177 states /
  372 real ORM edge-prefix replays**, five model fault controls) cover delegation,
  wrong-object binding, attenuation and revocation. Full-width identifiers and
  malformed/substituted upload journals have native regressions. Generations are
  trusted host metadata; this does not add an ORM incarnation registry or store
  branding. See [testing scope](Testing-History.md#shared-object-authority-domains).
- [x] **Typed storage keys and immutable snapshot coordinates (2026-09-25).**
  `storage::ObjectKey` now keys every Store operation, batch, index and recovery
  record; `durable_bulk::JournalId` separates upload journals from target objects.
  Explicit conversion connects authority IDs to storage keys and journal IDs to
  their slots. Zero and every other `u64` key remain valid. Snapshots expose
  read-only object/revision metadata alongside
  their retained mapping. The binary storage format is unchanged. **34 compiler
  contracts** reject mixed domains and snapshot relabeling. `StorageObjectKeys.tla`
  checks **441 states / 1,567 real Store edge-prefix replays**, with six model
  fault controls for key isolation, snapshot coordinates/data and recovery.
  Native checks cover zero/full-width keys, snapshot lifetime and journal/target
  collision without writes. Keys are not Store-instance brands or authority.
  See [testing scope](Testing-History.md#storage-keys-and-snapshot-identity).
- [x] **Store-bound publication cursors (2026-09-25).** Numeric wire/checkpoint
  positions enter through `Store::publication_cursor`, which validates publication
  membership and retention. Immutable `PublicationCursor` values bind the issuing
  open Store, object and position; reads reject another Store or a reopened
  instance and recheck retention and writer health. Reading preserves the input
  for retry and returns a `Publication` pairing the snapshot with its next cursor.
  Moves and history-preserving compaction retain ownership; cursors alone hold no
  file locks. ORM history now retains this binding across waits. **25 external
  compiler contracts** and `PublicationCursor.tla` (**493 states / 3,218 real Store
  edge-prefix replays**, seven model fault controls) cover immutable bindings,
  foreign/reopened owners, drafts, gaps, retry and advancement. Native regressions
  also exercise full-width keys, snapshots after compaction and poisoned writers.
  Numeric wire/disk formats remain unchanged. These handles are local validation
  evidence, not capability authority or globally unique durable identities.
  See [testing scope](Testing-History.md#publication-cursor-ownership).
- [x] **Typed revisions and checked revision state (2026-09-25).** `Revision`
  separates storage counters from addresses, authority generations and counts
  throughout Store APIs, indexes, snapshots, history floors/cursors, ORM
  notifications and durable-upload metadata/checkpoints. `INITIAL` explicitly
  names zero (no entry); `MAX` remains readable and publishable, and
  `checked_next()` rejects exhaustion. Wire/disk/JSON conversion is explicit and
  retains the numeric encodings. `Revisions` has private fields and checked
  construction, preserving publication <= head without a deserialization bypass.
  **31 new compiler contracts**, numeric/serialization boundary tests and
  `RevisionBoundary.tla` (**965 states / 7,980 real Store edge-prefix replays**,
  seven model fault controls) check compare failures, exhaustion, atomic batch
  rejection, compaction and reopen at the final two `u64` revisions. Recovery
  rejects wrapped single/batch records without modifying disk. These counters
  remain object-local metadata; they do not confer ownership, entry existence
  or publication evidence. See [testing scope](Testing-History.md#revision-domains-and-exhaustion).
- [x] **Checked bulk configuration (2026-09-25).** `bulk::Config` has private,
  immutable limits, checked construction and read-only getters. Direct, JSON and
  RPC imports share validation; JSON cannot bypass it or silently ignore unknown
  fields. `Receiver::new` consumes checked limits and returns its pair directly.
  Durable metadata decoding preserves validation without redundant caller checks.
  **13 compiler contracts** protect construction/mutation boundaries and required
  result handling. `BulkConfigBoundary.tla` checks **9,421 states / 70,275 native
  edge-prefix replays**, with seven model fault controls. Native tests cover
  full-width bounds, malformed JSON, hostile framed RPC advertisements and invalid
  journals rejected without writes. The limits remain transfer metadata, not
  authority or a process-memory quota. See [testing scope](Testing-History.md#checked-bulk-configuration).
- [x] **Checked realtime configuration (2026-09-25).** Reliable and datagram
  snapshot streams share immutable clock/resource settings with checked
  construction and read-only accessors. Wire imports validate UTF-8, byte length,
  individual limits and the combined payload budget before issuing a value;
  receivers consume checked settings without repeating fallible validation.
  Datagram binding/import retains its smaller snapshot limit. **16 compiler
  contracts** protect construction, mutation and required result handling.
  `RealtimeConfigBoundary.tla` checks **15,108 states / 19,811 Rust edge-prefix
  replays**, with eleven model fault controls. Native tests cover full-width
  bounds, exact 64 MiB budgets, malformed framed RPC advertisements and datagram
  limits. Configuration does not establish clock synchronization, authorization
  or a process-wide memory bound. See [testing scope](Testing-History.md#checked-realtime-configuration).
- [x] **Discovery generation and resolved-binding types (2026-09-25).** Nonzero
  `DiscoveryGeneration` separates publication/renewal/advertisement metadata from
  RPC routes, object incarnations and storage revisions. Administrative creation
  uses `None`; replacement takes `Some(generation)`. Wire projection/import is
  explicit. `Resolved` preserves its validated host, recipient, endpoint,
  context, provider, generation and expiry behind read-only accessors; connection
  setup consumes it and retains recipient/deadline/authentication checks.
  **30 compiler contracts**, full-width wire and expiry tests, and real Native/RPC
  connections through captured bindings after rotation cover these boundaries.
  `DiscoveryGenerationBoundary.tla` checks **393 states / 1,498 native edge-prefix
  replays**, with six model fault controls for exhaustion, reuse, stale ownership
  and relabeling. Generations remain directory-local metadata; retained results
  are not authentication or revocation watches. See [testing scope](Testing-History.md#discovery-generations-and-captured-bindings).
- [ ] **Other distinct identifier types.** Audit remaining Store-instance
  ownership, identity and Join/transport primitives where distinct domains matter.
  Extend compile-fail cases and connection ownership contracts as needed.
  Newtypes alone do not prevent stale IDs from reaching a reused slot; retain
  generation and ownership checks.
- [x] **Window-bound bulk reservations (2026-09-25).** `CreditWindow::reserve`
  returns a private, immutable `Reservation`, retained with its pending RPC.
  Explicit `sequence()` projection supplies the wire number; `settle(&reservation)`
  checks issuing-window identity before releasing credit. `Settlement` names
  first release versus duplicate settlement. Reservations cannot be cloned,
  constructed from numbers or deserialized; handles and settlement outcomes are
  `must_use`. Dropping a handle keeps credit charged, and replacing a window does
  not authorize an old handle against its reused numeric sequence. Both types
  retain `Send`/`Sync`. **16 external compiler contracts**, native move/replacement
  and full 65,536-sequence boundary tests, and `BulkReservation.tla` (**1,749 states /
  8,866 production edge-prefix replays**, five model fault controls) cover this
  boundary. This is local accounting, not proof of remote execution; the sender
  separately checks replies and preserves its sticky error/cancellation rules.
- [x] **Immutable grant bindings (2026-09-25).** `Grant` now keeps object,
  generation and holder private and exposes value getters. A clone preserves all
  bindings; changing the holder requires rights-checked delegation. Root issuance
  remains an explicit trusted service responsibility. Grants are `must_use` and
  remain local to one thread with shared revocation lineage. ORM, durable bulk,
  persistence and handoff consumers use the immutable API. Compiler checks reject
  binding mutation and deserialization; exhaustive attenuation tests and existing
  revocation/model replay retain the runtime authorization checks.
- [x] **Checked identity construction (2026-09-24).** `Identity` has private,
  immutable key fields, checked pair/private-key imports, a public-key accessor
  and public-only diagnostics. Its owned scalar is zeroized on drop; temporary
  Snow derivation state is overwritten before release. Three compile-fail
  doctests reject direct construction, mutation and secret access. Independent
  RFC 7748 vectors and real mutual TLS with optional admission secrets sessions check derivation and authentication.
  `NativeIdentity` explores **75 states / 190 edge prefixes**, replayed against
  Rust with **12 real handshakes** and five required model fault detections.
  Configuration ownership survives dropping its source identity; this does not
  establish complete memory erasure, key entropy or cryptographic security.
- [x] **Authentication evidence in handoff APIs (2026-09-24).** Application
  `handoff::serve()` now obtains an `AuthenticatedSession` with the introduction's
  pinned host, recipient, PSK and context before installing a private bootstrap on
  that session's stream. Raw-byte `Introduction::accept()` and public `Acceptor`
  construction are removed. Introduction bindings are immutable; replacing the
  complete value during/after authentication cannot rebind the captured ID.
  A `Serving` owner cancels RPC and transport together. Three compile-fail cases,
  real UDP rejection/cancellation/revocation tests, and `AuthenticatedHandoff`
  (**158 states / 236 native edge-prefix replays**, six model fault controls)
  cover this boundary. The explicit old-route fence and trusted grant/ticket
  provisioning contract remain; this is not a cryptographic or restart proof.
- [x] **Typed handoff and stream-opening state (2026-09-24).** `HandoffState`
  now has private counters, a named `HandoffPhase`, derived acceptance and no
  deserialization bypass. `NativeStreamGate` uses explicit handshake, preface,
  ready and failed states with a typed initiator/responder role. Callers use
  observation methods; only verification adapters project phases to model
  numbers. Five compile-fail cases protect construction/mutation boundaries.
  The production guard graph still equals TLC (**324 states / 927 transitions**),
  and existing authenticated handoff/stream-gate traces exercise the new types.
- [x] **Reserve handoff drain capacity (2026-09-24).** Enqueue now checks pending
  plus previously drained calls before admitting work, preventing a pending call
  that could never complete after cumulative counter exhaustion. Transition
  results are `must_use`. `HandoffCounterBoundary` checks the last two counter
  slots: **597 states / 2,620 Rust edge-prefix replays**, five model fault controls,
  and unchanged-state assertions on rejection. This is bounded counter-policy
  evidence, not runtime-wide invocation accounting or an arbitrary-bound proof.
- [x] **Realtime record states and immutable snapshots (2026-09-25).** Queued
  records own their required payload and receipt waiters; finished records retain
  metadata plus an outcome, and cancellation-before-offer uses distinct canceled
  or closed tombstones. There is no unfinished record without a payload or
  terminal record retaining waiters. Admission chooses a complete variant before
  insertion, and completion extracts notifications for delivery after the state
  borrow ends. Published snapshot coordinates/data are private and read-only,
  including owned clones. **14 compiler contracts** protect that API.
  `RealtimeReceiptLifecycle.tla` checks **12,761 states / 41,966 Rust edge-prefix
  replays**, with seven model fault controls. Native regressions cover payload
  release, waiter exhaustion, reentrant notification, retained values and late
  observation of old application results after a newer publication. Existing
  realtime/datagram/fragmentation traces and Native RPC tests still pass.
  See [testing scope](Testing-History.md#realtime-record-and-receipt-lifecycle).
- [x] **Packet-bound reservations and reentrant datagram admission (2026-09-25).**
  A native regression exposed duplicate sequence allocation when a queue waker
  submitted through a sender clone before the outer offer advanced its counter.
  The sender now reserves its complete packet batch, commits the sequence, then
  publishes without subsequent counter writes. The private move-only reservation
  borrows its queue and immutable packets, releases capacity on drop and can send
  once; thirteen compiler contracts protect those boundaries. Reentrant offers
  cannot allocate the outer sequence again; close remains sticky and retries
  retain their original sequence.
  `DatagramReentry.tla` checks **3,021 states / 3,192 Rust edge-prefix replays**,
  with eight model fault controls. Native tests also cover queue closure during
  publication and reservation cancellation. Existing fragment/batch models and
  real Native datagram tests remain separate qualification. Submission establishes
  local queue admission, not remote delivery. See [testing scope](Testing-History.md#packet-reservations-and-reentrant-datagram-admission).
- [x] **Bulk and stream progress representations (2026-09-25).** Bulk receiver
  states own staging, published bytes or terminal errors. Repeated cancellation
  retains the first error and cannot revoke completion. RPC send/receive enums
  retain partial progress in independent reusable buffers and reject mutable
  buffer access while data remains pending. `RpcStreamProgress.tla` explores
  **1,306 states / 5,301 Rust edge-prefix replays**, with three fault controls for
  lost write progress, early FIN and premature half-close. Real Native packet tests
  cover native prefaces, bidirectional bytes and partial writes under small
  stream windows. See [testing scope](Testing-History.md#rpc-stream-progress-and-buffer-ownership).
- [ ] **Further explicit state representations.** Replace remaining public numeric phases and
  loosely related flags with enums or typestate where doing so removes invalid
  combinations. Use consuming transitions for single-use local owners when
  useful. Serialized-token replay still requires protocol state and binding.
- [ ] **Consistent numeric and completion contracts.** Audit checked arithmetic,
  bounds, zero-validity rules and `#[must_use]` on security/lifecycle outcomes.
  Distinguish accepted, queued, delivered and application-completed operations.
  Handoff counter admission, transition results, route-generation exhaustion and
  window-bound bulk settlement/exhaustion, immutable bulk/realtime limits, discovery
  generation exhaustion, storage revision exhaustion and datagram sequence
  commitment before queue wakeups are now checked; other
  protocol counters and completion contracts remain to be audited.
- [ ] **Enforced unsafe boundaries.** Apply `forbid(unsafe_code)` to protocol-only
  modules and explicit `unsafe_op_in_unsafe_fn` linting to unsafe modules. Document
  each unsafe abstraction's aliasing, lifetime, alignment and ownership contract.
  Identity, authority, application handoff/introduction, route generation/session
  ownership, RPC identifier/table modules, bulk credit ownership/configuration, shared object
  identifiers, discovery generation/resolved-result modules, storage/journal key
  and publication-cursor modules, realtime configuration/record/snapshot modules,
  transport packet reservations, persistent descriptors and pure
  transition modules now forbid local unsafe code; the broader audit remains open.
- [ ] **Expanded compile-time API tests.** Extend the existing compiler harness
  and doctests for new identity/state types, orphan ownership, `Send`/`Sync`
  expectations and generated APIs. `trybuild` is an optional replacement for
  orchestration, not an additional guarantee. Identity construction, field
  mutation and secret access, plus handoff authentication assertion, recipient
  mutation and bootstrap construction now have compile-fail coverage. Handoff
  state forgery/deserialization and stream gate construction/role checks are
  covered too. A separate external-consumer Cargo gate now checks **16 contracts**
  for immutable grants, opaque generations, local thread affinity and ignored
  grant results, with a positive control and exact Rust diagnostic codes. Six
  additional doctests cover grant mutation and generation construction/conversion.
  The same harness additionally checks **50 RPC contracts** by compiling the exact
  private modules, including all 20 directed ID mix-ups, wrong table keys/roles,
  restricted sparse allocation, private storage and explicit numeric conversion.
  A further **16 public bulk contracts** check opaque reservation construction,
  mutation, move ownership, must-use results, thread traits and removal of raw
  numeric settlement. Cross-window rejection remains a runtime identity check.
  **42 persistent-descriptor contracts** additionally cover all six directed ID
  swaps, immutable fields, typed factory/ORM entry points, separation from route
  generations, checked serialization and descriptor `must_use`.
  **17 shared-authority contracts** check typed grant/ORM arguments and getters,
  immutable ORM object/store bindings and local thread affinity.
  **34 storage contracts** cover typed Store/batch/journal entry points, explicit
  domain conversions, immutable snapshot coordinates and retained thread traits.
  **25 publication-cursor contracts** additionally cover checked construction,
  private owner/object/position bindings, paired snapshot/next-cursor ownership,
  explicit checkpoint projection, `must_use`, and retained `Send`/`Sync`.
  **31 revision contracts** cover typed expectations/results, separation from
  keys/generations/cursors, immutable checked state, numeric/Serde boundaries,
  explicit initial values and required handling of transition outcomes.
  **13 bulk-configuration contracts** cover checked construction, private fields,
  immutable sender limits, infallible receiver creation, `Send`/`Sync`, cloning and
  required handling of configuration/receiver results.
  **30 discovery contracts** cover the nonzero generation domain, typed
  administrative arguments, explicit absence, immutable resolved bindings,
  local capability ownership and required result handling.
  **14 realtime snapshot contracts** protect private construction and coordinates,
  immutable byte access, clone/Debug support, local thread traits and required
  result handling.
  **16 realtime configuration contracts** cover private settings, checked construction,
  read-only sender/domain access, absence of default/Serde bypasses, infallible
  receiver creation, thread traits and required handling of returned values.
  **13 packet-reservation contracts** compile the exact private transport module
  to check immutable queue/packet borrows, lifetimes, private fields, one-shot
  sending and required result handling. Runtime checks establish commitment
  before wakeups; the type alone does not enforce that call ordering.
  Broader generated/orphan API combinations remain open.

## Static analysis and memory safety

- [x] **Initial Miri gate.** Established 15 selected serialization/ownership
  regressions with pinned nightly-2026-08-29 (updated for Rust 1.97): aligned and
  unaligned features, strict provenance, Stacked Borrows (seed 0) and Tree Borrows
  (seed 1), plus native runs and a required freed-pointer rejection control.
  It covers checked readers, generated field views, local capability lifetimes,
  data-orphan resize/adoption, immutable external heap data, staged updates,
  scratch reuse and malformed wire pointers. The gate found and fixed invalid
  self-references in `CheckedStructReader`; cached metadata now borrows a stable
  private `Rc` owner only when producing a view. Logs, tool identities, test
  inventories and source hashes are retained under `target/verification/memory-safety`.
- [x] **Expanded Miri qualification.** `cargo test --test memory_safety` now runs
  23 tests: 46 native executions and 92 Miri executions. Added every scalar wire
  width, enums, nested pointer lists, text terminators, capability lists and
  unknown inline struct fields; generated list upgrades and group error/unwind
  rollback; owned schema replacement, failed batches, limits, native registration
  and loaded orphan authority. Results require unchanged source hashes throughout
  the run and matching native/Miri inventories. This covers selected operations
  at each boundary, not every schema graph or operation combination.
- [ ] **Further memory qualification.** Extend to recursive/branded schema graphs,
  full orphan-operation combinations, mmap, descriptors, networking and FFI.
  Cross-platform qualification remains separate. The default ownership matrix
  covers x86-64 Linux; the scheduled extension also interprets six wire tests on
  big-endian s390x with four additional seeds on each target.
- [x] **Additional CI checks configured (2026-09-27).** Allocation budgets run
  through the workspace suite and Linux PR job. Scheduled jobs add the bounded
  Miri endian/seed sweep, pinned cargo-careful native storage/descriptor checks,
  and cargo-llvm-lines reports. Workflow changes run actionlint/Zizmor; Lychee
  checks local documentation links with separate advisory external-link reports.
  Dedicated benchmarks include precompiled Gungraun serialization probes and
  validated instruction-count charts. Local checks exercised selected native and
  big-endian tests; the complete hosted workflows and remote measurements still
  require their first run. See [quality scope](../wiki/Quality-and-Benchmarks.md#focused-checks-and-tool-versions).
- [ ] **AddressSanitizer/LeakSanitizer runs.** Exercise native parser, runtime and
  descriptor lifecycles, including relevant native dependencies. Investigate
  both physical leaks and logical retention of tasks/capabilities. Native packet
  and stream fuzz targets now have bounded ASan runs; the broader surface and
  explicit leak qualification remain open.
- [ ] **MemorySanitizer where practical.** Cover uninitialized-memory-sensitive
  paths with correctly instrumented dependencies; document feasibility limits.
- [ ] **ThreadSanitizer where relevant.** Use for genuinely shared cross-thread
  state. It does not replace ordering or async cancellation tests.
- [ ] **Broader native memory checks.** Extend the existing targeted Valgrind
  gate when it covers a path that Miri or sanitizers cannot exercise.
- [ ] **Layout and platform qualification.** Add selected randomized-layout,
  32-bit, endian and supported-OS checks. Do not infer support from compilation
  alone, especially for mmap and ancillary descriptors.

Miri detects undefined behavior on explored executions; it does not prove every
safe use of an abstraction sound. External mutation of mmap backing storage also
remains outside a guarantee based only on Rust borrows and cooperative file locks.

## Testing methodologies

- [ ] **Stateful property testing with shrinking.** Use Proptest, QuickCheck or
  Bolero to generate valid and invalid command sequences for capability tables,
  promises, schemas, handoff and storage. Compare with an independent specification
  and retain minimized failing sequences. Existing serialization QuickCheck tests
  do not supply this runtime-wide coverage. Transport delivery decisions now use
  Proptest with bounded shrinking. The composed RPC handler histories below now
  cover retained capability counts and dispatch. The structured RPC fuzz target
  below adds bounded pending-call completion/cancellation and shrinking-compatible
  commands. Bounded compound pipeline and exported-promise histories now add
  cancellation/Resolve observations; broader promise graphs, handoff, runtime
  commands and storage remain open.
- [ ] **Coverage-guided fuzzing.** Add first-party targets for framing, pointers,
  schema loading, RPC dispatch, introduction tokens, Native handshake fragments,
  packet protection and storage recovery. Six first-party libFuzzer targets now
  cover bounded Native packets/streams, serialization framing/pointers and schema
  request loading, plus RPC dispatch and capability/pending-call lifecycles.
  Introduction tokens and storage recovery still need their own oracles and
  campaigns; broader RPC/schema histories and longer campaigns also remain open.
- [ ] **Structured and stateful fuzz inputs.** Generate valid initial state and
  authenticated sessions before corrupting later inputs. Use `arbitrary` or
  custom generators/mutators. Separate parser-only targets from targets that
  exercise real authentication and cryptography. The Native harness provides
  authenticated setup and shared native/fuzz oracles; pointer fuzzing also mutates
  independent valid/malformed wire shapes. Schema fuzzing starts with an existing
  definition and typed dependency, then mutates requests containing a valid
  upgrade prefix. RPC now has a bounded command grammar with real bootstrap/export
  setup before terminal corruption, exact ownership/dispatch observations during
  valid histories and cleanup checks afterward. Bounded promised-answer and
  exported-promise commands now cover outgoing Resolve and queued-call ownership.
  Stateful handoff, incoming Resolve, general promise graphs/schedules and
  composition with authenticated transport remain open.
- [ ] **Shared regression inputs.** Reuse minimized command/event sequences
  across ordinary Cargo tests, property tests, fuzzers and simulation where their
  observation contracts agree. Every discovered bug gets a regression.
  Packet properties now have standalone JSON replay, a versioned seed corpus and
  saved minimized inputs. Fuzz targets share native replay and seed matrices;
  the packed-table regression is retained in both the framing matrix and the root
  serialization tests. Future schema/default discriminants have shared replay
  seeds and a root loader regression. Sharing packet histories with fuzzers/runtime
  models remains open. Structured RPC seeds also share the native and libFuzzer
  entry point; command-deletion and truncated-input tests check their setup remains
  executable, and fixed inputs must reproduce their observations.
- [ ] **Expand independent differential testing.** Cover more malformed input
  and asynchronous behavior against supported C++ peers. C++ cannot be the oracle
  for this custom Native binding or operations its dispatcher does not implement.
- [ ] **Expand metamorphic properties.** Test framing/chunking invariance,
  schema-evolution preservation, equivalent safe representations and cancellation
  transformations with explicit expected semantics. Round trips alone are weak
  evidence because encoder and decoder may share a defect.
- [ ] **Independent vectors and fixtures.** Add full handshake, transport-key and
  handoff-binding vectors from an independent implementation or calculation.
  Retain primitive known-answer tests and malformed wire fixtures.
- [ ] **Optional snapshot testing.** Consider `insta` for generated source and
  diagnostics; review changes rather than automatically accepting snapshots.
- [x] **Scoped Rust mutation gate (refreshed 2026-09-25).** Pinned `cargo-mutants` runs
  against `authority` and the pure handoff/stream guards in a disposable source
  copy. Seven native tests catch **91 buildable mutations**; seven generated
  `Default` replacements fail to compile and are reported separately. These
  comprise **98 generated cases**; typed object/generation getters
  now reject the default-value replacement during compilation. No missed
  mutants or timeouts remain. The first run exposed **16 assertion gaps** in
  rights decoding, delegation, revocation waits and handoff observations. New
  tests exhaust all 256 rights bytes and 1,024 parent/child mask combinations,
  plus 48 revocation registration scenarios. The gate checks the complete mutant
  inventory, successful baseline, failure attribution, build/test outcomes and
  restoration of both source inventories. TLC faults are excluded from these
  Rust mutation runs. Commands and evidence: [TESTING.md](Testing-History.md#guard-mutation-and-coverage-evidence).
- [ ] **Extend implementation mutations.** Cover runtime authorization and
  cleanup, parser limits, storage and asynchronous route behavior beyond these
  three files. The selected mutation operators are not exhaustive defect models.
- [x] **Scoped LLVM coverage reports (refreshed 2026-09-25).** Pinned `cargo-llvm-cov`
  produces JSON/HTML and a source-bound summary for the same seven native tests,
  with default features disabled. Production line/region counts are **68/68 and
  86/86** for authority, **80/83 and 92/95** for handoff, and **39/39 and 39/39**
  for stream guards. Handoff test bodies are in a separate file. The uncovered
  drain-overflow defense is documented; no branch or MC/DC coverage is claimed.
- [x] **Complete Linux source coverage reporting (2026-09-26).** The quality
  collector inventories every bundled source file, merges Rust/C++ LLVM line,
  region, function and branch counters, using one default-feature workspace
  test invocation. Unmapped files are explicit N/A, never counted as covered.
  Per-file and aggregate regression gates complement the existing correctness
  controls; this does not claim 100% execution or MC/DC. Measurement, baseline
  review and generated evidence are described in [QUALITY.md](../wiki/Quality-and-Benchmarks.md).
- [ ] **Broader fault injection.** Compose partial IO, peer failure, cancellation,
  timeout, failed writes/syncs and resource exhaustion with active RPC operations.
- [ ] **Bounded-resource stress and soak tests.** Measure task/table/FD/buffer
  growth across repeated handoff, disconnect, reconnect and revocation, including
  hostile peers and stalled consumers.
- [ ] **Performance regression tests.** Use Criterion, Divan or a workload harness
  for latency distributions, allocations, throughput and recovery cost. Existing
  EAE/storage microbenchmarks do not qualify the complete RPC transport. The
  Linux loopback harness now compares Capn't Proto, C++ Cap'n Proto, gRPC and
  WebSockets over four payload sizes and five repetitions, retaining validated
  responses and raw latency samples. Different security/transport settings are
  explicit; allocation, concurrent throughput and recovery comparisons remain.
- [x] **Platform smoke and separate full quality workflows (2026-09-26).** Linux,
  macOS and Windows compile default features and run capability RPC smoke checks;
  Linux/macOS also smoke-test durable storage. The scheduled/manual full Linux
  job collects LLVM coverage around `cargo test --workspace`, including bounded
  models, Miri/mutations and fuzz controls. Manual performance jobs compile
  individual `cargo bench --no-run` targets and transfer them to a temporary
  dedicated CPU DigitalOcean droplet using GitHub Actions secrets. No feature
  combination matrix or local CI-runner timing is required. README artifacts
  identify each run's measured scope. Full coverage baseline establishment and
  actual hosted platform/droplet execution remain pending; workflow definitions
  alone do not establish passing results or configure branch protection.

## Deterministic simulation and concurrency

- [ ] **Event/effect boundaries.** Extend the existing pure transition guards so
  critical protocol decisions can be exercised independently of OS effects.
  Avoid maintaining a separate simulated protocol implementation.
- [x] **Synchronous RPC transport engine (2026-09-25).** The production engine
  advances the native stream gate, ordered byte progress, receipt shutdown and
  queued datagram admission without socket IO or executor waits. Its adapter
  supplies scheduling time, packet arrivals and application IO completions;
  migration decisions also take explicit time. Packet tests exercise this same
  engine. The shared clock backend below composes recovery time and runtime
  controls; the packet IO adapter below now exercises the driver. Identity/token
  entropy and full listener/mobility composition remain open.
- [x] **Acknowledged close across packet-burst yields (2026-09-25).** Live
  scheduling tests exposed loss of a validated receipt when sending CLOSE
  exhausted a one-packet burst and the next turn saw quiche draining. The engine
  retains that receipt in `Flushing` until output exhaustion. `NativeCloseFlush.tla`
  checks **12 states / 14 real Native packet edge-prefix replays**, with controls
  for forgetting the receipt and completing before CLOSE output. Existing
  one-sided/crossed receipt and live scheduling tests cover adapter integration.
- [x] **Clock injection through quiche and the runtime (2026-09-25).** The runtime
  enables the fork's optional `tokio-clock` backend. Recovery, packet pacing,
  idle/path expiry, datagram admission, migration and shutdown now share Tokio
  time. Paused-time tests start ten minutes ahead of wall time, recover dropped
  encrypted stream data, check deadline boundaries, preserve the original path
  after candidate timeout and run the actual outer driver through idle expiry
  and coincident shutdown expiry. `NativeClockDomains.tla` checks **56 states /
  99 real Native edge-prefix replays** for timer polling order and independent
  replacement deadlines, with three failing model controls. This establishes
  the transport clock boundary, not deterministic entropy or simulated OS IO.
- [ ] **Test entropy injection.** Reproduce connection IDs, introduction tokens,
  keys and Native ephemeral randomness in test builds. Production must continue
  using secure entropy; prevent deterministic test configuration from shipping.
  Quiche packet randomness and Native ephemeral keys are controlled under private
  `cfg(test)` scopes; runtime identity/token generation remains open.
- [ ] **Packet simulator.** Schedule loss, duplication, corruption, reordering,
  delay, partitions, migration and endpoint restart. Record seed, event log,
  configuration and build identity; minimize failing schedules. The two-peer
  loss/duplication/corruption/reordering foundation and bounded two-generation
  path/partition/restart scenarios above are complete. Generated delivery decisions
  now shrink within their bounds. The runtime adapter below adds generated
  delivery schedules to the real async driver. Arbitrary partition/restart
  histories, full route/listener/mobility composition and lifecycle-schedule
  shrinking are still pending.
- [x] **Equivalent packet IO adapter for the runtime (2026-09-26).** A private
  `cfg(test)` socket drives the production `drive`/`drive_packets` futures with
  paused time and a waker-aware local executor. It controls send readiness,
  send/receive failure, closure, delivery, loss, duplication, corruption, delay
  and reordering. mutual TLS with optional admission secrets tests cover bidirectional flow-control pressure,
  crossed receipt shutdown, a finite partition and replacement sessions at the
  same addresses/CIDs. `NativeSocketDriver.tla` checks **344 states / 790 real
  driver edge-prefix replays** for terminal precedence and absence of canceled
  sends, with four failing model controls. **32 fixed-seed generated schedules**
  support shrinking and saved semantic replay. The independent-process gate
  records source/compiler/binary hashes and rejects a corrupted expected
  receipt. Packet event logs are diagnostic: crypto entropy and Tokio's internal
  `select!` order remain uncontrolled, so this is not byte-identical execution.
- [x] **Candidate migration through simulated IO (2026-09-26).** Active dedicated
  paths and candidate sockets share the private datagram adapter. A reproduced
  bug let a blocked candidate send stall established RPC and hide migration
  expiry; probes now poll once and use quiche's loss recovery for retries.
  `NativeCandidateIo.tla` checks **44 states / 43 edge-prefix traces**, replayed
  under both mutual TLS with and without admission secrets (**86 driver runs**), with five failing model controls.
  Tests hold real encrypted validation responses, reject corrupted replies,
  race queued proof with cancellation/expiry, reject late proof and preserve
  the active stream after commit or retirement. Additional tests cover candidate
  send failure, successful retry and crossed receipt shutdown on the new path.
  Dedicated socket closure is modeled as IO failure; the old dedicated socket
  has no listener-lifetime watcher that could kill its replacement. These are
  bounded two-peer lifecycle checks, not arbitrary migration schedule replay.
- [x] **Shared-listener packet IO composition (2026-09-26).** Listener owners,
  reserved sessions and dedicated clients now use the same raw datagram adapter.
  Tests run the production receive loop, reservation accept, Native/native stream
  authentication and spawned drivers with two established routes and one pending
  reservation. `NativeListenerIo.tla` checks **213 states / 456 edge-prefix traces**,
  replayed in mutual TLS with and without admission secrets (**912 runs**), with five failing model controls.
  Traces cover blocked shared sends, route cancellation, admission drain,
  reservation expiry, transient/fatal receive errors, close and final-owner drop.
  The simulator wakes every blocked sender; a direct waker test checks readiness,
  errors, closure and absence of canceled output without relying on timer retries.
  Packet regressions cover pending-route flooding, altered destination CIDs,
  duplicate ciphertext, rejected identity/PSK proofs and stale packets/handles
  after reservation replacement. Counts, route survival and delivered bytes are
  checked against the model; cryptographic state is not fabricated.
- [x] **STUN discovery and mapped publication through simulated IO (2026-09-26).**
  A private borrowed datagram interface runs the same one-shot discovery loop
  with UDP and simulated sockets. A reproduced bug let a blocked send hide the
  overall timeout and replies to previous probes; sends now remain inside the
  deadline/receive selection. `NatDiscoveryIo.tla` checks **40 states / 47 traces**,
  replayed with IPv4/IPv6 observed addresses (**94 runs**), with four failing
  controls for timeout, foreign replies and late output. The existing
  `NativeMappingRefresh` model's **63 states / 122 traces** now also run through
  actual listener packets, mapping observers and recipient-scoped discovery
  publications under mutual TLS with and without admission secrets (**244 runs**). Native checks compare directory
  bindings, revoked provisioning capabilities and continuing encrypted streams.
  Additional cases cover replies during a blocked retry, mapping expiry under
  shared-send backpressure, stale replies/owners after replacement and listener
  drain/close. STUN hints do not establish NAT reachability or peer authority.
- [ ] **Extend simulated IO composition.** Run route arbitration, capability
  handoff, recipient-bound provisioning dials and authorized NAT rendezvous
  through simulated IO. Arbitrary combined histories, NAT filtering/translation
  behavior and whole-runtime replay remain open.
- [x] **Local notification schedule exploration (2026-09-24).** Pinned Shuttle
  0.9.4 runs the real grant `watch` futures and shutdown `Notify` futures using
  its local executor. Three scenarios sample **256 schedules each**, replay
  every decision/event sequence, and reproduce the full artifacts across two
  independent processes. They cover multiple waiters, cancellation before/after
  registration, replacement waits, sibling grant isolation, competing terminal
  results and independent old/new shutdown controls. Missing wakeups cause an
  observed deadlock in a deliberate negative control; saved failing schedules
  reproduce it. Truncated decisions and altered events are rejected. Evidence
  records source/compiler/binary hashes and retains per-case JSON schedules.
  Commands and limits: [TESTING.md](Testing-History.md#local-async-schedule-exploration).
- [x] **Pending route worker ownership (2026-09-24).** A private executor
  boundary runs the production owner, pending-selection future, terminal-state
  transitions and stop/drop logic under Shuttle. **256 schedules** with exact
  replays cover cancellation before/after the first poll, error delivery racing
  cancellation, retained observers, two workers per owner, repeated stop and
  independent generation outcomes. Native Tokio tests separately check resource
  release after stop/drop and starting a replacement before canceled workers
  have been released. These fixtures use failed/pending selection futures; they
  do not forge authenticated sessions or exercise successful installation.
  This scenario also runs in the independent-process qualification gate.
- [x] **Route writer and local output fences (2026-09-24).** Two additional
  Shuttle scenarios run the real serializer, write queue, two-party driver,
  shared output fence and route watcher, with controlled `AsyncWrite` readiness.
  Each samples **256 schedules**, with immediate and independent-process replay.
  Coverage includes short writes, ordered messages, rejection after the queue
  terminator, canceled/replacement fence waiters, connection-handle retention,
  write/flush/close errors, owner stop and an active drain preserving ownership
  of its terminal result. The watcher now explicitly polls the producer first,
  eliminating randomized `select!` polling.
- [x] **Shutdown completion and terminal precedence (2026-09-24).** The public
  shutdown path now uses a private helper composing queue flush, output close,
  peer outcome and deadline. A reproduced bug allowed a canceled route to return
  an already-recorded receipt; existing terminal causes now take precedence and
  wake waits blocked on local output. Completed fence chains win simultaneous
  expiration; incomplete chains time out. Two Shuttle scenarios sample **256
  schedules each**, with independent readiness or terminal-cause oracles,
  forced blocked cancellation, immediate replay and independent-process replay.
  A lost terminal-wakeup control must deadlock and reproduce that failure.
  Paused Tokio time checks all three blocking stages; real writer/fence and
  authenticated route-replacement tests exercise the surrounding boundaries.
  The gate now includes eight scenarios. Receipt/deadline inputs in Shuttle are
  scheduled signals; it does not run encrypted packet delivery or actual timers.
- [x] **Encrypted shutdown receipt and close boundary (2026-09-24).** Real
  mutual TLS with optional admission secrets packet tests exposed success after a crossed receipt and premature
  peer close. The close path now requires the authenticated graceful application
  code, a validated receipt and, for crossed shutdown, the reciprocal request,
  complete input delivery and a fully queued, unreset reply. Other closes and
  timeouts cannot substitute for those fences. Tests cover fragmented/unfinished frames,
  nonce/count/role/length errors, resets, insufficient reply credit, duplicate and
  damaged packets, and old ciphertext after reconnect with the same identities,
  addresses and CIDs. The actual outer driver checks close outcomes and deadline
  precedence. This is bounded packet replay and native regression coverage;
  runtime time is now controlled by the shared clock backend above. The packet
  IO adapter exercises the outer driver with receipts and faults; composition
  with Shuttle's schedule exploration remains open.
- [ ] **Extend Shuttle integration.** Cover authenticated route installation,
  connector arbitration, receipt-frame validation with packet delivery and
  full transport driver shutdown. Current fixtures schedule single-thread
  task polls and explicit actor yields, not Tokio's executor or
  synchronization internals. Integrate clocks/socket simulation separately;
  the current timeout contender supplies an error without advancing a timer.
- [ ] **Selective Loom checks.** Use for custom atomic/lock-free components if
  introduced or modified. The current `Rc`/`RefCell`/`LocalSet` protocol core makes
  async lifecycle exploration a higher priority than blanket weak-memory testing.
- [ ] **History checking.** Record invocation/completion histories and check the
  chosen ordering/consistency contract. Apply linearizability only where promised;
  transport receipt is not method completion or exactly-once execution.

## TLA+ and implementation correspondence

- [x] **Grant revocation notification lifecycle (2026-09-24).** A four-grant
  tree models ancestor/sibling revocation, construction, polling, cancellation,
  replacement and repeated revocation. Four watched-node configurations check
  **4,739 states** and replay **25,130 graph-edge prefixes** through Rust grants
  and futures. Four fault controls reject missed ancestors, sibling contamination,
  lost prior revocation and premature completion. Weakly fair polling establishes
  eventual observation or cancellation; native tests separately assert actual
  wakeups. This does not model an executor, multiple concurrent waiters, transport
  authentication or arbitrary grant trees.
- [x] **Shutdown waiter lifecycle (2026-09-24).** `ShutdownWaiters.tla` checks
  **3,874 states** and replays **24,752 edge prefixes** through production
  controls/futures, comparing outcomes, readiness and actual waker notifications.
  Two old-generation waiters and one new-generation waiter cover broadcast,
  first-result stability, cancellation/replacement and generation separation.
  Four fault controls reject overwritten outcomes, single-waiter wakeups,
  cancellation of another waiter and cross-generation completion. Liveness
  assumes weakly fair polling; it does not prove executor or packet progress.
- [x] **Route task ownership (2026-09-24).** `RouteTaskOwner.tla` checks
  **482 states / 2,008 graph-edge prefixes**, replayed through the production
  pending route/owner code with a manually polled executor adapter. Observations
  compare worker completion/cancellation, abort flags, resource release, retained
  terminal causes and replacement-generation isolation. Five fault controls
  reject retained owners, missed workers, overwritten causes, cross-generation
  cancellation and running canceled work. Weakly fair executor polling gives
  eventual resource release; native Tokio and Shuttle tests separately exercise
  scheduling. Admission arbitration, authenticated installation, route-table
  replacement, output fences, IO and arbitrary worker counts are outside this model.
- [x] **Route output fence composition (2026-09-24).** `RouteOutputFence.tla`
  checks **1,843 states / 4,803 edge-prefix replays** across successful output
  and write/flush/close failure configurations. Replay drives the real writer
  watcher and two-party driver, comparing I/O progress, queue/fence results,
  route status, first terminal cause, cancellation and retained connections.
  Five fault controls detect premature output publication, skipping flush,
  missed cancellation, overwritten causes and errors taking over an active drain.
  Weakly fair polling gives eventual fence resolution once I/O is ready, or
  cancellation completes. The bounds are one batch/two messages and one owner;
  input, authenticated installation, receipt frames and actual timers are excluded.
- [x] **Shutdown completion correspondence (2026-09-24).**
  `RouteShutdownCompletion.tla` checks **2,087 states / 8,368 edge-prefix replays**
  across healthy, flush-error, close-error and peer-error configurations. Replay
  drives the production completion helper and controls, comparing returned
  results, structured first causes, route status and sticky control outcomes.
  Six fault controls reject skipped flush/close/receipt fences, ignored terminal
  causes, deadline-first ties and overwritten causes. Weakly fair polling yields
  completion when a result, deadline or terminal transition is available. The
  model abstracts fence readiness and receipt/deadline arrival, not packet
  authentication, real time, generation replacement or whole-runtime execution.
- [x] **Shutdown packet correspondence (2026-09-24).**
  `NativeShutdownPacketFence.tla` checks **489 states / 737 edge prefixes** across
  ordinary/crossed receipts, blocked/reset reciprocal writes and invalid nonce
  binding.
  Each prefix runs through production control-stream parsing, transitions and
  peer-close completion using real protected packets in both Native modes:
  **1,474 native replays**. Seven model faults detect skipped FIN, bad binding,
  missing crossed fences, incomplete/reset reply writes, unrelated close codes
  and success after failure. Fair cooperative peers with sufficient credit eventually
  succeed; other configurations check termination, including close errors. Bounds
  are one two-fragment receipt, one reciprocal request, two input bytes and one
  close/failure, optionally resetting the reply immediately before close. Peer
  behavior is scripted; this does not prove peer honesty, cryptographic security,
  arbitrary packet loss or complete runtime refinement.
- [x] **Resolve the composed-model diagnostics (2026-09-25).** Explicit Release
  now uses wire ID/count and local export state; request receipt copies wire
  method/data. `ReleaseAnnotation` and `RequestPairing` state the ownership and
  identity relation, checked on generated traffic in the explicit-release
  workload. Annotation-isolation checks include unrelated credits.
  `ComposedWireBoundary.tla` calls the actual composed handlers: **279 states /
  1,092 edge-prefix replays** through raw Rust RPC messages, observing destruction,
  Abort, dispatch and results. Three model fault controls detect ticket-based
  Release and paired method/data dependencies; native tests cover maximum wire
  release counts. All 12 conformance checks now run without ignores. This closes
  the two diagnostics, not full wire/local-state refinement: implicit releases,
  request identity and other shared metadata still rely on auxiliary state.
- [x] **Generated composed RPC histories (2026-09-25).** The same composed
  Release/Call handlers now have a finite cyclic projection: **630 states /
  12,040 edges**, two exports and up to three references each. Proptest selects
  **128 histories** with up to 64 generated operations followed by terminal
  release/error checks. Each history runs twice through real Rust RPC messages
  in each of two independent processes (**512 native executions**). Repeated
  calls cross merged states and cycles, reuse two finished question IDs and vary
  explicit task yields. Final release order, partial/zero release, over-release
  and unknown/retired exports are checked against modeled observations.
  State-relative choices remain valid during shrinking; a synthetic failure
  reduces to two method calls and succeeds against the real runtime. Versioned
  JSON replay rejects altered/truncated histories, with graph/source/compiler/
  executable provenance in qualification reports. This is sequential handler
  history coverage; it does not enumerate executor schedules or concurrent calls.
- [ ] **Broader histories and scheduling.** Extend generated histories to pending
  RPCs, promise resolution, capability-slot reuse, route changes and storage,
  with scheduler variation and expanded bounds alongside current TLC replay.
- [ ] **Cross-layer handoff scenarios.** Combine route generations, capability
  references, embargoes, cancellation, packet recovery and restart in bounded
  models and real implementation replay.
- [ ] **Observable invariant mapping.** Document which Rust observations witness
  each invariant, what is abstracted away, and what fairness assumptions support
  liveness. Preserve graph/corpus provenance and negative controls.
- [ ] **Selective code-level proofs.** Evaluate Kani for bounded arithmetic/parser
  harnesses and Verus, Creusot or extraction-based tools for a justified critical
  algorithm. These are optional projects with explicit trusted-code and language
  support boundaries; they do not replace TLC or runtime tests.

Current limits: [CONFORMANCE.md](../wiki/Model-Conformance.md). Edge-prefix replay selects
one reachable prefix per graph edge. The additional generated RPC histories
sample paths and cycles; neither covers every history or executor schedule.
Component results do not constitute a whole-runtime refinement proof.

## Native and handoff security qualification

- [ ] **Threat model and independent review.** State trust in introducer, host,
  recipient and storage, and review Native/transport/delegation composition.
- [ ] **Symbolic protocol model where useful.** Consider Tamarin or ProVerif for
  identity/context binding, delegation, replay and compromise scenarios. A proof
  of that model would still need a correspondence argument to the implementation.
- [ ] **Key lifecycle qualification.** Exercise directional/context separation,
  packet-number/nonce limits, key updates, retransmission and key retirement,
  including overlap with migration and capability handoff.
- [ ] **Cross-session/restart replay tests.** Specify which authority persists,
  which reservations are retired, and which operations may repeat after failure.
  Distinguish packet retransmission from application retries.
- [ ] **Malicious participant scenarios.** Test wrong-recipient introductions,
  forged or substituted bindings, stale/reused slots, contradictory responses
  and resource exhaustion with each participant malicious in turn.
- [ ] **Primitive and timing checks where applicable.** Evaluate relevant
  Wycheproof vectors and constant-time analysis/measurement for custom handling;
  trusted or verified primitives alone do not verify protocol composition.

Application 0-RTT remains intentionally rejected. This roadmap does not propose
enabling it or adding TLS/standard QUIC interoperability. See [SECURITY.md](../../SECURITY.md).

## Dependency and release assurance

- [ ] **Advisory/source/license policies.** Add suitable `cargo-audit`/RustSec and
  `cargo-deny` checks with reviewed exceptions.
- [ ] **Dependency review evidence.** Consider `cargo-vet` for the coordinated
  runtime and crypto supply chain; preserve fork/source provenance.
- [ ] **Unsafe inventory.** Consider `cargo-geiger` or a maintained source audit
  to locate unsafe boundaries, without treating counts as a soundness score.
- [x] **Platform compile/smoke automation.** Linux, macOS and Windows default
  builds are configured. Full cross-platform correctness/durability qualification
  remains outside these smoke checks; the feature combination matrix is removed.
- [ ] **API compatibility checks when a policy is declared.** Consider
  `cargo-semver-checks` once stable compatibility promises matter. Do not retain
  compatibility-only code merely to constrain this unreleased 0.x design.

## Tool references

- [Miri](https://github.com/rust-lang/miri)
- [Turmoil](https://github.com/tokio-rs/turmoil),
  [UDP API](https://docs.rs/turmoil/latest/turmoil/net/struct.UdpSocket.html)
- [Shuttle](https://github.com/awslabs/shuttle),
  [instrumented wrappers](https://github.com/awslabs/shuttle/blob/main/wrappers/README.md)

Tool support changes. Pin evaluated versions and record the tested integration
rather than treating a tool's name as evidence of coverage.

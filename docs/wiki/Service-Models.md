# Schema exchange, bulk transfer, and encrypted transport

These are three executable TLA+ component models checked with TLC. They model application and transport contracts around Cap’n Proto RPC. They do not introduce new `rpc.capnp` message tags and are not yet composed with `CapnpNetwork` or its capability reference ledger. Their results therefore do not establish end-to-end RPC/transport refinement.

The baseline is Cap’n Proto commit `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. Its [roadmap](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/doc/roadmap.md) describes schema transmission and encrypted transport as intended features, without complete wire specifications. The schema and encryption contracts below are explicit design proposals. The bulk model incorporates documented streaming behavior already present in that checkout.

A Rust implementation of the schema-transmission contract now exists in
`src/schema_exchange.rs`, with an ordinary RPC catalog capability and bounded
TLC/Rust trace replay. See [the implementation guide](Schema-Exchange.md).
The [realtime snapshot service](Realtime-Snapshots.md) is implemented over
ordinary RPC and optional authenticated Native datagrams. The datagram adapter
uses session-local capability tokens, bounded fragmentation up to 59,392 bytes
and reliable control/status queries. Both
receiver and adapter model traces replay against Rust; separate tests exercise
real Native UDP, capability revocation and session isolation.
The [bulk transfer service](Bulk-Transfer.md) implements strict payload credit,
sticky errors, cancellation and atomic in-memory publication, with component and
real RPC trace replays. It uses ordinary Call/Return acknowledgments; optimized
RPC streaming and its flow controller remain separate APIs.

The [durable bulk service](Durable-Bulk.md) adds resumable file upload,
disk-backed chunk checkpoints and atomic typed ORM publication with a completion
receipt. Native and RPC traces cover retries, restart, cancellation, target
conflicts, revocation and digest failure; separate tests exercise lost replies,
batch recovery and authenticated Native. Transfers remain bounded by store quotas.

The [shared Native listener](Shared-Listeners.md) adds bounded, single-use
connection reservations on one UDP socket and native directory integration.
Its registry model replays allocation, expiry, queue and shutdown traces against
Rust; real Native tests include native three-party handoff.
The [capability provisioning service](Provisioning.md) creates fresh
reservations and PSKs on demand through existing encrypted control connections.
An introducer relay keeps provisioning reachable before the direct native route
exists. Lease lifecycle traces replay against Rust, and real handoff tests retain
the direct capability after provider revocation and introducer disconnection.
The [native arbitration profile](Session-Arbitration.md) handles crossed dials
without replacing pending RPC connection identities. Both peers agree on one
authenticated stream before sending RPC data, and expose its datagram lane for
realtime use. Safety and healthy-progress models replay against the Rust handshake;
UDP tests cover capabilities, simultaneous provisioning, cancellation and handoff.

The [persistence realm](Persistence.md) implements standard Persistent.save,
durable owner seals, Native-authenticated restoration, key rotation, revocation
and typed ORM reconstruction. Its TLC model replays factory completion races
against Rust, with separate restart and torn-transaction recovery tests.

The [ORM history service](Publication-History.md) adds typed publication replay,
consumer-owned resumable cursors and explicit retention gaps. History-preserving
mmap checkpoints survive restart; blocked reads respond to cancellation and
grant revocation. Storage and RPC models replay against Rust, alongside full
restart, fault-injection and Native integration regressions.

## Dynamic schema transmission

[`CapnpSchemaExchange.tla`](../../verification/CapnpSchemaExchange.tla) models requests and responses as separate messages that can be handled in either order. The client discovers dependencies from accepted schema bodies, requests each key at most once, and caches validated results. Activation is separate from receipt and requires the entire dependency closure.

A key is a pair of schema ID and immutable revision. Requests pin the revision; the server can publish a newer revision while preserving old ones. Responses must match the requested key. This policy is a proposed exchange contract, not a claim that Cap’n Proto schema IDs encode revisions. The two concrete body versions abstract their contents and compatibility. Byte decoding, arbitrary schema structures, recursive type checking, and content-hash computation are outside this model.

The configurations use three schema IDs and two revision values and cover normal retrieval, revision two, a publication race, cyclic dependencies, a dependency shared by two nodes, a missing dependency, and a response containing the wrong revision. Missing or invalid responses lead to failure rather than usable partial schemas. Cycles are allowed because activation checks that all nodes have arrived; it does not require a topological loading order.

This is deliberately stricter than [`SchemaLoader::load()`](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/schema-loader.h), which can create dependency stubs and select a newer compatible schema. No equivalence to that loader is claimed. A schema service capability and an authenticated RPC channel are assumed already available.

Safety checks cover cache identity, dependency closure, pinned revisions, and exclusion of rejected schemas from active use. Weak fairness of requests, responses, activation, and failure establishes eventual readiness or failure. Positive retrieval cases additionally require eventual readiness. Mutations accept a wrong revision or activate before dependencies arrive. Witnesses establish successful retrieval, cyclic retrieval, rejection, and retention of a pinned revision after publication.

## Bulk transfer

[`CapnpBulkTransfer.tla`](../../verification/CapnpBulkTransfer.tla) models one streaming object over reliable FIFO request and reply channels. The checked transfers contain four or six alternating one- and two-unit chunks with windows of two, three, or five units. Sending reserves byte credit immediately; processing and receiving the corresponding Return are separate steps. Only a received Return releases sender credit. Multiple calls may be outstanding, so send admission is not evidence of remote execution.

The source contract is the pinned [streaming flow-control description](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/doc/_posts/2020-04-23-capnproto-0.8.md). A failed streaming call makes later invocations on the object fail too. An explicit non-streaming `done()` observes the final error. The model checks both subsequent chunk results and the final result.

The fixed, strict byte window is a model policy. It does not reproduce socket-buffer sizing, adaptive bandwidth estimation, scheduling across streams, or the implementation's precise admission thresholds. The chunks must fit within the configured window. Cancellation is an ordered application-level abort: previously submitted calls may execute before the abort is processed. Its reply establishes resource cleanup; this is not a model of RPC Finish cancellation or of canceling already committed effects.

Safety checks cover the byte limit, credit conservation, ordered execution without duplicates, sticky errors, sound completion, and terminal cleanup. Optional duplicate acknowledgments stress credit idempotence; they are not expected from the assumed reliable FIFO RPC transport. Under weak fairness, each finite transfer completes or is canceled. Mutations bypass the window, release credit twice, or forget a previous error. Witnesses exercise completion, cancellation, backpressure, and error reporting.

## Encrypted capability transport

[`CapnpEncryptedTransport.tla`](../../verification/CapnpEncryptedTransport.tla) models a recipient, a trusted introducer, a capability host, and an active network attacker. The introducer distributes an epoch-specific pre-shared key over existing protected channels. Delivery to the recipient and host are independent. The grant authorizes one service capability. This abstract model permits encrypted application data before host confirmation.
The production Native TLS profile instead waits for authentication and rejects
application 0-RTT; do not use this model as its handshake specification.

This follows the abstraction boundary in the pinned [`VatNetwork` sketch](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.capnp): pipelined traffic can wait for authentication or be protected so that only the authentic destination can decrypt it. An explicit authenticated confirmation channel returns host acceptance to the recipient. It is abstracted as an epoch-tagged acknowledgment, not as a fully modeled reverse-direction cipher stream.

Cryptography is idealized. The attacker can replay an observed frame, alter protected data, substitute an outer capability selector, redirect an old frame to a new session, or forge a frame under its own known key. Authentication accepts honest sealed frames or frames created with an attacker-known key; the host must also possess the corresponding authorized key. The attacker cannot forge a frame under an unknown key. One injected packet is allowed per configuration; the checked bounds are one or two epochs and one or two application messages per epoch. Honest messages can arrive in arbitrary order. Honest delivery is lossless for the progress checks.

The sender uses a fresh abstract key for each epoch and a unique nonce for each message under that key. The receiver checks capability scope and the current epoch and suppresses repeated key/nonce pairs. Old keys and replay history remain stored so that rejection of stale sessions is tested explicitly rather than obtained by deleting all old traffic. Rotation can race with an undelivered old message.

Safety checks cover authenticated delivery, capability binding, session freshness, at-most-once acceptance, nonce uniqueness, and absence of attacker-learned application plaintext. The confidentiality property is a symbolic knowledge check, not computational indistinguishability. Endpoints and the introducer are trusted and uncompromised; the introducer knows the distributed key. Forward secrecy, key erasure, arbitrary compromise, negotiation/downgrade resistance, and public-key identity enrollment are not modeled.

The [TLS specification](https://www.rfc-editor.org/rfc/rfc8446.html) motivates the authenticated-encryption and unique-nonce requirements. This model does **not** implement a TLS handshake, Diffie–Hellman, a concrete cipher, or a libsodium API. It cannot establish their cryptographic security. Mutations skip authentication, omit capability/session checks, admit replay, reuse a nonce, or disclose cleartext. Witnesses demonstrate acceptance before confirmation, replay rejection, and a confirmed rotated session.

The separate [realtime snapshot contract](Realtime-Model.md) extends this collection with a deadline-aware, replaceable-data interface and its own TLC configurations. It does not change the three models described here.

## Reproduce the checks

Use Java 17 and TLC 2.19 through the native Cargo harness:

```sh
CAPNTPROTO_TLC_CASES=BulkTransfer,BulkCancellation,BulkError,BulkDuplicateAck,EncryptedTransport,EncryptedReplay,EncryptedCapability,EncryptedEpoch cargo test --test protocol_models protocol_reference -- --ignored --exact
```

Omit `CAPNTPROTO_TLC_CASES` to check every configuration. Module and expected
violation annotations live in `verification/configs/*.cfg`; see [Testing](Testing.md).
The configuration generator is `scripts/generate_feature_configs.py`; it does not
run tests.

Every positive configuration checks its listed safety invariants and its progress property under explicit weak fairness. Mutation and witness configurations instead require the specifically named invariant counterexample; parser errors, timeouts, and unrelated failures do not pass. There are no state constraints or depth cutoffs. TLC exhausts the finite configured state graphs for positive results. This is not a theorem for arbitrary numbers of schemas, chunks, sessions, or attacker packets.

## Recorded verification

All **44 configurations passed** against the final source hashes: 21 safety/progress configurations, 12 deliberately broken variants with their expected counterexamples, and 11 reachability witnesses. No sources changed during that historical run. Its report and raw logs are not bundled here; see the [evidence policy](../../research/reports/README.md). Rerun the current catalog for current-source qualification.

| Model | Configurations | Largest completed positive graph |
| --- | ---: | --- |
| `CapnpSchemaExchange` | 12 | `SchemaShared`: 40 |
| `CapnpBulkTransfer` | 14 | `BulkCancelError`: 208 |
| `CapnpEncryptedTransport` | 18 | `EncryptedConcurrentRotation`: 5,810 |

State counts belong to individual configured models and must not be added together as a state count for their composition. Configuration generation reproduced byte-for-byte; the existing RPC models and TLC runner are unchanged; the pinned schema and all eight wire inventories still match.

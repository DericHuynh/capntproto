# ActorDB Implementation Checklist

**Status: Planned work. Date: 2026-10-02.** This checklist implements the
[event sourcing and incremental views proposal](ActorDB-Proposal.md). All
implementation items are open. Existing RPC/storage features are inputs to this
work, not evidence that an actor database already exists.

Complete phases in dependency order. A task is complete only when its code,
contract documentation and relevant validation evidence are linked beside it.
Use a commit or retained report with its revision, command, result and known
limits. Do not check an item because an API was sketched or a dependency supports
something similar. This file is the implementation tracker; the proposal owns
architectural intent, and the [actor specification](../proposals/Capnt-Actors.md)
owns the underlying actor guarantees.

The single-host prototype requires phases 0–4 plus applicable phase 7 checks.
The replicated MVP also requires phase 5. Phase 6 is an extension; its operators
must pass phase 7 qualification before they are advertised.

## Phase 0 Freeze the database contracts

- [ ] Define the initial workload, supported operators, durability profile and measurable resource/latency budgets; keep unmeasured performance targets distinct from results.
- [ ] Freeze actor, operation, event, batch, snapshot and commit-position records, including generation and source-lineage rules.
- [ ] Reuse the actor request fingerprint and recovery contracts; define golden vectors and schema evolution for all new database records.
- [ ] Define atomic batch visibility, no-event operation progress, deterministic reduction and the prohibition on replaying external effects.
- [ ] Define projection identity/generation, checkpoint contents, contiguous input frontiers, query-token encoding and typed gap/incompatibility errors.
- [ ] Select candidate event/index providers and a reference IVM implementation; document evaluation criteria for Differential Dataflow and any dependencies.
- [ ] Define tenant authorization, feed/query separation, policy-change behavior, retention reservations and overload/control budgets.

Gate: the commit, replay, freshness and retention contracts are reviewable without
relying on unspecified storage or dataflow behavior. Initial provider and engine
decisions have recorded reasons and explicit unsupported features.

## Phase 1 Implement the required actor foundation

- [ ] Implement automatic/prepared operation identities and authorized recovery after a lost first response; freeze original requests across retries.
- [ ] Implement a single-host durable provider whose ownership/revision checks and actor state/result/effect writes form one atomic decision.
- [ ] Implement exclusive turns, bounded admission, deterministic reset after abort and reconciliation before retrying an uncertain commit.
- [ ] Implement authoritative actor reads with provider proofs and explicit durability/failure-domain receipts.
- [ ] Implement pre-start cancellation tombstones and separate observation permission from resubmission permission.
- [ ] Implement executor-local/shared client boundaries and persisted authority descriptors; never move raw local capabilities into storage workers.
- [ ] Reserve bounded status, cancellation, acknowledgement, recovery and shutdown capacity independently of a busy actor turn.
- [ ] Run the applicable actor specification acceptance cases, including identity conflict, revocation timing, stale ownership and result retention.

Gate: one actor can commit durably, lose its response, restart and recover the
original result without an additional managed mutation. This gate does not
advertise multi-host failover.

## Phase 2 Implement event storage and replay

- [ ] Implement indexed per-actor event reads, ordered event batches and a durable shard feed without requiring one subscription per actor.
- [ ] Commit events, sequence advancement, operation result and staged effects atomically under the provider fence.
- [ ] Record replayable domain rejections and successful no-op completions without inventing domain events; advance feed progress for their commit positions.
- [ ] Implement deterministic reducers and versioned snapshots bound to a covered event sequence and actor revision.
- [ ] Implement snapshot-plus-tail recovery and full replay; verify equivalent actor state and prevent historical effects from being resent.
- [ ] Implement byte/count limits before commit, explicit retention gaps, corruption handling and bounded recovery scans/index loading.
- [ ] Implement bounded group commit where the selected provider supports it; reconcile each transaction independently after uncertain sync or replication outcomes.
- [ ] Inject crashes around append, durability, publication and reply boundaries; verify that no partial event batch is visible as committed.

Gate: a retained history deterministically reconstructs the actor, duplicate
commands replay one result, and a lost feed wakeup does not lose committed input.

## Phase 3 Implement minimal incremental views

- [ ] Define typed base relations and versioned event-to-relation mappers with stable keys and explicit old/new row changes.
- [ ] Implement signed insertions and retractions for keyed projections and filters, including updates that move records between keys.
- [ ] Implement count and sum with specified null, numeric and overflow behavior; reject unsupported operators explicitly.
- [ ] Co-locate a projection partition's output, required operator state and input checkpoint in one atomic persistence boundary.
- [ ] Advance only contiguous completed input positions and preserve complete actor event batches at output publication.
- [ ] Expose typed indexed queries over pinned published output revisions, with matching progress metadata and authorization.
- [ ] Bound input batches, queues, operator state and output buffers; demonstrate backpressure while control requests retain progress.
- [ ] Compare incremental output against full recomputation after randomized inserts, retractions, duplicates, reordered deliveries and process restarts.

Gate: every supported view matches the reference computation at its declared
input frontier, including after a crash between processing and checkpointing.

## Phase 4 Implement read progress and view lifecycle

- [ ] Return commit tokens that resolve actor revisions to source positions, including operations with no events.
- [ ] Implement current-published-view reads and include-commits reads with bounded waiting, typed timeout and incompatible-generation/lineage errors.
- [ ] Match a query's pinned output revision to its frontier; never report progress newer than the returned data.
- [ ] Track all relevant input partitions; test that an advanced partition cannot hide a stalled partition or a missing earlier batch.
- [ ] Implement projection generation creation, replay from a retained cut or compatible baseline, catch-up validation and atomic reader cutover.
- [ ] Retain old generations for bounded existing readers/cursors and define retirement behavior when those budgets expire.
- [ ] Add bounded consumer/rebuild retention reservations, lag visibility and explicit gap handling; protect required input during compaction.
- [ ] Implement schema/reducer/projection compatibility checks and suspension before advancing past unsupported input.
- [ ] Implement authorized snapshot/delta subscriptions with generation-bound cursors, bounded buffering and reconnect recovery.

Gate: a view can prove it incorporated a supplied commit, and a replacement view
can rebuild and take over while writes continue. Neither behavior claims a
global transaction snapshot. Bounded staleness remains unavailable until its
source freshness and clock proof is implemented.

## Phase 5 Implement replicated durability and owner movement

- [ ] Select and integrate a consensus or equivalent provider, documenting quorum acknowledgement, membership changes and durability/failure domains.
- [ ] Group many logical actors into bounded physical storage/replication shards; qualify ordering and per-actor atomicity within shared batches.
- [ ] Replicate domain data, operation deduplication/results and required runtime obligations together; apply recorded changes without repeating handler I/O.
- [ ] Implement fenced acquisition/transfer, uncertain-commit reconciliation and authoritative read proofs across owner loss.
- [ ] Implement replica bootstrap, catch-up and snapshot installation with source lineage and complete event-batch validation.
- [ ] Implement the selected restoration/delegation strategy before relocation of actors with saved capabilities; reject stale-host authority on live paths.
- [ ] Preserve or explicitly translate accepted commit positions and projection source bindings across ownership/shard changes; reject unsupported split/merge operations.
- [ ] Transfer consumer checkpoints, retention obligations and pending effects without dropping or duplicating acknowledged work.
- [ ] Exercise leader loss, partitions, stale owners, interrupted transfer and lost replies over authenticated TCP and quiche QUIC v1.

Gate: a partitioned old owner cannot commit new work or serve unproved
current reads; a new owner recovers each original operation and its event batch.
Previously valid query tokens either retain their meaning or fail explicitly.

## Phase 6 Extend the query model

- [ ] Specify the consistency model for joins across inputs, including how a complete consistent cut is identified when that guarantee is requested.
- [ ] Evaluate Differential Dataflow against the reference engine using identical persisted inputs, recovery cases, skew and memory budgets; record the engine decision.
- [ ] Implement supported joins with retractions and indexes, including changes on both sides, duplicate keys and bounded fan-out behavior.
- [ ] Qualify coordinated checkpoints or deterministic replay for distributed operator state before distributing a projection graph.
- [ ] Add distinct, minimum/maximum or richer aggregation only with explicit retained-state and recovery contracts.
- [ ] Define event-time watermarks, lateness, correction and finality before enabling windowed queries.
- [ ] Define deterministic extension-function contracts and explicit recomputation for functions without supported incremental semantics.
- [ ] Add declarative query/schema tooling only after its lowering preserves authorization, generations, operator semantics and resource limits.

Gate: each advertised operator is equivalent to full recomputation under the
specified semantics and survives checkpoint/replay tests. Automatic view
promotion/demotion is deferred until rebuild cost and state budgets are measured.

## Phase 7 Qualify operation and release claims

Apply these checks to each delivered profile and operator set; they are not a
substitute for the earlier phase gates.

- [ ] Run fault tests for lost commit/admission replies, duplicate commands/events, partial batches, projector crashes and acknowledgement loss.
- [ ] Test history pruning, disk-full admission, slow consumers, suspended projections and rebuilds competing with live traffic; preserve durable obligations.
- [ ] Validate tenant isolation, revoked grants, raw-feed restrictions, projection caches, live capabilities and subscription delivery across policy changes.
- [ ] Verify dependency/executor shutdown ordering and reserved control progress during overload, stalled handlers and storage failures.
- [ ] Exercise backup and restore; preserve source/token continuity only when proven, otherwise issue a new lineage and explicit incompatibility errors.
- [ ] Measure cold replay, snapshot recovery, view rebuild/cutover, retained state and compaction under sustained writes.
- [ ] Report throughput and p50/p95/p99 commit, write-to-view and query latency with hardware, payloads, workload skew, view count and durability settings.
- [ ] Compare local and replicated profiles separately, including retractions, joins when enabled, group commit and slow-consumer pressure; retain reproducible inputs/results.
- [ ] Provide diagnostics for ownership, uncertain operations, feed lag, checkpoint age, frontier gaps, retained bytes and authority failures without leaking tokens or payloads.
- [ ] Update runtime status, guides and negotiated feature declarations to match tested behavior; publish limitations and evidence links beside completed checklist items.

Gate: a release claim names its actual durability, query and recovery profile.
Passing simulation alone, an in-memory benchmark or RPC transport tests cannot
qualify the composed database.

## Evidence required for the first replicated MVP

| Scenario | Required observation |
| --- | --- |
| Reply lost after event commit | Same operation returns the stored result; one event batch exists |
| Crash during an event batch | Recovery exposes the full committed batch or none of it |
| Duplicate/reordered feed delivery | Supported projection equals full recomputation at the same frontier |
| Crash after output work before checkpoint | Recovery cannot skip input or double count published effects |
| Read immediately after a command | Include-commits read waits for matching output or returns an explicit bounded failure |
| One input partition stalls | Other partitions cannot falsely satisfy its progress requirement |
| No-op or rejected operation | Its valid completion position does not block progress forever despite having no domain events |
| Rebuild during ongoing writes | Cutover has a validated frontier with no missing or duplicate contribution |
| Retention quota reached | Admission/consumer policy is explicit; required input is not silently discarded |
| Old owner resumes after transfer | New managed commits and unauthorized old-host invocations are rejected |
| Backup restore changes history | Old tokens are not accepted against an unrelated position space |
| Heavy projection workload | Business queues stay bounded and restricted control work retains its allocated capacity |

The proposed first demonstration is an order actor with status and line-item
changes, a current-order view, and open-order counts and totals by tenant. Drive
commands concurrently, lose a commit reply, restart a projector, fail the owner,
and rebuild the view. Compare every published checkpoint against a reference
replay. This validates the composition before expanding the query language.

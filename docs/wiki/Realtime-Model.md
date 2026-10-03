# Realtime snapshot protocol: interface and reasoning

This is a proposed application/transport contract for Cap’n Proto, checked with TLC. The pinned roadmap describes dropping messages that would arrive too late, and the 2017 streaming slides sketch `sendFrame(...) -> realtime`; neither defines a complete realtime wire protocol. This model introduces no new `rpc.capnp` discriminants. It is an independent component, not yet composed with `CapnpNetwork`, the bulk model, or the encrypted transport model.

The Rust [realtime snapshot service](Realtime-Snapshots.md) implements a
receiver for this contract over ordinary RPC and an optional capability-authorized
Native datagram adapter. Separate implementation models replay receiver and
adapter transitions against Rust, including reliable status queries, loss,
cancellation and revocation. Real Native tests exercise unreliable data alongside
RPC control. These checks do not compose all models into an end-to-end
refinement proof.

## Interface

The caller receives a fresh stream capability bound to a receiver, clock domain, and negotiated queue capacity. The capability supplies authority; sequence numbers alone do not. The model starts after this authenticated setup. This is interface notation, not supported Cap’n Proto schema syntax:

```
RealtimeSnapshots.offer(key, sequence, notAfter, snapshot) -> localSubmission(receiptFuture)
RealtimeSnapshots.cancel(sequence) -> receipt
RealtimeSnapshots.close() -> closed
waitUntil(receiptFuture, localDeadline) -> outcome | unknown  // local helper
```

`offer` means local submission. It must not wait for an older update's remote completion before submitting a replacement. Its eventual receiver receipt is one of `applied`, `expired`, `superseded`, `busy`, `canceled`, or `closed`. A local deadline timeout yields `unknown`, which a later authoritative receipt can resolve. The local wait does not resolve or cancel the underlying receipt future; a later receipt remains observable. An application must not interpret `unknown` as non-execution or retry a non-idempotent effect under a new sequence number.

Each stream uses unique, increasing sequence numbers across its keys. Reconnect creates a new capability and sequence namespace; sequence reuse within a stream is forbidden. A key identifies one independently replaceable value. Snapshots must be self-contained: dropping intermediate values is valid. This contract is unsuitable for dependent video delta frames, append-only event streams, payments, or arbitrary RPC methods whose effects must all occur. Those require a different dependency or reliability contract.

One pending snapshot per key is enough for this interface. The receiver remembers the highest observed sequence for each key, including updates it rejects as expired or busy. Learning a newer snapshot makes an older queued snapshot obsolete. It replaces that queued snapshot first, then admits the new one if it is still valid and capacity remains. Otherwise it returns an explicit outcome. A new key cannot displace an unrelated key: it receives `busy` when capacity is exhausted. This avoids choosing an undocumented eviction priority between independent streams of values.

The receiver caches the outcome for every known sequence. Duplicates of pending work do not enqueue another copy; duplicates of completed work return the cached outcome. Out-of-order older sequences cannot undo a newer applied value. Receipts are independently delivered and can arrive after newer receipts.

Cancellation targets a sequence and can arrive before its data. The receiver retains a cancellation tombstone so later data cannot revive it. If the update was already applied, cancellation returns `applied`; cancellation does not undo effects. Close stops local submission and travels on a reliable control channel. Receipt of close prevents new application, discards queued work, and produces a close acknowledgment. Updates may still apply before the receiver handles close, even if the sender has already requested it.

## Time and application semantics

`notAfter` is an absolute deadline in the stream's negotiated reference clock domain, not a TTL restarted at each hop or at reception. Translating a producer timestamp into this domain is an external contract. The model uses an observer clock `now` and a receiver clock whose fixed offset is nondeterministically chosen within `[-ClockSkew, ClockSkew]`. The receiver uses only its local clock for admission and application. It permits application when:

```
receiverClock + ClockSkew < notAfter
```

This conservative check prevents late application even when the receiver clock is slow; a fast clock may discard a still-useful update early. Equality means expired. Without a bounded mapping between clocks, a receiver cannot establish an end-to-end absolute deadline merely by starting a local timer when a packet arrives.

The deadline is checked again at the atomic application step, together with stream openness and freshness. Admission alone is insufficient: queuing can consume the remaining lifetime. The atomic step represents the visible commit point of the snapshot. Long-running work or an actuator that changes state after this point requires a separate execution-time bound and another deadline check at the actual effect.

Ticks never wait for delivery or application. Therefore this is a soft realtime contract: it guarantees that stale or late work is not applied, and allows drops. It does not promise that every update is delivered before its deadline. Weak fairness is used for eventual cleanup and control/receipt delivery, not as a finite latency guarantee.

## Modeling boundaries

Data packets may be lost, duplicated, delayed, or reordered. Cancellation, close, and receipts use an independently reliable authenticated control path in the model. Permanent failure of that path is outside the progress claim; sender timeout still cannot reveal remote execution. Payloads are abstract snapshot identities without embedded capabilities, so this model does not prove release of capability references when discarding a payload.

The workload submits finitely many updates before `SubmitBefore`; each expires `Lifetime` ticks after submission. The clock saturates strictly after every possible deadline and conservative receiver expiration. Further real time cannot change a deadline predicate, and no further submissions are enabled. Protocol and cleanup actions remain enabled at that final clock value. This is a finite abstraction of the terminal time region, not a state constraint that removes pending cleanup behavior. Weak fairness of ticks excludes stopping time before that region.

Bounded queue occupancy applies to receiver work, not to arbitrary network buffers or retained receipt/deduplication history. History is retained for the entire finite stream. A production implementation needs a negotiated replay window, acknowledgment-based history reclamation, or a maximum stream length before rotation; forgetting history while old packets remain possible would invalidate the duplicate/cancellation argument.

The explicit-clock method follows [Lamport's real-time modeling approach](https://lamport.azurewebsites.net/pubs/real-simple.pdf). Protocol motivation comes from the pinned [roadmap](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/doc/roadmap.md) and [streaming slides](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/doc/slides-2017.05.18/index.md).

## Implementation and checks

[`CapnpRealtime.tla`](../../verification/CapnpRealtime.tla) keeps sender deadlines/observations separate from receiver deadlines/outcomes. Data packets explicitly carry sequence, key, deadline, and duplicate-copy identity. The receiver copies the deadline from the arriving packet into its local state. Application and receipt delivery are separate actions; receiver decisions do not read sender observations.

The safety invariants check packet metadata preservation, no late application, at-most-once application, per-key sequence order, queue capacity, one pending update per key, pending freshness, truthful receipts, consistent duplicate receipts, cancellation tombstones, the close barrier, and correspondence between `applied` outcomes and actual application history. The positive configurations also check eventual observation or local timeout for every submitted offer, acknowledgment of requested close, and eventual draining after time reaches its terminal region.

All **28 configurations passed** with TLC 2.19 on Java 17: 10 safety/progress configurations, 8 mutations, and 10 reachability witnesses. Source and configuration hashes remained unchanged during the run and were rechecked afterward. Configuration generation is deterministic. The historical report and logs are not bundled in this checkout; see the [evidence policy](../../research/reports/README.md). This is a recorded run, not fresh qualification of the current source.

| Positive configuration | Distinct states | Search depth |
| --- | ---: | ---: |
| `Realtime` | 390 | 12 |
| `RealtimeCancellation` | 5,220 | 16 |
| `RealtimeCapacity` | 632 | 12 |
| `RealtimeClockSkew` | 1,388 | 14 |
| `RealtimeClose` | 2,152 | 15 |
| `RealtimeCombined` | 343,874 | 21 |
| `RealtimeDelayedSubmission` | 1,383 | 13 |
| `RealtimeDuplicates` | 3,069 | 14 |
| `RealtimeLoss` | 530 | 12 |
| `RealtimeMultipleKeys` | 5,478 | 16 |

The checked bounds are two offers on one key by default, three offers on two keys for the multi-key case, receiver capacities of one or two, a two-tick lifetime, submission before tick one or two, and clock error zero or one tick. Duplicate-enabled configurations contain two copies per offer. Each submitted sequence can be canceled once; the stream can close once. The combined case enables loss, duplication, cancellation, and close together. Clock-skew and delayed-submission scenarios are checked separately. These counts describe individual graphs, not their composition.

The deliberate bugs omit the apply-time deadline check, omit the clock-error margin, admit stale or duplicate application, exceed queue capacity, forget a cancellation tombstone, apply queued work after close, or misreport a sender timeout as a known receiver drop. Each mutation must violate its specifically named invariant; parser failures, unrelated errors, and timeouts do not count as successes.

Witnesses demonstrate successful application, expiration, replacement, capacity rejection, cancellation, unknown outcomes, delayed receipts for applied work, acknowledged close, cancellation after application, and loss of an update that never reached the receiver. These prevent the safety results from being satisfied solely by never submitting or applying work.

Run the realtime model, trace replay and reference configurations through Cargo:

```sh
cargo test --test protocol_models realtime_model -- --exact
cargo test --test realtime replay_tlc_realtime_traces -- --exact
cargo test --test protocol_models runtime_reference -- --exact
```

The reference check includes all realtime mutation and witness configurations.
`JAVA` and `TLA2TOOLS_JAR` select tool installations. See
[Testing](Testing.md) for the native runner and its limits.

The completed run used a 120-second per-case limit; the reproduction examples allow more time for slower machines. No depth cutoff or state constraint is used. The results establish properties of these finite contracts and workloads, not hard realtime guarantees for a deployed implementation or end-to-end conformance of the composed RPC model.

# Portable architecture review — 2026-10-22

The next useful redesign is to preserve the owner of each piece of work from admission through completion and retirement. The portable path often knows which question, observer or capability changed, but passes a broad progress signal upward; later scans rediscover the affected owners. Replace that rediscovery with explicit owner bindings and bounded notifications, and remove the superseded scanning paths.

Keep the engine/wire/hosted separation. It provides real protocol-authority, storage and lifetime boundaries. Flattening crates, removing generative lifetimes, or adding more small-call special cases would not address the measured work.

Reviewed source: `788103bc106df1c21e2ff17b5030b6a0870803fb`. C++ reference: `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. This is a source review and reanalysis of existing profiles, not an implemented optimization or a new benchmark campaign. Production code is unchanged. Tracking remains [gap #11](https://github.com/DericHuynh/capntproto/issues/11).

## What is already improved

The [October 19 worker redesign](../2026-10-19-worker-readiness/README.md) already added operation-specific wakers, a bounded FIFO, stale-generation protection and continuation through ready execution stages. Its diagnostic established zero unrelated application-future polls after a selected application wake. Do not reimplement that queue or describe the old sibling-poll problem as current behavior. Protocol-driven invalidation still uses a conservative scan.

The outer TaskSet already polls one connection directly. Large input frames can transfer their immutable allocation. Prepared calls/results retain their envelopes; ordinary immediate calls avoid an internal completion channel. The active QUIC packet route already decrypts the UDP buffer in place, and its stream accesses Quiche directly. These are useful existing simplifications.

Public Driver fields do **not** force the hosted coordinator to rescan: its run methods consume it, and ControlContext already records mutation effects. The hosted problem is coarse effect reporting. Public mutation does matter if the standalone driver is redesigned to rely exclusively on precise notifications; that API would then need to make unreported mutation impossible.

## Path reviewed

| Stage | Current ownership and work | Main remaining opportunity |
| --- | --- | --- |
| Request creation | Generated RequestHook, prepared parameters, admission credit, owned Call and Inbox | Preserve the original operation identity into publication |
| Public pipeline | PipelineState, pending projection queue, queued client, scoped route, exported Hook and Projected lifetime wrapper | Bind a projection directly to its operation and normalized path |
| Hosted admission | Calls slots, optional Invocations binding, immediate/queued/local routes, reply-owner index | One explicit operation binding; retain distinct pre-question and wire lifetimes |
| Protocol | Branded graph, authoritative question/answer/import/export ledgers, exchanges and observers | Notify affected owners rather than rediscover them |
| Execution | Worker ready queue and stages, local/foreign hooks, results staging | Preserve specific protocol readiness as well as application readiness |
| Output | Engine FIFO → Output flush tracker → vat Writer → OutgoingStream → local transport FIFO → OutputDriver | One hosted output queue and one owner of deferred flushing |
| Input | Buffered frame stream → vat Reader → owned Frame/admission → result owner | One transport contract with validated frame ownership |
| Completion | Exchange → WireCall → owned capability table/Payload → completion and pipeline interests | Deliver to the registered owner after its required observers settle |
| Retirement | Endpoint release journal, graph retirement, Finish/Release and observer cleanup | Queue the exact obligation when it is created |
| Hosting | Runtime progress/output lanes → RpcSystem TaskSet → Coordinator lanes | Remove redundant scheduling only after notifications and batching ownership are explicit |
| TLS/QUIC | TLS framing over Tokio; QUIC router, direct stream, packet/recovery driver | Preserve security/recovery; coordinate deferred output through an explicit interface |

The Rust and C++ fixtures agree: dependent-read waits for open's capability before read; pipelined-read sends read through the promised capability before awaiting open. Sources: [Rust fixture](../../../../benchmarks/rpc/capability-workflows.rs#L145), [C++ fixture](../../../../benchmarks/rpc/workflows.c++#L97).

## Evidence and its limits

The latest [accepted comparison](../2026-10-22-pipeline-cleanup/README.md) has five runs per cell with mean and spread. Portable remains slower than legacy in all four dependent/pipelined TLS/QUIC cells. The cleanup-cursor change produced mixed latency results and essentially unchanged instructions. It does not support another broad speedup claim.

Whole-peer successful socket writes/reads already match legacy for TLS: 54/53 for dependent-read and 30/29 for pipelined-read. C++ has 56/55 and 32/31 in those lifecycle traces. QUIC portable still has three more successful lifecycle writes and reads than legacy. These include startup/shutdown, ciphertext and QUIC control traffic; they are not RPC message counts. There is no equivalent C++ QUIC adapter.

For this review, eight existing accepted Callgrind captures were re-annotated with exclusive instruction/conditional-miss attribution: two peers × two cases × two transports, each with 10 warmups and 1,000 measured operations. No new execution measurements were taken. Raw-profile hashes, annotation command, per-file values and totals are in [profile-attribution.json](profile-attribution.json).

| Selected source attribution, both peers | TLS dependent | TLS pipelined | QUIC dependent | QUIC pipelined |
| --- | ---: | ---: | ---: | ---: |
| Coordinator, excluding transport/multiparty files | 4.90% | 5.57% | 3.29% | 4.36% |
| Maintenance file | 1.97% | 2.92% | 1.32% | 2.29% |
| Calls files | 0.97% | 1.11% | 0.65% | 0.87% |
| Hosted transport adapter | 1.26% | 1.06% | 0.84% | 0.83% |
| Outer runtime/connection/TaskSet files | 1.48% | 0.82% | 1.00% | 0.65% |
| Multiparty control files | 0.55% | 0.53% | 0.37% | 0.41% |

These are percentages of **whole-process simulated instructions**, including startup, handshake and shutdown. They are disjoint source-file attributions, not complete component costs, removable overhead, native time, or predicted gains. Inlined work can be attributed to callers and library files. Annotation totals were checked against raw summaries; tiny unreported row residuals are recorded. Inclusive costs were not added.

The evidence supports distributed ownership/scheduling work as a target. It does not identify one removable function that will deliver 30% end-to-end improvement. Core source instruction counts are almost identical between the corresponding TLS and QUIC captures; reducing that work can benefit both, but transport and scheduling costs still differ.

## Ranked changes

### 1. Give every outgoing operation one completion binding and its own observer list

**Highest priority for pipelines and dependent reads.**

[Calls](../../../../crates/capntproto-rpc/src/scoped/wire/calls.rs#L99) owns application slots and a separate reply-ID lookup. `immediate_receiver()` excludes used pipelines; those return to a rotating scan. `send_next()` searches the application entries for a ready task. [WireCall](../../../../crates/capntproto-rpc/src/scoped/wire.rs#L479) represents routes with optional task, receiver, direct stage, pipeline, local results and tail response fields.

The protocol already has a generational question key. Attach an opaque local operation token to its authoritative exchange/question owner at publication. Keep the same operation token through queued invocation → wire publication → completion, with an explicit route/state enum. Deliver Return, cancellation and resource readiness to that owner. Remove ReplyOwners and the small-vector/hash-map lookup split once all routes use the binding.

Associate observers with their exact question or pre-publication invocation. Currently [take_output](../../../../crates/capntproto-rpc-wire/src/owned/connection/awaiting.rs#L205) asks both observer stores whether any observer belongs to the question. [Invocation bindings](../../../../crates/capntproto-rpc-wire/src/owned/connection/invocation_pipeline.rs#L64) scan slots; [ordered propagation](../../../../crates/capntproto-rpc-wire/src/owned/connection.rs#L741) can scan exchanges and then observer stores. A per-owner list/count can make the fence constant-time and visit only that owner's affected projections.

A public projection should reference the shared operation and normalized path. For a known remote question, it should not need a generic queued-client resolution merely to acquire a scoped route and export it back into an owned hook. Keep the generic adapter for genuinely foreign/local hooks and policy membranes. [PipelineState](../../../../crates/capntproto-rpc/src/scoped/wire.rs#L1149) and [request_pipelined_prepared](../../../../crates/capntproto-rpc/src/scoped/response.rs#L562) are the concrete integration points.

**Work removed:** extra owner lookup, scan-driven delivery, unrelated observer checks, repeated owned/scoped conversions and some projection/completion wrappers. Exact allocation savings require measurement; several current owners are already cached.

**Must retain:** a canceled protocol question can outlive its application operation; pre-publication invocation identity is not a question ID. Retain generational authorization, response ownership, early/late projections, pipeline-only interest, projection memoization, embargoes and handoffs. Do not merge these distinct lifetimes into one refcount.

C++ [QuestionRef](../../../../vendor/capnproto/c++/src/capnp/rpc.c++#L2387) and [handleReturn](../../../../vendor/capnproto/c++/src/capnp/rpc.c++#L4022) provide the owner-binding model. [RpcPipeline](../../../../vendor/capnproto/c++/src/capnp/rpc.c++#L2795) still has promise branches, wrappers and a projection cache; copying its semantics does not mean deleting every wrapper.

The previous eager-delivery experiment was rejected: it kept the old architecture and added observer work at delivery. The proposed change must replace those discovery paths, rather than adding another bypass.

### 2. Make readiness and cleanup explicit effects of protocol transitions

[progressed()](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/work.rs#L17) reactivates several lanes and invalidates Calls, ForeignPromises and maintenance scans. `changed()` also invalidates Worker protocol readiness. [Maintenance](../../../../crates/capntproto-rpc-wire/src/owned/connection/maintenance.rs#L246) combines 18 phases, capacity tracking, idle proofs and retry state. Those phases contain necessary protocol operations; scanning all possible owners is not itself required by the protocol.

Record the affected operation/observer/import/export/answer when a transition creates work. Use bounded deduplicated queues or intrusive slot links, reserving notification capacity with the owner. Blocked work should wait for the particular resource that can release it: FIFO space, graph space, barrier completion or application readiness. A busy owner must not spin or block another owner that can free its resource.

Extend the existing effect boundary to return precise owner changes, including changes committed before Pending or errors. Drain Finish/Release obligations through the normal ordered output queue. Remove scan cursors, repeated idle certification and cleanup-order preferences once the replacement covers their producers.

**Work removed:** repeated discovery of pending obligations, empty-phase walks, capacity-dependent scans and broad invalidation. This should improve scaling with retained but inactive state; short single-call cases may gain little.

**API consequence:** hosted precision can be introduced internally. Replacing the standalone core's conservative scan completely requires controlled mutations instead of public mutable ledgers/runtime plus manual invalidation. With no compatibility requirement, narrow that API when converting the core; do not keep two permanent schedulers.

**Must retain:** bounded fairness at budget 1, allocation-free operation with supplied storage, slot reuse protection, cleanup order, reentrant callbacks, retained Busy input and backpressure. Reuse the worker's proven readiness discipline; do not replace its thread-safe wakers with Rc-only wakers.

### 3. Use one hosted frame interface and one owner of output batching

The [vat adapter](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/transport/vat.rs#L74) negotiates optional frame streams, message streams and per-message futures. Writer supports both stream completion and per-frame receipts. The native path already selects the better options, so deleting fallbacks alone may mostly reduce code size and reasoning cost.

Make the hosted transport split explicit at construction: framed input, ordered output, write failure and shutdown, with their owners and resource limits. Keep byte IO adaptation at the edge for TLS, QUIC and Unix streams. Convert repository transports, then remove optional migration interfaces from the portable contract.

The larger opportunity is output ownership: the engine FIFO is transferred into a second transport FIFO and tracked by several flush states. Let hosted publication reserve and transfer an immutable output owner into one ordered queue. A single end-of-turn owner should batch ready frames after application/protocol progress. The scoped core can retain a storage-backed sink implementation without forcing an additional hosted queue.

[Runtime](../../../../crates/capntproto-rpc/src/portable/runtime.rs#L150) already has a progress/output boundary. Use it to remove overlapping batching decisions in [pump](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/pump.rs#L45), OutgoingStream and OutputDriver. Do not remove TaskSet just because it appears in the call stack: it also owns accept/background/protected-call work, and one connection is already polled directly.

**Work removed:** queue handoffs, capability checks on optional interfaces, redundant completion observation, and multiple decisions about when to flush. The native path does not allocate a Promise per outgoing frame today; that is a fallback cost, not a current benchmark cost.

**Must retain:** atomic publication/resource reservation, FIFO ordering, partial writes, bounded batching, independent failure observation, attachment/credit lifetime and shutdown. Queue acceptance, local flush completion and peer RPC completion remain different events. C++'s [queuedMessages/evalLast](../../../../vendor/capnproto/c++/src/capnp/rpc-twoparty.c++#L175) is the scheduling reference.

### 4. Refresh only the capabilities that changed

[ClientHooks::export_retained](../../../../crates/capntproto-rpc/src/scoped.rs#L292) linearly searches for an existing endpoint and a vacant slot. [synchronize](../../../../crates/capntproto-rpc/src/scoped.rs#L429) walks entries to find unsettled routes or changed metadata. Metadata revisions avoid expensive refreshes, but the entry walk remains. [ForeignPromises](../../../../crates/capntproto-rpc/src/scoped/wire/foreign.rs#L40) similarly scans observer slots.

Carry a hosted hook binding on the graph owner where ownership permits, or use an index keyed by the full generational graph key. Send resolution and metadata-dirty notifications to subscribed hooks. Use exact foreign-promise readiness and abandonment notifications rather than polling every registration. Retain the existing endpoint release journal: it already records precisely which final owner dropped.

**Work removed:** repeated registry deduplication scans, settled-route checks and unrelated future polls. The strongest expected benefit is capability-heavy concurrency and cross-service forwarding; the current two-call profiles establish only modest direct source attribution.

Avoid introducing several new independently maintained maps. Share owner bindings with change 1. Preserve original membrane hooks, late introduction/FD metadata and callback-safe release. Generic SlotStorage still has some linear vacant-slot searches, but LocalTable already has a free list; there is no justification for replacing every table.

### 5. Keep unsupported network-control features out of ordinary two-party turns

Portable connection setup always constructs NetworkControl. Its [poll/preserves_idle/set_idle](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/multiparty.rs#L1119) repeatedly checks multiple empty collections; `network_idle()` also looks for worker redirects.

Select the control mode from an explicit, immutable transport capability contract at connection construction. A genuinely two-party transport can use the existing TwoParty handler without allocating/checking multiparty state. Networks supporting handoffs, introductions or joins keep the full handler, allocated when appropriate.

**Work removed:** repeated negative checks and some setup ownership. This is a smaller, more contained simplification; multiparty source attribution is about 0.4–0.6% of whole-process instructions in these captures, not a large predicted latency win.

Ordinary two-party capability promises, reflected-route embargoes and tail-call semantics must remain. A callback returning false today is not automatically an immutable promise that a feature can never activate; the new capability contract must make that distinction explicit.

### 6. Integrate QUIC's deferred packet flush with the output turn, after the RPC changes

The active [router](../../../../src/rpc/quic/endpoint.rs#L177) already feeds Quiche directly. [State::receive](../../../../src/rpc/quic/driver.rs#L34) wakes the stream reader and the packet driver; the driver then [yields before flushing](../../../../src/rpc/quic/driver.rs#L243) so RPC output can share the flight with ACKs. This is a timing-based coordination point between two schedulers.

Expose a transport end-of-turn flush hook so ready RPC output can explicitly precede the next packet flush. Preserve a bounded fallback for ACK-only traffic, pacing deadlines and a slow/blocked application. Keep packet recovery alive independently after the public stream closes.

**Potential work removed:** redundant notifications and a forced executor turn. Confidence is lower than the core recommendations: sharing a UDP endpoint across connections, recovery timers and flow control are real reasons for separate transport ownership. Do not collapse the whole endpoint into one RPC connection or claim that the three extra lifecycle syscalls are caused by this yield without packet-level evidence.

Retain the current scalar QUIC stream writes initially. Both stack-gathering and owned-gathering experiments [regressed the full campaign](../2026-10-22-quic-routing/README.md#experiments-and-verification); fewer calls to Quiche did not establish lower latency.

## Changes I would not prioritize

- More one-/eight-entry special cases. The rejected eager-delivery experiment and mixed cleanup-cursor results argue for removing an entire discovery path first.
- Universal zero-copy. Input deliberately copies small frames and shares sufficiently large frames; a shared owner can cost more than a small copy. Preserve the measured policy while simplifying the ownership interface.
- Removing validation wholesale. A private validated frame owner can carry exact segment extents across internal boundaries and avoid reconstructing them, but untrusted input, traversal limits, capability authority and attachments still require their respective checks.
- Flattening crates or deleting fixed-storage support. Source organization and generic types are not evidence of runtime overhead. Preserve a single semantic engine with different storage/sink implementations.
- Removing locks/atomics from ordinary Wakers because the runtime is local. User futures may retain and invoke wakers from another thread; current tests explicitly cover that.
- Dropping TLS verification, QUIC flow control/pacing, or retirement messages to improve a benchmark.

## Implementation and qualification order

1. Introduce the operation binding and per-owner observer accounting. Convert both immediate and queued routes, then delete the old reply index/lookup branches. Keep the existing scheduler as the driver during this ownership change.
2. Use that ownership to target hosted call/protocol readiness. Convert cleanup producers one class at a time, then remove the superseded scans. Narrow core mutation APIs when converting standalone maintenance.
3. Standardize the frame transport boundary and consolidate hosted output ownership; preserve the one deferred output turn and bounded fairness.
4. Measure hook/foreign-promise indexing and the explicit two-party control mode. Attempt QUIC turn integration only after the common path is stable.

For each replacement, count actual work separately from wall time: operation polls, unrelated slot visits, notifications, queue transfers, allocation/refcount activity, copied bytes, RPC frames, and positive/attempted socket IO. Compare owner counts before and after fanout growth followed by sparse activity, not only at a table's initial capacity.

Add a network diagnostic that parks many calls and releases one, including a Return with one affected pipeline among many unrelated questions. Preserve old/new generation, cancellation, early pipeline, reflected-route, capacity-one and shutdown cases. Existing deep/concurrent and multiservice fixtures should remain equivalent across implementations.

Then run the 24-case TLS/QUIC suite five times with mean, sample SD, range and tail values, frozen current portable and legacy controls, and equivalent C++ TLS fixtures. Profile separately from native timing. Preserve payload, security, credit and output-order checks. Require no unexplained read/write increase and investigate any timing regression relative to spread. C++ QUIC remains unsupported, so a cross-transport latency comparison cannot qualify it.

No percentage speedup is promised by this review. The first success criterion is a smaller set of ownership/scheduling mechanisms that demonstrably performs less work without regressing correctness or the repeated end-to-end measurements.

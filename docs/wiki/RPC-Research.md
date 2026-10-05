# RPC, pipelining and Cap'n Proto research

Research date: 2026-10-01, America/Edmonton.

The strongest opportunities are **bounded ordinary-call admission, visible
in-flight resource usage, pipeline-preserving application APIs, and cheaper
message construction**. The runtime already implements the core pipelining
mechanisms. We should first make those mechanisms easy to use and safe under
load, then qualify transport-specific optimizations. Changing Cap'n Proto's
encoding is not required for these improvements.

The original research audited the source, added a reproducible protocol
experiment and recorded implementation priorities without changing production
RPC behavior. The first implementation follow-up now adds opt-in outgoing Call
count admission and output/runtime diagnostics; see the
[application contract](RPC-Applications.md#outgoing-admission-and-resource-observation).
Byte/principal budgets, local unresolved-call admission and bounded batches are
still pending. The frozen experiment below describes the original source hashes;
it is not a performance measurement of these later runtime changes.

## Existing implementation and remaining opportunities

| Area | Implemented here | Useful next work |
| --- | --- | --- |
| Promise pipelining | Promised-answer targets, queued clients, resolution and embargo ordering | Preserve pipelines in application helpers; bound unresolved calls and measure deep/wide dependency graphs |
| Result publication | `Results::set_pipeline`, `set_pipeline_from`, typed `PipelineBuilder` | Prefer independent capability publication when the response contains large data; document readiness and late-error contracts |
| Call hints | Generated scalar results disable pipeline bookkeeping; `send_for_pipeline` suppresses ordinary result observation | Make ownership/error differences clear; benchmark returned capabilities and wire traffic, not just echo calls |
| Tail calls | Typed forwarding helpers, consuming structured replies, return redirection and third-party answer adoption | Qualify explicit peer profiles and maintain lifecycle contracts |
| Streaming | Fixed/variable/adaptive windows and generated `-> stream` methods | Aggregate budgets across streams, final completion barriers, mixed small-call/bulk benchmarks |
| Output | Scatter/gather batching, FIFO shutdown fences, active/queued diagnostics and opt-in outgoing Call-count admission | Cap batch work, add byte/principal budgets and reuse framing metadata |
| Input | Buffered short-lived messages, independent retained payloads, reader limits | Bound retained contexts and capability tables; test control-message progress under overload |
| TCP/TLS/QUIC | Owned drivers, TLS/mTLS, quiche v1, native three-party routes | Measure loss/latency and copy costs; consider explicitly negotiated bulk separation |

Primary implementation paths are [RPC dispatch](../../crates/capntproto-rpc/src/rpc.rs),
[queued capabilities](../../crates/capntproto-rpc/src/queued.rs),
[generated call hints](../../crates/capntproto-codegen/src/codegen.rs),
[flow control](../../crates/capntproto-rpc/src/flow_control.rs), and
[byte-stream output](../../crates/capntproto-futures/src/write_queue.rs).
The [parity audit](Cpp-Parity.md) describes the wider existing feature set and
its limits. It should not be interpreted as complete interoperability with every
Cap'n Proto implementation.

## What the new protocol experiment establishes

The [probe](../../examples/rpc_pipeline_probe.rs) runs the production bilateral RPC
engine and serialization through two in-memory byte streams. Each direction has
a bounded frame relay. A frame is scheduled relative to its own arrival, so
propagation delays overlap; the relay does not add a fresh sleep after every
previous frame. Frame order is preserved. Bootstrap resolution is outside the
observation window.

Tokio's paused clock models RTTs of 0, 2 and 20 ms. These are **simulated times**,
not measured network latency or CPU throughput. Five scenarios at each RTT make
15 cases; three fresh processes produced identical times and wire counters.
Assertions verify values, invocation counts, promised-answer targets, generated
hints, Return counts and early publication before parent completion.

Frozen [results](../../research/reports/rpc-pipeline/2026-10-01/results.json) and
[environment/source hashes](../../research/reports/rpc-pipeline/2026-10-01/environment.json)
are captured by the [runner](../../dev/src/probes.rs).

For four dependent capability-returning calls followed by a scalar call:

| Style | Simulated reply time at 2 ms RTT | At 20 ms RTT | Calls / Returns | Framed bytes, client→server / server→client |
| --- | ---: | ---: | ---: | ---: |
| Await each capability before the next call | 10 ms | 100 ms | 5 / 5 | 704 / 704 |
| Send through each returned pipeline | 2 ms | 20 ms | 5 / 5 | 832 / 704 |
| Pipeline-only intermediate calls | 2 ms | 20 ms | 5 / 1 | 832 / 128 |

The extra forward bytes encode promised-answer paths. Pipelining saves dependency
round trips; it does not necessarily reduce request bytes. Pipeline-only calls
remove four observed Returns in this compatible implementation, while retaining
the five method invocations. The probe keeps parent promises or pipelines alive
through the terminal result. Wire totals cover the observation prefix, including
stream framing, **before final Finish/Release cleanup**; they are not full
connection-lifetime bandwidth measurements.

For a parent that takes 50 ms of simulated server time:

| Publication policy, 20 ms RTT | Child reply | Parent reply |
| --- | ---: | ---: |
| Publish capability when parent finishes | 70 ms | 70 ms |
| Publish capability immediately using `set_pipeline_from` | 20 ms | 70 ms |

Early publication changes when the child can run, not when the parent completes.
The zero-delay cases advance no virtual time except for the deliberate 50 ms
server delay. They say nothing about CPU cost. The experiment has no bandwidth
limit, congestion, packet loss, jitter, TLS, QUIC, storage or three-party routing.
It demonstrates protocol behavior; real transport performance still needs
measurement. This is consistent with Cap'n Proto's capability-pipelining model.
[Official RPC explanation](https://capnproto.org/rpc.html).

## Priority 1: bounded admission and complete resource diagnostics

The output queue is an unbounded `VecDeque`. `Sender::send()` synchronously
enqueues a complete message. Its receipt resolves after writing and flushing;
dropping the receipt does not remove the message. `Receiver::next()` takes the
entire pending queue as one active batch and resets the pending metrics. Therefore
`message_count == 0` can coexist with a large batch blocked in the writer. The
[existing regression](../../tests/outgoing_queue.rs) explicitly checks that contract.
The new `output_snapshot()` preserves that pending-only API and separately
reports the active batch through write/flush, failure and cancellation.

Separately, unresolved clients use [SenderQueue](../../crates/capntproto-rpc/src/sender_queue.rs)
to retain calls. A cap on socket buffering alone cannot bound pending calls,
question/answer state, retained results or capabilities. Streaming credit is also
not an ordinary-call admission limit.

Proposed implementation sequence:

1. Add separate observations for queued and active message bytes/count/age,
   live questions/answers/imports/exports, unresolved-call counts and held result
   contexts. Keep the existing pending-only snapshot contract intact. Label each
   counter's lifetime and whether it is exact or sampled; do not log payloads or
   capability tokens to obtain these measurements.
2. Introduce per-connection and trusted-principal ordinary-call count/byte
   reservations, including local unresolved calls. Reserve before materializing
   large payloads and allocating wire-visible state where possible. A typed
   builder can require a bounded size allowance and checked growth. Distinguish
   rejection before sending from uncertain execution after sending.
3. Bound each write batch's bytes/messages and work per executor turn while
   retaining FIFO order, partial-write ownership and flush/shutdown fences. A
   batch limit alone does not bound the total backlog. Benchmark its fairness
   and throughput tradeoff before choosing defaults.
4. Reserve resources for protocol progress and bound incoming application work
   separately. If calls must be rejected, consume and account for their protocol
   state correctly, produce the appropriate exception and release capabilities.
   Never silently drop an already-enqueued Call, Return, Finish or Release.

The current incoming `RpcSystem::set_flow_limit()` pauses the **entire reader**
above its threshold, including Return and Finish. It also admits the call that
crosses the threshold. The source documents possible deadlock when accepted work
waits on the peer. A replacement must keep required control/response progress
possible without admitting unlimited application work. Giving control messages
a separate queue and blindly overtaking earlier messages is not sufficient:
ordering and capability lifetimes still apply. Hard resource exhaustion may
require terminating a peer connection cleanly instead of buffering indefinitely.

Acceptance cases: blocked writer, many principals, an unresolved capability,
large retained responses, mutual callbacks, shutdown at a full budget, partial
write failure and clients retaining pipelines after dropping response promises.
Measure RSS as well as accounted bytes; neither reader traversal limits nor
payload credits alone describe total process memory.

## Priority 2: APIs that preserve useful pipelines

Application helpers should return a generated `RemotePromise`/`PendingCall` or
an owned wrapper containing both completion and pipeline. An `async fn` that
awaits discovery before returning a capability forces callers to wait unless it
explicitly exposes a future capability. The generated request and field APIs
already provide pipeline access. An additional generic actor framework would
not improve this primitive.

Use capabilities as intermediate results when callers need to invoke further
operations: `open() -> Session`, `beginUpload() -> Upload`, or
`snapshot() -> ReadView`. Scalar values cannot be read from a pipeline and used
as arbitrary arguments before the response arrives. If the next operation
depends on an integer or branch decision, move the decision into an appropriate
server-side method or await it. Pipelining is not arbitrary remote computation.

The [application guide](RPC-Applications.md#promise-pipelines-and-completion)
now shows the existing APIs and their ownership rules. The original audit proposed two small wire-compatible ergonomic changes;
the first has since been implemented:

- A typed `Results<T>::tail_call(Request<P, T>)` convenience method. The original
  [tail-call tests](../../tests/tail_transfer.rs) reach through
  `results.hook.tail_call(request.hook)`. Type the result compatibility and retain
  the existing rule that the result payload must not already be initialized.
- A typed local call-options/admission wrapper that preserves the pipeline while
  exposing cancellation, local deadlines and observation. Define its permit
  lifetime around all live owners, not only the response future.

Implementation follow-up: the typed legacy tail-call helper is now available.
New bindings can select `structured_replies(true)` for consuming `Reply<T>`,
editable `ReplyBuilder<T>` and immutable `PublishedReply<T>` stages. That profile
prevents writing-then-forwarding and editing/republishing after publication at
compile time. Ordinary `RemotePromise` is directly awaitable and supports
`into_parts()`; both client facades support consuming `with_params` construction.
See [the API contract](RPC-Applications.md#structured-server-replies). This is an
API/lifecycle change, not a new throughput result or a byte-budget implementation.

`send_for_pipeline()` is appropriate when only result capabilities are needed.
It is not fire-and-forget; retain the pipeline or derived clients while work is
needed. Use ordinary `send()` when the parent outcome/data matters. A pipeline-only
request may receive a Return from an older peer, and the existing implementation
has compatibility handling; the one-Return probe result is not a universal peer
guarantee. Generated calls already set `noPromisePipelining` conservatively for
results which cannot contain capabilities, so that optimization needs no new
feature switch.

Early publication deserves particular care. `set_pipeline()` snapshots the
current result structure and copies it in the RPC implementation. For a large
response, constructing a small independent `PipelineBuilder` and calling
`set_pipeline_from()` avoids copying that large response solely to expose its
capabilities. The final response must contain the same capability identities or
promises resolving to them. Publish only after authority and usable behavior are
established. A later parent failure cannot undo child effects or retract an
already published capability. E-order is delivery order on a reference, not
transactional execution of all async handlers.

## Priority 3: serialization, allocations and scheduling

Cap'n Proto's segmented binary representation already permits direct reading of
aligned message data. This does not make a TLS/QUIC request end-to-end zero-copy:
transport encryption, framing and retained ownership remain relevant.
[Encoding and stream framing](https://capnproto.org/encoding.html).

Specific candidates from this implementation:

| Candidate | Current source behavior | Qualification needed |
| --- | --- | --- |
| Extend output framing scratch | Small batches (up to two messages with up to two segments each) use stack framing; larger batches still allocate framing metadata | Allocation counts, short/partial writes and cancellation; retain every referenced segment until write completion |
| Size-aware generated request construction | Generated `*_request()` passes no size hint; lower-level client calls and result builders accept hints | Typed ergonomic size hints, cap count inclusion, small/large/segmented payloads; hints must not become unchecked limits |
| Bounded receive-buffer pools | Short-lived control messages share buffered input; single-segment metadata is inline, while retained payloads detach into owned storage | Pool byte cap, retained-reader lifetimes and no reuse while any reader/capability still owns data |
| Specialize local output coordination | Generic write queue supports cross-thread `Send` use, while common RPC messages contain local `Rc` | Profile mutex/metadata cost; compare a local queue without removing the existing generic cross-thread contract |
| Reduce transport bridge copies | Bilateral QUIC uses a 64 KiB duplex bridge and 16 KiB staging arrays | Backpressure, cancellation, stream FIN acknowledgement, TLS ownership and packet pacing under load |

Further improvements are hypotheses, not measured speedups. Existing batching already uses
scatter/gather writes; adding batching from scratch is not missing work.
Do not remove traversal/nesting validation to speed up decoding. The serialization
format needs checked pointer traversal even when no separate decode allocation
is required. [Reader limits](https://capnproto.org/cxx.html#security).

Keep the current wire encoding. Packing is a bandwidth/CPU tradeoff worth testing
on sparse records versus dense blobs, but the normal RPC adapter uses unpacked
framing. A packed/compressed transport profile must be explicitly agreed by both
peers and enforce decoded-size/resource limits. Schema evolution should follow
the existing ordinal, type and default-value compatibility rules.
[Evolution rules](https://capnproto.org/language.html#evolving-your-protocol).

## Priority 4: streaming and QUIC without losing ordering

Streaming completion grants permission to send more; it does not certify remote
method completion or durable storage. Applications need an ordinary final method
to report accumulated errors and completion. The upstream streaming design
describes this pattern; this fork already implements streaming support in Rust.
Its 2020 statement about Rust support is historical, not this repository's status.
[Streaming semantics](https://capnproto.org/news/2020-04-23-capnproto-0.8.html).

Benchmark ordinary calls alongside several streams with fixed, variable and
adaptive windows. A rough initial window estimate is bandwidth × RTT, constrained
by connection/principal budgets. Separate per-capability windows can add up to an
unbounded aggregate unless the connection also has admission controls. For QUIC,
derive estimates from the QUIC path or measured RPC acknowledgements; a UDP socket
send-buffer size is not the QUIC congestion window. TLS currently snapshots the
TCP send-buffer size, while the plaintext TCP adapter samples it through a getter.

The [bilateral QUIC adapter](../../src/rpc/quic/driver.rs) carries RPC bytes on stream
0. It inherits ordering within that stream, including loss blocking later bytes.
QUIC's cross-stream loss isolation helps only when independent traffic actually
uses independent streams. [RFC 9000, section 13](https://www.rfc-editor.org/rfc/rfc9000.html#section-13).

The first useful multiplexing experiment is a negotiated bulk lane or independent
RPC connections for clearly independent workloads. One QUIC stream per Call is a
larger protocol adaptation: questions, imports, exports, Return/Finish ownership,
promise resolution and embargo ordering currently share a connection. Design
those dependencies and fences before splitting messages across streams. Existing
TCP/TLS and quiche v1 support should continue using their current ordered profile
until an additional profile is specified and tested.

## Priority 5: deadlines, receipts and explicit interop profiles

A response timeout does not establish that a method did not execute. Dropping a
parent future may also leave live pipelines or other owners. Cancellation policy,
Finish and disconnect are resource/lifetime operations; none is a durable rollback
receipt. For storage-backed mutations, add an application operation identity with
a retained result/deduplication record in the same durable commit as the mutation.
Bind it to the authenticated principal and operation, and specify retention and
retry horizons. RPC question IDs are connection-local and reused; they are not
durable operation identities. This builds on the [storage work](Storage-Worker.md).

The pinned schema documents E-order, connection-local table lifetimes and the
pipeline hints used here. Preserve those contracts when adding admission or
scheduling. [Pinned RPC specification](https://raw.githubusercontent.com/capnproto/capnproto/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.capnp).

Third-party capability introductions, answer adoption and active pipeline
migration already exist, but support differs by peer/network. Native
`supports_third_party_answers()` and `supports_pipeline_join_fence()` are network
policy, not a general peer negotiation exchange. The [answer-adoption guide](Third-Party-Answers.md)
notes that the pinned C++ dispatcher does not implement the adoption messages.
Qualify these as explicit interoperable profiles; the presence of schema fields
does not prove a peer implements them. Add a version/feature agreement in the
authenticated setup profile before enabling new transport behavior, with a tested
forwarding fallback where possible. The project's TLS/QUIC ALPN is project-specific,
not an upstream-assigned identifier.

## Validation and next implementation slice

At the recorded research revision, the focused suites passed **68 tests**: call hints (7), incoming flow
(6), native vats (8), outgoing queues (13), result pipelines (11), secure RPC
(18) and tail transfer (5). They include their checked-in/TLC replay paths,
pinned C++ comparisons, TCP/TLS/mTLS and quiche v1 scenarios. These results
validate those finite cases, not universal interoperability or production capacity.
The new probe passed its 15 scenarios in three identical repetitions and Clippy
with warnings denied using the minimal feature configuration.

The original first slice is partly implemented: active/queued diagnostics,
opt-in outgoing Call-count admission, typed tail calls and structured reply
ownership are available. Next extend admission to **byte/principal budgets and
local unresolved calls**, and bound write-batch work. Measure real RPC workloads
before changing allocation strategy or queue primitives. This keeps the existing pipelining and third-party machinery usable
while giving it explicit load boundaries.

Qualification matrix for those changes:

- Chain depths 1/4/16, fan-out 1/16/128, plain and capability-rich results,
  early success/failure, ordinary versus pipeline-only sends and tail forwarding.
- Real TCP/TLS and quiche v1 with RTT, jitter/loss, mixed bulk/control traffic,
  burst and fixed-rate arrival workloads, plus stalled peers and readers.
- Queue wait, service/response latency, rejected/expired/uncertain outcomes,
  allocations, RSS, retained words, table high-water marks and route changes.
- Bounded interleavings for admission/release, pending/active batch ownership,
  callback progress, cancellation, shutdown and three-party migration. Expand
  hostile-wire fuzzing without assuming current finite proofs cover new scheduling.

Reproduce the protocol evidence without compiling during capture:

```sh
cargo build --locked --no-default-features --example rpc_pipeline_probe
cargo run --locked -p capntproto-dev -- probe rpc --output target/rpc-pipeline-local
cargo clippy --locked --no-default-features --example rpc_pipeline_probe -- -D warnings
```

The output directory must be new. Frozen results are observations of the recorded
source hashes and toolchain; new behavior belongs in new evidence. Repeated virtual
results establish repeatability of this schedule, not statistical confidence.

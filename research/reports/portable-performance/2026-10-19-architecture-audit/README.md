# Portable architecture audit — October 19, 2026

The best next simplification is to preserve the identity of ready work through the hosted scheduler. The current implementation repeatedly reconstructs readiness by scanning lanes and operation slots, then adds special paths to avoid those scans. A diagnostic of the unchanged coordinator reproduces the cost: with sixteen parked services and a budget of 64, waking one service causes **18–26 polls of unrelated service futures before its reply is published**.

This audit examines source at `b352a21345deebcc9c1d2b27aa7ab32c94c5658b` and C++/KJ at `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. It adds reproducible diagnostics and architectural findings. Production implementation and the existing [24-case performance results](../2026-10-19-maintenance-branches/README.md) are unchanged. Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12).

## Correction to the previous assessment

Public driver fields are **not, by themselves, the reason the hosted coordinator needs broad scans**. Its [run entry points consume `self`](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/coordinator/lifecycle.rs#L15). Application code cannot concurrently obtain that coordinator through those fields. External controls already use an [effect-reporting `ControlContext`](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/coordinator/control.rs#L15), whose destructor commits invalidation even after Pending or errors.

The actual hosted limitation is lost wake identity plus coarse effect reporting. Ordinary [`WorkWake`](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/coordinator/scheduler.rs#L126) sets every application lane ready. [`changed()`](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/coordinator/work.rs#L10) invalidates call, worker, foreign-promise and maintenance scans. A worker then finds the ready operation by revisiting slots. The standalone portable driver still permits caller-directed mutation, so replacing its conservative maintenance scans needs explicit transition effects or a documented conservative adapter. That lower-level requirement need not block a hosted worker ready queue.

## Ranked simplifications

| Priority | Current complexity | Simpler ownership or scheduling boundary | C++ reference |
| --- | --- | --- | --- |
| 1 | Six coordinator lanes, broad notifications, per-lane scans, then separate direct-call/direct-worker paths. A pending worker traverses Prepare, Execute, Collect and Publish through repeated rotations. | A bounded queue of ready operation keys; each suspended operation registers its own notifier. Keep a runnable operation selected while advancing its ready stages within the shared budget. Suspend only for an actual dependency, capacity or budget limit. | [KJ EventLoop::turn](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/kj/async.c++#L1828) removes and fires one queued event. [TaskSet::Task](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/kj/async.c++#L265) registers its own readiness. [Answer completion](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L3943) directly continues to sendReturn. |
| 2 | Outgoing progress spans question ownership, exchange storage, invocation bindings and hosted WireCall state. Immediate replies additionally maintain a small-vector scan / ReplyOwners hash-map split. | One explicit local operation binding carried by the authoritative question/exchange owner, with a typed completion route. Deliver to that binding rather than maintaining an independent reply-ID index. Retain pre-question invocation bindings only where they are needed for queued/local calls. | [Question::selfRef](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L690) links the protocol record to its local owner; [handleReturn](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L4022) fulfills that owner. |
| 3 | Eighteen maintenance phases repeatedly rediscover Finish, Release, retirement and pipeline obligations. Coarse invalidation and idle proofs require many exceptional paths. | Record dirty owners when the owning transition creates an obligation; drain bounded, generation-checked queues. Keep unusual untracked mutations on a conservative path until every producer is covered. | [QuestionRef destruction](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L2396) owns Finish creation and the point at which the question can be erased. |
| 4 | The hosted transport negotiates optional frame streams, message streams and per-message futures; output can use a stream or per-frame receipts. The adapter retains state for these combinations. | Select one explicit input/output contract at connection construction. A typed transport split can own input, ordered output, failure and shutdown directly; a byte-stream adapter can remain at the transport boundary. Remove optional migration fallbacks from the hosted API once repository transports are converted. | [TwoPartyVatNetwork output](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc-twoparty.c++#L170) owns the message batch and schedules one deferred write through previousWrite/evalLast. |

These are proposed changes, not implemented speedups. Priority 1 has new measured work-count evidence. Priorities 2–4 are source-grounded designs requiring their own prototypes and measurements. The engine/wire/hosted crate separation serves real allocation and ownership requirements; flattening those crates is not the recommendation.

### 1. Preserve readiness instead of inferring it

[`Worker::poll_using`](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/server/worker.rs#L336) rotates through the entry vector. Progress resets the scan, so a completed execution can revisit parked siblings while reaching collection and publication. The coordinator's general dispatcher is 20,881 bytes in the frozen release executable; its pipeline client/server self costs include 12,165/16,252 simulated branch misses in the previous complete-process capture. Those figures exclude callees and include startup, warmup and teardown; they are not hardware counters or attributable latency savings.

The repository already has [ReadySet](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/task_set/ready_set.rs#L1), which preserves task readiness for the outer executor. Its notification design is a useful starting point. A hosted operation queue should carry slot/generation keys and borrow the driver when serviced; it should not require moving a branded driver into a separate static future.

Every readiness source must be explicit: initial admission, the operation's future, cancellation/Finish, early pipeline results, authenticated handoff, and capacity becoming available. An output-blocked operation belongs on the output-capacity wait list. A graph-blocked operation needs the relevant graph notification. Coalescing repeated wakes, stale keys after slot reuse, self-wakes, parent-task replacement and bounded fairness remain part of the queue contract. Merely filtering application-future wakes would miss protocol-driven progress.

### 2. Consolidate the completion binding, not unlike lifetimes

[`WireCall`](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire.rs#L479) represents routes through optional task, receiver, direct-stage, pipeline, local-result and tail-response fields. [Calls](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/calls.rs#L99) separately maintains reply ownership and retirement. A route enum would make valid combinations explicit; a local completion token on the question/exchange owner could remove the additional reply-ID lookup and threshold-dependent code.

These are not duplicate protocol-authority tables. A queued invocation can exist before a wire question, and a canceled question must survive its application owner until the late Return is handled. C++ preserves that distinction too: QuestionRef can disappear while Question remains. Keep that tombstone, branded/generational checks, pipeline observers, third-party authorization, response reader limits and credit lifetime. Do not equate a wire ID with a storage index.

### 3. Make cleanup an effect of the owner transition

Question and import ledgers already maintain pending-Finish/pending-Release counts. The next useful extension is identifying the pending owners, rather than repeatedly walking phase capacities to find them. The generic graph dispatcher also [scans queued targets in next_ready](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc-engine/src/clients/dispatch.rs#L446). Target-specific wait lists could eventually replace that scan, but must preserve the oldest-ready ordering across promise resolution, embargoes and capacity retries.

C++ destructor-driven cleanup is a reference for where obligations originate. In the allocation-free driver, explicit release operations can emit the equivalent bounded cleanup work without introducing destructor IO or allocating per event. Record effects before invoking callbacks; callbacks can create further work even when a poll returns Pending.

### 4. Choose the transport shape once

The [Connection trait](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/lib.rs#L470) and [vat adapter](https://github.com/DericHuynh/capntproto/blob/b352a21345deebcc9c1d2b27aa7ab32c94c5658b/crates/capntproto-rpc/src/scoped/wire/coordinator/transport/vat.rs#L74) expose the optional paths. Some are already bypassed by the native TCP path, so removing them is not yet a demonstrated benchmark improvement. The simplification is a single ownership contract selected at setup, with frame/attachment lifetimes and ordered flush behavior encoded in its types. Keep publication separate from physical flush completion; combining them would reintroduce the earlier scheduling bottleneck.

## New wake-scaling diagnostic

The coordinator probe parks every service, waits for the scheduler and notifications to become idle, then wakes the first, middle or last service. It counts actual application-future polls until the selected Return enters the FIFO. It validates exactly one reply, its question ID and payload, then terminates and verifies buffer reclamation. It does not run sockets or measure time.

There are five repetitions for each combination of widths 1/2/8/16, budgets 1/8/64 and borrowed/owned output queues: 360 coordinator trials. Duplicate first/middle/last selections for small widths are retained. Every repeated configuration has identical counts; both output modes agree. The ranges below are across selected positions, not run spread. The selected future is polled exactly once in every trial.

| Parked services | Unrelated polls, budget 1 | Unrelated polls, budget 8 | Unrelated polls, budget 64 |
| --- | ---: | ---: | ---: |
| 1 | 0 | 0 | 0 |
| 2 | 2–3 | 1–2 | 1–2 |
| 8 | 14–21 | 7–18 | 8–13 |
| 16 | 30–45 | 21–29 | 18–26 |

A separate isolated Worker probe contributes 60 trials. The [KJ reference probe](kj-ready.c++) contributes 60 trials with the same parked widths and selected positions; fulfilling one promise executes one selected continuation and zero sibling continuations. **KJ continuations and Rust future polls are different events. This is not a numerical cross-language speed comparison or a full C++ RPC benchmark.** The source comparison explains why KJ need not poll parked promises to find the fulfilled one.

The current deep/concurrent TCP fixtures exercise forwarding and burst completion, but do not deliberately hold N−1 operations parked while releasing one controlled completion. Add that sparse-readiness pattern before replacing the scheduler. Also vary the number of ready operations independently of width, cancel a parked operation, release several operations in a burst, and run the same pattern through the multi-service topology. Measure end-to-end mean/spread and all-process I/O alongside the deterministic poll counts.

The temporary diagnostics were removed after capture; all production and test files match the audited commit. The worker and coordinator diagnostic tests pass, including all reply/reclamation assertions. Full workspace, coverage and formal suites were not rerun for this documentation-only audit.

## Reproduction and evidence

Use a separate clean checkout of the audited Rust commit. Apply [coordinator-probe.patch](coordinator-probe.patch), then run:

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --locked -p capntproto-rpc --lib architecture_probe_coordinator_single_service_wake -- --nocapture
```

The Rust diagnostic uses a debug build because it records work counts, not timing. Compile the KJ probe with C++23, -O3, -DNDEBUG, -pthread and the headers/static kj and kj-async libraries built from the pinned C++ revision. Exact commands, source/library/binary hashes, every trial, scope and prior profile attribution are in [evidence.json](evidence.json). The local archive also retains both test patches, full logs, the compiled coordinator diagnostic, KJ executable, original source snapshots and a read-only verifier.

Local sealed archive: `/workspace/capntproto/target/portable-architecture-audit-20261019`; 21 files, 116,901,089 logical bytes. SHA256SUMS SHA-256: `72297eda51dcd55e2526fae59cbea5a9e53315ba394cb55ef343737279077e1d`. Archive files are immutable; rerun diagnostics in a separate checkout/output directory.

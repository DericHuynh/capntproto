# Legacy allocation parity work — 23 October 2026

## Status

**Complete measured comparison: 10/48 allocation targets met, 38 gaps, zero unmeasured cases. Further optimization is in progress.**

Implementation: [9650ddb00df52e5b5aa1e1aa6d4c2c17963a125f](https://github.com/DericHuynh/capntproto/commit/9650ddb00df52e5b5aa1e1aa6d4c2c17963a125f).
Frozen legacy control with the same diagnostic fixtures: [b00460e3f6452449296c3b56b34372c6e6c23831](https://github.com/DericHuynh/capntproto/commit/b00460e3f6452449296c3b56b34372c6e6c23831).

[Dedicated allocation run 40002634814](https://github.com/DericHuynh/capntproto/actions/runs/40002634814) completed successfully for source `42eccc56f3c3b2207bf9adef0c1ebd581855e7ec` (driver implementation 9650ddb). All 336 DHAT trials completed, with no failures or unpaired peers. Build checks, encrypted fixtures, certificate rejection, and cleanup of both droplets and SSH keys passed. No local performance measurements were run. This source predates the portable idle-publication correction described below.

The [complete comparison table](allocation-comparison.md) and [machine-readable comparison](allocation-comparison.json) retain all 48 cases and the frozen control. Legacy is below C++ in all 24 supported TLS cases; portable is the lower reference throughout. Legacy meets that lower target in 7 TLS and 3 QUIC cases. These are cumulative allocation counts, not native latency or peak-memory results.

| Workload | Frozen legacy | Legacy | Portable | C++ |
| --- | ---: | ---: | ---: | ---: |
| TLS unary-small | 28 | 15 | 12 | 91.49 |
| TLS dependent-read | 118 | 86 | 64 | 246.98 |
| TLS pipelined-read | 135 | 106 | 76 | 233.49 |
| TLS concurrent-32 | 754 | 338 | 362 | 2062.49 |
| TLS services-4-deep-16-clients-8 | 4563.09 | 2612 | 2344.60 | 8564.47 |
| QUIC concurrent-32 | 888.98 | 472.96 | 432.13 | unsupported |
| QUIC services-4-deep-16-clients-8 | 5298.18 | 3337.18 | 2836.04 | unsupported |

The scheduler follow-up `8aa48c0367ab0a49d27babf4211abaa4d152da1d` is being measured in [run 40010928237](https://github.com/DericHuynh/capntproto/actions/runs/40010928237), with `931c91ce3dd2751208fbdd3365c8286d126cac45` as its frozen legacy control. Its build and fixture checks passed; no scheduler savings are claimed yet.


The target is legacy allocation blocks per complete operation <= min(portable, C++) for each equivalent workload. C++ QUIC is unsupported; portable supplies that target. Shared-library changes also affect portable, so the new candidate must be compared against portable rebuilt from the same source.

## Last measured comparison, before this implementation

Source: [dedicated CI run 39976113886](https://github.com/DericHuynh/capntproto/actions/runs/39976113886), candidate source 8effa2ab8f7af1b94b6d200482035c99077d90b4. These are **pre-change measurements**, not results for 9650ddb.

Counts sum the client and every service peer. Each value is the difference in cumulative DHAT allocations between a long and short trial, divided by their iteration difference. Concurrent counts describe a complete batch, not one RPC. Fractional values include transport/background work.

| Transport / workload | Legacy before | Portable before | C++ | Target before |
| --- | ---: | ---: | ---: | ---: |
| TLS dependent-read | 118 | 64 | 247 | 64 |
| TLS pipelined-read | 135 | 77 | 233.50 | 77 |
| QUIC pipelined-read | 157.04 | 92.98 | unsupported | 92.98 |
| TLS concurrent-32 | 754 | 362 | 2062.49 | 362 |
| TLS deep-32-concurrent-32 | 770 | 378 | 2146.49 | 378 |
| TLS services-4-deep-16-clients-8 | 4564 | 2548.84 | 8567.11 | 2548.84 |
| QUIC concurrent-32 | 888.93 | 432.44 | unsupported | 432.44 |
| QUIC deep-32-concurrent-32 | 926.20 | 467.16 | unsupported | 467.16 |
| QUIC services-4-deep-16-clients-8 | missing complete pair | missing complete pair | unsupported | unmeasured |

The earlier campaign completed native timing and syscall traces but failed six DHAT QUIC service-chain trials. Partial failed profiles are not valid allocation comparisons. Full earlier evidence and limitations are in [the preceding report](../2026-10-23-allocation-concurrency/README.md).

## Allocation traces and resulting changes

The TLS concurrent-32 trace attributed 394 blocks to the legacy client and 360 to its service. Repeated allocating sites included the separate question completion slot, generated typed-response forwarding future, message wrappers, copied receive buffers, response owner, Results completion channel, result/parameter adapters, cancellation owners, and task wrappers.

The four-service trace additionally showed repeated protected-call Shared future machinery, task completion/cancellation channels, and FuturesUnordered nodes. This is primarily an ownership and scheduling allocation gap, rather than evidence that replacing local Rc/RefCell tables with generic lock-free containers would help.

The implementation now:

- Stores ordinary reply completion in QuestionRef. Join/adoption retain their independently owned protocol completion channels.
- Uses a dedicated ResponsePromise<T> to select generated result types without a second forwarding future. It releases completed underlying request owners promptly.
- Enters legacy incoming calls through borrowed dispatch. Taking ownership creates the result completion channel lazily.
- Transfers a standard two-party incoming message directly into its parameter hook instead of boxing a second parameter wrapper.
- Counts ReturnGuard interests in the existing AnswerStatus owner instead of allocating another Rc control block.
- Reuses one tracked completion slot for both task results and cancellation in eagerly_evaluate, removing a second channel and boxed select branches.
- Replaces the general dynamic protected-call fork with two fixed execution interests. Caller and executor can both drive the method; dropping either does not cancel the surviving owner. Only atomic wake metadata crosses threads.
- Caches empty outgoing envelopes, response owners, answer statuses, and released retained-result contexts. Payloads, capabilities, admission guards, and cancellation interests are released before caching.
- Reuses independently owned receive buffers only after their readers release them. Retained readers remain independent of later reads and stream teardown.

The complete measured counts and bytes are in the saved comparison. The main concurrent TLS batch falls from 754 to 338 blocks (55.2% fewer); the four-service/eight-client TLS batch falls from 4563.09 to 2612 (42.8% fewer). These improvements do not establish full parity.

## Public API and memory tradeoffs

RemotePromise<T>.promise and into_parts() now expose ResponsePromise<T> rather than Promise<Response<T>, Error>. Awaiting and other Future operations remain available. Custom RequestHook implementations construct the response future with ResponsePromise::new(). General Promise storage remains small.

Caches retain at most 64 empty entries per category per connection. The receive-frame cache accepts buffers up to 16,384 words each and at most 64 buffers (8 MiB maximum capacity per reader); it grows with released workload storage, rather than reserving that limit up front. Retained frames are never recycled early. The existing outgoing segment pool remains separately bounded.

Allocation reuse exchanges bounded retained memory for fewer allocator calls. Both cumulative allocation bytes and native latency must be checked; reducing allocation count alone does not establish faster execution.

## Validation and an outstanding architecture gap

- 81 core and 77 framing/futures unit tests passed after the completion-lifetime fixes. The final protected-fork foreign-thread wake test also passed.
- All 363 RPC unit tests pass with the portable idle-publication correction, including the strengthened publication-order regression.
- All 28 selected cancellation, static cancellation/model-trace, admission, reply ownership, and ignored-result integration tests pass.
- Expanded integration checks passed 79/80 tests: 33/34 answer-adoption, 6 disconnect-cleanup, 9 storage-history, 10 multiparty-join, 5 native pipeline-migration, 11 result-pipeline, and 5 revocable-server tests.
- Core/futures/RPC Clippy passes with warnings denied. Root test compilation passes with default features; several root test targets unconditionally import native modules and cannot be compiled with all default features disabled.
- All 26 cloud orchestration tests and 15 Python analysis/fixture tests pass. The comparison tests reject missing required comparators, unmatched trials, missing service peers, and inconsistent all-peer totals.

Expanded testing reproduced an existing publication-order defect in frozen b00460e: portable bootstrap/control work could publish directly into the physical FIFO while the transport was still marked idle. The coordinator previously cleared idle at the later flush boundary. It now clears idle before producers execute and waits for control work and notifications to settle before announcing idle again. No new per-call allocation is introduced. This correction clears seven adoption failures.

One pre-existing native integration failure remains: `third_party_tail_returns_over_authenticated_native` stalls in the portable branch while awaiting early pipeline resolution. Frozen b00460e with only the idle correction and test diagnostics reproduces the same failure; the legacy branch completes. During the stall, all three drivers remain active, the caller has one published provision and no rendezvous waiter, and the projected endpoint is bound but unsettled, with no forwarding hook or introduction. The ten-second timeout is unchanged.

The portable pipeline architecture uses a coarse `PipelineState.used` marker: deriving a capability with `get_pipelined_cap()` marks the whole pipeline used, and `CallTasks::adopt_pipeline()` refuses to replace a used pipeline. This rule preserves earlier-call ordering but also constrains early route migration. It needs investigation of individual projection use and ordering fences; blindly clearing the marker or rebinding previously used references would violate existing route-ordering tests. The observed native timeout is recorded as an outstanding gap, not a qualified passing test or a regression attributed to the new legacy allocations. Speculative forwarding-queue and resolution-wait changes did not clear it and were removed.

Two failed experiments were corrected, rather than weakening the relevant tests: executor-only protected execution broke caller ownership after executor shutdown, and retaining an already-completed response adapter delayed pipeline cancellation. The current implementation retains both execution interests and releases completed response adapters immediately.

## Droplet campaign and comparison contract

The performance workflow now offers campaign=run-allocations. It uses the existing dedicated-droplet provisioning, artifact capture, and unconditional cleanup path. It runs DHAT across 24 workloads on each of TLS and QUIC (48 cases), wherever supported by each implementation, using 10 warmups and paired 5/50-iteration profiles, with every service peer included.

The profiler sets CAPNTPROTO_BENCH_QUIC_IDLE_MS=120000 for all Rust diagnostic peers and records it in scope.json. Native runs keep the normal 10,000 ms timeout. The prior failures were consistent with already-connected service hops idling during instrumented setup. The complete follow-up ran all 336 trials successfully with this diagnostic timeout; that result does not independently prove the cause of the earlier failures. Downstream/service RPC errors are now retained in fixture stderr.

Allocation-only reports explicitly contain no native latency or syscall-comparison claims. Complete allocation stacks are written incrementally to avoid holding every expanded stack report in memory.

Campaign command (already dispatched for run 40002634814):

```sh
gh workflow run performance.yml \
  --repo DericHuynh/capntproto \
  --ref codex/legacy-allocation-concurrency \
  -f campaign=run-allocations \
  -f legacy_base=b00460e3f6452449296c3b56b34372c6e6c23831 \
  -f region=auto -f size=auto
```

This compares candidate legacy, frozen legacy, current portable, and pinned C++ with matched diagnostic fixtures. Inspect all profile failures, paired counts, bytes, and full allocation stacks; continue optimizing any remaining gap. Once allocation parity is established, run the five-repetition native qualification campaign to check latency and spread.

The saved-evidence analyzer now emits `allocation-comparison.json` and `allocation-comparison.md`. Each declared workload requires both complete DHAT trials and every client/service peer for legacy and its comparators. TLS requires portable and C++; QUIC requires portable. The target is their minimum unrounded block count, with only floating-point roundoff tolerance. Incomplete comparisons remain unmeasured. Cumulative bytes and the frozen legacy control are retained alongside the target. These reports contain no native latency or peak-memory claims.

## Follow-up: reusable scheduler notifications

The next candidate removes the permanent per-task allocation transition at eight live scheduler tasks. `ReadySet` now reuses up to 128 notification slots, backed by a bounded multi-producer atomic FIFO. A free-slot bitmap avoids scanning suspended futures. Slots and their wake identities survive completed tasks; task payloads still drop immediately. More than 128 simultaneously live tasks retain the existing `FuturesUnordered` fallback.

The earlier service-chain trace attributed 128 allocation blocks per batch to driver-owned `FuturesUnordered` nodes, separately from the unchanged fixture's 33 nodes. This establishes an allocating site to target; it does not establish the new candidate's savings. Pinned KJ `TaskSet::add()` in `c++/src/kj/async.c++` appends task bookkeeping to its promise arena. Reusing notification storage applies the same ownership principle without assuming Rust futures can use KJ's representation.

The atomic queue preserves reservation order, publishes duplicate wakes before a consuming poll, and never polls user futures from producer threads. A delayed producer can leave the next reservation unpublished; the consumer returns pending until that producer publishes and wakes it. Notification storage stays bounded. This is not a claim of a formally lock-free consumer: progress past an unpublished reservation depends on its producer.

The change trades a larger fixed notification array and additional atomic operations for reuse under concurrency. It must be checked against native latency as well as allocation counts. No local performance measurement has been run.

Validation: all 365 RPC unit tests, 28 cancellation/ownership/admission integration tests, and warnings-denied RPC Clippy pass. The scheduler tests cover simultaneous producers and consumption, duplicate data publication, FIFO order, cursor wraparound, a suspended producer reservation, reused wake identities, foreign-thread wakes, fallback promotion, teardown, and bounded self-wakes. The preceding ownership candidate also passed 72 further streaming, flow-control, reconnect, join, authentication, and native multiparty tests.

The next allocation campaign uses `931c91c` as its frozen legacy control to isolate this scheduler change, while comparing candidate legacy with current portable and pinned C++. Run 40002634814 remains the comparison of the earlier ownership changes against b00460e. The earlier ownership candidate met 10/48 targets; the scheduler follow-up is not measured yet.

## Residual allocation attribution

The new unary TLS trace totals 9 client and 6 service blocks. In addition to the remaining request/question/response/message wrappers, it records an extra `Vec<&[u8]>` in rustls's vectored writer on each peer. Portable's contiguous frame avoids those two slice-vector allocations. The legacy answer-status allocation also remains at one block per call despite the earlier cache: the answer can retire while completion owners still retain its status.

QUIC exposes the frame split more directly. In concurrent-32, legacy allocates 64 quiche send buffers on each peer, while portable allocates 32 on each. The separate segment-table and payload slices therefore account for 64 extra allocations per complete batch. Native bytes, partial-write semantics, and flushing still have to remain correct when removing that split.

Intermediate legacy service peers retain protected execution interests, completion slots, boxed method futures, and scheduler nodes. The ordinary data-only TLS concurrent case is already below portable; capability-bearing dependent/pipelined calls and intermediate services remain the larger protocol gaps. Large QUIC cases also include variable packet/ACK/range-buffer work; a one-pair allocation delta is not a native timing result.

The raw profiles cover every peer. Archive SHA-256 values:

- `encrypted-benchmark-analysis` (artifact 12527596421, 119657446 bytes): `396f904a242b99052e4c07ca61a0e9a6cdbbc5b76ba8e6860c4564eb752b1b1b`.
- `dedicated-benchmark-report` (artifact 12527033432, 284725274 bytes): `69d0cb8e2f78e847d8d42071dd168d91071183b24941bcd88635475e82d598b7`.

The full allocation-stacks JSON expands to 3.68 GB. It is preserved in the workflow artifacts; only compact summaries and the profile scope are committed here. All 336 diagnostic trials together took 3429 seconds, close to that revision’s one-hour SSH command budget. The next candidate increases that budget before the full native-plus-tracing qualification campaign is run with this expanded DHAT matrix.


## Follow-up: encrypted batches and answer-status ownership

The next candidate coalesces encrypted vectored writes into reusable plaintext scratch storage when the complete batch fits within 128 KiB. This removes the segment-table/payload split at the rustls/quiche boundary. Large batches retain scatter/gather; ordinary unencrypted sockets retain their existing path. Partial writes and Pending consume only the reported prefix and retain no input borrow. Scratch capacity grows on demand and stays bounded per encrypted writer. Both drivers use this transport adapter, so current portable must be rebuilt for the comparison too.

Answer status now returns to its bounded cache when the final status owner releases it, rather than only when the answer-table entry drops. That covers the no-Finish completion order seen in the allocation trace. User response contexts drop before the cache is borrowed; cached statuses contain no response context. The cache retains at most 64 statuses and its owners cannot keep the connection alive.

Validation for this candidate: all 366 RPC unit tests, all 6 transport-adapter tests, and 100 selected encrypted transport, cancellation, admission, reply, pipeline, disconnect, and adoption integration tests passed. The known native portable early-pipeline failure described above was explicitly excluded from that selected run. RPC and root-library Clippy pass with warnings denied; all 26 cloud orchestration tests pass. Performance savings remain unmeasured until the dedicated campaign completes.

The encrypted remote-command budget is now two hours and its CI measurement job allows 150 minutes, based on the measured 57-minute allocation matrix. This leaves room for the later native and Callgrind phases without dropping workload coverage. The planned frozen legacy control is scheduler candidate `8aa48c0367ab0a49d27babf4211abaa4d152da1d`.

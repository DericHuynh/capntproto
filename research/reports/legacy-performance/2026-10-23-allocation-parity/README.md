# Legacy allocation parity work — 23 October 2026

## Status

**Latest measured dispatch qualification: 44/48 allocation targets met, four bulk-transfer gaps, zero unmeasured cases. See the [qualification report](../2026-10-24-dispatch-qualification/README.md) for allocation attribution, five-run latency means/spread, and remaining read-count gaps. Later candidates are being qualified; full allocation parity remains unmet.**

The ownership comparison preserved below met 10/48 targets. It predates the scheduler and subsequent candidates.

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

The scheduler follow-up `8aa48c0367ab0a49d27babf4211abaa4d152da1d` completed [run 40010928237](https://github.com/DericHuynh/capntproto/actions/runs/40010928237), with `931c91ce3dd2751208fbdd3365c8286d126cac45` as its frozen legacy control. Its [measured report](../2026-10-23-scheduler-allocation/README.md) attributes 128–384 removed driver nodes per concurrent service-chain batch. The target remains unmet overall; no native latency was measured in that allocation-only campaign.


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

The next allocation campaign uses `931c91c` as its frozen legacy control to isolate this scheduler change, while comparing candidate legacy with current portable and pinned C++. Run 40002634814 remains the comparison of the earlier ownership changes against b00460e. The earlier ownership candidate met 10/48 targets; the completed scheduler follow-up meets 12/48. Its same-run frozen control meets 11/48; the extra large-QUIC passing row is not attributed to scheduler reuse. See the linked measured scheduler report for exact stack attribution.

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

## Follow-up: borrowed completion for capability-bearing calls

The encrypted-batch/status candidate `bbe8af44a55f49b3c82138aef9955e4e23aa9bde` is measuring in [run 40016773400](https://github.com/DericHuynh/capntproto/actions/runs/40016773400). Its allocation savings remain unmeasured.

The next dispatch change publishes the queued answer pipeline before entering application code. Ordinary capability-bearing handlers can then borrow the inbound context and complete on the first poll, as data-only handlers already do. Pending calls keep their independently scheduled execution; redirected and pipeline-only calls retain their existing ownership policy. Streaming barriers and protected-call admission remain in the local dispatcher.

The saved dependent-read trace attributed deferred context ownership, protected execution forks, a general shared completion future, and eager task wrappers to an immediately completing capability-returning handler. Publishing the answer route first allows these owners to be omitted for immediate completion. Pinned C++ `RpcConnectionState::handleCall()` similarly records an answer context before `startCall()` and explicitly accounts for immediate tail-call effects; its KJ promise representation differs from Rust's.

All 367 RPC unit tests and 143 selected integration tests pass. The added regression projects the answer reentrantly before results are published, verifies the projection resolves to the returned capability, and covers both immediate completion and a suspended first poll. Existing cancellation, revocation, streaming, flow-control, pipeline migration, native multiparty, encrypted transport, and adoption checks passed; the previously recorded portable native timeout remains excluded. No allocation or latency improvement is claimed for this change until its own droplet profile completes.

Settled legacy exports now leave `resolve_op` absent, matching C++ `Export::resolveOp = kj::none`. The old constructor created a never-used failed promise, including its error string, error details, and boxed error. The saved capability-returning trace records these three allocations at `Export::new`. Actual unresolved exports still own their resolution task and cancel it on release. All 367 RPC unit tests, 48 selected pipeline/join/reconnect/encrypted integration tests, and warnings-denied RPC Clippy passed after this change. This is an allocation-site removal, not yet a measured campaign delta.

## Follow-up: consumed request storage

The ordinary request hook's box is now reused after sending consumes its state. Each connection keeps at most 64 empty request boxes; the entire live request, including its permit, target, message, and capability table, moves into sending before the box enters the cache. No cache borrow spans transport or capability callbacks. Unsent requests still release their owners when dropped. This targets the 88-byte `rpc::Request` allocation appearing once per ordinary outgoing call in the saved trace, without changing the public RequestHook contract or adding unsafe code.

All 368 RPC unit tests pass, including a burst/reuse regression that checks the bound, empty cached contents, reused addresses, and connection teardown. RPC Clippy passes with warnings denied. With the bounded QUIC-prefix follow-up, 100 root RPC/transport unit tests and 86 selected admission, reply, pipeline, streaming, reconnect, encrypted transport, and adoption integration tests passed. One existing root test remains ignored, and the recorded native portable early-pipeline failure remains explicitly excluded. These are correctness checks, not local benchmark evidence.

For encrypted writers that do not support scatter/gather, the transport now also coalesces a prefix of large batches, capped at 128 KiB. After that prefix, a remaining contiguous payload passes directly to the writer. Quiche's `SendBuf::write()` splits each submitted slice into 4096-byte send buffers, so sending a separate short frame header creates its own buffer. The bounded prefix avoids that split without retaining a full multi-megabyte copy. Writers with scatter/gather, including TLS, keep the previous large-batch path. The regression covers Pending, partial consumption, exact complete byte order, and the scratch bound; root-library Clippy passes with warnings denied. The final droplet comparison must still establish allocation, byte, and native timing effects.

Local server shortening cancellation is now allocated only when `shorten_path()` supplies a task. Settled servers previously allocated a canceler and then created/cloned cancellation errors while dropping it, despite having no resolution work. Active shortening retains the existing cancellation path, including revocation and last-client teardown. This shared change affects both legacy and portable and must not be credited as a legacy-only advantage. Five focused local unit tests, 28 shortening/revocation/pipeline integration tests, and RPC Clippy passed.

Candidate `1a2d77250d49691a600ca97552003d95d053c202` is dispatched in [run 40022705577](https://github.com/DericHuynh/capntproto/actions/runs/40022705577) using the full encrypted qualification campaign: five native repetitions with mean and spread, selected ciphertext syscall traces, Callgrind diagnostics, and the complete 48-case DHAT matrix. Its frozen legacy control is `bbe8af44a55f49b3c82138aef9955e4e23aa9bde`. It was dispatched after the preceding run acquired the dedicated benchmark slot, so GitHub's single pending concurrency slot cannot cancel that measurement. No measurements are run in this workspace.

## Correctness gate: explicit executor admission

Review of immediate completion found that an inbound protected call could enter a synchronous handler even when the server's explicitly supplied executor had stopped. The new regression reproduced a successful call where rejection was required. A CallContext now carries a deferred-protection restriction through generated type conversions, and the local dispatcher disables its speculative first poll when an explicit executor is selected. That executor must accept the task before protected application entry. Ordinary inbound calls using the driver's owner retain borrowed immediate completion.

The regression passes for both immediate and suspended handlers and both pipeline hints. All 33 selected cancellation/revocation/server-hook integration tests, 81 core unit tests, 368 RPC unit tests, and core/RPC Clippy passed after the fix. This gate is part of the next qualification candidate; the frozen benchmark controls do not exercise an explicitly stopped application executor.

## Follow-up: answer cancellation without a result channel

The complete serial four-service trace totals 93.378 legacy blocks versus 73 portable blocks. An intermediate legacy peer contributes about 26 blocks versus portable's 20. Two sites are a completion slot and boxed receiver created by `eagerly_evaluate()` solely so dropping the stored receiver cancels execution; ordinary answer dispatch never awaits that result.

Ordinary pending answers now retain a cancellation interest in their existing answer status. The scheduled task registers its wake there, observes cancellation, and clears the wake on completion or driver teardown. The bounded status cache resets cancellation before reuse. This removes the unused result channel/receiver while preserving deferred cancellation and protected method ownership. Specialized adoption, Join, and provision continuations retain their existing result-bearing promises. The generic eagerly-evaluated promise remains for operations that consume its result.

All 368 existing RPC unit tests, the new cancellation/wake/cache regression, warnings-denied RPC Clippy, and 158 selected integration tests passed. Those integrations cover cancellation, response ownership, Finish/disconnect, admission, flow control, streaming, Join, adoption, native multiparty routing, pipelines, and encrypted transports. The known portable native early-pipeline timeout remains explicitly excluded. No new performance result is claimed before droplet measurement.

## Follow-up: direct ownership of exported call targets

Ordinary calls to an exported capability now retain its shared export hook directly. The previous target lookup called `add_ref()` to allocate another boxed local-client hook for every incoming call. Promised-answer projections retain their existing owned hook, and control operations retain their existing target interface. Export creation/resolution now pays for the shared owner once; steady calls reuse it. No export-table borrow spans application entry.

All 370 RPC unit tests, 101 selected cancellation/adoption/Join/native/pipeline/encrypted integration tests, and warnings-denied RPC Clippy pass. The new regression verifies that dispatch performs no hook clone and that releasing the export from inside the handler does not destroy the capability until dispatch returns. This directly targets the boxed local-client site in the serial and concurrent traces. Its measured savings and the one-time export-owner tradeoff remain subject to the next droplet profile.


## Follow-up: streaming state only for streaming calls

The scheduler campaign's saved TLS pipelined-read trace contains three allocations for `Rc<RefCell<Option<Box<dyn FlowController>>>>` per operation, although the fixture sends no streaming calls. C++ `RpcClient::flowController` is absent until the first streaming call (`vendor/capnproto/c++/src/capnp/rpc.c++`, near line 1182).

Legacy now keeps a lazy streaming cell in the capability's existing shared identity. Ordinary clients, re-imports, requests, and weak downcast entries need no separate flow-owner allocation. First streaming use initializes the shared owner; aliases created before and after that use obtain the same owner. Its reference is cloned before calling transport or flow-control code, so no capability-state borrow spans those callbacks. The flow controller itself and its acknowledgement/error policy are unchanged. This also removes redundant weak flow-state bookkeeping from the downcast map.

All 371 RPC unit tests, 78 selected streaming, flow-control, import-alias, result-pipeline, adoption, native migration, and multiparty-join integration tests, and warnings-denied RPC Clippy pass. The regression covers lazy construction, request preparation before streaming, aliases/re-imports, weak lookup, and capability teardown with a retained flow owner. The previously documented native portable early-pipeline failure remains explicitly excluded. Before this follow-up, `cargo check --locked --workspace --all-targets` also passed for the full qualification source.

This change follows the frozen `1a2d772` qualification candidate. That run's measurements must not be attributed to this later lazy-flow change. A subsequent droplet comparison is required before claiming its count or latency savings. No local performance measurements were run.


## Follow-up: typed replies poll the existing question

The saved ordinary-call trace still allocates a boxed `MapOk<QuestionReply<...>>` for each request even though QuestionRef already owns completion. Typed response futures can now hold that existing shared question directly and poll it through the core SharedPromise interface. A local QuestionCell type wraps the existing RefCell so the runtime can implement that interface without unsafe casts or a second allocation. Ordinary replies transfer their one receiver interest into the typed future; tail and protocol-specific continuations retain their ordinary future adapters.

ResponsePromise now owns a flat response-specific state: immediate result, deferred future, reusable receiver, or direct shared question. Core Promise no longer carries the response-only variant or response conversion callback. Portable's reusable response receiver moves directly into ResponsePromise too. The previous hidden `Promise::from_shared_response` constructor is replaced by `ResponsePromise::from_shared_response`; the repository's callers are migrated with no compatibility shim. Generated result selection still moves the same owner. Completed futures release receiver ownership promptly, even if the completed future itself is retained.

Layout regressions enforce the existing limits of three machine words for Promise<(), Error> and four for ResponsePromise<any_pointer::Owned>. Success, errors, pending cancellation, unique receiver ownership, hook lifetime, and producer-state reuse are covered. This changes ownership representation, not remote cancellation or ordering rules.

The final implementation passes all 82 core and 371 RPC unit tests, warnings-denied core/RPC Clippy, all 177 selected integration tests (including TLS and QUIC), the alloc-only/no-std core check, and every workspace target. Core/RPC documentation tests pass; their existing ignored examples remain ignored. The known pre-existing native portable early-pipeline timeout remains explicitly excluded. One local compiler invocation was killed by memory pressure; clearing only older rebuildable debug test executables allowed the affected checks to pass. Benchmark measurements remain confined to CI droplets, and evidence archives were preserved.

This reply change and lazy streaming ownership are later than the `1a2d772` qualification source. They need their own allocation campaign against that frozen legacy revision before measured savings or full parity can be claimed. Shared core changes are also present in the current portable comparator.


## Follow-up qualification of direct replies and lazy streaming

[Run 40030571555](https://github.com/DericHuynh/capntproto/actions/runs/40030571555) invokes the same full dedicated campaign for implementation `3aec134` against frozen `1a2d772`. Qualification source `4fa3011` adds only a workflow caller on the isolated `codex/legacy-allocation-qualification-20261023` branch. The GitHub connector started it after the previous candidate acquired the dedicated slot. Native means/spread, syscall traces, Callgrind and all 48 allocation comparisons remain pending for this source.


## Follow-up: borrow QUIC routing IDs

The completed [encrypted batching profile](../2026-10-23-encrypted-batching/README.md) attributes 571.20 legacy blocks per bulk-4m operation to copying short-header connection IDs in the endpoint router, followed by the same copies inside quiche's receive parser. Current portable has the same duplication (567.98 router blocks in that trial).

The router now reads the unprotected destination ID directly from its UDP receive buffer for short headers. Quiche still receives the complete packet and performs packet parsing, validation, authentication, and decryption. Long headers retain the full parser for admission/version/Retry handling. Empty or truncated packets and unknown short-header destinations are discarded. The existing fixed 16-byte local connection-ID policy is unchanged. No payload, route, or connection ownership is borrowed across an await.

Both endpoint tests, all 22 encrypted RPC integration tests, and warnings-denied root-library Clippy pass with the QUIC feature enabled. The new regression compares routing against quiche for every short-header first byte, exercises every truncated destination length, and checks unknown destinations and malformed long headers using a packet-order barrier. Existing tests cover listener shutdown, certificate rejection, TLS/QUIC capabilities, pipelining, large messages, and transport teardown.

This shared transport change follows the direct-reply qualification source. Neither run 40022705577 nor 40030571555 contains it. Its complete count, cumulative-byte, and latency effects remain unmeasured; the 571.20-block trace identifies removed work and must not be reported as a measured candidate improvement.

[Run 40031849761](https://github.com/DericHuynh/capntproto/actions/runs/40031849761) schedules the full dedicated qualification of borrowed routing implementation `1a9e5fc2950c0b6c7025a0383d0a2d48a8f5d667` against frozen `3aec134`. Source `6ee9ecb23597f03604cbf8c265a9df55a764e1f0` adds only the isolated workflow caller. Its preliminary job waits for run 40030571555 to acquire the dedicated measurement slot before starting the build, preserving GitHub's single pending benchmark job. This is scheduled validation, not a completed performance result. The shared adapter is rebuilt into both current legacy and portable.

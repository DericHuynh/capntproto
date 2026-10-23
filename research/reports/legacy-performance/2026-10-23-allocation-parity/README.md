# Legacy allocation parity work — 23 October 2026

## Status

**Implementation pushed; allocation parity is not yet verified.**

Implementation: [9650ddb00df52e5b5aa1e1aa6d4c2c17963a125f](https://github.com/DericHuynh/capntproto/commit/9650ddb00df52e5b5aa1e1aa6d4c2c17963a125f).
Frozen legacy control with the same diagnostic fixtures: [b00460e3f6452449296c3b56b34372c6e6c23831](https://github.com/DericHuynh/capntproto/commit/b00460e3f6452449296c3b56b34372c6e6c23831).

The managed workspace became unavailable during final lint/build checks, after the implementation commit was pushed. The remote branch head was verified through the GitHub connector. No new droplet campaign was dispatched, and no local performance measurements were run.

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

These are implementation changes, not measured savings. The exact allocation and byte reductions still require the new droplet results.

## Public API and memory tradeoffs

RemotePromise<T>.promise and into_parts() now expose ResponsePromise<T> rather than Promise<Response<T>, Error>. Awaiting and other Future operations remain available. Custom RequestHook implementations construct the response future with ResponsePromise::new(). General Promise storage remains small.

Caches retain at most 64 empty entries per category per connection. The receive-frame cache accepts buffers up to 16,384 words each and at most 64 buffers (8 MiB maximum capacity per reader); it grows with released workload storage, rather than reserving that limit up front. Retained frames are never recycled early. The existing outgoing segment pool remains separately bounded.

Allocation reuse exchanges bounded retained memory for fewer allocator calls. Both cumulative allocation bytes and native latency must be checked; reducing allocation count alone does not establish faster execution.

## Validation completed before the environment outage

- 81 core, 77 framing/futures, and 362 RPC unit tests passed after the completion-lifetime fixes.
- 28 selected integration tests passed, including cancellation, static cancellation/model traces, admission, reply ownership, and ignored results.
- 26 cloud orchestration tests passed, including allocation-only success/failure cleanup and evidence salvage.
- 12 Python analysis/fixture tests passed, including allocation-only reports and all-peer marginal counts.
- A subsequent dedicated protected-fork cross-thread wake regression test was added; its final run result could not be retrieved before the outage.
- Final Clippy and the broader root test compilation were not confirmed complete. An earlier Clippy pass reported a test-only byte-literal style issue, which was corrected before the implementation commit.

Two failed experiments were corrected, rather than weakening the relevant tests: executor-only protected execution broke caller ownership after executor shutdown, and retaining an already-completed response adapter delayed pipeline cancellation. The current implementation retains both execution interests and releases completed response adapters immediately.

## Droplet campaign prepared

The performance workflow now offers campaign=run-allocations. It uses the existing dedicated-droplet provisioning, artifact capture, and unconditional cleanup path. It runs DHAT across all 24 TLS/QUIC cases supported by each implementation, using 10 warmups and paired 5/50-iteration profiles, with every service peer included.

The profiler sets CAPNTPROTO_BENCH_QUIC_IDLE_MS=120000 for all Rust diagnostic peers and records it in scope.json. Native runs keep the normal 10,000 ms timeout. The prior failures were consistent with already-connected service hops idling during instrumented setup, but that explanation is not yet confirmed by a successful new comparison. Downstream/service RPC errors are now retained in fixture stderr.

Allocation-only reports explicitly contain no native latency or syscall-comparison claims. Complete allocation stacks are written incrementally to avoid holding every expanded stack report in memory.

Resume command after workspace recovery:

```sh
gh workflow run performance.yml \
  --repo DericHuynh/capntproto \
  --ref codex/legacy-allocation-concurrency \
  -f campaign=run-allocations \
  -f legacy_base=b00460e3f6452449296c3b56b34372c6e6c23831 \
  -f region=auto -f size=auto
```

This compares candidate legacy, frozen legacy, current portable, and pinned C++ with matched diagnostic fixtures. Inspect all profile failures, paired counts, bytes, and full allocation stacks; continue optimizing any remaining gap. Once allocation parity is established, run the five-repetition native qualification campaign to check latency and spread.

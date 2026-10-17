# Legacy RPC optimization, October 17, 2026

Tracking: [legacy gap #15](https://github.com/DericHuynh/capntproto/issues/15).

**The 2× goal is not achieved.** The final local campaign passes the exact pooled-p50 gate in 1/16 cells: 64-KiB loopback. That cell includes variable slow baseline runs; it does not establish a stable 2× gain. All twelve application workflows miss the target. The largest workflow speedup in this campaign is 1.63× at 4 MiB.

Legacy is again the benchmark default; explicit `CAPNTPROTO_BENCH_PORTABLE=1` selects portable. The production `RpcSystem` already used legacy. Tokio remains the host. The earlier portable implementation and its evidence remain available.

## Baseline and acceptance

The before control is legacy at `6e37505d701df4dbea52a92e0691b6037f7f1922`, frozen before edits. Its executable is SHA-256 `2fa2440ebb65083316d7cf2ea08fef1ef2651aca71d2c1c9f3a92cef0bf33557`. The prior compiled tree and this commit have identical code, manifests and benchmark fixtures. Every Rust measurement explicitly selects legacy with `CAPNTPROTO_BENCH_PORTABLE=0`.

The acceptance test is `2 * candidate_p50 <= baseline_p50` in every cell, without a tolerance. C++ is context, not the before control. Workflows and the separate loopback fixture retain their respective CPU placement, validation and sampling budgets. All measurements here are local; no dedicated remote before/after claim is made.

## Implementation and C++ correspondence

1. Owning incoming spill frames return their storage only after the last reader releases it. One lazily allocated cache retains at most 8 MiB per reader, independently of the existing borrowed-frame spare. A weak cache reference lets readers outlive the stream. Reused initialized capacity avoids repeated clearing, while exact frame bounds prevent consuming the next frame or exposing stale bytes. The spill owner is boxed to avoid enlarging every ordinary message. No unsafe code is added.

2. Ordinary Returns settle the existing legacy pipeline directly before sending the application response. This removes shared-future tasks and resolution waiters from that path. Pipeline-only interest still retains its question; redirected and third-party adopted answers retain asynchronous driving and producer ownership. Pipeline installation precedes transport publication, including synchronous custom transports. No table borrow spans callback or wake delivery.

3. Selected Calls with promise pipelines, and Finish messages, enter the existing FIFO without an immediate write. A following terminal call can flush the parent and child together; otherwise the notified network driver makes progress. Ordinary terminal Calls and Returns retain immediate driving. Partial-write, flush, admission and cancellation ownership remain with the existing writer.

C++ reference `0de72d8d8cec6b69edaa29de51d3bd490341f9c2` deliberately uses `kj::evalLast()` to batch messages in `rpc-twoparty.c++`. The selective Rust batching follows that scheduling intent, but is not a literal port of KJ. C++ `RpcPipeline` still uses KJ forked promises; direct Rust settlement specializes the ordinary Return path rather than copying those internals verbatim. No public Rust API break is needed for these changes.

## Profiles and wire traces

Callgrind counts include startup, 100 warmups and 1,000 operations. They are not latency measurements.

| Case / peer | Before instructions | After instructions | Change |
| --- | ---: | ---: | ---: |
| unary-empty / client | 17,804,544 | 17,922,592 | +0.66% |
| unary-empty / server | 16,282,981 | 16,329,086 | +0.28% |
| pipelined-read / client | 54,862,164 | 43,039,991 | -21.55% |
| pipelined-read / server | 56,491,229 | 56,624,639 | +0.24% |
| callback / client | 36,423,814 | 36,593,824 | +0.47% |
| callback / server | 45,074,608 | 45,240,660 | +0.37% |
| bulk-64k / client | 247,617,111 | 175,448,894 | -29.15% |
| bulk-64k / server | 235,761,810 | 163,266,209 | -30.75% |

Pipeline-client malloc calls fall from 68,521 to 56,411. Batching increases pipeline-server malloc calls from 82,034 to 89,322; server instructions remain nearly flat. The unbatched direct-settlement candidate uses fewer allocations than the batched candidate, and remains retained for comparison. Reduced client instruction counts do not imply a matching reduction in whole-round-trip latency.

All 24 peer journals decode completely, including resumed and vector/message syscalls. Before/after message-kind counts and total bytes match in all six peer comparisons. Unary and callback byte streams match exactly; pipeline control-message ordering changes. Existing protocol and ownership tests validate ordering independently.

For the traced pipeline exchange (2 warmups, 10 operations), client writes fall from 39 to 26 and server writes from 24 to 14. C++ uses 13 and 14. Rust still emits separate cleanup writes. Trace timings are excluded from performance qualification.

## Final uninstrumented workflow campaign

Four variants rotate and alternate order across five repetitions: baseline, direct settlement with reuse, the final batched build, and C++. Each trial uses 100 warmups and 1,000 samples, server CPU 1 and client CPU 2. Pooled nearest-rank percentiles are independently recomputed.

| Workflow | Before µs | After µs | C++ µs | Speedup | 2× |
| --- | ---: | ---: | ---: | ---: | :---: |
| unary-empty | 19.929 | 19.910 | 21.813 | 1.001× | miss |
| unary-small | 19.859 | 29.193 | 22.333 | 0.680× | miss |
| unary-1k | 20.720 | 20.661 | 22.533 | 1.003× | miss |
| fanout-32 | 139.095 | 142.480 | 137.463 | 0.976× | miss |
| dependent-read | 46.359 | 44.896 | 48.081 | 1.033× | miss |
| pipelined-read | 29.023 | 28.392 | 31.176 | 1.022× | miss |
| write-read | 40.200 | 40.580 | 45.608 | 0.991× | miss |
| callback | 41.811 | 41.561 | 45.367 | 1.006× | miss |
| bulk-64k | 49.063 | 51.937 | 54.891 | 0.945× | miss |
| bulk-256k | 143.141 | 121.379 | 253.704 | 1.179× | miss |
| bulk-1m | 647.295 | 435.081 | 1020.114 | 1.488× | miss |
| bulk-4m | 3899.391 | 2391.416 | 4669.868 | 1.631× | miss |

## Separate loopback campaign

Five rotating repetitions preserve the existing 10,000 warmups, 1,000 samples and discovered physical-core placement. This topology is not pooled with workflows.

| Payload bytes | Before ns | After ns | C++ ns | Speedup | 2× |
| --- | ---: | ---: | ---: | ---: | :---: |
| 0 | 20,230 | 19,940 | 24,396 | 1.015× | miss |
| 64 | 29,834 | 19,799 | 21,942 | 1.507× | miss |
| 1,024 | 30,345 | 20,320 | 24,125 | 1.493× | miss |
| 65,536 | 101,960 | 47,249 | 61,831 | 2.158× | pass |

## Variability and intermediate candidates

Both complete campaigns, both twenty-repetition follow-ups, p95/p99, throughput and every repetition median remain in `report.json`; all raw samples remain in the archive. The initial direct-settlement campaign reaches 1.85× at 4 MiB, but passes 0/16 cells. This is a separate observation, not the final build’s result. The initial inline spill owner was replaced with a boxed owner after small-message instruction regressions.

The final five-repetition campaign shows a 47% pooled regression for 64-byte unary calls. The twenty-repetition follow-up returns those calls to approximately parity. It also repeats 1-KiB calls, fanout, pipelines, callbacks and 64-KiB payloads. Median paired batched/direct ratios range from 0.994 to 0.999, while pooled medians vary more, including a 4.6% callback difference. This does not establish broad latency improvement from batching. The complete campaign’s misses are not replaced by the follow-up.

The passing 64-KiB loopback cell has baseline repetition medians of 104.103, 105.896, 49.102, 48.572 and 104.013 µs. The candidate ranges from 46.558 to 53.329 µs. C++ also shows slower timing modes. The observed pooled pass is reported, but is insufficient to qualify a stable 2× speedup. No warmups, slow samples or failing cells are discarded.

## Remaining architecture costs

Small calls change little in instruction count and remain around 20 µs end to end. Halving them requires work beyond spill reuse and pipeline completion. Outgoing builder clearing/copying, request/response owner allocations, transport scheduling and kernel wake latency remain. Batching exposes additional server allocations and still does not reproduce C++ cleanup grouping. The next ownership change must reduce these costs while preserving response cancellation, pipeline-only interest, retained readers, adopted producers and flush ownership. Profiling alone cannot establish that the unchanged end-to-end target is attainable.

## Validation and reproduction

311 RPC unit tests, 66 framing tests, 99 integration tests and nine focused Miri tests pass. Integration covers independent pipelines and TLC/C++ replay, adopted answers, tail transfers, ownership, admission, membranes, TCP and native pipeline migration. Strict Clippy passes with `-D warnings`. No unsafe code, fixture changes, relaxed thresholds or coverage-floor changes are introduced. The preceding `6e37505d` CI failed its Cargo coverage job; this local validation is not a claim that workspace coverage passes.

The final source tree is `a185c21e11ec18470d6e4a331e7cbb5ee4bb3602`; executable SHA-256 is `4d08065d88510247db91f31bbc212fdc618ea7b0662076d8962a9afeeb801c0c`. `source.patch` replays against the frozen baseline to that tree. All three candidate patches, binary hashes and unchanged fixture hashes validate. The report JSON records the exact compilation arguments.

Across the two full campaigns and two follow-ups, all **1,260,000 uninstrumented samples** validate, along with **384 mixed-peer workflow cells**. Builds, tests and profiling do not overlap timing campaigns. Earlier compile errors, failed draft test assertions, the missing TLC configuration and the corrected Miri fixture failure remain in the evidence.

Raw logs, samples, launchers, profiles, traces, source freezes and binaries are retained under `target/legacy-2x-20261017/`. Run the archived campaign scripts in a clean performance environment with the frozen binaries; source replay uses `legacy2x_replay20261017.py`. The original and final campaigns remain separate.

The raw archive contains 4,893 files (438,619,458 bytes); `SHA256SUMS` SHA-256 is `37672c2e02bb5c4f6b72311a643a9600ecfc40b98632b2213d64a5850b023361`. Repository/wiki checks, formatting and the workspace unsafe documentation gate pass. Default/explicit legacy and explicit portable identities also validate; invalid driver selection is rejected.

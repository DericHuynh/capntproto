# Portable latency and benchmark CI — 24 October 2026

The [receive batching and rejected scheduling experiments](../2026-10-24-portable-latency-followup/README.md) records the next experiments and dedicated latency campaign.

Portable/legacy latency parity remains **open**. Runtime `b311711` removes redundant framing and scheduling work, and reduces concurrent-32 simulated instructions by 3.5% against frozen `8f7f500`. The full local timing comparison is effectively unchanged against that frozen portable control (+0.06% geometric mean), and portable remains 12.46% above current legacy across the 48 rows. These are exploratory results on a shared host, not qualified speedups.

## Shorter CI feedback

Commit `a065181` starts release benchmark builds after workflow validation, alongside the verification lanes. The final required CI result still requires Cargo/coverage, models, fuzzing, extended verification, portability, platform tests, documentation, benchmarks and cleanup. The default benchmark campaign is now `run-latency`: all 48 TLS/QUIC case combinations, five fresh-process repetitions, 100 warmups and 1,000 measured operations per repetition, with mean and spread. Full tracing/Callgrind/DHAT (`run-secure`), allocation-only diagnostics (`run-allocations`) and two-peer VPC measurements (`run-remote`) remain explicit manual choices. All six protocol fixtures still compile and receive local correctness smoke checks. This changes the routine performance scope; VPC timing and instrumented diagnostics are no longer on that required path.

The current qualification built, checked and archived its bundle in about 13 minutes (15:50–16:03 UTC). The preceding full campaign completed native timing and Callgrind, then exceeded its 7,200-second SSH limit during DHAT. Its cleanup succeeded. The fix removes profiling from routine latency feedback rather than extending the resource lifetime. The existing dedicated-measurement concurrency group, two-hour janitor, credentials restrictions, failure salvage and cleanup remain in place. A running long campaign can still occupy the shared measurement slot. New end-to-end CI duration is **not measured yet**, and other required verification lanes can still determine overall CI time.

Native-only analysis explicitly records that diagnostics were not requested. Full analysis still fails on missing profiles but preserves validated native data in `native-summary.json`; missing diagnostics cannot become a successful full qualification. Regression tests cover complete native data, corrupted samples, missing diagnostic evidence, default workflow policy, sampling budgets and cloud cleanup after injected failure.

## Implementation steps

Each step was frozen before timing; native measurements ran separately from all compilation and profiling. Steps 1–5 use seven selected cases per transport (14 rows); step 6 covers the full catalog. Instruction counts sum both peers and subtract matched 10-operation from 50-operation captures with two identical warmups. They are simulated work, not native time.

| Step | Change | Portable unary instructions | Portable pipeline | Portable concurrent-32 |
| --- | --- | ---: | ---: | ---: |
| 1 | Poll maintenance readiness once per coordinator visit | 47,010 | 145,273 | 898,156 |
| 2 | Retain parsed frame metadata through lookahead and receive | 46,443 | 144,202 | 892,411 |
| 3 | Return unused call and worker scheduling allowance | 46,486 | 144,355 | 890,355 |
| 4 | Resume growing-buffer searches after the last reservation | 46,784 | 144,437 | 882,836 |
| 5 | Batch plain outgoing calls while preserving idle lanes | 46,742 | 144,316 | 861,657 |
| 6 | Retain separate buffer-search cursors by size class | 46,768 | 145,160 | 862,973 |

The framing change reduces pipeline frame parsing from 26 calls to 6; current legacy also parses 6 frames. Buffer search previously walked 1,120 iterator entries per concurrent batch. Search cursors avoid repeatedly restarting at slot zero, but the full 42,520 self instructions of that old iterator are not all eliminated: some work moves into inlined search loops. Budget accounting now charges actual work while charging idle visits one step, preserving bounded polling. The outgoing fast path consumes plain calls while there is budget and keeps unrelated idle lanes asleep; a later capability-bearing call resets the direct-publication certificate and uses the checked path.

The size-class cursor experiment adds 512 bytes per growing buffer provider on a 64-bit host. Its final concurrent-32 cost (862,973 instructions) is slightly above the preceding single-cursor/batching step (861,657); no latency benefit is established for this extra metadata. Treat it as an experiment pending dedicated evidence, not a demonstrated win.

## Remaining latency gaps

Final portable pipeline work is 145,160 instructions versus legacy's 95,868 (+51.4%). It still performs 17 readiness probes, 13 hook collections and 13 work-budget entries per pipeline operation. Plain-call batching helps concurrency but does not remove the capability pipeline's repeated scheduling/validation path. The next architectural target is to retain readiness and validated ownership across those pipeline transitions while preserving cancellation, ordering, capability checks and bounded work.

The [complete timing table](comparison.md) reports means ± sample SD for all 48 rows. Portable has a lower mean than legacy in only 11/48 rows and lower than C++ in 23/24 supported TLS rows; those point estimates do not establish all-case performance parity. The host had a four-CPU outer quota, no observed native throttling, and per-trial steal peaks of 22% aggregate and 50% on a selected CPU. [Host evidence](host-health.json) explains why these results require dedicated confirmation.

The [five-pair socket trace table](io-comparison.md) confirms the common TLS paths already match legacy: dependent reads use 4 successful reads/4 writes per operation, and pipeline, fanout-32 and concurrent-32 use 2/2. The four-service/eight-client case remains a real coalescing gap: portable uses 32.70 ± 0.67 reads and 32.93 ± 0.59 writes versus legacy's 27.18 ± 0.50 and 27.19 ± 0.51. C++ uses about 50/50. Bulk writes are equal at 18; read counts vary and overlap in spread. Further common-path syscall reduction is unlikely to explain the pipeline CPU gap, but multi-service batching still warrants work.

The [allocation regression checks](allocation-comparison.md) retain TLS unary 7/7 and pipeline 58/58 allocations for portable/legacy; concurrent-32 remains 198 versus 202. The selected QUIC counts are also below legacy in this single paired sample. This does not repeat the full allocation catalog or qualify a five-repeat QUIC allocation target.

## Validation and qualification

The final runtime passed 221 engine, 375 RPC, 163 wire, 30 secure TLS/QUIC and 32 serialized Unix tests (821 executions), warnings-denied engine/RPC Clippy, formatting and alloc-disabled engine/wire checks. The earlier framing step also passed 78 futures tests and futures Clippy. Full workspace coverage was not rerun. A step-3 release build was killed for memory pressure, then rebuilt with the same flags; an intermediate missing test call-site update and Unix setup/parallel-run failures are retained with their corrected logs. The two parallel Unix EOF-witness failures are not diagnosed as a proven runtime bug; all 32 passed serialized.

The CI change passed all 68 development-tool library tests, 16 Python benchmark-analysis tests, development-tool Clippy, formatting, workflow policy validation and actionlint 1.7.12 (local shellcheck unavailable). The broader development-tool run exposed a stale Kani FIFO mutation pattern from the retired scanning dispatcher; the manifest now mutates ready-head selection to ready-tail selection. An isolated native FIFO regression passes on the original source and rejects that mutation. Kani itself was not run.

[Dedicated run 40108765911](https://github.com/DericHuynh/capntproto/actions/runs/40108765911) has passed its build and is measuring runtime `b311711` against frozen portable `8f7f500`, current legacy and supported C++. It retains the previous full campaign; the faster CI change does not alter an already-running workflow. Results are pending. Earlier run [40078632826](https://github.com/DericHuynh/capntproto/actions/runs/40078632826) timed out during DHAT after native timing and Callgrind; resource cleanup passed. No dedicated parity claim is closed by either a partial run or the local measurements.

## Evidence

[Provenance](provenance.json) records source and executable hashes, fixture hashes, all intermediate binaries, diagnostic scope and verified raw archive checksums. [Step data](steps.json), [selected function counts](function-counts.json), the per-step tables, validation logs and compressed raw latency samples are checked in. The three workspace archives preserve every original trial, full profiles, trace logs, setup failures, binaries, generated test certificates and host captures; their manifests were verified against every file before earlier expanded duplicates were pruned. Those archives are local evidence, not a claim of GitHub-hosted raw artifacts.

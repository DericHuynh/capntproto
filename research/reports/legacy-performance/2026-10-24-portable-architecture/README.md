# Portable architecture: producer effects, ownership and frozen publication

Implemented all three architectural changes in `da47cad`, then compacted their retained metadata after profiling, following the [architectural assessment](../2026-10-24-portable-latency-followup/README.md#architectural-assessment). Each step was committed, built, measured five times in fresh processes and profiled before proceeding to the next step's measurement. Steps 10–11 use 14 targeted TLS/QUIC rows; steps 12–13 use all 48 rows. Local latency is exploratory; the dedicated droplet workflow qualifies latency.

## Implemented changes

1. **Producer-specific scheduling (`750e7a5`).** Application, foreign, worker and command wakes retain their work kind. The wire driver reports typed maintenance effects, including work done before a later suspension or budget exhaustion. The coordinator preserves the unfinished fair rotation and applies these effects even when the final result is Pending. The six application lanes and sixteen maintenance classes still exist; general graph/network transitions retain conservative invalidation.
2. **Shared ownership mailbox (`4ff2c4e`).** Endpoint releases and metadata notifications use the inbox's retained ownership context and a dedicated consumer. Ordinary call registration no longer replaces ownership registration. Idle collections bypass the reconciliation body. A new regression exposed a pre-queued metadata watch which suppressed a subsequent wake; watches now register as parked, and consumed parent registrations are rearmed. The test exercises repeated notifications, aliases and final-owner destruction. The scoped capability graph remains the sole protocol authority; the lifetime bridge remains at that boundary.
3. **Immutable publication (`da47cad`).** Application results explicitly freeze into `FrozenReturn`, which retains segment layout and successful validation. The inline result or shared segmented owner carries that metadata across storage retries. Prepared calls cache validation until mutable parameter access invalidates it. Certificates match capability count and reader traversal/nesting limits; descriptor authority and resource ownership are still checked during publication. The result-copy path reads indexed segments directly, removing its temporary segment-reference vector. Large payload retention, bounded storage accounting, fixed-buffer fallback and transactional publication remain intact.

The third change follows C++'s retained response/message shape (`RpcServerResponseImpl`, `rpc.c++:2994`), where the response retains its message and payload and `send()` stages descriptors before sending. Its receive loop transfers an owned incoming message into dispatch (`rpc.c++:3568`). Portable retains its explicit capability validation and caller-backed storage contracts; this pass changes publication ownership and metadata, not the physical TLS/QUIC implementation.

## Breaking low-level API changes

`PreparedReturn` is the mutable construction phase. Call `freeze()` before passing the result to `try_send_answer_prepared*` or `local_answer_prepared*`; they now accept `FrozenReturn`. Read-only result access and framed sizing live on the frozen owner. `into_builder()` consumes it and discards its certificates before mutation. Generated RPC client/server interfaces are unchanged.

`maintenance_effects()` replaces `maintenance_published_retirement()`. Its bits are conservative scheduling consequences of attempted work, including work before a wait. The output bit is not a receipt or proof of publication. No compatibility wrapper preserves the previous low-level contracts.

## Work measured after each step

All-peer TLS Callgrind instructions per whole operation, using paired (50−10)/40 slopes and two identical warmups. Parentheses compare portable against the frozen `87be38e` portable control measured in the same step. These are simulated work counters, not latency ratios.

| Case | Step 10 | Step 11 | Step 12 | Step 13 | Legacy (final capture) | C++ |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary-small | 46,817 (+0.08%) | 46,830 (+0.03%) | 46,987 (+0.41%) | 46,869 (+0.27%) | 46,051 | 114,836 |
| pipelined-read | 145,186 (+0.05%) | 145,587 (+0.37%) | 145,776 (+0.50%) | 145,745 (+0.50%) | 95,862 | 303,261 |
| concurrent-32 | 855,530 (+0.01%) | 856,363 (+0.11%) | 867,174 (+1.31%) | 863,815 (+0.92%) | 674,517 | 3,221,817 |

Step 12 added 24 allocation bytes per ordinary prepared request and increased concurrent instruction cost by about 1.3% against its frozen control. Step 13 compacts the exact validation identity to two machine words on this host, uses the unused sign bit of a positive nesting limit to distinguish unlimited traversal from every finite limit, and retains only framed length because header length follows directly from segment count. The ordinary first validation avoids constructing a comparison key when no certificate exists. Its regression includes finite `usize::MAX`, unlimited traversal and maximal nesting/capability counts.

The final portable instruction counts remain 0.27% higher for unary, 0.50% higher for pipeline and 0.92% higher for concurrent-32 than the frozen portable control. Compaction recovered part of the initial metadata overhead, but this pass does not demonstrate a CPU-work reduction or legacy parity in those fixtures. Retained validation principally avoids repeating work during publication retries; ordinary calls still pay for their first validation.

The [profile data](profile-comparison.json) also records conditional and indirect branch misses. Per-step comparisons and validation logs are under [steps](steps/). Function-count evidence is retained with the raw profiles; inlining a guard is not itself evidence that its work disappeared.

## Latency, allocations and socket calls

The [full comparison](comparison.md) covers 48 TLS/QUIC rows with five fresh-process repetitions, 50 warmups and 500 measured operations. Values are mean ± sample SD of the five run means. Concurrent rows time a whole batch. Current legacy, current portable, frozen legacy/portable and pinned C++ are interleaved; C++ has no QUIC fixture. The JSON retains both frozen controls.

Local point estimates:

- portable-baseline: lower mean in 25/48 rows; geometric-mean change -2.37%
- legacy: lower mean in 11/48 rows; geometric-mean change +12.17%
- cpp: lower mean in 22/24 rows; geometric-mean change -29.99%

These point estimates do not establish a dedicated-host performance target. [Host evidence](host-health.json) records enclosing CPU quota, throttling and selected-CPU steal; native trials did not overlap compilation, profiling or evidence archiving.

The [allocation table](allocation-comparison.md) reports all-peer 10/50-operation DHAT slopes for unary, pipeline and concurrent-32 on TLS/QUIC. It is one diagnostic pair per case/implementation, not repeated allocation qualification. Final TLS allocation counts are unchanged from frozen portable: 7, 58 and 198 per whole operation, respectively. Retained metadata adds about 16 bytes per ordinary request after compaction (512 bytes per concurrent batch of 32). The fixed-storage two-segment publication regression separately asserts zero allocations, including validation and reading back both publications.

The [socket table](io-comparison.md) reports means and sample SDs of five paired (100−10)/90 slopes for successful TLS ciphertext reads/writes across every peer. [Attempted calls](io-attempt-comparison.json) retain EAGAIN/unsuccessful calls separately. Dependent, pipeline, concurrent, fanout, bulk and four-service/eight-client patterns are covered. Diagnostic timings are excluded from the latency table. Portable matches legacy's successful counts for dependent, pipeline, concurrent and fanout fixtures. The service-chain gap remains: portable averages 33.14 reads / 33.32 writes per operation versus legacy's 27.82 / 27.85. That is essentially unchanged from frozen portable's 33.15 / 33.31; these architectural changes do not resolve service-chain coalescing.

## Validation and qualification

The final runtime passed 221 engine, 377 RPC, 166 wire, 30 secure TLS/QUIC and 32 serialized Unix tests: **826 test executions**. Engine/RPC/wire Clippy passed with warnings denied, as did formatting and engine/wire no-allocation checks. The tests cover stricter limits after cached success, changed capability-table bounds, mutation after thawing, capacity rollback, cancellation, retained response ownership and repeated metadata notifications. Full workspace coverage and Miri were not rerun.

Intermediate failures are retained in the validation directories: the maintenance API's test migration, the ownership test's lifetime annotation, the metadata notification regression, and Clippy's request-owner size diagnostic. The final compact representation keeps requests inline and passes Clippy without the large-variant allowance needed by the initial representation.

[Final dedicated qualification](https://github.com/DericHuynh/capntproto/actions/runs/40124330566) uses the fast `run-latency` campaign and frozen `87be38e` controls. Its qualification commit is `f404764`; its runtime source is `31f7a33`. The [initial three-change qualification](https://github.com/DericHuynh/capntproto/actions/runs/40122332537) separately measures `da47cad` and was preserved. Status is recorded in [provenance](provenance.json). This report does not substitute local timings for its result.

The prior full campaign [40108765911](https://github.com/DericHuynh/capntproto/actions/runs/40108765911) reached its 7200-second child-process deadline; its incomplete profile then failed validation. The earlier fast campaign [40116177710](https://github.com/DericHuynh/capntproto/actions/runs/40116177710) completed successfully and measures the preceding runtime, not these architectural changes. Both were preserved. The feature branch was pushed; main was not merged.

## Evidence and remaining work

[Provenance](provenance.json) pins source revisions, executable hashes, compiler/profile settings, controls, checks and the checksum-verified raw archive. The archive retains every native trial, all-peer profile, allocation capture, socket trace, helper and validation log. The dedicated workflow publishes its own artifacts.

The architecture now preserves producer identity and immutable publication metadata at the relevant boundaries. Remaining costs include general graph-transition invalidation, the scoped/owned lifetime boundary, small-frame receive copies and service-chain flush/coalescing decisions. Equal allocation counts or fewer adapter calls alone do not establish legacy latency parity; the benchmark tables show the remaining measured gaps.

Final runtime: `31f7a335773a6faae60b529aa178821daacff150`.

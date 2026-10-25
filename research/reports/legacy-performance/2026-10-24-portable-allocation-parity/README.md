# Portable allocation ownership — 24 October 2026

This pass separates temporary inspection from retained ownership, removes duplicate owners around prepared frames and replies, and lets the portable driver use the same direct response-future path as legacy. Six independently frozen implementation steps have validation logs and five-repeat local timing comparisons. The final candidate is `8f7f500a0584b0dec725e3598637d09494057cc3`.

The original three TLS allocation gaps are closed in the step-5 profiles: portable unary falls from 10 to 7, pipelined-read from 67 to 58, and bulk-1m from 156 to 143 allocations per operation, against legacy's 7, 58, and 145. These are warmed, startup-subtracted counts summed over both peers, not peak live allocations. Final broader measurements follow below.

## Changes and intermediate measurements

| Step | Commit | Portable unary | Portable pipeline | Portable bulk-1m |
| --- | --- | ---: | ---: | ---: |
| Frozen starting implementation | `36318e3` | 10 | 67 | 156 |
| Borrow validation readers; embed retained payloads | `5302f84` | 10 | 64 | 156 |
| Fuse retained output ownership; inline small headers | `ba5c9e9` | 10 | 64 | 148 |
| Transfer unique reply readers; borrow server parameters | `dbe7024` | 8 | 61 | 146 |
| Keep contiguous results in producer storage; traverse pipeline paths directly | `3d2287f` | 7 | 58 | 146 |
| Borrow builder segments during certification | `22f373f` | 7 | 58 | 143 |
| Direct response future, bounded concurrent completion reuse, inline first waiter | `8f7f500` | 7 | 58 | 143 |

Temporary validation no longer promotes a frame to a shared reader or materializes a vector of segment references. Unique reply readers retain their consumed traversal budget through selection and retry. Shared observers retain the original shared arena and its budget. The selected payload remains checked under the same bounds, nesting, capability-index and traversal rules.

Retained output stores its parts and accounting in one shared owner, with small immutable envelope bytes inline. Partial writes and retained aliases continue to hold the original message storage and its complete byte charge. Single-segment results stay in the existing producer state; segmented output retains a separate message owner so an outstanding write does not pin a reusable worker context.

The wider step-4 profile uncovered a different concurrency problem: 258 allocations versus legacy's 202 for concurrent-32, and 264 versus 208 for deep-32-concurrent-32. Each batch allocated 30 completion states and 30 separate response receivers after exhausting a two-entry completion cache. The four-service fixture additionally allocated 96 waiter vectors; completion and receiver churn each contributed about 101.51 allocations per batch in that profile.

Portable now polls the existing completion through `ResponsePromise::from_shared`, as legacy already does. The obsolete `ResponseReceiver`, `from_shared_response`, and cached-response enum variant are removed. Two common completion-cache entries remain inline, with a lazily grown overflow bounded by the inbox's existing maximum-call setting. Only empty, exclusively owned states may be reused. A pending server invocation keeps its first waiter inline; additional distinct observers still receive their notifications. Waker clone/drop callbacks run outside the waiter borrow.

This retains empty completion metadata up to the connection's observed concurrency, bounded by its configured call limit. New high-water marks still allocate. The cache does not retain application results or task wakers, and live response/pipeline interests prevent premature reuse. The measured reductions describe warmed connections; they are not a claim that first-use allocations disappear.

## Final allocation catalog

All **24 TLS cases** now meet or beat legacy's allocation count; every TLS count is also below the complete C++/OpenSSL fixture. The QUIC catalog has **20/24** point estimates at or below legacy. The [full table](allocation-catalog.md) and [counts plus bytes](allocation-catalog.json) include all 48 case/transport combinations. There is no supported C++ QUIC fixture. Different crypto libraries contribute to the complete TLS implementation comparison; these figures do not isolate RPC-engine efficiency.

| TLS workload | Legacy | Portable | C++ |
| --- | ---: | ---: | ---: |
| unary-small | 7.000 | 7.000 | 91.489 |
| dependent-read | 46.000 | 46.000 | 246.978 |
| pipelined-read | 58.000 | 58.000 | 233.489 |
| callback | 36.000 | 34.000 | 204.978 |
| bulk-1m | 145.000 | 143.000 | 2363.800 |
| concurrent-32 | 202.000 | 198.000 | 2062.489 |
| deep-32-concurrent-32 | 208.000 | 204.000 | 2146.622 |
| services-4-deep-16-clients-8 | 1660.089 | 1571.756 | 8559.822 |
| services-8-deep-16-clients-8 | 3522.222 | 3383.222 | 16967.533 |

The catalog uses 240 DHAT trials: 5- and 50-operation captures with 10 warmups, summed over all peers, then divided by 45. Startup/teardown allocations are subtracted. Concurrent cases measure a whole batch, not an individual RPC. No trial failed. Allocation count, cumulative bytes and peak live memory are different metrics.

The initial QUIC bulk-1m, deep-32, write-read and deep-8 portable totals exceeded legacy slightly. Five paired allocation samples per implementation (the original pair plus four more pairs, 64 additional trials) test that variation:

| QUIC case | Implementation | Total blocks, mean ± SD | Quiche blocks | Other blocks |
| --- | --- | ---: | ---: | ---: |
| bulk-1m | legacy | 1250.951 ± 6.910 | 1194.787 | 56.164 |
| bulk-1m | portable | 1253.782 ± 8.563 | 1199.271 | 54.511 |
| deep-32 | legacy | 22.516 ± 0.264 | 16.471 | 6.044 |
| deep-32 | portable | 22.071 ± 0.351 | 16.071 | 6.000 |
| write-read | legacy | 43.204 ± 0.312 | 32.182 | 11.022 |
| write-read | portable | 43.138 ± 0.228 | 32.138 | 11.000 |
| deep-8 | legacy | 22.462 ± 0.236 | 16.418 | 6.044 |
| deep-8 | portable | 22.133 ± 0.228 | 16.133 | 6.000 |

Across these five samples, portable's mean is lower for deep-8, deep-32 and write-read. Bulk-1m remains **+2.831 blocks (+0.226%)** above legacy: 1,253.782 ± 8.563 versus 1,250.951 ± 6.910. The difference is smaller than the observed sample spread, but that does not establish equal totals. Dedicated measurements remain the qualification gate.

The [repeated samples](quic-repeated-allocations.json) retain every result. Attribution first assigns a stack containing `quiche::` to quiche, then received rustls plaintext, then the remainder. In the full catalog, none of the 24 QUIC cases has a portable excess outside the quiche-attributed stacks. “Other” includes RPC, runtime, fixture and remaining transport work; it is not a pure RPC count. These packet-dependent totals must not be rounded into an all-QUIC parity claim.

## Timing, instructions and socket calls

The [48-case latency table](comparison.md) reports all five fresh-process repetition means and their sample standard deviation. Each repetition uses 50 warmups and 500 measured operations. The final campaign has **1,080 native trials**; raw per-operation samples are retained in [latency-samples.json.gz](latency-samples.json.gz). Its before controls are step 5 (`22f373f`), not the initial `36318e3` implementation. Separate [step reports](steps/) preserve the incremental comparisons after each change.

Portable has a lower local mean than step 5 in 29/48 rows, with a -0.66% geometric-mean change. It has a lower mean than C++ in 23/24 TLS rows, with a -32.08% geometric-mean difference. These are exploratory results with visible run spread, not dedicated-host qualification or a uniform speedup. The [host counters](host-health.json) retain enclosing CPU quota, throttling and trial steal measurements.

There was no enclosing quota throttling during either native phase, but aggregate trial CPU steal reached **9.09%**. Pipelined-read is the only TLS row with a higher portable mean than C++: **71.93 ± 20.87 µs versus 66.31 ± 6.82 µs**. Its mean also rose from step 5’s 59.27 ± 5.17 µs. This possible regression needs dedicated verification; the large spread and nearly unchanged simulated instruction count do not establish its cause.

Callgrind gives portable/legacy **46,808/46,375** instructions for unary, **146,030/96,206** for pipelined-read, and **18,105,392/18,066,793** for bulk-1m. Pipeline remains about **51.8% above legacy**, although allocation counts now match. Its profile contains more layout traversal, copying, readiness and maintenance work; file-level attribution identifies areas to investigate, not proof that all of that work can be removed. Step 6 changes pipeline instructions by only +0.062% versus step 5. The [full simulated counters](profile-comparison.json) retain branch and cache events and hot files; instrumented timing is excluded from latency results.

The [socket table](io-comparison.md) reports successful ciphertext reads and writes across both peers. Five 10/100-operation pairs with identical warmups remove fixed handshake and teardown costs. [All attempts](io-attempt-comparison.json), including unsuccessful calls, are retained separately. These four TLS cases do not establish read/write parity for the entire catalog or QUIC.

Portable and legacy both use **4 reads / 4 writes** for dependent-read and **2 / 2** for pipelined-read and concurrent-32. The C++ pipeline also uses **2 / 2**. Bulk-1m portable averages **32.142 ± 7.782 reads / 18 writes**, versus legacy **31.131 ± 6.554 / 18** and C++ **258.000 ± 0.011 / 257.991 ± 0.012**. Pipeline's remaining CPU gap therefore is not explained by extra successful ciphertext socket calls in these captures.

## Validation

The final candidate passes core and RPC unit tests, wire tests, selected core reader/budget tests, TLS/QUIC secure RPC tests, native/identity/multiparty/shutdown tests, core/RPC documentation tests, warnings-denied Clippy, formatting, and alloc-disabled core/wire compilation. Final passing executions total **745**: core 82, wire 163, RPC 374, selected core integration 25, secure RPC 30, native/identity/multiparty/shutdown 35, and documentation 36. Eight documentation examples are ignored. [Exact commands and outcomes](checks.json) accompany retained logs in the raw archive. One native-test attempt omitted `TLA2TOOLS_JAR`; the corrected run passed with the repository's existing jar. The earlier steps also retain their engine and serializer checks. This is not a fresh full-workspace coverage run.

## Public API and remaining architecture

The public low-level `OutputSegments` enum gains a two-segment form, retained parts accept owned storage, prepared-result callbacks expose inline/shared payload ownership, and prepared capability lists use an exact-size iterator. Selected-reader recovery is additive. The response receiver API is removed in favor of the existing direct shared response constructor. Repository callers are migrated without compatibility shims. The shared framing representation also increases legacy’s measured pipeline allocation bytes by about 64 versus the frozen starting implementation, while its allocation count stays at 58.

Allocation-count parity does not establish byte or latency parity. In the final profiles, portable still allocates about 40 extra bytes for unary, 780 for pipeline, and 592 for bulk-1m. Pipeline scheduling, readiness propagation and maintenance remain separate CPU costs. These byte and instruction gaps remain open even where allocation counts match.

## Rustls comparison with our original patch

This pass does not change the vendored TLS implementation. Our original `58205dd` patch already encrypted ring TLS 1.3 directly into caller output, so adopting upstream PR #3127 did not remove another allocation on that active path. The original patch retained owned `encrypt` and added a default `encrypt_into` fallback; upstream replaces the provider contract with borrowed-output `encrypt` and implements direct output for all seven ring/AWS-LC TLS 1.2/1.3 paths. The provider trait changes, while capntproto's TLS constructors stay the same.

Both versions still copy plaintext once into its ciphertext destination. The targeted 0.23 backport also retains receive-record ownership: both Rust drivers allocate 130 received-plaintext record buffers per selected bulk-1m round trip. Upstream main's surrounding 0.24 implementation has borrowed receive handling, split send/receive state, a chunk-oriented `write_tls_into` entry point and buffered output recycling; those surrounding changes are not part of this backport. The borrowed receive path is a shared transport opportunity, not a portable-versus-legacy allocation delta. The [preceding report](../2026-10-24-upstream-encryption/README.md) preserves the 543 ring, 549 AWS-LC and 950 non-FIPS upstream test executions and original-versus-upstream measurements.

## Dedicated qualification

[Run 40078632826](https://github.com/DericHuynh/capntproto/actions/runs/40078632826) uses workflow commit `3d2b627807d04246f474d8d9d80130209ed0219f`, with runtime parent `8f7f500`. Both controls are frozen at `aad53bf` (runtime `36318e3`). Its guard waited for run 40070062202 before entering the benchmark queue. The run completed native timing and Callgrind, then timed out during DHAT at the 7,200-second SSH limit. Cleanup of both droplets and keys passed; this incomplete campaign does not close qualification. The [latency and CI follow-up](../2026-10-24-portable-latency/README.md) records the new candidate, preserved evidence and faster routine workflow.

## Evidence and reproduction

[Provenance](provenance.json) records executable hashes, source/fixture hashes, commands, ordering, unsupported combinations, and the verified raw archive manifest. Native timing runs never overlap compiler or profiler execution. The initial ten-case native campaign and remaining fourteen-case campaign are sequential partitions of the same catalog with the same frozen executables and budgets. One attempt to nest the remaining allocation campaign failed its exclusive-lock setup check before any trial; it was rerun as a separate sequential campaign. The setup failure log is retained.

The raw archive contains all six stages' profiling records, validation logs, native samples, host captures, analysis scripts and build metadata. Generated certificates/private keys and executable files are excluded; binary hashes are retained. Its checksum is verified against every archived file. Local artifact paths are workspace evidence, not a claim that GitHub hosts the raw archive.

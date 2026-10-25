# Incremental legacy extraction: Return ownership

Runtime: `6904a4ce5dda49470303b586f4d2b87a7a25dc47`. Qualification: `bb7c58da6d2c853e25e7d42a320ecc62b0efeaf4`. Control: `a5677a63bb13273dff614205367f944a00145acf`, the preceding cancellation extraction. Main is unchanged.

## Decision from the preceding reports

The [direct-completion report](../2026-10-24-legacy-factoring/README.md) records unchanged TLS allocations and three additional delivery instructions per question. The [cancellation report](../2026-10-24-legacy-cancellation/README.md) records unchanged TLS allocations, one additional callback answer-task instruction and completed droplet latency of +0.33% TLS / +1.27% QUIC geometric mean. Those runs do not establish exact instruction parity or every-case timing preservation. The new candidate isolates the next small boundary identified by those reports; it does not silently treat earlier timing regressions as resolved.

The reports also identify a measurement confound: the same runtime produced different whole-binary sizes in different checkout locations. This focused workflow now checks out the frozen and candidate revisions sequentially into **the same `target/legacy-profile-source` directory**, using the same compiler, Cargo target directory and release profile. It saves the control executable before changing the checkout. Tests and the profiler remain in the outer candidate checkout. The reusable dedicated latency workflow still builds its controls in separate checkout paths; that limitation remains visible.

## Extracted rule

`engine::completion::return_guard::ReturnOwnership` borrows the existing guard count and Return-claimed flag. Legacy's constructor and clone retain a guard, explicit completion marks the Return sent, and the guard destructor releases one interest. Only the last unsatisfied release claims publication, before any connection lookup, wire output or user cleanup can reenter.

The hosted `ReturnGuard` still owns its original weak connection reference, answer ID, cached answer reference and flags. `AnswerStatus` retains the same field declarations and types. Neither object stores the borrowed core view. The original send/cleanup path remains direct; no new table, queue, task, owner allocation or reference count is introduced.

This follows the pinned [C++ `RpcCallContext` destructor and first-responder rule](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.c%2B%2B#L3193). A Finish does not by itself prove that all Results or pipeline owners have ended. The core counts retained interests; hosted code still decides whether to send Canceled, acknowledge redirected results, retire a pipeline-only answer or finish terminal cleanup.

Overflow and unmatched release panic before changing either cell. The previous hosted release used integer subtraction; the benchmark already enables overflow checks. The extracted public primitive makes underflow checking explicit in every build. Valid guard lifetimes keep the same transition order, but the invalid-input contract is now explicit.

## Verification

Four new tests cover zero allocation counts and bytes, caller storage reuse, explicit completion at every release boundary for one through four guards, publication before reentrant cleanup, and overflow/underflow without partial state mutation. Existing hosted RPC and encrypted TLS/QUIC suites cover the adapter's lifecycle behavior.

The complete engine builds for `thumbv7em-none-eabi` with no features. A [standalone probe](embedded-link.rs) links without a global allocator; the [ELF check](embedded-link-local.json) finds no undefined or allocator symbols. It exercises retained guards, final publication, reentry, explicit completion and storage reuse. This is link-only evidence, not execution on a board or a claim that the complete hosted driver is allocation-free.

[Focused CI](https://github.com/DericHuynh/capntproto/actions/runs/40140249948) passed **234 engine alloc, 188 engine no-alloc, 377 hosted RPC and 30 encrypted TLS/QUIC tests**: 829 executions across overlapping profiles. Warnings-denied engine/RPC Clippy, workspace formatting, the Cortex-M engine build, RPC no-default check and the standalone allocator-free link passed. [Validation](validation.json) records the job and source identities. All 48 profiling captures completed after compilation, with no failures. The dedicated-droplet campaign has now completed successfully. Its five-repeat latency results are retained below, including regressions.

## Measured result

The [function-level trace](instruction-functions.json) includes unchanged functions as well as differences. In all six cases, the measured self-instruction slopes for Return-guard destruction, `answer_has_sent_return`, Results construction/destruction and the other selected Return paths are identical between the two builds. For example:

| Case | Guard destruction, both builds | Answer completion, both builds |
| --- | ---: | ---: |
| Unary | 101 | 70 |
| Dependent read | 202 | 139 |
| Pipelined read | 202 | 139 |
| Concurrent-32 | 3,232 | 2,672 |
| Callback | 202 | 140 |
| Deep-32 / concurrent-32 | 3,232 | 2,672 |

These are all-peer, paired (50−10)/40 self-instruction counts per whole operation, with two warmups. The earlier callback answer-task slope remains 183 in both builds; this extraction does not remove the preceding cancellation extraction's one-instruction difference. Matching selected counts does not prove byte-identical machine code or identical latency on every path.

Whole-operation instruction changes are **+0.130%, +0.086%, +0.007%, −0.008%, −0.003%, and −0.005%** in the table's order. Most differences are cryptographic work, memory routines and small randomized-map differences outside the extracted rule. Conditional and indirect branch-miss counters still vary; [all counters](comparison.md) and [machine-readable values](comparison.json) remain visible. In particular, unary indirect misses rose 12.65%, while concurrent-32 indirect misses fell 37.30%. These single diagnostic pairs are not timing evidence.

TLS allocations remain exactly **7 / 58 / 202** for unary / pipeline / concurrent-32, with approximately **912 / 7,448 / 40,520 bytes**. Byte deltas are **+0.2 / 0 / +0.1** per operation. The pipeline's normalized allocation stacks and bytes match exactly.

QUIC counts are **21.850 vs 21.450**, **72.150 vs 72.850**, and **219.025 vs 218.700**. [Allocation-stack differences](allocation-stacks.json) reconcile with every original count and byte total. Next-packet-key derivation contributes **+0.350 / −0.700 / +0.350** allocations; signature encoding contributes **+0.050 / 0 / −0.025**; formatting count differences cancel. No differing count stack is in Return ownership. This supports retaining the small extraction but does not establish repeated QUIC allocation parity.

With both revisions built at the same source path, **`.text` is 9,872,167 bytes and `.rodata` is 828,005 bytes in both binaries**. The earlier apparent 768-byte code-size saving is absent in this comparison. No size reduction is credited to this extraction.

The measured guard/answer paths preserve their instruction counts and TLS allocation counts. The completed latency run does not establish every-case preservation.

## Completed dedicated latency

All jobs in [run 40140249948](https://github.com/DericHuynh/capntproto/actions/runs/40140249948) succeeded, including cleanup. The run retained 1,080 trials and 48 transport/case rows, with five fresh-process repetitions and 1,000 samples per repetition. [The complete table](latency.md) reports mean ± sample standard deviation of the five repetition means; [raw summary](latency-summary.json), [comparison](latency-comparison.json) and [provenance](latency-provenance.json) retain all values. This is a separate native campaign from the instruction captures.

Against the cancellation-extraction control, geometric-mean latency is **+0.90% TLS** and **+2.36% QUIC**. Lower means appear in 14/24 TLS and 11/24 QUIC cases. This is not a speedup or a parity result. Representative TLS values in microseconds are:

| Case | Frozen mean ± run SD | Factored mean ± run SD | Change |
| --- | ---: | ---: | ---: |
| Unary | 45.19 ± 10.38 | 51.27 ± 11.53 | +13.47% |
| Dependent read | 108.68 ± 19.97 | 86.82 ± 0.99 | −20.11% |
| Pipeline | 52.95 ± 0.93 | 59.71 ± 15.89 | +12.75% |
| Concurrent-32 | 135.14 ± 2.23 | 151.92 ± 26.92 | +12.42% |
| Callback | 103.15 ± 20.68 | 99.27 ± 19.67 | −3.76% |
| Deep-32 / concurrent-32 | 461.28 ± 19.65 | 461.86 ± 72.38 | +0.13% |

The unmodified portable-path comparison also swings, including +15.55% for QUIC deep-32 and −15.13% for TLS deep-32. Every trial has CPU accounting, with no observed cgroup throttling; maximum measured aggregate CPU steal is 3.03%, and maximum single-CPU steal is 12.5%. Repetition spread, control movement and separate build paths limit causal attribution. They do not prove that the listed regressions are harmless or entirely host-caused. No case or sample was removed.

The later [idle-task investigation](../2026-10-24-legacy-idle/README.md) addresses unnecessary scheduler work and repeats simulated counters with an identical-binary control. Its runtime is not in this native run.

## Remaining ownership boundary

The next substantive boundary is task readiness and storage. Hosted task admission still boxes arbitrary `'static` futures, retains a growing admission queue, and uses shared wake nodes with a `FuturesUnordered` fallback beyond the reusable set. Extracting readiness transitions can preserve the current wake-driven ordering, but a bounded implementation must also supply stable task storage, wake lifetimes and an explicit capacity-failure policy. Preserve ordinary-work-before-`evalLast` ordering and avoid an extra scheduler between receive and dispatch.

Hook and message lifetimes still require owned boxes, reference counts and vectors. Borrowed or arena-backed alternatives must retain results through pipelining, cancellation, partial output and reentrant destruction. The existing no-default-feature RPC build selects the bounded driver and excludes hosted legacy modules; it does not prove these remaining legacy owners are allocation-free.

A bounded output adapter must also distinguish **claiming a Return** from **acceptance by the output queue**. If output capacity is temporarily unavailable after the final guard claims publication, the existing answer owner must retain that pending publication until it is accepted or the connection becomes terminal. Resetting the claim to retry could allow reentrant duplicate Returns; dropping the pending owner could lose the response or reuse its ID too early. This extraction preserves legacy's current direct publication policy and supplies only the first-responder decision. It does not by itself implement bounded output backpressure.

## Reproduction

The exact [workflow](https://github.com/DericHuynh/capntproto/blob/bb7c58da6d2c853e25e7d42a320ecc62b0efeaf4/.github/workflows/legacy-factoring.yml) and [profile helper](https://github.com/DericHuynh/capntproto/blob/bb7c58da6d2c853e25e7d42a320ecc62b0efeaf4/benchmarks/rpc/legacy-factoring-profile.py) are saved on the qualification branch. [Provenance](provenance.json) records source identities and the comparison setup.

The [checksum-verified raw artifact](https://github.com/DericHuynh/capntproto/actions/runs/40140249948/artifacts/12579116027) contains all captures. Run `analyze-instructions.py ARTIFACT.zip OUTPUT_DIR` and `analyze-allocations.py ARTIFACT.zip OUTPUT_DIR` to reconstruct the traces. Both parsers check their totals against the original counters; the instruction parser explicitly retains any summary-minus-attributed-total residual.

For the local link probe, with Rust 1.97 and the Cortex-M target:

```sh
rustc --edition=2021 --crate-type=lib --crate-name capntproto_rpc_engine --target thumbv7em-none-eabi crates/capntproto-rpc-engine/src/lib.rs -o /tmp/libcapntproto_rpc_engine_factor3.rlib
rustc --edition=2021 --target thumbv7em-none-eabi -C panic=abort -C opt-level=2 --extern capntproto_rpc_engine=/tmp/libcapntproto_rpc_engine_factor3.rlib research/reports/legacy-performance/2026-10-24-legacy-return/embedded-link.rs -o /tmp/legacy-factor3-embedded.elf
python3 research/reports/legacy-performance/2026-10-24-legacy-return/verify-embedded-elf.py
```

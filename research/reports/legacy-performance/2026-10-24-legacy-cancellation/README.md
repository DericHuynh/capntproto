# Incremental legacy extraction: answer cancellation

Runtime change: `38da514a0016818195e112318a2a052e875138f7`; test lint correction: `a5677a6`. Qualification: `04951ba38fe74b8264e17098812b89bc84822f34`. The immediate control is the preceding direct-completion extraction, `54f9c2e829c5274ef10c0703d402172ff25031b1`. Main is unchanged.

## Change and ownership

Legacy now calls `engine::completion::cancellation::Cancellation` for answer-task polling, cancellation and wake-registration cleanup. This is a temporary borrowed view of the existing cancellation flag and waker. `AnswerStatus` retains exactly the same fields, their declaration order and types; the view is not stored in it. The extraction adds no task, channel, owner allocation, reference count or dispatch loop.

The algorithm retains both cancellation checks, waker identity reuse and the existing output-discard behavior: the wrapped answer future handles Return/Exception itself, and its completion or cancellation reports successful retirement to the task reaper. Canceling publishes the flag before waking; replacement and cleanup drop wakers outside the `RefCell` borrow. Future polling also runs outside that borrow. Task destruction remains the hosted owner's responsibility.

The component uses only `core`, accepts pinned futures that borrow caller storage, and does not impose `Unpin` on those futures. It does not own or recycle the operation cells. A caller must end previous cancellation interests before resetting and reusing them. Future and waker implementations retain their own allocation policies.

## Verification and acceptance

Five new tests exercise zero allocation counts and bytes, repeated wake registration, repeated cancellation, task completion, cancellation during replacement-waker destruction, teardown reentry and synchronous reentrant waking. Existing hosted lifecycle and encrypted-transport tests remain part of qualification.

The local engine builds for `thumbv7em-none-eabi` without features. The [standalone probe](embedded-link.rs) links against that engine without a global allocator, and its [ELF check](embedded-link-local.json) finds no undefined or allocator symbols. The probe covers a borrowed pending task, repeated polling, cancellation without another task poll, storage reset and successful completion. This is a link check, not execution on a board or proof that the entire hosted driver needs no allocator.

Local native profiling is unavailable after the previous environment OOM killed Docker. [Focused CI](https://github.com/DericHuynh/capntproto/actions/runs/40134974277) builds both revisions with the same compiler/profile, then runs paired Callgrind and DHAT captures only after all builds finish. Those captures measure work and allocations, not accepted wall-clock performance. The chained dedicated-droplet campaign uses five fresh-process repetitions with means and spreads for TLS and QUIC.

The first qualification attempt passed both engine test profiles, then Clippy rejected a deliberately destructor-only test waker as a manual no-op. A documented exception on that test adapter preserves the behavior under test; no runtime change was needed. The repeated focused job passed **230 engine alloc, 184 engine no-alloc, 377 hosted RPC and 30 encrypted TLS/QUIC tests**: 821 executions across overlapping profiles. Warnings-denied engine/RPC Clippy, workspace formatting, Cortex-M engine build, RPC no-default check and the standalone allocator-free link all passed. [Validation identities](validation.json) retain the failed initial attempt and successful replacement.

## Measured work and allocations

All 48 diagnostic captures completed without failure. [Full counters](comparison.md) and [machine-readable results](comparison.json) retain instructions, conditional and indirect branch misses, allocation counts and bytes. These are single paired (50−10)/40 slopes over all peers, with two warmups per capture.

| Case | Whole-operation instruction change |
| --- | ---: |
| Unary | −0.019% |
| Dependent read | −0.050% |
| Pipelined read | −0.040% |
| Concurrent-32 | −0.008% |
| Callback | +0.206% |
| Deep-32 / concurrent-32 | −0.007% |

The [function-level trace](instruction-functions.json) shows that most of the callback increase comes from elliptic-curve work in the independent startup captures. It also records a small actual answer-task difference: the callback `AnswerTaskDriver::poll` slope is **183 versus 182 self-instructions per operation**. That one instruction remains visible; this is not a claim of exact generated-instruction parity. Branch-miss counters also differ, including +11.0% conditional misses for concurrent-32 and −47.3% indirect misses for that case. These diagnostic counters do not establish a latency win or loss.

TLS allocation counts remain exactly **7 / 58 / 202** for unary / pipeline / concurrent-32. Bytes remain approximately **912 / 7,448 / 40,520** per whole operation, with paired-capture deltas of **+0.1 / −0.2 / −0.2 bytes**. There is no added per-operation owner allocation in these cases.

QUIC slopes are **22.050 vs 21.375**, **73.025 vs 72.725**, and **219.075 vs 219.425** allocations per operation. [Allocation stacks](allocation-stacks.json) preserve address/source-normalized frames; a separate [compiler-index-normalized view](allocation-stacks-canonical.json) groups Rust's renumbered `capnp_rpc::rpc::{impl#N}` frames. Both views reconcile exactly with the original allocation and byte totals. After that grouping, differing count stacks are next-packet-key derivation, ECDSA signature encoding and formatting. Key derivation contributes **+0.700 / +0.350 / −0.350** allocations; signature encoding contributes **−0.025 / −0.050 / 0**, and formatting count differences cancel. No remaining differing count stack is in the extracted cancellation path. Independent crypto/startup captures still prevent a claim of repeated QUIC allocation parity.

Candidate `.text` is **9,871,399 bytes**, the same byte count as the previous candidate built in the same workspace location. Its paired control is 768 bytes larger, but builds use different source checkout locations; the same gap appeared before this extraction. Treat this as observed whole-binary size, not a causal 768-byte saving from cancellation.

The candidate is retained for dedicated latency qualification. The chained droplet campaign is building its bundle and will compare this runtime with `54f9c2e` using five process repetitions per case. No wall-clock improvement or every-case preservation is claimed yet. The preceding extraction's completed droplet [table and spreads](../2026-10-24-legacy-factoring/latency.md) are also now saved: TLS geometric mean −0.24%, QUIC +0.73%, with noisy individual regressions retained.

## Next boundaries toward allocation-free legacy

1. **Return ownership.** `ReturnGuard` combines a small counter/first-responder rule with `Weak<ConnectionState>`, cached `Rc<AnswerStatus>` ownership and wire publication. Extract the counting decision next, keeping the existing owner and direct publication path. Finish, retained Results and pipeline owners may release in different orders; only the final unsatisfied guard sends cancellation. Do not duplicate this state in a second table. The pinned [C++ `RpcCallContext` destructor](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.c%2B%2B#L3193) uses the same first-responder rule after reference-counted context lifetime ends; its [`handleFinish`](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.c%2B%2B#L4165) separates table updates from destruction for the same reentry reason.
2. **Task admission.** `TaskSetHandle::add` boxes arbitrary `'static` futures. `ReadySet` reuses notifications but owns a growing vector and falls back to `FuturesUnordered` beyond 128 live tasks; its admission queue uses `Rc<RefCell<VecDeque<_>>>`. A bounded profile needs caller-pinned task storage, explicit capacity failure and a wake provider with appropriate lifetimes. Preserve ordinary-before-`evalLast` scheduling and wake-driven fairness when separating this policy from storage. Merely changing imports cannot remove these allocations.
3. **Hook and message lifetimes.** `Answer` retains boxed pipeline hooks, promises and vectors; `Connection::new_outgoing_message` returns a box. The bounded profile needs borrowed or arena-backed owners that survive pipeline use, cancellation, partial output and reentrant destruction. Reuse the existing engine's caller-backed tables and message leases where their semantics match, while preserving legacy's direct receive-to-dispatch path. Avoid converting every operation into a second scheduler's commands.

The existing `capntproto-rpc --no-default-features` profile exposes the bounded driver and engine while excluding hosted legacy modules. Its successful build is not evidence that legacy's full hook, task and transport path has become allocation-free. The goal is to make these existing algorithms usable with bounded ownership, one measured boundary at a time.

## Reproduction

The qualification branch contains the exact [workflow](https://github.com/DericHuynh/capntproto/blob/04951ba38fe74b8264e17098812b89bc84822f34/.github/workflows/legacy-factoring.yml) and [diagnostic helper](https://github.com/DericHuynh/capntproto/blob/04951ba38fe74b8264e17098812b89bc84822f34/benchmarks/rpc/legacy-factoring-profile.py). Source identities and job status are recorded in [provenance.json](provenance.json).

The [checksum-verified raw artifact](https://github.com/DericHuynh/capntproto/actions/runs/40134974277/artifacts/12576404686) contains every capture. Run `analyze-instructions.py ARTIFACT.zip OUTPUT_DIR` and `analyze-allocations.py ARTIFACT.zip OUTPUT_DIR` to reconstruct the traces; adding `--normalize-rpc-impls` to the latter produces the additional compiler-index-normalized view. Both parsers check their sums against the original counters.

For a link-only check from the runtime source with Rust 1.97 and the Cortex-M target:

```sh
rustc --edition=2021 --crate-type=lib --crate-name capntproto_rpc_engine --target thumbv7em-none-eabi crates/capntproto-rpc-engine/src/lib.rs -o /tmp/libcapntproto_rpc_engine_factor2.rlib
rustc --edition=2021 --target thumbv7em-none-eabi -C panic=abort -C opt-level=2 --extern capntproto_rpc_engine=/tmp/libcapntproto_rpc_engine_factor2.rlib research/reports/legacy-performance/2026-10-24-legacy-cancellation/embedded-link.rs -o /tmp/legacy-factor2-embedded.elf
python3 research/reports/legacy-performance/2026-10-24-legacy-cancellation/verify-embedded-elf.py
```

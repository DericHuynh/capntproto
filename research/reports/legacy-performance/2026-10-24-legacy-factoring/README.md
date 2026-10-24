# Incremental legacy extraction

Runtime candidate: `54f9c2e829c5274ef10c0703d402172ff25031b1`, based on `129e634`. Qualification commit: `4d288698f2ec9593a9b92ab4b4a851c59fdbd433`. The frozen runtime control is `31f7a335773a6faae60b529aa178821daacff150`; the intervening commit only records the preceding portable report.

## First boundary: direct completion

Legacy now uses `capntproto_rpc_engine::completion::direct` for the inline reply state, polling, delivery and cancellation. `QuestionCell` projects its existing `RefCell` borrow into the embedded reply. The state still lives inside the existing question allocation; the extraction adds no channel, worker, reference count or ready queue. The code uses `core` only, and accepts any `Future + Unpin`, including futures borrowing caller-owned data.

The unique producer remains authorized by legacy's existing question/fulfiller lifecycle. This low-level component does not create producer authority or replace protocol receipt accounting. Delivery returns a late or replaced value for cleanup outside the cell borrow. Polling a redirected future, invoking a waker and destroying retained values preserve reentry at the owner boundary.

The allocating shared completion used by Join and adoption remains a hosted adapter. This is one extraction, not a claim that the complete legacy driver is allocation-free.

## Verification

CI passed 225 engine tests with allocation support, 179 without it, 377 hosted RPC tests and 30 secure TLS/QUIC tests: 811 executions across overlapping profiles. Warnings-denied engine/RPC Clippy and formatting passed. Engine builds and RPC checks passed for `thumbv7em-none-eabi` with default features disabled. The focused CI also completed hosted RPC tests and warnings-denied RPC Clippy.

A separate Cortex-M link probe exercises pending registration, delivery of a borrowed future, resumed polling, completion, cancellation and late delivery. It linked without a global allocator; its ELF has no undefined or allocator entry symbols. This is a link check, not execution on a board. Probe source and identity are preserved with the evidence.

Local hosted validation was interrupted by an out-of-memory event that killed Docker. The daemon cannot be restarted by the environment's unprivileged user. Validation and profiling therefore continue in [CI run 40128516423](https://github.com/DericHuynh/capntproto/actions/runs/40128516423). No local latency results are claimed for this extraction.

## Performance acceptance

The focused job builds frozen and candidate binaries using the same compiler and release profile. It completes compilation and tests before profiling. Callgrind compares unary, dependent read, pipelined read, concurrent-32, callback and deep-32/concurrent-32. DHAT compares unary, pipeline and concurrent-32 on TLS and QUIC. Every diagnostic sums all peers and uses paired (50−10)/40 slopes with two identical warmups. These single pairs are diagnostic work counts, not repeated timing qualification. Allocation bytes and binary section sizes accompany allocation and instruction counts.

The diagnostic job completed successfully. The existing `run-latency` campaign compares current legacy, frozen legacy, portable and C++ on dedicated droplets using five fresh-process repetitions and mean/spread reporting. No prior qualification run was canceled. The next extraction should not be accepted on the strength of a `no_std` build alone: instruction, allocation and dedicated latency results must remain visible.

## Measured result

[Full counters](comparison.md) and [machine-readable values](comparison.json) retain each paired diagnostic. Instructions changed by +0.064% for unary, +0.017% for dependent read, +0.057% for pipeline, +0.022% for concurrent-32, +0.007% for callback and −0.002% for deep-32/concurrent-32. These are small observed differences, not proof of identical generated instructions or a latency improvement. Some conditional branch-miss counts increased; the complete counters are retained rather than summarized away.

A later [function-level trace](instruction-functions.json) resolves part of that difference: the self-instruction count moves unchanged from legacy `QuestionReply::poll_reference` to engine `direct::poll` in all six cases. `QuestionRef::deliver` adds three self-instructions per question (for example +3 in unary, +6 in dependent-read, and +96 in concurrent-32). This is a small real hosted-path difference, not exact instruction parity. The extraction also returns late/replaced values for destruction outside the owner borrow. Remaining deltas include cryptographic startup and compiler attribution effects; all symbol differences are retained. The [parser](analyze-instructions.py) verifies each raw self-cost total and explicitly retains Callgrind summary residuals.

The whole benchmark executable's `.text` section decreased from 9,872,167 to 9,871,399 bytes (768 bytes smaller). This is whole-binary code size, not solely the extracted function. A subsequent build of the same control runtime in the separate baseline checkout also produced the larger section, while both candidate builds retained 9,871,399 bytes. These builds do not isolate source changes from checkout/build effects, so the paired size gap cannot be attributed to this extraction alone.

TLS allocations remain exactly **7 / 58 / 202** for unary / pipeline / concurrent-32. Allocated bytes remain approximately **912 / 7,448 / 40,520** per whole operation; paired-capture differences are at most 0.3 bytes, with no added per-request record-sized increase.

The QUIC unary diagnostic is 22.050 versus 21.325 allocations per operation. [Normalized stack differences](quic-unary-allocation-stacks.json) attribute +0.700 to next-packet-key derivation and +0.025 to handshake signature work; two formatting differences cancel. No changed stack is in the extracted direct-reply path. Several slopes are negative, showing the limits of subtracting independent startup captures. Pipeline is 72.700 versus 73.050 and concurrent-32 is exactly 218.700 in both. These single pairs do not establish repeated all-QUIC allocation parity.

The dedicated run has now completed successfully. Across 24 cases per transport, the geometric mean of extracted/frozen legacy latency changed **−0.24% on TLS** and **+0.73% on QUIC**, or **+0.24% combined**. Exactly 12 of 24 means were lower on each transport. This is broadly neutral aggregate evidence, not every-case parity or a speedup claim.

The [full latency table](latency.md) reports means and the standard deviation of five fresh-process repetition means (1,000 samples per repetition). QUIC deep-8 rose 22.85% (74.25 ± 20.87 µs versus 60.44 ± 2.89 µs); TLS fanout-32 rose 18.02% (225.39 ± 77.37 µs versus 190.97 ± 2.57 µs). The untouched portable TLS fanout control also rose 22.77%. These outliers and spreads leave individual regressions unresolved; they are not removed from the aggregate. Raw repetition means and sample statistics remain in [latency-summary.json](latency-summary.json), with [provenance](latency-provenance.json).

The candidate remains a small measured extraction. A second, separately qualified cancellation extraction uses this runtime as its immediate control, so its incremental cost can be distinguished from the first extraction.

## Remaining boundaries, in order

1. Extract answer cancellation and completion guards while retaining the existing cached answer record. Keep wake registration and user destruction outside cell borrows, and verify that nesting state does not enlarge an allocation.
2. Separate protocol dispatch decisions from hosted hook ownership, retaining direct receive-to-dispatch calls. Do not route ordinary messages through the portable scheduler as part of this extraction.
3. Parameterize actual owner/message storage. Local/peer ID tables and reference accounting already have caller-backed providers. Question, answer, pipeline and retained-message lifetimes still require bounded storage and explicit capacity failure without changing protocol ordering.
4. Isolate allocating hook and transport adapters. `ClientHook`, `RequestHook` and shared promises currently expose `Box`, `Rc` and owned vectors; replacing `std` imports alone cannot make these contracts allocation-free. Borrowed or arena-backed owners must become real alternatives used by the same extracted algorithms.

The hosted driver should keep its existing fast storage and message policies. The bounded profile must supply storage and scheduling explicitly. Generic extraction is acceptable only when it preserves the measured hosted path; a second protocol ledger or extra adapter loop would undermine the purpose of this work.

## Evidence and reproduction

[Provenance](provenance.json) records the source and executable hashes and the checksum-verified [raw CI artifact](https://github.com/DericHuynh/capntproto/actions/runs/40128516423/artifacts/12573862084). [Validation](validation.json) distinguishes the two engine profiles. The [diagnostic script](https://github.com/DericHuynh/capntproto/blob/4d288698f2ec9593a9b92ab4b4a851c59fdbd433/benchmarks/rpc/legacy-factoring-profile.py) and [workflow](https://github.com/DericHuynh/capntproto/blob/4d288698f2ec9593a9b92ab4b4a851c59fdbd433/.github/workflows/legacy-factoring.yml) are committed on the qualification branch; their native timings are not used as latency evidence.

The [bare-metal source](embedded-link.rs) and [ELF check](embedded-link.json) are link-only evidence. Reproduce from this runtime with Rust 1.97 and the Cortex-M target:

```sh
CARGO_TARGET_DIR=/tmp/legacy-factor-host-cache cargo build --locked -p capntproto-rpc-engine --no-default-features --target thumbv7em-none-eabi
rustc --edition=2021 --target thumbv7em-none-eabi -C panic=abort -C opt-level=2 -L dependency=/tmp/legacy-factor-host-cache/thumbv7em-none-eabi/debug/deps --extern capntproto_rpc_engine=/tmp/legacy-factor-host-cache/thumbv7em-none-eabi/debug/libcapntproto_rpc_engine.rlib research/reports/legacy-performance/2026-10-24-legacy-factoring/embedded-link.rs -o /tmp/legacy-factor-embedded.elf
python3 research/reports/legacy-performance/2026-10-24-legacy-factoring/verify-embedded-elf.py
```

No main-branch merge was performed. This report covers direct completion only; cancellation and later ownership changes require their own measurements.

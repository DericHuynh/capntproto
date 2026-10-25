# Legacy extraction: caller-backed task readiness

Runtime `d413830`, profiler `7cbbb8f`, qualification `21c6a473b8243d74d625bcec8113d9d3903c844c`. The immediate control is `701e6a61f007dcd8ab9b3d12364d8cdbc0e615ec`, containing the measured borrowed-waker improvement. This extraction is on `codex/legacy-readiness-core`; main has not been updated.

**Retained:** readiness and combined scheduler instruction/indirect-branch counts match exactly in all five repetitions of all six cases. All 40 retained functions have identical normalized instruction structure and addresses. TLS allocation block counts match. Aggregate instructions and QUIC allocations have small differences retained below, including a reproducible deep-concurrent libc copy-loop difference. Native latency is unqualified.

## Extracted boundary

`capntproto_rpc_engine::readiness` now owns legacy's bounded atomic FIFO notification algorithm. `Storage<N>` provides inline caller-owned storage; `Queue<'a, N>` borrows either that storage or an existing owner's atomic fields. The implementation uses `core` only, requires no `alloc` feature, and is covered by the engine's `forbid(unsafe_code)` policy.

The hosted `Ready` keeps exactly the same field declarations and types: producer cursor, entry array, queued flags and `AtomicWaker`. Its methods project borrowed views into those fields. They do not retain another queue, owner, wake registration or task. `ReadySet` keeps its existing task vector, reusable wake nodes, consumer cursor, free-slot mask and `FuturesUnordered` overflow adapter. The 128-slot hosted limit and ordinary/deferred scheduling behavior are unchanged.

Publication follows the original operations and memory orderings:

1. The queued flag uses an acquire/release swap even for duplicate notifications. This publishes producer data to the consumer's acquire/release clearing operation.
2. A newly queued slot reserves a FIFO position with a relaxed fetch-add, then publishes its index through the entry's acquire/release swap.
3. The caller's notification callback runs after publication. An unpublished front reservation cannot be overtaken by later notifications.
4. The single consumer clears the entry before releasing the slot's queued flag, then advances its cursor with wrapping arithmetic before polling the task.

The callback is generic and inlined; hosted code still invokes its original parent waker directly. Consumers must register or arm their notification mechanism before checking the queue, as hosted `ReadySet::poll_next` already does. `front()` returning `None` may mean that the earliest producer is still publishing, not that no later reservation exists.

Each slot has at most one outstanding reservation, bounding the ring without a separate full-queue allocation or retry list. Capacity is a power of two from 1 through 128: this both fits the byte entry encoding and preserves ring indexing when the pointer-sized cursor wraps. Invalid capacities and enqueued slot indices are rejected before publication. Task admission capacity is a separate responsibility.

## Verification

Six new core tests cover allocation count **and bytes**, FIFO order and reuse at capacities 1/8/128, invalid inputs, a full ring behind a suspended producer across integer wrap, concurrent producers/consumer reuse, duplicate-wake data publication, and reentrant consumption/publication. Existing hosted tests continue to exercise wake replacement, stale notifications, task completion, promotion, teardown and deferred-write ordering through the extracted algorithm.

The [standalone probe](../../../../verification/portable/legacy-readiness-link.rs) drives four futures borrowing caller-owned state and queue storage. Each suspends, republishes its own slot, completes on its next poll and releases its notification. It uses no box, shared owner, hosted executor or global allocator. The [ELF verifier](../../../../verification/portable/verify-legacy-readiness-link.py) checks the entry point and rejects undefined or allocator symbols. This is link-only evidence, not execution on a board or proof that the complete hosted driver is allocation-free.

The module is available on targets with native pointer-width and byte atomics. CI builds/links it on Cortex-M4 (`thumbv7em-none-eabi`) and checks the engine on Cortex-M0 (`thumbv6m-none-eabi`), where this atomic module is gated out. The existing RPC no-default-feature check remains separate from hosted legacy.

Twenty local Python analysis tests, formatting and whitespace checks passed. [Qualification](https://github.com/DericHuynh/capntproto/actions/runs/40149781507) completed successfully: **842 Rust test executions across overlapping profiles**, comprising 240 engine alloc, 194 engine no-alloc, 378 hosted RPC and 30 encrypted TLS/QUIC tests. Four Python profiler tests, warnings-denied Clippy, formatting, Cortex-M4 engine build/RPC check, Cortex-M0 engine check and the standalone no-allocator link probe passed.

The [linked ELF evidence](embedded-link.json) records a 706,756-byte debug ELF with no undefined or allocator symbols. Its size includes debug information; it is not a firmware text-size result.

## Performance qualification

The comparison builds the control and candidate sequentially at the same source path with the same compiler, Cargo target directory and release profile. It measures all six existing TLS instruction/branch cases and unary/pipeline/concurrent allocation cases on TLS and QUIC, five times each. All peers are counted, using `(50−10)/40` with two warmups, rotating variant order and alternating short/long capture order. No cryptographic randomness is replaced.

The profiler now retains disassembly for `Ready`, `ReadySet`, `Notify` and the extracted core in addition to the existing scheduler/Return/TLS paths. The assessment must include the combined old/new function costs, binary sections, allocations and bytes. A changed function name or moved code cannot be counted as removed work. Simulated misses remain layout-sensitive, as established by the [preceding calibration](../2026-10-24-legacy-idle/README.md).

All **240 trials / 480 peer captures** completed without failures. [Complete mean/spread table](comparison.md), [individual samples](comparison.json), [selected self-event attribution](functions-selected.json), and [derived work/assembly analysis](analysis.json) are retained. Every function-attribution total reconciles with its aggregate event delta.

| TLS case | Readiness Ir delta | Readiness Bi delta | Scheduler + readiness Ir delta | Scheduler + readiness Bi delta | Total Ir delta/op, mean ± paired SD |
| --- | ---: | ---: | ---: | ---: | ---: |
| Unary-small | 0 | 0 | 0 | 0 | +0.430 ± 86.349 |
| Dependent-read | 0 | 0 | 0 | 0 | +32.595 ± 43.799 |
| Pipelined-read | 0 | 0 | 0 | 0 | +12.480 ± 45.292 |
| Concurrent-32 | 0 | 0 | 0 | 0 | +14.080 ± 61.140 |
| Callback | 0 | 0 | 0 | 0 | −46.380 ± 59.392 |
| Deep-32 / concurrent-32 | 0 | 0 | 0 | 0 | +244.490 ± 76.896 |

All four component columns have zero sample SD. The combined group includes `TaskSet`, `Activity`, `Ready`, `ReadySet`, `Notify`, and any outlined extracted readiness functions. It therefore retains costs that could move between those functions during inlining.

All 40 selected common functions preserve their instruction offsets/forms after linked addresses and RIP-relative displacements are normalized; their entry addresses and indirect-call site counts also match. This includes the readiness/wake implementation and the existing scheduler, Return and TLS paths. It is not a byte-equivalence claim: some read-only data addresses changed, and the two ELF hashes differ. Both binaries have **9,871,655 bytes of `.text` and 828,005 bytes of `.rodata`**. The borrowed view adds no measured execution work on these paths, and the preceding waker code-size saving is preserved.

The [symbol comparison](symbol-analysis.json) additionally finds identical names, addresses and sizes for all **13,034 text-symbol rows**. Selected read-only constants move by 112 bytes, including `NULL_ARENA` and the wire schema's encoded capability descriptor. These data-placement changes remain relevant to alignment-sensitive library routines even with stable code addresses.

Total instruction mean changes range from −0.045% to +0.028%. Conditional/indirect misses remain in the full table: the largest observed changes are +0.311% Bcm in pipeline and −0.015% Bim in pipeline. These aggregate changes do not overturn the exact component counts, but are not rounded to whole-operation parity.

## Deep-concurrent copy trace

The deep-concurrent aggregate difference is not attributed to generic startup noise. Libc `__memcpy_avx_unaligned_erms` contributes **+258.880 ± 9.599** instructions per operation across the five paired captures, offset by changes elsewhere. Its five deltas are 249.250, 255.000, 255.000, 274.450 and 260.700.

A [call-arc and assembly-line analysis](memcpy-analysis.json) of the first repetition's eight peer captures finds identical copy call counts and identical counts at every caller: **1,433 client and 2,580 service calls per operation** in both builds. The client contributes +255 self instructions, with +256 simulated iterations at assembly source line 660. Service-side copy paths contribute −5.750, giving the independently reconciled first-repetition +249.250 total.

The captured assembly file is `memmove-vec-unaligned-erms.S`. The [upstream glibc 2.39 source](https://github.com/bminor/glibc/blob/glibc-2.39/sysdeps/x86_64/multiarch/memmove-vec-unaligned-erms.S#L660) identifies line 660 as `rep movsb` after aligning the destination to 64 bytes; line 590 is its source-aligned counterpart. Callgrind counts repeated iterations here. [Reference provenance](memcpy-source.json) records the source hash and observed Ubuntu glibc package. This localizes the difference to libc's alignment-adjusted copy paths, with no added calls. The captures do not record pointer addresses or requested copy lengths, so they cannot prove which heap/read-only-data alignment or length differences explain every iteration. Native retired-instruction and latency effects are not established by this simulated counter.

## Allocations

TLS block counts remain **7 / 58 / 202** for unary / pipeline / concurrent-32 in every sample. Candidate-minus-control byte deltas are **−0.040 ± 0.114 / −0.100 ± 0.100 / 0.000 ± 0.100** per operation (paired mean ± sample SD).

QUIC block deltas are **−0.060 ± 0.290 / +0.300 ± 0.300 / +0.070 ± 0.461**, with byte deltas **−6.810 ± 28.984 / +28.460 ± 29.198 / +6.840 ± 45.373**. These are small observed differences with nonzero spread; exact QUIC allocation/byte parity is not established. The core unit test measures zero allocation count and bytes for queue construction, publication, consumption and reuse at capacities 1, 8 and 128.

## Evidence

The [80,790,192-byte artifact](https://github.com/DericHuynh/capntproto/actions/runs/40149781507/artifacts/12582602916) was streamed and SHA-256 verified as `6bb3fbfd9df93f0f92aa2aebadd561806eed264a501605c1951cd907d5e73f37`. It retains raw profiles, complete function attribution, symbol tables, disassembly and embedded-link evidence. [Provenance](provenance.json) records source/executable identities and validation. The control executable hash matches the previous waker candidate exactly.

This run establishes preserved work in the extracted readiness/scheduler paths and equal TLS allocation counts. It is not a native droplet latency campaign and does not close the earlier per-case latency gaps.

## Remaining no-alloc boundaries

This extracts notification transitions and supplies real caller-owned notification storage. The hosted legacy driver still allocates:

- Task admission boxes heterogeneous futures and uses a growing `VecDeque`. A bounded adapter needs stable pinned task storage and a capacity-failure result that returns ownership of rejected work before any protocol side effect.
- Slot notifications and parent activities have shared wake owners. Standard Rust `Waker` values can escape a task and outlive its task set. A stack-backed raw waker would be unsound without another lifetime guarantee. The bounded profile needs caller/static wake storage or scoped notifications whose lifetime cannot escape; the hosted adapter must retain its current teardown guarantees.
- Hook, pipeline and retained-message owners still use boxes, reference counts and vectors. Their bounded alternatives must preserve cancellation, pipelining, partial output, retained results and reentrant destruction.

The next extraction should parameterize task storage/admission while using this same readiness algorithm. Keep direct receive-to-dispatch calls and ordinary-work-before-deferred-publication ordering. Queue extraction alone does not make the entire driver no-std/no-alloc, and the no-default RPC configuration still selects the existing bounded path rather than hosted legacy.

## Reproduction

```sh
python3 benchmarks/rpc/legacy-work-profile.py --pair-only --factored CONTROL_BINARY --candidate CANDIDATE_BINARY --output OUTPUT
python3 benchmarks/rpc/legacy-work-profile.py --summarize-only --output EXTRACTED_CAPTURE
```

The [qualification workflow](https://github.com/DericHuynh/capntproto/blob/21c6a473b8243d74d625bcec8113d9d3903c844c/.github/workflows/legacy-readiness.yml) retains the exact build, test, embedded-link and capture commands.

Copy this report's `functions-selected.json` into the extracted artifact directory and run `analyze.py EXTRACTED_CAPTURE` to reconstruct the additional work/assembly analysis. Full per-function values can be checked against the artifact's `functions.json`. Run `analyze-copies.py EXTRACTED_CAPTURE OUTPUT_JSON` with `functions-selected.json` beside the output to reproduce the first-repetition copy call/line analysis; it checks its total against the independent function attribution.

# Legacy extraction: caller-backed task readiness

Runtime `d413830`, profiler `7cbbb8f`, qualification `21c6a473b8243d74d625bcec8113d9d3903c844c`. The immediate control is `701e6a61f007dcd8ab9b3d12364d8cdbc0e615ec`, containing the measured borrowed-waker improvement. This extraction is on `codex/legacy-readiness-core`; main has not been updated.

## Extracted boundary

`capntproto_rpc_engine::readiness` now owns legacy's bounded atomic FIFO notification algorithm. `Storage<N>` provides inline caller-owned storage; `Queue<'a, N>` borrows either that storage or an existing owner's atomic fields. The implementation uses `core` only, requires no `alloc` feature, and is covered by the engine's `forbid(unsafe_code)` policy.

The hosted `Ready` keeps exactly the same field declarations and types: producer cursor, entry array, queued flags and `AtomicWaker`. Its methods project borrowed views into those fields. They do not retain another queue, owner, wake registration or task. `ReadySet` keeps its existing task vector, reusable wake nodes, consumer cursor, free-slot mask and `FuturesUnordered` overflow adapter. The 128-slot hosted limit and ordinary/deferred scheduling behavior are unchanged.

Publication follows the original operations and memory orderings:

1. The queued flag uses an acquire/release swap even for duplicate notifications. This publishes producer data to the consumer's acquire/release clearing operation.
2. A newly queued slot reserves a FIFO position with a relaxed fetch-add, then publishes its index through the entry's acquire/release swap.
3. The caller's notification callback runs after publication. An unpublished front reservation cannot be overtaken by later notifications.
4. The single consumer clears the entry before releasing the slot's queued flag, then advances its cursor with wrapping arithmetic before polling the task.

The callback is generic and inlined; hosted code still invokes its original parent waker directly. Consumers must register or arm their notification mechanism before checking the queue, as hosted `ReadySet::poll_next` already does. `front()` returning `None` may mean that the earliest producer is still publishing, not that no later reservation exists.

Each slot has at most one outstanding reservation, bounding the ring without a separate full-queue allocation or retry list. Capacity is a power of two from 1 through 128: this both fits the byte entry encoding and preserves ring indexing when the pointer-sized cursor wraps. Invalid capacities and slot indices are rejected before publication. Task admission capacity is a separate responsibility.

## Verification

Six new core tests cover allocation count **and bytes**, FIFO order and reuse at capacities 1/8/128, invalid inputs, a full ring behind a suspended producer across integer wrap, concurrent producers/consumer reuse, duplicate-wake data publication, and reentrant consumption/publication. Existing hosted tests continue to exercise wake replacement, stale notifications, task completion, promotion, teardown and deferred-write ordering through the extracted algorithm.

The [standalone probe](../../../../verification/portable/legacy-readiness-link.rs) drives four futures borrowing caller-owned state and queue storage. Each suspends, republishes its own slot, completes on its next poll and releases its notification. It uses no box, shared owner, hosted executor or global allocator. The [ELF verifier](../../../../verification/portable/verify-legacy-readiness-link.py) checks the entry point and rejects undefined or allocator symbols. This is link-only evidence, not execution on a board or proof that the complete hosted driver is allocation-free.

The module is available on targets with native pointer-width and byte atomics. CI builds/links it on Cortex-M4 (`thumbv7em-none-eabi`) and checks the engine on Cortex-M0 (`thumbv6m-none-eabi`), where this atomic module is gated out. The existing RPC no-default-feature check remains separate from hosted legacy.

Twenty local Python analysis tests, formatting and whitespace checks passed. [Qualification](https://github.com/DericHuynh/capntproto/actions/runs/40149781507) has passed its core/hosted, Clippy, formatting and embedded validation step; release builds and profiles are pending. Final counts and artifacts will be recorded after completion.

## Performance qualification

The comparison builds the control and candidate sequentially at the same source path with the same compiler, Cargo target directory and release profile. It measures all six existing TLS instruction/branch cases and unary/pipeline/concurrent allocation cases on TLS and QUIC, five times each. All peers are counted, using `(50−10)/40` with two warmups, rotating variant order and alternating short/long capture order. No cryptographic randomness is replaced.

The profiler now retains disassembly for `Ready`, `ReadySet`, `Notify` and the extracted core in addition to the existing scheduler/Return/TLS paths. The assessment must include the combined old/new function costs, binary sections, allocations and bytes. A changed function name or moved code cannot be counted as removed work. Simulated misses remain layout-sensitive, as established by the [preceding calibration](../2026-10-24-legacy-idle/README.md).

Results are pending. No instruction, allocation or latency parity is claimed yet. Native droplet latency is not measured by this diagnostic job.

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

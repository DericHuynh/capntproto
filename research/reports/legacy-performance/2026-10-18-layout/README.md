# RPC layout and scheduling experiments, October 18, 2026

Tracking: [gap #15](https://github.com/DericHuynh/capntproto/issues/15), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). This continues the [deep/concurrent report](../2026-10-17-deep/README.md).

**The requested target is not met.** The selected full campaigns observe **3/24** pooled-p50 passes at Rust/C++ ≤ 0.70. Lightweight read/write counts and attempts pass **64/64 process rows**; full tracing passes **63/64**. Earlier diagnostic misses remain retained. These are measured captures, not a universal syscall-count guarantee. The separate frozen-legacy 2× gate remains open and was not rerun.

## Retained changes

- Inline the common allocation helper used during deep copies so its success path can be optimized with its caller. Pointer validation, traversal budgets, nesting, capability handling and allocation policy are unchanged.
- Move error descriptions and exception metadata into optional cold storage. On this x86-64 build, `Error` shrinks from 56 to 32 bytes, reducing successful `Result` and future storage. Plain errors, empty descriptions, cloning plain errors and diagnostic reads remain allocation-free. Nonempty error descriptions gain one context allocation; this is a layout improvement, not a claim of fewer allocation calls.
- Preserve ordinary `error.extra` reads/edits through `Deref`; the first mutable diagnostic access allocates context. Struct literals and direct metadata-field access are intentionally replaced by constructors/accessors; use `Error::with_description` and `into_extra()` for owned descriptions. [Migration notes](../../../../crates/capntproto-core/REPROTO.md) describe the unreleased API change.
- Extend the topology runner with an interleaved frozen Rust control, balanced three-variant order, and CPU/quota/steal counters around each process lifecycle. Counters include setup/teardown and never filter samples. Fixtures, validation, warmups, deadlines and C++ controls are unchanged.

## Selected measurements

Six repetitions use 100 warmups/1,000 samples for workflows and topology batches, and 10,000/1,000 for loopback. Candidate, frozen Rust `6ff8b25` and C++ each occupy every order position twice. The selected campaigns retain **432,000 uninstrumented samples** and **124 mixed-peer checks**. No tests, builds or profilers overlap these selected latency campaigns. Every repetition, p95/p99 and throughput remains in [report.json](report.json) and the raw archive.

Multi-service p50 improves about 3–4% against the preceding Rust control; depth-32 cases are slightly slower. Small-message results still switch between timing modes in unchanged controls. Failed cells are not replaced by favorable results from the later experimental campaigns.

| Case | Previous Rust µs | Selected Rust µs | C++ µs | Rust/C++ | Meets 0.70 |
| --- | ---: | ---: | ---: | ---: | --- |
| unary-empty | 32.058 | 31.818 | 32.559 | 0.977 | no |
| unary-small | 32.198 | 32.038 | 32.569 | 0.984 | no |
| unary-1k | 32.839 | 33.009 | 33.300 | 0.991 | no |
| fanout-32 | 135.423 | 136.184 | 173.700 | 0.784 | no |
| dependent-read | 68.412 | 68.312 | 68.933 | 0.991 | no |
| pipelined-read | 40.501 | 41.002 | 40.050 | 1.024 | no |
| write-read | 43.595 | 63.675 | 64.497 | 0.987 | no |
| callback | 65.848 | 66.870 | 66.260 | 1.009 | no |
| bulk-64k | 62.523 | 51.758 | 60.430 | 0.856 | no |
| bulk-256k | 93.670 | 111.296 | 265.768 | 0.419 | yes |
| bulk-1m | 348.060 | 367.891 | 925.074 | 0.398 | yes |
| bulk-4m | 2636.955 | 2433.105 | 4588.980 | 0.530 | yes |
| loopback-0 | 31.517 | 21.533 | 21.812 | 0.987 | no |
| loopback-64 | 31.707 | 32.128 | 31.888 | 1.008 | no |
| loopback-1024 | 21.993 | 32.208 | 22.504 | 1.431 | no |
| loopback-65536 | 95.623 | 100.861 | 63.304 | 1.593 | no |
| deep-8 | 22.674 | 22.574 | 23.205 | 0.973 | no |
| deep-32 | 27.061 | 27.783 | 27.822 | 0.999 | no |
| concurrent-32 | 102.935 | 100.862 | 141.413 | 0.713 | no |
| deep-32-concurrent-32 | 229.396 | 234.788 | 271.609 | 0.864 | no |
| services-4-deep-16 | 155.795 | 150.462 | 142.825 | 1.053 | no |
| services-4-deep-16-concurrent-32 | 1169.531 | 1124.196 | 1162.260 | 0.967 | no |
| services-4-deep-16-clients-8 | 1185.148 | 1141.197 | 1089.138 | 1.048 | no |
| services-8-deep-16-clients-8 | 2552.069 | 2470.093 | 2369.525 | 1.042 | no |

For example, selected 1-KiB loopback repetition medians are 21.903, 33.480, 33.050, 33.069, 21.913 and 22.143 µs; C++ gives 33.570, 22.133, 22.293, 33.160, 21.943 and 22.104 µs. Host counters now retain more context, but these timing-mode changes are not attributed to a proven cause.

## Profiles and I/O

Whole-process Callgrind for four services and eight clients uses two warmups and ten batches, including startup and teardown. The selected binary is compared directly with the preceding Rust binary and fixed C++ control. Instruction counts fall in every process; they do not establish the same wall-clock reduction.

| Process | Previous instructions | Selected | C++ | Reduction |
| --- | ---: | ---: | ---: | ---: |
| client.callgrind | 13,076,677 | 12,777,707 | 15,984,405 | 2.29% |
| service-0.callgrind | 29,960,647 | 28,720,445 | 30,831,207 | 4.14% |
| service-1.callgrind | 28,329,422 | 26,965,591 | 29,517,265 | 4.81% |
| service-2.callgrind | 28,329,026 | 26,966,025 | 29,524,200 | 4.81% |
| service-3.callgrind | 13,645,643 | 12,992,943 | 16,856,564 | 4.78% |

The cold-context tradeoff is visible: whole-process malloc counts rise from 6,179 to 6,235 in the client and 16,543 to 16,632 in the first service. Full allocator counts remain in the machine-readable profile. The retained speed work reduces instructions and value size, rather than eliminating these allocations.

The lightweight probe records syscall wall/thread-CPU times and read-to-write CPU intervals. For unary server handling, the median read-to-write CPU interval is 2.203 µs versus C++ 3.014 µs; write CPU medians are 6.099 versus 6.340 µs. Rust still makes 225 reactor polls versus 114. Fanout server read-to-write CPU is 33.680 versus 52.939 µs. These instrumented intervals include probe overhead and are diagnostic only. They locate remaining work in scheduling/reactor transitions and transport delivery, rather than proving a native timing win.

The full sweep includes startup, warmup, EOF/EAGAIN, cancellation and teardown. All writes pass in both instruments. The remaining full-trace miss is the bulk-4m client: **40 positive reads / 53 read attempts versus C++ 30 / 43**, with 13 writes each. All 48 original peer pairs and 228 directed topology edges in the selected three-variant captures reassemble; every received frame matches its sender byte-for-byte. Unread shutdown Aborts/Finish/Release remain explicit. Prior captures also show bulk-256k fragmentation and variable teardown writes; a favorable later capture does not erase them.

## Rejected experiments

- Extra ordinary-task drain turns increased bulk-256k reads/writes.
- Immediate local wakes added an 80-byte cleanup write at teardown in the unary trace. The earlier startup diagnosis was corrected by inspecting complete wire events.
- Waking through a spawned runtime task preserved the screened full-wire counts but added allocation/scheduling work without a convincing latency gain.
- Raising the exact-body receive hint from 1 to 4 MiB passed the full-wire sweep, but still missed bulk-256k lightweight reads (306 versus 225). It made 4-MiB p50 consistently slower across all six repetitions: pooled 2.767 ms versus 2.484 ms for the allocation-only candidate, while allowing Linux receive capacity to grow to 8 MiB. It is reverted.

All four binaries, source patches and raw diagnostics are retained. The failed `dependent-call` trial (a harness typo), the exploratory run using the topology C++ binary for original fixtures, and a short formatter overlap in the inline-only exploratory topology campaign are retained and excluded from selected qualification.

## Dedicated-host context

The preceding commit `6ff8b25` completed [dedicated run 39078456460](https://github.com/DericHuynh/capntproto/actions/runs/39078456460), including verified resource cleanup. Its two-host VPC workflows have **0/12** passes at the requested 0.70 ratio. Unary-empty is 128.128 µs versus C++ 131.617 µs; fanout is 433.806 versus 494.045 µs; 4 MiB is 19.201 ms versus 18.423 ms. Passing that lane’s existing loopback 1.15× gate does not qualify the stricter goal. Downloaded artifact SHA-256 values are verified. These measurements precede this change and are not mixed into local results.

## Correctness and provenance

- `core`: passed, 138 tests.
- `rpc-framing`: passed, 388 tests.
- `codegen`: passed, 2 tests.
- `allocation-exception-interop`: passed, 81 tests.
- `core-no-alloc`: passed.
- `core-alloc-only`: passed.
- `miri-error`: passed, 1 tests.
- `miri-move`: passed, 8 tests.
- `format`: passed.
- `unsafe`: passed.
- `benchmark-python`: passed.
- `root-serial`: passed, 287 tests.
- `core-docs`: passed, 33 tests.

Initial failures are preserved: a missing TLA2TOOLS_JAR environment was corrected before rerunning the integration matrix; the parallel root run hit a storage-lock and descriptor-reuse test interference, both passing individually. The first serial root run passed those tests but hit a self-relaunch harness failure because a concurrent targeted rebuild replaced its executable. Its isolated SIGPIPE test passes. The final full root run uses one test case at a time and no overlapping builds; internal concurrency assertions are unchanged. Full hosted coverage is not claimed.

No unsafe operations or exceptions are added. The unsafe inventory updates only the reviewed core/layout source hashes, with unchanged exception counts and coverage thresholds. Focused Miri exercises compact error ownership and RPC result movement. The no-alloc and alloc-only core configurations, allocation contracts, generated code, exception metadata and upstream interoperability are checked.

Selected binary SHA-256: `18617746ceb65a0b0afeff0a3dc62478594be371a366aff59bf7ba96ecfbe016`. Compiled snapshot: `68c9ef49dba05438066654beef4067b76c42eaac`; review snapshot: `48c7eae7a3a066306fca90a6ed6f9ae6b98aa73b`. The [patch](source.patch) replays against `6ff8b25cb5dbcb8ea8d8efa95df70f8d78951ff1`. Changes after compilation are benchmark Python/README, migration documentation, allocation tests, the test-only policy constructor migration, and the unsafe inventory; production Rust is unchanged. Independent verification replays all seven snapshots, checks fixed binary/fixture identities and recomputes percentiles, throughput and balanced order from raw samples.

Raw archive: `/workspace/capntproto/target/legacy-layout-20261018`, **13,086 files / 1,364,576,787 bytes**. `SHA256SUMS` SHA-256: `4043fc4b6f090b8cc8c062c79477d402758e64014f61be3126c96547d28a9401`. The three preceding legacy archives verify unchanged.

The remaining optimization work is executor turn ordering without extra cleanup writes, and fragmented large-body readiness without excessive buffering or a latency regression. Keep gap #15 open and PR #12 in draft.

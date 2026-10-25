# Borrowed legacy scheduler wakers: measured work reduction

The change removes **58–132 executed instructions and 28–64 indirect branches per benchmark operation** from the scheduler/waker paths, identically in every repetition. Total instruction means are lower in all six measured TLS cases. TLS allocation counts remain equal; small byte and QUIC differences remain visible below. Simulated branch misses are mixed, including two substantial indirect-miss regressions versus factored legacy. Native latency is not qualified by this run.

Retained on `codex/legacy-idle-task`. Runtime `43bab8a`; test-only follow-up `b87e0c8`; qualification `24cff2308de03fbb04446b8eb188a7e7d76b01ac`. Control `d984b8ec7e6ca873481eb899640896651523324f`. This follows the [rejected sentinel experiment and repeated control calibration](../2026-10-24-legacy-idle/README.md). The sentinel removal remains reverted. Main has not been updated by this work.

## Change and attribution

`TaskSet` previously stored two owned `Waker` values and cloned/dropped temporary copies through their vtables on every regular/deferred poll. It now borrows temporary wakers from concrete `Arc<Activity>` owners using `futures::task::waker_ref`. The concrete owner is retained while mutable task state is polled. A future can still clone an owned registration, including one that outlives the task set and wakes from another thread. Temporary owner clones still perform reference-count operations; the change removes virtual dispatch, not all reference counting.

The two cached waker fields are removed. Admission, the idle sentinel, drain checks, bounded deferral and ordinary-before-deferred scheduling are preserved. The new test verifies wake delivery, parent release on teardown, and cross-thread use of retained connection/deferred registrations after destruction.

[Function attribution](functions-selected.json) reconciles with every total event delta. The [derived analysis](analysis.json) combines `TaskSet` and its concrete `Activity` helpers, including old/new helper names, so moving work between them cannot masquerade as a saving. For unary, `TaskSet::poll` self work increases by 54 instructions as reference counting becomes inline, while clone/drop helpers decrease by 112; wake helper costs are equal. The net saving is 58.

| TLS case | Scheduler/waker Ir delta/op | Scheduler/waker Bi delta/op | Total Ir delta/op, mean ± paired SD | Total Ir change |
| --- | ---: | ---: | ---: | ---: |
| Unary-small | −58 | −28 | −75.105 ± 45.227 | −0.163% |
| Dependent-read | −116 | −56 | −151.530 ± 45.448 | −0.128% |
| Pipelined-read | −58 | −28 | −57.290 ± 48.552 | −0.059% |
| Concurrent-32 | −66 | −32 | −43.920 ± 39.785 | −0.007% |
| Callback | −132 | −64 | −111.970 ± 60.379 | −0.107% |
| Deep-32 / concurrent-32 | −66 | −32 | −144.565 ± 92.594 | −0.004% |

The two scheduler columns have zero sample SD across five repetitions. Total instruction deltas retain setup variation from independent processes; lower total means are not a claim of statistically established latency gains in every case.

The full binary `.text` shrinks by 512 bytes (9,872,167 → 9,871,655); `.rodata` remains 828,005 bytes. `TaskSet<Error>::poll` itself grows from 875 to 887 static instructions as work is inlined, while its indirect call sites fall from 36 to 29. The savings are in executed work and overall text size, not a smaller static poll body.

## Simulated misses and the frozen reference

| TLS case | Bcm versus factored | Bim versus factored | Bcm versus frozen reference | Bim versus frozen reference |
| --- | ---: | ---: | ---: | ---: |
| Unary-small | +3.452% | −6.942% | +3.855% | +4.841% |
| Dependent-read | −4.479% | −8.573% | −1.151% | +1.999% |
| Pipelined-read | −5.582% | −7.658% | −2.040% | −2.098% |
| Concurrent-32 | −5.903% | **+60.648%** | −11.001% | +0.730% |
| Callback | −3.305% | −9.196% | +0.633% | +1.335% |
| Deep-32 / concurrent-32 | −28.451% | **+9.289%** | −26.972% | +0.659% |

Factored comparisons are paired within the new run. Frozen reference values come from the separate five-repeat calibration run, not new paired frozen/candidate trials. The factored executable hash matches exactly across runs. Individual frozen/candidate samples and spreads are preserved in `analysis.json`; no paired cross-run spread is fabricated. The observed miss means are now no more than 4.841% above that frozen reference, but the two regressions versus factored legacy remain real simulated-counter changes.

For concurrent-32, the largest indirect-miss increases are +64 in `StructReaderCache::select` and +61 in `OwnedIncomingStream::poll_receive`. They are outside the edited scheduler. Selected disassembly also retains identical normalized instruction forms in TLS buffered frame preparation (894 instructions), AES-GCM seal (235), and answer-return checks (176). TLS/AES code moves by −496 bytes, shifting the simulator's low-nine-bit predictor index by +16; answer checks shift by −64. This corroborates layout sensitivity, without proving every miss change harmless on hardware. The earlier identical-binary control showed that these large shifts are not ordinary process-to-process randomness.

## Allocations

DHAT slopes sum both peers. Values below are candidate minus factored, mean ± sample SD of paired differences. Full before/after means and spreads are in the [complete comparison](comparison.md) and [samples](comparison.json).

| Transport | Case | Block delta/op | Byte delta/op |
| --- | --- | ---: | ---: |
| TLS | Unary-small | 0 ± 0 | −0.040 ± 0.167 |
| TLS | Pipelined-read | 0 ± 0 | +0.080 ± 0.192 |
| TLS | Concurrent-32 | 0 ± 0 | +0.020 ± 0.192 |
| QUIC | Unary-small | −0.070 ± 0.293 | −6.960 ± 28.941 |
| QUIC | Pipelined-read | +0.005 ± 0.357 | +0.215 ± 35.126 |
| QUIC | Concurrent-32 | +0.135 ± 0.477 | +13.745 ± 46.923 |

TLS block counts are exactly 7 / 58 / 202 in every sample. The remaining byte/QUIC differences are smaller than their measured spread; this is not exact byte parity or proof of a QUIC allocation reduction.

## Qualification and reproduction

[Run 40146484719](https://github.com/DericHuynh/capntproto/actions/runs/40146484719) passed all 240 trials / 480 peer captures. Both revisions were built sequentially at the same path, target directory, compiler and release profile. Each fixture has five fresh-process pairs, alternating 10/50 operation order and rotating variant order, with two warmups. All peers are counted using `(50−10)/40`. These are Callgrind/DHAT diagnostics on a CI runner, not native droplet latency measurements. Cryptographic randomness is unmodified.

Validation passed: 830 Rust test executions across overlapping profiles (234 engine alloc, 188 engine no-alloc, 378 hosted RPC, 30 encrypted TLS/QUIC), warnings-denied Clippy, formatting, Cortex-M engine/RPC checks, and a standalone no-allocator link probe. Twenty local Python analysis tests passed; four repeated-profiler tests ran in CI. The first attempt passed its 800 core/hosted test executions but failed Clippy's `manual_noop_waker` lint in the new test. The test-only follow-up uses a notification counter and adds a wake-delivery assertion; the successful retry contains the same runtime change.

The [80,858,884-byte artifact](https://github.com/DericHuynh/capntproto/actions/runs/40146484719/artifacts/12581074802) was streamed and verified as SHA-256 `b6b2192f2af52a013b408a39bae33544d2fb290f3f9eca39f53e9a65943079c7`. It retains complete raw profiles, all function attribution, symbol tables and selected disassembly. [Provenance](provenance.json) includes binary hashes and section sizes; the baseline hash matches the preceding calibration.

```sh
python3 benchmarks/rpc/legacy-work-profile.py --pair-only --factored FACTORED_BINARY --candidate CANDIDATE_BINARY --output OUTPUT
python3 benchmarks/rpc/legacy-work-profile.py --summarize-only --output EXTRACTED_CAPTURE
```

To reproduce the additional scheduler/layout/reference analysis, copy this report's `functions-selected.json` into the extracted new capture directory, then run `analyze.py NEW_CAPTURE CALIBRATION_CAPTURE`. Full per-function values can also be checked against `functions.json` retained in the raw artifact.

Native latency remains open. The [preceding Return-only droplet report](../2026-10-24-legacy-return/latency.md) showed +0.90% TLS / +2.36% QUIC geometric-mean latency with substantial spread and unresolved per-case regressions. It does not contain this waker change. This qualification establishes reduced scheduler work, not a wall-clock speedup or completion of the broader performance target.

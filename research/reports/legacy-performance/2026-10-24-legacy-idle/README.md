# Legacy branch-counter investigation and rejected idle scheduler experiment

The original [Return-ownership comparison](../2026-10-24-legacy-return/README.md) contains single paired captures. It shows unchanged TLS allocation counts and selected Return-path instruction counts, but mixed simulated branch-miss changes. Those miss percentages are not native latency percentages.

The five-repeat comparison is complete. The original indirect-miss pattern is reproducible, with almost no change in the identical-binary control. The original small instruction increases do not persist. Removing the scheduler's idle sentinel increased its executed instructions; that experiment was rejected and reverted. See the [complete repeated table](comparison.md), [machine-readable samples](comparison.json), and [provenance](provenance.json).

Rejected runtime: `1b4b94bee09874e7c688616a8771983af217eaf6`, reverted in `72c57df`. Profiler: `0651e3c`. Qualification: `6a86d30713a4393d19bdea7586e5224ff71d8363`. Frozen control: `a5677a63bb13273dff614205367f944a00145acf`. Factored control: `d984b8ec7e6ca873481eb899640896651523324f`. Main is unchanged.

## What the original counters show

[Full self-event attribution](original-functions.json) reconciles every event with the original table, including omitted trailing zero counters and summary-minus-attributed-total residuals. Call-edge inclusive costs are excluded to prevent double counting.

The unary instruction delta is +59.950 per operation; elliptic-curve routines account for +57.925. Dependent read is +101.750 overall, with +103.100 in those routines offset by reductions elsewhere. These workloads reuse their authenticated connections. Handshake crypto remains in the difference between independent 50- and 10-operation processes, so it is residual setup work, not 58 new instructions added to the unary RPC dispatch path.

The total indirect branch count is essentially unchanged:

| Case | Frozen Bi | Factored Bi | Difference |
| --- | ---: | ---: | ---: |
| Unary | 568.975 | 569.025 | +0.050 |
| Dependent read | 1,262.025 | 1,262.000 | −0.025 |
| Pipeline | 944.000 | 944.000 | 0 |
| Concurrent-32 | 6,362.000 | 6,362.000 | 0 |
| Callback | 1,326.000 | 1,325.975 | −0.025 |
| Deep-32 / concurrent-32 | 29,532.000 | 29,532.000 | 0 |

Miss deltas are spread across unchanged encryption, buffering, request dispatch and result destruction. For unary, AES-GCM seal contributes +4 simulated indirect misses, Promise polling +3, buffered frame preparation +3, TLS polling +3, while task-set polling contributes −3. No new indirect-branch count appears in the extracted Return ownership.

Callgrind uses a small simulated predictor rather than the host CPU's hardware predictor. The [pinned upstream implementation](https://github.com/tklengyel/valgrind/blob/88813f16d17607abf985957d08267417095e6e87/cachegrind/cg_branchpred.c) indexes an indirect-target table using nine low address bits; the conditional predictor combines seven address bits and seven history bits. [Callgrind includes that implementation](https://github.com/tklengyel/valgrind/blob/88813f16d17607abf985957d08267417095e6e87/callgrind/main.c#L42). This explains why changes in code layout can change simulated misses even when branch counts and logical paths are unchanged. It is a plausible source of these differences, not proof that every miss delta is harmless on real hardware. The original capture identifies Valgrind 3.22.0. Its release archive contains a byte-identical predictor file (SHA-256 `b071f3bbf1b6a3dd3b786be8c855883d5e69f0297e464f335b807276983eac1d`), so the referenced predictor matches that release.

The original TLS allocation counts are exactly equal. The fractional TLS byte deltas occur in setup-related stacks. The QUIC allocation differences are principally in quiche next-packet-key derivation and BoringSSL signature handling, outside Return ownership; their counts need repeated measurement. The raw differences remain visible and are not rounded into parity.

## Repeated controls and layout evidence

Each row below compares means of five fresh-process slopes. Full means, sample SDs, paired-delta SDs and individual samples are in the linked table and JSON.

| TLS case | Factored/frozen Ir | Factored/frozen Bcm | Factored/frozen Bim | Identical control Bim |
| --- | ---: | ---: | ---: | ---: |
| Unary | −0.0444% | +0.6417% | +12.6804% | 0.0000% |
| Dependent read | −0.0222% | +3.5172% | +11.5683% | −0.0135% |
| Pipeline | −0.0077% | +3.8782% | +6.0348% | −0.0099% |
| Concurrent-32 | −0.0019% | −5.4310% | −37.2986% | +0.0129% |
| Callback | +0.0171% | +4.0874% | +11.5863% | −0.0030% |
| Deep-32 / concurrent-32 | +0.0008% | +2.0266% | −7.8954% | −0.0013% |

The identical control is the same executable hash **and path**, not a rebuild. Its Bcm changes are within 0.072%. Therefore, the large original Bim changes cannot be dismissed as fresh-process randomness.

[Selected disassembly comparison](layout-comparison.json) finds the same normalized instruction offsets/forms in TLS buffered frame preparation (894 instructions), AES-GCM seal (235), and answer-return checks (176). Linked addresses and RIP-relative displacements are normalized; this is not a claim of byte equivalence. Frozen-to-factored indirect sites move by 32 predictor indices in the TLS/AES functions and by 16 or 32 in the answer checks. This establishes a layout change in unchanged code that can affect the address-indexed simulator. It does not prove that every miss delta has that cause, or predict hardware misses.

TLS allocation slopes remain 7 / 58 / 202 blocks for unary / pipeline / concurrent-32. The identical-binary QUIC control itself changes by −0.005 ± 0.530 / +0.195 ± 0.213 / −0.010 ± 0.038 blocks per operation (paired mean ± sample SD). Tiny QUIC allocation differences must be interpreted with this measured variation; they are not silently rounded to parity.

## Rejected sentinel removal

`TaskSet::new()` installs a permanently pending sentinel future so its old task stream never appears empty. The current `ReadySet`, including its `FuturesUnordered` fallback, can accept new work after returning empty, making removal functionally possible.

The experiment removed that task, changed drain checks to count only real work, and treated an empty ordinary-ready stream as pending while admission, a direct connection or deferred tasks were still live. Empty reusable sets returned before registering an atomic parent waker. Pending application futures, their ownership, FIFO wake order, admission budget, direct connection polling and deferred-write scheduling were preserved.

It removed the sentinel's notification node and initial slot-vector allocation where no real pending task required that storage. However, those setup savings did not reduce per-operation TLS allocation slopes, and the new empty-state handling increased the hot scheduler's instructions.

| TLS case | Scheduler self-Ir delta/op | Total Ir change | Bcm change | Bim change |
| --- | ---: | ---: | ---: | ---: |
| Unary | +14 | +0.0993% | −7.7324% | −10.1599% |
| Dependent read | +28 | +0.0437% | −10.9076% | −12.1626% |
| Pipeline | +14 | +0.0238% | −9.1344% | −13.0729% |
| Concurrent-32 | +17 | +0.0042% | −4.3477% | +74.1494% |
| Callback | +35 | +0.0494% | −7.1268% | −4.5988% |
| Deep-32 / concurrent-32 | +17 | −0.0050% | −28.2382% | +11.4333% |

Scheduler self-Ir includes both Callgrind contexts of `TaskSet<Error>::poll`; every delta is identical across all five repetitions. [Selected self-event attribution](functions-selected.json) preserves these and the largest changes elsewhere. All three binaries also have equal `.text` and `.rodata` sizes (9,872,167 / 828,005 bytes). This was not a successful hot-path simplification, despite passing correctness checks. The implementation and its three experiment-specific tests were reverted together.

## Repeated qualification

The [completed focused run](https://github.com/DericHuynh/capntproto/actions/runs/40142420136) built all three revisions sequentially at the same source path with the same target directory, compiler and release profile. It ran each original fixture five times, rotating variant order and alternating short/long process order. A fourth label used the exact factored executable at the exact same path, providing an identical-binary control. All 480 trials / 960 peer captures completed without failure.

All raw captures, binary hashes, section sizes, symbol addresses and selected disassembly are retained. Reports include means, sample standard deviations, all five samples and paired-delta spread for instructions, branch counts, simulated misses, allocation counts and bytes. The function-level report uses the same counters and reconciles its totals. Cryptographic randomness is not fixed, and negative startup-subtracted values are retained.

The [161,438,439-byte diagnostic artifact](https://github.com/DericHuynh/capntproto/actions/runs/40142420136/artifacts/12580955997) was streamed and SHA-256 verified as `d6497c9427ab5beb18eef42743c8226e630f745e844d5dcbf63c75934ee2aa47`. The artifact retains complete raw profiles, symbol tables, selected assembly and full function attribution; the repository retains the compact results.

Local verification: 19 Python tests passed, including an end-to-end synthetic capture test for both peers and all repetitions; the all-event parser reconciled all 48 original peer captures; Rust formatting and whitespace checks passed. CI passed 832 Rust test executions across overlapping profiles (234 engine alloc, 188 engine no-alloc, 380 hosted RPC, 30 encrypted TLS/QUIC), Clippy with warnings denied, formatting, Cortex-M engine/RPC checks and the standalone no-allocator link probe.

Native timing for the rejected scheduler experiment was not qualified. The prior Return-ownership dedicated run has completed: [its full report](../2026-10-24-legacy-return/latency.md) shows +0.90% TLS / +2.36% QUIC geometric-mean latency and substantial repetition spread. It does not establish parity, and it does not contain the scheduler experiment. The regressions remain unresolved; simulated-counter improvements alone cannot certify native latency.

## Follow-up

Runtime `43bab8a` retains the sentinel and instead removes the two stored `Waker` fields, borrowing temporary wakers from concrete `Arc<Activity>` owners. This targets virtual clone/drop calls without changing admission, drain or deferred-write scheduling. A new lifetime test covers registrations retained across polls and awakened from another thread after the task set is destroyed. The [separate five-repeat comparison](https://github.com/DericHuynh/capntproto/actions/runs/40146245206) is pending; it compares the factored baseline directly with the candidate, reusing the completed control calibration above. No gain is claimed yet.

## Reproduction

From the repository root, reconstruct the original attribution with:

```sh
python3 research/reports/legacy-performance/2026-10-24-legacy-idle/analyze-original.py ORIGINAL_ARTIFACT.zip /tmp/original-functions.json
```

On the comparison runner, after builds finish:

```sh
python3 benchmarks/rpc/legacy-work-profile.py --frozen FROZEN_BINARY --factored FACTORED_BINARY --candidate CANDIDATE_BINARY --output OUTPUT
```

The output directory must be new. To reproduce the summaries from an extracted diagnostic artifact:

```sh
python3 benchmarks/rpc/legacy-work-profile.py --summarize-only --output OUTPUT
```

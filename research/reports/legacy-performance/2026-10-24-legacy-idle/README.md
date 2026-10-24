# Legacy branch-counter investigation and idle scheduler simplification

The original [Return-ownership comparison](../2026-10-24-legacy-return/README.md) contains single paired captures. It shows unchanged TLS allocation counts and selected Return-path instruction counts, but mixed simulated branch-miss changes. Those miss percentages are not native latency percentages.

Candidate runtime: `1b4b94bee09874e7c688616a8771983af217eaf6`. Profiler: `0651e3c`. Qualification: `6a86d30713a4393d19bdea7586e5224ff71d8363`. Frozen control: `a5677a63bb13273dff614205367f944a00145acf`. Current factored control: `d984b8ec7e6ca873481eb899640896651523324f`. Main is unchanged.

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

Callgrind uses a small simulated predictor rather than the host CPU's hardware predictor. The [pinned upstream implementation](https://github.com/tklengyel/valgrind/blob/88813f16d17607abf985957d08267417095e6e87/cachegrind/cg_branchpred.c) indexes an indirect-target table using nine low address bits; the conditional predictor combines seven address bits and seven history bits. [Callgrind includes that implementation](https://github.com/tklengyel/valgrind/blob/88813f16d17607abf985957d08267417095e6e87/callgrind/main.c#L42). This explains why changes in code layout can change simulated misses even when branch counts and logical paths are unchanged. It is a plausible source of these differences, not proof that every miss delta is harmless on real hardware. This source link is explanatory, not a claim that the exact CI Valgrind package was rebuilt from that commit.

The original TLS allocation counts are exactly equal. The fractional TLS byte deltas occur in setup-related stacks. The QUIC allocation differences are principally in quiche next-packet-key derivation and BoringSSL signature handling, outside Return ownership; their counts need repeated measurement. The raw differences remain visible and are not rounded into parity.

## Removed work

`TaskSet::new()` used to install a permanently pending sentinel future so its old task stream would never appear empty. The current `ReadySet`, including its `FuturesUnordered` fallback, can accept new work after returning empty. The sentinel is unnecessary.

The candidate removes that task, changes drain checks to count only real work, and treats an empty ordinary-ready stream as pending while admission, a direct connection or deferred tasks are still live. Empty reusable sets return before registering an atomic parent waker. Pending application futures, their ownership, FIFO wake order, admission budget, direct connection polling and deferred-write scheduling remain as before.

This also removes the sentinel's notification node and its initial slot-vector allocation where no real pending task requires that storage. It is not a claim that all task storage is allocation-free. A real suspended task still creates its first slot when needed.

Three regression tests cover repeated idle-to-active transitions with wake delivery, drain and shutdown; deferred work after admission closes; and empty-to-active reuse both before and after promotion to the fallback set. Existing task-set tests cover cancellation, reentrant reapers, wake replacement, admission budgets and ordinary-before-deferred ordering.

## Repeated qualification

The [focused run](https://github.com/DericHuynh/capntproto/actions/runs/40142420136) builds all three revisions sequentially at the same source path with the same target directory, compiler and release profile. It runs each original fixture five times, rotating variant order and alternating short/long process order. A fourth label uses the exact factored executable at the exact same path, providing an identical-binary control.

All raw captures, binary hashes, section sizes, symbol addresses and selected disassembly are retained. Reports include means, sample standard deviations, all five samples and paired-delta spread for instructions, branch counts, simulated misses, allocation counts and bytes. The function-level report uses the same counters and reconciles its totals. Cryptographic randomness is not fixed, and negative startup-subtracted values are retained.

Local verification: 18 Python tests passed; the all-event parser reconciled all 48 original peer captures; Rust formatting and whitespace checks passed. Focused CI has passed core/hosted tests, Clippy, formatting and embedded checks and is building the comparison executables. Encrypted tests and repeated profile results are pending.

Native timing for this scheduler candidate is not yet qualified. The prior Return-ownership dedicated run is separate and cannot validate this new scheduler change.

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

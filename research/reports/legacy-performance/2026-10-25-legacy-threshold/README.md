# Select the fairness threshold and share the required callback branch

Publication keeps the single private counter. A simple conditional selects limit 32 while busy, or 1 while idle. Comparing turns < limit is equivalent to (turns == 0) | (busy & turns < 32), for all input bytes. Increment is selected only below the limit, so wrapping_add cannot overflow on the executed path. Other values reset the counter to zero.

The counter update now shares the caller's existing callback decision. Forcing another conditional move for the counter duplicates selection work when that control-flow branch is required anyway. The native x86_64 probe uses a cmov for the32/1 threshold, then one conditional jump around the callback and corresponding state update. Exact production owners are compiled in separate translation units to prevent equivalent-function merging between experiments. Static selection instructions fall16->12, with one cmov and one conditional jump on each side. This is not complete scheduler or native latency evidence.

The exhaustive engine selector test covers all 512 byte/busy combinations and bounds every next state. Its existing fixed-storage tests cover consumer turns, saturation, cancellation and 32-turn fairness. Local checks and the five-repetition CI comparison against pre-branchless source c6900f3 have passed. Original Return release is retained on both sides. Previous Return experiment is rejected; no claim of an allocation-free complete connection or beating CPP is made.

All 229 no-default and 276 alloc engine tests, Clippy, nine actual alloc-only legacy conversations and host execution of the fixed-storage shared scheduler pass. The initial local compilation was stopped during cgroup memory thrashing; deleting only generated host debug cache resolved it. No new OOM kill occurred; preserved schema compiler, archives, reports and M4 probes were retained.

The shared scheduler M4 link has no allocator or undefined symbols; this is link evidence, not board execution or a complete connection proof.


## Completed instruction/allocation qualification

[Run40211796330](https://github.com/DericHuynh/capntproto/actions/runs/40211796330) passed all240 profile trials, scoped tests and20 both-width scheduler Miri executions. Original c6900f3 is the control; source3a0aa35 is the candidate. Return release is source-identical. Both executables are built at the same source path.

Exclusive scheduler paired Ir deltas (all SD0): unary+1, dependent+2, pipelined+1, concurrent-3, callback-8, deep-3. Bc/Bi deltas are zero. Scheduler Bcm improves in every case: about-6/-26/-14/-13/-8/-14 respectively, with paired SD below0.04. Scheduler Bim rises2/4/2/2/5/2. Actual TaskSet::poll static non-padding instructions fall1157->1150, conditional jumps179->177, and conditional moves3->2. This is the retained candidate for native qualification, not a claim of universally better latency.

Whole-program Ir mean changes lie-0.076%..+0.029% and retain startup/crypto variation. Whole Bcm improves dependent4.456% and concurrent5.760%, but regresses unary0.457%, pipelined1.522%, callback4.167%, deep37.829%. Whole Bim regresses in all six cases (4.674%..12.841%). TLS blocks stay7/57/202; bytes and QUIC regressions are retained.665/715 common normalized retained bodies match. The difference between component and whole-program events is why native timing is required.

Five fresh-process pairs retain means, sample SD and all regressions in comparison.md/json. The native campaign uses CI's existing dedicated droplets and run-latency mode with legacy control c6900f3. No local/hosted timing result, complete allocation-free connection, or CPP speedup is asserted here.

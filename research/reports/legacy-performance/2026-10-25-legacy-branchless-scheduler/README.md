# Select deferred-publication state before branching around its callback

The scheduler now combines the plain busy/counter/deferred flags with bitwise Boolean operators, selects the next counter using an all-zero/all-one byte mask, and writes both state fields before the one callback decision. Queue operations, polling, atomic ordering and the bounded 32-turn fairness policy are unchanged. The saturating increment is retained, including counter=255.

A local x86_64 rustc release probe extracts the exact candidate helper. The previous short-circuit policy has four static conditional-jump sites; the candidate has one, plus conditional moves for saturation and next-state selection. A plain conditional expression for the counter was also tried: LLVM still emitted two conditional-jump sites (ternary-probe). The source/assembly/counts are retained here. These are instruction-selection probes, not the complete instantiated scheduler or evidence of native speedup.

All 229 no-default and 276 alloc engine tests pass, including all 1,024 combinations of counter and flags and the existing external-consumer/fairness/cancellation cases. Full hosted five-repetition Ir/Bc/Bcm/Bi/Bim and allocation comparison against pooled-input source c6900f3, plus Miri, were pending at source commit; completed evidence follows. All regressions will be retained; fewer static branches do not establish fewer misses or faster latency.

After both branchless changes, nine actual alloc-only legacy conversations and all eight fixed/eight alloc generated-dispatch cases pass. The generated-dispatch fixture still does not instantiate the hosted scheduler or complete no-alloc connection; its unchanged allocator-free M4 link is not instruction evidence for these new paths.


## Completed qualification

[Immutable run 40205963099](https://github.com/DericHuynh/capntproto/actions/runs/40205963099) passed hosted checks and all 240 profile trials. Both revisions were built at the same source path; five paired repetitions retain means, sample SD, allocations/bytes and every regression in comparison.md/json. Branch misses are simulated, not native latency measurements.
The actual TaskSet::poll body changes from179 to177 static conditional jumps and3 to4 conditional moves; normalized non-padding instructions rise1157->1169.714/715 other common normalized bodies match. Exclusive scheduler self Ir increases51 (unary/pipelined),102 (dependent),130 (callback),59 (concurrent/deep), each with paired SD0. Bc falls2..6; scheduler Bcm rises~1 for unary and~3 for callback, falls~5 dependent/concurrent and~9 deep, and is nearly unchanged pipelined. Whole Bcm improves0.293..2.762%, while Bim regresses in five cases; all whole Ir means regress0.004..0.201%. These results do not establish a native speedup.90 Miri executions pass; TLS blocks remain7/57/202. The subsequent single-byte replacement is qualified separately.

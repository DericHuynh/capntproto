# Select deferred-publication state before branching around its callback

The scheduler now combines the plain busy/counter/deferred flags with bitwise Boolean operators, selects the next counter using an all-zero/all-one byte mask, and writes both state fields before the one callback decision. Queue operations, polling, atomic ordering and the bounded 32-turn fairness policy are unchanged. The saturating increment is retained, including counter=255.

A local x86_64 rustc release probe extracts the exact candidate helper. The previous short-circuit policy has four static conditional-jump sites; the candidate has one, plus conditional moves for saturation and next-state selection. A plain conditional expression for the counter was also tried: LLVM still emitted two conditional-jump sites (ternary-probe). The source/assembly/counts are retained here. These are instruction-selection probes, not the complete instantiated scheduler or evidence of native speedup.

All 229 no-default and 276 alloc engine tests pass, including all 1,024 combinations of counter and flags and the existing external-consumer/fairness/cancellation cases. Full hosted five-repetition Ir/Bc/Bcm/Bi/Bim and allocation comparison against pooled-input source c6900f3, plus Miri, remain pending. All regressions will be retained; fewer static branches do not establish fewer misses or faster latency.

# Select the fairness threshold and share the required callback branch

Publication keeps the single private counter. A simple conditional selects limit 32 while busy, or 1 while idle. Comparing turns < limit is equivalent to (turns == 0) | (busy & turns < 32), for all input bytes. Increment is selected only below the limit, so wrapping_add cannot overflow on the executed path. Other values reset the counter to zero.

The counter update now shares the caller's existing callback decision. Forcing another conditional move for the counter duplicates selection work when that control-flow branch is required anyway. The native x86_64 probe uses a cmov for the32/1 threshold, then one conditional jump around the callback and corresponding state update. Exact production owners are compiled in separate translation units to prevent equivalent-function merging between experiments. Static selection instructions fall16->12, with one cmov and one conditional jump on each side. This is not complete scheduler or native latency evidence.

The exhaustive engine selector test covers all 512 byte/busy combinations and bounds every next state. Its existing fixed-storage tests cover consumer turns, saturation, cancellation and 32-turn fairness. Local checks and final five-repetition CI comparison against pre-branchless source c6900f3 are in progress. Original Return release is retained on both sides. Previous Return experiment is rejected; no claim of an allocation-free complete connection or beating CPP is made.

All 229 no-default and 276 alloc engine tests, Clippy, nine actual alloc-only legacy conversations and host execution of the fixed-storage shared scheduler pass. The initial local compilation was stopped during cgroup memory thrashing; deleting only generated host debug cache resolved it. No new OOM kill occurred; preserved schema compiler, archives, reports and M4 probes were retained.

The shared scheduler M4 link has no allocator or undefined symbols; this is link evidence, not board execution or a complete connection proof.

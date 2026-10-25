# One byte represents the deferred-publication state

The old scheduler stored deferred_last and busy_turns independently. Initialization is false/0; a deferral makes them true/1..32; publication resets both to false/0. The flag therefore duplicates whether the counter is nonzero. Publication now owns one private byte and callers initialize it with new/default. Parts borrows this one state, used by both actual hosted TaskSet and fixed storage. The old public projection fields are removed without compatibility aliases.

The pure selector is (turns == 0) | (busy & (turns < 32)). core::hint::select_unpredictable chooses incremented turns or zero. Increment is observable only below 32, so wrapping_add safely avoids a redundant saturation select or overflow branch. All possible input bytes select a next state <=32. Short-circuiting around atomic loads and side effects stays unchanged.

The exact local x86_64 production policy probe reduces 19 static instructions/two cmov to 16 instructions/one cmov, with one callback decision in both. Counts are only the selection wrappers, not full callers, hardware misses or latency. Source/assembly/counts are retained. Full hosted five-repetition work/alloc comparison against bb024d6 and both-width Miri remain pending. Native speedup is unqualified.

Local checks pass 229 no-default/276 alloc engine tests, Clippy, nine actual alloc-only legacy conversations, and execution of the actual shared scheduler with fixed storage on the host. The exhaustive selector test checks all 512 byte/busy combinations against the old reachable-state interpretation and bounds every next state. Existing external-consumer turn, capacity-before-poll, cancellation and perpetually-busy fairness cases pass.

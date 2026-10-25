# Select final Return ownership without a nonfinal-release branch

ReturnOwnership::release still checks underflow before any mutation and decrements the guard count. Its next claimed flag is old_claimed | last, and its result is last & !old_claimed. Nonfinal releases preserve the flag; the final release claims it before caller cleanup can reenter. Explicit completion still suppresses later publication. This uses safe Cell operations and introduces no allocator, synchronization, unsafe code or layout change.

The exact production owners were compiled in local x86_64 release wrappers. The old release has two conditional-jump sites (underflow and last guard). The new release has only the underflow jump; setcc/or/and select the claim and result. It reads/writes the small claimed cell on nonfinal releases, so eliminating a branch can add work. A native speedup cannot be inferred from this probe.

Existing tests cover nonfinal/final releases, every explicit-completion boundary, reentrant publication suppression, answer-storage reuse, zero blocks/bytes, and overflow/underflow without mutation. Full hosted five-repetition instructions, branch events and allocation comparison against scheduler source 18c807c, plus both-width Miri, remain pending. This is a separate comparison so a branch-count improvement cannot conceal instruction or miss regressions.

# Fuse local-client liveness with dispatch state

The local client previously allocated RefCell<ClientState<S>> and a separate
Rc<Cell<bool>> for registration liveness. LocalState<S> now retains the Cell and
RefCell in one Rc allocation. One Weak<dyn RegisteredState> replaces two weak
owners in each registration. Its private is_live method reads only the outer
Cell, without borrowing dispatch data; safe Any trait upcasting retains typed
state lookup. There is no raw pointer erasure or new unsafe code. Public hosted
client methods and the explicit alloc-only ClientRegistry use the same ownership.

The reference-counted liveness allocation and duplicated weak metadata disappear.
The smaller registration also reduces per-interface map storage. Revocation marks
liveness false before application callbacks; weak registrations retain neither the
service nor its state. Three private hosted tests cover one allocation for raw
client construction, liveness during an exclusive dispatch borrow, and final
service release. They require CI's full hosted test compilation.

A source-snapshot comparison uses the actual alloc-only ClientRegistry and generated
Node services with all other dependencies/fixtures fixed. Source snapshots are built
at the same path; the real checkout is never edited. The outer registry map is
warmed, then 32 clients remain live throughout tracing. Five repetitions yield:

| Metric | Before | Fused | Change |
| --- | ---: | ---: | ---: |
| Allocations | 160 ± 0 | 128 ± 0 | -20.000% |
| Bytes | 16,512 ± 0 | 14,720 ± 0 | -10.853% |

These totals include service owners, state, per-interface maps and boxed hooks;
other allocating owners remain. They are not complete RPC allocation budgets,
no-allocator link evidence or native timings. Every sample and source checksum is
in allocation.json. Reproduce with:

    python3 verification/legacy-alloc/check-local-client-allocations.py --baseline 0c363cfdb0ea30d30956c8535df8b010a277050d --output /tmp/local-client-allocation.json

The nine actual no_std fixture tests pass locally, including wake-driven depth-32
and concurrent fragmented conversations, retained data, cancellation, exact EOF/
capacity failures and the specific revocable-alias rejection reason. Alloc-only
fixture Clippy, M4 compilation and four profile-parser tests pass. Hosted compiled
work, full test compilation and two-width Miri remain pending qualification.
The private liveness trait adds an erased read at registry lookup; its cold and hot
work must be measured, not assumed free because allocation decreases.

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
service release. They pass CI's full hosted test compilation and two-width strict-provenance Miri.

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
fixture Clippy, M4 compilation and four profile-parser tests pass. Hosted compiled work, full test compilation and two-width Miri pass qualification.
The private liveness trait adds an erased read at registry lookup; its cold and hot
work must be measured, not assumed free because allocation decreases.


## Qualification

Source `a80ba8b7e26982414c0157207f8f81c51f1fede6`, control
`0c363cfdb0ea30d30956c8535df8b010a277050d`, qualification
`16a37b1d30c1907de94705c862371a41318cd5f7`,
[run 40175537822](https://github.com/DericHuynh/capntproto/actions/runs/40175537822)
passes both jobs, including 384 hosted RPC tests. CI reproduces all five direct
allocation samples above. The actual alloc-only fixture and local ownership tests
also pass Miri on 64-bit and 32-bit targets (18 + 6 test executions).

All 240 hosted trials are preserved. Local plus scheduler self work saves 15 Ir
and 3 Bc in dependent/pipelined reads, with unchanged Bi; other cases are exact.
251/318 normalized common assembly bodies match. Normalization removes addresses
and RIP displacements, not instructions; layout/member offsets also change.
Whole TLS pipelined allocations improve 58 to 57 and bytes 7,447.940 to 7,392.060.
Unary/concurrent TLS block slopes remain 7/202. QUIC means, sample spreads and
paired samples are retained without assuming its variable transport setup is free.

There are substantial simulated predictor regressions: Bcm +14.205% unary,
+5.108% dependent, +5.612% pipeline, +20.828% concurrent, +6.195% callback and
+12.032% deep; Bim +76.904% concurrent, +19.273% deep, +1.308% dependent,
+0.973% pipeline and +2.550% callback (unary improves 3.569%). Whole Ir ranges
-0.338% to +0.028%. These data do not establish native latency parity or erase
the predictor regressions. Dedicated droplet qualification remains necessary.

Verified artifact 12592598120: 81,576,336 bytes, SHA-256
`5628e8c62f01d654276dc6b943a948a28525c369e3b8d063d8166508f311d146`.
Full raw assembly, symbols and profiles remain in the archive. Selected attribution
reconciles every whole-program delta and is reproducible with analyze.py.

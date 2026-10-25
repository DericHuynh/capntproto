# Remove connection identity allocations

Each acquired connection previously allocated Rc<()> solely for token identity.
Entries and affine Registration owners now store a NonZeroU64 from a monotonic
registry counter. The final ID is usable, exhaustion permanently rejects new
registrations, and live reuse still succeeds without consuming another ID.
Capacity/map reservation failures do not consume tokens. Old completion or
cancellation cannot remove a replacement connection sharing its transport ID.
Callbacks and detached state destruction remain outside map borrows.

This removes one mandatory registration allocation and its reference counting.
The counter increases registry setup size; map capacity and other Rc owners remain.
It does not make the entire registry or legacy runtime allocation-free.

Local comparison executes the actual baseline and candidate registry source in
one isolated crate. Across five repetitions, four waves of 32 simultaneous
unpublished registrations allocate 128 ± 0 blocks / 2,048 ± 0 bytes before and
0 ± 0 / 0 ± 0 after on x86_64. Map capacity and registry construction are excluded.
Allocation.json records every sample and both source SHA-256 values. Reproduce:

    python3 verification/legacy-alloc/check-registration-allocations.py --baseline 8cdaf8698b734e065bacaff8ccea5bb093695248 --output /tmp/registration-allocation.json

All 14 real registry tests and the separate original-source allocation assertion
pass locally, including stale completion/cancellation, reentry, early cancellation,
shutdown observers, exact token exhaustion and capacity recovery. The expanded
eight-case actual no_std conversation fixture, Clippy and M4 checks also pass.
The small isolated test harness avoids hosted test-codegen memory exhaustion;
CI must validate the final full hosted suite, two-width Miri and work profiles.

Steady-state 50−10 benchmark slopes subtract connection setup. They cannot on
their own show this allocation saving. Qualification therefore also records this
explicit setup/allocation comparison and includes registry assembly, alongside
repeated hosted profiles. No local/native latency claim is made.

## Qualification

Source `0c363cfdb0ea30d30956c8535df8b010a277050d`, control `8cdaf86`,
qualification `6c34f3c`, [run 40173642759](https://github.com/DericHuynh/capntproto/actions/runs/40173642759), passed.
CI repeats the direct source allocation comparison with the same five identical
128/2,048 → 0/0 samples. Full hosted verification passes, plus 16 actual alloc-only
conversation and 28 registry strict-provenance Miri executions across two widths.

The repeated six-case hosted profiles retain 240 trials. Registry plus scheduler
self Ir/Bc/Bi slopes are unchanged; some registry work occurs only at setup, so
this cannot replace the explicit allocation probe. 155/173 normalized common
functions match; all 18 changed bodies are registry removal/destruction functions,
as expected when eliminating Rc identity cleanup. TLS block slopes stay 7/58/202.
Whole Ir ranges -0.032%..+0.060%, with deep +434.595 including +405.090 in memcpy.
All bytes and QUIC spreads are preserved. Simulated Bcm improves in every case,
but Bim regresses 8.401% unary, 0.540% dependent and 6.382% callback. Those are not
hardware/native latency measurements and are not erased by the allocation gain.

Artifact 12591867455: 81,134,637 bytes, SHA-256
`fbdaaeea7b439846138b5c47ed4fb6ac96c9c49266ab5dc2a6cfdf85a8870459`.
The complete assembly/symbols/raw profiles remain in that verified archive.

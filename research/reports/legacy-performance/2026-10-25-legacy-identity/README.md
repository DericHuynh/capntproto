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

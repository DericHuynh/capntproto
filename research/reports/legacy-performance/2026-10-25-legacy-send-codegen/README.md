# Remove forced request inlining and its eight caller copies

The concrete Request extraction's helper-only work was equal, but its callers
were not. The repeated CI attribution shows +8 Ir per call and +256 per batch of
32 concurrent/deep accesses. A release-optimized compiler reproducer isolates
exactly eight extra stack-copy instructions while moving the typeless remote
promise into an async caller that only awaits its response.

The original send method did not force inlining. The new generic wrapper and
boxed publication method both used inline(always), exposing aggregate return
materialization too early to eliminate those copies in the caller. Restoring
only the intermediate typed-promise local leaves the extra instructions.
Removing either forced-inlining annotation eliminates them in the compiler
reproducer. The implementation removes only the outer Request::send annotation,
retaining the concrete-owner API and the default publication implementation.
No ownership, cancellation or allocation policy changes.

check-request-codegen.py copies core into an isolated workspace at one fixed path,
keeps all other source/dependencies fixed, and compares the exact baseline
capability.rs with the candidate. It injects a diagnostic-only original-send
reference into the snapshot, never the real checkout. The two exported async
callers compile to:

| Variant | Current caller instructions | Original-send reference |
| --- | ---: | ---: |
| Before | 212 | 204 |
| Outer annotation removed | 204 | 204 |

The candidate instruction forms match the reference after normalizing basic-block
label numbers and source-location constants. Full assembly and hashes are retained.
This is static assembly evidence on rustc 1.97, not a timing benchmark, byte
identity, proof for every schema, or the full benchmark workload. The diagnostic
future is boxed only to force emission of all its poll states; it is not executed.
The optional allocation dependencies are locked once for both compilations.

Reproduce:

    python3 verification/legacy-alloc/check-request-codegen.py --baseline ddeffe7e66aede9bfc614c966b9dc24f0f508c71 --output /tmp/request-codegen/evidence.json

The repeated benchmark qualification must compare against that same latest
call-context source, retaining 5 repetitions, all means/spreads, byte/allocation
counts and all predictor regressions. Assembly capture is expanded to include
application async access and capability workflow callers; unchanged helper
assembly is insufficient to establish caller parity. Native latency remains
restricted to dedicated droplets. Whole no-alloc legacy integration is pending.

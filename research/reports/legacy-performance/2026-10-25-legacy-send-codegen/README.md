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


## Repeated hosted qualification

Source `1148bb445d597f8a5702cdecedb3c7bba57f5828`, control
`ddeffe7e66aede9bfc614c966b9dc24f0f508c71`, qualification
`1a0d0625f0b6ab6ee06c9d59135cd03caba16931`,
[run 40183702611](https://github.com/DericHuynh/capntproto/actions/runs/40183702611)
passes full hosted verification, encrypted behavior and all 240 trials. This
annotation-only change adds no new Miri run; the preceding request/future/call
ownership changes have separate two-width qualifications. CI reproduces the
212-to-204 static caller result and normalized reference equivalence.

The real benchmark removes eight Ir per affected call (SD zero), including
-256 Ir per concurrent/deep batch (SD zero). Expanded disassembly confirms the
actual topology access poll shrinks 1717 to 1709 instructions. Across the six
cases, whole Ir improves 0.005%–0.055% and Bim improves 1.895%–8.580%.
Whole concurrent Ir falls 267.575 ± 91.014 and deep 208.885 ± 16.677 (paired
sample SD). Selected capability/scheduler Ir/Bc/Bi remains exact; 667/690 common
captured bodies match after normalization, with the caller changes retained.

TLS blocks remain 7/57/202. Every byte/QUIC mean, spread and paired sample is
preserved. Bcm regresses dependent +0.096%, pipeline +1.209%, concurrent
+0.615% and callback +0.467%; deep whole Bc increases 60.380 despite unchanged
selected helpers/caller branch counts. Those deltas are not erased by lower Ir
or Bim. This remains simulated/compiled-work evidence, not droplet native timing.

Verified artifact 12595317945: 82,639,809 bytes, SHA-256
`bb49db864aba69c22d8fa536b0add2b297e8d9b00a6f8f84848e5c83e225fce8`.
The archive retains raw profiles, full selected caller assembly, symbols and
both CI codegen reproducer assemblies. Local and CI reproducer hashes differ
with source paths; their recorded instruction results and normalization agree.

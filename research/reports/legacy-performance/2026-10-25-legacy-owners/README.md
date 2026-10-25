# Concrete legacy parameter and response owners

Source `df796ab`, baseline `0c44814`; qualification `36caebe3c50431d0a5b64e9cdbefd3eca790d2a2`, [run 40165853875](https://github.com/DericHuynh/capntproto/actions/runs/40165853875), succeeded. Artifact 12588319262 is 84,854,288 bytes with verified SHA-256 `52e73b58c182e4c790b5ac1e66cb95edf43dece3de7283fe464fc462ffc46522`.

Params<T,H> and Response<T,H> now retain concrete readers and selected frame owners in no-default builds. Read-only hook contracts no longer require allocation; the hosted default still uses boxed hooks with the same field types and layout. Moving, retyping, or changing wrappers preserves the owner's read budget. A concrete owner releases its byte guard once; borrowed data cannot outlive its backing. Explicit boxed forwarding preserves parameter identity/policy boundaries.

CI passed 86 hosted and 44 no-default core tests, documentation (including borrow rejection), 261 allocating and 214 no-default engine tests, 378 hosted RPC, 30 secure RPC and 7 task-support tests. All 14 targeted Miri executions passed (4 allocating and 3 no-default cases on each pointer width). The owner probe ran on the host and linked for M4 without allocator/undefined symbols; the board binary was not executed. Three preexisting heap-builder doc examples were feature-guarded so no-default documentation checks now pass too.

All hosted read-owner plus scheduler self Ir, Bc and Bi are exactly equal across all six cases/five repetitions, zero paired SD. All 261 common selected assembly functions retain normalized instruction structure. This is normalized structure, not byte/address equivalence. Original assemblies are about 28 MB each; retained assemblies are streamed/narrowed to direct read methods, read-hook implementations and scheduler/control hot paths, not every function mentioning a hook in its generic type.

TLS blocks remain exactly 7/58/202. Byte and QUIC deltas/spreads remain visible in comparison.json. Simulated concurrency Bim still rises 11.990% and deep Bim 3.437%; native hardware behavior/latency are unqualified. Whole-operation startup/libc contributions are preserved instead of being called generic noise. Raw profiles, original disassembly, exact fixtures and binary identities remain in the artifact.

This removes compulsory boxed ownership from read access, not the whole legacy protocol. Fixed/fallible output and internal protocol/transport ownership still need migration. No existing bounded driver is relabeled as completion.

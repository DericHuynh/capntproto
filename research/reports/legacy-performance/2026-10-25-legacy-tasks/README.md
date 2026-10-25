# Pinned legacy task ownership

Source `5578f6c6a4ecb1d3c6f52c56d36142fd1abe05b0`, baseline `18755f2e57a4157839d1390e03e2ec9fe823b521`. [Run 40161615453](https://github.com/DericHuynh/capntproto/actions/runs/40161615453), qualification `8746e2186a102f8e5bf23cca5beff363140c42f4`, succeeded. Artifact 12587426610 is 81,006,265 bytes, SHA-256 `30d108c01b13f9188d8ebe36ded270916f8183609eed197153f7e895967d137c`, stream-verified before extraction.

## Ownership change

The actual hosted ReadySet delegates its ready poll/retirement loop to the unsafe-free engine. Borrowed slot views retain its Vec, stable Arc wake nodes, sentinel and exceptional FuturesUnordered fallback. A fixed provider instead owns futures in caller-pinned arrays, supports !Unpin borrowing futures, returns rejected futures unpolled and destroys completed/canceled futures immediately in place. Merely retaining Pin<&mut Future> would have delayed destruction until the caller's future owner ended; that design was replaced before qualification.

Static standard wakers carry only static notification storage and a reusable index, never a task pointer. An escaped wake can schedule a replacement but cannot complete it or access a destroyed task. Raw-waker construction and structural pin projection are confined to the small no-std/no-alloc capntproto-task crate. The engine remains forbid(unsafe_code).

## Validation and measured work

870 unit/integration executions plus one compile-fail documentation test passed: 252 allocating engine, 205 no-default engine, 378 hosted RPC, 30 secure RPC and 5 task-support tests. Strict-provenance Miri passed 22 executions across 64-bit and 32-bit targets (5 support and 6 task tests on each). Clippy, formatting, M0 engine and M4 no-default RPC checks passed. The M4 task probe linked without allocator or undefined symbols; it was not executed on a board.

Five repetitions of six TLS Callgrind cases and three DHAT cases per transport retain means, sample SDs and all paired slopes in comparison.json. No native timing claim is made.

Combined scheduler/readiness self Ir falls by exactly 82 (unary), 164 (dependent), 82 (pipeline), 90 (concurrent) and 203 (callback) in every repetition. Their component indirect counts are exactly unchanged. Deep component Ir falls by 86.405 ± 8.039; one repetition includes extra scheduler work, retained in the samples. ReadySet::poll_next is the only changed selected assembly function (571 to 574 static instructions); 45 of 46 selected functions retain normalized structure. Dynamic instruction savings are localized to that poll function; larger static code does not imply more executed work. No new indirect task dispatch is introduced by the generic slot projection.

TLS block slopes remain exactly 7/58/202. Byte slopes and all nonzero QUIC differences remain visible with spreads. Whole-case Ir falls 0.011–0.183% in the first five cases; deep rises 192.660 (+0.005%), with +256.160 attributed to libc memcpy offsetting the scheduler reduction. Attribution reconciles to every aggregate delta.

Simulated indirect misses rise for unary (+10.760%), dependent (+3.538%), pipeline (+5.649%) and callback (+2.709%); concurrent improves 13.354% and deep 3.905%. Code/data placement changes, including +112 bytes for sampled read-only objects; .text grows 64 bytes. These are not binary or predictor parity. Native hardware branch behavior and latency remain unqualified.

## Remaining migration

This qualifies pinned task ownership, not the entire legacy protocol. The subsequent shared scheduler, heterogeneous task owners, pending-call storage, and protocol/hook/message ownership need their own qualification. Mandatory public Box/Rc client hooks still prevent a complete legacy RPC no-allocator profile. The existing bounded wire driver is not relabeled as completion.

Run `python3 analyze.py DIRECTORY` against retained selected-functions/disassembly evidence plus symbol files from the artifact to reproduce the component and assembly analysis. scope.json contains exact fixtures, CPU identity, binary hashes and section sizes. Full raw profiles remain in the linked artifact.

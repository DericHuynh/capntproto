# Legacy admission extraction qualification

Source: `18755f2e57a4157839d1390e03e2ec9fe823b521`, compared with `4888f89215b92e88b163bb0be6ceb0985d7f1de3`. Qualification commit `9ced26c6667bd94193621296d17d8e81852535c8`, [run 40158026841](https://github.com/DericHuynh/capntproto/actions/runs/40158026841), succeeded. Artifact 12586086149 has SHA-256 `7dcf78a6f22212cf584a4bbf4b09c5d0efb71ba6f01d65e36beacd73205f0395` (80,877,329 bytes, stream-verified before extraction).

## Result

The hosted admission channel now instantiates the same algorithms as a caller-borrowed fixed FIFO. Rejection returns the original input. Clone/drop shutdown, wake registration, exceptional capacity retirement and reentrant destruction retain their prior ownership rules. The sender count now explicitly rejects overflow instead of wrapping in optimized builds.

All 853 unit/integration test executions passed (246 allocating engine, 199 no-default engine, 378 hosted RPC, 30 secure RPC). Clippy, formatting, M0 engine checks and the no-default M4 RPC check passed. The borrowed-admission M4 ELF linked with no undefined or allocator symbols. It was not executed on a board.

Across six TLS cases and five repetitions, combined admission plus scheduler self Ir and indirect branch counts are exactly equal in every repetition. This groups renamed old and new functions; subtracting the old symbol alone would falsely report savings. TLS allocation block slopes remain exactly 7/58/202. Bytes and the small nonzero QUIC allocation differences, including their spreads, are preserved in comparison.md and comparison.json.

This is not full binary or branch-prediction parity. The common selected assembly has 28/40 identical normalized functions before mapping renamed callees; the sender clone adds checked-overflow work (the inlined handle clone grows from 28 to 34 instructions). Linked code and read-only locations change, including +16 bytes for the sampled read-only objects. Consequently simulated indirect misses improve in four cases, but concurrent-32 rises 13.955% and deep concurrency rises 3.421%. Concurrent receive/dispatch/result indirect sites shift prediction collisions in the trace; the API extraction did not remove indirect dispatch. These regressions remain qualification evidence, not a latency improvement.

Deep aggregate Ir increases 265.775 (+0.008%); 261.630 of this is attributed to libc memcpy self work. Other cases range from -28.215 to +4.680 Ir, with crypto startup and libc contributions retained. Per-function attribution reconciles to every whole-operation metric; no delta is discarded as generic noise.

## Method and limits

240 trials, alternating both builds at the same source path, five repetitions, 10/50-operation slopes with startup variability retained. Callgrind branches are simulated; DHAT includes all processes. Full fixture commands, CPU identity, hashes and raw profiles are in the linked artifact; compact results and attribution are retained here. The analyzer expects the selected-functions evidence produced by the streaming artifact reader. Native latency for this extraction remains unqualified; no timings from the shared local environment are used.

The completed component is admission, not the entire legacy driver. Pinned task ownership, scheduler policies and protocol/hook/message ownership are still tracked in the completion plan.

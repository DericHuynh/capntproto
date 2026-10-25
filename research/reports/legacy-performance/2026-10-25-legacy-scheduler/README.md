# Shared legacy scheduler qualification

Source `775afbe38e475d34067da6a16a293c6e14191693`, baseline `5578f6c6a4ecb1d3c6f52c56d36142fd1abe05b0`. [Run 40163004642](https://github.com/DericHuynh/capntproto/actions/runs/40163004642), qualification `3e53592573eb18c9c4b639369766e13fa81238c1`, succeeded. Artifact 12587444060 is 80,767,081 bytes, SHA-256 `31b48d1a61be1d0c3d62a7c2948474c59db5b4d01deb9dd3ead641f92660cfe0` (stream verified).

## Integration and correctness

The ordinary/deferred/direct-connection scheduling loop is now shared by hosted field projections and caller-backed providers. Bounded work is admitted only with storage credit, before its first poll; a retained blocked command is rescheduled when capacity returns. Independent caller-backed stop bypasses saturation. Fixed empty notifications, direct connection handling, bounded fairness and an external consumer turn before deferred publication are exercised with real !Unpin tasks, including two-word heterogeneous owners built using standard Rust trait objects. Their ownership guarantees immediate in-place destruction; no manually erased ownership pointer or second task allocation is introduced.

880 unit/integration executions plus two support compile-fail documentation tests passed: 256 allocating engine, 209 no-default engine, 378 hosted RPC, 30 encrypted RPC and 7 task support. Strict-provenance Miri passed all 34 support/task/scheduler executions across 64-bit and 32-bit. The scheduler probe's actual exercise function passed on the host, then linked for M4 with no allocator or undefined symbols. The M4 binary was not executed on a board. Formatting, Clippy and no-default target checks passed.

## Five-run profile

The readiness component has exactly equal Ir and indirect counts in every case/repetition. Combined scheduler/readiness Ir falls by 26/52/26/16/54/16 per operation for unary/dependent/pipeline/concurrent/callback/deep, with zero paired SD. Indirect counts are equal except callback, which adds two (also zero paired SD). Grouping includes both old and new reaper trait names and the inlined former update_on_empty function; renaming/inlining is not counted as removed work.

Conditional branches increase by 24/48/24/32/52/32 at the scheduler boundary. The callback trace attributes the two extra indirect calls to the two TaskSet poll contexts; the formerly private hosted reaper boundary now uses a public engine trait. This source projection also repeats the source-closed check before the hosted adapter's live-source selection. A follow-up will fuse that check, preserve the private hosted reaper vtable behind a static provider, and force the regular policy into the caller before alias analysis. That change is not included in these results and must be qualified separately.

TLS block slopes remain exactly 7/58/202; bytes and every nonzero QUIC delta are retained with spread. Simulated indirect misses improve slightly in all six cases, but conditional misses rise 0.179–3.766%. Whole-case instruction deltas include startup crypto and libc work; deep is +240.065 (+0.007%). No delta is discarded as generic noise. Full metric means, sample SDs, paired samples and provenance are preserved in comparison.json and scope.json.

This is qualified correctness with measured remaining work gaps, not compiled-work parity or native latency qualification. The pending-call and parameter/response ownership steps are separate snapshots. The legacy protocol still has mandatory owning hooks and growing result builders; an existing bounded driver is not relabeled as completion.

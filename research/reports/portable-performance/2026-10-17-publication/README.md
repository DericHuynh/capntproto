# Publication ownership and forwarding, October 17, 2026

Portable is not the fastest implementation, and the 5% milestone remains open.
The final local production build meets the milestone in 8/16 cells and is fastest
in 4/16. The latest dedicated campaign measures the preceding commit `d56aae2081ae`:
9/16 within 5%, 2/16 fastest. Different machines and topologies are separate
observations, not a before/after progression.

## Implementation and C++ correspondence

The C++ reference is `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`.
Its `QuestionRef` and `paramExports` in `vendor/capnproto/c++/src/capnp/rpc.c++`
keep response interest and parameter-grant bookkeeping separate from message
bytes. The Rust port follows those ownership boundaries while retaining branded
protocol tables, bounded output admission and cancellation guarantees.

* A projected child retains its parent in the existing request target slot until
  successful FIFO publication. It no longer allocates a response attachment just
  to retain that interest. Queued fallback, failed publication and cancellation
  keep their owner until normal release. Protocol target leases can outlive the
  application call slot; application-slot survival is not the ordering contract.
* The private one-capability encoder stores the exact method, handoff policy and
  exported capability ID in its question. It does not retain the entire outgoing
  frame until Return. Full publication still validates the immutable frame and
  grant batch. Output owns bytes through partial writes and flush. General
  questions retain their frame; their validated publication permits descriptor
  accounting without repeating payload traversal. Raw admission still validates
  payload capability indices. Adopted ThirdPartyAnswer frames have no parameter
  grants. Returned payloads and all descriptor authority checks remain validated.
* Queued forwarding embeds the queue receiver in its existing continuation,
  eliminating an intermediate boxed Promise. This shared code also serves legacy;
  the final legacy comparison is rebuilt from the same source.

The regression tests force output backpressure, drop both public pipeline owners,
exercise cancellation before publication, reclaim the complete frame pool before
Return, reject a wrong answer ID without consuming grants, receive a capability
as its last parameter grant is refunded, and drain a late canceled Return.
They check actual wire ordering and final resource recovery.

## Profiles and traces

Whole-process Callgrind includes startup, 100 warmups and 1,000 measured operations.
These are instruction counts, not uninstrumented latency measurements.

| Case / peer | Before instructions | After instructions | Change |
| --- | ---: | ---: | ---: |
| Empty unary / client | 16,847,133 | 16,847,673 | +0.003% |
| Empty unary / server | 15,077,507 | 15,079,114 | +0.011% |
| Pipeline / client | 72,631,870 | 72,222,502 | -0.564% |
| Pipeline / server | 63,453,652 | 63,455,307 | +0.003% |
| Callback / client | 55,880,737 | 55,706,157 | -0.312% |
| Callback / server | 56,247,346 | 56,247,829 | +0.001% |

Pipeline client malloc calls fall from 49,840 to 47,639: two per warmup/measured
child, plus one setup allocation. Callback malloc calls fall by three over the
whole process, not three per call. All six before/after peer comparisons preserve
the complete write bytes and syscall groups. Thirty final peer journals include
the runtime before/after, full production portable, current legacy and C++.
Matching C++ write grouping is already insufficient to close the latency gap.

Six other approaches were rejected: output-retirement notification suppression,
narrower worker wakes, outlining polling, owner-retirement notification changes,
response-state parent retention, and releasing private parameter builders at
publication. Their source, binaries, logs and profiles are retained. Earlier test
draft failures are retained too; the final regressions use protocol ordering and
valid Return semantics as their oracles.

## Uninstrumented local measurements

The final workflow campaign rotates five variants across five repetitions,
100 warmups and 1,000 measured operations per trial, pinned to client CPU 2 and
server CPU 1. Both benchmark frontends are retained to expose harness effects.
The production build's pooled p50s below use independently recomputed nearest-rank
percentiles. The JSON also retains p95, p99, throughput and repetition medians.

| Workflow | Portable µs | Legacy µs | C++ µs | Portable / faster reference |
| --- | ---: | ---: | ---: | ---: |
| Empty unary | 20.490 | 19.679 | 21.702 | 1.041 |
| Small unary | 20.971 | 20.130 | 22.303 | 1.042 |
| 1-KiB unary | 32.057 | 29.904 | 22.733 | 1.410 |
| Fanout 32 | 148.880 | 205.233 | 170.772 | 0.872 |
| Dependent read | 54.431 | 44.897 | 47.439 | 1.212 |
| Pipelined read | 43.875 | 28.902 | 31.116 | 1.518 |
| Write/read | 41.331 | 39.628 | 44.976 | 1.043 |
| Callback | 49.573 | 41.652 | 45.617 | 1.190 |
| 64-KiB echo | 48.412 | 48.141 | 52.768 | 1.006 |
| 256-KiB echo | 90.874 | 163.441 | 260.674 | 0.556 |
| 1-MiB echo | 471.144 | 724.769 | 993.526 | 0.650 |
| 4-MiB echo | 3,026.704 | 3,993.049 | 4,628.451 | 0.758 |

A separate twenty-repetition campaign repeats unary-1k, pipeline and callback.
The initial unary-1k harness discrepancy does not persist in that repeat; both
results remain retained. Large pooled timing shifts coexist with nearly flat
paired repetition medians. These measurements do not establish a broad latency
improvement. The follow-up does not replace the initial campaign's failed cells.

The separate loopback campaign keeps its existing 10,000 warmups, 1,000 measured
operations, five repetitions and discovered CPU placement. All 60 trial
percentiles and inclusive 5% gate decisions are independently checked.

| Payload bytes | Portable ns | Legacy ns | C++ ns | Portable / faster reference |
| --- | ---: | ---: | ---: | ---: |
| 0 | 28,673 | 19,709 | 32,999 | 1.455 |
| 64 | 31,447 | 19,619 | 22,162 | 1.603 |
| 1,024 | 31,697 | 20,180 | 24,646 | 1.571 |
| 65,536 | 56,584 | 48,141 | 61,881 | 1.175 |

All experiments together retain 1,680,000 uninstrumented local samples. Builds,
tests and profiling did not overlap timed campaigns. No benchmark fixture,
sampling budget, acceptance threshold or coverage floor was changed.

## Architecture assessment

The remaining costs lie across the hosted call, graph route, portable invocation,
response state and pipeline observers. They have distinct cancellation and
authority obligations, but currently require multiple owners and continuations.
A unified execution owner with separate response and pipeline interests is the
next substantial architectural target. It must preserve early pipeline dispatch,
protected execution, tail adoption, membranes and bounded publication. Removing
notifications or freeing payloads earlier did not demonstrate a useful gain in
this campaign. Replacing Tokio is not supported by the earlier Compio comparison.

## Validation and provenance

308 RPC tests, 147 wire tests and 300 mixed-peer combinations pass. Strict Clippy
passes both modified crates. ARM wire checks pass with and without allocation.
Six focused Miri tests cover publication ownership, compact grants and queue
cancellation. Repository, wiki, formatting and the workspace unsafe checks pass.
No unsafe code was added. Coverage has not been requalified; the
preceding commit's CI still fails coverage. Neither that floor nor the gate's
advisory status is changed.

The [machine-readable report](report.json) includes all final source hashes,
independent analysis and unchanged fixture hashes. The [source patch](source.patch)
applies to `d56aae2081ae28409e1d45e1ff6cc73c41f4542a` and reproduces source tree
`079e11910ba47b6d614158a06bbeb1d757ba57e5`. Its SHA-256 is
`80cb60ddbcb993ea1a5bdaa8413916e3f43ca0a795b799a4fcfc3ebba3930117`.
The Tokio-only binary SHA-256 is
`0a9edaf12f95fc3e390544045fa8a776f5fa68e22b36bdfecb40229f9df9274c`;
the full portable/legacy binary is
`2fa2440ebb65083316d7cf2ea08fef1ef2651aca71d2c1c9f3a92cef0bf33557`.
Documentation updates follow that source freeze and do not change compiled code.

The local archive is `target/portable-fastest-publication-20261017/`: 5,988 files,
596,625,256 bytes; SHA256SUMS SHA-256 is
`d1550e5e7efd1317dd6967c3964d0ab0bc185d6c6681486e27061782ef2aba0b`.
Rejected binaries compressed with gzip retain verified uncompressed hashes;
their freeze records name the original binary paths. Reproduction scripts,
raw samples, profiles and decoded journals remain available in the archive.

[Dedicated run 39008671050](https://github.com/DericHuynh/capntproto/actions/runs/39008671050)
measures `d56aae2081ae`, not this candidate. All 350 remote and 60 loopback trials,
build/runtime identity records and final cloud-resource cleanup were verified.
Its immutable archive, `target/portable-dedicated-d56-20261017/`, contains 1,480
files; SHA256SUMS SHA-256 is
`b9019086e8b25cc0a9177da6d898ad4fa0d80562a5d82cc39ba753ac1e3aae57`.
Workflow success is not latency qualification: its performance gate is advisory.

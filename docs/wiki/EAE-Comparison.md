# EAE comparison and integration decision

**Keep `storage::Store` as the runtime default.** EAE is technically usable as a
fixed-capacity delta backend, but its durable adapter does not implement the
current ORM/history contract. Measurements preceded the integration probe.
The probe stays outside the production dependency graph.

The measurements below describe the V4 whole-entry baseline. The later
[opt-in V5 component path](Component-Storage.md) shares unchanged typed
components without adopting EAE; its separate byte-count example does not
replace these timing results or use the same checkpoint accounting.

## Storage contract and review provenance

The input is [EAE-Reconstruction.zip](../../research/EAE-Reconstruction.zip), SHA-256
`55872653058ee56813a3417df242760b81dc002e53500ca13c7c696335cf7f5b`, containing
`eae_pages` 0.7.0 and the frozen v6 comparison. Its page-versioned `Engine` and
anonymous-mapping `DeltaEngine` are experiments with process-local metadata.
The reopenable backend evaluated here is `durable::Arena`: fixed segments,
undo on abort, checksummed redo, synced commits and atomic checkpoints.

The durable adapter loads a checkpoint into anonymous memory and replays redo;
it does not mutate committed file mappings in place. Its first snapshot after
a changed generation copies the whole arena. The page engine's snapshot costs
must not be attributed to this different durable implementation.

| Contract | Capn't Proto Store | EAE durable arena |
|---|---|---|
| Updates | Whole entries with per-object head/publication CAS | Byte ranges with an arena-wide generation CAS |
| Publication/history | Staged heads, published revisions, retained history and cursor gaps | Current image and detached snapshots; no ORM publication/history layer |
| Transactions | Atomic single-store batches, including publication plus bulk receipt | Ranges across fixed segments in one redo transaction |
| Snapshot ownership | Read-only file mappings; held snapshots pin generations and locks | Full-image copies; held snapshots own memory independently of the writer |
| Recovery | Independent framing checks and file/directory stabilization | Header/frame checksums, redo replay and recovery stabilization |
| Capability policy | Typed ORM envelopes, live-capability rejection and explicit SturdyRefs | Raw storage with caller validation; no RPC authority policy |
| Capacity/platform | Host-configured entry/file limits; Linux preview | Fixed segments, bounded arena; 64-bit Linux with 4 KiB pages |

The initial [review evidence](../../research/reports/README.md)
records 110 passing EAE cases in each of debug/release, including compile-fail
tests; these are not 110 independent crash scenarios. Those tests used upstream
`capnp` 0.21.7. The probe below separately exercises our vendored 0.25.6 core.
EAE has no TLC-to-Rust replay driver for its durable adapter.

The same review exposed Store framing and recovery bugs. Independent length
validation, unconditional recovery barriers and I/O/process-crash regressions
are now implemented as release gates R3–R5 in
[Release Acceptance](Release-Acceptance.md). RPROTO04 is the accepted format;
the pre-fix descriptions are not current Store behavior. The later storage worker exposes rejected versus uncertain outcomes, but
durable receipt lookup remains separate work: failed/lost mutation
replies may have committed, and are never made safe to replay by storage format
alone.

## Matched baseline before integration

Five trials, eight workloads, two engines: **80 fresh-process runs**, 200 updates
per run, on the same Btrfs filesystem on this Linux host. Both use one fsynced
transaction per update, per-object CAS metadata and latest-publication semantics.
Scheduled checkpoints and a final normalization checkpoint are included in byte
accounting. Initial seeding is also counted; write amplification divides all
those bytes by application edit bytes. Tests validate every object/revision,
held snapshots and reopened images. No builds/tests ran concurrently with timing.

Values below are medians across the five per-run results. Commit is the median
commit latency; snapshot is the first acquisition of a new generation. Wall time
includes preparation, verification, scheduled checkpoints and snapshot work.

| Workload | Commit µs Store / EAE | Wall ms Store / EAE | First snapshot µs Store / EAE | Total written bytes Store / EAE |
|---|---:|---:|---:|---:|
| 32 × 64 B, replace | 2706 / 2699 | 571 / 589 | 1.96 / 16.97 | 68,112 / 67,272 |
| 32 × 4 KiB, replace | 2861 / 2807 | 616 / 595 | 1.96 / 92.54 | 1,390,608 / 1,518,792 |
| 8 × 64 KiB, replace | 2979 / 2901 | 663 / 643 | 1.96 / 327.62 | 15,236,776 / 15,768,840 |
| 2 × 1 MiB, replace | 4488 / 3761 | 1325 / 981 | 2.31 / 1223.83 | 234,913,384 / 237,050,648 |
| 32 × 4 KiB, 8 B edit | 2848 / 2705 | 613 / 586 | 1.89 / 90.23 | 1,390,608 / 701,192 |
| 8 × 64 KiB, 8 B edit | 2989 / 2692 | 633 / 608 | 1.89 / 330.84 | 15,236,776 / 2,663,240 |
| Same edit, 16× arena capacity | 2989 / 2702 | 644 / 649 | 1.96 / 1840.25 | 15,236,776 / 34,135,880 |
| Same edit, snapshot every update | 2965 / 2715 | 648 / 666 | 1.89 / 289.77 | 15,236,776 / 2,663,240 |

For dense 64 KiB entries with 8-byte edits, EAE writes **5.72× fewer bytes**
(2.66 MB versus 15.24 MB), including checkpoints. Its first snapshot is about
175× slower (331 µs versus 1.89 µs). With 16× spare capacity, EAE writes **2.24×
more** than Store and its first snapshot costs about 1.84 ms. Frequent new
snapshots erase the small commit advantage in end-to-end workload time. Whole
1 MiB replacements favor EAE on this host, but do not reduce total bytes written.

Timing varies substantially between trials: the dense 64 KiB edit workload spans
625–1,217 ms for Store and 570–1,229 ms for EAE. A shared-host microbenchmark does
not establish statistical significance or deployment tail latency. EAE checks
CRC32C while Store uses SHA-256; these results compare implementations, not an
isolated delta algorithm. Same-generation snapshot repeats can share an existing
snapshot; the first snapshot after a commit cannot. Reopen measurements are
warm-cache, peak RSS is per-process, and logical bytes do not represent physical
CoW/compression allocation or peak filesystem space. Checkpoint cadence may leave
an empty redo log at the first reopen. Neither engine isolates synchronous I/O
from a LocalSet's network tasks.

[Original comparison](../../research/reports/storage-benchmark/before-integration.json) and
[all original runs](../../research/reports/storage-benchmark/before-integration-runs.jsonl)
preserve the pre-probe measurements and source hashes. The final-tree rerun is
[comparison.json](../../research/reports/storage-benchmark/comparison.json), with every trial,
P99, repeat-snapshot, reopen and RSS result in the adjacent `runs.jsonl`.

The second 80-run cohort on the final executable sources reproduced the same
byte totals. For dense 64 KiB field edits, first snapshots measured 1.96 µs
(Store) versus 344 µs (EAE), and workload medians were 641 ms versus 578 ms.
With 16× spare capacity, workload medians were 649 ms versus 666 ms; with a
fresh snapshot each update they were 670 ms versus 664 ms. Trial-to-trial drift
remains large, so the byte-accounting result is stronger than a latency claim.

## Concrete integration probe

`benchmarks/storage/tests/integration.rs` uses the coordinated `capnp` 0.25.6 and
`capnpc` generator with `field_api(true)`, rather than EAE's older stock-reader
dependency. Three tests run in both debug and release:

- A generated document updates through staged bytes and changed-word redo;
  its small edit creates a frame below 512 bytes. Metadata/publication CAS,
  checkpoint/reopen, schema mismatch, writer exclusion and a held snapshot are checked.
- Pre-I/O validation rejects root/nested capability pointers, malformed graphs,
  wrong schema, trailing bytes, exhausted capacity and traversal limits, without
  changing the log, generation or live image.
- Abandoned edits undo metadata/pointers/payload, and shrinking a document zeros
  the unused tail before committing.

Only validated deltas enter an EAE transaction. No mutable EAE slice is given to
our builder, so allocator/pointer writes cannot bypass undo/redo. This approach
still copies into a scratch builder and scans the fixed slot; the integration
probe itself is not the timed fixed-byte adapter. The latter deliberately omits
builder/graph overhead. No claim of ORM speedup follows from these numbers.

[Probe evidence](../../research/reports/eae-integration/verification.json) states the exact
boundary. There is no production backend switch, remote raw-write API, general
allocation/growth, retained publication history, subscription replay, persistent
capability graph or TLC refinement of EAE. Whole-entry Store recovery does have
new TLC/Rust checks; they do not prove this adapter.

## Next integration boundary

A useful optional backend would serve dense, bounded objects with frequent small
edits and few fresh snapshots. Before exposing it through ORM, implement staged
heads versus published versions, history floors, atomic bulk receipts, quotas and
revocation across restart. Variable-capacity objects need durable allocation and
reclamation, or an explicit exhaustion contract. Repeat matched benchmarks with
that actual ORM adapter and model/replay its transactions; review EAE's mmap/FFI
boundary for the supported platform. These are backend features, not blockers for
shipping the existing whole-entry Store preview.

Before allowing direct builder edits, define durable allocation/growth,
zero-initialization, reclamation, graph traversal limits and owner-pointer updates.
Every mutation must participate in undo/redo. Keep live capability hooks out of
storage; preserve explicit SturdyRefs and authority checks. Model and replay
begin/write/abort/commit/flush/checkpoint/crash/recover with visible versus durable
generations, uncertain outcomes, poisoned handles and held snapshots. EAE format
1 and RPROTO04 remain distinct formats, not an implicit migration.

To reproduce after preparing the pinned toolchain:

```sh
cargo test --test tooling eae_feasibility_probe -- --exact
python3 scripts/benchmark_storage.py
```

Run timing alone, on the filesystem you intend to evaluate. The scripts verify
the archive SHA-256 and extract it under `target/eae-benchmark`; they do not
modify `EAE-Reconstruction.zip` or add EAE to the runtime.

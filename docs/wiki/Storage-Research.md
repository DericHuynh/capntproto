# Storage and RPC performance: next steps

Research date: **2026-10-01 America/Edmonton**; the measurements began at
2026-10-02 00:14 UTC. This follows the [V5 component implementation](Component-Storage.md).

**Prioritize a bounded storage worker and group commit before another format
rewrite.** Components greatly reduce bytes written, but durable sync latency
still dominates this workload. Next, close the component API's transaction,
snapshot and retry gaps. Stable mappings and incremental metadata become the
next structural changes as retention and object counts grow.

This research snapshot predates the production worker. The
worker below is an isolated prototype in an example, not a production ORM
adapter. Recommendations are identified separately from measured behavior.

The subsequent [resilience research](Storage-Resilience.md) measures
fixed-rate overload, count/byte admission and deadlines, adds component process
crash coverage, and separates real writeback failures from the current recovery
test model.

The [bounded storage owner](Storage-Worker.md) is now implemented as an opt-in
host API. These earlier measurements remain frozen prototype results; they do
not measure the new worker's integrated RPC performance.

## Measurements at the recorded revision

The [probe](../../examples/storage_probe.rs) ran **36 fresh processes**, three
trials of twelve scenarios, serially in shuffled order. Files were created under
the workspace on NVMe-backed Btrfs with `compress=zstd:3`. `/tmp` is tmpfs on this
machine and was deliberately not used for durability timing. The CPU was an
AMD Ryzen 7 5800H with the `powersave` governor; the host was not isolated.

Each layout trial performs 128 edits to an 8-byte hot text value beside a total
of 64 KiB of cold text. Cold text is deterministic pseudorandom printable ASCII,
not a repeated character. It is still compressible. Whole-entry writes use one
typed Document containing both values. Component writes use the real typed ORM,
with one hot Document and the cold text divided among the remaining Documents.
Both paths have one atomic publication and file sync per edit. Seed time is
excluded. Ten thousand warm snapshot-acquisition-and-hot-field reads follow.

Table entries are medians of the three per-run statistics. They are **not**
pooled percentiles or statistically established performance guarantees.

| Layout, no held old snapshots | Logical bytes/edit | Commit p50, including encoding | Commit p99 | Warm acquire + hot read, mean | Warm reopen after history compaction |
| --- | ---: | ---: | ---: | ---: | ---: |
| Whole entry | 65,736 | 3.292 ms | 3.705 ms | 0.171 µs | 7.536 ms |
| 2 components | 216 | 2.741 ms | 3.216 ms | 0.208 µs | 2.948 ms |
| 16 components | 552 | 3.008 ms | 9.053 ms | 0.206 µs | 3.195 ms |
| 64 components | 1,704 | 3.049 ms | 6.379 ms | 0.211 µs | 3.521 ms |
| 256 components | 6,312 | 3.176 ms | 6.161 ms | 0.159 µs | 4.616 ms |

The two-component path has **304.33× fewer logical update bytes**, but only
about **1.20× lower median commit time** in this experiment. The whole-entry
baseline already supports reading its hot field without copying its cold data;
components do not automatically make a warm single-field read faster. Differences
among these sub-microsecond reads are too small and noisy to rank designs.
Component projections still avoid copying unrelated data into RPC replies,
which this local-read probe does not measure.

More components increase descriptor overhead: every update writes all component
descriptors, even when only one value changes. These typed messages append
`168 + 24 × component_count` bytes per edit. Splitting into 256 parts increases
update bytes 29.22× relative to two parts. This is an argument for splitting by
update/access pattern, not making every scalar a component. p99s varied widely:
the two-component p99 ranged from 3.110 to 5.893 ms across runs; the 16-component
p99 ranged from 6.086 to 9.121 ms. These short runs do not isolate CPU, device,
compression, filesystem or scheduling contributions.

### Blocking storage affects unrelated async work

A second experiment performs 256 raw two-component commits while a 1 ms Tokio
timer runs on the same current-thread executor. The direct version yields after
each commit. The prototype sends each write to one dedicated thread through a
bounded channel and awaits a reply after the ordinary Store commit completes.
There is only one request in flight, so this is an isolation probe, not a
concurrent throughput benchmark.

| Path | Commit p50 | Timer lateness p99 | Median of per-run maximum lateness |
| --- | ---: | ---: | ---: |
| Synchronous Store on executor | 2.998 ms | 8.827 ms | 15.351 ms |
| Dedicated Store owner thread | 2.988 ms | 1.715 ms | 1.722 ms |

The writer thread preserved commit latency while reducing interference with
the timer. These are timer measurements, not TCP/QUIC RPC tail latencies. The
timer skips missed ticks, and the sample counts differ. A network workload with
concurrent requests is the next acceptance benchmark.

### Amortizing syncs is a separate gain

The existing V4 `Store::commit` batches updates to distinct objects. For 256
total 8-byte object updates, with batches assembled before submission:

| Objects in each atomic batch | File syncs | Mean elapsed time per object update |
| --- | ---: | ---: |
| 1 | 256 | 2,845.6 µs |
| 4 | 64 | 743.3 µs |
| 16 | 16 | 174.6 µs |

This shows about **16.3× less amortized time per update** at batch size 16. It
does not mean a request completes in 174.6 µs: the median 16-object batch still
took 2.789 ms, and assembly/queueing delay was not included. An application
transaction and a group of independent transactions sharing a durability barrier
are different semantics. The current component API supports neither independent
group commit nor cross-object component batches.

### Held snapshots multiply mappings

Keeping the seed snapshot and all 128 new snapshots produced **129 mappings**
of the same file in both layouts. Total mapped virtual ranges were 10,543,104
bytes for two components and 551,473,152 bytes for whole entries. Without held
snapshots, one mapping remained: 94,208 and 8,482,816 bytes respectively.
Compaction temporarily made 130 mappings; releasing old snapshots returned the
count to one. This confirms the retention mechanism works, while exposing its
mapping overhead. These virtual-byte sums are **not RSS or independent physical
copies**; mappings can share filesystem-cache pages.

## Original recommendations and current follow-up

### 1. Bounded single-writer service, then group commit

The current `Store::append` writes, syncs, and maps the file synchronously;
`ComponentServer::commit` invokes it directly inside an async RPC method.
Give one persistent thread ownership of the Store. Keep generated RPC clients,
grants and other `Rc` state on the LocalSet. Send owned commands and return
revision results or immutable snapshots. Tokio recommends dedicated threads
for persistent blocking loops rather than occupying its blocking pool
indefinitely. [Tokio documentation](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

The original first API proposal was an explicitly owned service plus cloneable
handles, bounded by **queued bytes and command count**. The implemented
[storage worker](Storage-Worker.md) now provides that host boundary; ORM integration
and group commit remain open. The following criteria also guide that follow-up.
Report queue age, queued bytes, fsync duration, committed sequence and failures. Define cancellation at
admission: dropping a response cannot undo a write already accepted by the
writer. Define revocation and shutdown ordering before moving ORM calls across
an await; queueing must not silently weaken existing authority checks.

Then add group commit: validate requests in writer order, append separately
recoverable transaction records, perform one sync, publish the corresponding
in-memory state, and release all successful replies. Conflicts reject the
affected request without rejecting unrelated requests. A write/sync failure
quarantines the writer and marks affected admitted operations as uncertain.
Unacknowledged complete records can still survive recovery. Do not silently
turn a durability group into an all-or-none application transaction.

Start by draining already queued work up to byte/count bounds, without an
intentional delay. Later measure an optional maximum waiting budget. RocksDB's
documented group-commit strategy similarly combines queued writes to amortize a
sync; its documentation also explains why logical byte reductions need not
translate into proportionate device I/O reductions.
[RocksDB WAL performance](https://github.com/facebook/rocksdb/wiki/WAL-Performance).

Acceptance: mixed reads/writes over real TCP, TLS and QUIC, queue saturation,
revocation races, dropped replies, shutdown during sync, partial writes and
recovery. Measure p50/p99 request latency, maximum queue age and throughput at
several offered loads. Preserve synchronous Store access for simple local users.

### 2. Component snapshots, transactions and durable mutation receipts

The next features should make component storage practical for applications:

| Feature | Purpose | Required contract |
| --- | --- | --- |
| Snapshot capability / multi-get | Read several components at one revision; reuse cold data client-side | Server-bound component allowlist, one pinned root, bounded lifetime/resources |
| Typed multi-component RPC edits | Atomically update metadata and payload without resending unrelated parts | Heterogeneous typed envelopes, per-component authority, root CAS |
| Component history/change feed | Catch up after reconnect without transferring every cold value | Root revision and changed IDs, backpressure, explicit retention gaps; deletions need representation |
| Mutation receipts | Resolve a lost response without blindly applying an operation twice | Durable request identity, payload binding, result retention and authorized lookup |
| Cross-object component transaction | Commit application state, receipt and eventual outbox record together | One checked commit boundary and recovery of all members together |
| Resumable component bulk upload | Transfer large content without a monolithic RPC allocation | Staging quotas, content validation, atomic final reference/publication/receipt |

The existing bulk uploader already commits an object value and completion
receipt in one V4 batch. Generalize that principle. Proposed receipt keys contain
an authenticated client identity/incarnation plus request sequence or nonce;
bind the request's semantic content and expected revisions, and return the
recorded result for a valid duplicate. Reusing an identity with different content
must fail. Receipt lookup must not bypass current authorization. Persist a
retention/retry horizon and return an explicit expired/unknown outcome after it.

RIFL is useful research for associating retry metadata with the object it
protects, including migration and metadata collection. This is a design input,
not a claim that this runtime currently provides its semantics.
[RIFL paper](https://web.stanford.edu/~ouster/cgi-bin/papers/rifl.pdf).
An outbox can make state and intent atomic; external effects still require
destination-side deduplication or another protocol. It cannot make arbitrary
external actions exactly once by itself.

Prefer a returned snapshot capability with pipelinable typed facets over a
sequence of separate round trips to obtain revision, component and contents.
The runtime already supports promise pipelining, so this is API work.
[Cap'n Proto RPC](https://capnproto.org/rpc.html).

Keep root CAS as the default. Later offer explicit read-set validation for
applications needing disjoint component concurrency. Checking only the changed
component is insufficient when a decision also reads another component: two
disjoint writes can violate an invariant. Track absent/deleted components and
membership reads too. FoundationDB's conflict-range model illustrates why the
read set matters. [FoundationDB developer guide](https://apple.github.io/foundationdb/developer-guide.html#conflict-ranges).

### 3. Incremental realm metadata

`persistence::Core::commit` currently validates and JSON-serializes the entire
ledger for every durable change. Authorization also searches reference and
owner vectors. Changing JSON to another codec alone does not remove this work.

First add rebuilt in-memory indexes for token hashes and owner IDs without
changing the durable format. Then compare a typed mutation journal plus bounded
checkpoints against separate owner/reference records committed atomically with
their counters. Preserve realm identity, monotonic epochs/token serials,
revocation, expiration, duplicate-field rejection and recovery quarantine.
Benchmark 1, 64, 1,024 and 4,096 references before choosing the format.

Do not encode every reference as a component of one object: V5 permits only
256 components, while a realm permits up to 4,096 references. It would also
preserve a large manifest and a shared conflict domain. This is an entity/index
problem rather than a need for a more compact whole-ledger serializer.

### 4. Stable extents and persistent metadata when scale warrants them

The longer-term layout should separate frequently updated metadata from large
immutable values, with:

1. A bounded active append region and shared immutable payload extents.
2. Durable root/commit metadata identifying object revisions and their extents.
3. Immutable manifests or tree roots that snapshots retain without creating a
   new whole-file mapping per publication.
4. Garbage collection aware of retained history and outstanding snapshots.
5. A validated checkpoint/index to bound recovery work, with explicit policy for
   detecting corruption in data skipped by startup scanning.

WiscKey and RocksDB BlobDB provide primary examples of separating metadata from
large values and the garbage-collection obligations this creates.
[WiscKey](https://www.usenix.org/conference/fast16/technical-sessions/presentation/lu),
[BlobDB design](https://rocksdb.org/blog/2021/05/26/integrated-blob-db.html).
Our V5 sharing is within a single log; it does not yet keep cold blobs in
independent files through future compactions.

For a new layout, compare copy-on-write B+tree metadata against an LSM metadata
index and against a simpler validated checkpoint plus tail replay. B+tree roots
fit snapshot/range lookup needs; an LSM trades buffered writes for more merge,
cache and compaction policy. redb is a Rust reference design for immutable
tree pages and reclamation governed by live readers.
[redb design](https://github.com/cberner/redb/blob/master/docs/design.md).
This is not a recommendation to replace Store with redb or RocksDB without a
matched experiment preserving our publication and capability contracts.

Preserve contiguous Cap'n Proto segments. Arbitrary page splitting of a message
does not preserve its existing zero-copy reader. Keep each component contiguous,
or explicitly implement and validate a segment-aware reader. Packing/compression
requires decoding before ordinary mapped access, so make it a cold-data policy
with bounded decoded caches rather than a transparent promise of zero-copy.
[Cap'n Proto encoding](https://capnproto.org/encoding.html).

Do not pre-map unwritten mutable tails and expose them as immutable Rust slices.
One candidate uses owned immutable record buffers for the active region and
maps sealed extents, sharing those owners among snapshots. New files must be
durable before an acknowledged root references them; splitting payload and
metadata across files can introduce additional sync barriers. Garbage collection
must preserve every live root and reader. SQLite's WAL reader/checkpoint rules
are another useful illustration of the retention tradeoff, although our
generation-replacement mechanism differs. [SQLite WAL](https://sqlite.org/wal.html).

Before changing disk format, benchmark a sorted contiguous manifest in memory
against the current `BTreeMap`. With a 256-component bound, it may reduce
allocations and improve locality without changing V5. Disk-level manifest deltas
are a later choice: bound replay depth and keep live reads on resolved manifests.
New required record kinds or cross-file metadata need an explicit version and
migration plan; neither existing V4 nor V5 files should be silently rewritten.

## What to defer

Do not switch to a columnar representation for the primary mutable object API
without a scan/aggregation workload. Columnar projections can be separate
derived indexes later. Avoid broad automatic graph diffing, unbounded delta
chains, early 0-RTT mutation execution, multiwriter recovery, or weaker sync
semantics as shortcuts. Compression, checksum changes and io_uring should be
profile-driven experiments after the writer and queue architecture is measured.
The present evidence identifies filesystem barriers and scheduling interference;
it does not establish packet crypto or checksumming as the dominant cost.

## Reproduction and evidence

```sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
cargo build --locked -p capntproto --release --no-default-features --features storage --example storage_probe
mkdir -p target/storage-probe-data
cargo run --locked -p capntproto-dev -- probe storage --base target/storage-probe-data --output target/storage-probe-results --trials 3
cargo run --locked -p capntproto-dev -- probe summarize storage target/storage-probe-results
```

The output directory must be new. The runner uses `findmnt` and records platform,
filesystem options, compiler, binary/source hashes, exact invocations and case
order. It measures no competing builds or tests initiated by the runner; other
host activity and CPU frequency are uncontrolled. Raw files contain per-run
statistics rather than every individual latency sample. Warm reopen includes
the Store's file/directory recovery syncs; these results do not predict recovery
at millions of revisions, cold-cache lookup performance or deployment p99s.

- [Environment and source hashes](../../research/reports/storage-next/2026-10-01/environment.json)
- [All 36 runs](../../research/reports/storage-next/2026-10-01/runs.jsonl)
- [Medians and run ranges](../../research/reports/storage-next/2026-10-01/summary.json)
- [Serial runner](../../dev/src/probes.rs) and [summary tool](../../dev/src/probes.rs)

All trials completed their snapshot/content/reopen assertions. The research
example passed Clippy with warnings denied. Those checks validate the probe's
use of the recorded Store revision; they are not failure qualification of the proposed
storage service, group commit or new formats.

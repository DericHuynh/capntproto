# Storage performance, reliability and resilience

Research date: **2026-10-01 America/Edmonton**; measurements started
2026-10-02 00:37 UTC. This extends the [layout and scheduling research](Storage-Research.md).

**Build the storage service around bounded admission and explicit mutation
outcomes before adding automatic retries.** Larger queues absorb bursts but
increase latency and shutdown drain time. A deadline checked before execution
does not bound completion time. Component crash recovery now has additional
process-exit coverage; recovery from real device/writeback errors remains a
separate, unqualified failure model.

The measurements use a research prototype. Store and existing ORM handlers
remain synchronous; the later opt-in worker is described separately below.
Automatic mutation replay and replication remain unimplemented.

Implementation follow-up: the [bounded storage worker](Storage-Worker.md) now
provides an opt-in host API with size/principal budgets, cancellation outcomes
and shutdown. The measurements below remain evidence from the original research
prototype; ORM integration, durable receipts and group commit are still pending.

## Fixed-rate load and a stalled writer

The [example](../../examples/storage_resilience.rs) ran **27 fresh processes**:
three trials of eight load scenarios and a lost-reply scenario. Runs were serial
and shuffled on the same NVMe-backed Btrfs workspace (`compress=zstd:3`), with
an AMD Ryzen 7 5800H using the powersave governor. No competing builds/tests were
launched during measurement; other host activity was uncontrolled. `/tmp` is
tmpfs here and was not used for durability timing.

Each load run seeds an 8-byte hot component and a 64 KiB cold component. It
offers writes on a fixed schedule over one second, without waiting for replies
or retrying rejections. A dedicated thread owns the Store and executes ordinary
V5 commits with one file sync per write. It uses its current head for each CAS:
this measures admission/storage scheduling, not client conflict resolution.
Except for the baseline, the worker pauses for 100 ms before processing its
33rd dequeued request. This is a service stall, **not** a simulated fsync error.

Credits cover payload preparation, queued work and the executing request.
Both request count and payload bytes are bounded, and payloads are allocated
after credit reservation. The channel is otherwise an ordinary standard-library
channel. Credits do not account for total RSS, indexes, retained history,
snapshots, networking or kernel memory. Every run asserts outcome accounting,
credit release and limits, then reopens the file to check the exact final
revision, last committed value, publication and unchanged cold component.

Successful latency starts at the scheduled arrival time, including generator
lateness and queueing. Rejected and expired requests have separate counts and
latency distributions; they are not silently excluded from an apparent success
rate. Generator p99 lateness was at most 112 µs per run; its largest observed
lateness was 1.142 ms. These are short finite bursts, not sustained capacity or
TCP/QUIC RPC benchmarks. The reported quantile uses the sorted sample at
`ceil((n - 1) * q)`; for the 100-request baseline its p99 is the maximum.

Table entries are medians of three per-run statistics, not pooled percentiles.
The outstanding limit includes the executing request.

| Scenario | Offered writes | Outstanding limit | Committed, including drain | Rejected | Successful p50 | Successful p99 | Drain after the one-second window |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline, no pause | 100 | 8 | 100 | 0 | 3.14 ms | 6.25 ms | 0 ms |
| Steady load + pause | 100 | 8 | 97 | 3 | 3.18 ms | 103.68 ms | 0 ms |
| Overload + pause | 1,000 | 8 | 297 | 703 | 23.72 ms | 124.61 ms | 20.65 ms |
| Overload + pause | 1,000 | 64 | 355 | 645 | 190.71 ms | 292.51 ms | 189.42 ms |
| Overload + pause | 1,000 | 256 | 552 | 448 | 660.91 ms | 789.41 ms | 788.93 ms |

The larger limit completed more admitted requests **after arrivals stopped**;
552 is not a claim of 552 sustained writes/second. Median commit service time
remained near 3 ms. The limit-256 successful p99 ranged from 776.74 to 881.91 ms;
limit-8 ranged from 124.13 to 125.47 ms. Admission policy changes which requests
succeed, so lower successful p99 must always be read alongside rejection rate.
A rough queue-delay budget is queued work multiplied by service time; the
injected stall and filesystem tails add to that budget. No measured queue size
is being proposed as a universal default.

### A pre-execution deadline is not a completion deadline

With limit 64 and a 25 ms deadline measured from scheduled arrival, the worker
discards expired requests immediately before execution. Across the three runs:

- 61–62 requests were rejected at admission and 623–643 expired before execution.
- 295–315 writes committed; **278–300 committed after their deadline**.
- Only 15–24 writes per run both committed and finished within 25 ms.
- Median per-run successful p99 was 30.48 ms; drain was 24.74 ms.

The budget usually expires during a commit that started just before the deadline.
The prototype reports that outcome instead of pretending the write was rolled
back. Expired requests also wait for the stalled owner to resume before their
terminal outcome is recorded: median per-run queue-age p99 was 116.10 ms.
Thus this prototype does not provide prompt asynchronous timeout notification.
Admission should account for estimated queued service and leave a configurable
execution margin, while describing that estimate as a policy rather than a hard
disk deadline. A separate cancellation state machine must arbitrate cancellation
against the transition into execution.

### Byte bounds need fairness as well as memory limits

In mixed runs, every fourth offered write contains 64 KiB; the others contain
8 bytes. Large payloads are deterministic pseudorandom bytes. The outstanding
count limit remains 64 and all runs include the pause.

| Payload credit limit | Peak outstanding payload bytes | Large writes committed out of 250 offered | All writes committed | Successful p99 |
| --- | ---: | ---: | ---: | ---: |
| 4 MiB | 1,114,488 | 85 | 344 | 302.00 ms |
| 128 KiB | 66,040 | 8 | 359 | 265.41 ms |

At 128 KiB, two 64 KiB payloads cannot coexist with even one 8-byte payload.
Small requests can repeatedly occupy the remaining credits. All three tight-byte
runs committed only eight large requests. This demonstrates size bias in simple
first-come try-admission; it does not prove indefinite starvation. Reserve
bounded service for large requests or use weighted per-principal queues, and
report acceptance/latency by size class. Requests larger than the entire budget
must fail explicitly or use a chunked API, never wait for impossible credit.

Google's overload guidance likewise treats resource usage, customer quotas,
criticality and client throttling as separate controls; a request-rate limit
alone does not describe variable request cost.
[Primary source](https://sre.google/sre-book/handling-overload/).

## Reliability evidence and its boundary

The new [component crash tests](../../src/storage/components/crash_tests.rs) extend
the existing isolated storage recovery gate. They atomically change two
components while retaining a shared 64 KiB component from the previous revision.
Each scenario exits the child without Rust destructors, then exits another child
immediately after recovery's file/directory barriers, then opens a third time.

| First crash point | Checked result after the second crash and final open |
| --- | --- |
| Partial append | Entire old revision and old publication; no partial component edit |
| Complete append before sync | Entire new revision recovered and synchronized |
| Append after sync | Entire new revision |
| Compaction temporary file after sync, before rename | Entire new revision through the original generation |
| Compaction after rename, before directory sync | Entire new revision through the selected checkpoint |
| Compaction after directory sync | Entire new revision through the checkpoint |
| Commit returned successfully | Entire acknowledged revision |

Every scenario also verifies both changed components and the unchanged shared
bytes. The compaction cases exercise references whose original revision may be
removed and whose data must be rebased. The isolated gate passed **11 tests**,
including existing V4 framing, I/O faults, revocation and crash tests. These
tests check logical crash paths with an intact kernel/filesystem; the pre-sync
append remaining in the kernel cache is expected. They are not power-loss tests.

### Real writeback errors are a different recovery problem

The USENIX ATC 2020 study *Can Applications Recover from fsync Failures?* found
that, in its tested filesystem/kernel configurations, failed writeback could
leave pages marked clean; another successful fsync did not necessarily repair
missing disk data. Cache-visible recovery could therefore disagree with later
on-disk state. This is historical experimental evidence, not a reproduction on
this machine's current kernel.
[Paper](https://www.usenix.org/system/files/atc20-rebello.pdf).

Our [I/O fault seam](../../src/storage/io.rs) runs the injected error **before**
`File::sync_all()`. Existing tests establish how the Store reacts to that error,
not what a filesystem does after an actual failed writeback. Recovery maps and
validates the file, then syncs the file and directory. That sequence is useful
under the documented durability contract but is not sufficient evidence for
automatic recovery from every real EIO. This is a qualification gap; this work
did not reproduce lost data from a real device failure.

The proposed service should latch a degraded state after media/writeback errors,
stop mutation admission and retain diagnostics rather than automatically cycling
through close/open until one call succeeds. Repair needs an explicitly validated
recovery procedure and, where necessary, a trusted backup or replica. Checksums
detect damaged bytes; they do not reconstruct them or detect an otherwise-valid
rollback. A persistent incident marker alone is not reliable when written onto
the same failing device.

### A worker does not isolate every storage failure

The production API still returns borrowed views over `Mmap`. Cold page faults
can block the reader, and an I/O error during a mapped access can terminate the
process instead of becoming a normal Rust storage error. SQLite documents this
failure boundary for its own mmap path.
[SQLite mmap documentation](https://www.sqlite.org/mmap.html).

Measure cold reads and page-fault latency before assuming write isolation also
isolates reads. Possible later designs include bounded fallible read buffers or
a storage process; neither preserves every borrowed-view property for free.
Do not add an unsafe signal-recovery workaround to the current API.

Nor can an ordinary cancellation token interrupt an already-running fsync.
Tokio documents that started `spawn_blocking` work cannot be aborted and that a
shutdown timeout stops waiting without stopping that work.
[Tokio documentation](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).
A dedicated owner is appropriate for the long-lived Store, but a stuck owner
still requires admission closure and an explicit shutdown outcome. A separate
process may improve fault containment; even process termination is not a hard
completion bound for uninterruptible kernel I/O.

## Mutation outcomes and retry ergonomics

The lost-reply experiment drops its receiver before releasing the writer to
execute. All three trials still committed the write, failed to deliver the reply
and recovered the new revision. This is behavior of the prototype's command
protocol, not a newly introduced RPC cancellation API. It illustrates why losing
interest in a reply is not evidence that a mutation failed.

Proposed service outcomes should carry enough information for this distinction:

| Outcome | What the caller may conclude | Retry behavior |
| --- | --- | --- |
| Admission rejected | This attempt was not executed | Back off within a bounded budget |
| Queued cancellation/expiry confirmed by owner | This attempt did not enter execution | A new attempt is possible if still useful |
| Permission or CAS rejected before I/O | No mutation from this attempt | Reauthorize or resolve conflict explicitly |
| Commit acknowledged | Commit passed its required durability barrier | Return the recorded result |
| Reply lost, execution started, or ambiguous post-I/O error | Mutation may have committed | Query/retry by the same durable operation identity |
| Media/writeback failure | Integrity/availability incident, not ordinary overload | Quarantine and qualified recovery |

A local timeout alone cannot distinguish the middle states. Validate request
identity, authorization, generation and CAS on the owner at execution, since they
may change while queued. A revocation result must not be followed by a queued
write that was authorized only at admission. Cancellation/expiry confirmation
must win an atomic state transition against starting execution.

Durable receipts should bind principal, object incarnation, operation ID and
payload digest to the result. Record the receipt with the mutation atomically;
reject reuse of an ID with different contents. The existing
[V4 durable bulk completion](../../src/durable_bulk.rs) already commits a value and
receipt together and is a useful local precedent. V5 still needs an equivalent
transaction/receipt boundary. Root CAS alone cannot tell a retry whether its
earlier attempt succeeded when another writer has since advanced the revision.
Define a bounded receipt retention window and make an expired receipt an explicit
unknown outcome; do not silently execute it as a new operation.

Raft's client-interaction section also distinguishes replicated commit from
client deduplication: a lost reply can cause replay unless the state machine
retains request identity and its response. Adding replication later does not
remove this API requirement.
[Raft, section 8](https://raft.github.io/raft.pdf).

The existing [RetryingConnector](../../src/native_rpc/policy.rs) retries classified
connection setup failures with capped exponential delays, not application
mutations. Its delays are deterministic and it has no shared per-peer retry
budget. The next connection experiment should add injectable jitter and a
shared retry/admission budget, preserving deterministic tests and the total
setup deadline. Keep retries at one chosen layer. AWS's guidance explains why
bounded backoff with jitter and idempotency checks matter, and why retrying at
multiple layers can amplify overload.
[AWS guidance](https://docs.aws.amazon.com/wellarchitected/2024-06-27/framework/rel_mitigate_interaction_failure_limit_retries.html).

## Implementation order and acceptance evidence

1. **Bounded storage service.** Own the Store on a dedicated thread; reserve
   count and byte credits before copying payloads; keep credits through execution.
   Enforce per-principal limits and size fairness. Bound waiting submitters and
   pending replies too. Reserve admission for revocation/shutdown/control work,
   while preserving object operation order; a priority queue cannot preempt an
   active disk sync. Expose queue age, bytes, rejected/expired/uncertain counts,
   commit latency, last durable progress and shutdown drain.
2. **Outcome and receipt contract.** Implement owner-arbitrated queued
   cancellation and deadline expiry, commit/result lookup and durable receipts.
   Test cancellation versus start, reply loss, duplicate IDs, conflict, revocation
   while queued and shutdown during execution. Queue admission must not count as
   durable success. Draining shutdown must distinguish confirmed commits from
   abandoned queued work and uncertain in-flight work.
3. **Group commit with bounded delay.** Preserve each transaction's validation
   and atomicity while sharing a durability barrier. Bound group bytes, age and
   member count. A barrier failure makes affected operations uncertain; it must
   not produce success replies. Measure end-to-end tails under load, including
   sparse traffic and mixed request sizes, before selecting group defaults.
4. **Storage failure qualification and restore tooling.** Use disposable VM or
   loopback filesystems to inject block-level writeback errors, full-volume
   conditions, read-only transitions and crashes around rename/directory sync.
   Keep the acknowledgement oracle outside the faulted volume. Verify again
   after cache eviction/remount or VM power cycle. Record filesystem, kernel,
   mount options and device flush behavior. Do not inject block faults into the
   developer workspace. Add an offline backup manifest covering all application
   stores/realm files, hashes, revisions and restore lineage, then exercise
   restoration and reference/revocation policy before reopening admission.
5. **Long-lived RPC and recovery workloads.** Run the same fixed offered load
   through TCP and quiche QUIC with slow peers, disconnects and reconnect storms.
   Include compaction, pinned old snapshots, cold reads and bounded-memory soaks.
   Measure file growth, mappings, retained generations, recovery duration and
   actual device I/O separately from logical append bytes. Do not compare only
   successful-request latency when one run drops more work.

Replication/failover should follow these boundaries. Local file locks and
transport authentication do not select a unique distributed writer. Before
automatic ownership transfer, define durable epochs checked at mutation, replica
commit/read consistency, receipt replication and behavior of old capabilities.
An old backup can restore revoked authority or replay state; it needs a lineage
decision, not just a valid checksum. These are prerequisites for a future actor
host, without introducing an actor system into this workspace.

## Reproduction and evidence

```sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
cargo build --locked -p capntproto --release --example storage_resilience
python3 scripts/storage_resilience.py --base target --output target/storage-resilience-results --trials 3
python3 scripts/storage_resilience.py --summarize target/storage-resilience-results
cargo nextest run --locked -p capntproto --test tooling storage_crashes_in_isolated_process -- --exact
```

Choose an existing `--base` directory on the filesystem to measure and a new
output directory. The runner stores exact invocations, source/binary hashes,
filesystem and compiler details. Its 180-second subprocess limit is only a harness
watchdog, not the storage service's cancellation contract. Original evidence is
about **54 KB**, containing per-run distributions/counters rather than individual
request samples:

- [Environment and source hashes](../../research/reports/storage-resilience/2026-10-01/environment.json)
- [All 27 runs](../../research/reports/storage-resilience/2026-10-01/runs.jsonl)
- [Medians and ranges](../../research/reports/storage-resilience/2026-10-01/summary.json)
- [Runner and deterministic summary](../../scripts/storage_resilience.py)

All runs passed their credit/accounting/content/reopen assertions. Validation
also passed the 13 component unit tests, the 11-test isolated recovery gate,
Clippy for the package's library/tests/examples with warnings denied, and the
deterministic source-bundle round trip including both frozen research datasets.
Source/binary hashes, regenerated summaries and local research links were checked.
These measurements inform the service design; they do not close production
storage qualification or establish hardware durability, fairness, network
latency or a sustained throughput guarantee.

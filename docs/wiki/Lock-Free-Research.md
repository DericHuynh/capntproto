# Lock-free and reduced-coordination research

Research date: 2026-10-01, America/Edmonton (measurements on October 2 UTC).

The best next experiment is an **immutable published read view** with `ArcSwap`,
while keeping the storage owner responsible for writes and their durability.
A bounded atomic queue is also promising for heavily contended submission, but
replacing the worker's mutex requires a lifecycle design, not just a queue swap.
For durable write throughput, group commit remains the more direct opportunity.
These are recommendations from the audit and probes below, not implemented
runtime changes or claims of production speedup.

## What currently shares state

The [storage worker](../../src/storage/worker.rs) already gives one thread exclusive
ownership of `Store`. Its mutex is **not held over filesystem I/O, payload copying
or an async wait**. The condition variable releases it while the owner sleeps.

| Shared operation | Current purpose | Candidate improvement |
| --- | --- | --- |
| Reserve credits | Atomically check size-pool and principal count/byte limits | Resolve principal accounts once; model checked atomic reservations with rollback |
| Final submission | Order the phase check and queue insertion against shutdown | Short admission gate plus bounded queue; explicit publication/close protocol |
| Pop and start | Order shutdown against cancellation and execution start | Keep a start gate, or prove an equivalent atomic state machine |
| Completion and credit return | Quarantine before replying; charge unread replies until consumption/drop | Keep correctness state coordinated; separate observational counters only if semantics permit |
| Diagnostics | Coherent usage, queue, execution and lifecycle snapshot | Publish immutable telemetry if slightly stale observations are acceptable |
| Snapshot acquisition | Owner looks up an immutable mapping and returns it | Publish a coherent immutable index/root for authorized readers |

There are multiple lock acquisitions per request, not one. `BTreeMap` principal
insertion/removal, possible `VecDeque` growth and the boxed job allocation also
occur under the lock. Moving allocations out of the critical section and reserving
bounded queue space are simpler candidates to measure before restructuring it.
`Pending::cancel` already uses an atomic compare-and-exchange.

This is primarily a storage concurrency opportunity. The [native RPC tables](../../src/native_rpc.rs),
[quiche connection state](../../src/rpc/quic.rs) and [ORM](../../src/orm.rs) use local
`Rc`/`RefCell` ownership. They do not have a shared mutex to replace with a
concurrent map. Moving these tables across threads would change ownership and
capability-lifecycle assumptions. Partitioning independent local runtimes and
communicating through bounded messages is a better starting point for that work.
No virtual actor runtime is needed for these changes.

## Local measurements

The isolated [probe crate](../../benchmarks/concurrency) has its own lockfile and
adds **no production dependencies**. It compares `std` and `parking_lot` mutexes,
Crossbeam `ArrayQueue`, Tokio bounded MPSC, and `ArcSwap` reads. The
[runner](../../scripts/concurrency_probe.py) recorded **84 serial runs: 28 scenarios,
three shuffled fresh-process trials each**. Every run passed its checks: exact
message counts and per-producer FIFO, coherent snapshot contents, or successful
worker outcomes plus reopening and checking durable contents.

Evidence: [environment and hashes](../../research/reports/concurrency/2026-10-01/environment.json),
[individual runs](../../research/reports/concurrency/2026-10-01/runs.jsonl),
[summary](../../research/reports/concurrency/2026-10-01/summary.json), about 86 KB total.
Host: Ryzen 7 5800H, Linux, powersave governor, Btrfs on NVMe with zstd compression.
The host was shared and not CPU-pinned. These are exploratory measurements without
confidence intervals, warmup exclusion or CI timing thresholds.

### Bounded queues

65,536 messages, capacity 64, one consumer, either one or eight producers.
All variants retry with `thread::yield_now()` on full and yield on empty.
Consumer batches have a maximum of either one or 16 messages. Mutex queues take
one lock per batch; ArrayQueue and Tokio call their individual nonblocking receive
operations repeatedly. This **does not measure Tokio's `recv_many` implementation**.

Median elapsed ns/message, with min–max across three runs in parentheses:

| Queue | 1 producer, batch 1 | 8 producers, batch 1 | 8 producers, batch 16 |
| --- | ---: | ---: | ---: |
| `std::Mutex<VecDeque>` | 104.6 (104.0–108.7) | 523.8 (523.7–534.9) | 142.4 (139.7–146.5) |
| `parking_lot::Mutex<VecDeque>` | 82.4 (80.9–92.7) | 488.4 (464.3–605.0) | 170.0 (126.4–176.5) |
| `crossbeam_queue::ArrayQueue` | 49.8 (47.2–58.5) | 48.3 (46.6–56.0) | 46.6 (43.1–48.4) |
| `tokio::sync::mpsc` | 117.1 (114.1–126.0) | 174.8 (167.6–181.3) | 169.5 (166.1–171.8) |

At eight producers/batch 1, ArrayQueue's median throughput was about 10.8× the
simple mutex queue's. Their sampled p99 arrival-to-consumption latencies were
23.0 µs and 87.7 µs, respectively. Queue latency samples start at the **first
enqueue attempt**, including full-queue retries, for every 64th message per
producer. Mutex batching reduced elapsed ns/message by about 3.7× at eight
producers. A cheaper mutex alone was not a consistent improvement over batching.

These are saturation probes with tiny messages, no disk, no reply channel,
no admission accounting, no cancellation and no shutdown protocol. They check
per-producer order, not a proof of global linearizability. The busy-yield policy
is deliberately identical but is unsuitable for an idle storage worker. Raw
results include retry/empty-poll counts and process CPU time; CPU time includes
setup and validation and must not be divided by the timed-region duration as an
exact utilization measurement. Real parking, wakeup and burst behavior remain
unmeasured. Do not subtract these queue costs from production worker latency.

### Immutable shared reads

262,144 total reads of one immutable 72-byte state. One writer publishes a new
generation and sleeps 100 µs between publications. Mutex/RwLock readers clone
the protected `Arc`, release the lock and read it; ArcSwap uses either a short
guard or an owned `Arc`. Each read checks all fields belong to the same generation.

| Reader | 1 reader: elapsed ns/read | 8 readers: elapsed ns/read |
| --- | ---: | ---: |
| `Mutex<Arc<State>>` | 11.8 (10.6–11.8) | 71.8 (65.6–75.1) |
| `RwLock<Arc<State>>` | 9.2 (8.7–9.4) | 85.7 (85.7–87.7) |
| `ArcSwap::load()` guard | 8.5 (7.8–9.5) | 2.8 (1.7–2.9) |
| `ArcSwap::load_full()` owned | 8.4 (8.2–9.0) | 36.8 (35.9–37.4) |

These numbers are **inverse aggregate throughput**, not per-reader latency.
For eight readers the short guard achieved about 25× the mutex throughput;
the owned-Arc improvement was about 2×. Returning an owned snapshot needs the
second comparison. Guard acquisition is not equivalent to transferring an `Arc`
to an application. The fastest cases lasted under a millisecond and saw only
3–5 publications; counts are recorded. This identifies a candidate, not its
performance under frequent updates, a large index, long-held guards or cold mmap
faults. Per-read latency was not sampled (zero latency fields mean unmeasured).

### Current storage worker

Public APIs, default admission budgets, one outstanding request per producer,
each with its own principal/object. Status probes read an empty object 16,384
times. Write probes perform 128 V5 component edits of an eight-byte value, each
with root CAS and publication in one real durability barrier. Setup, shutdown
and reopen validation are outside the timed region. No requests were rejected.

| Operation | Producers | Median elapsed/update or query | Median per-run reply p99 |
| --- | ---: | ---: | ---: |
| Status | 1 | 16.73 µs (16.40–17.90) | 24.31 µs |
| Status | 8 | 4.86 µs (4.83–5.04) | 78.36 µs |
| Durable component edit | 1 | 3.017 ms (2.860–3.266) | 3.706 ms |
| Durable component edit | 8 | 3.001 ms (2.798–3.480) | 25.272 ms |

This is a closed-loop baseline, not the fixed-rate overload experiment in the
[resilience research](Storage-Resilience.md). More callers did not
materially improve this single-owner write throughput. There is no measured
lock-wait/fsync-time breakdown, so this does not quantify the mutex's contribution.
The earlier [batch experiment](Storage-Research.md) and these millisecond writes
make group commit a stronger write-path hypothesis than queue replacement alone.
Status latency gives a concrete reason to investigate an authorized read view.

## Which designs fit

### 1. Publish immutable views selectively

ArcSwap provides lock-free reads and distinguishes short borrowed guards from
owned loads; the latter contend on the `Arc` reference count. Its documentation
also warns that writes and hardware characteristics matter. Use a short `load()`
for local lookups and `load_full()` when a result must outlive that lookup.
[ArcSwap performance](https://docs.rs/arc-swap/1.9.2/arc_swap/docs/performance/index.html).

Proposed project design, not an existing API:

- The owner constructs and publishes a **single coherent root** after durable
  success and before acknowledging it. Multi-object commits cannot publish each
  object independently if readers are promised one atomic transaction view.
- Start with a small immutable metadata/read index and structural sharing; do
  not clone the complete object/history index on every edit without measurement.
  Decide explicitly whether reads promise latest committed, latest published,
  or a caller-pinned generation.
- Put availability/health and generation in the same published state. Publish
  the degraded state before an uncertain error reply. Define the ordering for
  readers already holding an older view; quarantine cannot retract an existing
  snapshot. A bypass path must not silently evade health checks.
- Preserve capability checks and revocation ordering. A worker principal ID
  identifies a quota, not authority to read. Integrate with ORM grants before
  exposing a read view through RPC.
- Bound retained generations and document that old snapshots can retain maps,
  files and path locks. Cold page faults can still block the reading thread.

One load should cover related field accesses: repeated loads can combine values
from different generations. Avoid storing many short guards or holding them
across an await; acquire an owned snapshot for such use.
[ArcSwap API and consistency](https://docs.rs/arc-swap/latest/arc_swap/struct.ArcSwapAny.html).

Diagnostics are a simpler first integration target if callers accept explicitly
stale telemetry. Such a published view must not become the source of truth for
admission enforcement. The current diagnostics contract provides a coherent
locked snapshot, so changing that requires an explicit API decision.

### 2. Separate queue mechanics from admission and lifecycle

`ArrayQueue` allocates bounded storage up front and returns the unsent value when
full. It is the strongest measured queue candidate here, but provides neither
async wakeups nor the worker's quota/lifecycle policy. Never use `force_push`:
overwriting older jobs is incompatible with reliable request completion.
[Crossbeam API](https://docs.rs/crossbeam-queue/0.3.14/crossbeam_queue/struct.ArrayQueue.html).

Tokio bounded MPSC is already a production dependency and supplies receive
wakeup/parking machinery. It is not an entirely lock-free primitive: the bounded
channel's semaphore has a mutex-protected waiter list.
[Tokio semaphore source](https://docs.rs/tokio/latest/src/tokio/sync/batch_semaphore.rs.html).
Closing a receiver also allows permits acquired earlier to send; draining waits
for those permits to be released. That differs from this worker's final phase
check after preparation. A sender-held permit could otherwise delay drain for
as long as the sender retains it.
[Tokio close/receive contract](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.Receiver.html#method.close).

The queue's capacity is not the worker's outstanding-request budget: the latter
also covers payload preparation, execution and unread replies. Keep a separate
RAII credit permit. A safe migration must prevent this schedule:

1. Producer checks that the worker is running, reserves credit and prepares data.
2. Shutdown seals admission; the consumer sees an empty queue and exits.
3. Producer publishes its job and returns successful admission.

An atomic phase flag plus an unrelated queue does not exclude this stranded job.
Use a publication/close gate or a proved protocol with in-flight publishers,
rollback and a wakeup fence. Also order `CancelQueued` against each execution
start. Bulk dequeue must not mark all prefetched jobs started: those jobs still
need cancellation, deadline, diagnostics and shutdown handling until execution.
Dequeue batching is separate from group commit and does not share fsync by itself.

### 3. Reduce sharing before adding atomic counters everywhere

Principal accounts could be resolved when a client is constructed, avoiding a
tree lookup on every request and credit return. Repeated IDs must resolve to the
same account and idle accounts must remain reclaimable without unbounded registry
growth. This needs a lifecycle design even if the counters use atomics.

Global small/large count and byte limits plus principal count and byte limits are
a compound reservation. Independent `fetch_add` calls followed by refunds expose
overshoot and overflow hazards. Checked CAS reservations with RAII rollback are
possible, but transient partial reservations can reject otherwise admissible
work. Specify fairness and observability; do not assume portable 128-bit atomics
or that a packed integer can represent every existing `usize` budget. Hot global
atomic cache lines can still contend. Local credit caches only preserve a hard
global bound if their allowances are partitioned or reserved from it.

`parking_lot` is a useful lower-complexity comparator, not a lock-free design.
The measured gains do not justify a blanket replacement. Its mutex differs from
`std`, including poisoning behavior, which matters to an owner-panic review.
[parking_lot](https://docs.rs/parking_lot/latest/parking_lot/).

Independent Store owners can remove shared coordination and parallelize I/O.
This introduces sharding semantics: the current atomic batch does not span
independent Stores. Per-producer SPSC queues likewise need bounded registration
and a merge policy; they do not automatically preserve the current global FIFO
enqueue order. Both are architectural choices, not transparent substitutions.

### 4. Keep progress and memory safety claims precise

An API named `try_*` is not a wait-free guarantee. Lock-free progress means some
operation can finish despite delayed participants; wait-free progress additionally
bounds each participant's work. Neither promises bounded fsync, allocator, page
fault or scheduler latency. The classic Michael–Scott queue paper distinguishes
absence of locks from actual progress guarantees.
[Original paper](https://www.cs.rochester.edu/~scott/papers/1996_PODC_queues.pdf).

The tested ArrayQueue source reserves a tail position before publishing its slot
stamp, and receivers retry while awaiting publication. A paused producer in that
interval can delay consumption. This source-level observation is why this report
calls it a **bounded atomic queue**, not a proof of a wait-free storage pipeline.
Tests here did not inject a pause inside the queue implementation.
[ArrayQueue source](https://docs.rs/crossbeam-queue/latest/src/crossbeam_queue/array_queue.rs.html).

Custom `AtomicPtr` swaps need safe reclamation; an atomic pointer alone cannot
protect a reader from freed memory. Epoch reclamation defers destruction until
relevant pinned readers finish, so long-held pins can retain memory. This project
already needs to account for retained mapped generations; another reclamation
scheme would add complexity. Prefer vetted ownership primitives over a custom
unsafe queue or snapshot pointer.
[Crossbeam epoch](https://docs.rs/crossbeam-epoch/latest/crossbeam_epoch/).

Do not implement a seqlock by racing ordinary Rust field reads/writes and checking
a version afterward. Retrying does not make a data race defined. A suitable safe
implementation or atomic fields are necessary, and a stalled writer can still
make readers retry.
[Rust's data-race rules](https://doc.rust-lang.org/nomicon/races.html).

## Implementation order and acceptance evidence

1. Measure a real read-heavy ORM/RPC workload, including authorization, mmap
   access and updates. Prototype a small immutable read view and an owned-snapshot
   path; record read p99, update cost, retained bytes and maximum generation age.
2. Continue bounded group commit and durable receipts for writes. Retain the
   owner's durability and uncertain-outcome rules; do not weaken acknowledgements
   to improve benchmark numbers.
3. If submission contention remains material, compare the existing gate with
   preallocation, batched dequeue, ArrayQueue plus correct parking, and Tokio
   MPSC. Include idle CPU, executor latency, fixed-rate overload, paused producers,
   many principals and sustained load, not just saturated message transfer.
4. Before changing synchronization, model admission/close/publication/start races
   and run a bounded interleaving test (for example Loom) for credit conservation,
   start/cancel arbitration and no lost wakeups. Retain all existing worker tests,
   including unread replies, shutdown during preparation, panic after durable
   commit and I/O quarantine. A model must represent the custom atomic protocol;
   merely wrapping an opaque third-party queue does not check its internals.

The research probes do not replace these acceptance tests. No production queue,
read API, storage format, quota or durability behavior was changed by this work.

## Reproduce

Use the repository Rust toolchain. The standalone crate avoids adding experimental
dependencies to the main lockfile. Run builds before measurements, with no other
build/test workload in parallel. Use a real storage filesystem for `--base`;
`/tmp` on this machine is tmpfs.

```sh
CARGO_TARGET_DIR="$PWD/target/concurrency-research" \
  cargo build --release --locked --manifest-path benchmarks/concurrency/Cargo.toml
python3 scripts/concurrency_probe.py --base target \
  --output target/concurrency-local
CARGO_TARGET_DIR="$PWD/target/concurrency-research" \
  cargo clippy --locked --manifest-path benchmarks/concurrency/Cargo.toml \
  --all-targets -- -D warnings
python3 scripts/concurrency_probe.py \
  --summarize research/reports/concurrency/2026-10-01
```

Measurement output must be a new directory. Summarization rewrites only the
derived summary. The checked-in evidence is frozen; new experiments belong in a
new directory. Source/binary hashes identify the measured dirty checkout more
precisely than Git HEAD alone. Hashes cover the probe, lockfiles and central storage
sources, not every transitive path-dependency file.

# Bounded asynchronous storage

`storage::worker::Worker` owns one Store on a dedicated thread. Opening,
recovery, mutations, publication and compaction execute there, allowing an
async executor to continue handling other work while storage blocks. It supports
both the whole-entry V4 and component V5 formats without changing their record
or durability contracts. This is an opt-in API enabled by the `storage` feature.

The [lock-free research](Lock-Free-Research.md) audits synchronization and compares
bounded queues, immutable read views and the current worker. Its measurements are
research evidence; the worker retains its coordinated admission/lifecycle gate.

The existing `Store`, `ObjectState`, `ComponentState` and their RPC servers retain
their synchronous behavior. This worker is a **privileged local host API**;
quota identities are not capability grants. An ORM/RPC adapter with execution-time
authorization, schema reservations and revocation ordering is separate work.

## Use

```rust
use reproto::storage::{Limits, ObjectKey, Revision};
use reproto::storage::worker::{Config, Format, Options, ShutdownMode, Worker};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let owner = Worker::open(
        dir.path().join("objects"),
        Format::WholeEntry,
        Limits::default(),
        Config::default(),
    ).await?;
    let client = owner.client(7); // Trusted host-selected principal, shared by clones.
    let pending = client.try_put(
        ObjectKey::new(1), Revision::INITIAL, b"draft", Options::default(),
    )?; // Bounded admission, not a durable acknowledgement.
    let revision = pending.await?; // Existing Store sync completed successfully.
    client.try_publish(
        ObjectKey::new(1), revision, Revision::INITIAL, Options::default(),
    )?.await?;
    let report = owner.shutdown(ShutdownMode::Drain).wait().await;
    assert!(!report.degraded);
    Ok(())
}
```

`try_commit` submits an atomic whole-entry batch of up to sixteen objects.
`try_edit_components` submits an atomic component edit, optionally publishing
its new root. Borrowed input is copied before submission returns, so temporary
input buffers can be released before awaiting the result. CAS comparisons occur
on the owner during execution. Concurrent callers observe FIFO **enqueue**
order; payload preparation may finish in a different order than invocations.

The client also exposes snapshot acquisition, explicit revision reads, status,
publication cursors and one-at-a-time history reads for both layouts, retention
compaction and storage-limit changes. There is no arbitrary closure execution
API or implicit retry. `Worker::from_store` moves an already-open Store to the
owner; use `Worker::open` when recovery must also happen off the async executor.

The runnable [component example](../../examples/async_storage.rs) uses raw component
bytes and a fresh temporary file:

```sh
cargo run --locked --no-default-features --features storage --example async_storage
```

## Admission and memory accounting

Submission reserves credits before copying payloads. Requests never wait for
admission with an owned payload. `AdmissionError` is immediate and means that
this attempt was not enqueued or executed. Invalid shapes, an impossible payload
size, temporary exhaustion, closed admission and degraded storage are distinct.
Repeated overload retries need caller-side backoff and a bounded attempt budget.

Default policy:

| Pool | Classification | Outstanding requests | Outstanding payload bytes |
| --- | --- | ---: | ---: |
| Small | Total submitted payload ≤ 4 KiB, including payload-free reads/control | 48 | 1 MiB |
| Large | Total submitted payload > 4 KiB | 16 | 32 MiB |
| Per principal | Aggregate across both pools | 8 | 16 MiB |

These are configurable policy defaults, not measured deployment capacity. Size
pools never borrow each other's allowance. Flooding either class therefore cannot
consume the other class's reserved slots or bytes. Per-principal allowances apply
to all clones and clients created with the same ID. The host must map identities
consistently; a remote caller cannot be allowed to bypass quotas by choosing IDs.
Idle principal accounting is removed when its last credit is released.

The tradeoff is that capacity can sit idle in one pool while the other rejects
requests. The FIFO owner preserves operation order across pools. This provides
resource isolation, not weighted execution fairness, guaranteed admission for
every principal, or preemption of an active storage operation. A long compaction
can still delay all requests.

Credits cover preparation, queued work, execution and **unread completed replies**.
They are released when the result is consumed/dropped, or when an abandoned
queued/executing request is retired. A cancelled queued payload keeps its credit
until the owner removes it. Keeping many unpolled results intentionally causes
admission to reject more work rather than creating an unbounded reply backlog.

The byte budget counts input payloads, not total RSS. Command metadata is bounded
by request count and the batch/component count limits. Caller-owned input,
serialization work, Store encoding scratch, indexes, mappings and snapshots
retained after consuming a reply are outside the payload budget. Existing Store
file/entry limits still apply and can reject an admitted request during execution.

## Cancellation, deadlines and outcomes

`Pending<T>` is an awaitable result. `cancel()` atomically competes with the
owner's transition from queued to started:

- `Cancellation::Cancelled` confirms this request will not execute. Its eventual
  result is `Error::Cancelled`; resources are reclaimed when it is dequeued.
- `Cancellation::TooLate` makes no non-execution promise. Await the actual result.
- Dropping `Pending` attempts the same queued cancellation. A started request
  continues even when its receiver disappears.

`Options::deadline` is a local monotonic `Instant` checked immediately before
starting. An expired queued request returns `Error::Expired` without execution.
Expiry and queued cancellation have one atomic winner. An executing request can
complete after its deadline; the worker does not relabel a committed write as
expired. Neither a caller timeout nor dropping a future interrupts fsync or
rolls back a mutation. Cancellation and shutdown start decisions are ordered
against owner selection before executing the storage operation.

| Result | Contract |
| --- | --- |
| Successful mutation | The ordinary Store operation and its required durability barriers completed |
| `NotApplied(storage_error)` | Validation/CAS/layout/quota failure, without an uncertain I/O outcome |
| `Cancelled` / `Expired` | Request did not enter execution |
| `Unavailable` | Owner shutdown/failure prevented this queued request from executing |
| `Uncertain(storage_error)` | I/O failed or the Store became poisoned; this attempt may have taken effect |
| `OwnerLost` | Owner unwound during execution; this attempt may have taken effect |

I/O errors conservatively latch the worker's degraded state, including errors
during compaction preparation where the original Store might remain intact.
Admission closes before the uncertain result is delivered. Queued requests are
rejected, the owner drops its Store, and no automatic reopen/retry occurs.
Owner panic is contained when Rust unwinding is available; process abort, power
loss and mmap fault signals are outside that mechanism.

An uncertain mutation requires inspection or an application-specific durable
receipt. There is no general durable operation-ID/receipt API yet. Local CAS
does not prove whether an earlier timed-out attempt committed. Real device
writeback failures require the separate qualification described in the
[resilience research](Storage-Resilience.md).

## Shutdown and snapshots

`shutdown(mode)` closes admission synchronously without queue credit:

- `Drain` completes admitted requests subject to their deadlines/cancellation.
- `CancelQueued` rejects queued requests and permits the currently started
  request to finish. It can escalate an earlier drain.

`Shutdown::wait()` can be wrapped in a caller timeout. Dropping that observer
does not interrupt the owner; another call can observe completion later.
Dropping an open Worker closes admission and cancels queued work without joining
the thread on the caller. Dropping a Worker after explicit drain leaves that
drain in effect. Retained Client clones cannot keep an abandoned owner serving.
A hung filesystem call can still prevent completion; there is no hard shutdown
deadline or forced thread termination.

The completion report is published after dropping the Store and retiring all
queued requests. It does not wait for applications to consume their replies.
Snapshots in caller variables or unread replies can retain mappings and file
locks after shutdown; release them before reopening the same store. Reading
snapshot bytes still happens on the calling thread: cold mmap page faults and
mapped I/O failures are not isolated by this writer service.

## Diagnostics and validation

`diagnostics()` exposes current phase, a sticky degraded flag, small/large
credit usage, queue length/oldest enqueue time, whether the owner is processing
a request, terminal outcome counts, failed reply deliveries and last completion
time. Successful operations include reads and administration, not only commits.
Last completion time is not a durable revision watermark. After shutdown,
`Phase::Stopped` and the degraded flag distinguish clean from failed termination.

Tests cover both budget directions, principal sharing, unread replies, shutdown
during preparation, queued cancellation/expiry/drop, start/cancel races, abandoned
started writes, executor progress, drain/cancel/drop lifecycle, startup failure,
owner panic, append and compaction I/O quarantine, V4 atomic batches, V5 component
sharing/deletion, publication history and snapshot locks after shutdown.

```sh
cargo test --locked -p reproto --lib storage::worker
cargo test --locked -p reproto --test storage_worker
cargo test --locked -p reproto --no-default-features --features storage --test storage_worker
```

Group commit, general durable receipts, execution-time capability authorization
and automatic integration with ORM RPC facets remain follow-up work.

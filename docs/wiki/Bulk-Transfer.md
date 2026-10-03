# Bulk transfer capabilities

The separate [durable bulk service](Durable-Bulk.md) adds resumable file upload,
disk-backed chunk journals and atomic typed ORM publication with a durable
receipt. The original in-memory `Transfer` contract below is unchanged.

`src/bulk.rs` implements `schemas/bulk.capnp`. One `Transfer` capability
authorizes one bounded transfer into an application-owned receiver. Separate
capabilities isolate staging, sequence numbers, failure and publication. Payloads
are opaque bytes; this interface does not transfer embedded capabilities.

Construct checked limits with
`Config::new(length, max_chunk_bytes, window_bytes, max_chunks)?`. Fields are
private and immutable; the matching getter methods return their numeric values.
JSON and RPC imports use the same validation, and JSON rejects unknown,
duplicate or missing fields. Zero length is valid; zero chunk size/count is not.
The numeric wire and journal encodings are unchanged.

Create `(receiver, capability)` with the infallible `Receiver::new(config)`, retain the receiver,
and export the capability over an existing RPC connection. Create one
`Sender::connect(capability).await` per fresh transfer. The sender validates the
advertised limits. Its mutable methods serialize admission; there is no implicit
reconnect, retry or allocator reset on an existing capability.

| Wire method | Meaning |
| --- | --- |
| `describe()` | Returns declared total length, maximum chunk size, payload window and chunk count. |
| `write(sequence, data)` | Processes one ordered chunk and returns its sequence as an acknowledgment. |
| `done()` | Checks the exact total length and atomically exposes the complete value; returns byte/chunk counts. |
| `cancel()` | Releases unpublished data; returns `Canceled` or `Complete` if publication already won. |

The adapter uses ordinary RPC Calls and Returns. Unlike an optimized RPC
streaming send's readiness promise, a wire `write` reply acknowledges remote
processing. Several calls can be outstanding. `Sender::write(bytes).await`
reserves payload credit, sends a call and returns on admission, before that call
necessarily executes. `flush().await` observes outstanding replies;
`done().await` observes final publication. A successful flush alone does not
publish the transfer.

## Credit and ordering

`CreditWindow` reserves bytes before sending and returns them only when the
matching call settles. Successful acknowledgments and error replies both settle
reservations; the first observed error prevents further writes. A transport
failure also ends the wait, with execution potentially unknown. Out-of-order
replies are supported. Duplicate acknowledgments cannot release credit twice.
RPC itself delivers one Return per question; duplicate-ack injection tests the
ledger. The sender independently rejects mismatched wire acknowledgment numbers.

The 0.x credit API returns `Option<Reservation>` from `reserve(bytes)`. Retain
that opaque handle alongside the pending call; `sequence()` explicitly projects
the number for wire encoding. `settle(&reservation)` replaces raw-number
`acknowledge(sequence)` and returns `Settlement::Released` or
`Settlement::AlreadySettled`. A reservation from another window is rejected
without mutation, even if both windows issued the same sequence. Window and
reservation moves preserve the identity binding; replacement windows cannot
accept old handles. Private immutable fields and the absence of `Clone`, `Copy`,
numeric construction and deserialization prevent callers from fabricating or
retargeting reservations. Both the window and its handle remain `Send + Sync`.

Dropping a reservation leaves its credit charged: local abandonment is not a
remote acknowledgment. Handles and settlement results are `must_use`. Repeated
settlement borrows the same retained handle and is idempotent. The low-level
caller must wait for the matching call's terminal reply; possession of a handle
alone does not establish that the call finished. The production sender owns
reservations in its pending-reply futures and settles only after those futures
resolve, retaining the first error independently of released credit.

The strict window counts payload bytes, not RPC framing or total connection
memory. It is independent of the runtime's existing streaming flow controller.
Chunks must be nonempty, fit the advertised maximum, arrive with consecutive
sequence numbers starting at one, and stay within the declared length and chunk
budget. Invalid or duplicate chunks poison an open receiver. That failure is
sticky, clears staging and is returned by later writes and completion attempts.
`Receiver::fail(error)` lets the owning application reject a transfer explicitly.
Independent holders of the same capability share its authority and can invalidate
or cancel the transfer; use one ordered producer for normal operation.

## Completion and cancellation

Only `done()` publishes. An incomplete `done()` fails the transfer and clears its
staging. A zero-length transfer completes without chunks. Repeated completion
returns the same summary and immutable published allocation. Canceling after
completion returns `Complete` and does not undo publication. Later writes cannot
modify a completed value.

Dropping a wait on `write` while it is blocked for credit does not reserve bytes
or a sequence. Dropping a wait on `flush`, `done` or `cancel` keeps outstanding
RPCs owned by the sender. Completion/cancellation replies remain cached so a
subsequent call can resume observing them. A timeout or transport error is not
evidence that the remote operation did not execute. The sender cannot report
successful completion from admission, a partial acknowledgment or a mismatched
receipt.

Cancellation is an ordered application operation. Previously submitted writes
may execute before it, and `done()` may already have published. Dropping the
entire sender abandons observation; it does not imply a remotely acknowledged
cancel. The RPC methods permit cancellation of abandoned calls, without undoing
effects that already executed. Dropping the last application `Receiver` cancels
unpublished staging. Applications retaining abandoned receivers must cancel or
release them according to their own timeout policy.

## Storage boundary and limits

This receiver stages in memory. Configured limits are 64 MiB total payload,
1 MiB per chunk, a 16 MiB payload window and 65,536 chunks. The window must fit
one maximum-sized chunk, and the length must fit the chunk budget. Allocation
capacity, RPC buffers and transient publication copies are additional memory;
this is not a hard process-memory limit.

`completed()` returns an immutable `Rc<[u8]>` only after publication.
`progress()` reports historically accepted byte/chunk counts, while
`staged_bytes()` reports currently retained unpublished payload. These counts
are intentionally different after failure or cancellation. `failure()` retains
the first application failure even after staging is canceled.

Publication here is a logical in-memory commit. It does not acknowledge disk
durability or automatically write/publish an ORM revision. An application can
consume the completed bytes using the separate storage/ORM APIs. Unbounded file
streaming remains outside this service. Resumable transfers and transactional
ORM publication use `DurableTransfer` instead.

## Verification

Run `cargo test --test protocol_models bulk_model -- --exact`, or the complete suite with
`cargo test --locked --workspace --all-targets`. The focused report is
`reports/capntproto/bulk/verification.json`.

Seven configurations of `RpcBulkTransfer.tla` explore 2,147 states across
separate graphs. All 4,365 edge-prefix traces replay against the production
receiver and credit ledger. Another 1,351 prefixes, excluding injected duplicate
Returns, drive the production sender and receiver through real two-party RPC.
A test proxy controls dispatch and reply delivery, including out-of-order
acknowledgments and completion/cancellation replies. Six model mutations are
rejected: window bypass, double credit, forgotten errors, premature publication,
incomplete publication and leaked staging after cancellation.

All 14 original `CapnpBulkTransfer.tla` configurations are rerun. Separate Rust
regressions cover bounds, capability isolation, owner lifetime, disconnect,
canceled waits, mismatched replies, and mutually authenticated QUIC.
The transfer model uses two chunks and finite windows; these are bounded component
and RPC checks, not a proof of every executor schedule, durability or the whole
composed runtime.

`cargo test --locked --test bulk_reservations -- --nocapture` checks the additional
`BulkReservation.tla` model: **1,749 states / 8,866 edge prefixes**, each replayed
against the production credit window. It models two independent two-byte windows,
two issued sequences per window, one retained handle per window, duplicate and
foreign settlement, backpressure, exhaustion and discarded handles. Five controls
detect wrong-window release, duplicate credit, release on drop, sequence-budget
bypass and charging a backpressured reservation. This is a bounded safety model;
it does not claim liveness or model RPC delivery. Native tests separately cover
window replacement with retained old handles and all 65,536 permitted sequences.
The external-consumer Cargo compiler gate adds **16 contracts** for this public
API, including a positive control and exact diagnostics for required failures.

`cargo test --locked --test bulk_config -- --nocapture` checks immutable limits
and `BulkConfigBoundary.tla`: **9,421 states / 70,275 edge prefixes**, replayed
through direct construction, JSON import or the sender's RPC describe boundary,
then through receiver writes, completion and cancellation. Seven model fault
controls must fail. Full-width invalid inputs, malformed JSON, framed hostile
RPC advertisements and corrupted durable metadata have native regressions.
The external compiler gate adds **13 configuration contracts**. See
[bounds and limitations](../archive/Testing-History.md#checked-bulk-configuration).

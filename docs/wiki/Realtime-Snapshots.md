# Realtime snapshot capabilities

`src/realtime.rs` implements `schemas/realtime.capnp` as an ordinary Cap’n Proto
RPC service. A `Snapshots` capability authorizes updates to one receiver's
latest-value store. Separate capabilities have separate sequence namespaces,
queues and visible values. Payloads are opaque bytes without embedded
capabilities. Updates are complete replacements, not dependent deltas or
arbitrary application side effects.

## Interface and ownership

Create `(receiver, capability)` with `Receiver::new(config, clock)`. Retain the
receiver in the application and export the capability over an existing RPC
connection. `Sender::connect(capability).await` reads and validates the advertised
configuration. Create one sender allocator per fresh stream capability; clone it
to share sequence allocation. Reconnecting must obtain a fresh stream capability
rather than resetting an allocator on an existing stream.

The wire methods are:

| Method | Behavior |
| --- | --- |
| `describe()` | Returns clock domain, skew allowance and resource limits. |
| `offer(sequence, key, notAfter, snapshot)` | Admits a snapshot and eventually returns its terminal outcome. |
| `cancel(sequence)` | Installs a cancellation tombstone or returns an existing terminal outcome. |
| `close()` | Stops admission and resolves all pending snapshots as closed. |

`Sender::offer` returns a `Receipt` immediately, so later offers can supersede
pending work without awaiting it. `Receipt::outcome()` waits for the authoritative
outcome: `Applied`, `Expired`, `Superseded`, `Busy`, `Canceled` or `Closed`.
`wait_until(local_deadline)` returns `None` on a local timeout. That means execution
is unknown, including when application has already happened but the receipt is
delayed. The retained receipt can still be awaited. A transport error similarly
does not prove nonexecution.

Dropping all receipt clones releases the RPC waiter; accepted work stays queued.
Use explicit `cancel` to cancel work. Canceling an already applied snapshot
returns `Applied` and does not undo publication. Canceling before the data arrives
prevents later admission of that sequence. Closing preserves terminal outcomes
and previously visible values. Dropping the last application `Receiver` closes
queued work, while dropping the remote capability alone does not. A lost close
reply does not establish remote closure.

## Replacement, deadlines and application

Keys range from zero to `keys - 1`. Sequences range from one to `max_sequence`
for the stream lifetime. Each sequence identifies immutable key/deadline/content
metadata. Matching duplicates share pending work or return the cached terminal
outcome. Changed metadata is rejected; terminal content identity uses SHA-256.

The receiver remembers the highest admitted sequence per key. A newer sequence
supersedes queued older work for that key even if the new snapshot is already
expired. Old traffic cannot restore superseded work. There is at most one queued
snapshot per key. A fresh key at queue capacity returns `Busy` without evicting
another key; its sequence is still remembered. Pre-admission validation or waiter
quota errors do not supersede existing work.

`Receiver::apply(sequence)` performs one synchronous logical publication into
the receiver's visible store. It checks freshness and the conservative deadline
predicate `now + clock_skew < not_after` immediately before publication. Equality
and arithmetic overflow fail closed. The application reads the latest immutable
snapshot with `get(key)`. `Snapshot::sequence()`, `key()`, `not_after()` and
`bytes()` expose read-only coordinates and a borrowed byte slice. Fields are
private, including in an owned clone. Retained snapshots survive replacement
and stream closure without relabeling their data; the publication deadline does
not expire a retained read. `expire()` removes expired queued work without applying
it. `run(period)` provides an optional Tokio worker that expires and applies work
at each tick, skipping missed ticks. Explicit calls to `apply` allow applications
to choose their own scheduling/coalescing policy.

The injected `Clock` must return nondecreasing ticks in the configured domain.
Producer deadlines must already be mapped into that domain with the advertised
error bound. `MonotonicClock` supplies saturating microsecond ticks from an
explicit Tokio instant and epoch; it does not synchronize clocks. A detected
clock regression closes pending work and returns an error.

The checked effect is logical store publication without an intervening await or
application callback. OS preemption, clock synchronization, later consumer
actions and actuator timing are outside this guarantee. No delivery latency or
hard realtime deadline is promised.

## Bounds and transport

`realtime::Config::new(clock_domain, clock_skew, keys, capacity, max_sequence,
max_payload_bytes, max_waiters)` returns checked, immutable settings; getters
expose each field. The clock domain must be valid UTF-8 and 1–128 bytes.
`Receiver::new(config, clock)` now returns the receiver/client pair directly.
Wire imports use the same validation boundary. To change settings, construct a
new configuration and a fresh stream; existing senders cannot relabel or enlarge
their negotiated limits. A valid configuration does not establish synchronization
or authenticate the supplied clock.

Configuration limits are 1–1,024 keys, a queue capacity of 1–`keys`,
1–65,536 sequence IDs, 1 byte–1 MiB per snapshot and 1–65,536 receipt waiters.
The configured pending-plus-visible payload budget must fit within 64 MiB.
Terminal records retain metadata/outcomes for the stream lifetime, not full
payloads. Cancellation tombstones consume the same bounded sequence namespace.
The receiver represents queued, finished and pre-offer tombstone states with
separate enum variants. Only queued records can retain payloads or waiters.
Finishing drains the waiter ledger before notifying outside the state borrow;
dropping a receipt releases its waiter's quota while preserving admitted work.
Application-held `Rc<Snapshot>` values and buffers owned by RPC/transport callers
are outside the receiver's retained-payload budget. Rotate to a new capability
when the lifetime sequence budget is exhausted.

The `realtime::Sender` adapter uses reliable RPC calls and can incur transport
head-of-line blocking. `realtime_datagram` provides the optional authenticated
unreliable adapter described below. Neither changes the standard RPC message
discriminants.

## Authenticated datagram adapter

`AuthenticatedSession::take_datagrams()` yields its single datagram lane after
the pinned Native handshake and native stream preface have completed. Take it
before moving the session into `native_rpc::Network::attach`. Keep the session or
network route alive: datagram handles do not extend its lifetime. The protocol
remains TLS 1.3 with pinned mutual authentication, or TLS admission for introduced PSK sessions,
using the pinned upstream TLS verification rules.

On the receiving side, `realtime_datagram::Router::new(port)` returns a router
and a driver future to spawn on the LocalSet. `router.bind(config, clock)` creates
an application `Receiver` and a fresh `DatagramSnapshots` RPC capability. Export
that capability through the application's authorized confidential RPC route.
Retain the driver task and arrange application scheduling through `Receiver::run`
or explicit `apply`. The driver admits data but never automatically applies it.

On the sending side, retain `port.sender()` and call
`realtime_datagram::Sender::connect(capability, sender).await`. As with the
reliable adapter, create one sequence allocator per fresh capability and clone
it to share allocation. `offer(key, not_after, bytes)` returns a `Receipt` after
bounded local queue admission. It does not wait for remote admission or execution.
A full local queue returns an overloaded error without consuming a sequence ID.

The control methods are ordinary RPC:

| Method | Behavior |
| --- | --- |
| `describe()` | Returns the bounded configuration and a fresh 256-bit bearer token. |
| `status(sequence)` | Returns `Unknown`, `Pending`, or `Terminal(outcome)`, plus whether the receiver is closed. |
| `cancel(sequence)` | Returns the authoritative existing outcome or installs a cancellation tombstone. |
| `close()` | Revokes the token and closes pending snapshots; published values remain. |

`Receipt::status()` queries this control capability. A status describes the
receiver when the query was processed. `Unknown` and `Pending` are not final:
a reply may arrive after the data has been applied. Invalid or dropped datagrams
may remain unknown indefinitely. Transport errors and canceled status waits
also leave execution unknown. Terminal outcomes are retained and cannot be
rolled back. `Receipt::resend()` explicitly retries the identical sequence,
metadata and payload; no automatic retransmission or reliable delivery is
promised. Cancellation works before data arrives, and an old retry cannot revive
canceled, superseded or closed work. Dropping a receipt only releases local
ownership. `Sender::close()` seals all sender clones immediately; only a
successful reply establishes remote closure.

Offers reserve capacity for the entire packet batch before consuming a sequence.
The counter advances before the first packet can wake the queue receiver. A wake
callback may submit through a clone, retry or close the sender synchronously;
it observes committed state, and the outer call does not overwrite nested changes.
Dropping an unsubmitted reservation releases capacity without sending. Closing
during publication does not revoke the existing batch's permits. Local admission
and remote execution remain separate outcomes.

Each router has a session-local registry. A token grants authority only on that
session, including when two sessions have the same pinned peer identities. It
is a bearer secret: applications must not disclose it outside authorized
capability holders. Wire data alone cannot create a receiver. Closing a grant
or releasing its last control capability removes the token and closes pending
work. Dropping the router driver also closes every grant, even if the future
was never polled. Keep the control capability/receipt alive while sending data;
its lifetime is separate from the application receiver handle. Previously
received packets can be drained before the driver observes session EOF.

Single-packet frames use `RDS1`, a 32-byte token, big-endian `u64` sequence, `u32` key, `u64`
deadline, then snapshot bytes. The header is 56 bytes. Version/magic, total
length, token, receiver key/sequence/payload bounds and immutable metadata are
checked before admission. Malformed, unknown-token and conflicting frames are
discarded without a datagram reply. Payloads contain no live capabilities.

The transport accepts at most 1,024 bytes per datagram. RDS1 supports 968 snapshot
bytes; RDS2 supports larger values as described below. Local send and receive queues
hold at most 64 datagrams each, in addition to quiche's bounded 64-entry queues.
An unread receive queue drops excess datagrams without blocking the RPC stream.
Packets can also be lost at transport admission or on the network. A router
issues at most 64 grants during its lifetime and retains retired token IDs to
prevent reuse. Every grant has its own receiver quotas and sequence namespace.
Datagrams share packet congestion control with RPC; they bypass stream ordering
but have no latency guarantee or reserved bandwidth. On a replacement session,
obtain new grants rather than carrying old tokens across reconnect or handoff.

## Fragmented snapshots

`Router::bind()` and `Sender::connect()` accept payload limits up to
`MAX_SNAPSHOT_BYTES` (59,392 bytes). `Sender::offer()` selects RDS1 for values of
at most `MAX_PAYLOAD_BYTES` (968 bytes), preserving existing peers and framing.
A capability advertising a larger payload limit supports RDS2. Older senders
reject that larger configuration instead of silently trying an unsupported frame.
The RPC schema and its status/cancel/close methods are unchanged.

RDS2 keeps the token, sequence, key and deadline in their RDS1 positions and adds
these fields. All integers are big-endian.

| Offset | Field |
| --- | --- |
| 0 | Four bytes `RDS2` |
| 4 | 32-byte session-local capability token |
| 36 | `u64` sequence |
| 44 | `u32` key |
| 48 | `u64` deadline |
| 56 | `u32` total snapshot length |
| 60 | `u32` fragment byte offset |
| 64 | 32-byte SHA-256 digest of the complete payload |
| 96 | Up to 928 payload bytes |

RDS2 requires a total length greater than 968 bytes, at most the capability's
limit, and at most 59,392 bytes. Offsets must be multiples of 928; every fragment
must have its exact expected length, including the final fragment. This gives
at most `MAX_FRAGMENTS` (64) nonoverlapping positions. Malformed frames and unknown
tokens are discarded before allocating reassembly state. The digest commits to
payload contents; the session token and authenticated transport provide authority.
The Native profile remains unchanged.

The first valid fragment binds its sequence to a key, deadline, length and digest.
This commitment remains for the grant's lifetime, even if allocation is refused
or a partial buffer is discarded. The existing finite `maxSequence` bounds this
metadata ledger. Conflicting metadata, encoding changes and duplicate positions
cannot overwrite accepted chunks. Fragments may arrive in any order. Publication
requires every position and a matching whole-payload digest; a digest failure
discards the buffer, and an identical retry can start again.

Each grant buffers at most `min(config.capacity, MAX_REASSEMBLIES)` incomplete
values, with `MAX_REASSEMBLIES = 8`. Payload buffers therefore total at most
475,136 bytes per grant, separate from the existing receiver's bounded pending
and visible snapshots. Capacity pressure drops fragments without admitting work.
A partial newer sequence cannot supersede an already complete snapshot. Once
complete, the value enters the ordinary receiver, including its per-key freshness,
capacity, immutable-sequence, cancellation and final publication-deadline checks.

An incomplete sequence reports `Unknown`, never `Pending` or `Applied`. It may
remain unknown indefinitely when fragments are lost or expire. Ingress and status
queries sweep expired buffers; call `Router::expire()` to reclaim them on idle
lanes using the application's clock. This sweep also expires pending receiver
work. Cancellation installs the ordinary receiver tombstone and frees any partial
buffer. Token revocation, control-capability release and driver shutdown free
buffers and close admitted work. Clock regression closes the receiver; skew
overflow cannot admit fragments. No wall-clock timer or delivery guarantee is
introduced by reassembly.

The sender reserves queue slots for the entire batch before enqueueing any packet.
Queue-full failure admits no prefix and consumes no sequence number. A successful
offer returns one receipt owning the exact batch, and `resend()` retries those
same bytes atomically at the local queue. Transport and network loss remain
possible after admission; retry is explicit and covers the whole snapshot.

## Verification

Run `cargo nextest run --test protocol_models realtime_model -- --exact` for the focused check, or
`cargo nextest run --locked --workspace --all-targets` for the complete current runtime suite.
The focused report is `reports/capntproto/realtime/verification.json`.

Five `RpcRealtimeReceiver.tla` configurations contain 6,336 states across separate
graphs. All 32,979 edge-prefix traces replay against the production Rust receiver,
checking outcomes, pending work, visible snapshots, application counts, clock
and closure state after each step. Six mutations exercise deadline bypass,
forgotten cancellation, duplicate application, stale rollback, queue overflow
and application after close. All 28 configurations of the original
`CapnpRealtime.tla` contract are also rerun, including its loss, reordered data,
control delivery, receipt and timeout scenarios.

Separate Rust tests cover real RPC cancellation, delayed receipts after
application, independent stream capabilities, authenticated Native UDP,
resource limits, clock regression and overflow. The implementation model uses
two immutable snapshots and bounded keys/time; its trace replay is a component
check, not a proof of arbitrary executions or composed transport/RPC refinement.

`cargo nextest run --locked --lib realtime::lifecycle_tests -- --nocapture` additionally
checks `RealtimeReceiptLifecycle.tla`: **12,761 states / 41,966 edge prefixes**
replayed through the receiver and real receipt futures, with seven model fault
controls. Native tests cover payload release, reentrant wakes, waiter exhaustion
and retained snapshot identity; **14 external compiler contracts** protect the
immutable snapshot API. See [bounds and observations](../archive/Testing-History.md#realtime-record-and-receipt-lifecycle).

`cargo nextest run --locked --test realtime_config -- --nocapture` checks immutable
configuration imports and subsequent admission/publication against
`RealtimeConfigBoundary.tla`: **15,108 states / 19,811 edge prefixes**, with eleven
model fault controls. Native tests exercise production-width resource boundaries,
malformed clock domains, framed RPC advertisements and sender exhaustion;
compiler contracts reject configuration bypasses. See [scope](../archive/Testing-History.md#checked-realtime-configuration).

The focused datagram checker is `cargo nextest run --test protocol_models realtime_datagram_model -- --exact`.
`RpcRealtimeDatagram.tla` explores 1,669 states and replays all 10,566 edge-prefix
traces through the production datagram ingress and real RPC control calls.
The model covers two copies of one immutable snapshot, packet loss, retries,
invalid tokens, malformed frames, cancel-before-data, revocation, driver stop,
and delayed status replies. Five mutations must fail: authority bypass,
forgotten cancellation, duplicate application, admission after revocation and
false applied receipts. Its report is `reports/capntproto/realtime-datagram/verification.json`.

Separate Rust tests exercise maximum-size datagrams over real TLS admission Native UDP,
session isolation, reliable status under datagram pressure, queue bounds,
capability release, unpolled/aborted drivers, explicit retry, expiration and
replacement. Trace replay controls ingress directly to explore loss/order
without nondeterministic UDP timing. This checks composition at the adapter and
RPC control boundary; it is not a proof of cryptography, arbitrary network
schedules or whole-program refinement. Automatic grant migration across
three-party handoff and hard realtime execution remain outside this adapter.

Run `cargo nextest run --test protocol_models realtime_fragments_model -- --exact` for the fragmentation
checks, also included in the canonical runtime runner. `RpcRealtimeFragments.tla`
has 1,191 states and 10,321 edge-prefix Rust replays with two fragment positions
and a competing incomplete transfer. It covers reordering, duplicate and corrupt
data, metadata commitments, authorization, capacity, cancellation, expiry and
closure. `RpcDatagramBatch.tla` has 130 states and 417 replays against the real
sender and queue with two free slots, two sequence numbers, backpressure, retries
and shutdown. Eleven injected faults must violate their specified invariants.
The combined report is `reports/capntproto/realtime-fragments/verification.json`.

`cargo nextest run --locked --lib transport::datagram_reentry_tests -- --nocapture`
also checks `DatagramReentry.tla`: **3,021 states / 3,192 native edge prefixes**,
with eight model fault controls. It covers synchronous wake callbacks during
submission, nested offers/retries/close, capacity reservations and commitment
ordering; thirteen compiler contracts protect the reservation's borrowed inputs
and one-shot send. See [bounds and observations](../archive/Testing-History.md#packet-reservations-and-reentrant-datagram-admission).

Separate regressions cover all 64 fragments, malformed offsets/lengths, hash
failure recovery, capability release, compact single-packet framing, cross-key capacity,
supersession, clock regression/overflow and a 59,392-byte snapshot over real Native
UDP with reliable RPC control. The models do not prove all fragment counts,
arbitrary schedules, eventual delivery or cryptography. Omitted fragments model
loss; the existing datagram model separately explores queued packet loss.

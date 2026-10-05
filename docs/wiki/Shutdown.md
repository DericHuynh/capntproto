# Peer-acknowledged Native shutdown

`Handle::shutdown(peer, timeout)` drains one established native RPC route and
returns a `native_shutdown::Receipt`. Success means the authenticated peer has
copied every ordered stream byte queued before the shutdown fence into its
bounded RPC input bridge. `Receipt::bytes` includes arbitration bytes, when
arbitration is enabled, and excludes the native one-byte stream preface.

The application must quiesce its work and await any required RPC responses before
starting shutdown. A receipt does not mean remote methods finished, external
side effects committed, replies were received, or unreliable datagrams arrived.
An error or cancellation leaves execution uncertain and does not authorize
replaying mutations. The original `disconnect()` and listener `close()` remain
immediate local teardown operations.

## API and lifetime

Calling `shutdown()` synchronously marks the route `Draining` and inserts a
terminator behind the messages already admitted to its RPC write queue. A future
returned by a request constructed earlier but sent after this fence cannot add
work to the drain. Keep polling the RPC system while awaiting shutdown.

After the serializer finishes, the two-party writer closes its output and resolves `output_closed()`.
This completion is independent of input EOF and connection-handle lifetime. This EOF propagates through any connector/arbitration bridge. The transport
continues draining its bounded queue, then emits a request naming the final byte
count. The peer's acknowledgement is withheld until that count has actually been
written into its RPC input bridge. Flow control and slow consumers can therefore
delay shutdown; timeout is an error rather than a fabricated receipt.

Timeouts must be positive and at most 60 seconds. The deadline starts when the
operation is requested and covers queue drain, pacing/socket writes and the peer
exchange. Cancellation, timeout, network/listener close and explicit disconnect
release that route and its datagrams. Cleanup captures the endpoint generation;
a stale shutdown future cannot remove a newly installed route for the same peer.
A second shutdown of a draining route is rejected. A peer that has already
requested shutdown cannot be given a new, independent local fence after its
response has been selected. Concurrent requests begun before either peer sees
the other's request use the crossed exchange below.

`AuthenticatedSession::shutdown(self, timeout)` consumes an unattached session.
`Handle::shutdown_all(timeout)` permanently closes network admission, starts all
current established drains concurrently, and returns one result per peer.
Pending and failed routes are aborted and reported as failures, not successful
receipts. Dropping this future aborts the generations it captured.

For a shared listener, use this sequence after quiescing application work:

```rust
listener.stop_accepting();
let receipts = handle.shutdown_all(std::time::Duration::from_secs(5)).await?;
listener.close();
// Inspect each peer's result; failed drains have no delivery guarantee.
```

`stop_accepting()` rejects new reservations and retires pending reservations,
including unpolled accepts. Authenticated packet routes remain live so their
shutdown exchanges can complete. Drain every network using the listener before
closing it; unattached sessions must be consumed separately. Calling `close()`
or dropping the final listener during a drain aborts the exchange.

## Authenticated wire extension

The standard Cap'n Proto RPC schemas are unchanged. On quiche sessions, this
Capntproto extension uses two reliable unidirectional control streams per endpoint:
request streams 2/3 and acknowledgement streams 6/7 (initiator/responder).
Each stream contains exactly one 29-byte frame followed by FIN:

| Bytes | Meaning |
| --- | --- |
| 0–3 | ASCII `RPS1` |
| 4 | Request (1), acknowledgement (2), crossed acknowledgement (3) |
| 5–20 | Random 128-bit request nonce, echoed in its acknowledgement |
| 21–28 | Final RPC byte count, little-endian UInt64 |

The control bytes are processed only after the pinned Native handshake and native
stream preface. The encrypted connection binds the identity and generation; the
nonce and exact byte count bind its one receipt. Unknown kinds/versions, wrong
stream roles, truncated/oversized frames, unsolicited or mismatched receipts,
repeated protocol transitions, and resets fail the session. Control buffering is
fixed-size, and the transport grants 128 bytes of initial receive credit per
unidirectional stream. Existing peers without this extension cannot confirm a
shutdown and produce an error/timeout, never a local-success fallback.

Control streams can arrive before their corresponding RPC bytes. A receiver
waits for the byte boundary before acknowledging. In a crossed exchange it also
waits for its own writer fence, then sends a crossed acknowledgement indicating
that its own request is in flight. Each side waits for the reciprocal receipt to
be explicitly confirmed before initiating connection close. The confirmation
stream echoes the validated reciprocal receipt's nonce and byte count. The
graceful close uses application code `0x525053` and carries that same echo in its
reason bytes. If the close overtakes the confirmation stream, the receiver may
complete only with the exact echoed receipt, its reciprocal request, full input
delivery and fully queued reply. This is the `capntproto/3` Native QUIC profile.
Other application/transport close codes and idle timeout cannot replace those
fences. An authenticated peer's delivery assertion is trusted; the protocol cannot
prove that a malicious peer actually delivered bytes. Packet loss and
retransmission use the existing engine.
Local success sends connection close and releases the route; loss of that final
packet can delay remote resource reclamation until transport failure/timeout.

Arbitration exposes only the selected session's shutdown handle. Losing sessions
cannot acknowledge or keep the endpoint's drain alive. Reconnecting creates a
fresh handle and nonce; old receipts never complete a new route. Datagram lanes
are closed as the drain proceeds and have no receipt guarantee.

When [split-plane bulk streams](Split-Plane.md) are admitted, shutdown immediately
rejects new grants and waits for those transfers to settle before issuing the
final receipt/connection close. Successful streams retain their final consumption
receipt until Quiche acknowledges and collects the stream. Canceled/failed
transfers retain their own error; shutdown is not an application publication
receipt. Its byte count continues to describe RPC input only.

TCP/TLS carries the same receipt frames through bounded multiplexing alongside
RPC data in its ordered TLS stream; it does not create QUIC control streams.
Both transports acknowledge delivery into the peer RPC input, not method execution.
See [the TCP bridge](../../src/transport/tcp.rs).

## Verification

`cargo nextest run --test protocol_models native_shutdown_model -- --exact` checks **807 states** and replays
**1,496 graph edge prefixes** through production frame, transition and completion
helpers. It covers empty/nonempty streams, data/control reordering, crossed
receipts, malformed receipt rejection, failure and late completion. Healthy
progress assumes fair writer drain and data/control/acknowledgement delivery.
Six faults must violate their designated invariants: sending before drain,
acknowledging before input delivery, early reciprocal acknowledgement, missing
receipt binding, premature crossed close and success after failure.

The listener model additionally covers admission stop, pending-lease retirement,
continued authenticated routing, late reservation rejection and final close.
Its one/two-slot graphs have **1,246/1,526 states**, producing **23,976 Rust
replays**, with nine detected faults.

Runtime tests exercise dedicated/shared sockets, incoming/outgoing connectors,
arbitrated routes, both shutdown directions, queued messages larger than the
input bridge, unread-peer timeout, cancellation, duplicate requests, generation
replacement and listener/network drain. Packet-engine tests cover partial frames,
loss, duplicates and malformed finished control streams. The canonical
`cargo nextest run --locked --workspace --all-targets` includes these tests and models.

The additional `NativeShutdownPacketFence` model checks **489 states / 737 edge
prefixes**, each replayed against real encrypted mutual TLS with and without admission secrets connections
(**1,474 replays**). Seven fault controls cover receipt framing/binding, crossed
fences, reply writes/resets, close codes and sticky failure. Packet regressions include
wrong counts, resets, incomplete FIN, insufficient stream credit, damaged tags
and replay after reconnect with the same static identities and CIDs. The outer
driver separately checks close/deadline precedence. See
[testing commands and bounds](../archive/Testing-History.md#shutdown-receipt-packets-and-peer-close).

These are bounded component checks and integration regressions. They do not
prove arbitrary executor/network schedules, cryptographic security, application
quiescence, or end-to-end RPC execution refinement.

## Route diagnostics

`Handle::observe_route(peer)` returns a generation observer. It retains the first
terminal cause after the route is removed, without keeping a connection, task or
capability alive. Causes distinguish acknowledged shutdown, cancellation and
structured failures. Direct, connector and arbitration-selected sessions use the
same lifecycle installation and task owner. Native QUIC receipt confirmations
echo the exact nonce/count over an authenticated control stream or close reason.
Quiche stream collection/reset errors are never treated as receipt evidence.

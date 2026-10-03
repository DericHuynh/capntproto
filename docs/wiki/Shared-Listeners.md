# Shared Native listeners

`native_listener` provides bounded, single-use incoming connection reservations
on one UDP socket. The quiche engine uses standard QUIC v1/v2 and pinned mutual
TLS. Quiche clients can select the unpredictable, provisioned
initial destination ID. Admission verification binds that ID into the signed
certificate context; possession of the routing ID alone grants no authority.

## Provisioning and ownership

Call `Listener::bind(address, Rc<Identity>, limits).await` inside a Tokio LocalSet.
Create immutable identities with `Identity::generate()`,
`Identity::from_private_key(private)`, or `Identity::from_keypair(private, public)`.
Imports derive the public key; pair import rejects a mismatch before listener or
transport setup. Use `identity.public_key()` for the peer's public identity.
The unreleased 0.x API no longer exposes mutable key fields or a private-key getter.

`reserve(peer, psk, context)` allocates one connection for that pinned peer and
returns a `Reservation`. Deliver its `target()` and matching PSK/context through
the application's authorized provisioning channel. The target contains the host
public key, socket address and a random 16-byte connection ID. An application
can replace the advertised address for a wildcard bind or configured port map.
The dialer rejects unspecified advertised addresses.

Run `reservation.accept()` and `native_listener::connect(socket, target, identity,
psk, context)` concurrently. Both return the existing `AuthenticatedSession`,
which can supply a realtime datagram lane and attach to `native_rpc::Network`.
The server publishes the session only after the pinned Native proof and encrypted
native stream preface complete, and while the reservation is still live.

`DirectoryConnector::insert_reserved(target, psk, context)` configures a native
on-demand route. Each reserved directory entry permits exactly one dial attempt;
retry requires explicitly provisioning a fresh target. Entries can be replaced
through a shared `Rc<DirectoryConnector>`. Replacement affects future dials and
preserves existing RPC route identities and outstanding calls. Dedicated-socket
entries installed with `insert` retain their previous behavior.

Keep a Listener clone alive for the lifetime of its reservations and sessions.
Dropping the final listener or calling `close()` removes pending and established
routes, wakes their packet readers, and cancels drivers blocked in packet pacing
or socket writes. Dropping an unused reservation, canceling
its accept future, handshake failure, or dropping its authenticated session
retires that connection ID. Driver task cancellation may release its lease on a
subsequent executor turn. Retired IDs cannot address replacement connections.
Peer failure detection still follows RPC failure or transport timeout; listener
close does not promise a peer-acknowledged graceful shutdown. For that contract,
call `stop_accepting()`, drain attached networks with `Handle::shutdown_all()`
and then close the listener; see [Shutdown](Shutdown.md).

## Routing, authentication and bounds

Connection IDs are visible routing selectors, not bearer authority. The listener
allocates no state for an unknown ID and does not expose RPC on the basis of an
address or packet header. Native authenticates the configured peer identity, PSK,
application context and reservation ID. The listener-domain binding prevents a
shared reservation handshake from being used as an ordinary dedicated session.
A forged/invalid initial aimed at a known reservation can fail that attempt;
applications must provision a fresh reservation after failure. This is not a
stateless retry or denial-of-service prevention mechanism.

Limits permit 1–64 active routes and 1–64 queued packets per route. Packets exceed
neither 1,350 bytes nor the configured queue capacity; excess/unknown/malformed
traffic is dropped. Routing and notification never wait for a stalled peer.
Expired and closed routes immediately discard their queued packet buffers.
Queue notifications and destructors run outside registry borrows.

The reservation timeout starts at allocation, including time before `accept`
is polled. It must be positive and at most 60 seconds; the default is 10 seconds.
Authenticated connections are removed from the reservation timer and use the
transport's existing idle timeout. Cleanup runs on packet ingress, explicit
registry operations and a periodic timer. IDs are retained in a bounded retired
set: at most 4,096 IDs may be issued during a listener's lifetime. Contexts are
limited to 1,024 bytes. A fresh listener supplies a fresh issuance namespace.
The application controls provisioning, address selection and listener rotation.

The [capability provisioning service](Provisioning.md) automates fresh
reservations and PSK delivery over existing authorized control connections,
including an introducer relay for native handoff. [Discovery and mobility](Discovery-and-Mobility.md)
add directory lookup, STUN mapping discovery, delegated punching and validated path
changes. Replacement CIDs route to the original session and are removed on
retirement; they count toward the listener's lifetime ID budget. `Stats::issued`
counts reservations and `Stats::connection_ids` counts all issued routing IDs.
`maintain_mapping()` runs bounded STUN refresh transactions on the shared socket
while its owner is held, reporting changed/unavailable addresses without
replacing RPC generations. See the deployment guide for ownership and limits.
[Opt-in native arbitration](Session-Arbitration.md) now coalesces
crossed authenticated sessions before RPC publication.

## Verification

Run `cargo test --test protocol_models native_listener_model -- --exact`, or the complete
`cargo test --locked --workspace --all-targets`. The focused report is
`reports/reproto/native-listener/verification.json`.

The shared listener also runs through in-memory packet IO with real mutual TLS authentication, with and without admission secrets. `NativeListenerIo.tla` covers blocked concurrent sends, drain,
expiry, receive errors and close: 213 states and 456 traces replayed in both
Native modes. See [runtime packet testing](../archive/Testing-History.md#runtime-packet-io-and-semantic-replay)
for the bounded scope and command.

`RpcNativeListener.tla` checks two unique IDs, one/two active slots, bounded packet
queues, allocation rejection, authentication publication, expiration, packet
routing, late authentication, retired leases and listener shutdown. The one-slot
graph has 1,246 states; the two-slot graph has 1,526 states. All 23,976 graph
edge-prefixes are replayed through production Rust allocation, packet parsing,
queue and lifecycle helpers with a controlled monotonic clock. Admission stop
retires pending leases while preserving authenticated routes and refusing new
reservations. Nine injected
faults cover cross-route packets, stale lease cleanup, queue overflow, expiry
leaks, shutdown leaks, publication without the expected peer proof, pending
leases surviving admission stop, late reservations and premature removal of
authenticated sessions.

The model abstracts the handshake proof. Separate real UDP tests exercise
multiple simultaneous authenticated RPC sessions and datagrams on one listener,
wrong peers/PSKs/contexts, rewritten reservation selectors, canceled/expired
accepts, directory consumption/reprovisioning and native three-party capability
handoff that survives introducer disconnection. This is bounded component and
integration evidence, not a cryptographic proof or full composed refinement.
A separate custom-waker regression verifies that packet delivery and shutdown
permit reentrant registry access, including both packet reads and driver-stop
notifications.

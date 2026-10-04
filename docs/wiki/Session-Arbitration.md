# Native session arbitration

`Network::with_arbitration(local_identity, optional_connector, limits)` opts a
native RPC network into duplicate-session arbitration. Both peers must enable it.
Existing `new` and `with_connector` networks retain their previous stream profile.
Arbitration runs after pinned Native authentication and the encrypted native stream
preface, before any queued Cap'n Proto RPC bytes enter the selected stream.

The peer with the lexicographically smaller authenticated public key selects the
first eligible candidate whose hello it receives. The other peer acknowledges
that selection. The selecting peer then sends a commit, and the other peer waits
for that commit before publishing its stream. Each route generation has a random
16-byte epoch; decisions, acknowledgements and commits echo both epochs and the
leader's random 16-byte session selector. Handshake frames have fixed lengths,
strict message order and no downgrade fallback. A legacy or malformed peer fails
the connection rather than receiving application RPC data from this profile.

Crossed incoming and outgoing sessions join the same pending endpoint. Queued
calls, question IDs, capabilities and `Connection::connection_id()` remain bound
to that endpoint. The incoming session can win even if its outgoing connector is
pending or fails. Losing candidates and the unused dial are canceled when one
session wins. Established endpoints are never replaced by a duplicate.
Explicit `Handle::disconnect(peer)` permits a fresh generation. Alternatively,
`Network::with_options` can enable `recover_failed_routes` alongside arbitration
to replace a failed endpoint on the next connect/attach. Stale tasks hold only
their original generation and cannot update its replacement.

Once a candidate has been selected, failure closes that generation. It cannot
fall back to another candidate: the other peer may already have observed the
selection or commit. Applications reconnect through explicit disconnect or the
opt-in [recovery policy](Connection-Recovery.md). Arbitration does not guarantee both peers remain available
after the exchange, and it adds a hello/selection/acknowledgement/commit exchange
to connection setup. The policy does not prefer an outgoing connection: either
physical direction works even when only one side can dial.

`Limits` accepts 1–4 candidate sessions per generation and a positive timeout of
at most ten seconds from endpoint allocation for the entire pending generation,
including time before the task is polled and connector work.
The existing 64-route limit and bounded 64-KiB RPC bridges still apply. Candidate
admission is bounded and never waits for another candidate's packets. Dropping
the final endpoint or network, disconnecting, or expiry cancels pending work.
`Handle::selected_session(peer)` exposes the non-secret selector for diagnostics;
it grants no authority and does not keep the route alive.
After the route becomes authenticated, `Handle::take_datagrams(peer)` transfers
the winning session's datagram lane once. It returns no lane while arbitration
is pending or after route failure. Realtime routers can bind fresh grants to that
lane. Losing candidate lanes close; existing grants do not migrate on reconnect.

The remote provisioning service permits an incoming candidate while an arbitrated
outgoing route is pending. Its `Lease.ready()` acknowledges authentication and
admission to that pending endpoint. Arbitration must still finish before native
RPC is published. This avoids a dependency cycle in which the outgoing connector
waits for a provisioning acknowledgement while the host waits for arbitration
bytes from that connector. Legacy networks still acknowledge direct installation.
Provisioner revocation cannot revoke an admitted candidate; the route's selection,
deadline, disconnect and network lifetime govern it after admission.

## Wire profile and verification

Each side sends a 24-byte hello: `RPA1` followed by four zero bytes and its
16-byte route epoch. A control frame is 49 bytes: one kind byte, sender epoch,
receiver epoch, and session selector (16 bytes each). Kinds are reject (0),
select (1), acknowledge (2), and commit (3). All these bytes are carried inside
the existing authenticated ordered Native stream. This is a Capntproto transport
extension; the standard Cap'n Proto RPC schemas are unchanged.

Run `cargo nextest run --test protocol_models native_arbitration_model -- --exact` or the canonical
`cargo nextest run --locked --workspace --all-targets`. TLC checks one and two candidate sessions
after authentication and hello exchange. It explores message delivery, commit,
cancellation, closure, stale epochs and publication. Every graph edge prefix
replays production Rust handshake transitions and fixed-size frame encoding.
Seven mutations check multiple selections, early RPC data, skipped acknowledgement
or commit, fallback after a failed selection, stale epochs and publication after
closure. Handshake authentication and arbitrary packet/executor schedules are
outside this bounded model.
The one-candidate graph has 209 states and 409 edges; the two-candidate graph has
6,312 states and 21,200 edges. All 21,609 edge-prefix traces replay against Rust.
Separate temporal checks prove that both healthy peers become ready under weak
fairness of the handshake/message delivery actions. Suppressing the commit
message produces a temporal counterexample. Those progress checks assume valid
authenticated candidates, eventual delivery and no cancellation or connection
failure; deadline/cancellation regressions cover failure cleanup separately.

Real Native UDP tests cover both crossed-dial arrival orders, queued bidirectional
RPC, capability round trips, stable connection identity, cancellation of losing
sessions/dials, failed outgoing dials rescued by incoming sessions, fresh reconnects,
candidate quotas, incompatible profiles and native three-party provisioning and
handoff. They also exercise crossed remote provisioning, winning-session datagrams,
64-route quota, last-reference/network-drop cancellation, stale generation isolation
and timeout measured from allocation. Partial-I/O unit tests and malformed
version/epoch/token/order tests check the frame driver. The checks do not establish whole-runtime refinement or cryptographic
security. [Discovery and mobility](Discovery-and-Mobility.md) extend the selected session
with a capability directory, STUN rendezvous, CID rotation and validated path
changes. [Peer-acknowledged shutdown](Shutdown.md) drains the selected
session while preserving generation ownership and canceling losing candidates.

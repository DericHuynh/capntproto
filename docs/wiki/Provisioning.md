# Capability-based Native provisioning

`native_provisioning` supplies the control protocol for creating fresh native RPC
routes through the shared UDP listener. A `Provisioner` is a delegated capability
bound to one host, recipient public key, advertised socket address and application
context. Its parameterless `reserve()` cannot widen that authority. It returns a
fresh listener connection ID, random 32-byte PSK, the fixed bindings, and a `Lease`.
The secret authenticates admission through the shared TLS identity policy,
bound to both peers, the reservation ID and application context. It does not
select a different cryptographic handshake or act as a TLS traffic PSK.

Create the provider with `Provisioner::new(listener, network_handle, recipient,
advertised_address, context)` and export `client()` over an existing confidential,
authorized RPC connection. The listener and native network must have the same
local identity. Advertised addresses support configured NAT mappings but must
be concrete, non-multicast addresses with nonzero ports. Keep the shared listener
alive for installed sessions. Run all operations inside a Tokio LocalSet.

Configure `ProvisioningConnector::insert(host, provider, address, context)` and
pass it to `Network::with_connector`. Every dial requests a fresh reservation.
Before binding/sending UDP, the connector checks the returned host, its own
recipient identity, address, context, PSK length and connection-ID length. Neither
a peer contact nor a ticket authorizes an unconfigured destination. Directory
replacement affects future dials. It does not replace live RPC routes. Failed
routes require explicit `Handle::disconnect` under the default policy.
`Options::recover_failed_routes` permits a subsequent connect/attach to replace a
failed generation. Wrapping this connector in `RetryingConnector` adds bounded
setup retries with fresh reservations; neither policy replays RPC methods.
See [Connection Recovery](Connection-Recovery.md). Each provisioning dial has a ten-second
deadline including the control request, additionally bounded by the configured
per-attempt and overall network deadlines.

The control route must be independent of the route under construction. At an
introducer, `relay(provider)` supplies a local forwarding server for both the
provider and its returned leases. Export that relay on the recipient's existing
connection to the introducer. This keeps control calls on the established
host–introducer–recipient path, even when the native RPC engine can introduce
ordinary capabilities directly. Exporting the original provider directly can
create a circular dependency on the missing host route. Relay rights are exactly
the authority of the supplied capability; relaying does not identify the caller
or change the pinned recipient.

## Lease and revocation semantics

The host starts accepting immediately after allocating a reservation. `ready()`
waits for the pinned Native handshake, encrypted native-stream gate, and successful
installation in the host's native network. The connector returns its session
only after both its local authentication and this host acknowledgement succeed.
A duplicate route or failed attachment cannot yield successful readiness.
Provisioning does not export an object capability; normal Bootstrap or native
Provide/Accept still governs RPC authority and handoff ordering.

For [arbitrated networks](Session-Arbitration.md), readiness acknowledges admission
of the authenticated candidate to a pending endpoint. The network then selects
one shared stream before publishing any RPC bytes. The connector must receive
the admission acknowledgement before it can participate in selection; waiting
for completed selection inside that acknowledgement would deadlock crossed dials.
Pending arbitrated routes accept incoming candidates, while established and
failed routes keep their existing duplicate-rejection policy.

`cancel()` retires only a pending lease and is idempotent. Dropping the last lease
reference and outstanding ready call cancels its pending accept task. Dropping a
ready waiter alone preserves other lease references. Reservation expiry and
handshake failure also retire pending work. The listener timer starts at
allocation, including time spent delivering the ticket. Task abortion releases
listener resources on a subsequent executor turn.

Once authentication claims the session for synchronous route installation,
cancellation and provider revocation cannot undo it. A failed installation still
rejects readiness. Successful routes outlive the lease, provider revocation and
introducer connection. This acknowledgement does not promise perpetual peer
availability: either side can disconnect immediately afterward. Explicit network
disconnect and listener closure retain their existing behavior.

`Provisioner::close()` rejects future reservations and cancels pending leases.
Dropping the final local provider owner and exported provider capability does the
same. Leases do not keep that provider owner alive. There is one pending/installing
lease per provider, at most 64 ready waiters per lease, and at most 64 configured
peers per connector. The listener additionally bounds active routes, queues and
4,096 lifetime reservation IDs. Separate provider authorities share those listener
limits. A relay does not add a second reservation or reset any deadline.

Owned PSK buffers are zeroized when dropped. Serialized RPC messages and the
transport's own internal buffers are subject to their existing allocation
lifecycle; there is no claim of erasing every historical copy. Carry tickets on
confidential control routes and avoid logging them. This adds authorized remote
provisioning. Optional [simultaneous-dial arbitration](Session-Arbitration.md) is now
available. [Capability discovery and STUN rendezvous](Discovery-and-Mobility.md) now
support dynamic endpoint lookup and recipient-authorized punching. A named
binding can publish a new key for subsequent connections; existing vat IDs stay
immutable. Full ICE/TURN deployment and zero-round-trip application data remain
unimplemented.

## Verification

Run `cargo nextest run --test protocol_models native_provisioning_model -- --exact` or the canonical
`cargo nextest run --locked --workspace --all-targets`. The focused report is
`reports/capntproto/native-provisioning/verification.json`.

`RpcNativeProvisioning.tla` explores two successive leases, one waiter per lease,
allocation/rejection, authentication claim, attach success/failure, acknowledgement,
capability/waiter release, cancellation, expiry, revocation and disconnect. Every
edge prefix replays production Rust registry and lease futures. Seven mutations
check closed issuance, overlapping allocations, unauthenticated installation,
early acknowledgement, stale cancellation, installed-route revocation and orphaned
accepts. Handshake proof, clock, network attachment and RPC delivery are abstract
in this component model; existing models cover their separate bounded behavior.
The graph contains 8,324 states and 46,217 edges, all replayed as edge-prefix traces.
TLC also checks that pending leases settle and outstanding ready waiters finish,
assuming weak fairness of local expiry, installation and ready callbacks. This
does not assume that a peer finishes Native. Removing expiry fairness yields a
temporal counterexample, demonstrating that progress needs the local timer and
executor to run; it is not a claim about arbitrary stalled executors.

Integration tests exercise actual Native UDP and native three-party capability
handoff with provisioning relayed across both encrypted control hops, retaining
the direct capability after introducer disconnect and provider revocation. Other
tests cover real RPC release/cancellation, deadline expiry, binding rejection,
automatic fresh provisioning on reconnect and competing-provider attach races.
Cancellation during control-reply delivery and final provider-owner release are
checked to reclaim pending accepts before their ten-second deadline. Unit tests
cover malformed ticket fields, waiter quotas and reentrant completion callbacks.
This is bounded lifecycle and integration evidence, not a cryptographic proof or
complete composed-model refinement.

# Discovery, rendezvous and mobility

The native Noise profile now supports recipient-scoped service discovery,
STUN-assisted UDP rendezvous, connection-ID rotation and validated client path
migration. Active caller pipelines can also shorten through an authenticated
Join fence. These operations retain the existing
`Noise_IK_25519_ChaChaPoly_BLAKE3` / `Noise_IKpsk2_25519_ChaChaPoly_BLAKE3`
profiles and Snow feature flags.

## Discovery and service identity rotation

`noise_discovery::Directory` is the administrative publication handle.
`Directory::client(recipient)` grants read access to that recipient's bindings.
Distribute this capability over an existing confidential, authorized control
connection. A reader cannot register or replace entries. The directory is a
trusted bootstrap authority for names; it is not an unauthenticated global
registry, DNS replacement or authority to invoke an application object.

Publish a `Binding` containing the host's Noise public key, recipient public
key, endpoint, application context and recipient-bound `Provisioner` capability.
Names contain 1–128 ASCII alphanumeric or `._-/` characters. Entries live for
at most 60 seconds. `publish(name, binding, expected_generation, lifetime)` uses
compare-and-replace: `None` creates an absent name; renewals and replacements
require `Some(current_generation)`, even after expiry. Publication, renewal,
advertisement status and lookup results share the nonzero `DiscoveryGeneration`
type. `get()` explicitly projects its wire number; `new(number)` imports nonzero
checkpoint metadata. Generations are directory-local comparison values, not
ownership or authentication evidence. They increase across
revocation and recreation. A stale revoke cannot delete a replacement.
The final `u64` value is usable once; exhaustion fails without wrapping, changing
the existing binding or extending its deadline. Revoke/close remain available.

The directory holds at most 64 names/recipient pairs. Expired entries remain
available to their publisher for compare-and-replace; revoke unused names to
release slots. There is one host binding per recipient, making key lookup
unambiguous. `close()` prevents further publication and lookup. For low-level
`publish`/`maintain`, revocation affects future lookups and does not revoke an
already delegated provisioner. Close that authority explicitly, or use the
managed advertisement below to coordinate its lifecycle. Established application
capabilities are independent of directory publication.

`DiscoveryConnector::new(identity, bind, reader, optional_stun_server)` implements
the native `Connector` trait. It looks up the requested key on every dial and
obtains a fresh reservation. A lookup cannot substitute another key or recipient.
Single-reader lookup has a three-second budget; setup also respects the returned entry's
remaining lifetime, conservatively measured from request start. Returned
providers are wrapped by `noise_provisioning::relay` so they stay on the control
route while the data route is being created.

`resolve(reader, recipient, name)` returns an immutable `Resolved` value.
`binding()`, `generation()` and `expires()` expose the validated record and its
conservative local deadline. A cloned binding is a separate publication proposal;
it cannot change the captured result or be converted back into a resolved result.
`connect_resolved(result)` consumes that result, rechecks recipient and expiry,
and enforces expiry throughout setup. Noise still authenticates the pinned host.
Retaining a result does not follow later directory changes or subscribe to
revocation; a previously delegated provider remains usable until its own
authority ends or the captured lookup expires.
`DiscoveryConnector::connect_name(name)` resolves and authenticates it in one
operation. Attach the returned session under `session.peer()`. Replacing a named
binding with a new host key therefore moves subsequent name-based connections
to that key. Existing sessions and capabilities retain their original vat ID.
Applications own private-key generation and retirement;
there is no background key-rotation scheduler or transparent relabeling of a
pinned vat, introduction token or SturdyRef.

### Discovery control-route failover

Pass a `Discovery` instead of a single reader to use several independently
delegated control routes:

```rust
let discovery = reproto::noise_discovery::Discovery::new(
    vec![primary_reader, secondary_reader],
    reproto::noise_discovery::DiscoveryOptions::default(),
)?;
let connector = reproto::noise_discovery::DiscoveryConnector::new(
    identity, bind, discovery, None,
)?;
```

`Discovery::resolve(recipient, name)` and `lookup(recipient, host)` also expose
the lookup policy directly. Configure one to four readers, in preference order.
Each lookup starts with the first reader and tries each configured entry at most
once. There is no binding cache, persistent failure cache or shared retry cursor.
The default per-reader deadline is three seconds and the total lookup budget is
six seconds. Both durations must be positive; the per-reader deadline must not
exceed the total, and the total cannot exceed 30 seconds. The remaining overall
budget caps each attempt. A single reader keeps its three-second bound.

Only `Disconnected`, `Overloaded` and timeout advance to the next reader.
An absent/revoked entry, a rejected or unsupported method, a malformed response,
an expired record or a mismatched recipient/pinned host ends the lookup. A
successful binding ends failover before provisioning starts: failed reservation
or Noise authentication does not select another authority. Binding expiry is
measured from the successful reader's request start, conservatively accounting
for transit time. A response at or after an attempt/overall deadline is discarded
even when it is ready before the timer is polled. Dropping the lookup stops local
progress and drops the outstanding request; no detached retry task continues.
It does not promise to cancel work already executing at a remote directory.

Each configured capability is an explicitly trusted discovery authority for the
query. For names, any reached reader can select the host key; pinned-key lookup
still requires an exact match. Failover does **not** synchronize independent
directories or establish a shared revocation order. If the preferred reader is
unavailable, a secondary may still return its older binding. Deployments must
coordinate publication/revocation across those authorities, or delegate multiple
control routes to the same directory owner. Readers still need an existing
confidential authorized control route; public bootstrap, automatic acquisition
of new directory capabilities and directory replication remain separate work.

### Owned publication renewal

`Directory::maintain(name, binding, expected_generation, lifetime)` publishes
immediately and returns a `Publication` owner. Run it on a Tokio LocalSet. It
renews at half the chosen lifetime (1–60 seconds), with a new generation on each
renewal. `generation()` returns the last generation this owner published;
`status()` reports whether it is active, revoked, superseded, expired, closed,
exhausted or explicitly stopped. A missed expiry ends renewal permanently even
though an administrative `publish()` can explicitly republish an expired entry.

Dropping the owner or calling `stop()` cancels its task and revokes only its own
current generation. To replace the binding, call `maintain` with
`Some(old_owner.generation())` before dropping that owner. The old owner's
cancellation cannot delete the successor. Administrative revocation and directory closure wake the
renewal task; it cannot recreate a removed name. Terminal cleanup releases the
entry slot. Directory lookup delegates only read authority, not this renewal
authority or access to arbitrary publications.

Renewal establishes neither provider health nor listener health. Keep the owner
with the service's lifetime and stop it when the service becomes unavailable.

## NAT rendezvous

`Listener::bind_discovered(bind, identity, limits, stun_server)` returns the
listener and its observed address. Discovery occurs before packet routing starts,
on the exact socket later used by the shared listener. The publisher advertises
this address in its directory binding and provisioner.

`Provisioner::with_rendezvous(...)` additionally delegates bounded UDP punching
to its named recipient. Ordinary `Provisioner::new(...)` rejects punching
requests. `ProvisioningConnector::with_stun(...)`, or a `DiscoveryConnector`
configured with STUN, discovers the recipient's mapping before reserving and
dials from the same socket. The reserve request carries that mapping. The host
sends 20-byte Binding indications at most every 250ms until authentication,
cancellation, provider closure or reservation expiry. Reservation and lease
limits still apply. Export rendezvous authority only where sending these probes
to recipient-supplied addresses is intended.

The address-discovery usage implements unauthenticated STUN Binding requests,
IPv4/IPv6 XOR-MAPPED-ADDRESS parsing and optional FINGERPRINT validation. It checks
source address, random 96-bit transaction ID, exact framing, attribute lengths
and required-attribute handling; it rejects oversized datagrams and does not
follow alternate-server redirects. Retransmissions start at 500ms and back off
to two seconds within a bounded budget. Addresses and Binding indications grant
no identity or capability authority. Noise must still verify the pinned keys,
fresh reservation PSK, connection ID and application context before publication.

This enables direct traversal where STUN mappings are reachable by the peer and
reciprocal outbound packets open filtering entries. STUN alone cannot guarantee
traversal: see [RFC 8489](https://www.rfc-editor.org/rfc/rfc8489.html) and the
broader candidate-checking procedure in
[RFC 8445](https://www.rfc-editor.org/rfc/rfc8445.html). Endpoint-dependent
mapping, multiple candidate nomination, TURN relaying and a production server
daemon remain separate work. Failed direct
setup is bounded; it is not a claim of universal NAT reachability or automatic
application-call replay.

### Idle listener mapping refresh

`listener.maintain_mapping(stun_server, MappingOptions::default())` starts one
owned refresh service on the listener's existing UDP socket. Keep the returned
`Mapping` owner alive. The driver starts a Binding transaction and refreshes
15 seconds after each completed transaction by default; configuration permits
15–120 seconds between transactions and a positive timeout of at most ten
seconds (default three). Each transaction has a fresh random ID and bounded
500ms/1s/2s retransmissions. Missed timer ticks do not produce catch-up bursts.
The packet loop sends probes without blocking RPC on socket writability and
demultiplexes replies before connection-ID routing.

`changed().await` reports observations, failure or stop; changes may coalesce.
`status()` and `address()` suppress an observation older than interval plus
timeout, even before the driver's next timer tick. Failure withdraws the address
hint and the next scheduled transaction can recover. Replies from another
source, old transactions, duplicates and replies at/after the deadline cannot
publish a new observation. Transient UDP/ICMP errors retire a pending refresh
without closing the shared listener's authenticated routes.
`subscribe()` creates independent `MappingObserver`s without retaining ownership
of the mapping or listener. Their `status()`/`changed()` methods also report
observation expiry even if the packet driver has not yet reported refresh failure.
Expiry is reported once per observation, so repeated waiting does not busy-loop.

`close()` or dropping the mapping owner stops probes. Closing/dropping the final
listener stops the mapping too; a mapping owner does not retain the listener.
Only one mapping service is active per listener. A closed old owner cannot stop
its replacement. `stop_accepting()` preserves an existing refresh service while
sessions drain, but refuses a new one.

These Binding transactions maintain and observe the mapping to the configured
STUN server. They do not keep every peer-specific NAT filter alive, guarantee a
NAT lifetime, keep an RPC session past its idle timeout, or prove reachability.
STUN's role and limitations are described in [RFC 8489](https://www.rfc-editor.org/rfc/rfc8489.html).
Use a managed advertisement below to coordinate these observations with provider
and directory updates. Low-level users must implement that coordination
themselves. Neither path revokes already installed capabilities. A production
deployment daemon remains separate work.

### Managed mapped-service advertisements

`MappedService::new(listener, network_handle, &mapping)` checks that the listener,
Noise identity and mapping belong together. It can advertise to several
recipients using one mapping. Keep the `Mapping` owner alive separately: the
service observes it and does not own or restart it. The service and advertisement
owners retain listener/network handles. Drive their network with an RPC system.

After a fresh mapping observation, call
`service.advertise(&directory, name, recipient, context, options)` on the LocalSet.
For example, using a listener and network handle for the same identity:

```rust
let mut mapping = listener.maintain_mapping(stun_server, Default::default())?;
mapping.changed().await; // advertise() rejects unavailable/stopped observations
let service = reproto::noise_discovery::MappedService::new(listener, handle, &mapping)?;
let advertisement = service.advertise(
    &directory, "objects", recipient_key, b"objects-v1",
    reproto::noise_discovery::AdvertisementOptions {
        rendezvous: true,
        ..Default::default()
    },
)?;
// Keep mapping and advertisement with the service's lifetime.
```

Options default to a new name (`expected_generation = None`), a 30-second renewed
lifetime and **no punching authority**. Set `rendezvous` explicitly to delegate
punching to that recipient. Lifetimes remain 1–60 seconds; name, recipient,
context, host uniqueness and 64-entry limits are unchanged.

For each mapping change, the coordinator closes the previous provisioner and
publishes a matching new provider/address in one directory replacement. Cached
old providers remain closed, and pending reservations are canceled. On
`Unavailable`, it hides the entry from both name and host lookup, closes its
provider and continues renewing the hidden generation-owned claim. The claim
still occupies a directory slot and prevents another publisher using an
absent-name comparison (`None`) from taking the name. Recovery obtains fresh
provider authority and restores lookup under a new generation, even when the
address is unchanged.

An administrative revoke, replacement, directory closure or missed publication
expiry permanently ends that advertisement; a later mapping recovery cannot
recreate it or overwrite its successor. Listener admission stop, network
shutdown/drop, mapping stop, owner stop/drop and driver cancellation also retire
it. An old owner's cleanup cannot revoke a replacement. `status()` reports the
last processed `Published`, `Suspended` or `Stopped` state; `changed().await`
observes transitions, including renewal generations. `generation()` supplies the
current owned generation for administrative CAS; `failure()` reports an internal
update error if one terminated the coordinator.

External notifications are processed by the LocalSet; they do not promise
unconditional progress on a stalled executor. Explicit owner `stop()` performs
cleanup synchronously. Neither suspension nor stop replays application methods,
changes an authenticated session's identity, stops the shared mapping, or closes
other recipients' advertisements. Existing authenticated RPC/capabilities remain
usable until their own session or service is closed. The low-level directory and
provisioner APIs remain available for applications choosing another lifecycle.
Endpoint updates apply to fresh connections. If an existing route's old network
path disappears, it still needs validated path migration or
[connection recovery](NOISE_RECOVERY.md); advertisement updates do not replay
calls or guarantee that the old path remains reachable.

## Connection IDs and path changes

[Local scheduling controls](NOISE_SCHEDULING.md) bound packet bursts and optionally
pace outgoing datagram admission without changing session identity. They remain
attached to the same session through the path/CID operations below.

Obtain `session.mobility()` before attachment or `handle.mobility(peer)` for an
authenticated installed route. The control does not keep a dropped session
alive. Its queue holds at most eight commands.

`rotate_connection_id().await` publishes a new source connection ID and requests
retirement of older IDs as required by the negotiated allowance. Success means
local publication, not a peer retirement acknowledgement. Drivers replenish
spare IDs up to the negotiated limit of four. Shared listeners register each ID
before publication, route it to its owning session, and remove it on retirement
or owner cleanup. `Stats::issued` counts reservations; `connection_ids` includes
replacement IDs. The listener's 4,096 lifetime ID limit and each session's 128
additional-ID limit bound retained history.

`mobility.migrate(new_socket, peer_address, timeout).await` probes a concrete
client source address. Application traffic stays on the existing socket until
quiche validates the candidate with encrypted PATH_CHALLENGE/PATH_RESPONSE.
Only then does the driver switch its active path. The server follows the
authenticated migration. Validation follows quiche's existing machinery
described by [RFC 9000, section 8.2](https://www.rfc-editor.org/rfc/rfc9000.html#section-8.2);
this custom Noise profile does not offer standard TLS QUIC interoperability.

There is one pending candidate, at most eight probe attempts per session, and a
maximum ten-second probe deadline. Failure or cancellation before commitment
keeps the old path. Cancellation after commitment cannot undo the migration.
Packets for discarded source sockets are dropped rather than sent from the wrong
address. The client supplies a suitable new socket; automatic interface discovery
and server-initiated address changes are not provided. Path migration preserves
the authenticated session, RPC generation and datagram lane. It is different
from reconnecting or changing a static identity key.

## Active caller-pipeline migration

Native Noise enables `Connection::supports_pipeline_join_fence()`. After answer
adoption, used or encoded caller references queue subsequent calls while
independent authenticated Join shares follow the old and direct paths. The old
share is ordered behind earlier calls, and equality must be established at the
same capability. Promised-answer targets allow this after `set_pipeline()` and
before final Return. The acquired capability preserves opaque endpoint policy.

The answer-setup deadline also bounds this fence. Unsupported paths, inequality,
failure or timeout release queued calls onto the old route. Those calls have not
yet executed and are not replays. Other holders retain their own references;
shortening one caller does not rebind every remote holder. Networks that do not
opt in retain the previous ordered-relay behavior. No new standard wire message
or third-party use of `senderLoopback` is introduced. See
[ANSWER_ADOPTION.md](ANSWER_ADOPTION.md) for lifecycle details.

## Verification

Run the focused checks through Cargo:

```sh
cargo test --locked --test noise_deployment --test noise_pipeline_migration
cargo test --locked --lib nat::
cargo test --locked --lib noise_discovery::publication::
cargo test --locked --lib noise_discovery::advertisement::
cargo test --locked --lib noise_discovery::readers::
cargo test --locked --test noise_discovery_failover
cargo test --locked --lib transport::mobility::
```

Fresh TLC explorations are replayed by native Rust tests:

| Model | States | Transition-prefix replays | Rust boundary |
|---|---:|---:|---|
| `NoiseDiscovery` | 447 | 895 | Directory publication, expiry, rotation, revocation and RPC lookup |
| `NoiseRendezvous` | 18 | 23 | Real UDP punching, Noise authentication, provisioning cancellation/expiry and late dials |
| `NoisePathMigration` | 8 | 7 | Driver's validation/commit/retirement gate |
| `RpcPipelineFence` | 23 | 22 | Three real Noise RPC systems, queued/canceled calls, Join, timeout fallback and relay loss |
| `NoisePublication` | 267 | 454 | Production lease renewal, expiry, generation replacement, revocation, stop and closure |
| `NoiseMappingRefresh` | 63 | 122 | Production refresh state, timeout withdrawal, source/transaction checks, retry and closure |
| `NoiseAdvertisement` | 86 | 133 | Production coordinator, generated lookup/reservation calls, suspended claims, fresh providers, administrative retirement and cancellation before the driver's first poll |
| `NoiseDiscoveryFailover` | 67 | 110 | Production lookup loop and generated directory calls, three readers, availability errors, authoritative rejection, invalid key, deadlines, cancellation and late replies |

The first six models have three named invariant-breaking mutations each;
`NoiseAdvertisement` has four (mismatched address/provider, stale provider
authority, resurrection and stale-owner deletion). `NoiseDiscoveryFailover` has
three (retrying authoritative rejection, accepting a substituted key and
committing a late reply). Temporal checks state their
local-progress fairness assumptions. Separate integration tests exercise real CID
rotation and source migration with RPC and datagrams, failed/canceled paths,
encoded pipeline references and service-name key replacement. Parser tests check
STUN framing, transactions, source matching, fingerprints and timeout. These are
bounded component checks and loopback integration evidence, not real-world NAT
qualification, a cryptographic proof or a composed whole-program refinement.
Publication tests also exercise the actual renewal task and generation exhaustion.
A shared-listener integration test exercises repeated STUN refresh, outage,
changed mappings and cancellation alongside real UDP RPC, checking that the
authenticated connection generation is preserved.
Managed-advertisement integration tests use a real directory RPC control route,
three recipient scopes, STUN outage/recovery, and a replacement endpoint backed
by a loopback UDP forwarder. A fresh Noise/RPC connection succeeds through the
new endpoint while an existing connection survives unchanged. Other checks cover
provider cancellation before reservation expiry, revocation during suspension,
administrative replacement, listener/network shutdown and reentrant stop from
provider/publication callbacks. The coordinator model abstracts mapping events;
STUN parsing/refresh has its separate model and actual UDP tests. A loopback
forwarder is not qualification against endpoint-dependent NATs or TURN.
Discovery-failover integration tests disconnect one directory RPC control route,
resolve through another, authenticate a fresh Noise session, and invoke RPC after
both control routes are gone. Both named and pinned-key connector paths are
exercised. Other checks reject fallback after authoritative absence or failed
provisioning, exercise all four reader slots, and verify that each new lookup
starts with the preferred reader. The model bounds lookup to three readers and
abstracts clock ticks and RPC outcomes; it is not a directory replication model.

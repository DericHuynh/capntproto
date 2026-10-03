# Multiparty capability Join

`RpcSystem::get_joiner().join(capabilities)` supports independent proxy paths
on the native network. Each input carries a separate random key share in
a standard RPC `Join` message. Transparent RPC proxies forward each part
independently; they retain the downstream response until the corresponding
upstream Finish. An opaque wrapper receiving an individual share is an equality
endpoint, preserving its authority boundary. A complete caller-side batch can
cross a membrane through explicit [policy-preserving delegation](Capability-Join.md#policy-preserving-membrane-join)
before the runtime routes its independent shares.

At the ultimate host, parts are grouped by operation and settled capability
identity, never by host alone. Each part gets an immediate `JoinResult`, even
before all parts arrive. Different objects on the same host receive different
random group IDs; different hosts report different pinned Native identities.
The initiating vat compares every result and returns `None` for conflicting
endpoints. Malformed responses, unsupported routes, proof failures and broken
capabilities return errors.

When all distinct parts for one object arrive, the host combines their shares
and includes a host proof in the final result. The caller verifies that proof
before contacting the host. It then sends a separate caller proof in a standard
`Accept` over the direct Native connection. The host checks the authenticated
peer identity, complete share set, operation, object group, cancellation state
and caller proof before releasing the capability. The returned capability is
independent of both proxy paths.

## Network and security contract

The schemas retain the standard `Join.keyPart`, `Return.results` and
`Accept.provision` fields. Their network-specific payloads use Cap'n Proto Data
values with the following versioned encoding. Integers are big-endian.

| Payload | Fields in order | Bytes |
| --- | --- | ---: |
| Key part | `RJK1`, caller key (32), operation nonce (32), count (2), index (2), share (32), remaining hops (2) | 106 |
| Result | `RJR1`, caller key (32), nonce (32), count (2), index (2), host key (32), group ID (32), arrival ordinal (2), proof or zeroes (32), proof-present flag (1) | 171 |
| Acceptance | `RJA1`, caller key (32), nonce (32), group ID (32), caller proof (32) | 132 |

Shares are independent 256-bit OS-random values. Their XOR is the operation
key. The host and caller proofs are HMAC-SHA-256 over `Capn't Proto Native Join v1\0`,
the role (`host` or `caller`), caller key, nonce, count, host key and group ID.
Verification uses the cryptographic library's constant-time MAC check. Role
separation prevents reflection of the host proof as caller authentication.
A relay with only a proper subset of shares cannot produce either proof under
the MAC security assumption. Relays that jointly see every share can cooperate;
this is the trust boundary described by the standard Join protocol.

This is additional object authorization over authenticated Native transport.
It does not replace or derive the Native reservation admission secret. New routes use the
configured connector and its existing peer/PSK policy; existing authenticated
routes can be reused. The shared TLS identity checks and
TLS 1.3 with pinned mutual authentication / TLS 1.3 with reservation admission
suites are unchanged. Addresses and discovery remain connector policy;
untrusted Join results supply a pinned identity, never a socket address.

`VatNetwork::join_network()` supplies the network profile;
`Connection::supports_multiparty_join()` enables its wire handling.
`JoinNetwork` and `JoinSession` separate share/authentication policy from RPC
question ownership. Defaults preserve compatibility with existing adapters.
The Native adapter enables the profile. Bilateral networks continue to use
`rpc-twoparty` Join; their encoding is not used for independent paths.
`ClientHook::forward_join()` forwards only transparent RPC clients. Its default
identifies an opaque/local endpoint.

## Lifetime and limits

An incoming Join answer retains its registration or downstream response until
Finish, including results that contain no capability. The initiating caller
retains every part until it has acquired the direct capability. Dropping its
pending Join sends Finishes on the outstanding questions. At the host, any
Finish before acceptance invalidates that operation and clears the collected
secret. A late part or acceptance of a retired operation fails. Accepted
capabilities survive release of the old paths. Acceptance is single-use.

Secret share/key arrays are zeroized when their owners are released. Serialized
wire-message buffers are ordinary Cap'n Proto allocations and are not promised
to be scrubbed. Proxies must treat key-part messages as sensitive data.

The Native profile permits 4,096 live parts and bounds active groups plus retired
operation records at 4,096. Retired nonces persist for the network lifetime;
exhaustion returns an overload error. Each honest forwarding hop consumes a
budget initially set to 64. As with other RPC calls, applications requiring a
bounded wait must apply a deadline. Failed direct dialing returns an error;
there is no automatic replay of a Join on another route.

## Verification

`cargo test --test protocol_models multiparty_join_model -- --exact` checks
`RpcMultipartyJoin.tla`: **326 states and 392 edge-prefix replays** for two and
three shares, equal objects, different objects on one host, different hosts,
tampered shares, proof validation, direct acquisition, cancellation and
independent Finish. Replays use production parsing, share combination, MACs,
registration and acceptance code. Seven injected faults must be detected.
Fair arrival, decision, acceptance and Finish imply eventual settlement and
release in these finite scenarios.

The original `CapnpJoin` configurations additionally check independent wire
delivery, abstract share secrecy, distributed equality, retention, progress
and completion/cancellation witnesses. Separate Rust tests exercise malformed,
duplicate and retired parts, proof forgery by a partial host, peer/role binding
repeated acceptance, resource quotas and Join-handle shutdown ownership. Real four/five-vat Native tests send standard Join
messages through separate proxies, establish the direct route on demand, check
two/three shares and unequal endpoints, cancel before acceptance, check legacy
Join rejection without breaking existing capabilities, and call the
joined capability after both relays stop. Membrane regressions exercise explicit
caller-side and nested authorization, denial before any host contributions, and
policy interception and revocation of the directly acquired capability after
both relays stop. Individual-share membrane forwarding remains disabled.
Bilateral Join regressions also run.

These checks assume ideal cryptography in TLA+; Rust replay exercises the actual
MAC implementation. They do not prove cryptographic security or whole-runtime
refinement under arbitrary transport/executor schedules. The pinned C++
dispatcher does not implement general Join, so there is no C++ general-Join
interoperability claim.

# Capability Join

`RpcSystem::get_joiner()` returns a clonable handle for capability equality:

```rust
let joiner = system.get_joiner(); // Obtain before spawning the RPC driver.
let joined = joiner.join(vec![first.clone(), second.clone()]).await?;
if let Some(cap) = joined {
    // cap retains the authority of the joined object.
}
```

Inputs have the same generated client type; use untyped capability clients when
joining different interface views. One through 65,535 inputs are accepted.
Resolution waits for promise inputs and detects cycles or excessive depth.
Equal settled identities return a retained capability. Distinct settled local
objects return `None`. Broken promises, unsupported routes, malformed replies
and disconnects return errors. They do not establish inequality.

Distinct remote inputs sharing one supported bilateral connection use the
standard `Join`, `rpc-twoparty.JoinKeyPart` and `JoinResult` wire structures.
The receiver collects all parts before performing a local join or forwarding a
new complete join across the next boundary. All results agree on success, and
exactly one successful result carries the joined capability. Transparent RPC
proxies can forward across independent `RpcSystem`s and different `VatId` types:
the input's private client hook owns routing and retains its response context.
Opaque hooks, including membranes, do not delegate Join implicitly. Equal
aliases preserve their wrapper. Membranes may explicitly authorize complete
batches as described below; the default policy rejects distinct wrappers.

`Connection::supports_two_party_join()` defaults to false. The ordered-stream
two-party network enables it. A custom network may enable it only for connections
that actually bisect the network, as required by `rpc-twoparty.capnp`. It must
not be enabled merely because a multiparty transport has bilateral links.
The bilateral algorithm does not handle inputs spanning independent connections
or reflection back onto the incoming connection. The Native network instead
implements [multiparty secret-share Join](Multiparty-Join.md), including object
discrimination, share-possession proofs and direct authenticated acquisition.

Every incoming part retains an answer until Finish, even for empty results.
Relays retain all downstream results until every upstream part is finished.
Dropping a pending join or finishing a pending incoming part cancels its batch
and releases downstream questions. Connection failure cancels pending work.
Outgoing Join IDs stay reserved until all corresponding Finishes are sent;
incoming IDs remain reserved while any part's answer is live. Parts are stored
sparsely. Invalid counts, part numbers, duplicate parts/question IDs or inconsistent
counts abort the offending connection. Inconsistent result IDs, disagreement,
missing or duplicate successful capabilities, and capabilities on failure reject
the batch. A Join Return declaring `noFinishNeeded` aborts the connection.
Legacy `Unimplemented(Join)` rejects the operation while preserving ordinary calls.

## Policy-preserving membrane Join

`membrane::Policy::allow_join(direction, targets)` opts a membrane into joining
distinct wrapped capabilities. It defaults to `Ok(false)`, which returns an
unsupported-operation error rather than establishing inequality. The callback
receives the underlying clients already governed by that policy. Returning true
authorizes the operation; the runtime still proves equality using its existing
local, bilateral or native multiparty algorithm.

Every input must cross the same membrane instance in the same direction.
Mixed boundaries, opposite directions and custom-substitution wrappers reject
before authorization; sharing a root policy is insufficient. Each nested
boundary authorizes its own batch. Equal aliases retain their existing wrapper
without invoking the callback. Normal resolution and explicitly configured
custom substitutions retain their existing crossing semantics.

Successful results cross back through the membrane's normal wrapping and
transformation policy. Calls on the returned capability remain intercepted, and
revocation still applies. Pending joins race against policy revocation; checks
after authorization and result transformation also catch callbacks that revoke
reentrantly. Dropping the join cancels downstream questions. Downstream response
guards survive wrapping and remain retained until all upstream parts Finish.

Delegation is bounded to 64 nested boundaries, independently of the existing
1,024-step promise-resolution limit. Other opaque hook implementations remain
equality endpoints unless they implement the private `ClientHook::delegate_join`
contract, validating the complete batch and preserving result policy and guards.

This authorization applies to complete batches available locally or collected
by bilateral Join. An individual multiparty secret share arriving at a remote
membrane still treats that wrapper as its equality endpoint. `forward_join`
does not implicitly forward shares through membrane policies. Caller-side
authorized batches can use independent Native paths after delegation.

Run `cargo nextest run --test protocol_models membrane_join_model -- --exact` for **246 TLC states,
640 Rust wire trace replays and eight detected fault mutations**. The bounded
model covers two imports, both directions, authorization, mixed boundaries,
equal/unequal results, either response order, cancellation, revocation,
disconnect, protected result calls and Finish cleanup. Separate regressions
cover nested policies, related roots, opposite directions, policy revocation
signals, reentrant callbacks, the depth limit and retained downstream answers
across independent RPC systems. Real Native tests verify local and nested
policies, denial before shares are sent, direct capability calls after both
relays close, and continued interception and revocation. These are bounded
checks, not a proof for arbitrary policies or whole-runtime composition.

## Bilateral verification

Run `cargo nextest run --test protocol_models two_party_join_model -- --exact` for TLC and production
Rust trace replay. `RpcTwoPartyJoin.tla` checks four local/relayed, equal/unequal
scenarios: 408 states and 536 edges. Each edge is exercised through a shortest
prefix and replayed through real Join wire messages. Checks cover arrival and
reply order, promise resolution, cancellation, disconnect, response authority,
Finish retention and one legal ID reuse. Nine injected faults must violate their
specified invariants. The original `CapnpTwoPartyJoin.tla` local, remote and
unequal configurations and its completion witness also run.

These are finite, drained-executor scenarios with two parts and at most one
relay, not whole-protocol refinement. Separate Rust regressions cover malformed
parts/results, legacy rejection, retained capability calls, fragmented streams
and a bridge between independent systems. The C++ interoperability suite checks
Join rejection followed by continued calls on both original capabilities.

General multiparty secret-share Join, authenticated acceptance and direct route
formation are implemented in the [Native Join profile](Multiparty-Join.md).
[Third-party answer adoption](Third-Party-Answers.md) is implemented separately.
The pinned C++ dispatcher leaves Join and ThirdPartyAnswer unimplemented; these
implementations extend protocol support beyond that dispatcher.
Native suite selection and introduction protocols are unchanged.

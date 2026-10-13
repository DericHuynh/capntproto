# Capability equality and ordering

This audit applies the Grant Matcher Puzzle, reference sameness, Lost Resolution,
and partial-order examples in the supplied E equality research to Capntproto.
E's language-wide equality and reference taxonomy are not the Cap'n Proto API.
The relevant security requirement is that proving equality must neither widen
authority nor let a new route bypass earlier calls.

## Findings and defenses

| Failure mode | Implementation and executable evidence |
| --- | --- |
| A selective forwarder answers equality queries honestly, then steals the valuable call | Join compares settled runtime identities or authenticates independent shares; it never invokes an application `equals` method. The result is a capability, not a boolean authorizing use of an arbitrary input. The grant-matcher regression in [equality.rs](../../tests/multiparty_join/equality.rs) exercises a forwarding server that redirects the valuable call, in both input orders and both drivers. |
| Equality strips an opaque policy boundary | Membranes retain wrapper identity. Delegation requires explicit authorization of the entire batch and wraps the result again. Individual shares cannot implicitly cross a membrane. [Membrane Join tests](../../tests/membrane_join.rs) exercise mixed boundaries, revocation, nesting, reentrant callbacks and retained guards; [Native tests](../../tests/multiparty_join.rs) exercise both drivers. |
| Unknown capability equality becomes false or true | Structural comparison returns `UnknownContainsCapabilities`, even for a capability compared with itself. A definite difference elsewhere can still establish inequality. [Structural tests](../../tests/structural_equality.rs) verify that comparisons never query capability hooks, with additional C++ comparisons and bounded trace replay. Join waits for unresolved inputs; unsupported routes and failures return errors. |
| Settled but broken references are treated as usable Join inputs | **Fixed by this audit.** Revoked local clients, settled error hooks and disconnected imports could pass the identity fast path or contribute to an inequality decision. `ClientHook::get_failure()` separates known failure from settlement. [Shared Join resolution](../../crates/capntproto-rpc/src/join.rs) checks every resolution step and rechecks all inputs after asynchronous settlement. Both public drivers check a returned capability, and independent-share processing checks its endpoint before forwarding or contributing. |
| A joined reference overtakes either input history | Authenticated shares follow their respective RPC routes; direct acquisition requires all shares at one endpoint. The four-vat regression in [equality.rs](../../tests/multiparty_join/equality.rs) sends calls on both paths before Join, delays one reference's resolution, and queues a call on the joined promise. It covers equal targets, unequal targets, failed resolution and a disconnected relay-to-host route for both drivers. Only equal, live inputs deliver the joined call, after both predecessors. |
| Third-party transfer bypasses queued calls or grants a forged recipient access | Provide/Accept and disembargo retain the old route's ordering and authenticated recipient binding. [Runtime tests](../../tests/runtime.rs), [deferred handoff tests](../../tests/deferred_handoff.rs), [handoff authentication](../../tests/handoff_auth.rs) and [pipeline migration](../../tests/native_pipeline_migration.rs) exercise early Accept, queued calls, cancellation, forged bindings and relay loss. A failure query itself does not accept a deferred handoff. |
| A stale barrier, result or reconnect revives old authority | Engine embargo IDs are never reused on a connection; graph handles include generation/ownership. Native Join validates operation, part count/index, object group, proofs, peer, cancellation and single acceptance. Failed routes acquire a new generation; old capabilities stay broken and calls are not replayed. See [Native Join](Multiparty-Join.md), [connection recovery](Connection-Recovery.md) and the bounded proofs below. |

The reproduced Join bug did not make revoked calls succeed: invocation still
rejected them. It did incorrectly report a successful equality result or an
inequality result involving a known broken input. Regressions also cover an
input revoked while another input is still resolving, so checking only at the
start is insufficient.

Already transferred authority remains independent: a capability published by
`shorten_path` or completed direct acquisition can survive loss/revocation of its
former owner. A regression joins such a shortened capability after the old
owner is revoked. The new failure checks do not recall that released authority.

## Use the returned authority

```rust
match system.get_joiner().join(vec![first, second]).await? {
    Some(common) => {
        // Send the valuable operation using common.
    }
    None => {
        // No common endpoint was established. Do not choose one original input.
    }
}
```

Do not turn an error into `None` in a security decision. Do not infer capability
identity from debug strings, schema types, message-local capability indices,
host identity alone, or raw hook pointers. Private hook identity is meaningful
only while its owning reference lives, within the runtime's contract. Native
Join retains owners while comparing endpoints; it does not authenticate a
remote claim about a Rust pointer.

## Differences from E

Capntproto does not expose E's synchronous, monotonic `==` or `EMap` keys for
live capabilities. Generated clients and `capability::Client` have no general
`Eq`/`Hash` implementation. A third-party capability may be a promise while
acceptance and ordering complete; this is supported protocol behavior, not an
E-style assertion that a far reference must deserialize already settled. The
Lost Resolution example therefore cannot silently corrupt a built-in
capability-keyed map: there is no such map contract. Applications needing map
keys should use explicit immutable data keys and resolve/validate authority
separately. A data identifier alone is not permission to invoke an object.

Structural comparison uses bounded readers and reports malformed/cyclic or
over-budget data as errors. It does not implement E's equality over arbitrary
cyclic immutable object graphs. Equality of mutable builders is also a statement
about their current contents, not a permanent identity assertion.

The explicit reconnecting client is an opaque service wrapper whose factory can
replace its target. Its identity belongs to that wrapper, not the current remote
object. It is not an E fail-stop far reference; callers requiring that behavior
must retain ordinary session capabilities. A failed method is never replayed by
the reconnect wrapper.

RPC ordering concerns admission/delivery along the represented route, not
completion of asynchronous application work or database commits. Dropping a
pending promise may cancel work. A lost response does not prove non-execution.
Applications still need deadlines for unavailable peers and unresolved inputs;
Join does not promise eventual completion without delivery or polling progress.

## Verification scope

The audit adds eight integration tests in
[equality.rs](../../tests/multiparty_join/equality.rs), each exercising both
hosted implementations. Existing suites cover bilateral Join, membrane policy,
structural equality, introductions, revocation and failure cleanup. The separate
`verification/portable` suite exercises the allocation-free engine/wire paths,
including owned network Join and embargo handling.

The revoked-input regression failed before the fix. An additional fault
injection removed only the post-await batch recheck: the concurrent-revocation
regression failed, then passed again after restoring the check.

Audit validation passed **748 distinct selected nextest tests**: 514 RPC runtime,
engine and wire unit tests, 148 root-package tests, and 86 allocation-free
portable checks. The four Kani harnesses below passed with Kani 0.68.0 / CBMC
6.11.0. Clippy, formatting and wiki validation passed. Bare-metal
`thumbv7em-none-eabi` checks passed for the no-allocation fixture and its
`no_std + alloc` library profile. This was targeted audit validation, not a fresh
full-workspace or release qualification run.

Relevant Kani harnesses over production engine code are:

- `clients::proofs::frozen_route_and_reflected_barrier_preserve_order`
- `embargoes::proofs::stale_echo_cannot_open_reused_storage`
- `provision::proofs::accept_embargo_replay_and_revoke_preserve_authority`
- `provision::proofs::scoped_embargo_claims_survive_capacity_and_revoke`

These bounded checks do not prove arbitrary distributed schedules, cryptographic
security or correctness of custom hooks/networks. Custom `ClientHook` code is
trusted runtime code: it must report its known terminal failures and obey
identity, ordering and explicit delegation contracts. Allocation-free network
Join deliberately leaves authentication to the caller's network implementation;
an admitted wire Join alone does not authorize release of a capability.

See [Capability Join](Capability-Join.md) and
[Multiparty Join](Multiparty-Join.md) for the protocol contracts and their limits.

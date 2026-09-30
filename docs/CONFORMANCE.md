# Composed-model conformance boundaries

Updated 2026-09-25. `CapnpNetwork.tla` is a partial composed RPC abstraction,
with focused regression models and bounded workloads. Passing those workloads
does not establish full protocol conformance or equivalence with C++ or Rust.
The [C++ parity checklist](CPP_PARITY.md) separately tracks implementation
gaps; a model limitation does not by itself mean a runtime feature is absent.

The reference schema is pinned to Cap'n Proto commit
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`; copies and hashes are in
[upstream](../vendor/provenance). The schema inventory checks compare pins and discriminants,
not transition semantics. [PROTOCOL.md](PROTOCOL.md) maps schema behavior to the
models and records their abstraction boundaries.

## Repaired handoff behavior

The original audit found that acceptance required explicit application commands
and that introduced capabilities in payloads were not routed automatically.
The current model includes runtime-driven acceptance, Call/Return/Resolve
introductions and routing of the application's original capability handle.
[HANDOFF.md](HANDOFF.md) documents that repair and its remaining composition
limits. The old missing-handoff verdict applies only to the pre-repair snapshot.

## Repaired wire-handler boundaries

The two original diagnostics now pass in ordinary Cargo runs after changes to
the composed handlers. Request data is covered as well:

| Diagnostic | Current receiver behavior |
|---|---|
| `WireReleaseEffect` | Export accounting uses wire ID/count and the receiver's current export table. Unknown exports and over-release close the connection; zero is a no-op only for a live export. |
| `WireRequestMethod` | The receiver copies the wire method into its operation record. |
| `WireRequestData` | The receiver copies wire method/data even when the paired caller values differ. |

`ReleaseAnnotation` explicitly relates generated Release annotations to the
channel, export ID/incarnation, reference count and locally released import
credits. `RequestPairing` relates request identity to the pending operation,
without requiring caller method/data to equal the incoming values.
`GeneratedWireRelation` checks this relation at queue heads throughout the
bounded explicit-release workload. `WireReleaseIsolation` varies annotations,
including an unrelated export's credit, while checking equal export-table and
connection effects. Sources: [audit model](../verification/audit/CapnpConformance.tla) and
[Cargo checks](../tests/conformance.rs).

[ComposedWireBoundary](../verification/ComposedWireBoundary.tla) invokes the actual
composed handlers and checks an independent scalar ID/count and dispatch
contract: **279 states / 1,092 graph-edge prefixes**, with two exports, one/two
references each and up to two received actions. [Rust replay](../tests/composed_wire_boundary.rs)
sends real RPC messages and observes capability destruction, Abort, dispatched
method/target/data and returned values. Three model fault controls restore the
old ticket/method dependency or ignore wire data; each violates its intended
invariant. Native tests additionally cover `u32::MAX` release counts. The bounded
model's method names map to distinct test-server methods; application method
semantics, native export-counter internals and C++ execution are not compared.

The additional cyclic configuration explores **630 states / 12,040 edges** with
up to three references per export. [Generated histories](../tests/composed_wire_history/mod.rs)
sample 128 paths with up to 64 prefix operations, then check final releases or
rejection. Each runs twice in each of two independent processes, using real RPC
messages and reusing completed question IDs. Shrinking preserves legal graph
transitions; replay rejects altered/truncated expected histories. This supplements
edge-prefix coverage with sampled histories. Calls settle between actions and
task yields vary explicitly; this does not enumerate concurrent RPC schedules,
capability-slot reuse, reconnects or cryptographic behavior.

These are handler projection checks, not a whole-ledger erasure or runtime
refinement proof. Erasing or forging annotations can still break the ownership
relation for subsequent actions; implicit releases still use local credit sets.
Requests still rely on paired operation identity and other shared metadata.
The ledger and operation records therefore remain auxiliary execution state,
not independent ghost observers.

## Transport, equality and composition

The composed Join model uses shared join-group state and installs authenticated
grants through an ideal VatNetwork. It does not derive authentication from
separate per-vat knowledge or prove what each proxy can learn. Secret-share and
transport authentication properties need their own implementation/model evidence.
The Rust [Join](JOIN.md) and [multiparty Join](MULTIPARTY_JOIN.md)
contracts describe that component coverage without claiming full composition.

Wire/local-state refinement, arbitrary proxy chains, disconnect/reconnect
combinations, cancellation/ID reuse, memory ownership, hostile wire traffic and
the full RPC/schema/bulk/realtime/transport/storage composition remain broader
obligations. See [ROADMAP.md](ROADMAP.md#verification-and-interoperability-gaps).

## Verification and historical evidence

Current commands are maintained in [TESTING.md](TESTING.md):

```sh
cargo test --test tooling schema_pin_and_wire_inventory -- --exact
cargo test --test conformance
cargo test --test composed_wire_boundary -- --nocapture
```

All 12 conformance checks run normally; none is ignored. The replay test runs a
fresh TLC graph, then checks each model fault control for the intended failure.

Historical [pre-repair audit](../reports/conformance-before-handoff/audit.json),
[post-repair audit](../reports/conformance/audit.json),
[RPC aggregate](../reports/checks.json),
[feature checks](../reports/features-verified/checks.json) and
[realtime checks](../reports/realtime/checks.json) retain exact inputs and outcomes.
They describe their recorded snapshots, not fresh qualification of today's tree.
The corresponding model snapshots and raw logs remain available; obsolete
Markdown run summaries and abandoned Apalache instructions have been removed.

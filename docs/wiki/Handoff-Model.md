# Automatic handoff in the composed model

This guide describes operators in `CapnpNetwork.tla`, not Rust API names.
For application usage, see [RPC applications](RPC-Applications.md#three-party-capability-transfer).
The model moves three-party acceptance into runtime transitions. Application plans no longer have to send Accept or Disembargo to use an introduced capability. The model remains a bounded abstraction; this change does not establish full protocol conformance.

## Capability behavior

- `EncodeOne` can introduce a remote capability in Call parameters or Return results. Resolve introductions use the same Provide/vine ownership mechanism.
- `RegisterOffers` processes descriptors in Call, Return, and Resolve. Discarded Return/Resolve payloads are released instead of starting handoffs.
- `StartHandoff` allocates an ordinary Accept question and sends an embargo barrier on the provider connection, after previously queued calls. New calls can pipeline on Accept while the host waits for that barrier.
- `ClientRef` routes the application's existing capability handle through the acceptance and then to the accepted capability. The handoff workload makes its old-path and new-path calls on the same original promise reference.
- Shortcuts include the import generation. The old export forwarding path continues using `Follow`, which deliberately ignores client shortcuts so previously announced promise edges stay pinned.
- The original promise handle keeps its redirect after the provider connection closes. Disconnect finishes provisions whose vines were implicitly released; it does not invalidate an already accepted capability on another connection.
- `FinishHandoff` closes the runtime-owned Accept question without implicitly releasing the capability now held by redirected handles. Vines remain held until acceptance settles. Live handles, pending calls, and capability parameters retain the direct import; unused-reference cleanup releases it later.

The parameter-capability check exposed a premature-release race during development: after Accept completed, cleanup could drop its capability while a pending local call still targeted its question. Import retention now follows that pending target and accounts for redirected handles. This is checked alongside reference conservation and vine lifetime.

## Verification

The dedicated configurations are:

| Configuration | Scenario |
| --- | --- |
| `NetworkHandoff` | Promise resolves to a third-vat capability; old/new calls share the original handle; new results contain a capability that is invoked |
| `NetworkReturnHandoff` | Third-party capability appears directly in a Return; recipient calls it and uses a capability it returns |
| `NetworkParameterHandoff` | Third-party capability appears in Call parameters; the callee invokes it |
| `NetworkCanceledReturnHandoff` | Finish races with returning and accepting an introduced capability |
| `NetworkProviderLossHandoff` | The original promise capability is called successfully after the provider connection is closed; abandoned provisions are cleaned up |
| `NetworkWitnessRuntimeHandoff` | Reachability of a completed direct call made through the original handle |

These positive configurations check the composed safety invariants, application/result progress, automatic acceptance completion, and eventual release of vines, provisions, and Accept table entries. The independent `ProviderCapabilitySurvives` requirement additionally requires a successful result from the post-disconnect call. `NetworkBugEmbargo` and `NetworkBugBarrierOrder` must still find their specified counterexamples. The existing composed regressions also exercise capability descriptors, ID reuse, cancellation, redirected returns, Join, and disconnect behavior.

Run the dedicated checks:

```sh
CAPNTPROTO_TLC_CASES=NetworkCanceledReturnHandoff,NetworkProviderLossHandoff,NetworkWitnessRuntimeHandoff,NetworkBugEmbargo,NetworkBugBarrierOrder CAPNTPROTO_CHECK_TIMEOUT=2400 cargo test --test protocol_models protocol_reference -- --ignored --exact
cargo test --test conformance
```

The independent Cargo audit checks initiation and successful completion of
automatic handoff. All 12 conformance checks now run normally, including the repaired wire-handler
diagnostics. Auxiliary-state refinement remains incomplete despite those repairs.
[Model Conformance](Model-Conformance.md) separates the remaining model limitations from
the repaired handoff behavior; [docs/wiki/Testing.md](Testing.md) owns current
verification commands. Historical runs remain in the machine-readable reports.

## Remaining limits

Credit tickets and paired operation records remain auxiliary execution state. Wire/local-state refinement, transport authentication, forwarded multi-recipient introductions, and the full combination of shortening, disconnect, and reconnect are not proved by these checks. In particular, calling an original handle after provider loss is covered; subsequently serializing that same shortened handle into another payload is not. The encoder does not yet universally normalize such handles through `ClientRef`. Non-introduction loopback shortening remains represented by the existing focused/scenario models. The historical audit and its exact model version are preserved under `research/baseline/pre-handoff/` and `reports/conformance-before-handoff/`.

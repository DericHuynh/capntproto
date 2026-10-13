# Interoperability

Interoperability means exchanging messages with an independent implementation,
not running both endpoints against our fork. The standard TCP profile is tested
against source-pinned C++ and the published capnproto-rust crates, in both client
and server roles, for both the legacy and portable hosted drivers.

This is a versioned behavioral qualification, not a claim that every possible
schema, schedule, transport, or upstream release has been exhaustively verified.
Project-specific multiparty authority payloads and Native transport framing are
not the standard two-party stream profile.

## Reproduce the matrix

```sh
git submodule update --init -- vendor/capnproto
cargo nextest run --locked -p capntproto --test upstream_interop
```

The test builds these independent peers:

| Peer | Source and generator |
| --- | --- |
| C++ | Cap'n Proto `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, including its compiler, generator, and static RPC libraries |
| Upstream Rust | Registry `capnp = 0.27.2`, `capnp-rpc = 0.27.0`, `capnpc = 0.27.0`; isolated [manifest and lockfile](../../verification/upstream-rpc/Cargo.toml), no workspace dependencies or patches |
| Legacy and portable | This checkout's runtime and generated bindings, produced by our Rust schema compiler |

Each pair communicates over localhost TCP in separate OS processes. All twelve
directed cross-runtime pairs run, plus two explicit optimized legacy/C++ pairs.
Each capntproto row must complete all eleven scenario groups; there are no
exceptions for either driver. The two upstream-only controls are reported
separately, including their known failures described below.

The [orchestrator](../../tests/upstream_interop.rs) writes
`target/verification/upstream-interop/{README.md,matrix.json}` and per-peer logs.
The regular Cargo coverage job runs this test and uploads the
`upstream-rpc-interoperability` artifact. JSON records both the required and
actually completed scenarios, and never marks an upstream limitation as passed.
Build failures, missing completion markers, unexpected failures and timeouts in
any capntproto row fail CI. Processes are terminated and reaped on failure.

## Covered behavior

| Scenario | Assertions |
| --- | --- |
| Bootstrap and payloads | Call before bootstrap resolves; exact scalar, Unicode text including embedded NUL, and binary payload round trips at 0, 1, 7, 8, 1 KiB, 64 KiB, 256 KiB, 1 MiB and 4 MiB |
| Pipelining and reflection | Call a returned callback before its parent resolves; preserve dispatch order on that same reference across resolution; keep the returned capability usable |
| Tail calls in both directions | Callback forwarding, optimized transfer to/from C++ when explicitly enabled, ordinary reply forwarding to upstream Rust |
| Promise resolution | Queue calls on an exported unresolved capability, then resolve it or reject it; repeated calls to the rejected capability stay broken |
| Exceptions | Failed, Overloaded, Disconnected and Unimplemented preserve kind and diagnostic text |
| Unknown dispatch | Unknown interface and method return Unimplemented without breaking the connection |
| Cancellation | Finish cancels a running cancellable method; a server-side drop counter confirms cleanup |
| Release | Dropping the final imported child capability destroys the server object |
| Streaming | 32 ordered streaming calls update a server-side counter, observed through a subsequent ordinary call |
| Recovery and shutdown | Calls still work after errors; both processes terminate after disconnect |

The scenario source is shared between the two Rust fixture executables, while
their generated bindings and runtime crates are independent. C++ implements the
same application contract independently. The existing installed-C++ test also
checks descriptor transfer, exception traces, unsupported Join rejection, and
allocation-free bounded RPC. The new matrix also runs that allocation-counted
caller-buffered client against published upstream Rust: bootstrap and result
pipelines, reflected capabilities, errors, cancellation, Finish/Release and
complete buffer reclamation must perform zero heap allocations on our side.
See [Testing](Testing.md) for the additional schema,
serialization, malformed-input and cancellation-schedule suites.

## Upstream Rust tail-transfer limitation

Published capnp-rpc 0.27.0 has an unimplemented `ResultsHook::tail_call()` API and
incomplete progress handling for redirected answers. Its fixture uses ordinary
application forwarding instead of calling the unimplemented API. Independently,
an optimized C++ callback tail transfer stalls against that upstream server, and
upstream Rust's client can deliver the reflected pipeline calls out of order
against C++. The same cases pass with capntproto's compatible forwarding mode.
These upstream-only control failures remain visible, with narrowly matched
diagnostics; an unrelated control failure is not accepted.

`twoparty::VatNetwork` therefore defaults to ordinary reply forwarding for tail
calls. Applications keep their normal tail-call API, pipelining and cancellation
behavior. Ordinary echo calls are unaffected; tail calls lose the optimization
that avoids a return trip through the forwarding vat.

For a peer known to support optimized transfers, configure the network before
passing it to `RpcSystem`:

```rust,ignore
let mut network = capnp_rpc::twoparty::VatNetwork::new(
    reader, writer, side, Default::default(),
);
network.set_tail_call_optimization(true);
```

This is an explicit peer assumption, not protocol negotiation. Receiving tail
transfers remains supported in either mode. Custom `Connection` implementations
retain standard tail transfer by default and can override
`supports_tail_call_transfer()` when connecting to a limited peer. Native
authenticated networks retain their existing policy. The portable hosted driver
already uses ordinary same-connection forwarding.

## Boundaries

- The matrix qualifies the pinned versions and these scenarios on standard TCP.
  It does not certify every version or exhaust all possible failure schedules.
- The standard protocol does not prescribe our TLS, QUIC, Native authentication,
  split-plane framing or multiparty introduction profile. Peers need matching
  adapters and application policy for those features.
- General Join, third-party answer adoption and active pipeline migration have
  upstream implementation limits; see [C++ and Rust parity](Cpp-Parity.md).
- E-order concerns delivery on the same reference across resolution, not ordering
  across independently obtained aliases or completion of asynchronous method
  bodies. See [capability equality](Capability-Equality.md) and the upstream
  [RPC specification](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.capnp).

These boundaries prevent a defensible blanket claim of “100% interoperability”.
The CI gate instead requires every supported capntproto matrix case to pass and
preserves evidence of the independent upstream limitations.

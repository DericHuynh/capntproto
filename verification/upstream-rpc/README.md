# Independent upstream Rust peer

This executable links only the published `capnp 0.27.2`, `capnp-rpc 0.27.0` and
`capnpc 0.27.0` crates, resolved by its own committed lockfile. It deliberately
does not depend on or patch any capntproto crate. Our schema compiler and code
generator do not build this peer's bindings.

Run the two-process matrix from the repository root:

```sh
cargo nextest run --locked -p capntproto --test upstream_interop
```

The orchestrator builds the pinned C++ compiler and supplies it through `CAPNP`.
Standalone builds use the installed compiler. Behavioral scenarios live in
`tests/interop/rpc_peer.rs`; the small adapter here handles upstream API
differences without modifying upstream implementation code. In particular,
upstream's unimplemented `ResultsHook::tail_call()` is replaced at the application
layer by an ordinary call and result copy. The independent C++ controls still
expose the upstream runtime's optimized-transfer limitations in the report.
The `bounded-server` mode implements the existing C++ fixture's application
contract so the same allocation-counted portable client can run against both
independent peers.

See [interoperability scope](../../docs/wiki/Interoperability.md).

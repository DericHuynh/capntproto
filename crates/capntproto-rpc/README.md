# Cap’n Proto capability RPC

The maintained legacy RPC implementation: generated clients and servers,
capability pipelines, cancellation, flow control, reference counting, joins and
third-party protocol hooks. Hosted network applications use Tokio and rustls
through the main `capntproto` crate’s `rpc::tcp::tls` module.

`RpcSystem` is the protocol future. `twoparty::TwoPartyClient` owns a two-party
system; `ConnectionOptions` selects reader limits and optional diagnostic clocks.
There is no driver selector or bounded/portable RPC backend. The facade’s
`run` and `run_until` methods poll the driver alongside application work without
an extra spawned task. `TwoPartyServer` accepts authenticated networks and shares
a bootstrap across connections. The examples show a Tokio `LocalSet` and explicit
TLS configuration.

The implementation retains pooled envelopes, cached segment tables, wake-driven
ready queues and batched transport publication. It allocates; fixed-buffer message
construction and framing utilities are separate operations with narrower
allocation guarantees. No API compatibility shim for retired drivers is included.

See the [greeting service](../../examples/configurations/README.md) and
[key/value schema](../../examples/kv/key_value.capnp).

# Examples

Start with a message, then one RPC, then concurrent capability pipelines.
All three use the runtime re-exports from `capntproto`; applications do not need
separate `capnp` or `capnp-rpc` runtime dependencies.

| Example | Run from this checkout | What it demonstrates |
| --- | --- | --- |
| [message_basic.rs](message_basic.rs) | `cargo run --no-default-features --example message_basic` | Build, serialize and read a message. |
| [rpc_simple.rs](rpc_simple.rs) | `cargo run --no-default-features --features services --example rpc_simple` | A generated service and one call with scoped connection owners. |
| [rpc_deep.rs](rpc_deep.rs) | `cargo run --no-default-features --features services --example rpc_deep` | 32 concurrent calls, each following a 32-level capability pipeline without awaiting intermediate results. |

The RPC examples share a small [Node service](support/rpc_service.rs) and
[schema](rpc_examples.capnp). They use an in-memory duplex stream on a Tokio
`LocalSet`. Keep each `rpc::Connection` alive while using its capabilities;
`shutdown()` completes bounded cleanup, and dropping the owner cancels its task.
Use `rpc::tcp`, `rpc::tls` or `rpc::quic` explicitly for network transports.
`rpc::raw` is for integrations that deliberately manage detached task handles.

For a separate consumer with its own schema and only one runtime dependency,
see [downstream](downstream). Its generator selects
`::capntproto::capnp` with `CompilerCommand::capnp_root()`.

The larger [native_store.rs](native_store.rs), [component_store.rs](component_store.rs)
and [async_storage.rs](async_storage.rs) examples cover authenticated native RPC,
durable data and asynchronous storage. The `*_interop` examples exercise
independent C++ peers; they have the additional prerequisites documented in the wiki.

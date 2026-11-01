# Shared greeting service

The [configuration example](../configurations/README.md) uses this service and its
[schema](schema/greeting.capnp). The server returns a session capability; the
client pipelines an echo call through it and checks both replies. Schemas compile
in Rust without an external Cap’n Proto compiler.

`greeting` constructs the legacy connection; `greeting_with_driver` accepts a
configured driver and disconnects after the verified application finishes.
`greeting_scoped` drops its connection on scope completion. Retain and poll the
connection for the entire period during which its capabilities are in use.

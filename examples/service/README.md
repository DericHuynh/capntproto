# Shared greeting service

The configuration and WASI examples share this service and the schema at
`../webtransport/schema/greeting.capnp`. The server returns a session capability;
the client pipelines an `echo` call through it and checks both replies. Schema
compilation uses Rust and needs no external Cap'n Proto compiler.

`greeting_with_driver` drives a native connection through graceful disconnection.
`greeting_scoped` drops the connection after verified replies, which also releases
the browser-owned WebTransport resource. Keep the driver alive for the full scope
in which its capabilities are used.

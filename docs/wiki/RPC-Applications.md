# Building applications on RPC

Use `rpc::Connection` to own one bilateral connection, and `native_rpc::Vat`
to own an authenticated multiparty RPC system. Both run on a Tokio `LocalSet`.
Their owners start the driver, expose typed capabilities and cancel the driver
when dropped. Call `shutdown()` to wait for bounded protocol shutdown.

The transport and capability layers are the foundation for future higher-level
runtimes. This workspace no longer includes an actor runtime, activation
scheduler or placement service.

## One connection

Wrap a configured transport driver with `Connection::spawn`:

```rust
use capntproto::rpc::{self, Connection};

let driver = rpc::tcp::connect(address, None, Default::default()).await?;
let connection = Connection::spawn(driver);
let service: my_schema::service::Client = connection.bootstrap();
// Send ordinary generated requests, callbacks and pipelined calls on service.
connection.shutdown(std::time::Duration::from_secs(3)).await?;
```

`rpc::tcp` is plaintext. For encryption, establish and authenticate a stream
with `rpc::tls` or `rpc::quic`, then pass its `client(...)` driver to the same
`Connection::spawn`. This preserves socket-aware flow control, reader limits
and transport configuration. `Connection::new` also accepts generic Tokio byte
IO with default settings. See the [TLS/mTLS and QUIC guide](TCP-TLS-and-QUIC.md).

`bootstrap()` returns the remote bootstrap on either side. `outgoing_queue()`
provides queue diagnostics; `on_disconnect()` returns a shared completion
observer. Dropping an observer does not cancel the connection. The older
`rpc::client` and `rpc::serve` functions still return bare task handles whose
drop detaches the task; retain and explicitly cancel those handles if using them.

## An authenticated multiparty vat

A vat is an RPC endpoint identified by a public key. It has no activation or
message scheduling policy. Configure it once, then obtain typed bootstraps
whenever needed, including from request handlers through a cloned bootstrapper:

```rust
use capntproto::native_rpc::Vat;

let vat = Vat::builder(identity.public_key())
    .connector(directory.clone())
    .bootstrap(service.clone())
    .start()?;

// Sessions accepted by a listener are attached only after authentication.
vat.attach(session)?;
let remote: my_schema::service::Client = vat.bootstrap(peer_key);
let bootstraps = vat.bootstrapper();
// Move bootstraps into other local tasks or services without retaining the vat.
```

`bootstrap(service)` accepts a generated client and exposes it to all
authenticated peers. No bootstrap and no outbound connector are installed by
default. For different authority per peer, use a factory:

```rust
let vat = Vat::builder(identity.public_key())
    .bootstrap_with(move |peer| {
        if *peer == authorized_peer {
            Ok(service.client.clone())
        } else {
            Err(capnp::Error::failed("bootstrap access denied".into()))
        }
    })
    .start()?;
```

The factory receives the authenticated public key. Self-bootstrap uses the
local key. Rejected requests do not close the route, and a later request
re-evaluates policy. Previously issued capabilities retain their authority;
use the existing revocation/realm mechanisms to revoke them. Existing
`capnp_rpc::BootstrapFactory` implementations work through `bootstrap_factory`.

`connector` accepts `DirectoryConnector`, `DiscoveryConnector`,
`ProvisioningConnector`, `RetryingConnector`, or an application implementation.
`connect_with(move |peer| async move { ... })` replaces a custom connector
struct when a closure is sufficient. It must return an authenticated session;
the network checks that the session belongs to the local and requested peer
identities. See [deployment](Discovery-and-Mobility.md) and
[connection recovery](Connection-Recovery.md) for route policy and setup deadlines.

`options` configures setup deadlines, arbitration and opt-in route recovery.
`flow_limit` sets incoming call backpressure in words, with the same caveats as
`RpcSystem::set_flow_limit`: it is not a hard memory limit and can deadlock
applications whose outstanding calls depend on messages from a paused peer.
`handle()` exposes the existing route diagnostics, mobility, scheduling,
datagrams and individual route controls without duplicating their APIs.

## Outgoing admission and resource observation

Configure outgoing Call admission before spawning a bilateral driver:

```rust
let mut driver = rpc::tcp::connect(address, None, Default::default()).await?;
driver.set_outgoing_call_limit(128);
let connection = Connection::spawn(driver);
let resources = connection.diagnostics();
let output = connection.outgoing_queue();
```

The same driver setting applies to TLS/mTLS and quiche QUIC v1. For a native
vat, use `Vat::builder(...).outgoing_call_limit(128)`; introduced connections
inherit the limit. The lower-level `RpcSystem::set_outgoing_call_limit` also
updates existing connections. Defaults remain unlimited; zero rejects new
requests and lowering the limit never cancels already accepted work.

Each connection reserves a slot when it creates a wire request. Unsent builders,
streaming requests, tail calls and pipeline-only calls share that budget.
Exhaustion produces a local `Overloaded` error before the rejected Call is sent.
There is no admission wait queue or automatic retry. Release unused builders;
an already rejected request stays rejected even if capacity later becomes free.

Sent calls retain their slot until both their protocol question and local write
completion release it. A response reader or unresolved pipeline can retain the
question after the response future is dropped. Canceling a regular call also
retains its slot while awaiting the peer's Return; canceling a pipeline-only call
still retains credit until its queued write completes. A local timeout after
sending does not prove the method did not execute. Bootstrap, Return, Finish,
Release and shutdown continue without acquiring Call credit. Nested outgoing
application calls can still be rejected at capacity, so size budgets for the
dependency chains the application permits.

`resources.snapshot()` returns active connections sorted by local connection
ID, with exact counts for reservations, questions, answers, imports, exports,
embargoes and retained wire response contexts. It also reports incoming call
words under the existing incoming-flow accounting. Protocol questions include
Bootstrap/Join as well as Calls; a shared response context counts once. Contexts
for locally redirected/adopted responses are not included in `held_responses`.
Observers retain no connection and return an empty list after system cleanup.

`output.snapshot()` keeps its existing pending-only contract.
`output.output_snapshot()` additionally distinguishes `queued` and `active`
counts, bytes and ages. Active bytes include the full messages while writing or
flushing, even after a partial write; ages start at enqueue. Bytes exclude stream
framing. Failure or driver cancellation clears both categories. A custom
transport supplying only legacy diagnostics returns `None` for this richer
snapshot, rather than reporting unknown active work as zero.

This first admission layer bounds **Call count per connection**, not process
memory. Payload allocation and response size are not bounded by this setting.
Purely local unresolved clients, aggregate authenticated-principal budgets,
incoming work, capability growth and bounded write batches remain separate
work. See the [ordered research plan](RPC-Research.md#priority-1-bounded-admission-and-complete-resource-diagnostics).

## Three-party capability transfer

Pass generated capabilities in ordinary request or result fields. If B holds a
capability hosted by A and passes it to C, native RPC performs introduction,
authentication and ordering barriers. C's connector chooses the authorized
route to A. An introduction identifies a host; it does not grant permission to
dial an arbitrary address or obtain that host's bootstrap.

For example, the test schema's `bounce(cap)` method returns and retains a
capability. At B, the entire transfer is:

```rust
let owner: harness::Client = broker.bootstrap(host_a);
let receiver: harness::Client = broker.bootstrap(recipient_c);
let mut request = receiver.bounce_request();
request.get().set_cap(owner);
let pending = request.send();
let pipelined = pending.pipeline.get_cap();
// Requests on pipelined can be sent before awaiting pending.promise.
let returned = pending.promise.await?.get()?.get_cap()?;
```

No application ticket or actor identifier is needed for this automatic handoff.
C can retain the received capability and call A after B disconnects once the
direct route is established. A plain two-party connection still supports
capability passing through proxies; use native vats for automatic direct routes.

To compare capabilities through their protocol identities, use a vat's joiner:

```rust
let joined: Option<harness::Client> = vat.joiner().join(vec![first, second]).await?;
```

`Some` is an equal capability with the joined authority. `None` means distinct
settled objects. Unsupported joins, disconnected routes and malformed results
are errors, not inequality. Dropping the join promise cancels it. See
[multiparty Join](Multiparty-Join.md) for its authenticated share protocol.

## Promise pipelines and completion

Keep a call's pipeline available before awaiting its response. For the test
schema's capability-returning `bounce` method:

```rust
let pending = remote.bounce_request()
    .with_params(|mut p| { p.set_cap(service.clone()); Ok(()) })?
    .send();

let target = pending.pipeline.get_cap();
let mut child = target.echo_request();
child.get().set_value(42);
let child = child.send(); // Sent before the parent response arrives.

let (_parent, response) = futures::try_join!(pending, child)?;
assert_eq!(response.get()?.get_value(), 42);
```

Application helpers should expose the generated `RemotePromise`/`PendingCall`
when callers need this behavior. A helper that awaits the parent before returning
its capability adds a dependency round trip. Pipelines address capabilities;
they cannot inspect an unresolved integer or choose a branch from response data.

Both generated client APIs support `send().await`. Use `into_parts()` when the
completion and pipeline need separate owners. `with_params(...)` consumes the
request on construction failure, releasing its admission reservation before any
Call is sent. A borrowed parameter editor cannot escape its closure. The ordinary
mutable request API remains available for incremental construction.

| API | Meaning |
| --- | --- |
| `send()` | Observe the normal response/error and use the returned pipeline |
| `send_ignoring_result()` | Await normal completion/error but discard response data; no retained pipeline |
| `send_for_pipeline()` | Observe only result capabilities; retain the pipeline or derived clients while needed |
| Generated `-> stream` send | Await flow-control readiness; use an ordinary final method to check completion/errors |

None of these turns a timeout or dropped future into proof of rollback. A retained
pipeline can keep parent work relevant after its response future is dropped.
Streaming readiness is not a durable acknowledgement.

## Structured server replies

Generate new server bindings with the ownership-based reply API:

```rust
capnpc::codegen::CodeGenerationCommand::new()
    .structured_replies(true)
    .output_directory(out_dir)
    .run(compiled_schema_request)?;
```

`capnpc::CompilerCommand` exposes the same option. It changes generated server
`FooResults` aliases to `capnp::capability::Reply<T>`. The wire schema, client
methods, inheritance, generic result brands and cancellation policy are unchanged.
Existing modules keep their legacy signatures until regenerated with the option.

Each reply owns its context exclusively and exposes only operations valid for
its current stage:

| Type | Available operations | Operations rejected by the compiler |
| --- | --- | --- |
| `Reply<T>` | `complete(closure)`, `build()`, `tail_call(Request<P,T>)` | Direct edits, cloning, forwarding another result schema or a streaming request |
| `ReplyBuilder<T>` | Fallible `edit()`, `init()`, `copy_from()`, orphanage access, `publish()`, `finish()` | Tail forwarding, reuse after publishing/finishing |
| `PublishedReply<T>` | Hold across asynchronous work, then `finish()` | Editing, root replacement, republishing, tail forwarding |

For a simple server response:

```rust
async fn echo(self: Rc<Self>, p: base::EchoParams, reply: base::EchoResults)
    -> capnp::Result<()>
{
    reply.complete(|mut out| {
        out.set_value(p.get()?.get_value());
        Ok(())
    })
}
```

Forwarding consumes a **fresh** reply and requires the exact result schema:

```rust
reply.tail_call(target.echo_request()
    .with_params(|mut p| { p.set_value(42); Ok(()) })?).await
```

Declare a named result struct for methods intended to forward one another, such
as `echo (...) -> Value` and `forward (...) -> Value`. Two independently declared
anonymous structs with the same fields remain different Rust result types. This
prevents accidental structural reinterpretation. The
[test schema](../../schemas/rpc-api.capnp) demonstrates named scalar and capability
results. Typed forwarding preserves optimized tail transfers and authenticated
three-party answer adoption. Legacy `Results<T>` also has a typed `tail_call()`
helper, but its mutable API still checks initialization order at runtime.

To publish capabilities before the method completes, freeze the built payload:

```rust
let mut draft = reply.build();
draft.edit()?.set_cap(ready_capability);
let published = draft.publish()?;
finish_application_work().await?;
published.finish()
```

`publish()` consumes the editor, snapshots its capability pipeline and returns
an immutable context. Outstanding editor/orphanage borrows prevent that move.
The final successful result therefore cannot replace capabilities advertised by
this snapshot. Fill **all** result data before publishing: freezing includes scalar
and large data fields. Publication may copy the current payload. The legacy
independent-pipeline API below remains available when an application needs a
different construction policy and accepts its identity checks at runtime.

These are compile-time guarantees for the ordinary generated API. Low-level hook
implementations and manual dispatch are trusted runtime boundaries. Rust still
allows explicit drops, forgetting owners and ignoring `Result` values; `must_use`
helps catch accidental omission but is a lint, not linear ownership. Network
failures, invalid received data, authorization and application failures remain
runtime errors. Early publication cannot undo completed child effects if the
parent later fails, and a failed wire parent may break later calls through its
unresolved pipeline path.

## Legacy independent pipeline publication

Servers can publish result capabilities before finishing with `set_pipeline()`.
For large responses, `PipelineBuilder<T>` and `set_pipeline_from()` construct the
capability pipeline independently, avoiding a snapshot copy of the large response
solely for publication. Publish only once, after authorization and usable behavior
are established. The eventual result must contain the same capability identities
or promises resolving to them. A later parent error does not undo child effects.

The [RPC research](RPC-Research.md) includes an executable protocol probe,
round-trip/wire-count comparisons and priorities for bounded admission,
observability, tail-call ergonomics and transport optimization.

## Ownership and shutdown

Keep the `Connection` or `Vat` owner alive while using remote capabilities.
Dropping it initiates cancellation even if no task has been polled yet.
Canceling a consuming `shutdown()` future also drops the owner. Bootstrap and
join handles are weak: they return disconnected errors after the system closes.
They can safely be retained by services without owning the driver's lifetime.

Stop issuing application work before shutdown. Connection shutdown closes RPC
within the requested positive timeout. Native vat shutdown stops route admission
and concurrently drains all current routes with a timeout in `(0, 60s]`:

```rust
let report = vat.shutdown(std::time::Duration::from_secs(3)).await?;
for (peer, result) in report {
    // Handle each peer separately: result is a receipt or a route failure.
}
```

Native receipts acknowledge byte-stream delivery, not method completion or
durability. Pending or failed routes appear as failures in the report.
`on_disconnect()` observes RPC driver completion; dropping the native driver
cancels its session workers. Neither facade retries application calls or
provides exactly-once execution, crash failover or durable ownership.

Executor-neutral applications can continue to drive `Network` and `RpcSystem`
directly. `RpcSystem::get_bootstrapper()` supplies the same weak typed bootstrap
handle without a Tokio dependency in `capnp-rpc`.

## Executable examples and checks

The [storage example](../../examples/native_store.rs) uses a native vat and owns the
authenticated connection from setup through shutdown:

```sh
cargo run --locked -p capntproto --example native_store -- /tmp/example.rp
cargo test --locked -p capntproto --test rpc_ownership --test native_vat --test secure_rpc
```

[Native vat tests](../../tests/native_vat.rs) exercise three-party pipelining,
on-demand direct dialing, capability joins, introducer removal, per-peer drain,
authenticated bootstrap policy and cancellation. The handoff scenario runs over
TCP/TLS and quiche QUIC v1. [Connection tests](../../tests/rpc_ownership.rs)
cover drop, active/unpolled shutdown cancellation, blocked writers and retained
clients. The secure transport suite also exercises the owned connection facade
with TLS, mTLS and QUIC v1.

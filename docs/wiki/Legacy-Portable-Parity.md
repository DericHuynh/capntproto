# Legacy and portable implementation audit

This audit compares the maintained legacy RPC implementation at `6c29732965e7a745c55529b679812d6eb640c479` with its portable implementation. Every one of its **53 tracked files** was checked: 46 files under `src`, the build script, and six manifest, schema, documentation or license files. The Rust files contain 24,250 lines at that baseline. The table below records every file, its counterpart and the result. A shared implementation is compared by its actual call path rather than copied into another module.

The inventory is `git ls-tree -r --name-only 6c29732 -- crates/capntproto-rpc`, excluding `src/portable.rs`, `src/portable/`, `src/scoped.rs` and `src/scoped/`, which are the destination implementation. This is an audit of the legacy RPC implementation, not a claim to have reviewed unrelated storage, compiler or application code, or archived upstream sources. Both drivers use the maintained `capntproto-futures` framing/output crate and the same root TCP/TLS/QUIC/Unix transport adapters; those are shared dependencies, not missing driver ports.

The audit found two missing integrations and one shared implementation problem, addressed in this batch:

- Hosted portable streaming now uses `Connection::new_stream()` and legacy's fixed, variable or adaptive byte-window implementation. Accepted body words are charged exactly once. Credit readiness is separate from Return ownership; dropping credit cannot cancel an accepted RPC. Local, queued, membrane, reconnect, projected and imported-promise routes retain their forwarding behavior. This includes calls issued before Bootstrap resolves and networks with optimized tail transfer disabled. Hosted `call_with_hints` now delegates through the original ResultsHook, as legacy does, while retaining the same outgoing-call credit in the outer response when forwarding copies the inner payload. The prior recursive request adapter could overflow the stack.
- The two-party facade now accepts `RpcDriver::Portable(options)` for owned, borrowed and scoped networks and accepted server connections. It runs the portable Runtime and retains the same diagnostics, policy controls, drain and disconnect behavior. Legacy stays the default.
- Shared reconnect middleware snapshots an `Rc` target and its generation together. Request forwarding avoids a boxed external hook clone under a mutable-state borrow, and an old Disconnect cannot reset a reentrant replacement.

Breaking hosted API changes: custom `FlowController` implementations implement `sent(words, ack)`; the default `send` publishes then calls it. `TwoPartyServer::accept_borrowed` returns `Result`, like configured/scoped acceptance, so invalid portable options can be reported. Custom resource-aware hosted outputs can retain `StreamPublication` when they configure a streaming factory. The fixed no-std/no-alloc API acquires none of these hosted dependencies.

## Every legacy file

Paths in the first column are relative to `crates/capntproto-rpc`. “Adapted” means the legacy responsibility is implemented with portable's caller-owned storage and explicit protocol lifetimes. It does not mean the object layout or generated machine code is identical.

| Legacy file | Result | Portable/shared counterpart and comparison |
| --- | --- | --- |
| [CHANGELOG.md](../../crates/capntproto-rpc/CHANGELOG.md) | Provenance | [README.md](../../crates/capntproto-rpc/README.md). Preserved upstream history; current driver contracts are documented in the maintained README. |
| [Cargo.toml](../../crates/capntproto-rpc/Cargo.toml) | Shared profiles | [lib.rs](../../crates/capntproto-rpc-engine/src/lib.rs), [lib.rs](../../crates/capntproto-rpc-wire/src/lib.rs). std gates hosted middleware/transports; fixed engine/wire remain available without std or alloc. |
| [Cargo.toml.orig](../../crates/capntproto-rpc/Cargo.toml.orig) | Provenance | [Cargo.toml](../../crates/capntproto-rpc/Cargo.toml). Historical upstream manifest; not the workspace dependency selection. |
| [LICENSE](../../crates/capntproto-rpc/LICENSE) | Provenance | [LICENSE](../../crates/capntproto-rpc/LICENSE). MIT provenance retained for the shared and adapted implementation. |
| [README.md](../../crates/capntproto-rpc/README.md) | Updated | [portable.rs](../../crates/capntproto-rpc/src/portable.rs), [scoped.rs](../../crates/capntproto-rpc/src/scoped.rs). Checked documented protocol support, storage limits, driver selection and migration caveats. |
| [build.rs](../../crates/capntproto-rpc/build.rs) | Shared | [lib.rs](../../crates/capntproto-rpc-wire/src/lib.rs), [persistent.capnp](../../crates/capntproto-rpc/schema/persistent.capnp). Both drivers use the same RPC schema bindings; Persistent generation is shared hosted API. |
| [schema/persistent.capnp](../../crates/capntproto-rpc/schema/persistent.capnp) | Shared | [lib.rs](../../crates/capntproto-rpc/src/lib.rs). Same Persistent interface; no separate portable protocol encoding to port. |
| [src/admission.rs](../../crates/capntproto-rpc/src/admission.rs) | Shared | [admission.rs](../../crates/capntproto-rpc/src/scoped/admission.rs), [outgoing.rs](../../crates/capntproto-rpc-engine/src/flow_control/outgoing.rs). Same admission permits and diagnostics; portable credit uses the shared outgoing counter. |
| [src/answer_adoption.rs](../../crates/capntproto-rpc/src/answer_adoption.rs) | Adapted | [handoff.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/multiparty/handoff.rs), [tail.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/multiparty/tail.rs), [network.rs](../../crates/capntproto-rpc-wire/src/owned/connection/network.rs). Authenticated adoption, either arrival order, early Return, independent producer lifetime, cancellation, deadline and used-pipeline fences. |
| [src/attach.rs](../../crates/capntproto-rpc/src/attach.rs) | Shared | [attach.rs](../../crates/capntproto-rpc/src/attach.rs). Same future attachment/lifetime helper. |
| [src/bootstrap.rs](../../crates/capntproto-rpc/src/bootstrap.rs) | Adapted | [portable.rs](../../crates/capntproto-rpc/src/portable.rs), [bootstrap.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/bootstrap.rs). Weak typed bootstrap access, self-bootstrap policy and pre-poll remote admission. |
| [src/broken.rs](../../crates/capntproto-rpc/src/broken.rs) | Shared | [broken.rs](../../crates/capntproto-rpc/src/broken.rs). Same broken request/client/pipeline hooks; default resolution helper contains no legacy protocol dispatch. |
| [src/fd.rs](../../crates/capntproto-rpc/src/fd.rs) | Shared | [resources.rs](../../crates/capntproto-rpc/src/scoped/wire/resources.rs), [vat.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/transport/vat.rs). Same outgoing descriptor policy; portable receiver retains exact-message resources. |
| [src/flow_control.rs](../../crates/capntproto-rpc/src/flow_control.rs) | Ported in this batch | [streaming.rs](../../crates/capntproto-rpc/src/scoped/streaming.rs), [flow_control.rs](../../crates/capntproto-rpc-engine/src/flow_control.rs). Portable now creates the connection stream and uses this exact fixed/variable/adaptive controller, with separate credit and acknowledgement ownership. |
| [src/join.rs](../../crates/capntproto-rpc/src/join.rs) | Adapted | [join.rs](../../crates/capntproto-rpc/src/portable/join.rs), [join.rs](../../crates/capntproto-rpc-wire/src/owned/join.rs), [join.rs](../../crates/capntproto-rpc-wire/src/owned/connection/join.rs). Bilateral Join, batch policy, retained response ownership and optional rejection; bounded IDs do not wrap into live or queued work. |
| [src/lib.rs](../../crates/capntproto-rpc/src/lib.rs) | Shared / adapted | [portable.rs](../../crates/capntproto-rpc/src/portable.rs), [response.rs](../../crates/capntproto-rpc/src/scoped/response.rs), [connection.rs](../../crates/capntproto-rpc-wire/src/owned/connection.rs). Compared public network, client/server, response, flow, factory, cancellation and diagnostics contracts; shared hooks stay shared. |
| [src/local.rs](../../crates/capntproto-rpc/src/local.rs) | Shared | [local.rs](../../crates/capntproto-rpc/src/local.rs), [response.rs](../../crates/capntproto-rpc/src/scoped/response.rs), [results.rs](../../crates/capntproto-rpc/src/scoped/wire/server/results.rs). Same server identity, protected execution, streaming order, shortening, revocation, owned request and result hooks; local streaming forwarding now reaches portable credit. |
| [src/local/allocation_tests.rs](../../crates/capntproto-rpc/src/local/allocation_tests.rs) | Shared | [allocation_tests.rs](../../crates/capntproto-rpc/src/local/allocation_tests.rs). Shared local dispatch allocation regressions; portable integration has separate transport/ownership regressions. |
| [src/local/owned_results_tests.rs](../../crates/capntproto-rpc/src/local/owned_results_tests.rs) | Shared | [owned_results_tests.rs](../../crates/capntproto-rpc/src/local/owned_results_tests.rs). Same retained payload, canceled caller and result-context lifetime regressions. |
| [src/local/protected.rs](../../crates/capntproto-rpc/src/local/protected.rs) | Shared | [protected.rs](../../crates/capntproto-rpc/src/local/protected.rs). Same two-interest protected-execution owner and completion. |
| [src/membrane.rs](../../crates/capntproto-rpc/src/membrane.rs) | Shared | [membrane.rs](../../crates/capntproto-rpc/src/membrane.rs). Same bidirectional policy, capability transformation, Join delegation, revocation and streaming request forwarding. |
| [src/membrane/resolution_tests.rs](../../crates/capntproto-rpc/src/membrane/resolution_tests.rs) | Shared | [resolution_tests.rs](../../crates/capntproto-rpc/src/membrane/resolution_tests.rs). Same in-place settlement and boundary lifetime regressions. |
| [src/multiparty.rs](../../crates/capntproto-rpc/src/multiparty.rs) | Shared | [multiparty.rs](../../crates/capntproto-rpc/src/multiparty.rs). Same authenticated network-facing Join contract. |
| [src/multiparty_join.rs](../../crates/capntproto-rpc/src/multiparty_join.rs) | Shared / adapted | [join.rs](../../crates/capntproto-rpc/src/portable/join.rs), [multiparty.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/multiparty.rs). Common share validation and Join outcome helpers; portable owns bounded authenticated requests and exact completion wakes. |
| [src/pipeline_builder.rs](../../crates/capntproto-rpc/src/pipeline_builder.rs) | Shared | [pipeline_builder.rs](../../crates/capntproto-rpc/src/pipeline_builder.rs). Same explicitly supplied early-pipeline builder API. |
| [src/queued.rs](../../crates/capntproto-rpc/src/queued.rs) | Shared | [queued.rs](../../crates/capntproto-rpc/src/queued.rs), [wire.rs](../../crates/capntproto-rpc/src/scoped/wire.rs). Same promise queue, resolution order and hook adoption; portable private projections preserve their own bounded graph route. |
| [src/reconnect.rs](../../crates/capntproto-rpc/src/reconnect.rs) | Shared | [reconnect.rs](../../crates/capntproto-rpc/src/reconnect.rs). Shared route/generation snapshot fix removes external hook clones under the route borrow and protects reentrant replacement. |
| [src/revocable.rs](../../crates/capntproto-rpc/src/revocable.rs) | Shared | [revocable.rs](../../crates/capntproto-rpc/src/revocable.rs). Same explicit authority revocation and cancellation wrapper. |
| [src/rpc.rs](../../crates/capntproto-rpc/src/rpc.rs) | Adapted | [scoped.rs](../../crates/capntproto-rpc/src/scoped.rs), [call.rs](../../crates/capntproto-rpc/src/scoped/call.rs), [hook.rs](../../crates/capntproto-rpc/src/scoped/hook.rs), [response.rs](../../crates/capntproto-rpc/src/scoped/response.rs), [wire.rs](../../crates/capntproto-rpc/src/scoped/wire.rs), [results.rs](../../crates/capntproto-rpc/src/scoped/wire/server/results.rs), [connection.rs](../../crates/capntproto-rpc-wire/src/owned/connection.rs). Compared every type, hook method and lifecycle path. See responsibility mapping below; allocating table/arena ownership is intentionally bounded in portable. |
| [src/rpc/capabilities.rs](../../crates/capntproto-rpc/src/rpc/capabilities.rs) | Adapted | [exporting.rs](../../crates/capntproto-rpc-wire/src/clients/exporting.rs), [resolution.rs](../../crates/capntproto-rpc-wire/src/inbound/full/resolution.rs), [foreign.rs](../../crates/capntproto-rpc/src/scoped/wire/foreign.rs), [introduction.rs](../../crates/capntproto-rpc/src/portable/introduction.rs). Import alias identity, one promise obligation, export grants/refunds, reflection, introduction forwarding and descriptor metadata. |
| [src/rpc/dispatch.rs](../../crates/capntproto-rpc/src/rpc/dispatch.rs) | Adapted | [connection.rs](../../crates/capntproto-rpc-wire/src/owned/connection.rs), [full.rs](../../crates/capntproto-rpc-wire/src/inbound/full.rs), [returns.rs](../../crates/capntproto-rpc-wire/src/inbound/full/returns.rs), [multiparty.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/multiparty.rs). Every wire union arm compared: Call/Return/Finish/Resolve/Release/embargo, Bootstrap, Join, introductions, third-party answers, Abort and optional/obsolete messages. |
| [src/rpc/references_tests.rs](../../crates/capntproto-rpc/src/rpc/references_tests.rs) | Compared tests | [tests.rs](../../crates/capntproto-rpc-wire/src/owned/tests.rs), [tests.rs](../../crates/capntproto-rpc-wire/src/inbound/full/tests.rs), [tests.rs](../../crates/capntproto-rpc/src/scoped/wire/tests.rs), [call_hints.rs](../../tests/call_hints.rs), [import_alias.rs](../../tests/import_alias.rs), [disconnect_cleanup.rs](../../tests/disconnect_cleanup.rs). Compared ownership, overflow, reentry, synchronous completion, pipeline cancellation, streaming aliases, lookup and empty-storage reuse cases; internal legacy fixture is not copied into the portable driver. |
| [src/rpc_tables.rs](../../crates/capntproto-rpc/src/rpc_tables.rs) | Shared core / bounded storage | [local_table.rs](../../crates/capntproto-rpc-engine/src/local_table.rs), [peer_table.rs](../../crates/capntproto-rpc-engine/src/peer_table.rs), [free_slots.rs](../../crates/capntproto-rpc-engine/src/free_slots.rs). Same lowest-free local-ID algorithm. Legacy sparse HashMap provider is replaced by caller-backed records, generations and bounded growth. |
| [src/rpc_tables/tests.rs](../../crates/capntproto-rpc/src/rpc_tables/tests.rs) | Compared tests | [local_table.rs](../../crates/capntproto-rpc-engine/src/local_table.rs), [peer_table.rs](../../crates/capntproto-rpc-engine/src/peer_table.rs), [free_slots.rs](../../crates/capntproto-rpc-engine/src/free_slots.rs), [tests.rs](../../crates/capntproto-rpc-wire/src/owned/tests.rs). Lowest vacancy, sparse/high IDs, retirement and reuse checked against portable table and generation regressions. |
| [src/sender_queue.rs](../../crates/capntproto-rpc/src/sender_queue.rs) | Shared | [sender_queue.rs](../../crates/capntproto-rpc/src/sender_queue.rs). Shared affine queued sender/cancellation primitive; bounded protocol queues use explicit caller-owned storage. |
| [src/split.rs](../../crates/capntproto-rpc/src/split.rs) | Shared | [split.rs](../../crates/capntproto-rpc/src/split.rs). Shared future split helper; portable ordinary completions use one directly owned completion instead. |
| [src/system.rs](../../crates/capntproto-rpc/src/system.rs) | Shared / adapted | [portable.rs](../../crates/capntproto-rpc/src/portable.rs), [connection.rs](../../crates/capntproto-rpc/src/portable/connection.rs), [registry.rs](../../crates/capntproto-rpc/src/system/registry.rs). Shared network lifecycle helper/registry; portable owns its bounded connection construction, progress and output runtime. |
| [src/system/registry.rs](../../crates/capntproto-rpc/src/system/registry.rs) | Shared | [registry.rs](../../crates/capntproto-rpc/src/system/registry.rs). Exact same initializing/live/draining identity registry, close completion and stale-generation ownership. |
| [src/system/registry/tests.rs](../../crates/capntproto-rpc/src/system/registry/tests.rs) | Shared | [tests.rs](../../crates/capntproto-rpc/src/system/registry/tests.rs). Same identity reuse, reentrant construction and shutdown completion tests for the shared registry. |
| [src/system/tests.rs](../../crates/capntproto-rpc/src/system/tests.rs) | Compared tests | [tests.rs](../../crates/capntproto-rpc/src/portable/tests.rs), [bootstrap_factory.rs](../../tests/bootstrap_factory.rs), [disconnect_cleanup.rs](../../tests/disconnect_cleanup.rs). Matched factory callbacks, bootstrap lifetime, pre-poll drop, weak handles, registration and repeated shutdown. |
| [src/task_set.rs](../../crates/capntproto-rpc/src/task_set.rs) | Shared | [task_set.rs](../../crates/capntproto-rpc/src/task_set.rs). Portable system/network/protected/stream drivers use the same hosted TaskSet; fixed protocol work remains caller-polled. |
| [src/task_set/queue.rs](../../crates/capntproto-rpc/src/task_set/queue.rs) | Shared | [queue.rs](../../crates/capntproto-rpc/src/task_set/queue.rs). Same hosted task admission, cancellation and completion queue. |
| [src/task_set/ready_queue.rs](../../crates/capntproto-rpc/src/task_set/ready_queue.rs) | Shared | [ready_queue.rs](../../crates/capntproto-rpc-engine/src/ready_queue.rs). Same allocation-free atomic FIFO core. |
| [src/task_set/ready_set.rs](../../crates/capntproto-rpc/src/task_set/ready_set.rs) | Shared | [ready_set.rs](../../crates/capntproto-rpc/src/task_set/ready_set.rs). Same ready-only polling used for hosted connections and callback owners; bounded operations additionally reject stale generations. |
| [src/third_party.rs](../../crates/capntproto-rpc/src/third_party.rs) | Shared | [third_party.rs](../../crates/capntproto-rpc/src/third_party.rs). Same authenticated ThirdPartyExchange rendezvous and embargo primitive. |
| [src/transport_frame.rs](../../crates/capntproto-rpc/src/transport_frame.rs) | Shared | [transport_frame.rs](../../crates/capntproto-rpc/src/transport_frame.rs). Portable uses the same OutgoingFrame owner. Raw externally supplied parts retain framing validation; typed writer semantics do not require a payload walk. |
| [src/transport_frame/tests.rs](../../crates/capntproto-rpc/src/transport_frame/tests.rs) | Shared | [tests.rs](../../crates/capntproto-rpc/src/transport_frame/tests.rs). Same retained bytes/parts ownership, malformed framing and resource lifetime checks. |
| [src/twoparty.rs](../../crates/capntproto-rpc/src/twoparty.rs) | Shared | [twoparty.rs](../../crates/capntproto-rpc/src/twoparty.rs), [vat.rs](../../crates/capntproto-rpc/src/scoped/wire/coordinator/transport/vat.rs). Both drivers use the same input buffering, physical output FIFO, segment pool, window policy, flush and close. Portable uses exclusive FrameIo rather than allocating IncomingMessage wrappers. |
| [src/twoparty/facade.rs](../../crates/capntproto-rpc/src/twoparty/facade.rs) | Ported in this batch | [driver.rs](../../crates/capntproto-rpc/src/twoparty/facade/driver.rs), [twoparty_facade.rs](../../tests/twoparty_facade.rs). Explicit RpcDriver selection now covers owned, borrowed, scoped and server acceptance. Default remains legacy; portable keeps explicit storage options. |
| [src/twoparty/facade/borrowed.rs](../../crates/capntproto-rpc/src/twoparty/facade/borrowed.rs) | Shared | [borrowed.rs](../../crates/capntproto-rpc/src/twoparty/facade/borrowed.rs). Exact same bounded copying pump, partial IO, borrow release and physical closure behavior. |
| [src/twoparty/facade/notify.rs](../../crates/capntproto-rpc/src/twoparty/facade/notify.rs) | Shared | [notify.rs](../../crates/capntproto-rpc/src/twoparty/facade/notify.rs). Exact same wake-driven shutdown observers; completed observers are consumed once. |
| [src/twoparty/output_driver_tests.rs](../../crates/capntproto-rpc/src/twoparty/output_driver_tests.rs) | Shared | [output_driver_tests.rs](../../crates/capntproto-rpc/src/twoparty/output_driver_tests.rs). Same physical FIFO/flush/resource tests; new facade matrix exercises both drivers on those shared streams. |
| [src/twoparty/send_buffer.rs](../../crates/capntproto-rpc/src/twoparty/send_buffer.rs) | Shared | [send_buffer.rs](../../crates/capntproto-rpc/src/twoparty/send_buffer.rs). Same weak socket-window query and cached fallback; portable streaming now reaches it through Connection::new_stream. |

## Protocol and ownership mapping inside rpc.rs

| Legacy responsibility | Portable implementation checked |
| --- | --- |
| Question lifecycle, Finish, canceled/late Return, no-Finish and high IDs | Shared engine `question`/`questions`; wire `owned/connection`, invocation and exchange owners. Pipeline high IDs are monotonic; bounded exhaustion falls back to ordinary calls. |
| Direct response and public pipeline interests | `scoped/response.rs`, `scoped/wire.rs`; one completion owns status/results and parent interests. Return settles ordinary projections without a second background response task. |
| Request construction, final headers, descriptor indices and request-time limits | `scoped/call.rs`, `scoped/hook.rs`, `local.rs`, wire prepared Call templates, immediate/queued invocation publication and shared outgoing admission. |
| Parameter/result lifetime, traversal budgets, synchronous dispatch, protected execution | Shared local hooks, `scoped/wire/server.rs`, `server/results.rs`, payload owners and bounded worker. Fixed buffers copy when their storage cannot escape; shareable owners retain their original segments. |
| Export identity, repeated grants/refunds and promise observers | Wire export/reference ledgers, canonical identity index, `scoped/wire/foreign.rs`; staged publication rolls back uncommitted grants. |
| Import aliases, descriptor resources, resolution and reflection | Shared engine promise/reference transitions; wire import ledger and resolution; scoped resources/metadata. Original opaque policy hooks survive reflection. |
| Sender/receiver loopback, used-pipeline migration and Join | Wire loopbacks/barriers, bilateral Join and hosted authenticated Join; independently authenticated adopted paths retain their own fence. |
| Provide/Accept, vines, introductions, third-party results and tail transfer | `portable/introduction.rs`, coordinator multiparty/handoff/tail owners and bounded wire network requests. |
| Task readiness, fair receive budget, idle, output, failure and orderly shutdown | Shared TaskSet/ReadySet/registry and engine ReadyQueue, coordinator ready operations, portable Runtime output phase and shared FrameIo. |
| Streaming controller lifetime and acknowledgement driver | Shared flow controller, `scoped/streaming.rs`, exact output publication resources and local forwarding markers. |

The new `twoparty/facade/driver.rs` and `scoped/streaming.rs` integration modules were also reviewed in full.

Streaming, native-tail and third-party publication metadata share one mutually exclusive call state. This retains the ordinary request storage size and removes 16 allocated bytes per measured workflow versus the first streaming integration.

The completed source comparison found no further missing legacy protocol or shared middleware responsibility. This is a source audit with regression evidence, not a proof that every possible execution is equivalent.

## Differences retained deliberately

Portable still validates bounded capacity, graph identities and generations and separates affine protocol ownership from application futures. Legacy instead retains allocating connection/table owners in its hooks. Copying those owners verbatim would remove the fixed no-std/no-alloc contract. Sparse-ID fallback, explicit capacity failure, caller-backed completion registries, publication reservations and retained retired generations therefore remain. Hosted adapters still allocate for generated owned clients and service futures.

Both drivers use the same physical framing/output implementation. Portable's adapter and bounded graph/invocation work still have measurable instruction costs; completing this audit does not establish performance parity. Performance results below measure that remaining difference separately from functional coverage.

## Verification and measurements for the integration batch (`3cdec87`)

The following checks passed on the final source:

- 458 RPC unit tests (`cargo test --offline --locked -p capntproto-rpc --lib`).
- 238 protocol integration tests across 26 targets, including the shared legacy/portable trace replay and the new facade/streaming regressions.
- 33 TLS/QUIC tests (`cargo test --offline --locked -p capntproto --features quic --test secure_rpc`).
- 86 fixed-storage and 89 growing-storage tests in the isolated `verification/portable` workspace (`cargo test --offline --locked --manifest-path verification/portable/Cargo.toml --lib`, repeated with `--features growing`).
- Three RPC doctests passed; four existing examples are ignored (`cargo test --offline --locked -p capntproto-rpc --doc`).
- i686 Miri: both streaming publication tests and the reconnect route/generation regression passed on `nightly-2026-08-29`.
- Clippy with `-D warnings` passed for RPC all-targets and the changed `call_hints`, `twoparty_facade` and `scoped_vat` integration targets. Workspace formatting passed.

The ARM `thumbv7em-none-eabi` release fixture build was killed by the workspace memory limit (`SIGKILL`) during optimized compilation. It is **not qualified for this batch**; the final no-allocator link/symbol check could not run. The fixed-storage allocation regressions above passed. No bounded engine or wire source was changed in this batch.

### Dedicated read instrumentation

Local five-run measurements use real two-party framing over a 64 KiB Tokio duplex stream, with generated clients and an actual service on both ends. Each workflow opens a capability with a 1 KiB payload, then reads that capability. Dependent reads await Open; pipelined reads project the capability before Return. Both endpoints use the indicated driver. Eight warm-up workflows precede measurement. Differential totals `(16 workflows - 4 workflows) / 12` remove setup and teardown costs. The same active executable path is used for all variants and run order rotates.

These are stock Rust 1.97.0 release builds with fat LTO, one codegen unit and debug information. Callgrind records executed instructions (`Ir`) and simulated conditional/indirect branch misses (`Bcm`/`Bim`), not hardware cycles. Each cell below is **minimum / mean / spread**, where spread is maximum minus minimum over five runs. Minima are selected independently for each metric. Branch predictor simulation is sensitive to code layout; these numbers do not establish hardware or latency parity.

| Case | Driver | Ir: min / mean / spread | Bcm: min / mean / spread | Bim: min / mean / spread |
| --- | --- | ---: | ---: | ---: |
| dependent-read | Portable before (`6c29732`) | 106,495.667 / 106,496.367 / 1.000 | 1,475.250 / 1,475.833 / 2.167 | 265.000 / 265.000 / 0.000 |
| dependent-read | Portable after | 107,465.250 / 107,465.933 / 1.167 | 1,504.000 / 1,504.700 / 1.750 | 289.000 / 289.000 / 0.000 |
| dependent-read | Legacy after | 67,175.333 / 67,176.517 / 1.750 | 566.917 / 567.150 / 0.500 | 183.000 / 183.000 / 0.000 |
| pipelined-read | Portable before (`6c29732`) | 107,929.833 / 107,930.317 / 0.917 | 1,836.167 / 1,837.383 / 1.583 | 319.000 / 319.000 / 0.000 |
| pipelined-read | Portable after | 108,533.333 / 108,552.233 / 58.000 | 1,794.167 / 1,797.517 / 9.833 | 335.000 / 335.000 / 0.000 |
| pipelined-read | Legacy after | 66,186.083 / 66,229.433 / 64.417 | 557.833 / 563.867 / 8.833 | 168.000 / 168.000 / 0.000 |

Native runs independently repeat allocation and accepted nonzero byte-stream I/O counters five times. Every reported native count is identical across all five runs: **minimum = mean; spread = 0**. Counts cover both endpoints. Reads/writes are adapter calls, not operating-system syscalls or network packets.

| Case | Driver | Allocations: min = mean | Allocated bytes: min = mean | Reads: min = mean | Writes: min = mean |
| --- | --- | ---: | ---: | ---: | ---: |
| dependent-read | Portable before (`6c29732`) | 38 | 5,044 | 4 | 6 |
| dependent-read | Portable after | 38 | 5,052 | 4 | 6 |
| dependent-read | Legacy after | 41 | 4,212 | 4 | 12 |
| pipelined-read | Portable before (`6c29732`) | 46 | 5,984 | 2 | 6 |
| pipelined-read | Portable after | 46 | 6,000 | 2 | 6 |
| pipelined-read | Legacy after | 55 | 4,768 | 2 | 12 |

For both drivers, total bytes read and total bytes written across both endpoints are each 2,552 per dependent-read workflow and 2,584 per pipelined-read workflow. The port retains portable's allocation counts and I/O counts. Endpoint streaming metadata adds 8 allocated bytes per dependent workflow and 16 per pipelined workflow; mutually exclusive call metadata avoids enlarging ordinary requests.

For dependent-read, this correctness/integration batch changes minimum Ir by +0.910%, Bcm by +1.949% and Bim by +9.057%. Portable still executes 59.977% more instructions than legacy in this probe.

For pipelined-read, this correctness/integration batch changes minimum Ir by +0.559%, Bcm by -2.287% and Bim by +5.016%. Portable still executes 63.982% more instructions than legacy in this probe.

Completing the file comparison closes missing integrations; it does not close this instruction/branch gap. Portable uses fewer allocation blocks and half as many write calls here, but allocates more bytes. The bounded graph, protocol ownership, coordinator and hosted future adapters remain candidates for architectural optimization. This probe does not measure TLS/QUIC throughput or C++ performance.

## Source organization refactor

The hosted bridge now separates the registry (`scoped.rs`), request/admission owners (`scoped/call.rs`) and client routing (`scoped/hook.rs`). The public `scoped::Call` path remains available. `CallPermit` names the queue/destination guard separately from outgoing response `Credit`; its fields are private to the request module. Field/drop order, dispatch policy, data storage and the moved function bodies are preserved. Async and capability traits are imported at the relevant module boundaries. Streaming documentation explains ownership during reentrant publication.

Qualification: 458 RPC unit tests, 238 protocol integrations, 33 TLS/QUIC tests, three doctests, three targeted i686 Miri tests and Clippy with warnings denied passed. The no-default-features RPC library check passed for `thumbv7em-none-eabi`; the optimized full fixture mentioned above was not rerun.

The same read probe and five-run method described above compare `3cdec87` with this refactor. Each cell is **minimum / mean / spread**. Branch misses are simulated; no latency or hardware-cycle claim is made.

| Case | Version | Ir: min / mean / spread | Bcm: min / mean / spread | Bim: min / mean / spread |
| --- | --- | ---: | ---: | ---: |
| dependent-read | Before (`3cdec87`) | 107,464.417 / 107,465.283 / 1.500 | 1,504.250 / 1,505.067 / 1.250 | 289.000 / 289.000 / 0.000 |
| dependent-read | Refactored | 107,465.917 / 107,466.417 / 0.750 | 1,459.917 / 1,460.200 / 0.583 | 288.000 / 288.000 / 0.000 |
| pipelined-read | Before (`3cdec87`) | 108,545.333 / 108,545.517 / 0.667 | 1,796.500 / 1,796.550 / 0.167 | 335.000 / 335.000 / 0.000 |
| pipelined-read | Refactored | 108,544.667 / 108,545.350 / 0.917 | 1,732.583 / 1,732.733 / 0.417 | 338.000 / 338.000 / 0.000 |

Native allocation and I/O counts are identical before and after, with zero spread across five runs: dependent-read uses 38 allocations / 5,052 bytes / 4 reads / 6 writes; pipelined-read uses 46 allocations / 6,000 bytes / 2 reads / 6 writes. Minimum instruction changes are +0.0014% and -0.0006%, respectively. Simulated Bcm falls 2.95% / 3.56%; Bim changes -0.35% / +0.90%.

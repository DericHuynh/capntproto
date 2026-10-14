# 0.1.0 developer preview (unpublished)

- Replace the four Kani CI shards with source-bound Verus contracts and
  verus-tla induction, including full-width arithmetic, ownership rejection
  contracts, temporal fairness assumptions and deliberate-fault controls.
  Keep optional bounded Kani investigations available outside the CI gate.
- Reduce portable plaintext TCP instruction cost below 20 million for both
  peers in the small-call workload with ordinary full LTO. Reuse checked
  immutable message views and completion storage, fuse ready dispatch and reply
  handoff, preserve idle scheduler state, and isolate multi-segment lookup from
  common bounds checks. The instruction ceiling now gates both peers.
- Preserve queued-hook reentrancy and partial-write ownership, check bounds
  without overflowing word counts, and add 32-bit Miri coverage for cached
  reader provenance and promise ownership. Immediate hosted promise errors are
  boxed; the allocation-free engine retains caller-owned storage.

- Guarantee eight-byte alignment in the caller-owned retained-parameters test
  fixture, fixing its 32-bit Miri failure while preserving single/multi-segment
  and shared/copied parameter lifetime checks.

- Avoid four temporary receipt allocations per capability-free portable Call,
  retaining the target transaction in stack storage and enforcing the same
  capacity limit. Remove admission-only capacity policies from retained receipt
  owners. Repeated ordinary full-LTO profiles reduce server instructions 3.60%
  and client instructions 0.13%; portable/legacy parity remains outstanding.
  Report both peers' instruction counts in the development benchmark JSON.

- Verify outgoing question lifecycle contracts and unbounded safety/progress with
  Verus and verus-tla, replacing the eight-operation Kani search while retaining
  constructor and ledger checks. Add five deliberate-fault controls.
- Retain admitted portable RPC parameter readers without decoding their envelope
  again; preserve capability-receipt handling and caller-owned buffer fallback.

- Add pinned Verus and verus-tla verification of the actual no-alloc handoff
  implementation. Replace its Kani transition proof with full-width contracts
  and unbounded temporal induction; preserve the Default wrapper and distributed
  TLC checks. CI requires complete positive proofs and four logical negative
  controls, publishes separate diagnostics/timing graphs, and verifies the
  temporal library itself before use. Normal Rust builds have no new dependency.
- Include the RPC wire crate in LLVM coverage; retain
  measured counters and detailed errors when the coverage gate fails. Permit
  at most 0.5 percentage points of scheduling variation while still rejecting
  lost instrumentation and zero-hit regressions.
- Derive the nested nextest reporting fixture's binary inventory from the real
  resource-group configuration, including upstream interoperability tests.
- Allow reconnect callbacks to replace or reset their target without borrowing
  panics or overwriting the newer target. Reject recursive connection attempts
  with `Overloaded` and recover after an unwinding callback. Requests which
  trigger reconnection retain their original error and are never replayed.

- Qualify legacy and portable RPC against pinned C++ and published capnproto-rust
  in both roles, including payloads through 4 MiB, promise/answer pipelines,
  callback tail calls, cancellation, release, streaming and exception recovery.
  Standard two-party networks now forward tail replies normally by default to
  avoid capnp-rpc 0.27.0's incomplete redirected-answer progress handling;
  `set_tail_call_optimization(true)` retains optimized transfers to known peers.
  CI publishes per-peer evidence and separately reports two reproduced failures
  between the upstream implementations rather than counting them as passes.

- Follow upstream Rust RPC's execution-before-results ordering in the portable
  hosted worker, avoiding completion waiter allocation and redundant wakes for
  ready methods. Preserve early-pipeline notifications, pending execution and
  delayed result ownership. Fix transferred pipelines polling a completed
  shared future while the application retains its result builder; keep the
  observer registered until that builder returns. Add scheduling and retained
  transfer regressions and document the pinned upstream comparison. Three
  ordinary full-LTO profiles reduce portable server instructions 7.21%, from
  31.84M to 29.54M; client instructions remain essentially unchanged. Latency
  shows no consistent gain, and portable/legacy parity remains outstanding.

- Reuse checked incoming Call parameter views in legacy TCP, including answer
  adoption, and reduce ordinary reply-channel copies by boxing the local-only
  redirected builder. Avoid copying portable maintenance capacities until they
  change, and remove division from the hosted server's rotating worker cursor.
  Three alternating ordinary full-LTO profiles reduce legacy server instructions
  another 3.16% and portable server instructions 0.84%; client cost is essentially
  unchanged. Small-call portable latency remains about 1.15–1.16× legacy, so the
  1.00× target is still open. Add regressions for cached payload ownership and
  cleanup created by a pending pipeline callback. Separate CI report generation
  from the final mandatory-stage gate so upstream failures have a clear label.

- Reuse Return envelopes in the legacy and portable drivers, preserving segmented
  payloads without rebuilding their pointer graphs. Keep reader limits,
  capability validation, cancellation and retry ownership intact; batch empty
  maintenance slots within the caller's existing polling budget. Local ordinary
  full-LTO profiles reduce legacy server instructions 4.3% and portable server
  instructions 10.6%. Portable small-call latency is about 1.13× C++ and 1.15×
  legacy; the 1.00× target remains unmet. Add header-mutation, segment-limit and
  publication regressions. Repair Kani import fixtures for pending-release
  accounting, report proof-build failures with their diagnostic log paths, and
  budget CI for the complete proof and fault-control campaign.

- Extend the two-droplet workflow suite with validated 256 KiB, 1 MiB and 4 MiB
  echoes across legacy/portable RPC, C++, gRPC and WebSockets. Bound the larger
  workflow messages at 8 MiB, including framing, without changing the small-call
  microbenchmark configuration. Require all 58 supported cells in remote reports
  and exercise 48 cross-language cases before provisioning benchmark hosts.

- Reduce the portable TCP client's local instruction diagnostic from 66.40M to
  28.37M in the ordinary full-LTO build, and to 20.98M with opt-in PGO, native CPU
  targeting and static C++ runtime linkage. Add a reproducible Rust measurement
  command with independent legacy training and an instruction ceiling. Use
  immutable request templates, checked reader selections, retained
  framing, direct response ownership and bounded wake-driven polling. Preserve
  fixed-buffer/no-allocator operation. Fix a missed network-control wake exposed
  by eager maintenance and add ownership, cancellation, framing and slot-reuse
  regressions. Three runs of the tuned build meet the 21M instruction target.
  Choose protocols outside the shared benchmark's timed loop, retaining all
  calls, timeouts and validation for both Rust drivers. All 2,174 workspace tests
  pass. Two separate local plaintext TCP comparisons put the tuned candidate's
  median latency at 1.01–1.07× the existing pinned C++ reference across 0–64 KiB;
  this does not qualify ordinary-build latency or complete driver parity.

- Reduce portable RPC overhead with reusable application message segments,
  direct ready-call scheduling, smaller hosted metadata, immutable framing
  certificates and fewer empty maintenance/output scans. Keep caller-supplied
  fixed storage allocation-free and preserve validation, cancellation and
  backpressure. Add ownership, malformed-input, scheduling and capacity
  regressions. Two local release comparisons pass the portable 1.5× C++ small-call
  gate; default-driver migration still requires the remaining protocol parity.
  Make benchmark reports identify the measured driver, retain failed-gate graphs,
  and replace stale README charts with an explicit unavailable result.

- Integrate automatic third-party tail transfer into the opt-in portable RpcSystem.
  Authenticate introductions, retain queued destination metadata, publish adopted
  answers through the ordinary ledger, and share one callee execution across both
  answer lifetimes. Preserve late pipeline projections after ResultsSentElsewhere
  and keep previously used routes ordered through the relay. Retain direct result
  capabilities independently of the introducer. Fix eager bootstrap forwarding
  and account for the metadata copy staging word in exact-sized output buffers.
  Add mixed-driver, cancellation, local-adoption, relay-failure and authenticated
  native transport regressions, plus allocation-free output retry coverage.
  Default-driver migration and performance qualification remain outstanding.

- Integrate live outgoing Call limits and exact diagnostics into the portable
  RpcSystem. Share an allocation-free admission counter with the legacy driver;
  retain credit through unsent requests, responses/pipelines, late Return/Finish
  and physical transport writes. Reject excess Calls locally without breaking
  connection progress. Observe actual table record lifetimes without retaining
  protocol owners or changing no-alloc storage. Add cancellation, transport,
  occupancy and TCP regressions, plus a Kani admission proof and mutation control.
  The default driver switch and performance qualification remain outstanding.

- Integrate live incoming flow limits, opt-in exception trace encoding and transport
  idle notifications into the portable RpcSystem. Share an allocation-free credit
  counter with the engine; release charges on Return publication, including when
  pipelines retain results. Keep shutdown responsive while input is paused.
  Size and cache traces before publication retries, and preserve terminal reasons
  in retained ClientHooks. Add TCP, buffered-output, overflow and lifecycle
  regressions plus a Kani hysteresis proof and intentional fault control.

- Add opt-in `portable::RpcSystem` with owned portable-driver tasks, automatic
  VatNetwork dialing/acceptance, bounded connection reuse, peer-specific bootstrap
  factories and protected call execution. Keep the generated client/server API;
  admit bootstrap calls before first poll and expose weak bootstrap handles.
  Require explicit storage limits and preserve caller-driven scheduling/deadlines.
  Wait for close receipts and protocol storage teardown before network completion
  terminates tasks; allow abort during graceful close and preserve its reason.
  Propagate fatal accept-loop failures after retiring connections, so a failed
  listener cannot leave clients parked in a still-pending system.
  Add fault/lifecycle regressions, mixed-driver TCP calls and promise pipelines,
  and real Unix bootstrap/Call/Return/Resolve descriptor transfers. The default
  RpcSystem switch still awaits authenticated multi-party and policy parity;
  no new benchmark qualification is claimed.

- Adapt existing VatNetwork connections to the portable coordinator with
  `register_vat`. Capture trusted transport identity, preserve original segment
  bytes and capability indices, and queue immutable frames through the existing
  TCP/native/Unix writers. Carry descriptor ownership, bound input, observe
  write failures independently of reads, and separate accepted output, write
  completion and close. Keep pending receives alive through drain so transports
  that close both halves on canceled input cannot discard accepted output.
  Add transport fault regressions, TCP interoperability with the default driver,
  and real Unix resource coverage. Fix two-party peer identity and shutdown
  completion before physical close; preserve deliberate disconnect completion
  in the hosted façade. Deliver adopted local answers only once completed, with
  retained capability/frame ownership. Invalidate stale partial idle scans after
  progress so newly queued forwarded calls cannot be stranded. Default RpcSystem task creation, multi-party
  controls, idle/limit integration and benchmark qualification remain open.

- Register portable coordinators through the shared hosted connection registry.
  The new coordinator network exposes weak bootstrap lookup and independent
  disconnectors, with completion held through transport drain/close and protocol
  cleanup. Registered owners expose only managed run methods. Preserve caller
  abort priority, wake parked reads for shutdown, and retire canceled owners
  before first poll or during drain. Check shutdown before reserving input so a
  full pool can still drain accepted output. Duplicate/closed registration returns
  the coordinator for retry. Cover pipelined 64 KiB RPC, ID reuse, IO failures and
  real Unix descriptor transfers. Automatic VatNetwork adaptation, authenticated
  multi-party controls, idle/limit policies and the default driver switch remain.

- Move default RpcSystem connection registration and shutdown ownership into a
  shared hosted registry. Reserve transport identities before driver callbacks,
  keep detached connections counted until IO shutdown, and protect replacements
  from stale completion. Independent disconnectors now all wake and observe the
  first shutdown error. Canceling an unpolled or running connection task reports
  cancellation instead of leaving shutdown pending forever. Bootstrap, accept,
  Join and third-party adoption use this registry. The default protocol driver
  still uses the hosted implementation; switching it to the portable coordinator
  and completing multi-party, idle and limit integration remain outstanding.

- Add allocation-free buffered output progress and explicit flush/close IO
  interfaces. The migrated coordinator now flushes accepted partial frames,
  fixing buffered-transport stalls. Managed stream/resource connections drain
  accepted output before closing; a caller-supplied abort/deadline remains active
  through blocked writes, flushes and close. Stop owned command admission before
  draining and retain protocol owners until IO ends. Cover buffered duplex RPC,
  shutdown errors/cancellation, once-only resources and allocator-free output
  with regression tests and a bounded Kani proof. Default RpcSystem registration,
  authenticated multi-party lifecycle, idle notifications and limits remain open.

- Integrate ancillary resources into the hosted portable-driver coordinator.
  Byte-only and resource-bearing transports share admission, fair polling and
  terminal cleanup; Unix socket adapters attach descriptors to the exact frame.
  Preserve incoming resources across retries and outgoing resources across
  partial writes. Fix missing import metadata before synchronous dispatch,
  result delivery and pipeline notification. Add real-socket bootstrap,
  Call/Return/Resolve, descriptor-free fallback, frame isolation, duplicate-index
  and terminal-cleanup regressions. Default RpcSystem migration still needs
  authenticated network lifecycle, connection registration/shutdown and limits.

- Add typed allocation-free Bootstrap completion and pipeline-only bootstrap
  requests to the portable driver. The hosted coordinator now supports static
  and peer-specific bootstrap factories through its two-party control handler,
  preserving exception details and retrying output without rerunning policy.
  A bounded weak handle queues bootstrap requests after the connection starts;
  abandoned commands do not publish and shutdown rejects pending commands.
  Cover pending capabilities, pipelined calls, factory failure followed by
  recovery, Finish-before-Return, stale answer generations and pool exhaustion.
  Authenticated multi-party lifecycle, resource/limit integration and switching
  the default RpcSystem remain outstanding.

- Add a hosted per-connection coordinator over the portable RPC driver, caller
  IO, storage, poll budget and stop future. Combine calls, service execution,
  foreign promises and protocol maintenance with ordered input retries and FIFO
  backpressure. Distinguish scan yields from application wakes to prevent idle
  spinning and lost resolution notifications. Add allocation-free receive
  cancellation that recovers the reserved buffer before terminal cleanup.
  Cover fragmented 64 KiB duplex calls, one-slot execution/output, interrupted
  IO, malformed input, output failure and cancellation. Authenticated network
  lifecycle/resource integration remain prerequisites for
  the default RpcSystem switch.

- Add bounded, caller-polled foreign ClientHook promise resolution to the hosted
  portable-driver bridge. Preserve original proxy ordering and policy, chained
  resolution, wakeups, abandoned-observer reclamation and retryable capacity
  failure. Callable local promises now remain senderPromise exports until their
  producer settles. Failed application endpoints retain owned diagnostics for
  local invocation and publish wire exceptions. Per-connection IO and the
  authenticated lifecycle still precede the default RpcSystem switch.

- Add owned outgoing-call coordination over the portable invocation ledger,
  including bounded admission, local service execution, fair polling, remote
  output, cancellation and tracked delivery. Preserve local exception details
  and early pipelines after parent failure and through late projections. Wake
  the caller when protected execution independently publishes an early pipeline.
  Size outgoing descriptors and promised-answer paths before reserving frames.
  Resource-bearing output uses the same coordinator. Fix a null-pointer copy in
  bounded capability remapping for untouched message roots and absent fields,
  exposed by an empty local result. Per-connection IO,
  authenticated lifecycle and foreign promise integration still precede the
  default RpcSystem switch.

- Add a caller-polled incoming service worker over the portable driver's accepted
  operations. Bound execution independently of protocol storage, retain work
  through publication failure, poll pending methods fairly, and release requests
  explicitly at shutdown. Preserve protected execution after Finish and support
  resource-bearing replies through the same worker. Size large payloads and
  structured exceptions before reserving output. Outgoing-call coordination and
  authenticated lifecycle integration still precede the default RpcSystem switch.

- Add portable service completion and retryable Return publication from retained
  answer results, including optional resources, redirected replies and Finish
  racing publication. Add hosted results collection, early pipeline resolution,
  generic tail forwarding and executor-backed cancellation lifetimes. Preserve
  exception reasons/details on the wire. The default RpcSystem connection worker
  and authenticated lifecycle integration remain unfinished.

- Bind outgoing transport resources to the portable FIFO's accepted message.
  Preserve resources across Pending/Interrupted and emit them only with the
  first accepted byte, using the authoritative send cursor. Support bounded
  inline or explicit growing bundles, Call and Resolve generation, tracked
  invocation output, and resource-bearing Return publication. Reuse Unix
  descriptor IO through a caller-polled portable writer. Reserve before protocol
  commit and release tentative/terminal resources exactly once. The default
  RpcSystem switch still requires hosted result and authenticated lifecycle
  integration; this checkpoint does not claim that migration is complete.

- Admit optional capability attachments in the portable Call/Return/Resolve
  path. Deliver caller-owned resources only after transactional admission,
  preserve them on rejection, support late import aliases, and consume each
  attachment index at most once. Ignore unavailable attachments as required by
  the protocol. Project hosted descriptors and diagnostics into owned clients
  through explicit endpoint metadata; keep OS types outside the portable layer.
  The default RpcSystem switch remains integration work.

- Add explicit bounded growth for per-message capability receipts and pipeline
  transforms throughout the portable wire driver and owned hook bridge. Support
  zero inline capacity, keep fixed profiles allocation-free under feature
  unification, and preserve owners on admission or allocation failure. Copy
  unchanged capability indices without allocating identity maps. This removes
  the per-message fixed-array blocker; the default hosted runtime migration
  still requires descriptor and authenticated network lifecycle integration.

- Extend the scoped owned-client bridge to pending promises and portable wire
  invocations. Keep routable imported promises unsettled until resolution and
  reflected barriers complete; preserve frozen advertised routes. Bind owned
  payloads, early and late pipelines, cancellation and replies to the existing
  invocation tracker. Preserve null capability-table entries and full remote
  call exceptions. Release pipeline interest before waking cancellation. The
  pipeline-only regression also exposed a hosted `RefCell` reentry panic;
  queued pipelines now detach driving futures before dropping their callbacks.
  Prepare owned server parameters from admitted wire receipts and local
  invocations while preserving the original result context. Break invalid late
  projections and preserve required completion for redirected pipeline-only calls.
  The default hosted `RpcSystem` migration remains open; this does not replace its
  descriptor attachments or authenticated network orchestration.

- Add a scoped `ClientHook` bridge for owned generated clients. Retain settled
  graph routes inside their scope, bound queued/running calls, preserve early
  pipelines and capability payloads, and reclaim routes after the final owner
  drops. Private downcast identity prevents parameter lookup from bypassing
  opaque wrappers or crossing connections. Fix hosted streaming sends being
  overtaken by later ordinary sends before their first poll. Cover the bridge
  with engine dispatch regressions and 32-bit Miri. Pending graph promises,
  wire-driver admission and default `RpcSystem` migration remain unfinished.

- Add independently bounded adopted-question growth and owned reply slots for
  portable RPC sessions. Shared completion endpoints transfer into the tracked
  delivery interface without allocation, lifetime extension or duplicate owners.
  Typed owned calls and streaming acknowledgments use the same cancellation,
  pipelining and receipt recovery as fixed calls. Replay third-party admission,
  abandoned replies and returned capabilities with growing resources; retain
  allocation-free fixed profiles and 32-bit Miri coverage. Migration of the
  default hosted client hooks remains in progress.

- Add optional stable growing message buffers, send records and FIFO queues to
  the portable RPC engine. Enforce independent count, message-size and retained
  byte budgets; preserve suspended buffer views and partial-frame order across
  growth. Replay fixed and growing driver conversations with the same limits,
  including canceled result drafts and nested generated server calls. Cover
  segment indexing with Kani, real loans with 32-bit Miri, and queue order with
  an injected rotation fault. Default hosted ownership migration remains open.

- Add fallible growing dispatch, capability bindings, request tracking, loopback
  receipts and invocation completion bindings behind the existing transitions.
  Preserve task identity, FIFO order and reserved protocol capacity as storage
  grows from zero. Reserve third-party exchange storage before admission and
  refresh all growing maintenance scans. Replay fixed-capacity protocol limits
  with growing resources; add cancellation, stale-key, sparse-ID and late cleanup
  regressions plus 32-bit Miri coverage. The default hosted client ownership
  bridge remains integration work.

- Share fixed and growing import, export, question and answer ledgers. Add
  independent growth bounds and keep fixed providers allocation-free. Refresh
  maintenance scans after growth so cleanup reaches new slots without starving
  later phases. Move hosted local-ID storage onto the engine's growing provider
  and avoid scanning unissued capacity. Test rollback, late returns, stale keys,
  retained pipelines and one-frame cleanup with native tests, 32-bit Miri and
  a fault control restoring the stale scan. Replay portable conversations with
  growing ledgers at the same limits as fixed storage; fix third-party answer
  publication when its answer table starts empty. Growing message storage
  and the hosted client-hook bridge remain in progress.

- Share capability-graph transitions between fixed storage and optional fallible
  heap growth. Preserve live routes and generations across relocation, reserve
  both imported-promise nodes before consuming their owner, and keep fixed
  storage allocation-free even when `alloc` is enabled. Add bounded graph limits,
  Kani arithmetic checks, a maximum-bypass fault control and 32-bit Miri paths.
  Other connection resources and the hosted default ownership migration remain.

- Add a sole-driver wake registration to portable RPC sessions. Application
  enqueue, input and resource release wake a parked driver; idle polling does
  not self-wake. Release session borrows before wake callbacks or old-waker
  destruction. Preserve rejected operations on recursive polling and prevent
  teardown while the driver handle is live. Cover real call/reply ownership,
  allocation counting, bare-metal linking, Miri and missed/early-wake faults.

- Run the bounded RPC driver against the C++ TCP peer in interoperability tests.
  Cover generated calls, bootstrap/result pipelines, reflected capabilities,
  exceptions, canceled calls, Finish/Release and fixed-buffer recovery under
  fragmented IO through the public portable RPC facade.

- Expose the bounded RPC driver and engine through `capntproto-rpc` with default
  features disabled. Add independent `alloc` reply ownership and default `std`
  hosted APIs; exclude futures and hosted transports from the portable target
  graph. Exercise the public facade in the allocator-free ARM fixture and CI.

- Add caller-backed streaming acknowledgment tracking with bounded polling,
  deferred error reporting and automatic receipt cleanup. An early failure
  cannot report drain completion before later calls settle. Cover allocation
  counting, busy retries, cancellation, 32-bit Miri and allocator-free ARM linking.
- Fix hosted pipeline queue cancellation across drains: old promises retain
  distinct identities and cannot cancel newly queued work. Report identity
  exhaustion, detach canceled inputs before destruction, and release client
  state borrows before resolving queued calls and notifying observers.

- Share the portable reply state machine with all hosted RPC one-shot replies,
  local calls, pipeline queues, task notifications, rendezvous and credit waiters through an optional `no_std` + `alloc` ownership
  adapter. Keep caller-owned slots allocation-free. Detach flow-control waiters
  before readiness, failure and shutdown wakeups so reentrant callbacks cannot
  panic on the controller's mutable borrow. Add Miri checks and a fault control
  which restores the held borrow and must fail the reentrancy test.

- Complete bounded third-party result handoff through owning replies and result
  pipelines. Bind result destinations at question reservation, require the
  original caller's handoff opt-in, and authenticate immutable rendezvous
  metadata before exposing the supplied result. Preserve old-route ordering,
  retry ownership and cancellation. Exercise three vats and both arrival orders.
  Add explicit forwarding vines whose acceptance markers follow older calls,
  retain their exact export generation, and survive output backpressure without
  authorizing ordinary services. Return vine owners during terminal cleanup.
  Add Kani destination/permission proofs and fault controls for authorization,
  ordering and released-export authority. Hosted network integration remains.
  Reject capability handoff after noFinishNeeded while permitting scalar results.

- Publish callee-selected answer IDs atomically with their third-party notices,
  reserve that ID range from ordinary calls, and verify retained pipeline
  ownership across reuse with Kani. Route Provide acceptance markers behind
  earlier dispatch work and require an ordered marker type before opening an
  embargo. Add bounded Join request/result ownership, retryable replies and
  target retention through Finish. Cover cancellation, stale registrations,
  output saturation and premature target release with portable fault tests.

- Add bounded incoming third-party answer adoption with separate caller-owned
  question storage and authentication-gated responses and pipelines. Preserve
  completion metadata across early Return, retain canceled IDs until late
  Return, and release denied capability grants exactly once. Verify cancellation
  and reuse with Kani and a negative control; exercise both arrival orders,
  malformed metadata, capacity pressure and teardown without allocation.

- Route bounded Provide/Accept work through the owning connection driver.
  Retain registered graph authority until Finish or disconnect, reject late
  registration callbacks, and preserve network jobs after rejection. Require a
  ready embargo handle for accepted-capability publication; preserve it across
  backpressure and retain completed pipelines through early Finish. Publish
  network failures through ordinary RPC exceptions. Exercise cleanup before
  teardown, denial, cancellation and sender FIFO ordering with no allocations,
  ARM linking, 32-bit Miri and a missing-registration-release fault control.

- Add allocation-free third-party provision lifecycles, owning Provide/Accept
  requests, and transactional no-Return sender handles. Preserve target receipts
  across Finish, reject provision pipelines and duplicate embargo claims, and
  keep empty embargo IDs distinct from absent embargoes. Share embargo policy
  with the hosted runtime. Exercise separate connections through partial IO,
  capability Return/Release/Finish cleanup, zero allocations, ARM and 32-bit
  Miri; verify four new Kani properties with targeted negative controls.
  Network authentication, automatic multiparty orchestration and hosted
  ownership migration remain separate integration work.

- Add an allocation-free shared RPC session with generated reply schemas,
  typed returned capabilities and typed pipeline paths. Reject recursive
  access without losing the operation or its owners. Route streaming methods
  through the same invocation registry with acknowledgment-only futures.
  Build suspended local service results directly in reserved message storage;
  canceled/failed drafts recover capability references before terminal cleanup.
  Cover local and remote calls, backpressure, nested dispatch, exception mapping,
  interface/borrow compile contracts, ARM linking and 32-bit Miri. A cleanup
  fault control verifies that dropping a retained draft capability is detected.

- Generate context-aware borrowed services for allocation-free nested calls.
  Preserve branded inheritance, streaming metadata and rejection before output
  mutation. Exercise actual nested invocation/completion while the parent
  service awaits, with zero allocations, ARM linking and 32-bit Miri.

- Support shared claims on caller-owned reply-slot arrays. Keep slots unavailable
  until both endpoints (including a delivery observer) drop, preserve cleanup
  across a panicking wake hook, and verify stale-endpoint isolation with Kani
  and a mutation control.

- Attach reply futures before generated calls reach the wire. Use one bounded
  invocation registry for local/remote completion, rejected output, broken
  routes, cancellation and terminal cleanup. Retain local capability results
  until their pipelines settle, preserve retry after transient resolver errors,
  and reject completion with a different generated method. Borrow local call
  parameters across suspension while independent buffers remain usable.

- Bind owning RPC responses to bounded caller-owned reply slots. Wake the
  producer when a reply is consumed or canceled, reclaim abandoned capability
  receipts, preserve senders after capacity rejection, and recover forgotten
  receivers during explicit shutdown. Check zero allocations, bare-metal
  linking, Miri, Kani ownership, and missing-cleanup/wakeup fault controls.
  Validate Kani mutation sites before starting verification. Isolate ignored
  test inventory builds from executable files queued by the outer test runner.

- Add caller-owned reply futures with affine completion, cancellation wakeups
  and recovery of undelivered RPC responses. Refuse slot reuse until canceled
  responses are recovered for explicit receipt cleanup. Exercise real replies
  without allocation and verify ownership with Kani and fault controls.

- Queue generated outgoing calls against retained graph capabilities through
  the same ordered dispatcher as input and loopback controls. Publish question
  IDs and parameter grants only with accepted output, preserve rejected calls,
  support local borrowed dispatch, and prevent newer work bypassing deferred
  input. Count allocations across the owning driver lifecycle fixtures.
  Create result pipelines before wire publication, preserve them across
  cancellation and early Return, and settle local execution through the same
  graph. Delay following Resolve until retained result observers are bound.

- Coordinate allocation-free same-connection tail calls and pipelined redirects.
  Retain tail children through parent Finish, preserve retry after rejected
  output, and distinguish answer generations when peer wire IDs are reused.
  Exercise generated calls under IO faults and exhausted storage, with 32-bit
  Miri and a fault control for generation isolation.

- Add consuming, budgeted terminal RPC cleanup. Recover message buffers after
  partial writes, pending replies and deferred input; transfer retained producers,
  imports and servers outside state transitions. Add bounded Kani owner-transfer
  and terminal-output proofs with fault controls.
  Handle Abort and Unimplemented Resolve controls, preserving active call leases
  and rejecting further admission after the first terminal error.

- Drive reflected-capability loopbacks automatically with caller-owned storage.
  Preserve call order through rejected output, unresolved answer targets and
  early Finish; reject duplicate echoes. Reserve dispatch capacity for loopback
  markers so waiting application calls cannot prevent their own release.
  Exercise two connection domains with IO faults and saturated queues.

- Add an owning allocation-free connection coordinator with once-only input
  admission, retained outgoing requests/replies and fair bounded maintenance.
  Preserve deferred Call/Resolve operations, cancel queued work without service
  execution, and drain wire cleanup through the FIFO. Freeze exported promises
  and reflected result routes; retry reflected output without losing its frame,
  grants or receipts. Exercise separate connection domains under IO faults and
  resource exhaustion. Automatic tail/third-party orchestration and
  migration of the default hosted runtime remain in progress.

- Bind owning answer pipelines to frozen graph routes through early Finish and
  wire-ID reuse. Retain completed replies and exact grants across output
  backpressure. Add a single-use domain for graphs whose endpoints own imports
  while exports and answers own graph clients. Apply owning Resolve tasks once,
  retain replacements across retry, and preserve reflected loopback ordering.
  Add cancellation, capacity, duplicate-resolution, compile-contract and Kani
  checks. Full connection driving and hosted ownership migration remain ongoing.

- Add owning allocation-free RPC requests, questions, replies and retained
  answer frames, plus caller-pinned receive/build tasks. Keep frames and
  capability receipts together across suspension, preserve exact originals for
  implicit releases, and allow independent buffers to progress. Borrow the
  ordered dispatcher inside established connection scopes without reusing task
  identities. Exercise bootstrap through generated async dispatch and cleanup,
  cancellation, rejected admission and terminal IO faults. Pipeline binding,
  automatic connection driving, third-party orchestration and hosted migration
  are still in progress.

- Add a caller-backed capability graph and ordered task dispatcher. Freeze
  advertised remote routes, preserve reflected loopback barriers, reject cycles
  atomically and retain endpoint ownership through backpressure/cancellation.
  Generate borrowed async server adapters with inherited/branded dispatch,
  streaming policy and directly constructed fixed-buffer results, including
  locally retained tail-call results. Verify with allocation checks, compiler
  contracts, fault-injected framed IO, Kani, Miri and bare-metal linking. Complete
  connection-driver integration and default hosted ownership migration remain.
  Bind admitted imports and reflected exports to owning graph clients, preserve
  promise identity across aliases, and release wire references when their frozen
  endpoints retire. Keep adapter capacity failures retryable without partial nodes.

- Bind incoming Finish and explicit Release to atomic export cleanup. Retain
  the exact Return for validation and ignore duplicate Finish without replaying
  releases. Let mutable generated results survive suspension while other buffers
  progress; add consuming fixed-segment publication for direct buffer construction
  without copying or clearing the published bytes. Add allocation, borrowing,
  Kani, failure and portable IO checks.

- Add caller-backed connection scopes and same-connection tail-call adoption.
  Bind redirected Call/Return destinations, claim local results once, and retain
  their producer across Finish and wire-ID reuse. Canceled observers consume
  late adoption without adding a dependency. Verify framed tail exchanges with
  partial/Pending/Interrupted IO and fault-injected publication, plus Kani and
  compiler contracts. General promise dispatch, third-party orchestration and
  hosted ownership migration remain required for full allocation-free RPC.

- Complete bounded reflected-capability admission for Resolve and ordered
  publication for Call, Return and Resolve. Bind retained target/parameter
  references to new questions, preserve references on output rejection, and
  queue capability frames before making cleanup eligible. Reject reflected
  pipelines after noFinishNeeded. Add allocation-free fault tests, caller-IO
  exchanges, compiler contracts and a Kani proof with a deliberate fault.

- Admit reflected receiverHosted/receiverAnswer capabilities with atomic local
  export and pipeline lease batches. The bounded Call admission combines them
  with its own target and answer; Return admission combines them with implicit
  parameter releases, preserving a reflected capability even when its last
  remote grant is released in that same message. Deferred cleanup runs no owner
  destructors within these transitions. Add rejection, generation reuse,
  zero-allocation, compiler-contract, portable and Kani coverage.

- Share imported-promise obligations and loopback embargo ownership with the
  hosted runtime. Keep hooks outside import-table borrows, preserve pending
  promises across hosted aliases, reject duplicate resolution and premature ID
  reuse, and prevent stale echoes from opening a later barrier. Bind bounded
  outgoing Resolve to its exact frame, original promise and replacement grants;
  atomically admit incoming Resolve and replacement ownership. Handle late
  resolutions after abandonment and level-1/2 forwarding vines. Add Kani proofs,
  deliberate fault controls, allocation checks and caller-polled framed fixtures.
  General promise dispatch/one-hop forwarding,
  third-party orchestration and hosted ownership migration
  remain required for full allocation-free RPC.

- Admit bounded incoming calls as one target/answer/import transaction. Bind
  ordinary Return handling to the retained original request, implicit parameter
  releases and result imports. Preserve all ownership on rejected input; discard
  canceled results without importing capabilities. Keep retired export owners
  for explicit draining outside the transaction. Use these bindings in the
  portable framed RPC conversation and verify preflight/count invariants with Kani.

- Add explicit allocation-free capability-index remapping for forwarding entire
  wire trees, preserving unknown fields and rolling back missing mappings or
  buffer exhaustion. Allow scoped RPC ownership across caller-polled async
  suspension. Borrow request views independently of the message pool so nested
  calls and partial sends can progress while generated parameters remain live.
  Reclaim deliberately forgotten views only after terminal scope teardown.

- Add bounded ordinary/streaming method metadata, typed parameter/result dispatch,
  capability indices with message-origin checks and caller-backed typed pipeline
  paths. Scope questions and answers to their connection; retain cleanup through
  rejected output, cancellation and late Return. Keep admitted pipeline targets
  alive across Finish and safe peer-ID reuse. Bind question/answer/cleanup effects
  to exact immutable frames and FIFO admission; share caller-polled sends with
  the hosted futures bridge. Admit incoming descriptor batches atomically, with
  caller-owned receipt storage and no callbacks. Add fault, zero-allocation,
  compiler-contract, Miri/ARM and Kani coverage. Complete promise resolution,
  tail calls, wider descriptor semantics and hosted ownership migration remain
  in progress.

- Extract shared RPC wire schemas into `capntproto-rpc-wire`, with no default
  target features and Rust-only schema compilation on the host. The hosted RPC
  crate reexports the same types. Bind outgoing Call/Return sender-hosted grants
  to their actual sealed capability table with caller-supplied count scratch.
  Missing/extra descriptors, malformed frames and rejected output abort before
  publication; consuming acceptance publishes the batch and exposes that exact
  send. Add fault/zero-allocation tests, compiler contracts, a framed bounded
  import/export roundtrip, Miri/ARM coverage and descriptor-count Kani proofs.
  Third-party publication, dispatch and hosted ownership migration remain open;
  bounded promise publication is now covered by the Resolve work above.

- Add caller-backed immutable message leases and resumable send records to the
  allocation-free engine. Readers and queued sends independently retain bytes;
  checked generations reject late completions after slot reuse. Add a futures
  IO adapter with a caller-chosen polling budget, immediate partial-write progress
  and cancellation/resume on the same stream. Exercise every byte boundary,
  capacity/count limits, zero allocations, borrowing contracts, a real framed
  portable/Miri/ARM roundtrip and two Kani proofs with reclamation/completion
  mutations. Generated bounded RPC dispatch, wider descriptor publication and
  migration of the hosted message path remain open.

- Add caller-backed export ownership with scoped keys and call leases. Final
  peer release invalidates the wire target while active calls retain the owner.
  Stage descriptor grants in slot metadata and commit only after output accepts
  the complete message; abort restores live counts and retains unpublished owners
  for explicit draining. Recover forgotten batches, reject stale keys and retire
  exhausted generations. Add fault/zero-allocation checks, compiler contracts,
  portable/Miri/ARM coverage and two Kani proofs with publication/lease mutations.
  Generated dispatch, wider descriptor publication and hosted-handle migration
  remain open.

- Add caller-backed import ownership to the allocation-free RPC engine. Scoped,
  noncopy local handles cannot cross connections; delayed keys reject released,
  revived and reused slots. Check local/remote reference exhaustion and retire
  exhausted generations without wrapping. Keep cleanup in existing slots until
  output acceptance, then return the detached owner. Add compiler contracts,
  fault/zero-allocation checks, portable/Miri/ARM coverage and two Kani proofs
  with a stale-generation mutation. Full bounded dispatch and hosted handle
  integration remain in progress.

- Move import/export wire-reference accounting into the allocation-free RPC
  engine and use it in the hosted runtime. Check overflow and over-release before
  mutation; prevent terminal export resurrection and keep import cleanup pending
  until an explicit release ticket is committed. Roll back grants from failed
  descriptor batches, reject unsent calls without leaking questions/admission,
  and send exceptions for failed result serialization. Add native fault injection,
  allocation/borrowing checks, two full-width Kani proofs and a release-commit
  mutation. Complete bounded dispatch remains open.

- Add explicit bounded struct-list upgrades, including compatible primitive and
  pointer-list promotion. Reserve before moving child pointers; preserve unknown
  fields, union selection and capacity on failure. Generate bounded generic type
  mappings and add AnyPointer casting, reads and transactional data copying.
  Exercise exact capacity, unknown enum ordinals, nested generics and incompatible
  bit lists in the isolated portable fixture and allocation/borrowing checks.

- Extend the bounded no-alloc builder with primitive, enum, struct and nested
  pointer lists, generated group/Void descriptors, transactional recursive data
  copies and explicit struct upgrades. Shared copying helpers return capacity
  failures; unpublished fixed-arena allocations are zeroed and reclaimed on
  errors/unwind. Copying preserves unknown fields and rejects capabilities
  before hook extraction; struct upgrades move existing payloads. Exercise
  exhaustion, malformed/cyclic input, far-pointer sources, borrowing contracts,
  zero allocations, Miri and bare-metal linking. Add a production Kani rollback
  proof and a required cursor-rollback mutation. Bounded capabilities and full
  no-alloc RPC integration remain in progress.

- Add `message::FixedBuilder` with checked caller-owned storage, fallible
  reservations and restricted schema-typed editors for structs, scalars, text
  and data. Shared wire helpers reserve before replacing a published object;
  rejected capacity/length requests preserve the old bytes, cursor and union tag.
  Generate scalar descriptors alongside pointer descriptors. Keep errors compact
  with `alloc` enabled and reclaim monotonic reservations on reset/drop.
  Add a bounded Kani proof of fixed-arena reservations with an exact-fit mutation
  control. New unsafe blocks document their contracts; refactoring shared helpers
  reduces existing undocumented unsafe-operation debt by 18, without new exceptions.

- Add an allocation-free, resumable stream receiver over caller-owned buffers.
  Borrowed read windows preserve partial progress across cancellation, prevent
  prefetching later frames and keep live readers from overlapping storage reuse.
  Use it in synchronous and async fixed-buffer readers; share header validation
  with hosted owning/buffered readers. Preserve compact async errors without
  allocating strings. Test fragmentation, cancellation, IO faults, capacity and
  length overflow, including allocation counting, Miri and bare-metal linking.
  Complete bounded RPC dispatch and fallible builders remain in progress.

- Move application handoff admission, revocation and drain accounting into the
  shared allocation-free RPC engine. Re-export the same types from the hosted
  API and exercise them in the allocator-free ARM and Miri fixture. Add pinned
  Kani proofs of production state transitions, ID tables and streaming credit,
  with isolated production mutations that must fail named assertions. Replace
  the small ID-table and handoff-counter TLC models/replays; retain distributed
  ordering and liveness models. CI runs Kani before the remaining TLC campaign
  and uploads separate proof results, counterexamples and duration graphs.

- Keep exclusive ownership of C++ codec and complete-reference test outputs
  across each whole invocation. Concurrent workspace/coverage checks can no
  longer relink the codec while another invocation executes it, or collide on
  the reference-suite build and evidence paths.

- Move ordinary, pipeline-only and adopted local RPC ID tables into the portable
  engine, using the same implementation in the hosted runtime. Store the free-ID
  min-heap alongside caller-owned slots so removals require no extra allocation.
  Preserve lowest-free-ID reuse, sparse-range isolation and rejected ownership;
  return explicit capacity, duplicate and invalid-range errors. Exercise faults,
  high-ID wraparound, teardown/reuse, compile-time role restrictions and reentry.
  Capability ownership, complete dispatch and caller-driven IO remain open.

- Extract typed answer/import table storage into the portable RPC engine and
  use it in the hosted runtime. Provide caller-borrowed bounded sparse slots
  with explicit duplicate/capacity errors and ownership-preserving failure,
  replacement and removal. Preserve the hosted low-ID fast path and sparse
  `HashMap`; high peer IDs never determine a backing array size. Exercise
  capacity faults, teardown/reuse and zero allocations, including Miri and
  bare-metal linking. Complete bounded dispatch remains part of the migration.

- Make generated borrowed data views and in-place field editors available
  without `std` or `alloc`, following the upstream serialization feature split.
  Keep owners, staged copies and capability hooks allocation-gated; select
  generated copying methods using the target runtime's features. Preserve
  error kinds without owned diagnostics and allow field-location formatting
  into caller storage. Test isolated target profiles and link generated views
  without a global allocator on ARM; interpret the fixture with Miri in CI.
  Bounded builders and full RPC remain open. Review the two core fingerprints
  changed only by removing allocation gates; unsafe-debt counts stay unchanged.

- Check fixed and growing packed-output sinks against the same known wire
  vectors, including zero runs and 255-word boundaries. Remove dependence on
  random segments for coverage of the growing writer's zero-run path.
  Review the packed source fingerprint without changing its unsafe-debt counts.

- Advance the simulated mapping deadline explicitly when testing a blocked
  shared listener, avoiding an assumption that packet ticks take exactly 1 ms
  under build contention. Keep the replacement reply and stale-ID checks.
  Service quiche recovery timers in the in-memory engine test, with an injected
  lost handshake packet and bounded completion of both streams and FINs.

- Isolate concurrent verification captures with per-file ownership through
  command completion and parsing. Prevent overlapping runs from truncating or
  mixing JSON and diagnostics, and apply cancellation/deadlines while waiting
  for capture files. Exercise concurrent writers and timeout without truncation.

- Default standard RPC to plaintext TCP and native directory routes to
  authenticated TCP/TLS. Select QUIC explicitly. Preserve vectored writes at
  the Tokio IO boundary, avoiding separate header/body sends. Add a matched
  release TCP/C++ benchmark with a 1.15× latency gate at every payload size.

- Extract streaming credit, adaptive windows and question Return/Finish
  lifecycles into a dependency-free `no_std` RPC engine used by hosted defaults.
  Check counter overflow, backwards clocks and duplicate/late returns; keep
  acknowledgements alive when accounting fails after a send. Link these
  transitions without a global allocator on bare-metal ARM in CI. Full bounded
  RPC dispatch, resource providers and fallible generated builders remain open.

- Add validated, caller-indexed multi-segment reading without allocation and
  with constant-time segment lookup. Reject truncated owned frames before
  publishing segment ranges, and check no-allocation frame-length arithmetic
  for overflow. Test truncation, capacity exhaustion and unchanged input/index
  state on failure, including allocation counts with heap features enabled.

- Use a reusable Linux monotonic timerfd for QUIC packet pacing below Tokio's
  millisecond timer resolution. Keep portable fallback timers, virtual-clock
  packet simulations, upstream congestion control, and packet deadlines intact.
  Allocate timer resources only when a packet actually needs to wait.

- Register the independent s2n-quic peer's endpoint shutdown before exchanging
  payloads, avoiding a missed wakeup even if the peer retires first. Exercise
  that race with a delayed post-FIN continuation in the interoperability check.
  Report the peer role, Retry mode, and current phase on interoperability
  failures while retaining payload, key-update, FIN, and timeout checks.

- Seed packed-framing property tests with empty segments and zero blocking
  periods so discard-guard coverage does not depend on the random corpus.

- Verify that an already-closed RPC reader immediately fails the QUIC driver
  and delivers the same error to shutdown waiters, without socket timing races.
  Exercise the same failure through public native-network read cancellation,
  including its structured transport error and listener-route cleanup.

- Make schema-call cancellation wait for request admission without a timer race,
  and exercise write batches beyond the metadata-retention bound with receipts,
  a subsequent batch, and sender shutdown. Check canceled write queues reject
  later sends, fail outstanding receipts, and clear retained metrics.

- Track `LLVM_PROFILE_FILE` in owned build scripts so each coverage run records
  fresh schema-generation counters even when Cargo reuses its target directory.
  Test cold builds, warm builds with a new profile destination, and unchanged
  warm builds against the actual RPC package.

- Invalidate prior JUnit references before repeating a quality step, including
  runs that exit without producing a report. Exercise actual passing and failing
  nextest fixtures, source changes, and separate stdout/stderr capture.
  Add regression coverage for rejected stream transitions, counter overflow,
  QUIC resets, copying-output shutdown, multi-segment write batches, scratch
  storage fallbacks, and high-ID ownership. Requalify the measured coverage
  baseline after reviewing the nextest/lane split and RPC refactors; retain the
  per-file and group regression gates and all zero-hit mappings.

- Forward the dedicated benchmark credential explicitly through reusable CI
  workflows, and check this contract alongside cloud trust restrictions.
  Use exact version comments for pinned installation actions so moving major
  tags cannot break workflow security validation.
  Isolate the platform CLI build from runtime feature builds to avoid repeatedly
  recompiling shared dependencies, and allow cold Windows builds to finish.

- Skip inactive bulk processing on the QUIC RPC path, keeping the full bulk
  handler separate. Remove its duplicate readable-stream allocation and
  reuse its bounded admission queue storage. Add a stalled-stream allocation and
  resume regression, plus a native TCP/TLS benchmark using the same authenticated
  RPC workload as QUIC. Record rejected timer and readiness experiments.
  Count MTU recovery only after quiche's own timer expires, so migration polling
  and advanced application clocks cannot trigger spurious path reprobes.

- Keep two-segment receive-frame metadata inline, eliminating its heap
  allocation. Build larger segment tables directly into immutable ranges and
  preserve bounded parsing, cancellation and retained-message ownership. Add
  allocation contracts and differential fragmented-framing coverage.

- Skip redundant diagnostic clock reads when adding RPC messages to an existing
  write batch. Recheck queue state under its locks when the receiver races the
  producer, and keep user clock callbacks outside locks. Add timestamp, stale
  hint and reentrant-clock regressions.

- Retain owned TCP/TLS frame payloads through bounded batched writes, removing
  two outgoing staging copies. Reuse receive storage and transfer it into an
  empty RPC bridge, preserving partial admission, wire framing and receipt
  fences. Add fragmented-frame, truncation and blocked-receiver regressions.

- Bound retries for DigitalOcean's explicit rejection of a newly registered SSH
  key, verifying key ownership and absence of a matching host before retrying.
  Ambiguous creation responses and unrelated errors still stop provisioning.

- Correct the C++ benchmark's per-byte assertion overhead and excluded response
  cleanup. All implementations now use bulk payload comparison and ten-second
  per-call deadlines inside timed round trips. Version measurements to reject
  legacy or mixed comparisons; previous large-payload ratios overstated progress.

- Retain the native recovery timer and scheduling notification across packet
  events. Reduce two-party RPC read-ahead storage to 8 KiB so larger frames
  reach their final allocation after a smaller copied prefix; message limits
  and the general buffered-reader API remain unchanged.

- Transfer native QUIC receive buffers into an empty RPC bridge without a
  second payload copy. Reserve for readable fragments once, reuse drained
  buffers, and retain bounded copying for partial admission. Exercise owned
  writes against Tokio's duplex stream alongside ordinary and vectored writes.

- Skip inactive shutdown-stream reads using quiche's readiness state, while
  retaining reset and empty-FIN errors. Keep RPC framing state in one stable
  allocation and sample migration/datagram deadline clocks only when needed.
  Correct result-allocation hints for the omitted empty capability table.

- Keep the first 16 peer-assigned RPC IDs inline, as in C++'s import table,
  avoiding hash lookups for ordinary answers and imports. Larger IDs remain
  sparse, with full-range, replacement and reentrant-drop regression coverage.

- Transfer owned buffers from the native RPC bridge directly into QUIC's send
  queue, removing a payload copy while preserving bounded admission, partial
  writes, cancellation and retransmission ownership. Share answer-status flags
  in one allocation, combine arena allocation with segment lookup, and omit
  empty capability-table tags as in the C++ RPC implementation.

- Send owned RPC buffers through upstream quiche's zero-copy API, preserving
  retransmission views and partial-write accounting. Reclaim acknowledged slabs
  and use their remaining capacity before allocating; retain bounded bridge reads.

- Validate near struct pointers and their complete targets with one segment
  lookup, preserving traversal and bounds checks without caching movable storage.
  Install schema compiler prerequisites in the dedicated benchmark report job.

- Reuse outgoing RPC segments in a pool bounded to 128 KiB and 16 segments per
  connection, clearing used words before reuse. Avoid protected background-task
  allocation for immediately completed non-streaming, non-pipelined RPC methods.
  Pending methods retain cancellation protection and local calls remain deferred.
  Bound task admission and reap ready work before allocating scheduler nodes.

- Run the complete CI graph on PRs and pushes to main: validation, platform
  checks, Cargo/coverage, models, fuzzing and extended checks, then release
  benchmarks and cleanup. Fork/Dependabot PRs compile benchmarks without cloud
  credentials. Publish distinct lane artifacts from one CI run atomically to
  the reports branch; preserve independent graphs, schedules and manual runs.
- Inline the first message-builder segment's metadata, following C++, removing
  its metadata allocation while preserving multi-segment and external-buffer
  ownership. A fitting scratch builder now requires no heap allocations.

- Add opt-in split control/bulk planes on the existing authenticated quiche
  session. Single-use capability grants authorize bounded payload streams;
  capability RPC, pipelining, cancellation and publication stay ordered on
  stream 0. Preserve old-peer/TCP bulk fallback, reserve control credit, expose
  bulk counters and drain reliable streams before graceful connection close.
  See `docs/wiki/Split-Plane.md` for limits and completion semantics.

- Reuse RPC write-batch storage, keep small framing tables on the stack, and
  complete immediately-ready non-pipelined calls without a background completion
  task. Native transport samples application clocks only for pending datagrams
  and migration, plus deadlines for active bulk transfers. Keep single-segment receive metadata inline, retain one input
  cancellation registration per connection, generate QUIC packets directly into
  bounded send batches, and construct disabled-pipeline errors only on use.
  The 1.2× C++ latency target has not yet been met.
- Retain outgoing call admission permits in the queued write instead of a
  separate completion task on the built-in transports. Unobserved sends avoid
  completion channels. A separate connection-level write completion signal
  fails RPC calls promptly even if input and close remain blocked. The output
  closure fence still waits for close; the network driver waits for disconnect.
- Use bounded initial segments for unhinted calls and two-party responses, avoiding
  tiny multi-segment responses and oversized empty-call arenas. Honor explicit
  request size hints with space for the RPC envelope. Reap RPC tasks directly and reuse a local task
  admission queue, with wake and destructor callbacks outside queue borrows.
- Use bounded local byte streams between native RPC and its TCP/QUIC drivers.
  The existing local runtime can split the stream without mutexes, while keeping
  vectored writes, backpressure, cooperative scheduling, and half-close behavior.
  Peer receipt validation still determines graceful shutdown completion.
- Build local result wrappers only when an RPC pipeline can observe them, and
  reuse the call executor per connection. Tail-call redirection retains its
  explicit result ownership and cancellation behavior.
- Keep the first buffered input segment's range directly, following the pinned
  C++ implementation's common-segment optimization. Retained frames own their
  word storage without a separate reference-count allocation; short-lived views
  retain safe shared ownership. Preserve multi-segment bounds and empty segments.
- Avoid scheduling idle checks while a live import or export proves that an RPC
  connection remains active. Preserve deferred checks on final release and
  reentrant table changes, following the C++ connection's early activity check.
- Submit native TCP/TLS frame headers and payloads in one vectored write instead
  of three separate TLS writes. Preserve partial-write handling, exact wire bytes,
  flush errors, and the receipt acknowledgement fence without copying payloads.
- Imported the maintained Cap’n Proto Rust runtime, RPC engine, async framing and
  generator as `capntproto-{core,rpc,futures,codegen}` workspace crates, retaining
  upstream licenses and generated Rust import names.
- Replaced the Quiche fork with unmodified crates.io quiche 0.30.0. Explicit QUIC
  v2 requests fail without downgrade. Native QUIC uses `capntproto/3` and explicit
  receipt confirmations; conventional QUIC write shutdown now reports local FIN
  submission. Recovery uses upstream system time. TCP/TLS remains supported.
- Capability RPC runtime and field-oriented Rust generator with bounded TLC,
  Rust trace replay and selected C++ reference/interoperability checks.
- TCP/TLS and quiche QUIC v1 transport with native mutual authentication, dynamic schemas,
  bulk/realtime services and explicit three-party introductions.
- Typed durable objects, publication history, atomic batches and revocable realms.
- Opt-in bounded async storage owner for V4/V5, with size/principal admission
  budgets, queued cancellation/deadlines, unread reply accounting, I/O quarantine
  and drain/cancel shutdown. Existing ORM RPC handlers remain synchronous.
- Opt-in RPROTO05 component manifests share unchanged data, support atomic edits
  and publication, typed local reads and RPC facets, and rebase shared references
  during compaction. Existing RPROTO04 files remain supported by `Store::open`.
- Owned RPC connections and native vats with typed bootstrap access after startup,
  peer-specific bootstrap factories, async connector closures, capability joins
  and bounded shutdown. Applications can build higher-level runtimes on these
  APIs; the experimental actor workspace crate has been removed.
- Opt-in outgoing RPC Call count limits per connection, retained through
  question/write ownership, plus queued/active output metrics and protocol
  resource snapshots. Payload-byte and local unresolved-client budgets remain
  separate work; the default remains unlimited.
- Opt-in generated structured replies make writing and forwarding mutually
  exclusive and freeze results on pipeline publication. Typed tail calls,
  directly awaitable calls and consuming parameter-construction closures improve
  client/server ergonomics without changing the wire protocol.
- Storage format **RPROTO04** independently checks record framing before tail
  recovery and stabilizes the recovered file and directory before serving.
  Versions 1–3 are rejected; there is no automatic migration.
- Coordinated core/RPC/generator/futures dependencies work from an external
  application without root Cargo patches. Distribution is a source bundle.
- EAE comparison and private integration probe; the default backend remains
  the whole-entry store, with component storage available explicitly.
  See `docs/wiki/EAE-Comparison.md` and `docs/wiki/Component-Storage.md` for measurements.

- Canonical guides now live in `docs/wiki/`, with task-oriented navigation, a
  validated GitHub Wiki export, and an archive for superseded development ledgers.

This is an experimental preview, not complete C++ API parity or a production
security qualification. See `docs/wiki/Release-Acceptance.md` for outstanding gates.

## Review follow-up (2026-10-04)

- Replace repository Python tooling with the `capntproto-dev` workspace crate,
  invoked through `cargo run --locked -p capntproto-dev -- <subcommand>`. Port CI,
  reporting, cloud benchmark lifecycle, fuzzing, model generation and research
  probes, with Rust regression tests. Retire obsolete archived Python runners;
  historical measurements retain their original provenance.
- Use independent s2n-quic peers for QUIC v1 interoperability checks, including
  Retry, key rotation and stream shutdown. The production backend remains quiche.
- Separate generated CI evidence into an orphan `reports` branch. Keep the source
  README editable and discover documentation automatically instead of maintaining
  a duplicate page inventory. Existing measured history is retained as a frozen seed.
- Move the unimplemented Capnt Actors design to `docs/proposals/`; correct QUIC v1,
  unsupported v2 and the distinct native TCP/QUIC ALPN identifiers.
- Enforce unsafe documentation for application code; forbid unsafe in RPC/codegen/
  futures and track source-bound core-runtime documentation debt in CI. Document
  arena and descriptor ownership contracts without claiming a full unsafe audit.
- Split RPC dispatch and capability handling, and schema-node generation, into
  focused modules. Replace unresolved RPC size hints and clarify constructor checks.
- Document minimal TCP/TLS builds separately from QUIC and full verification tools.
  Retain nextest process isolation and the existing feature/transport contracts.

# Capnt Actors — Rust virtual actor API specification

Capnt Actors is a proposed virtual actor layer for **Capntproto**, the system in this workspace. It builds on the maintained `capnp`, `capnp-rpc`, `capnp-futures` and `capnpc` crates, the Rust schema compiler, and the `capntproto` RPC, native transport, authority and storage modules. Its wire foundation remains Cap'n Proto serialization and capability RPC. The application API combines ordinary Rust methods, authorized typed references, exclusive actor turns, and an explicit transaction boundary for durable state and effects.

This is a normative design proposal, not an implemented actor runtime. The planned workspace crate is `capnt-actors`, imported as `capnt_actors`; the actor macros, generated clients, host, CLI and persistence guarantees below are proposed. Their examples have not been compiled or benchmarked. Existing packages and wire identifiers keep their current names. “Must” identifies a requirement for a future conforming implementation, not a property already supplied by `capntproto`. Provider construction and routine application plumbing are omitted where they do not affect the contract.

## Relationship to the current system

The [runtime status](docs/wiki/Runtime-Status.md) and [RPC application guide](docs/wiki/RPC-Applications.md) describe the implemented foundation. This proposal adds actor policy above those APIs; it does not introduce another RPC engine or QUIC backend.

| Area | Current Capntproto foundation | Capnt Actors work still required |
| --- | --- | --- |
| RPC ownership | `capntproto::rpc::Connection` for two-party connections; `capntproto::native_rpc::Vat` for authenticated multiparty RPC, both on a Tokio `LocalSet` | Actor host, activations, exclusive turns and bounded actor mailboxes |
| Transport | Plain `rpc::tcp`; conventional CA-validated `rpc::tls` and `rpc::quic`; native pinned mutual-TLS sessions over TCP or quiche QUIC v1/v2 | Actor service registration and deployment policy; production actor peers use authenticated sessions |
| Capabilities and pipelines | Generated clients, `RemotePromise`, field-API `PendingCall`, `with_params`, `send().await`, `into_parts`, capability pipelines, typed tail calls and structured server replies | Lazy actor invocation builders, method modes and durable operation observation |
| Three-party RPC | Native introductions, authenticated Join, answer adoption and pipeline migration with ordering fences | Actor ownership/placement protocol and storage fencing; capability handoff alone does not transfer an activation |
| Authority and restoration | Authority grants and revocation; `persistence::Realm`, owner-sealed `persistence::SturdyRef`, factories and authenticated restore | Tenant/actor bindings, typed actor restoration wrappers and operation tokens |
| Storage and ORM | `Store::open` for RPROTO04 whole-entry storage; `Store::open_components` for opt-in RPROTO05 component storage; `ObjectState`, `ComponentState`, atomic publication, history and compaction | Fenced actor transactions combining state, input, result, outbox and timer records; durable actor deduplication and recovery |
| Async storage | Opt-in `storage::worker::Worker`, bounded count/byte/principal admission, explicit outcomes and shutdown | Actor-authorized transaction adapter, execution-time grant checks and integration with actor scheduling; existing ORM RPC handlers remain synchronous |
| Resource observation | Opt-in per-connection outgoing Call-count limits, pending/active output metrics and protocol snapshots | Actor/tenant limits, byte budgets, unresolved-call budgets and fair scheduling across actor work |

The [transport guide](docs/wiki/Native-Transports.md), [storage guide](docs/wiki/Storage-and-ORM.md), [worker contract](docs/wiki/Storage-Worker.md) and [persistence realm](docs/wiki/Persistence.md) define the limits of reuse. There is currently no actor runtime, placement service, replicated actor store, general durable operation receipt API, or distributed ownership fence in this workspace.

### Terminology and identity boundaries

| Term | Meaning here |
| --- | --- |
| Actor host | Proposed executor/service owner for activations; it owns or coordinates a native RPC vat on a `LocalSet` |
| Vat / `VatId` | Existing native RPC endpoint and its authenticated 32-byte Ed25519 public-key identity; neither a logical actor nor an ownership lease |
| Native connector / peer directory | Existing `Connector`, `DirectoryConnector`, discovery and provisioning policy for authorized network routes |
| Placement directory | Proposed actor-generation-to-owner mapping and ownership metadata; distinct from the peer directory |
| `ActorId` / `EntityGeneration` | Proposed logical entity identity and retirement/recreation boundary |
| Ownership term | Proposed activation fencing epoch enforced by the actor transaction provider; distinct from native route generations and persistence owner-key epochs |
| `ObjectKey` / `Revision` | Existing store-local address and storage revision domains; neither grants authority nor proves distributed ownership |
| Tenant / principal / realm owner | Application tenancy, admitted workload identity and persistence authority are separate bindings; a worker principal number or an authenticated peer key is not automatically an actor grant |

References below to a “provider” mean a proposed adapter that must implement the stated actor guarantees. Existing local storage, transport receipts, capability Join and persistence realms do not imply those guarantees by sharing similar terminology.

## Scope and defaults

The core profile has one authoritative owner per logical actor, exclusive execution across suspension points, bounded admission, actor-local atomic commits, durable operation deduplication, and capability-based access. Physical activations are disposable. Logical identities, committed state, accepted durable work, and restoration grants have separate lifetimes.

| Concern | Default | Explicit alternative |
| - | - | - |
| Actor implementation | Struct plus annotated inherent methods | Imported schema and generated server trait |
| Contract | Generated default `Api` interface | Named interfaces with declared inheritance |
| Remote use | `reference.method(args).await` | Admission handles, durable operations, pipelines |
| Scheduling | One exclusive turn, including across `await` | Immutable projections or separate workers |
| State | `State<T>` with staged mutation | `Table<K, V>`, `Log<E>`, `EventState<S, E>` |
| Mutation | One actor transaction | Separate future transaction extension |
| Reads | Authoritative owner with a valid read barrier | Explicit snapshot reference and freshness policy |
| Failure | Typed domain outcomes and bounded recovery | Suspension, quarantine, administrative repair |
| Schema identities | Checked-in manifest | Explicit IDs and rename mappings |
| Deployment | Ordinary Rust executable | Optional hosting integrations |

The runtime does not transparently persist Rust futures, make external HTTP effects transactional, or provide global ordering between independent callers. Single ownership does not mean that a disconnected old process immediately stops executing; fencing prevents its managed writes and effects from committing.

The [ActorDB proposal](docs/wiki/ActorDB-Proposal.md) and
[implementation checklist](docs/wiki/ActorDB-Checklist.md) describe a future
event-sourced database profile with incremental views. That profile requires
additional event-feed, projection-checkpoint and query-progress contracts;
these are not implied by the generic actor core or implemented by this document.

## The everyday actor

```rust
use capnt_actors::prelude::*;

#[derive(Wire, Persistable, Debug, thiserror::Error)]
pub enum CounterError {
    #[error("counter overflow")]
    Overflow,
}

#[derive(Actor)]
#[actor(key = String)]
pub struct Counter {
    value: State<u64>,
}

#[capnt_actors::methods]
impl Counter {
    #[query]
    pub fn current(&self) -> u64 {
        *self.value
    }

    #[command]
    pub fn increment(&mut self, by: u64) -> Result<u64, CounterError> {
        let next = (*self.value)
            .checked_add(by)
            .ok_or(CounterError::Overflow)?;
        self.value.set(next);
        Ok(next)
    }
}
```

`Actor` generates activation construction and managed-field metadata. `capnt_actors::methods` generates dispatch metadata and verifies the implementation against the generated contract. `State<u64>` starts at `u64::default()` for a new entity unless an explicit initializer is declared.

The separate contract crate exposes `counter::Entity`, `counter::Api`, a generated concrete `counter::Client`, method descriptors, owned wire types, borrowed wire views, and the Cap'n Proto schema. `ActorRef<counter::Api>` resolves to that generated client specialized with an actor handle. Clients do not depend on the implementation struct or server crate. DTO declarations such as `CounterError` are contract sources; production projects place them in the shared contract source/module and import the canonical types into implementations. The examples colocate declarations for readability.

```rust
use app_contracts::counter;
use capnt_actors::prelude::*;
use std::time::Duration;

// `client` is an authenticated connection to an application bootstrap service.
let ns = client.namespace("acme").await?;
let counter = ns.actor::<counter::Api>("homepage").await?;

let value = counter.increment(1).await?;
let current = counter.current().timeout(Duration::from_secs(2)).await?;
```

Namespace acquisition and actor lookup check authority and bind an entity generation. Lookup does not load actor state or pin an activation. Cached lookup may avoid another network round trip when its authority and generation binding remain valid. Invalid keys and denied authority are reported at lookup instead of being hidden in the first business call.

The common command requires no explicit transaction parameter. Add one only when staging effects or using transaction metadata:

```rust
#[derive(Wire, Persistable)]
pub struct CounterChanged {
    pub value: u64,
}

// Alternative implementation of increment, replacing the version above.
#[capnt_actors::methods]
impl Counter {
    #[command]
    pub fn increment(
        &mut self,
        by: u64,
        tx: &mut Transaction,
    ) -> CommandResult<u64, CounterError> {
        let next = (*self.value)
            .checked_add(by)
            .ok_or(CounterError::Overflow)
            .map_err(CommandError::domain)?;
        self.value.set(next);
        tx.publish(CounterChanged { value: next })?;
        Ok(next)
    }
}
```

`CommandResult<T, E>` is `Result<T, CommandError<E>>`; `CommandError::Domain(E)` records a domain rejection and `CommandError::Abort(StageError)` aborts the transaction. `From<StageError>` supports `?` for staging and storage failures; domain errors use `CommandError::domain`. Generated command clients return `Result<T, CommandFailure<T, E>>`; query and request clients use their own mode-specific errors. For ordinary `Result<T, E>` methods, all returned errors are domain rejections. No macro rewrites the meaning of Rust’s `?` operator.

## Identity and entity lifetime

```rust
pub struct ActorId {
    pub tenant: TenantId,
    pub kind: ActorKindId,
    pub key: CanonicalKey,
    pub generation: EntityGeneration,
}
```

`ActorKindId` is a stable manifest identity for the logical entity type. It is independent of Rust names, deployment locations, and interface IDs. The canonical key encoding is versioned, size-bounded, and identical in every language. Key strings use their specified bytes; normalization is never silently introduced during an upgrade.

An interface selects operations and authority on an actor. An ownership term identifies a particular activation owner’s fencing epoch. Neither belongs in the logical key. Multiple interfaces share one actor identity, state, mailbox, revision sequence, and ownership record.

Lookup resolves the current entity generation. Existing references remain bound to that generation forever. Explicit retirement fences the owner, records a tombstone, and prevents old references and accepted work from targeting a later recreation. Recreating the same key requires an authorized lifecycle operation and creates a new generation.

Resolving an identity, activating it, initializing durable state, and performing business creation are different operations. A document can exist as a resolvable identity while its business state is `None`. Applications implement create-if-absent as an ordinary command with a domain result. Querying a fresh actor may read default state; default construction alone does not imply a durable commit.

`ActorId` is inspectable routing identity, not permission to invoke an actor. Constructing or guessing an ID cannot create a capability. IDs and generation comparisons do not establish equality of authority grants. Existing capability Join checks supported capability identity/equality semantics; it does not compare actor placement records or acquire an ownership term.

## Explicit interfaces and contracts

Small actors use the generated `Api` interface. Public methods become remotely callable only when annotated with a method contract. Unannotated helpers remain local. Internal completion, reminder, and subscriber handlers are excluded from public interfaces.

Named interfaces are declared on method groups when applications need separate authority:

```rust
#[derive(Actor)]
#[actor(key = String)]
pub struct Document {
    title: State<String>,
}

#[capnt_actors::methods(interface = Reader)]
impl Document {
    #[query]
    pub fn title(&self) -> String {
        self.title.get().clone()
    }
}

#[capnt_actors::methods(interface = Editor, extends(Reader))]
impl Document {
    #[command]
    pub fn rename(&mut self, title: String) {
        self.title.set(title);
    }
}
```

The contract emits `document::Reader` and `document::Editor`, both associated with `document::Entity`. `Editor` includes `Reader` through declared interface inheritance. There is no default public `Api` unless a default method group is present.

```rust
let editor = ns.actor::<document::Editor>("guide").await?;
let reader = editor.control().restrict::<document::Reader>().await?;
let title = reader.title().await?;
```

Restriction creates or derives receiver-enforced authority, using the existing authority/revocation machinery where its contracts fit. It is not merely a Rust interface cast or a capability Join operation. A restricted reference must not retain an externally usable route to the original stronger grant. Further delegation cannot exceed the parent grant’s operations, scope, expiry, or delegation budget.

Business methods live directly on the generated client. Framework actions such as saving, restricting, inspecting identity, and obtaining event streams live on `.control()`. Only that accessor name is reserved; a remote method named `control` requires an explicit Rust-side name mapping. An application method named `save`, `events`, or `id` remains usable normally.

`#[query]` describes managed-state and retry behavior. It is not a security annotation. Reader interfaces are explicit because even a query can return a powerful capability. Interface review includes the authority of returned references and every inherited method.

Remote methods cannot have unconstrained type parameters, expose Rust references with remote lifetimes, return `impl Trait`, or depend on platform-sized integer wire encodings. Generic local helpers remain ordinary Rust. Context parameters are runtime-injected and absent from the business argument list.

## Method contracts

| Annotation | Handler | Managed durable state | Retry contract |
| - | - | - | - |
| `#[query]` | `&self`, sync or async | Read committed state only | May repeat; latest read can differ |
| `#[command]` | `&mut self`, sync or async | Stage changes and effects in one transaction | Deduplicate by durable operation identity |
| `#[request]` | `&mut self`, usually async | No managed durable writes | No automatic retry after possible execution |
| `#[reply]` | `&mut self`, optional transaction | Internal durable completion turn | Deduplicated internal delivery |
| `#[reminder]` | `&mut self`, optional transaction | Internal durable timer turn | Deduplicated occurrence identity |
| `#[subscribe]` | `&mut self`, optional transaction | Internal stream-consumer turn | Cursor or inbox deduplication |

Commands and internal durable handlers accept ordinary `T`, `Result<T, E>`, or `CommandResult<T, E>` results. Plain `T` is lifted to an infallible domain outcome. `CommandResult` is needed only when handling fallible staging or transactional storage operations with `?`. Query and request infrastructure failures are represented separately by their generated adapters.

Public commands require persistable owned argument representations and persistable success and domain-error results. A handler may borrow `&str`, `&[u8]`, or generated views from the runtime-owned request during its turn. These views cannot escape into detached work or durable state.

A query promises no externally visible domain mutation. Temporary capability allocation is allowed, but retry may produce a different equivalent live capability. Interior mutability or injected clients do not magically make side effects safe; Rust cannot prove this contract. A request is the appropriate boundary for volatile effects.

The basic profile rejects public `#[request]` methods on an actor containing managed durable state. Split volatile adapters into separate actors or services. This avoids implying that a mutable volatile request participates in the durable transaction model.

## State and the commit boundary

Every actor field must declare its role through its type or an attribute. Unknown ordinary fields are a compile error, preventing a newly added business field from accidentally becoming transient.

| Field | Role | Mutation behavior |
| - | - | - |
| `State<T>` | Small durable value | `set` stages replacement; `update` stages a mutation |
| `Table<K, V>` | Durable indexed records | Asynchronous transactional reads and writes |
| `Log<E>` | Actor-owned append-only records | Appends staged with the actor transaction |
| `EventState<S, E>` | State derived by deterministic event reduction | Stages events and the derived working state |
| `Inject<T>` | Runtime dependency | Never serialized; external effects stay external |
| `#[transient(reset_on_abort)] T` | Cache or activation-local resource | Reset after failed turns before reuse |

`State<T>::get()` and `Deref<Target = T>` provide read access. `set(&mut self, T)` and `update(&mut self, impl FnOnce(&mut T))` stage writes. There is no `DerefMut`. The convenient `update` requires a provider-supported working copy, normally `T: Clone`; `set` does not require cloning the previous value. Large structures should use granular tables rather than repeated whole-value replacements.

Durable mutations require an active command transaction (including an internal durable-handler transaction). Access from activation hooks or other nontransactional contexts is rejected as a handler programming fault. The runtime must detect this even when a helper method hides the mutation from a macro. Managed values may not expose mutable aliases or interior-mutability escape hatches that bypass staging. This is a runtime-checked transaction context in the convenient field API, not a claim that Rust can statically prove arbitrary helper code transactional. Validated field/value traits and compile-time signature restrictions complement that runtime guard; arbitrary user `Wire` implementations do not certify the absence of interior mutation.

On successful execution, one fenced transaction commits the managed writes, input acknowledgement where applicable, operation completion and encoded result, staged outbox records, reminder changes, and continuation registrations. None may become externally visible as committed before the others. The provider must support atomicity across these records for one actor. A local implementation may encode one actor’s state and metadata in a V4 object/batch or a V5 object’s components, subject to existing limits. V5 component IDs are bounded structural parts, not unbounded table rows. Its root CAS and atomic publication do not provide cross-object component transactions, distributed fencing, or deduplication by themselves. Multi-host operation requires an authoritative transaction/ownership service; sharing or copying a local Store file is not such a service.

Result serialization and effect validation occur before commit. A serialization failure therefore cannot follow a committed mutation with an unreplayable result. Responses claiming success are released after durable commit. Outbox dispatch begins only from committed records. Actor commit metadata must distinguish the storage head from the published revision: a private draft is not a completed actor command. `storage::worker::Worker` moves blocking Store operations to their owner thread, but does not add the actor transaction boundary or grant checks.

| Turn outcome | Managed state and staged effects | Operation record | Activation |
| - | - | - | - |
| Success | Commit atomically | Save success result | Reuse |
| Domain rejection | Discard staged changes and effects | Save rejection and acknowledge accepted input atomically | Reuse after transient reset |
| Confirmed abort | Discard | No successful completion; retry or suspension follows policy | Reset or reconstruct |
| Panic with unwind | Discard if commit has not started | Resolve any uncertain commit before retry | Retire |
| Commit outcome unknown | Do not assume commit or abort | Reconcile the original operation | Block further mutable turns |

An abort cannot retract an already committed external HTTP request. Such effects must use an outbox adapter with its own idempotency contract, or be treated as volatile effects outside Capnt Actors’ atomic guarantee.

A domain rejection that intentionally needs durable audit effects should instead return a successful business decision such as `Ok(Decision::Denied { reason })`, staging the audit in that successful transaction. Returning a domain error always rolls back staged business changes.

Transient reset is explicit: `Default` reset is the convenience path; a declared reset hook handles resources that are not default-constructible. A reset failure retires the activation. Caches tagged by committed revision can be invalidated instead of reconstructed. Panicked activations are never reused merely because a mutex was unpoisoned.

## Ownership and transaction-provider protocol

The provider is the authority for ownership, accepted work and commit state. A directory record is a routing hint derived from that authority, never a write credential. The ownership record binds full `ActorId`, a strictly increasing non-reusable term, authenticated owner identity, phase, and committed revision. Terms do not reset on restart; retiring and recreating a key changes the entity generation. A provider may use consensus, a transactional authority service, or another implementation with equivalent guarantees; this contract does not require Raft specifically.

| Transition | Required atomic decision |
| - | - |
| Acquire vacant ownership | Compare the authoritative record, allocate a fresh term, fence earlier terms, and enter `Recovering` with one candidate owner |
| Begin transfer | Compare the active term, record the transfer identity/target, allocate the next term in `Recovering`, and fence source admission/commits at a recoverable handoff boundary |
| Recover | Reconcile uncertain source transactions, accepted operations, results, outbox and cancellation records; never infer abort from an absent reply |
| Activate target | After recovery and expiry/revocation of conflicting old read proofs, atomically enter `Active` for the new term; load state through the recovered committed position before serving business calls |
| Publish route | Advertise the active term after its authority exists; a stale or prematurely observed route still cannot authorize work |
| Retire | Fence the generation and record its terminal lifecycle disposition, including unfinished durable obligations |

Acquisition/transfer tokens authorize only provider recovery/control work while `Recovering`; business starts and commits require `Active`. These transitions are idempotent under one acquisition/transfer identity. A target crash before activation and a source crash during transfer leave provider-owned recovery state; they do not allow two owners to win independently. Source cooperation can shorten transfer but is not required for safety. A provider must specify how it revokes or waits out old read leases before enabling a conflicting new owner. Clock-based expiry is usable only with the read-proof assumptions below. Disabling a route or timing out a process is not a storage fence.

Every mutable provider transaction supplies an `OwnershipToken`, expected committed revision, operation/input identity and fingerprint, plus the complete state/result/effect write set. The provider validates ownership and expected revision in the same atomic decision that publishes that write set. A preflight ownership check followed by an unfenced store write is invalid. Admission, cancellation, start records, maintenance, supervision counters and internal completion turns require the same fencing contract; they cannot bypass it by being runtime metadata. All writes that affect execution eligibility must be ordered with retirement and transfer.

A successful commit returns `CommitProof` binding actor generation, committing term, operation/input, committed revision, authenticated write-set/outbox digest and advertised durability. Effect dispatch must prove membership of that exact effect ID, target and request fingerprint in the committed write set; possession of an unrelated commit receipt cannot authorize a fabricated effect. Proofs are provider-authenticated or unforgeable service handles; caller-supplied numbers are not proof. An old owner cannot create a new valid proof after being fenced. A proof for work already committed under an old term remains valid after transfer and permits recovery/delivery of that recorded effect. It does not grant the holder permission to perform a new actor transaction. `OwnershipToken` (permission to attempt a new commit) and `CommitProof` (evidence of an existing commit) are different types.

An unknown commit result blocks subsequent mutable turns until provider reconciliation identifies the exact transaction as committed or aborted. The new owner inherits that obligation. Recovery uses the authoritative transaction identity/log, not a local cache or re-execution to discover whether side effects happened. Provider failures may stop availability; they must not downgrade uncertainty to abort.

### Read proofs

An authoritative query must linearize at a committed position obtained after that query was invoked, or use a provider-certified lease covering an equivalent linearization point. The provider returns an opaque `ReadProof` bound to actor generation, ownership term, committed position, and validity conditions. The query pins one immutable view at or beyond that position, satisfying any `AtLeast` constraint; it cannot combine fields from different revisions. If validating or acquiring the view takes the proof outside its valid interval, the owner obtains another proof before serving it.

Supported proof mechanisms must state their assumptions:

| Mechanism | Required evidence |
| - | - |
| Quorum-confirmed position | A current authority/term check ordered after invocation, a safe committed read position, and local application through that position |
| Validated lease | An exclusive read authority whose expiry, clock drift/skew bounds, restart behavior and transfer waiting rules preclude a concurrent conflicting owner; a wall-clock timestamp alone is insufficient |
| Equivalent provider guarantee | A documented linearizable read transaction or other proof satisfying the same invocation-order and fencing requirements |

With a consensus-backed provider, an unconfirmed old leader or a cached earlier read index is insufficient. Implementations must establish current-term authority according to that provider's protocol and wait for the selected position to be applied. A lease is invalid after loss of its clock assumptions, an unaccounted process pause, or a restart unless the provider explicitly proves continued validity. When proof is unavailable, authoritative reads wait within their budget or fail. They do not fall back to a stale projection.

### Authority checks and durability profiles

The core authorization policy checks invocation authority at admission and again at durable execution start, serialized with the relevant grant-policy epoch. Revocation before start records terminal nonexecution for queued work. Revocation after start does not retroactively cancel an authorized running turn or its committed effects. Deployments requiring revocation at commit must negotiate that stronger policy and integrate the grant check into the atomic commit decision; a cached preflight check cannot advertise it. Ownership fencing is always checked at commit, independently of this grant policy.

Duplicate observation and result replay require current observation authority; they must not accidentally require permission to execute a new mutation. A still-valid observer can retrieve a result after invocation permission is revoked. Conversely, possession of the original invocation grant cannot bypass a revoked observation token. Control handlers check their own restricted grants.

Provider profiles are explicit: `ProcessOnly` loses state on process loss, `SingleHostDurable` supplies durable actor transactions with exclusive local ownership and no automatic multi-host failover, and `MultiHostFenced` supplies the transfer and read-proof contract across hosts. Only the last may advertise automatic owner relocation. Single-host replacement requires verified exclusive access and crash recovery; an unavailable host cannot be presumed dead. Negotiation and receipts name the profile, storage durability/failure domain and retention policy. A process-only development service must not claim durable recovery, and no production fallback may silently select it.

## Generated calls and bounded admission

Generated actor calls are named, `#[must_use]` `ActorCall` builders. They own their arguments before dispatch, or own a wire message that safely backs them. Implementing `IntoFuture` keeps ordinary use concise without eagerly sending a request.

```rust
// Constructing or dropping an unstarted call does not invoke the actor.
let call = counter.increment(1);
let result = call.await?;

// Immediate local dispatch admission. Failure returns the unstarted call.
let pending = counter.increment(1).try_start()?;
let result = pending.await?;

// Asynchronous admission, bounded by waiter and byte budgets.
let pending = counter.increment(1).start().await?;
let result = pending.await?;

// Remote durable inbox admission; processing may occur later.
let operation = counter.increment(1).enqueue().await?;
let result = operation.result().await?.value;
```

`call.await` on an `ActorCall` is shorthand for bounded admission followed by waiting for the result. `try_start()` performs no wait for local capacity. `start().await` waits for local capacity while respecting a finite waiter budget and timeout. A successfully started `PendingInvocation<T, E, M>` means the runtime owns the dispatch attempt, not that the remote actor has accepted or executed it.

`enqueue()` exists only for persistable commands. It returns an `Operation<T, E>` only after a fenced durable inbox admission transaction. If the acknowledgement is lost, `CommandFailure::Tracked` reports uncertain admission and retains the authorized recovery handle and frozen request. An ordinary command call guarantees durable completion on success but does not promise to survive server failure while merely waiting in a volatile mailbox.

The proposed actor facade exposes `try_start`, `start`, and `enqueue` to distinguish its three admission guarantees. The existing Cap'n Proto request `.send()` API remains supported and is used underneath; it returns an already-sent `RemotePromise` (or field-API `PendingCall`), not a lazy actor invocation or a durable acceptance receipt. `try_start` returns `TryStartError<ActorCall>` with a reason and the owned unstarted call. A command cannot dispatch until its authorized recovery handle and immutable request have been retained; asynchronous admission errors follow the same rule.

Admission accounts for message count, encoded size bounds, decoded materialization, waiters, queued outbound work, and outstanding reply buffers. Reserving capacity before expensive encoding is supported through `Client::reserve(DispatchBudget).await`, producing an RAII permit consumed by `ActorCall::start_with(permit)`. A permit exceeding its declared budget must acquire more capacity or fail before remote submission. It cannot silently enlarge the queue. These are actor-layer requirements: current RPC outgoing Call admission is opt-in and count-based per connection. Output metrics, reader limits and streaming flow control are not a complete byte/tenant/unresolved-call budget.

The generated builder surface is mode-specific:

| Operation | Available on | Result |
| - | - | - |
| `.await` | Every generated call | `Result<T, QueryError<E>>`, `Result<T, RequestError<E>>`, or `Result<T, CommandFailure<T, E>>` by mode |
| `.timeout(duration)` | Every generated call | Same builder with a caller budget |
| `.retry(policy)` | Every generated call | Same builder; mode restricts safe retry classes |
| `.start().await` | Every generated call | Bounded local admission to `PendingInvocation` |
| `.try_start()` | Every generated call | Immediate admission or recoverable unstarted call |
| `.idempotency(key)` | Commands | Builder with an explicit `OperationKey` |
| `.expires_at(deadline)` | Commands | Builder with an immutable latest-start constraint |
| `.prepare().await` | Commands | Frozen `PreparedCall` with automatic or explicit key, without business submission |
| `.enqueue().await` | Commands and prepared commands | Durable admission to `Operation<T, E>` |
| `.with_receipt()` | Commands | Success adapter to `Committed<T>`; command rejection receipts are always retained |
| `.read(policy)` | Queries | Owner read constraint; snapshot references have their own modes |
| `.view()` | Calls with a generated message-view output | Success adapter to `ActorResponse<T>` |
| `.pipeline()` | Started calls with capability result paths | Generated capability-only pipeline view |

Preparation freezes the operation key, arguments, method version, and execution deadline. Observation timeouts may still be adjusted without changing identity. Typestate is used internally so invalid combinations are missing methods or direct compile errors; users do not write a long list of builder type parameters. View and receipt adapters compose as `Committed<ActorResponse<T>>` and do not alter the stored wire result. The `T` in `CommandFailure<T, E>` and its recovery handle names the original generated result contract, independent of a selected success adapter; recovery restores the original operation rather than a caller-specific borrowed view. All generated calls, prepared calls, pending handles, and unfinished effect builders are `#[must_use]`. `PendingInvocation` is distinct from RPC `PendingCall` and storage-worker `Pending<T>`; `ActorResponse` owns a client result and is distinct from the existing server-side `capnp::capability::Reply<T>`.

### Overload and control progress

Business admission and control work have separate bounded budgets. Each actor/tenant/host reserves capacity for authenticated status, cancellation, ownership recovery, result reconciliation, outbox acknowledgements and shutdown. Control classification is selected by registered protocol operations and restricted grants; a caller cannot bypass business quotas by setting a priority flag. Limit control request size, outstanding requests, per-principal rate and response retention as well.

Control handlers read/update authoritative operation and ownership records without acquiring the busy actor's exclusive business turn. Cancelling queued work or reporting its state must not wait for the running handler to finish. Controls that actually change business state or require quiescence remain fenced maintenance operations and may report pending instead of executing concurrently with an actor borrow. Stalled storage, a partition or exhausted control budget may still cause bounded failure; reserved capacity is not an unconditional liveness guarantee.

Preserve capability E-order and all native handoff/pipeline ordering fences. Control priority is not permission to reorder arbitrary RPC messages, discard Return/Finish/Release, or overtake an ordering-dependent business call. The integration must either keep protocol progress running with bounded business rejection/draining, or provide a separately authenticated and budgeted control connection with independent protocol ordering. Cross-connection cancellation races are resolved by the operation record, never assumed resolved by arrival order. This must work over both TCP and quiche without relying on datagrams.

The current RPC incoming `flow_limit` can pause the whole message reader; setting it alone cannot satisfy this contract or provide a hard process-memory limit. If that behavior remains, the host needs an independent control path or a qualified RPC progress adapter. Business backpressure must take effect before consuming the reserved progress capacity. Global frame and decoding limits remain enforced, and an abusive connection may be failed rather than allocating without bound. Completion, acknowledgement and reclamation have reserved provider capacity so quota exhaustion cannot indefinitely prevent the actions that free capacity.

## Outcomes timeouts and cancellation

Client errors are mode-specific. Handler-side `CommandError<E>` remains the staging/domain-error type; it is not a client invocation failure. The following is the proposed public semantic shape; diagnostic payloads and non-exhaustive evolution follow the contract manifest.

```rust
pub enum QueryError<E> {
    Rejected(E),
    NotStarted { reason: NotStartedReason },
    Interrupted { cause: QueryFailure },
}

pub enum RequestError<E> {
    Rejected(E),
    NotStarted { reason: NotStartedReason },
    Uncertain { cause: Uncertainty },
}

pub enum CommandFailure<T, E> {
    // Available only if no attempt could have reached the remote service.
    NotSubmitted { reason: AdmissionError },
    Tracked {
        outcome: CommandOutcomeError<E>,
        recovery: CommandRecovery<T, E>,
    },
}

pub enum CommandOutcomeError<E> {
    Rejected { error: E, commit: CommitInfo },
    NotExecuted { proof: NonExecutionProof },
    Aborted { proof: AbortProof },
    Uncertain { cause: Uncertainty },
    Suspended { reason: SuspensionReason },
    ResultExpired { completion: CompletionMarker },
    RetryExpired,
    IdentityConflict,
    ObservationFailed { error: ObservationError },
}
```

A query/request rejection cannot contain a command receipt. A command rejection must contain its durable rejection receipt. Every command failure after possible dispatch, including a lost admission reply, is `Tracked` and retains `CommandRecovery<T, E>`. `OperationId` is diagnostic data and never substitutes for this authorized handle. Recovery may subsequently be denied by revocation or expiry; retaining a handle does not override authority checks.

Three different facts must remain separate:

| Fact | Meaning and evidence |
| - | - |
| Attempt outcome | Whether this particular dispatch reached or ran a handler; useful for diagnostics and retry policy |
| Durable operation state | The authoritative record across every attempt, owner term, and reconnect |
| Client observation | What this caller learned before its observation budget or authority ended |

`NotStarted` proves nonexecution for the query/request attempt. `NotSubmitted` proves that no command attempt could have reached the service. For a tracked command, `NotExecuted` requires a terminal nonexecution record preventing delayed attempts from starting. `Aborted` requires authoritative reconciliation showing no committed completion and no unresolved attempt that could commit; it is a retryable state, not a terminal cancellation. An individual handler abort without that reconciliation is only an attempt outcome. A later submission may still execute an aborted operation under the same identity.

The SDK carries uncertainty across automatic retries. For example, a lost first reply followed by a second attempt rejected before dispatch remains `Uncertain` unless reconciliation establishes a stronger operation outcome. `RetryExpired` rejects further execution permission without asserting that an earlier attempt failed. A timeout, disconnect, or unreadable store never establishes nonexecution. For volatile requests, possible execution means possible external effects.

`timeout(Duration)` limits the caller's current admission and wait attempt. Once a `PendingInvocation` is returned, its timeout keeps running; starting the handle does not reset the deadline. Retrying transport hops consumes the remaining budget. A fresh user retry may choose a new waiting timeout while keeping the same operation identity.

`expires_at(ExecutionDeadline)` is separate: it establishes a durable latest-start constraint for a command. The server records it with the operation's immutable request identity. Duplicate submission cannot extend it. Expiration before starting records a terminal nonexecution outcome; expiration while a handler runs does not retroactively undo the handler. Cross-node enforcement uses an advertised authority-clock model, never an unexplained comparison of unsynchronized wall clocks.

Dropping an unstarted call prevents dispatch. Dropping a pending handle abandons observation; it is not a rollback request. Accepted durable operations survive loss of observers. `Operation::cancel().await` attempts a fenced terminal cancellation before execution starts and returns `Result<CancelOutcome, ObservationError>`, where `CancelOutcome` is `Cancelled { proof: NonExecutionProof }` or `TooLate`. An observation error leaves the cancellation outcome unknown; callers retain the operation handle and inspect it again.

Cancellation of a valid but unseen operation must atomically record a nonexecution tombstone bound to its immutable request fingerprint before returning `Cancelled`. A concurrent or delayed submission then encounters that tombstone. Returning success merely because a mailbox lookup found no entry is forbidden. Cancellation, durable admission, start, and completion serialize through the same operation record. No API claims that cancellation undoes a commit.

The actor runtime must retain an admitted durable operation independently of the caller's RPC response/pipeline ownership and choose the generated method cancellation policy accordingly. Core RPC cancellation can stop permitted handler work, and dropping a storage-worker `Pending<T>` attempts cancellation before start; neither supplies the actor's durable lifetime automatically. `#[must_use]` is a lint, not a linear ownership guarantee, and Rust permits destructors to be skipped. Durability, fencing, release of durable obligations, and correctness must not depend on `Drop` running.

Query retry is permitted under its no-domain-mutation contract. Command retries reuse the exact operation identity. Volatile requests retry only when nonexecution is proven. Retry policies classify failures and apply bounded attempts, elapsed time, jitter, and tenant budgets; they never retry domain rejections automatically.

## Durable operation identity and recovery

Every command receives an operation identity and authorized observation handle before its first possible dispatch, including a simple `.await`. The automatic path acquires a valid retry scope, freezes the request, and creates recovery state before sending business work. A failure during this preparation is `NotSubmitted`; once dispatch is possible, failures carry recovery. Scoped observation authority must permit lookup of that identity even if the first remote reply is lost and the server has never observed it. Preparing an operation must not require a successful business admission first.

```rust
let scope = client.retry_scope().await?;
let key = scope.key("http-request-481")?;
let prepared = counter.increment(1).idempotency(key).prepare().await?;

// Observe before submission; this does not activate or invoke the actor.
let handle = prepared.operation();
let token = handle.save().await?;
// Persist `token` and the original versioned request if resubmission is needed.

let operation = prepared.enqueue().await?;
let value = operation.result().await?.value;

// A later client process restores observation without invoking again.
let restored: Operation<u64, CounterError> =
    client.restore_operation(&token).await?;
let status = restored.status().await?;
```

`prepare().await` is command-only and consumes the call into a `PreparedCall<T, E>` that owns its immutable request. With no explicit key it obtains an automatic key; an explicit key is validated against its scope. Preparation may contact the scope/authority service but neither submits business work nor activates the target. `PreparedCall` supports `.await`, `start`, `try_start`, and `enqueue`. `try_start()` on an ordinary unprepared call can succeed only when the necessary scope and local preparation are already available; otherwise it returns the unstarted call without network work.

`CommandRecovery::operation()` returns a cloneable authorized `Operation<T, E>`. `CommandRecovery::retry()` returns the original `PreparedCall<T, E>` or a typed permission/expiry error; it never invents a new identity. It retains the frozen request, its original schema/fingerprint versions, and the original invocation grant, separately from observation authority. Reconciliation precedes another handler execution whenever an earlier commit may still be unresolved. A retry may refresh routing or observation timeout, not the immutable request.

A saved `OperationToken<T, E>` grants observation and, only when explicitly delegated, cancellation. It does not contain arguments or grant resubmission authority. The token binds the operation and expected fingerprint even before admission. Cross-process resubmission additionally requires the original versioned request and still-valid invocation authority, retained through an explicit protected encoding API. Neither a saved diagnostic ID nor possession of a result grants fresh mutation rights. Automatic SDK retry cannot promise recovery after the client's own process dies unless the application saved these artifacts.

`RetryScope` is issued by an authority and has a server-enforced expiry. `scope.key(value)` derives a scoped operation key from a bounded caller identifier. Arbitrary UUIDs without a valid scope are not accepted as an eternal deduplication promise. Automatic SDK scopes are renewed for new work; existing operation identities are never silently moved into a newer scope.

Completed results remain available until at least the later of the retry scope's execution expiry and `completed_at + minimum_result_retention`, measured by the advertised authority clock. A provider must publish that nonzero minimum observation interval, size limits, and any longer application policy before admission. Unfinished accepted operations retain their request and recovery records until a terminal outcome; scope expiry alone cannot erase them. A command completing after its scope expires still receives the minimum post-completion observation interval. Observation credentials may expire earlier, but an authorized renewal path must not change the operation identity or retention deadline.

After full results become eligible for pruning, retained markers distinguish known completion or nonexecution from unknown history. Expired scopes are rejected before invoking, even when all per-operation metadata has been reclaimed and after migration/restart. A cancelled operation's tombstone must remain effective for every period during which its identity could be executed. Thus an old retry cannot become new work. Runtime-produced outbox identities use the separate acknowledgement protocol below.

`status()` returns `Result<OperationStatus, ObservationError>`. Authoritative status includes its read position and distinguishes `NotObserved`, `Accepted`, `Running`, `Suspended`, `Completed`, `NotExecuted`, `CompletedResultPruned`, and `ExpiredUnknown`. `NotExecuted` includes cancellation, expiry before start, and terminal authorization rejection. `Running` is advisory. `NotObserved` is a read at an instant, not a promise that a concurrent submission cannot arrive. `CompletedResultPruned` requires a retained completion marker; absent evidence after expiry is `ExpiredUnknown`. Lookup failures, denied access, and revoked tokens are `ObservationError`, never a fabricated business status.

`result().await` returns `Result<Committed<T>, OperationResultError<E>>`. Its error variants are `Rejected { error, commit }`, `NotExecuted { proof }`, `Suspended { reason }`, `ResultExpired { completion }`, `ExpiredUnknown`, and `Observation { error }`. Rejection includes a mandatory commit receipt; `ResultExpired` requires a known completion marker. Decoding a stored result can fail observation without changing the operation. A still-pending operation waits only within its observation budget. `status()` and `result()` never submit business work and never manufacture resubmission authority; the operation handle remains usable after an observation error.

External business idempotency that must last longer than a retry scope belongs in durable domain state, for example an order table keyed by the merchant's order ID. The actor protocol's deduplication horizon cannot replace that invariant. RPC question IDs, native delivery receipts and reconnection retries are not durable operation identities or completion records. These guarantees cover one managed commit per valid operation identity, not exactly-once delivery or arbitrary external effects.

### Request fingerprint version 1

Identity conflict is checked before execution or result replay. The lookup key is the full actor generation plus scoped operation identity; its immutable request fingerprint binds the fields below. An unauthorized caller must not use conflict responses to inspect another principal's operations. Observation authorization is checked before revealing records.

The version-1 fingerprint is `SHA-256(domain || frame(f1) || ... || frame(f10))`. `domain` is the exact ASCII byte sequence `capnt-actors/request-fingerprint/v1` followed by one zero byte. `frame(x)` is the unsigned 64-bit little-endian byte length of `x` followed by `x`. Integers in fields use unsigned fixed-width little-endian encoding; there are no host-sized integers. The fields, in order, are:

| Field | Encoding |
| - | - |
| f1: actor identity | Frames of the manifest-defined canonical tenant ID, UInt64 actor-kind ID, versioned canonical key, and entity generation |
| f2: operation identity | Frames of the issuing authority ID, retry-scope/producer-generation ID, and scoped operation key |
| f3: authorization binding | Frames of principal ID, grant ID, and grant-policy epoch; stable IDs, not a renewable credential's signature bytes |
| f4: method identity | UInt64 interface ID followed by UInt16 method ordinal |
| f5: semantic contract version | UInt64 version from the contract manifest |
| f6: original argument schema | UInt64 root type ID followed by the 32-byte SHA-256 digest of its checked-in schema descriptor artifact |
| f7: request representation | UInt32 canonical-argument format version, initially 1 |
| f8: arguments | Canonical Cap'n Proto root-struct bytes described below |
| f9: execution deadline | One byte 0 for absent; otherwise byte 1, framed authority-clock ID and UInt64 latest-start tick |
| f10: execution semantics | Canonical bytes of the versioned execution-options struct, including required durability/feature semantics and any declared execution/read preconditions |

ID/key/clock encodings and the descriptor artifact are frozen, language-neutral protocol artifacts. Their exact byte widths where variable and interpretation versions must be declared in the manifest. Nested fields in f1–f3 use the same length framing. Opaque IDs compare by exact bytes. A schema descriptor digest is over the exact distributed artifact bytes; regenerating an equivalent descriptor with different bytes is a new representation version. Actor-protocol schemas, stable numeric IDs, those artifacts, and cross-language golden vectors must be checked in before any implementation advertises version-1 compatibility.

Canonical-argument format 1 uses Cap'n Proto's canonical single-segment tree representation, including the root pointer and excluding stream segment-table framing or packed encoding. Validate traversal, depth, allocation and size limits before canonicalizing; capability pointers and cyclic structures are invalid durable arguments. Canonicalization preserves present unknown data and pointer fields rather than decoding into a smaller DTO and dropping them. Noncanonical sender allocation, segment boundaries, and pointer placement cannot change the digest. Primitive defaults follow the declared Cap'n Proto wire defaults. Null pointers and explicitly allocated default objects remain distinct when canonical bytes distinguish them; the SDK must freeze and reuse the original representation rather than reconstruct it from getters.

Text is validated UTF-8 with no implicit Unicode normalization; data uses exact bytes. Floating-point values preserve bit patterns, including signed zero and NaN payloads. List order is significant. A manifest-declared map is encoded as entries sorted lexicographically by each key's canonical bytes, with duplicate canonical keys rejected; an ordinary list of entries is not implicitly a map. Canonicalization does not make all application-equivalent values identical. Schemas that want case folding, normalized numbers or another equivalence must apply an explicit versioned conversion before preparation.

Restorable references use their complete frozen versioned durable representation: realm and opaque token bytes plus actor/interface, owner and grant bindings when present in that format. Current raw realm tokens include realm ID and token bytes, while their owner binding resides in the realm ledger; the adapter must not invent an owner field absent from that format. Tokens are hashed locally as part of the request and never emitted in diagnostic fingerprint logs. Renewing or resealing a token changes that argument unless a separately versioned stable descriptor protocol defines otherwise. Relocation must resolve existing accepted descriptors, not rewrite their recorded fingerprint.

Observation timeout, trace/span IDs, transport addresses, current host/ownership term, connection IDs, and retry count are excluded. Execution deadline, principal/grant identity, actor generation, business arguments and execution preconditions are immutable. Different fingerprints under the same operation identity produce `IdentityConflict` without a new handler invocation; this is a tracked submission failure with the original recovery state. The client's recovery handle does not authorize inspection of a conflicting record with another fingerprint.

Wire compatibility does not imply fingerprint compatibility: a list-layout upgrade can change canonical bytes even when ordinary decoding remains compatible. Retries preserve the original schema artifact, format version and canonical bytes; a new generator must not re-encode an old operation under its current schema. Migration can transform a separate execution copy only under declared behavioral compatibility, retaining the original identity and fingerprint. Unsupported old work is suspended with a compatibility reason. It is never executed as a new operation.

## Execution and concurrency

One activation owns its managed state. The default executor admits one turn at a time, including queries, and retains exclusivity across every `await`. Awaiting storage suspends the actor task, not the RPC executor thread, while its exclusive turn remains held. The existing storage worker performs blocking I/O on a dedicated owner thread; generated RPC clients, grants and other `Rc` state remain on their owning `LocalSet`.

No `&self` annotation implies concurrent access to an object while another turn holds `&mut self`. There is no default reentrancy and no automatic self-call shortcut that executes a second turn recursively. Local helper calls remain ordinary Rust calls inside the current turn.

A query waiting for another actor can deadlock through a dependency cycle. Direct self-waits are rejected; bounded call-chain metadata may detect and reject further known cycles. Untrusted chain metadata is size-limited and never grants authority or enables reentrancy. Exceeding its budget reports a diagnostic or bounded failure rather than enabling interleaving. This is diagnostic assistance, not complete distributed deadlock detection. Durable continuations, separate worker actors, and immutable projections are the supported ways to release dependencies.

While a durable turn is active, Capnt Actors clients reject immediate outbound command/request dispatch and direct durable enqueue. Those effects must be staged through the current transaction. Queries may be awaited while retaining exclusivity, although long dependencies are better represented by continuations. This guard covers Capnt Actors dispatch; it cannot intercept arbitrary application sockets or injected third-party clients.

Independent actors run concurrently. Work that must not hold an actor turn can be placed in a bounded compute or I/O worker pool with a completion message. Detached tasks cannot capture actor borrows, a transaction, or borrowed message views. Blocking CPU work must use a bounded compute executor; it must not block the RPC executor.

Concurrent submissions from different callers have no global ordering guarantee. Even one caller must not assume that independently started calls complete in source order. Awaiting command completion before issuing a dependent call establishes the required dependency. The runtime preserves the existing capability E-order and native handoff/pipeline fences. These constrain delivery on references and route transitions, not completion order of arbitrary async handlers. Durable retries across connections still require actor-level sequencing.

## Durable continuations

Durable continuations commit an outgoing request and a typed completion descriptor, then release the actor turn. Completion arrives as a later transactional turn. Capnt Actors persists a descriptor and owned captures, never a Rust stack frame, closure environment with arbitrary pointers, or future.

```rust
#[derive(Actor)]
#[actor(key = String)]
pub struct Checkout {
    pending: State<Option<JobId>>,
    quote: State<Option<Quote>>,
    issue: State<Option<QuoteIssue>>,
}

#[capnt_actors::methods]
impl Checkout {
    #[command]
    pub fn refresh_price(
        &mut self,
        sku: String,
        pricing: SturdyRef<pricing::Api>,
        tx: &mut Transaction,
    ) -> CommandResult<JobId, Never> {
        let job = tx.new_job_id();
        self.pending.set(Some(job));
        self.issue.set(None);

        tx.request(pricing.quote(sku))
            .complete_within(Duration::from_secs(30))
            .reply_to(checkout::replies::price_ready(job))?;

        Ok(job)
    }

    #[reply]
    fn price_ready(
        &mut self,
        job: JobId,
        outcome: Outcome<Quote, PricingError>,
    ) {
        if *self.pending != Some(job) {
            return;
        }
        self.pending.set(None);

        match outcome {
            Outcome::Returned(quote) => self.quote.set(Some(quote)),
            Outcome::Rejected(error) =>
                self.issue.set(Some(QuoteIssue::Rejected(error))),
            Outcome::Failed(error) =>
                self.issue.set(Some(QuoteIssue::Failed(error))),
            Outcome::Uncertain(info) =>
                self.issue.set(Some(QuoteIssue::Uncertain(info))),
        }
    }
}
```

`Quote`, `PricingError`, and `QuoteIssue` are persistable application contract types. `Never` is Capnt Actors’ uninhabited domain-error type. `Duration` is imported from `std::time` as in the client example. `SturdyRef` exposes generated lazy call descriptors; immediate use restores as necessary, while `tx.request` captures the restorable target without dispatching.

The `reply_to` call completes staging and validates the descriptor. Dropping its unfinished builder stages nothing; an unused expression produces a must-use diagnostic. Explicit disposal or suppressed lints are possible, so transaction validation must not rely on that diagnostic. Captures, request arguments, and both success/error result representations must be persistable, including when the target method is a query. Handler IDs and capture schemas are versioned in the manifest. Invocation of the reply handler requires a runtime completion grant and is not available through the actor’s public business interface.

Outgoing commands receive a child operation identity derived from the parent operation and staged effect identity. The outbox preserves that identity across retries and relocation. Queries may be repeated under their read-only contract. Volatile requests are rejected by this durable request API unless an explicit external adapter supplies a compatible effect contract.

Runtime effects do not inherit a client retry scope that may expire while the outbox is pending. Each record contains a stable `EffectId` derived from parent operation/input identity and effect ordinal, the parent's immutable commit proof, producer generation, full target generation, per-target sequence, authorized target grant/descriptor, original request bytes/fingerprint, and any completion descriptor. It becomes dispatchable only with its parent commit. Commit evidence authenticates the recorded intent; recipient invocation/grant checks still apply according to the negotiated revocation policy. The dispatcher validates that proof even when its committing term is older than the current owner; it must not demand that the original term still owns the actor. Newly created effects still require a current fenced commit.

Delivery and business callback are separate persisted state machines:

| Delivery state | Transition and recovery rule |
| - | - |
| `CommittedPending` | Dispatch the retained request under its original identity; never synthesize a replacement operation |
| `DispatchUnknown` | Reconcile or resubmit that same identity after a lost reply; a process crash cannot establish nonexecution |
| `Accepted` | Recipient has durably retained the child operation; wait/observe across restart without creating another child |
| `CompletionRetained` | Producer has durably retained the terminal result/nonexecution proof and any callback input |
| `Acknowledged` | Recipient has accepted an idempotent durable acknowledgement; per-effect results may be reclaimed under the frontier rules |
| `Uncertain` / `Suspended` | Preserve evidence and recovery authority when the target cannot be reached, decoded or restored; require explicit resolution, never silent success |

A callback independently moves from pending to one selected completion input, then to committed or suspended. Timeout may select an uncertain callback while delivery remains `Accepted` or `DispatchUnknown`. A later target result is retained and acknowledged without selecting another callback. Executing the callback and acknowledging its input use the producer's fenced actor transaction; acknowledging the target result only requires durable retention of that result and callback obligation, not successful callback execution first.

Deduplication uses a durable producer-generation namespace and monotonic sequence per full target actor generation. The recipient retains completion records until durable producer acknowledgement permits reclamation. A retained contiguous acknowledgement frontier rejects delayed duplicates at or below it after individual results are pruned. Out-of-order acknowledgements are stored as bounded gaps; highest-seen sequence is insufficient. A gap can close only with a retained completion, proven cancellation/nonexecution, or an explicit protocol resolution acknowledged by both sides. A sender-local timeout or deleted outbox row cannot advance it.

An authorized abandonment stops further dispatch and records the business decision, but does not prove that a dispatched child failed. If the recipient cannot confirm its outcome, the obligation remains uncertain and retains its deduplication evidence. A sequence allocated but never dispatched is resolved by an authenticated skip/cancel handshake that prevents delayed execution. Producer retirement closes its namespace with a final allocated sequence per target and transfers unfinished obligations to a recovery owner. Namespace closure rejects new submissions while retaining already accepted child work; compact closure/frontier tombstones survive as long as old identities could be presented. Recreating an actor cannot reopen its retired producer generation.

Revocation, target retirement, an unsupported schema or permanent delivery failure is a visible terminal nonexecution only with authoritative evidence that no earlier attempt committed or can still execute. Otherwise it suspends delivery or reports uncertainty. Retiring a target generation needs an explicit disposition for accepted effects and retained results; lookup must not redirect them to a recreated generation. External adapters must publish their own deduplication and recovery limits and cannot inherit this managed-actor guarantee by returning an HTTP success.

Admission reserves bounded durable capacity for outbox records, result retention and callback obligations before the parent can commit. Unavailable capacity aborts staging or rejects admission; it cannot discard an already committed effect. Advertised payload/result maxima bound reservations before dispatch, with reserved space for an abort/suspension record if execution cannot fit its contract. Recipient retention credits and bounded sequence windows limit unacknowledged work; quota pressure stops new work, not completion, reconciliation or acknowledgement. No TTL may erase accepted unfinished work or make a replay executable. Administrative expiry must use the explicit terminal/namespace protocol and retain the required tombstones.

`complete_within` records one durable completion deadline relative to the parent’s commit authority time. Dispatch retries do not extend it. Timeout and response compete to commit one terminal completion selection. Late responses cannot execute the callback again. A timed-out mutation may still complete at the target, so the callback receives `Uncertain`, including an authorized child-operation observation handle for managed command targets. Queries have no command operation; external adapters must declare whether they can provide recovery authority. `Failed` is reserved for a definite protocol outcome such as proven nonexecution.

The pending job check in the example is still necessary: internal deduplication prevents duplicate delivery of one job, but a newer business request may supersede an older job. A suspended callback retains its completion input until explicitly resumed or resolved; it is never silently acknowledged as successful.

For durable inter-actor fire-and-observe effects without a business callback, `tx.enqueue(command)` stages an outbox record and returns a job identity. Its acceptance and failure remain inspectable. It cannot mean untracked, infallible fire-and-forget.

## Granular persistence and event state

```rust
#[derive(Actor)]
#[actor(key = String)]
pub struct Cart {
    lines: Table<String, Line>,
}

#[capnt_actors::methods]
impl Cart {
    #[command]
    pub async fn put_line(
        &mut self,
        sku: String,
        line: Line,
    ) -> CommandResult<(), Never> {
        self.lines.insert(sku, line).await?;
        Ok(())
    }
}
```

`Line` is a persistable application record. The table is automatically bound to the active actor transaction. `get`, `insert`, `remove`, and range/index operations read their own staged writes. Storage errors convert into the abort branch of `CommandError`; applications cannot accidentally cache a transient provider failure as a domain rejection through that path.

Indexes are declared in the schema manifest and maintained in the same transaction as their rows. Range cursors have explicit row and byte limits and close at the end of the turn. Application-visible cross-turn pagination uses versioned cursor tokens with declared snapshot or continuation semantics; a borrowed database cursor cannot survive passivation.

`Log<E>::append` stages actor-owned records. A local log is not automatically a distributed stream. Publishing requires a stream/outbox adapter with its own acknowledgement and retention semantics.

`EventState<S, E>` requires a registered reducer implementing `Reducer<E> for S` with `fn apply(&mut self, event: &E)`. `emit(event)` updates a working state and stages the event. The reducer must be deterministic and perform no I/O. Event append, operation completion, and staged effects commit together. Snapshotting may accelerate replay but cannot replace event history without an explicit retention policy.

Replay cannot publish effects, schedule reminders, or rerun external requests. Invalid event history or failed state migration quarantines the entity for inspection. Reducer and event-schema upgrades require explicit version support; changing Rust code alone is not an event migration.

These tables, logs and reducers are proposed actor persistence APIs. `ComponentState` currently supplies typed, whole-object-CAS component revisions; publication history is not an actor event log or durable stream subscription. All proposed persistence variants must share the actor’s atomic boundary. A provider that cannot atomically combine the chosen state primitives with inbox, result, and outbox records must fail configuration validation. `Table` does not claim cross-actor foreign-key maintenance, distributed serializability, or distributed transactions.

## Capabilities and persistence types

| Type | Meaning | Durable representation |
| - | - | - |
| `Cap<I>` | Live interface capability, potentially a temporary object | None by default |
| `ActorRef<I>` / `LocalActorRef<I>` | Shared-dispatch / executor-local authorized interface on one actor generation | Must explicitly obtain a restoration token |
| `SturdyRef<I>` | Proposed typed client/restoration wrapper for an authorized interface | Explicit realm token plus validated actor/interface binding |
| `ActorId` | Logical identity for diagnostics and routing | Persistable data without invocation authority |
| `Operation<T, E>` | Typed observation handle for one command | Explicitly saved `OperationToken<T, E>` |

```rust
let reader = editor.control().restrict::<document::Reader>().await?;
let saved: SturdyRef<document::Reader> = reader.control().save().await?;

let restored: ActorRef<document::Reader> = client.restore(&saved).await?;
let title = restored.title().await?;
```

Saving and restoring are authorized protocols. Restoration revalidates tenant, expiry, revocation, interface, actor generation, and delegation scope. Tokens are secrets: redacted `Debug`, no ordinary `Display`, no unguarded logging, and explicit encoding APIs for protected storage. Copying a token does not extend its lifetime. The existing `capntproto::persistence::SturdyRef` is a nongeneric realm-ID/opaque-token value, and `Realm` owns authenticated save/restore checks and host-registered factories. The generic actor `SturdyRef<I>` adds typed actor descriptors and lookup behavior; it is not an alias for that raw value. The adapter must preserve owner sealing, restore-time checks, key-rotation epochs and expiry. Revoking a realm token does not retract an already restored live capability or automatically revoke independently saved child tokens; actor grant lineage must be enforced explicitly where the proposal requires it.

`Wire` describes transportability. `Persistable: Wire` additionally declares an owned, versioned representation that can be decoded after restart. Derivation recursively verifies field types; an owned struct containing `Cap<I>` or an unsaved `ActorRef<I>` is not persistable. `SturdyRef<I>` being persistable does not promise that later restoration will succeed after revocation.

The first profile does not allow durable commands to return live capabilities. They return `SturdyRef<I>` when capability authority must be recorded in a replayable result. Queries and volatile requests may return live `Cap<I>` and `ActorRef<I>`. This makes result replay unambiguous and keeps capability reconstitution explicit.

Ephemeral capabilities are useful for cursors, uploads, resource sessions, and live callbacks. They need not be entered in the actor placement directory. Dropping their last live reference follows the advertised RPC lifetime behavior; a durable actor remains independent of live reference counts. Cap'n Proto Finish/Release and connection teardown govern live RPC ownership, not retirement of an `ActorId` or deletion of accepted durable work.

The authenticated bootstrap service grants namespaces or initial capabilities. Namespace possession conveys only its declared lookup and interface-grant permissions. Proxies and third-party handoff must preserve the actual caller/delegation authority; a server’s own privileged namespace cannot be substituted when forwarding someone else’s request. Native mutual TLS identifies the immediate peer. It does not establish the original caller’s tenant or grant through a proxy; those restrictions must travel with the delegated capability and be checked by the actor service.

### Restorable authority across owner relocation

The current realm authenticates the restoring peer against the owner key in its ledger. Copying host A's owner-sealed token into actor state on host B does not authorize B to restore it. Ownership terms, realm owner-key epochs and actor grant epochs are distinct. Multi-host providers must declare and implement one of these authority-preserving strategies before admitting actors that persist such references:

| Strategy | Required relocation behavior |
| - | - |
| Stable restoration service | An actor-aware service owns the realm binding and accepts term-bound delegated actor requests; it restores/proxies under its stable identity and enforces the current actor generation and grant lineage |
| Scoped rebinding/delegation | A coordinator authorized by the realm/reference policy transfers a reference or an actor-exclusive owner binding from source to destination, with replay-safe transfer IDs, expected epochs and recovery records |

The core adapter should prefer a stable restoration service when using the present realm API. It adds availability, capacity and authorization obligations of its own; it is not present in `capntproto::persistence`. Existing tokens sealed directly to a departed host require an authorized migration/delegation path. If that authority is unavailable, the operation is suspended as unrestorable rather than restored with a forged peer identity. Sharing a logical actor's private key among concurrent hosts is not a relocation protocol.

Relocation first records the affected actor generation, old/new terms, grant lineage and reference-binding plan. The provider fences old commits, then the restoration service/coordinator validates the destination's ownership proof and installs idempotent new bindings. The target activates only after all required bindings are usable, or records a recoverable suspension. Crash recovery completes or safely retries this plan; a partially moved reference set cannot justify unfenced execution. Existing accepted request bytes and token descriptors remain unchanged for fingerprint checking; a versioned binding resolver may map those original descriptors to their authorized current binding.

`Realm::rotate_owner` rotates every reference sealed to that owner. Rotating a shared host owner just to move one actor would affect unrelated actors and is forbidden as an actor-local migration strategy. Actor-exclusive owners or explicit reference-scoped delegation need a capacity and isolation design: current realm limits allow 1–256 owners and 1–4096 references, not an unbounded owner per actor. A provider must negotiate and enforce these limits or implement a separately qualified scalable authority service.

Rotation/revocation rejects future unauthorized restores but does not retract already restored capabilities or independently saved descendants. Actor-issued live capabilities must therefore carry receiver-enforced grant lineage/epoch checks at invocation admission and execution start, including local fast paths, proxies and third-party handoff. An actor acting as a caller must present current term-bound delegation where source relocation is intended to fence old-host invocation. A stale live proxy may not bypass those checks. Work authorized before the handoff follows the declared in-flight revocation policy; already committed effects remain obligations. Opaque external capabilities without equivalent revocation/fencing remain outside that guarantee and must be declared as such.

### Executor-local and transferable references

Reference mobility and payload mobility are different properties. `ActorRef<I>` is a proposed `Clone + Send + Sync` dispatcher handle; it does not make every method's arguments, result, future or error `Send`. The following distinctions are part of generated API validation:

| Surface | Allowed boundary |
| - | - |
| `LocalActorRef<I>` / `Cap<I>` | Owner-executor calls, including live local capabilities, borrowed message views and `!Send` handler futures |
| `ActorRef<I>` | Cross-executor calls only with generated owned `Transferable` argument/result/error representations |
| `SharedCap<I>` | Explicit bounded proxy to a live capability's owner executor, with the same transferable-payload restriction |
| `SturdyRef<I>` | Owned durable descriptor; restoration still requires authority and yields the selected local/shared client flavor |

`Transferable` requires a validated wire representation plus `Send + 'static`; it is not implied by `Wire` or `Persistable`. Generated shared methods are available only when all boundary payloads qualify. An interface with a live capability in a request/result uses its executor-local client, or an explicit generated shared projection substituting authorized `SharedCap` handles. Such projection is opt-in, accounts for proxy lifetime/queue budgets, and preserves the exact granted interface and delegation restrictions. It never serializes an `Rc` or moves a raw local capability through a channel.

`ActorRef::control().local()` resolves a `LocalActorRef` on the owning RPC executor. `cap.control().proxy()` creates `SharedCap` there before it crosses a thread boundary. A shared projection constructs local proxy endpoints on the appropriate executor while only transferable descriptors traverse dispatcher channels. Message-backed outputs may cross only when their owned backing storage and all embedded references qualify; a borrowed view cannot cross independently. The generator diagnoses unavailable shared methods and points to the local or explicit proxy surface. There is no unsafe `Send` implementation for current generated clients, native vat state, ORM grants or actor borrows.

## Pipelining and wire ownership

```rust
// `files` is a LocalActorRef on the RPC executor.
// `files.open` is a query returning a live file capability.
let pending = files.open("report.bin").start().await?;
let file = pending.pipeline().file();
let bytes = file.read(0, 4096).await?;
```

Generated pipeline accessors exist only for capability paths in results. The pipeline can queue dependent calls before the parent reply arrives, but cannot reveal an integer, string, list contents, or a domain-union discriminant early. A capability path that is absent because the parent rejected the request produces a failed dependent call, never a default authority grant.

An exported pipeline capability retains its parent question until dependencies release it; dropping the observation handle alone cannot invalidate live dependencies. The actor integration must add explicit budgets for pipeline queues, unresolved promises, and capability tables; current RPC diagnostics and optional outgoing Call-count limits do not enforce all of them. Disconnect and failure propagate through the dependency chain.

Actor-generated adapters must preserve the underlying `RemotePromise`/`PendingCall` pipeline and completion owners, using `into_parts()` where necessary instead of awaiting the parent before exposing a child capability. Only typed capability result paths are pipelinable; there is no actor method call encoded as an arbitrary byte-message envelope.

Owned DTOs are the convenient default. Generated `FooRef<'a>` views borrow a validated message. For large replies, a call can select an explicit view result:

```rust
let reply: ActorResponse<document::Metadata> = reader.metadata().view().await?;
let view = reply.view();
consume_title(view.title()?);
```

This snippet assumes a declared `metadata` query on the reader interface. `ActorResponse<T>` owns the backing message; the view cannot outlive it. Converting to owned data is explicit. Public borrowed-return annotations are lowered to owned or message-backed client representations, never to arbitrary remote Rust lifetimes. The adapter builds on `capnp::capability::Response<T>` and the generated reader/field APIs. The proposed `FooRef` naming does not rename the existing `Reader`, `MessageReader` or `MessageView` types.

`WireBytes` can carry already owned validated message storage, and `Bytes` can share ordinary byte buffers. Neither implies zero copies through encryption, persistence, alignment repair, or cross-process transport. Existing traversal, nesting and framing limits must remain in force. The actor layer adds decoded-allocation and retained-result budgets. Current RPC uses unpacked Cap'n Proto framing; a future compressed profile requires explicit negotiation and a decoded-size budget. None of these layers permits bypassing checked decoding at an untrusted boundary.

Generated actor servers should reuse opt-in `structured_replies(true)`: fresh `Reply<T>` can complete, build, or tail-call an exact-compatible `Request<P, T>`; `ReplyBuilder<T>::publish()` freezes the full result into `PublishedReply<T>`. Do not publish a successful durable command or its dependent capabilities before commit. Early capability publication for queries/volatile services requires established authority and usable behavior; a late parent error cannot retract child effects. Typed tail forwarding and third-party answer adoption preserve RPC semantics but do not transfer actor transaction ownership.

## Authoritative reads and immutable projections

Ordinary queries run against the authoritative owner’s committed state using the [read-proof contract](#read-proofs). Possession of an old lease or cached route alone is insufficient. During a partition, the runtime fails or waits rather than serving an authoritative query from a fenced owner.

```rust
let receipt = counter.increment(1).with_receipt().await?;
let value = counter.current()
    .read(ReadPolicy::AtLeast(receipt.version))
    .await?;
```

`with_receipt()` changes a command’s successful result to `Committed<T> { value, version, operation, durability }`. Versions bind tenant, actor kind, key, entity generation, and a monotonically increasing committed revision. They are comparable only for the same entity generation. Deduplicated completion returns its original revision. This actor `CommitInfo` is a proposed application receipt. It is distinct from a native transport shutdown/delivery receipt and from an unscoped `storage::Revision`; the adapter binds the selected committed/published storage revision to the complete actor identity and ownership proof.

`AtLeast(version)` provides a minimum committed version, not an exact historical read. A wrong-actor or wrong-generation token is rejected. On `ActorRef` this uses the authoritative owner. Replica selection requires a separate `SnapshotRef`, whose provider must prove sufficient progress. Existing mapped storage snapshots are local immutable versions; they are not replicated read proofs or automatically actor-authorized snapshot capabilities.

Concurrent read replicas use `SnapshotRef<I>`, whose handlers operate on immutable committed snapshot data rather than the live actor struct. Snapshot construction and replication must publish whole versions atomically. Declared modes are `AtLeast(version)`, `Stale { max_age }`, and provider-supported `AtVersion(version)`. Bounded staleness requires an explicit clock/freshness proof; missing proof is an error, not a best guess.

Snapshot interfaces and handler registration are an extension to the core profile. A plain `#[query]` never automatically becomes a snapshot handler. Reads across several actors do not acquire a consistent distributed snapshot unless a separate snapshot/transaction extension provides it.

## Activation hooks and supervision

`#[activate]` and `#[deactivate]` mark private lifecycle methods taking `&mut self` and an `Activation` or `Deactivation` context. They may be synchronous or asynchronous and return `Result<(), LifecycleError>`. Activation runs after committed state is loaded and dependencies are injected, but before business admission. Deactivation is best-effort cleanup, never the place to flush correctness-critical state.

Lifecycle hooks may initialize transient resources and register activation-local work. They do not have a command transaction and cannot mutate managed durable state. Durable initialization and upgrades use fenced initialization/migration transactions. Hook retries do not make external setup effects exactly-once.

Supervision is deployment policy, not a required base class:

```rust
let recovery = Supervision::restart_from_committed_state()
    .budget(RestartBudget::new(5, Duration::from_secs(60)))
    .backoff(Backoff::exponential(
        Duration::from_millis(100),
        Duration::from_secs(10),
    ))
    .on_exhaustion(Exhaustion::Quarantine);
```

Restart budgets and quarantine state live outside the activation and survive relocation. Repeated constructor failures cannot reset their own budget by restarting. Domain rejections do not count as supervisor failures. Temporary transport retries are governed by invocation policy rather than the activation restart counter.

Default handling of a caught panic retires the activation and durably suspends the responsible invocation before allowing further writes. Committed reads may resume after safe recovery, but later mutating turns are blocked until the suspended operation is explicitly retried or resolved, preserving the default durable input order. Applications may configure another ordering contract only explicitly. A suspension is not a success acknowledgement. An abrupt process failure can only recover work it had durably admitted; a transiently queued ordinary call may instead require the client to resubmit its original operation identity.

Known transient storage aborts may retry within a bounded invocation policy. Unknown commits must be reconciled before re-execution. Automatic retries after arbitrary handler panics are opt-in. An administrator can inspect, resume with a compatible deployment, or resolve a suspended operation under an auditable policy. Skipping accepted work requires an explicit terminal disposition.

| Supervised unit | Lifetime | Supported recovery |
| - | - | - |
| Actor activation | One physical owner incarnation | Retire and reconstruct from committed state |
| Owned child scope | Explicit tasks or temporary workers owned by an activation | Cancel/join and restart declared groups |
| Logical virtual actor | Durable identity and state | Quarantine, migrate, retire, repair |
| Service process | OS process or container | Host/orchestrator restart |

Activation context exposes `children()` for explicitly owned tasks and workers. Ownership is registered before work starts. `OneForOne`, `OneForAll`, and `RestForOne` apply only to declared ordered child groups, never to arbitrary actor references that happen to call each other. Child completion carries the activation ownership term so a late completion cannot mutate a replacement activation.

Safe unwind recovery requires `panic = "unwind"` and a runtime boundary that discards the affected activation. `panic = "abort"`, allocator aborts, and unsafe-code memory corruption require process recovery. A deadline cannot preempt a CPU-bound Rust function safely; watchdog escalation is a host responsibility. Cancelling an async handler requires dropping its transaction and retiring any activation whose transient consistency is uncertain.

## Timers and durable reminders

Activation-local timers are created through `Activation::timers()` and deliver private `#[timer]` callbacks through the actor scheduler. They disappear on passivation or crash and cannot mutate managed state without submitting a command. They are appropriate for cache cleanup and volatile polling.

Durable reminders are transaction effects:

```rust
tx.reminders().upsert(
    "expire",
    Schedule::once(expires_at),
    session::reminders::expire(expected_version),
)?;
```

The descriptor names a private `#[reminder]` handler and persistable captures. The handler may receive an injected `ReminderTick` containing the scheduled instant, occurrence identity, and coalesced missed count. The name is scoped to one actor generation. Upsert creates a new reminder generation; stale deliveries from a replaced or cancelled generation cannot execute business logic.

`Schedule::every(period)` requires a missed-tick policy: `Skip`, `Coalesce`, or `CatchUp { max }`. There is no unlimited catch-up default. Delivery is at least once; deduplication and acknowledgement are atomic with the handler commit. A successful handler therefore applies one committed effect per occurrence identity, despite duplicate transport delivery.

Reminders can reactivate a passivated actor. Cancellation is atomic with state changes but cannot undo an occurrence that has already committed. Reminder clocks, storage, scheduler leases, and recovery are runtime-managed; process-local sleeping is not a durable reminder implementation.

## Streams and subscriptions

`tx.publish(event)` stages an event on the emitting actor’s typed event stream. The event type must be persistable and registered in the contract catalog. `tx.publish_to(destination, event)` supports an explicitly authorized named stream destination through its configured adapter. Publication success at the command boundary means the outbox record is durable, not that every subscriber has processed it.

```rust
use futures::TryStreamExt;

let events = counter.control().events::<CounterChanged>().await?;
let mut subscription = events.subscribe("analytics")
    .start(StreamStart::Resume)
    .await?;

while let Some(delivery) = subscription.try_next().await? {
    // Application sink must make duplicate event application harmless.
    sink.apply_once(delivery.id(), delivery.value()).await?;
    delivery.ack().await?;
}
```

`Subscription<E>` implements standard `Stream<Item = Result<Delivery<E>, StreamError>>`. This is a durable actor event protocol, distinct from Cap'n Proto `-> stream` flow-control methods, ByteStream adapters, bulk transfer and replaceable realtime snapshots. Existing streaming readiness and native byte-delivery receipts do not acknowledge subscriber processing or durable cursor advancement. `Delivery` owns its payload and acknowledgement authority. Dropping it without acknowledgement permits redelivery. The subscriber name is bound to an authorized consumer identity; another tenant cannot claim or reset its cursor.

Backpressure bounds prefetched messages and bytes. Stream order is per advertised partition, normally one producer actor, and producer sequence is assigned at commit. There is no implicit total order across actors. `Resume`, `Earliest`, `Latest`, and explicit cursor starts are distinct. A cursor outside retention produces `CursorExpired` unless the application explicitly requests another start position.

A private `#[subscribe]` actor handler can use a runtime-managed subscription. If cursor advancement and actor state share the required atomic provider boundary, they commit together. Otherwise an actor inbox must deduplicate a stable event ID before acknowledging the external broker. The provider must advertise which guarantee it supports; an external broker acknowledgement and unrelated database write are not one transaction.

Implicit subscriptions are deployment bindings from a stream pattern to a declared private handler. They do not manufacture permission from a type name or discover arbitrary consumers globally. Bindings, filter versions, replay policy, and quotas are inspectable configuration.

## Hosting and deployment policy

```rust
ActorHost::builder()
    .vat(config.vat)
    .authority(config.authority)
    .placement_directory(config.placement_directory)
    .storage(config.transaction_store)
    .service(config.http_client)
    .actor::<Counter>()
    .actor::<Document>()
    .actor::<Checkout>()
    .actor::<Cart>()
    .policy::<counter::Entity>(
        ActorPolicy::default()
            .mailbox(QueueLimits::new(512, ByteSize::mib(8)))
            .idle_timeout(Duration::from_secs(300))
            .placement(Placement::balanced().prefer_callers())
            .supervision(recovery),
    )
    .build()?
    .run()
    .await?;
```

This `ActorHost` builder is proposed. `config.vat` is an owned `capntproto::native_rpc::Vat` configured with an actor bootstrap service/factory and authorized connector; it runs on the same `LocalSet` as the actor RPC integration. The existing `Vat::builder(...).connector(...).bootstrap_with(...).start()` configures RPC ownership, not actor scheduling. A host keeps that vat and its required drivers alive. The other `config` values are constructed actor adapters, not connection strings that imply a consistency protocol. Actor policy is keyed by entity kind so interfaces cannot accidentally receive independent mailbox or supervision policies. Runtime dependencies resolve through typed `Inject<T>` handles; missing or ambiguous registrations fail startup.

The placement directory provides actor ownership metadata; the native connector separately discovers authorized peer endpoints. Actor authority grants and validates capabilities; the proposed transaction provider enforces ownership fencing at commit. The existing Store path lock and revision CAS provide local protection, not this distributed ownership protocol. They may share infrastructure but have distinct contracts. Ownership transfer follows the [provider protocol](#ownership-and-transaction-provider-protocol). New commits require the current ownership token; delivery of an older committed outbox record uses its immutable commit proof. Moving authority does not invalidate that prior committed obligation.

A stale placement-directory cache triggers bounded actor lookup refresh and retry. Native `RetryingConnector` retries classified connection setup failures; it does not replay business commands or establish actor ownership. It cannot authorize a write. An actor host losing ownership must cease authoritative reads unless it can obtain a fresh valid read proof. A partitioned actor host may continue computing useless work, but cannot publish it as committed.

Production startup validates required durability, atomicity, fencing, result retention, supported schemas, cryptographic identity, transport limits, and timer/stream capabilities. An adapter cannot silently weaken the method contract. In-memory providers are exposed only through an explicit development/test profile whose actor service negotiation and commit receipts advertise process-only durability. TLS/QUIC authentication does not negotiate or certify storage durability.

Placement applies hard constraints first: tenant isolation, data locality, hardware, and required services. Load and caller affinity are preferences within eligible actor hosts. Migration moves authority and durable work, not arbitrary live futures. Hot single actors remain limited by exclusive execution; scaling them requires application partitioning or a separately declared replication model.

Passivation occurs at quiescent boundaries after transactions are resolved and activation-local children are cancelled/joined. Durable work can later reactivate the actor. Recency/frequency policies and estimated activation memory guide eviction. The activation-memory target is not an exact accounting of all Rust heap allocations; message, frame, and queue bounds remain enforceable.

Graceful shutdown stops new admission, resolves or transfers ownership, drains within a deadline, and preserves accepted durable work. Deactivation callbacks are not required for correctness. Durable operations accepted before shutdown must not disappear because the mailbox was in memory. Actor admission/commit draining precedes native vat shutdown. A successful native shutdown receipt confirms bounded RPC byte delivery, not completion or durability of actor turns; a deadline cannot forcibly terminate a blocked storage sync.

## Wire protocol and foreign language clients

Capnt Actors uses ordinary Cap'n Proto interfaces and typed values on the existing capability RPC engine. Actor lookup/placement, durable operations, and actor-specific restoration metadata form a proposed versioned application protocol above it. They do not replace `rpc.capnp`, native three-party message arms, or the storage record formats. The base RPC question ID is never used as a durable operation ID.

The runtime contract includes these versioned records and services:

| Contract | Required content or operations |
| - | - |
| ActorBootstrap | Use authenticated transport context, negotiate actor protocol/features, grant namespaces |
| Namespace | Resolve actor generation and grant an authorized interface |
| CallContext | Protocol/fingerprint versions, required semantics, call mode, full operation key and request fingerprint for commands, authority binding, original schema version, immutable execution constraints, separate observation budget |
| ResultContext | Mode-specific outcome union: command success/rejection requires commit metadata; nonexecution requires proof; uncertainty retains operation binding; query/request arms contain no commit receipt |
| OperationControl | Inspect authoritative status/read position, retrieve typed recorded result, save/restore observation, authorized pre-start cancellation/tombstones; no implicit submission |
| Ownership / read provider | Term-bound ownership tokens, atomic commit/membership proofs, recovery phases and validated read positions; provider service contract separate from business schemas |
| EffectControl | Producer-generation registration/closure, sequence-window credits, stable effect identity/fingerprint, result acknowledgement, contiguous frontiers and skip/cancel proofs |
| ActorRestorer | Integrate with the realm/Persistent boundary; validate actor/interface generation, grant lineage, relocation binding and current term-bound delegation |
| ContractMetadata | Entity/interfaces/methods, schema and behavior versions, fingerprint artifacts, durability/failure domains, retention and capacity limits, revocation timing, supported ownership/read proofs |

These are proposed actor application schemas. `Actors` in the schematic excerpt names that schema namespace, not the core `capnp_rpc` protocol or an existing file. An authenticated native bootstrap factory supplies the peer-bound actor/realm service; a client-supplied peer ID is never authentication. For example, the generated schema retains a typed business signature:

```capnp
# Schematic excerpt; generated files include stable IDs and imports.
interface Counter {
  current @0 (context :Actors.CallContext)
      -> (reply :CurrentReply);
  increment @1 (context :Actors.CallContext, by :UInt64)
      -> (reply :IncrementReply);
}

struct IncrementReply {
  context @0 :Actors.ResultContext;
  union {
    value @1 :UInt64;
    rejected @2 :CounterError;
    failed @3 :Actors.CommandOutcomeFailure;
    accepted @4 :Actors.OperationToken;
  }
}
```

The wire decoder validates the mode-specific result context before constructing Rust types. A command success/rejection missing a valid receipt is a protocol/observation failure with recovery, never a successful or receipt-less domain result. A wire failure carries operation evidence, while the SDK retains its pre-dispatch recovery handle; an unknown reply cannot erase that handle. `NotSubmitted` is a local SDK outcome and cannot be asserted by a remote server.

`accepted` is returned for the durable-admission call mode; ordinary completion returns value, rejection, or failure. An enqueue retry can report the existing operation rather than enqueue it again. Capability-bearing query results retain typed capability fields so ordinary Cap'n Proto pipelining remains possible. The runtime does not wrap every business result in an opaque byte blob.

Durable operation result retrieval is schema-typed by its contract token and checked by generated clients. A generic control service may carry a typed `AnyPointer` with an explicit schema identity; this does not erase the typed ordinary invocation path. No capability pointers appear in durable payloads without a defined restoration representation.

Feature negotiation is behavioral as well as syntactic. A peer must reject a missing required durability or ownership guarantee before invocation. Ignoring an unknown field is acceptable only if the sender did not require that field’s semantics. Foreign-language clients use the same actor schema and conformance vectors; they do not need Rust’s builder implementation. Current native network support flags such as `supports_third_party_answers()` are local policy, not this proposed actor feature negotiation. General Join, answer adoption and pipeline migration must follow their tested peer-profile limits; their Rust implementation does not establish support in the pinned C++ dispatcher.

Actor traffic uses the maintained TCP and quiche profiles:

| Profile | Current integration | Actor contract |
| --- | --- | --- |
| Authenticated multiparty host traffic | `native_rpc::Vat` over `transport::tcp` with `Backend::Tcp`, or quiche with `Backend::Quiche` / `Backend::QuicheV2` | Pinned Ed25519 mutual TLS 1.3, authorized native routes and supported three-party capability operations |
| Conventional two-party client/service traffic | Owned `rpc::Connection` over `rpc::tls` or `rpc::quic`, with CA/name validation and configured client authentication | Actor capabilities remain application-authorized; this connection alone does not become a native multiparty vat |
| Local explicit test traffic | Plain `rpc::tcp` | Unauthenticated; never silently selected for an authenticated actor deployment |

Quiche is the sole QUIC engine, with explicitly selected v1/v2 (`rpc::QuicVersion::V1` / `V2`). Native ALPN remains `reproto/2`; conventional two-party TLS/QUIC ALPN remains `capntproto-rpc/1`. The actor rename changes neither identifier. Native reservation `psk` parameters mean an optional admission secret bound to the authenticated session, not another transport encryption protocol. Application 0-RTT is disabled, and reconnect does not restore ephemeral capabilities or deduplicate commands.

The shared actor contract uses ordered capability RPC on every selected backend. Unreliable datagrams, path migration and CID rotation are optional quiche transport controls, not portable actor semantics; TCP does not emulate them. QUIC connection/path migration, capability route migration and actor placement/ownership migration are three distinct operations. A transport delivery receipt does not acknowledge a method invocation or an actor commit.

## Generation and schema evolution

Rust-first authoring uses annotated implementations and registered wire DTOs as its signature source. A proposed actor generation stage emits a contract crate, Cap'n Proto schemas, and a checked-in identity manifest, then uses the existing `capntproto-compiler` frontend and maintained `capnpc` generator. Those tools currently compile schemas/generate bindings; they do not implement `Actor` derives, Rust-first actor extraction, or the CLI below. Ordinary compilation verifies those artifacts rather than silently assigning or rewriting public IDs.

```text
cargo capnt-actors schema update
cargo capnt-actors schema check
cargo capnt-actors schema diff
```

The manifest records actor-kind IDs, interface IDs, method ordinals, state-field identities, DTO fields and union tags, event identities, private handler IDs, capture schemas, and compatibility mappings. Source reordering does not change any identity. A rename needs an explicit mapping such as `#[capnt_actors(rename_from = "old_name")]`. Explicit numeric IDs remain supported for interoperability and imported contracts.

Removed identities are never reused. Generated Cap'n Proto schemas retain compatibility placeholders where needed to preserve ordinal continuity; “reserved” does not mean emitting illegal gaps. Every state or wire default is explicit in the normalized contract representation. Unknown union variants use generated `Unknown` representations where safely supported, otherwise a typed compatibility error.

Schema-first authoring imports Cap'n Proto declarations and generates Rust server traits. Rust-first and schema-first paths use one normalized contract model and the same runtime protocol metadata. Each contract has exactly one authoring authority; handwritten schemas and handwritten Rust signatures are not maintained independently as competing sources.

The generator parses an intentionally supported Rust declaration subset before ordinary Cargo compilation. All exported types must be built-ins, registered DTOs, or imported schema types. Aliases must resolve through declared contract metadata. Conditional compilation uses a declared feature set. Arbitrary macro-generated public methods or compiler-inferred types require explicit contract declarations; generation must diagnose them instead of guessing.

Generated methods are inherent methods on types owned by the generated contract crate. The runtime does not ask that crate to add inherent methods to a foreign generic `ActorRef` struct, which Rust forbids. Each interface supplies an associated generated `Client<R>` through the runtime `Interface` trait. `R` is a sealed runtime reference flavor such as `ActorHandle<I>`, `LocalActorHandle<I>`, `LiveHandle<I>`, `SharedHandle<I>`, or `SturdyHandle<I>`.

In the proposed actor API, the public spellings `ActorRef<I>`, `LocalActorRef<I>`, `Cap<I>`, `SharedCap<I>`, and `SturdyRef<I>` are associated-type projections to that generated client with the corresponding handle. `counter::Client` defaults its handle parameter to `ActorHandle<counter::Api>`. A live-only interface defaults to its live handle and cannot be looked up as an actor without a declared entity binding. Method bodies delegate through the handle and existing generated capability clients without unsafe layout casts. Core clients still use their existing `ClientHook` dispatch boundary; this proposal does not claim that all RPC dispatch is statically monomorphized.

This arrangement preserves the same business syntax for actor, live, and restorable references while giving each handle its own `Send`, ownership, and persistence properties. Generic library algorithms should accept the public `CapabilityClient`/`ActorClient` traits and use their associated interface types, avoiding inference through a non-injective associated-type alias. Applications normally use inferred clients or concrete generated client names; no per-interface extension-trait import is required.

This allows bootstrapping a new contract crate without compiling a server that already depends on it. Procedural macros generate local dispatch and validation; they do not create and compile a new peer Cargo crate as a hidden side effect.

Compatibility has several independent dimensions:

| Dimension | Required check |
| - | - |
| Wire decoding | Stable IDs, field types, defaults, unknown variants |
| Method behavior | Query/command/request mode and required feature compatibility |
| Stored state | Versioned migration and minimum compatible writer |
| Event history | Reducer and event migration/upcast compatibility |
| Accepted operations | Preserve original request/fingerprint/schema artifacts and decode recorded results; separately validate any execution migration |
| Continuations | Ability to interpret saved handler IDs and captures |
| Capabilities | Stable interface/grant interpretation, relocation bindings, lineage and revocation timing |

Changing a query into a command is a behavior change even if its wire fields still decode. Pending durable work must be routed to compatible code or explicitly migrated/suspended. Repeated calls replay the stored terminal result; they do not reinterpret a rejection under new business logic.

State migrations execute in fenced maintenance transactions before new-version business turns. The migration either commits with its new version or leaves the previous state available. Large migrations use declared resumable steps with controlled visibility. Older deployments are prevented from becoming writers once they would discard fields or violate invariants; preserving unknown fields alone does not prove behavioral compatibility.

## Packages and application composition

| Package | Responsibility |
| - | - |
| `capnt-actors` | Public actor, call, state, capability, and host types |
| `capnt-actors-macros` | Actor/method/DTO declarations and validation |
| `capnt-actors-codegen` | Actor contract metadata, manifest and Rust-first declarations layered over `capntproto-compiler` and `capnpc` |
| `capnt-actors-protocol` | Versioned actor application schemas above the existing Cap'n Proto/native RPC protocol |
| `capnt-actors-rpc` | Adapter to existing `capntproto` connection/vat and maintained `capnp-rpc` APIs; no second transport engine |
| `capnt-actors-store-*` | Validated persistence implementations |
| `capnt-actors-sim` | Deterministic model runtime and fault injection |
| `capnt-actors-observe` | Tracing, metrics, and inspection integration |
| `capnt-actors-app` | Optional host composition and deployment integrations |

These are proposed logical package boundaries. Start with the requested `capnt-actors` workspace crate and split modules only when needed; this document does not add these packages to Cargo. Existing `capntproto`, `capnp`, `capnp-rpc`, `capnp-futures`, `capntproto-compiler`, `capnpc` and quiche dependencies retain their identities and ownership. There is no requirement to expose every proposed crate to application developers. The default facade reexports the common surface. Contract-only consumers depend on generated contracts plus the client runtime, not the actor host or persistence providers.

An ordinary executable is the primary host. `capnt-actors-app` remains optional. Its initial scope is service configuration, resource references, health/readiness, telemetry, and adapters for existing hosting systems. An Aspire integration can be developed independently of actor semantics. This specification does not commit to a second complete deployment orchestrator.

If a future Rust application graph is added, resource handles own stable IDs rather than borrow the mutable graph builder. Build-time dependency order, runtime health, process restart, actor supervision, and workflow dependencies remain distinct concepts. A readiness dependency must not silently create restart cascades between unrelated services.

## Performance contract

The API is designed to permit efficient implementations; it makes no unmeasured latency or throughput promise.

Generated client and server dispatch should use static types and concrete futures. Dynamic erasure and allocation are allowed at registry, transport, storage-plugin, and heterogeneous mailbox boundaries. Requiring `async_trait` boxing for every business call is unnecessary. An activation owns its state directly; applications do not wrap ordinary actor fields in `Arc<Mutex<_>>`.

Shared and local reference flavors follow the [executor-boundary contract](#executor-local-and-transferable-references). Handler futures may remain executor-local and `!Send`; the runtime need not move an active borrow between worker threads. An explicit `SharedCap` proxy adds a hop, admission and lifetime accounting. The benchmarks must distinguish that path from executor-local capability dispatch.

Local calls may bypass serialization by moving typed owned values, but they must preserve authorization, admission, turn ordering, transaction, deduplication, cancellation, and error behavior. They cannot directly borrow another actor’s live state. Local and wire paths share conformance tests.

Decode into borrowed views where useful and retain ownership through message containers. Copy only when values need independent ownership, representation conversion, persistence, or another executor boundary. Immutable message sharing and zero-copy traversal do not imply zero allocation or zero transport copies.

Budgets cover tenant requests, actor queues, dispatch waiters, frame bytes, reply retention, open capabilities, unresolved pipelines, outbox backlog, recovery concurrency, and compute jobs. A hot or failing actor must not monopolize host recovery or exhaust all queue memory. Fairness policy is configurable and observable; priority never bypasses ordering required by accepted durable work.

Providers may group independent actor transactions behind a shared durability barrier. Each transaction retains its own identity, fence validation and atomic write set; group commit is not an implicit multi-actor transaction. Preserve per-actor accepted-input order and revision dependencies. Publish each result only after that transaction has reached its advertised durability. A failed or uncertain batch must reconcile each transaction individually; it cannot report every member committed or aborted solely from another member's outcome. Bound both batch bytes and maximum wait so throughput does not hide unbounded tail latency.

Read barriers may be shared only among queries already invoked before the confirming authority check, or under a lease/provider proof explicitly valid for later invocations. Each query still selects a pinned consistent view satisfying its own minimum watermark and deadline. An `AtLeast` token cannot be ignored because another query in the batch requested a lower version. Batching cannot turn an earlier cached read index into a fresh linearizability proof.

Prefer state partitions based on update/access patterns, measuring encoded bytes, physical write amplification, synchronization cost and retained-snapshot memory separately. Existing V5 supports at most 256 structural components per object; `Table` needs a real indexed layout, not one component per row. The [current storage experiment](docs/wiki/Storage-Research.md) reduced logical update bytes by about 304× for its two-component workload but improved median commit time only about 1.20×. That local experiment is evidence to measure durability barriers and descriptor overhead, not an actor performance guarantee. Include retained maps/snapshots, compaction, failure recovery and slow consumers in memory/disk measurements.

The benchmark suite must report payload distributions, hardware, transport, storage durability, contention, and failure conditions alongside throughput and latency percentiles. Measure local and remote calls separately; compare borrowed/owned decoding, command commit, durable admission, hot-actor saturation, many idle actors, cold activation, failure recovery, and bounded-memory overload. Do not compare a process-memory command with a replicated durable commit under one undifferentiated throughput number.

## Diagnostics and administration

Tracing links invocation, admission, activation, execution, commit, outbox dispatch, and completion. A continuation creates a later span linked to its origin rather than pretending one live stack lasted through a restart. W3C trace context may propagate as diagnostic metadata; it never grants authority or determines idempotency.

Metrics use bounded labels such as actor kind, outcome class, provider, and tenant class. Actor keys, operation IDs, payloads, and capability tokens are not default metric labels. Targeted inspection can expose actor/operation identities under a separate administrative grant, with token and payload redaction.

```text
cargo capnt-actors inspect actor Counter homepage
cargo capnt-actors inspect operation <operation-id>
cargo capnt-actors schema check
cargo capnt-actors test --seed 42
```

The inspector reports the current ownership term, committed revision, admission/turn/commit latency, queue and reserved-control bytes, pending operations, oldest uncertain attempt, outbox delivery/callback state and acknowledgement gaps, retention deadlines, read-proof age/failures, authority relocation progress, reminder progress, restart budget, and quarantine/suspension reason. It distinguishes process health, actor availability, and business completion.

Administration is a separate capability surface. Inspection, cancellation, suspension resolution, quarantine release, migration, retirement, and recreation have separate permissions and audit events. Business interfaces never acquire these powers merely by being implemented in the same Rust struct.

## Verification requirements

The proposed actor simulator injects clocks, ID generation, randomness, storage, transport, scheduling, and faults. Existing tests use bounded TLA+/Rust replay, targeted simulations and real transport/storage regressions. Native TLS handshakes retain fresh cryptographic entropy; packet scheduling control does not make their encrypted bytes deterministic. An attribute cannot make arbitrary Tokio code or external I/O deterministic. Tests using uncontrolled dependencies must identify that limitation. Simulation complements real provider and network tests; it is not a formal proof.

```rust
#[capnt_actors::simulation(seed = 42)]
async fn retry_after_lost_reply(sim: &mut Simulation) -> TestResult {
    sim.actor::<Counter>();
    let client = sim.client().await?;
    let ns = client.namespace("test").await?;
    let counter = ns.actor::<counter::Api>("visits").await?;
    sim.fault_once(Fault::DropReplyAfterCommit);
    let first = counter.increment(1)
        .retry(Retry::never())
        .timeout(Duration::from_secs(1))
        .await;
    let recovery = match first {
        Err(CommandFailure::Tracked {
            outcome: CommandOutcomeError::Uncertain { .. },
            recovery,
        }) => recovery,
        _ => panic!("lost reply must retain uncertain-operation recovery"),
    };

    sim.crash_owner(&counter).await?;
    let result = recovery.retry()?.await?;
    assert_eq!(result, 1);
    assert_eq!(counter.current().await?, 1);
    Ok(())
}
```

This is a proposed test surface, not a test that has been executed. `DropReplyAfterCommit` with retries disabled must cause that call’s observation to fail while retaining its committed result. A simulator implementation must make the fault semantics precise enough for this assertion. This case uses an automatic operation identity and recovers it from the failed call, rather than reconstructing an invocation with an independently remembered key.

Required acceptance cases include:

1. Reader and editor interfaces resolve to one entity and see the same committed state.

2. A revoked or weaker capability cannot invoke stronger methods, including through local fast paths and handoff.

3. Borrowed request data cannot escape its message owner; live capabilities fail persistable-boundary compile checks.

4. A lost reply followed by client retry and owner relocation applies one committed mutation and replays its result.

5. Expired retry scopes cannot execute again after deduplication records are pruned.

6. A stale owner cannot commit state, results, reminders, or outgoing effects, and cannot serve authoritative reads without proof.

7. No success or staged outgoing effect becomes externally visible before commit.

8. Domain rejection discards staged business state and effects while durably recording the rejection.

9. Timeout/response, cancel/start, reminder replacement/delivery, and callback supersession races have one valid outcome.

10. Panics, lifecycle failures, poison operations, and recovery storms respect persisted budgets and bounded memory.

11. Schema renames, retirement, older writers, accepted old-version work, and continuation capture upgrades preserve their declared compatibility.

12. A non-Rust client and a Rust client observe the same operation, error, capability, and version semantics.

The following adversarial cases are also release gates, not optional examples:

| Area | Required scenario and invariant |
| - | - |
| Ownership | Pause the old owner before commit, transfer, then resume it; every stale state/result/outbox/admission write is rejected atomically |
| Transfer recovery | Crash source, coordinator and target at each phase, including after an unknown commit; one new owner reconciles the original transaction and obligations |
| Read proof | Partition an old leader, expire a lease, suspend a process past its clock bound and restart with a cached read index; authoritative reads wait/fail rather than return stale state |
| Batched reads | Invoke a query after a shared barrier, then request a higher `AtLeast` watermark; neither may incorrectly reuse an insufficient proof/view |
| Automatic identity | Lose the first admission or completion reply without an explicit user key; every post-dispatch failure retains an authorized recovery handle |
| Attempt versus operation | Lose a first reply, then reject a retry before start; the SDK cannot overwrite unresolved uncertainty with nonexecution |
| Cancellation | Cancel an unseen operation, lose the cancel reply, and deliver the original submission later; a confirmed tombstone prevents execution |
| Observation | Revoke an observation token, expire a retry scope and inject a result decode failure; business outcome stays independent and no new operation is submitted |
| Retention | Finish an accepted command after scope expiry; preserve its full result for the advertised post-completion interval and reject old execution after pruning |
| Fingerprints | Publish cross-language hex vectors for equivalent allocation layouts, segment boundaries, defaults/null pointers, unknown fields, signed zero/NaNs, maps/list order, text bytes, tokens and immutable options |
| Fingerprint upgrades | Replay original bytes across schema/list-layout evolution and actor movement; preserve original hash, reject changed immutable semantics and never re-encode under a newer schema |
| Restoration | Move A to B with a token sealed to A, crash between binding steps, and rotate a shared host owner in a negative test; only authorized scoped transfer succeeds and unrelated actors retain access |
| Live authority | Invoke a pre-transfer live capability and saved descendant through a proxy/third-party route; enforce the declared lineage and term policy independently of realm token rotation |
| Revocation timing | Revoke before admission, while queued, after start and after commit; outcomes match the negotiated policy and observation rights remain separate |
| Old-term outbox | Commit under term 7, move to term 8 and dispatch; the valid old commit proof authorizes delivery, while an uncommitted term-7 effect is rejected |
| Outbox recovery | Lose dispatch, response and acknowledgement replies; redeliver one identity, retain callback input, and never advance a contiguous frontier across a gap |
| Timeout and retirement | Time out a callback before the child commits, then retire producer/target; preserve uncertain/accepted work, select one callback and prevent recreation from receiving the old effect |
| Durable quotas | Exhaust producer/recipient result and outbox budgets; new admission stops while existing completion, acknowledgement and reclamation can progress |
| Control isolation | Fill mailboxes/output queues and block a handler across `await`; bounded status/cancel/recovery/shutdown remain independent of that turn on TCP and quiche |
| Control abuse | Flood control requests and mix Finish/Release/Return with business work; enforce budgets and E-order without dropping required protocol progress or claiming unlimited liveness |
| Group commit | Crash before/after the shared durability barrier and inject one member's fence conflict; reconcile each transaction without publishing premature or fabricated outcomes |
| Rust boundaries | Reject shared calls with local capability payloads/borrowed views; accept explicit bounded proxies with unchanged authority; skip destructors and preserve durable correctness |

Compile-fail suites cover invalid method signatures, unknown actor fields, unregistered wire types, nonpersistable durable captures/results, receipt-less command rejections, query errors containing command receipts, shared calls containing nontransferable payloads, and public access to private handlers. Runtime tests separately exercise hidden helper mutations and side effects that the type system cannot prove absent. Model checking targets ownership transfer, commit/output ordering, cancellation races, deduplication expiry, and reminder generations. Extend the existing generated-API compiler contracts, bounded TLA+/Rust replay, property/fuzz checks, isolated process-crash tests and Miri where applicable. Add bounded concurrency exploration for new synchronization; no tool substitutes for an explicit invariant. Run real TCP/TLS/mTLS and quiche v1/v2 coverage, with native three-party introductions, Join, answer adoption, late parent errors and shutdown. The [testing guide](docs/wiki/Testing.md) names current gates; actor deduplication, fencing, relocation and recovery need new acceptance evidence rather than inheriting those pass results.

## Core profile and extension boundaries

These are targets for a future conforming actor implementation, not a claim that the present workspace meets the profile. The first conforming core includes typed contract generation, explicit interfaces, exclusive turns, `State<T>`, fenced ownership, authoritative queries, transactional results and outbox, durable admission and operation recovery, bounded queues, capability restoration, supervision, and the verification cases relevant to those features.

Tables, logs, event state, durable continuations, reminders, and streams are specified modules enabled only when providers advertise their required capabilities. Their APIs must not silently degrade to volatile behavior. Immutable projections require their separate read contract.

The following remain separate extensions, not implied core guarantees:

| Extension | Required additional design |
| - | - |
| Multi-actor transactions | Isolation, participant discovery, coordinator recovery, atomic visibility |
| CRDT replicas | Merge laws, causal metadata, explicit eventual-consistency API |
| Durable imperative workflows | Persisted steps, recorded nondeterminism, versioned replay and upgrades |
| Concurrent snapshot handlers | Snapshot construction, publication, routing, and freshness proofs |
| Speculative execution | Effect containment and validated dependencies before commit |
| Durable live-capability results | Authority-preserving reconstitution and replay identity |

The stable ergonomic rule is progressive disclosure: a small actor needs `Actor`, `State`, `query`, and `command`; explicit transactions appear when staging effects; named interfaces appear when separating authority; operation tokens appear when clients need recovery across their own restarts. Additional guarantees require explicit contracts rather than changing the meaning of the simple API.

### Implementation sequence and acceptance boundaries

Implementation should proceed in this order; these are milestones, not permission to advertise the full core before its guarantees exist:

1. **Operation protocol and recovery.** Freeze mode-specific error/status schemas, request fingerprint artifacts and vectors, automatic/prepared recovery handles, cancellation tombstones and retention rules. Test lost first replies and cross-language replay before convenience code generation expands.
2. **Ownership, commit and read proofs.** Implement a provider adapter with atomic fencing and reconciliation; validate its declared durability profile, transfer failures and authoritative read barriers. Qualify multi-host guarantees separately from a single-host durable prototype.
3. **Restoration and delegation across movement.** Implement the chosen stable service or scoped rebinding protocol, live grant checks, bounded authority storage and interrupted-transfer recovery. Owner relocation cannot ship while persisted capabilities only restore on the departed host.
4. **Minimal durable actor runtime.** Deliver `State`, exclusive turns, generated local/shared calls, authoritative queries, command completion, durable admission, outbox recovery and reserved control progress. Measure group commit and granular storage behind the same conformance suite. Label earlier prototypes experimental with their actual provider profile.
5. **Optional modules and placement optimization.** Add durable continuations, reminders, streams, tables/event state and richer placement only as their adapters pass retention, backpressure, upgrade and fault gates. Load balancing must not precede the ownership and authority handoff it relies on.

Protocol and provider interfaces may be developed with simulators before the actor facade exists. A milestone report must identify implemented guarantees, unsupported negotiated features and evidence from real crash/network tests. It must not describe a passed simulation, current RPC interoperability, or a local Store CAS as proof of the entire actor profile.

## Design basis

The APIs and guarantees above are proposed choices. The [current architecture](docs/wiki/Architecture.md), [RPC ergonomics](docs/wiki/RPC-Applications.md), [component storage](docs/wiki/Component-Storage.md), [storage resilience](docs/wiki/Storage-Resilience.md) and [persistence realm](docs/wiki/Persistence.md) establish the local integration contract. The external sources below inform particular design decisions; this design does not inherit an existing framework’s implementation, correctness proof, or benchmark results.

| Source | Influence on this specification |
| - | - |
| [Orleans request scheduling](https://learn.microsoft.com/en-us/dotnet/orleans/grains/request-scheduling) | Separate non-reentrant execution from explicitly interleaved scheduling; adapt the former to Rust ownership |
| [Cap'n Proto RPC](https://capnproto.org/rpc.html) | Capability references, promise pipelining, disconnection, and explicit persistent restoration |
| [Cap'n Proto schema language](https://capnproto.org/language.html#evolving-your-protocol) and [canonicalization](https://capnproto.org/encoding.html#canonicalization) | Stable wire identities and layout evolution; freeze original canonical request bytes rather than assuming wire compatibility implies identical fingerprints |
| [Chubby paper](https://www.usenix.org/legacy/event/osdi06/tech/full_papers/burrows/burrows.pdf) | Reject stale owners with receiver-enforced fencing, rather than trusting a process to stop after losing a lease |
| [Raft paper](https://raft.github.io/raft.pdf) | Current authority, confirmed committed positions and timing assumptions for linearizable reads; an example provider design, not a mandated backend |
| [Tokio bounded sender](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.Sender.html) and [LocalSet](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html) | Bounded admission, reserved capacity and executor-local `!Send` state |
| [Rust destructor omission](https://doc.rust-lang.org/std/mem/fn.forget.html) | Do not rely on `Drop` or a must-use lint for durable correctness |
| [Rust implementation rules](https://doc.rust-lang.org/reference/items/implementations.html) and [associated types](https://doc.rust-lang.org/reference/items/associated-items.html) | Generated local client types and associated-type reference aliases that respect Rust coherence |
| [Erlang supervision principles](https://www.erlang.org/doc/system/sup_princ.html) | Explicit child ownership, restart strategies, and restart intensity limits |
| [Pekko fault tolerance](https://pekko.apache.org/docs/pekko/current/typed/fault-tolerance.html) | Separate expected domain outcomes from supervised failures |
| [RIFL paper](https://web.stanford.edu/~ouster/cgi-bin/papers/rifl.pdf) | Durable completion records, retry rendezvous after movement, and safe reclamation of deduplication metadata |
| [Histrio paper](https://arxiv.org/abs/2410.21793) | Queryable granular state and transactional actor effects |
| [SmSa paper](https://arxiv.org/abs/2410.15831) | Fine-grained state as a useful boundary; dependency transactions require additional machinery |
| [KAR implementation](https://github.com/ibm/kar) | Explicit durable coordination and the importance of retry ordering beyond a simple outbox |

Capnt Actors combines concise Rust authoring, schema manifests, transactional actor effects, transient-state reset, explicit supervision, standard streams, and ordinary executable hosting. Its integration keeps entity kinds distinct from interfaces, actor ownership distinct from native route ownership, restoration tokens distinct from live capabilities, and durable actor acceptance distinct from ordinary RPC admission or transport receipts.

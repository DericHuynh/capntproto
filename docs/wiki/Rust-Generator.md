# Rust reader and field-operation generator

The opt-in generator implements the reader/editor facade and core ownership contracts of
[the API design](../archive/Rust-API-Design.md) on the vendored `capnpc` and
`capnp` runtime. It emits an `api` module alongside the established bindings;
existing reflection, RPC dispatch, wire layout, and default generator output
remain available. Generated RPC clients also expose `method_call()` constructors
with field-operation request editors. This is an experimental API, not completion of every runtime
and optional convenience feature proposed in the design.

## Enable it

Use `capnp` and build dependency `capnpc` from the same source bundle, with
explicit path dependencies. The [external example](../../examples/downstream)
and [preview guide](Getting-Started.md) show the tested dependency recipe; root Cargo
patches are unnecessary.

```rust
// build.rs
capnpc::CompilerCommand::new()
    .src_prefix("schemas")
    .file("schemas/person.capnp")
    .field_api(true)
    .run()
    .expect("generate schema");
```

`CodeGenerationCommand::field_api(true)` also accepts an existing compiler
request. The `capnpc-rust` plugin accepts `--field-api` when called directly
with a binary `CodeGeneratorRequest` on stdin. `--help` describes its arguments.
Generate all referenced schema files with the same option: imported struct and
enum references target their corresponding `api` modules. The shared facade
runtime requires the `capnp` `alloc` feature (included in its defaults).

Enums nested in generic scopes remain ordinary enum types in both generated
APIs. Their native reflection and identity erase enclosing parameters, matching
C++; generated annotation callbacks use default `AnyPointer` arguments. The
runtime schema loader separately retains enum wire brands and rejects
assignments between different loaded brands. [Tests and bounds](Cpp-Parity.md)
cover nested scopes, annotations, lists and unknown enum numbers.

```rust
use capnp::field_api::{Message, MessageView};
use crate::person_capnp::api::Person; // Your generated person.capnp module.

let mut message = Message::<Person>::new()?;
{
    let mut person = message.edit();
    person.id().set(42);
    person.name().copy_from("Deric")?;
    person.address().ensure()?.city().copy_from("Edmonton")?;
    person.phones().init_with(2, |i, mut phone| {
        phone.number().copy_from(["555-0100", "555-0101"][i])
    })?;
    person.employment().school().copy_from("NAIT")?;
}
assert_eq!(message.read().name()?, "Deric");
let frozen = message.freeze();
let bytes = frozen.to_vec();
let view = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
assert_eq!(view.read().address()?.city()?, "Edmonton");
```

The executable version, including imports and assertions, is in
[`tests/field_api.rs`](../../tests/field_api.rs). Legacy generated readers/builders
have consuming `into_api()` conversions, including RPC method parameter and
result structs. Existing clients, servers, request dispatch, and capability
hooks are reused.

The independent `.structured_replies(true)` option changes generated server
result aliases to consuming reply stages. It can be combined with the field API;
see [structured replies](RPC-Applications.md#structured-server-replies) for the
state transitions, exact result types and publication contract. Client calls in
both APIs support `with_params(...)`, direct awaiting and `into_parts()`.

## Schema documentation

Both standard bindings and the field API render `CodeGeneratorRequest.sourceInfo`
as Rustdoc. Node IDs and schema member indices select the comments, so `$Rust.name`
renaming and source declaration order do not change their attachment. Types,
groups, field readers/builders, setters/initializers, presence checks, pipelines,
union/enum variants, constants, annotations and RPC client/server methods receive
their schema comments. Field descriptors, consuming editors, projections and
native values use the same indexed documentation.

File comments have a public `schema_documentation` module, with a numeric suffix
when a schema declaration already uses that name. This gives file documentation
a Rustdoc page while keeping generated code compatible with `include!`. Comments
are escaped string attributes; quotes, carriage returns and comment delimiters
cannot become Rust source. Missing source-info or member entries are accepted;
duplicate node metadata and invalid UTF-8 return generator errors.

Schema comments are language-neutral Markdown. Untagged fenced and indented
examples are rendered as `text` blocks, and explicit Rust fences remain doctests.
CommonMark parsing and serialization preserve nested lists/quotes and select
fences long enough for embedded backticks. Bare HTTP(S) URLs in prose become
links; existing links, inline code and code blocks keep their roles. The parser
and serializer are build-time dependencies of the generator.

The [fixture](../../crates/capnp-compiler/examples/rustdoc.capnp) and
[HTML/doctest integration test](../../tests/schema_compiler/rustdoc.rs) exercise both
compiler inputs, both generated APIs, imports, streaming, renaming, ordinal order,
partial metadata and comment escaping. The test generates actual Rustdoc with
warnings denied and verifies comments on the corresponding HTML items.

## Implemented contracts

* `Person<M = mode::Schema>`, `PersonRef<'a>`, and `PersonMut<'a>` use sealed
  modes. Readers are copyable; editors and consuming field handles are not.
  Generic schemas place the mode parameter after their schema parameters.
* Readers return scalars, open enums (`Unknown(u16)`), validated `&str`, byte
  slices, struct readers, lists, and existing capability clients. Pointer
  failures stay errors. Explicit defaults and `$Rust.option` are respected.
* Editors expose lazy field handles. Scalar `set`, text/data `copy_from`,
  struct/list `edit`, `ensure`, `init`, `replace`, `clear`, and strict recursive
  `copy_from` share the runtime. `edit` refuses missing or undersized storage;
  `ensure` can materialize defaults or upgrade. `init` requires a null slot.
* Pointer descriptors such as `Person::NAME` support `is_null`, `present`, and
  deferred UTF-8 validation through `wire_text`. Descriptors retain schema
  names and explicit field ordinals and cannot be used with another schema.
* Lists use checked `usize` lengths and indices, fallible pointer iteration,
  lending mutable element access, and higher-ranked callbacks. `init_with`
  and `replace_with` publish only after every callback succeeds.
  `try_for_each_mut` applies sequential edits without rollback. Native slices
  are available only when element representation, stride, alignment, and
  endianness permit them.
* Named unions offer flattened decoded views and tag-only reads. Acquiring an
  arm does not select it. Pointer copies stage before committing the tag and
  clearing known overlapping storage. Unknown tags remain `Unknown(u16)`.
  Group arms return `GroupField`: `edit` requires selection; `ensure` selects
  a default group when necessary; `replace` resets its known union storage.
  `replace_with` constructs a whole group before selecting/publishing it.
* `Message` establishes a typed root, supports custom allocators, preserves
  local capability contexts, and freezes by consuming the mutable owner.
  `MessageView` checks frame/root acquisition and borrows the input bytes;
  nested validation remains lazy and shares a traversal budget. Exact and
  prefix framing are separate constructors. `compact_copy` copies reachable
  storage and capabilities into a new owner.

Staged pointer operations build in the destination arena, clear failed
candidates on errors or unwinding, and transfer the pointer on success.
Compatible text/data storage is reused after preflight. Failed staging can
consume arena space; it preserves the destination's logical value, not its
allocation history. The existing allocator cannot report recoverable allocation
failure. Freezing does not deeply validate payloads. Serialized capability
pointers still require an RPC capability table; byte serialization alone cannot
persist local client hooks.

## Cached occupied entries

`field.entry()?` distinguishes absent and occupied storage under an exclusive
parent borrow. The occupied handle retains the editor acquired during inspection
for built-in text, data, struct/list, generic/AnyPointer and capability kinds.
Its consuming `edit()` or `ensure()` returns that editor without acquiring the
pointer again or allocating arena storage when the existing layout is sufficient.
Capabilities retain their acquired hook until consumption or drop. Entries and
child editors remain move-only; the parent cannot be mutated through another
borrow while either is live. Pointer-list elements use the same cache.

Inspection does not materialize defaults, select a different union arm, or grow
an old layout. An undersized struct or struct list retains its pointer slot and
cached `NeedsUpgrade` result: `edit()` returns that error without another arena
access, while `ensure()` uses the existing allocating upgrade path. Primitive
lists interpreted as struct lists also require this explicit upgrade. A readable
external data pointer remains occupied but caches `ReadOnlySegment` when a
writable view cannot be acquired; consuming it does not copy the bytes.
Malformed pointers and invalid native text still fail inspection. Generic
leaves retain their existing reader/builder validation contracts.

`PointerType::acquire` is required for custom pointer kinds. Construct an entry
with `OccupiedField::from_acquired` or `requiring_upgrade` after validating its
readable value. The unreleased read-then-edit fallback has been removed, so every
kind explicitly participates in the acquisition contract.

`RpcFieldEntry.tla` checks vacant, sufficient, old, and malformed layouts;
`RpcFieldEntryCaps.tla` checks the acquired hook, consuming it, clearing the
source and dropping the returned client. Together they explore **44 states and
42 edges**, replayed in **84 Rust cases** using default and fixed one-word
segments. Replays measure actual arena accesses/allocations and capability
acquisitions, verify preserved values/identity, and observe destruction. Seven
fault mutations detect eager/implicit upgrades, repeated acquisition, lost
values, leaked entry hooks and lost returned clients. Separate regressions cover
readonly external data, generic upgrades, pointer-list elements, inactive unions,
UTF-8 and downstream custom kinds. Compiler acceptance includes cached entries
outliving their parent, overlapping parent/child edits, cloning and double use.

These are bounded behavioral and operation-count checks, not a wall-clock speed
benchmark or a proof for every schema or custom `PointerType`. Run
`cargo test --test protocol_models field_entry_model -- --exact` and
`cargo test --test tooling generated_api_compile_contracts -- --exact`, or the canonical runtime checker.

## Remaining design work

The following parts of the design remain:

* Native generic/AnyPointer payloads are owned opaque wire leaves;
  native conversion of these leaves, derives, Serde and logical comparison
  remain optional follow-up work.
* Union decoded enums have a hidden phantom variant for otherwise-unused
  generic parameters/lifetimes. Exhaustive matches should include a fallback.
* Field names that collide with reserved facade methods are rejected with a
  `$Rust.name` diagnostic.

Whole-message infallible validation was explicitly deferred in the design and
is not claimed here. The existing allocator does not report recoverable
allocation failure, so adoption cannot be tested against a returned allocation
error without a separate allocator-contract change. Global OOM/panics are not
covered by the returned-error atomicity guarantee.

## Ownership, staging, and RPC additions

Union-group `replace_with` lends a default-valued detached group to a scoped
callback. Only a successful return replaces the previous arm, even when that
arm is already the same group:

```rust
use capnp::field_api::Message;
use crate::person_capnp::api::Choice; // Generated schema containing Choice.

let mut message = Message::<Choice>::new()?;
message.edit().details().replace_with(|mut group| {
    group.count().set(7);
    group.label().copy_from("complete group")?;
    group.nested().value().copy_from("nested value")
})?;
```

Generated `GroupSchema` metadata merges overlapping data-bit masks and pointer
slots across nested groups/unions. Publication preserves siblings sharing the
same data word, moves descendant pointers without recopying them and releases
replaced capabilities. Callback errors and panic unwinding discard the candidate
without changing the parent. Independently retained clients remain usable.
Readers/editors cannot escape the callback, and the parent remains exclusively
borrowed. Callback side effects and arena allocation history are not rolled
back; allocator failure and panicking capability destructors are outside this
guarantee. The callback's success does not establish deep schema validation.

`RpcGroupStaging.tla` explores 192 states and 339 transitions, with every
shortest edge prefix replayed against the generated Rust API: 1,356 cases
cover normal/one-word segments and replacement of either a capability arm or
an already selected group. Six injected faults violate publication, ownership,
sibling-isolation or payload-address invariants. In-callback observations check
candidate values and capability lifetimes; attached values are inspected after
callback return. Separate regressions cover packed bits, defaults, generic
payloads, nested staged callbacks and zero-pointer groups. Compile checks reject
escaped callback readers/editors and parent reborrowing; the ordinary regressions
also run under Valgrind. These are bounded component checks.

`Message::edit_with_orphans()` separates the root borrow from the lifetime
of a detached object. `take` checks the supplied orphanage, pointer shape, and
capability references before detaching. It traverses unknown fields and nested
lists too. `adopt` checks both arena and capability-table identity before
changing the destination. Failure returns `AdoptError { error, orphan }`.

```rust
let (mut person, orphanage) = message.edit_with_orphans();
if let Some(address) = person.address().take(&orphanage)? {
    person.previous_address().adopt(address).map_err(|e| e.error)?;
}
```

Orphans move payload pointers without copying payload bytes. Capability hooks
move out of their table slots while detached, and return to those exact slots
on adoption. Dropping the orphan releases its hooks immediately, including
unknown nested capabilities; retained clients remain independently usable.
Detached allocation bytes remain arena-owned until message destruction or
compaction. Dropping an orphan does not promise secure erasure. Far-pointer
landing pads can still allocate. Orphans have no standalone serialization or
mutable-view API.

`scoped_edit` mints a fresh invariant brand using a higher-ranked callback.
Its root uses typed descriptors for branded movement:

```rust
message.scoped_edit(|session| {
    let (mut root, orphanage) = session.into_parts();
    if let Some(address) = root.field(Person::ADDRESS).take(&orphanage)? {
        root.field(Person::PREVIOUS_ADDRESS).adopt(address).map_err(|e| e.error)?;
    }
    Ok::<(), capnp::Error>(())
})?;
```

`root.edit()` lends the ordinary generated editor for other operations.
Branded fields retain union selection metadata. The compiler rejects adoption
from a nested independent session; runtime identity checks remain in place.

`Pending<K, Draft>` exclusively borrows its destination without changing it.
Only `Pending<K, Ready>` has `commit`. Data and text support `write_chunk`,
`remaining`, `finish`, and `read_exact_from`; an incomplete fill or invalid
UTF-8 cannot become Ready. `read_exact_from` fills the remaining suffix after
any chunks. Overflowing a chunk preserves its candidate and progress.

```rust
let mut person = message.edit();
let mut staged = person.payload().stage_replace(4)?;
staged.write_chunk(b"ab")?;
staged.read_exact_from(&mut &b"cd"[..])?.commit()?;
```

Struct/list drafts support higher-ranked `fill_with` callbacks. Their Ready
state establishes successful construction and root shape, not deep schema
validation. Data/text/struct/list construction helpers share this publication
lifecycle. Dropping or forgetting Draft/Ready never publishes; all storage is
zero initialized. `previous()` inspects the still-published value during a
replacement. Callback side effects, consumed input, and allocation history are
not rolled back.

Generated non-group editors expose `copy_from` for fixed struct storage.
Copies check actual source data/pointer sections, including unknown fields,
and return `WouldTruncate` before mutation when the destination is too small.
Recursive copying stages before publication, so malformed descendants leave
old contents intact. Whole-list replacement preserves the source stride;
`edit` refuses an undersized old stride, while `ensure` upgrades it explicitly.
Generic pointer `edit` now preflights compiled layout metadata and works with
the generated and built-in `Owned` implementations without arena growth.

RPC clients expose `echo_call().edit()` and `send()` returns an awaitable
`PendingCall`. Its `pipeline` is available before awaiting, and `into_parts()`
separates pipeline ownership from the response future. Responses own their
hooks and lend result views through `read()`. `send_for_pipeline` and streaming
calls preserve the existing backend's hints, cancellation and flow control.
A streaming send becoming ready does not assert application completion.
These wrappers retain local capability semantics and do not require `Send`.

`RpcFieldOrphans.tla` and `RpcFieldStaging.tla` explore 48/15 states and 86/16
edges. All 204 shortest-edge-prefix replays check the actual field API with
normal and one-word segments. Six mutation checks reject wrong-arena adoption,
lost ownership on error/drop, premature readiness/publication, and failure
which destroys the prior value. These bounded models do not prove arbitrary
pointer graphs, allocator-panic safety, or whole-RPC refinement. Local and wire
RPC tests separately exercise the generated calls and pipelines. There are no
benchmark-based performance claims.

## Validate

```sh
cargo test --test field_api --test field_api_ownership --test field_api_rpc
cargo test --test field_group_staging
cargo test --test protocol_models field_ownership_model -- --exact
cargo test --test protocol_models field_group_staging_model -- --exact
cargo test --test tooling generated_api_compile_contracts -- --exact
cargo test --workspace --all-targets
cargo test --manifest-path vendor/capnp/Cargo.toml --all-targets
cargo clippy --workspace --all-targets --no-deps -- -D warnings
```

The compiler checks include a positive control and reject frozen mutation,
aliased editors, consumed handles, occupied initialization, cloned editors,
wrong-schema descriptors, escaping callback elements, readers outliving their
view owner, overlapping list element editors, premature commit, reused orphans, expired
message storage, escaping draft editors, and cross-session brands. Logs and source hashes go to
`reports/reproto/field-api/`. Runtime tests exercise defaults, invalid text and
pointers, atomic failure, schema upgrades, unknown discriminants, capability
identity/release, traversal limits, and segmented arenas.

## Optional native values and borrowed projections

Enable `.field_api_values(true)` and/or `.field_api_projections(true)` on either
compiler command. Each option also enables the field API; neither changes the
compiled reader/builder generator's default output. Direct plugin equivalents are
`--field-api-values` and `--field-api-projections`. Generate imported schemas
with matching options.

`PersonRef::project()` returns a `Result<PersonView<'_>>` containing public
fields. Scalars and immediate pointer headers are decoded once; text/data
remain borrowed. Nested structs and lists retain their usual lazy, fallible
access. Named unions contain their selected decoded arm; anonymous unions use
`__union`. Unknown tags remain visible as `Unknown(u16)` without decoding an
unknown payload. This is a projection, not a Rust layout cast or whole-message
validation. The compiler prevents it from outliving its message/view owner.

```rust
let projection = message.read().project()?;
assert_eq!(projection.name, message.read().name()?);

use capnp::field_api::native::{Limits, UnknownFields};
let mut value = message.read().to_value(UnknownFields::Discard, Limits::default())?;
value.name = Some("owned text".into());
let independent_message = value.to_message(Limits::default())?;
// Or construct an initially default-valued PersonValue:
let mut fresh = PersonValue::new()?;
fresh.name = Some("created natively".into());
```

`PersonValue` contains native scalars, open enums, strings, byte vectors, vectors
of native elements, boxed native structs, group values and capability clients.
Pointer fields and pointer list elements use `Option`, irrespective of the
reader's `$Rust.option` annotation. `None` records physical absence: it does not
materialize a schema default. This preserves default semantics on encoding and
terminates null recursive structures. Empty and absent pointers remain distinct.
Known union arms have a `PersonUnionValue`-style enum; an unknown union tag
returns an error because a native tag alone cannot preserve the arm's payload.

Conversion requires an explicit `UnknownFields::Discard` argument. Unknown
struct fields (including future fields that share existing padding) are not
represented. This is an opt-in loss policy, not a claim that all unknown fields
can be detected. Use wire copying to retain them. Unknown enum numbers survive
native conversion. Values can outlive the source and be edited independently.
Encoding creates a fresh message; an error returns no partial output and
releases any capability references acquired during the failed conversion.

Generic parameters and `AnyPointer` fields use `OpaqueValue<T>`: an independently
copied wire object with its own capability table and a fallible `read()` method.
Their unknown fields and nested capabilities survive, but their contents are
not eagerly decoded into native Rust fields. This is a deliberate boundary of
the current native conversion, not full native generic specialization.

`Limits` bounds decoded items, bytes and recursive native depth in both
conversion directions. List counts include zero-size elements before allocating
vectors. Opaque leaves charge their wire size and capability count, while their
wire traversal/nesting validation uses the source reader's existing limits.
The byte quota is a payload accounting limit, not an exact allocator/RSS bound;
allocator OOM remains outside the returned-error contract.

`tests/field_api_values.rs` covers recursive schemas, defaults, null list slots,
groups, unknown enums/fields/tags, malformed text, opaque nested capabilities,
budget failures and independent ownership. `RpcNativeValues.tla` checks the
bounded snapshot/mutation/encode/error/drop lifecycle: 301 states and 524 edges,
replayed as 1,048 Rust traces across default and minimal source segments. Three
mutants demonstrate detection of lost capability ownership, publication on
failure and source mutations changing a snapshot. These checks do not prove
arbitrary schema conversion or composed RPC protocol refinement.

## Custom message owners

`Message<S, State, A, C>` carries an allocator and a capability-context owner.
`C` implements `Borrow<CapabilityTable>` for reads and `BorrowMut<CapabilityTable>`
for editing. The default is a private table of owned hooks. `Box<CapabilityTable>`,
exclusive table borrows and application wrappers can supply mutable contexts;
immutable readers also accept shared tables such as `Rc<CapabilityTable>`.
`CapabilityTable` is a public alias for the existing hook table, not a wire format.

```rust
use capnp::field_api::{CapabilityTable, Message, MessageReader};
use crate::person_capnp::api::Person;

let mut capabilities = CapabilityTable::new();
let mut message = Message::<Person>::with_capabilities(&mut capabilities)?;
message.edit().name().copy_from("owned context")?;
let frozen = message.freeze();
let reader = frozen.into_reader(Default::default())?;
assert_eq!(reader.read().name()?, "owned context");
let (reader, context) = reader.into_parts();
let restored = MessageReader::<Person, _, _>::from_reader_with_capabilities(reader, context)?;
assert_eq!(restored.read().name()?, "owned context");
```

`MessageReader<S, Segments, C>` owns any existing `ReaderSegments` implementation,
including application providers, borrowed segment arrays and message builders.
`from_segments` creates a reader with explicit limits;
`from_segments_with_capabilities` additionally accepts the corresponding table.
`from_reader` / `from_reader_with_capabilities` adopt an existing reader and keep
its already-consumed traversal budget. Acquisition checks the root once; repeated
`read()` calls share that cached root and subsequent pointer reads share one budget.
Extracting and reattaching a reader charges root acquisition again without resetting
the budget. Creating a new reader from raw segments is an explicit new budget boundary.

The reader's private `Rc` allocation stabilizes both the arena and segment providers
with inline storage before caching root metadata. The cache holds raw pointers,
not a lifetime-extended reference into its owner. Each view borrows the live arena;
consuming the owner recovers the reader through `Rc::try_unwrap`. The capability table is borrowed and attached on each
`read()`, so moving its owner cannot leave a cached pointer into the old owner.
Providers must uphold `ReaderSegments`' stable-memory contract; custom context
wrappers must consistently expose the matching table and preserve index meanings.
These are application-supplied storage and authority, not policies inferred from
untrusted bytes. Read-only mappings still require stable backing data.

`MessageView<'a, S, C>` remains the borrowed-frame alias and retains its exact/prefix
parsing APIs. `from_unpacked_with_capabilities` and
`from_unpacked_prefix_with_capabilities` attach an explicitly supplied context.
Wire bytes alone cannot recover live capabilities. Legacy constructors use an empty
context, and invalid capability indices remain errors.

`with_allocator_and_capabilities` combines custom or scratch allocation with a
context owner. `freeze()` moves both owners; `into_reader()` transfers them into an
immutable reader without copying payloads or hooks. `into_parts()` consumes either
owner and returns storage plus its context. It does not permit existing views to
survive the transfer, nor does a consumed frozen wrapper constrain later use of
its recovered builder. `compact_copy()` copies reachable data and hooks into a new
private message; failure publishes no partial message. Retained clients remain
callable after the original segment and context owners are dropped.

Views keep their normal borrowing contracts. Imported segments have no editor;
use `compact_copy()` for an explicit mutable copy. Contexts need not implement
`Send`, and this API adds no unsafe `Send`/`Sync` implementations. Capability-table
mutation is exclusive, including while orphan or staged field borrows are live.

`tests/field_owners.rs` covers custom and inline segment providers, borrowed/shared
tables, scratch allocation, owner moves, malformed inputs, lazy descendants,
budget exhaustion and transfer, independent copies, cross-arena orphan rejection,
and calls through capabilities retained after owner disposal. Compiler acceptance
checks reject escaped views, premature storage/context release, competing context
mutation and editing immutable owners. Memcheck exercises the ordinary regressions.
`RpcFieldOwners.tla` models mutable/frozen/reader/parts lifetimes, capability
extraction and copying, missing authority, consumed budgets and fair reclamation;
its 4,808 states produce 12,602 edge traces replayed against production owners
with ordinary and one-word segments. Six fault mutations must fail. After each
trace, an additional read-to-exhaustion check measures the real remaining budget,
including after owner moves and parts extraction. The graph counts are separate
from the existing field and RPC models.
This is a bounded owner-lifecycle model, not arbitrary-provider or whole-RPC refinement.

## Field diagnostics

The field facade adds schema context to `capnp::Error::extra` when an operation
fails. Context includes the schema display name, original field name and explicit
ordinal; `$Rust.name` does not rename diagnostics. Groups have no synthetic
ordinal. Native conversions include their direction, enclosing fields and list
indices, for example:

```text
native encode field-api.capnp:Person: field-api.capnp:Person.phones (@4): [1]: field-api.capnp:PhoneNumber.number (@0): native conversion byte limit exceeded
```

Generated pointer and selected-union readers, borrowed projections, descriptor
inspection, pointer/group handles, cached occupied entries and staged operations
carry context. A failed callback retains its own reason under the enclosing field
and, for list construction, its element index. Unknown native union tags name
the union; root conversion budget failures identify the root schema. Capabilities
and opaque generic leaves use the same field context without inspecting them
merely to format an error.

Context extends the original error: `ErrorKind`, remote trace and detail records
are preserved. Branch on `ErrorKind`; diagnostic text is intended for people and
is not a stable machine-readable format. Formatting reads only static schema
names and indices and retains the original error text; it does not dump message
contents. Existing application/remote error text may itself contain payload data.

Success allocates no diagnostic strings and performs no additional wire reads.
Views and editors do not retain a root-to-leaf path: a later independent operation
on a returned child view identifies that child's field, and a pointer-list
element identifies its index. Recursive native conversion and scoped construction
accumulate the contexts of their enclosing operations. Opaque leaves remain opaque;
their internal unknown layout does not acquire fabricated field names. Legacy
generated readers and loaded-schema reflection retain their existing diagnostics.

Seven regressions in `tests/field_diagnostics.rs` cover malformed text, schema
renaming, nested fields/lists/unions, byte limits, inactive arms, partial fills,
cached upgrades, scoped handles, callback metadata and capability destruction on
failed publication. The native-value TLC replay also asserts the failed field
and conversion direction after acquiring a capability, across all 1,048 existing
traces. Entry, ownership and group-staging replays check that adding diagnostics
preserves their allocation, authority and publication contracts. These are
bounded lifecycle checks, not a proof of error text for every schema.

## Generic RPC and group aliases

Explicit parameter/result struct aliases use lexical declaration order for their
generic parameters, including enclosing scopes, repeated arguments and subsets.
Previously HashSet iteration could produce different orders for declaration and
use, breaking `Persistent(SturdyRef, Owner)` when its two arguments were distinct.
The regression compiles and calls nested data/text/capability methods and checks
16 independent generator runs for identical output. `RpcGenericParameters.tla`
checks 1,560 states and supplies 128 cases (64 brand combinations each for RPC
and union group aliases) for comparison with emitted Rust aliases and server
signatures. Three ordering/outer-scope/unused-parameter mutations fail.

Union `WhichReader` / `WhichBuilder` aliases that name a group bind every enclosing
parameter carried by the generated group type. Looking only at the group's
fields omitted unused parameters and produced uncompilable aliases. The group
regression now compiles reader and builder aliases with distinct data, text and
capability parameters even when the group uses none of them in its fields.

Vendored from crates.io capnp 0.25.6. Original MIT license and upstream metadata are retained. ReProto adds Unix capability file-descriptor hooks and the public asynchronous get_fd accessor. It also adds CallHints, new_call_with_hints and typed send_for_pipeline with backward-compatible default hook methods. Error now has optional boxed exception metadata with remote-trace and opaque-detail accessors; use constructors instead of Error struct literals.

The no-allocation synchronous stream reader fills every segment-table pair before
decoding it. Short `Read` results previously left table bytes in the message body.
Root Cargo regressions cover fragmented consecutive frames and every truncated
table/body prefix.

This fork requires Rust 1.97.0, matching the repository's stable toolchain. Its
maintained Cargo.toml explicitly includes sources, tests, schemas, license and
documentation when packaging. The upstream Cargo.toml.orig and .cargo_vcs_info.json
remain checkout provenance; Cargo generates the packaged manifest metadata from
the current fork instead.

The nightly-only `rpc_try` feature implements
`FromResidual<Result<Infallible, E>>` for `Promise<T, Error>` when `Error: From<E>`.
This preserves synchronous `Result?` in promise-returning methods, including
error metadata, ordinary early-return cleanup and borrowed/non-Unpin outputs.
It never polls or blocks on a promise. The old `Try` implementation whose
`branch()` always panicked has been removed: direct `promise?`, explicit `Try`
bounds and `Try::from_output` on promises no longer compile. Use `.await?` in
async code, `Promise::ok(value)` or `Promise::from_future(future)` as appropriate.
This is a deliberate change to the experimental feature, with no default-feature
API change. The feature is tested with both `std + alloc` and `no_std + alloc`;
without `alloc` it adds no promise API.

DispatchCallResult now carries the generated static cancellation policy. Its legacy new constructor keeps the old cancellable behavior; with_cancellation_policy selects it explicitly. CallExecutor and default ResultsHook lifetime/executor hooks allow protected methods to outlive a canceled caller without assuming a particular async executor.

ClientHook now has default local-server lookup readiness methods; the RPC runtime overrides them to prevent CapabilityServerSet lookups from bypassing queued streaming work.

ClientHook also has a default-rejecting join_capabilities hook and an erased
JoinedCapability response guard. The RPC runtime uses it to forward standard
bilateral Join across independent systems without bypassing opaque wrappers.
ClientHook also has a default opaque-endpoint forward_join hook for independent
multiparty shares; transparent RPC clients forward it explicitly. The Noise
profile authenticates the joined object before direct acquisition.
The guard retains downstream results until the joined authority is acquired or
all upstream parts have been finished. See docs/JOIN.md for scope and checks.

ClientHook's default `delegate_join` keeps opaque hooks as equality endpoints.
Explicit implementations can return a `JoinDelegation` for a complete batch,
with a completion callback that preserves boundary policy and downstream guards.
The membrane runtime uses this for opt-in equality without exposing an unwrapped
result or implicitly forwarding individual secret shares. Existing hook
implementations require no change; this adds no unsafe code.

RequestHook has default private third-party tail-target hooks. The runtime uses
an erased connection context only for direct requests on the same RPC system;
wrappers keep ordinary forwarding unless they explicitly support the hook.
Authenticated answer adoption is enabled separately by the network. See
docs/ANSWER_ADOPTION.md for protocol and verification scope.

Dynamic capability values own their hooks; dynamic Reader is Clone instead of Copy.
Compiled interface reflection supports inherited generic methods, reflected
requests/results/pipelines and dynamic server dispatch. Type/schema equality uses
explicit lazily evaluated generic brands. Checked operations reject missing legacy
generic metadata; regenerate bindings. Native recovery explicitly erases brands.
Recursive pointer cleanup now releases removed capability-table entries during
clear/replacement, including aggregate and far-pointer paths.

The alloc-gated field_api module supports the experimental reader/operation
facade: typed message owners, borrowed frames, checked list/field operations,
and staged pointer publication. Union-group replace_with constructs detached
storage before publishing schema-masked data and moving descendant pointers;
callback failure/unwinding preserves the old arm and sibling fields. Layout adds private staging and checked view
helpers; recoverable operation errors have dedicated ErrorKind variants.
See docs/RUST_GENERATOR.md for scope and remaining design work.

Field ownership adds detached pointer slots with context identity, capability
ownership transfer, strict fixed-layout copy, checked generic editing, branded
sessions and Draft/Ready staged filling. RPC request/response facades retain the
existing hooks and pipelines. These additions use the existing infallible
Allocator contract and do not claim recoverable allocation failure.

The alloc-enabled dynamic_orphan module adds message-scoped disown/adopt to
dynamic structs and lists, including scalar defaults, groups, inline struct
contents, runtime brands and capability ownership. Fixed-list adoption rejects
truncation before publication. Exclusive live-editor access adds independent
allocation/copy, scoped readers/editors with private capability ownership,
checked typed release and shallow list/blob resize preserving descendant
addresses and unknown fields. Shrink now retains outer storage, clearing removed
bytes and padding without additional arena allocation; growth may relocate it.
Copy concatenation plans the maximum physical data/pointer sections across
inputs, preserving unknown fields and independently owning copied capabilities.
Detached groups expose scoped aggregate reads/edits over owned fields, including
defaults, nested union selection, checked field movement and independent copies.
Virtual new groups need no synthetic parent allocation. Materialization moves
fields into contiguous storage for ordinary/typed callbacks without copying
children; parent adoption releases inactive union pointers. Immutable external
word/mmap owners back zero-copy read-only segments retained by the arena.
Tail shrinking reclaims words and growth uses available capacity in place;
otherwise growth relocates shallowly. Scalar builders expose values by copy,
matching C++.
See docs/DYNAMIC_ORPHANS.md for remaining APIs and differences from C++.

The alloc-enabled schema_loader module owns runtime-loaded schema arenas and
uses borrowed schema handles with separate schema/message lifetimes. It adds
transactional validation and compatible replacement, typed/constraint stubs,
generic/implicit method binding, native registration and casts, conservative
capability hints, dynamic message read/edit and RPC client/server dispatch.
Runtime capability reads own hooks independently of source responses. Loaded
reflection also exposes message/schema-borrowed orphan ownership, allocation,
copy/concatenation, scoped and fieldwise group access, resizing, immutable
external data and registered native conversion with exact generic arguments.
It reuses the existing arena and detached capability guards, with no additional
unsafe implementation. See
docs/SCHEMA_LOADER.md for verification and remaining reflection differences.

Loaded group initialization and clearing match C++ and compiled reflection:
reset the default union alternative and non-union fields recursively, preserving
storage exclusive to inactive alternatives. The returned group retains generic
bindings. Selecting a group view does not reset it. Orphan adoption separately
erases all inactive storage to preserve its ownership cleanup contract.

Loaded mutable struct/list getters now share the reader's inactive-union check
before pointer access or default materialization. Rejection preserves wire bytes
and retained capability owners; active views retain defaults and generic bindings.

Loaded lists now expose `get_list(index)` for existing nested mutable views,
preserving brands, capability tables and compatible storage. Loaded and compiled
nested struct-list getters use schema-aware upgrades. This also fixes the compiled
getter's debug assertion from routing struct lists through the primitive getter.
Portable regressions and pinned C++ comparisons cover empty/null children,
schema evolution, unknown fields, rejected accesses and live capability ownership.

Loaded fields and lists expose checked mutable text/data getters and sized
initializers. They preserve existing storage, materialize nonempty defaults,
retain null pointers for empty defaults and validate types/bounds/wire sizes
before replacement or union activation. Successful replacement releases retained
capability hooks. Compiled reflection also leaves empty blob defaults null,
matching C++. Pinned C++ comparisons, ownership regressions and borrowing doctests
cover these operations without adding unsafe code.

Loaded untyped fields have constraint-specific AnyPointer/AnyStruct/AnyList
getters and checked initializers, including explicit inline-struct lists.
Constrained views retain the outer pointer kind, actual layout, unknown fields
and capability table; unconstrained fields expose the existing raw builder.
Field constraints, union selection and initializer bounds are checked before
exposing or replacing storage. C++ layout/wire comparisons, capability ownership
regressions and borrowing doctests cover these APIs. No unsafe code is added.

Schema-free AnyStruct/AnyList readers and builders accept loaded metadata through
`get_as_loaded()`, without native registration or storage copies. Readers apply
missing-field defaults; mutable casts check physical section sizes before
exposing setters and never upgrade borrowed storage. List metadata and encoding
checks reject unresolved types and incompatible schemas. Brands, unknown fields,
capability tables, nesting and traversal limits are preserved without visiting
children. Compiled Rust/pinned C++ comparisons, capability ownership checks and
borrowing doctests cover the new alloc-gated APIs. No unsafe code is added.

Loaded mutable structs/lists also expose `into_any_struct()` / `into_any_list()`.
These consuming conversions retain the complete physical layout and capability
table, including unknown fields in projected lists and siblings in a group's
containing struct. Reborrowed views keep exclusive access. Erasure does not
materialize null children or inspect hooks; the original storage lifetime remains
in force. Native and pinned C++ mutation comparisons, ownership tests and borrowing
doctests cover the conversions without new unsafe code.

Occupied field entries now retain compatible built-in editors or capability
hooks acquired during inspection. Consumption avoids another arena acquisition;
undersized layouts retain the slot and require explicit ensure/upgrade. Readonly
failures are cached without copying external storage, and custom PointerType
implementations retain a default compatibility path. The cache adds no unsafe
code. See docs/RUST_GENERATOR.md#cached-occupied-entries for measured operation
counts, TLC replay and borrowing checks.

Field-operation handles, cached entries and staging retain static diagnostic
locations. Failure annotates the existing error without altering its kind or
exception metadata; success does not allocate diagnostic strings or inspect the
payload again. Native conversion and list iteration add enclosing fields and
indices. See docs/RUST_GENERATOR.md#field-diagnostics for scope and verification.

Typed Message owners now parameterize capability-context ownership via Borrow /
BorrowMut. MessageReader owns arbitrary ReaderSegments providers and matching
capability contexts; MessageView is its borrowed-frame alias. Root caching pins
inline segment providers, while capability tables are attached only to borrowed
views. Consuming transfers retain capabilities and existing reader budgets;
compact copies own independent hooks. See docs/RUST_GENERATOR.md#custom-message-owners.

The 0.x architecture consolidation moves cached struct-root ownership into
`message::CheckedStructReader`. The field facade delegates to this core boundary;
reader transfers preserve traversal consumption and views borrow the owner.
The allocator-free build does not include this owning abstraction.

The maintained schema.capnp and bootstrap-generated schema_capnp.rs expose the
additive Node and Node.SourceInfo/Member startByte/endByte fields and the
RequestedFile.FileSourceInfo identifier table from pinned C++. The identifier
union exposes type IDs and reserved member targets. Older requests retain zero
position defaults and empty reference tables. CodeGeneratorRequest source-info supports
the Rust frontend's documentation and source ranges. Regeneration uses the
maintained capnpc-rust-bootstrap with the pinned C++ compiler; the existing
AnyPointer representation for schema::Value list/struct fields is retained.

The source bundle also includes unmodified stream.capnp from the pinned C++
reference (0de72d8d8cec6b69edaa29de51d3bd490341f9c2), retaining its MIT header.
Together with c++.capnp it supplies the first-party Rust schema build's standard
imports without relying on an installed compiler include directory.

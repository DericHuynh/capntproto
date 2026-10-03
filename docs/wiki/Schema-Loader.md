# Runtime schema loading

`capnp::schema_loader::SchemaLoader` copies and validates schema nodes, resolves
typed dependency stubs, and returns schema handles that borrow the loader. A
mutable borrow is required to load or replace definitions; existing reflected
message views cannot be invalidated by a schema update. No arena is leaked and
no schema reader is promoted to a static lifetime. `Bundle::load()` activates a
retrieved immutable revision closure in a fresh loader, keeping different
catalog authorities and revisions separate. `get_or_load()` provides explicit
lazy loading before borrowing a schema. `load_once()` retains existing concrete
definitions; `load_request()` imports compiler metadata atomically.

The dynamic API reads and builds wire messages with scalar XOR defaults, text,
data, enums, structs, groups, unions, nested lists, generic substitutions and
capability pointers. It supports field clearing, pointer-default materialization,
list/struct initialization and checked copying. Schema and message lifetimes
are separate: extracted capability values own their hooks and can survive the
message or response from which they were read.

For groups, `Builder::init_struct()` and `clear()` restore the default union
alternative and non-union fields recursively. They select the group if it is a
union member and preserve unrelated parent fields and generic bindings. As in
C++, storage exclusive to inactive alternatives can remain physically present.
`group()` selects a writable view without resetting fields; use initialization
when replacement is intended. Orphan adoption has a separate ownership cleanup
contract: it erases inactive storage recursively before adopting new contents.

Interface metadata drives ordinary RPC requests, streaming requests, results,
promise pipelines, pipeline-only sends and inherited method dispatch. Implicit
method parameters bind both anonymous parameter structs and explicit result
brands. Conservative capability hints retain pipelining for unknown stubs and
possible capability fields. `ServiceSchema` owns an immutable shared loader for
server dispatch; `capnp_rpc::new_loaded_client()` exports the server through the
existing RPC hooks. Its shared-server variant accepts an executor for protected
local calls. `CallContext::get()` borrows parameters and results together for
capability-preserving copies; release, pipeline publication and tail forwarding
use the ordinary runtime. Schema annotations never override the dynamic server's
explicit cancellation policy. Metadata describes held authority; it creates none.

For example, after fetching a catalog bundle for an interface:

```rust
let loader = bundle.load(capnp::schema_loader::Limits::default())?;
let interface = loader.get(bundle.root().id)?;
let client = capnp::schema_loader::dynamic::Client::new(remote, interface)?;
let mut request = client.new_request("echo")?;
request.get()?.set_named("value", Value::UInt32(41))?;
let response = request.send()?.resolve().await?;
let value = response.get()?.get_named("value")?;
```

Loading is transactional, including dependency creation and compatible version
selection. An invalid node or batch leaves the previous registry intact. Newer
compatible structs/enums/interfaces replace older definitions; older schemas and
renames retain the existing version. Limits bound node count and copied words.
Validation checks field offsets, default discriminants, union tags, member code
orders, unique names, dependency kinds and group/inheritance cycles. Missing
struct, enum and interface dependencies become replaceable typed stubs. Group
size requirements propagate to containing structs. Slot-to-group and primitive
list-element-to-struct evolution preserve the original slot's offset/default
constraints, even when the target schema arrives later.

`load_compiled_type_and_dependencies<T>()` registers compatible native schemas.
Dynamic struct and list `downcast_native<T>()` methods then check the kind,
list depth, registration and nominal ID; like C++ native casts they erase generic
arguments. `Type::require_usable_as<T>()` exposes the shared metadata check.
Primitive lists need no registration; enum/struct/interface elements do, even
when the list is empty or nested. Native list views borrow the same storage and
capability table, with in-place writes and no capability extraction or promise
resolution during casting. Extracted native clients own their hooks. Use a
builder's `reborrow()` to retain it after a rejected cast. The cast itself does
not mutate storage, although acquiring a dynamic builder can materialize
defaults or activate a union arm. Native erasure does not relax the separate
dynamic assignment or orphan-transfer checks. Unknown future type discriminants
remain inspectable as `Type::Unknown` / `Value::Unknown`; writes refuse to assume
their layout. Pointer-default contents are validated on access, as in C++.

Loaded capability `cast_native<C>()` shares the held hook after checking native
registration and the declaring interface ID. `Client::release_native<C>()` and
`Pipeline::release_native<T>()` instead transfer the hook without cloning;
compiled reflection offers the same consuming operations. On failure they
return `(error, original_owner)`, allowing a corrected cast or ordinary dynamic
use. Generic arguments are erased for these native conversions. Converting a
pending pipeline preserves its path and the capability's membrane restrictions;
it does not await results, project a capability or resolve a promise. Returned
native clients and pipelines own their hooks and can outlive the loader. Null
dynamic clients still return errors. A typeless client target removes metadata;
a generated loaded-client target requires its registered interface metadata.

Individual enum values use `Value::cast_enum<E>()` (compiled reflection:
`dynamic_value::Enum::cast_native<E>()`). C++ checks only the enum ID here;
registration, generic brands and loader identity are not required. The target
must implement `Introspect + TryFrom<u16>`. Generated field-API open enums now
provide introspection and dynamic-reader conversion, preserving every ordinal.
Closed Rust enums return a `NotInSchema` error for unknown ordinals. Non-enum
values and targets fail without numeric coercion. This does not relax native
enum-list registration or the stricter loaded dynamic assignment checks.
`Value::get_enumerant()` optionally looks up the raw ordinal in its source schema
version, retaining that schema's owner and brand. It does not allocate a member
list; the returned member may outlive the message but borrows the loader.

This API is separate from static compiled reflection. It does not compile text
schemas or implement C++ callbacks that mutate the loader through live readers.
Lazy loading requires an exclusive Rust borrow. Full C++ reflection API parity and completion
of the entire RPC port are not claimed here.

## Constants and annotations

`Schema::constant_type()` and `constant_value()` expose loaded constants as
resolved `Type` and `dynamic::Value` values. Text/data and aggregate readers
borrow the loader; structs, lists and enums retain their schema and generic
arguments. The ordinary dynamic setters copy these values into messages and
reject aggregate values from a different loader, even when schema IDs match.

`Schema`, `Field`, `Method` and `Enumerant` expose `annotations()` lists with
fallible indexed lookup, ID lookup and iteration. `Schema::enumerants()` and
`enumerant(name)` provide enum-member reflection. Each applied annotation exposes
its original proto, bound declaration, resolved type and typed value. Annotation
brands resolve in the containing scope, including a method's implicit arguments.
For example:

```rust
let annotations = object_schema.annotations()?;
if let Some(annotation) = annotations.find(label_id)? {
    if let Value::Text(label) = annotation.get_value()? {
        println!("{}", label.to_str()?);
    }
}
```

Annotation declarations resolve lazily from the same loader. Missing declarations,
wrong node kinds, unresolved parameters and mismatched value tags return errors;
the loader does not invent annotation declarations or fetch them implicitly.
Looking up a known annotation does not require unrelated declarations to exist.
Load missing declarations before borrowing views that depend on them. Raw schema
protos remain available when a declaration is unavailable. Pointer contents are
checked on access, and future type tags remain opaque `Value::Unknown` values.

The schema wire representation permits only null interface values. These reflect
as null dynamic clients and cannot provide callable capability hooks. This wire
value support goes beyond the pinned C++ compiler's constant subset, which rejects
interface constants. Annotation metadata does not change a reflected server's
explicit cancellation policy or grant RPC authority.

## Generic brands and union fields

`Schema::is_branded()`, `generic()`, `generic_scope_ids()` and
`brand_arguments_at_scope(id)` expose the applied generic arguments.
`BrandArguments` reports the supplied length and supports indexed access and
iteration. Explicit `AnyPointer` arguments and empty scopes remain represented;
they are distinct from an absent brand. Missing arguments read as `AnyPointer`.
Unbound scopes instead yield symbolic `Type::Parameter` values, even for an index
beyond the current declaration. `generic()` removes all bindings and restores
the default `AnyPointer` substitutions; `get_unbound()` requests symbolic types.

Like the pinned C++ implementation, `generic_scope_ids()` returns the sorted
scope IDs represented in the applied brand. It does not enumerate all lexical
ancestors. A wholly unbound schema has an empty scope list; an inherited unbound
scope appears explicitly and supplies symbolic arguments. An argument list's
length is the number supplied, not the declaration's arity. Inherited argument
lists retain that length and any unused parameters. Duplicate scope IDs are
rejected. Binding or erasing a brand clears the receiver's old symbolic fallback.
Schema equality still checks loader identity, and now also distinguishes
explicit default arguments from omitted scopes.

`Schema::identity()` and member `identity()` methods construct immutable
`capnp::schema::SchemaIdentity` / `MemberIdentity` keys for `HashMap` or `HashSet`.
The keys include loader ownership, applied brands and the member's kind/index.
An inherited method uses its declaring interface; `bind_implicit()` changes
its call types without changing its declaration key. Keys borrow the loader,
so they may outlive a temporary schema handle but cannot survive loader disposal
or replacement. Construction returns an error beyond 128 metadata visits.
These process-local keys never grant capabilities and are not serialization IDs.
Compiled schemas have the same API with static keys, in a separate identity
domain; incomplete compiled generic struct metadata fails instead of merging brands.
Equality and hashing operate on snapshots and never invoke metadata callbacks.

Enums require a distinction between compiled and loaded reflection. C++ erases
enclosing parameters from native enum types and compiled enum fields/list
elements, even when the schema node is marked generic. Rust follows this rule,
including enum identity keys and generated annotation callbacks. Loaded enum
type resolution instead applies the wire brand in the containing scope. Thus
`Scope(Text).Tone` and `Scope(Data).Tone` share a compiled enum key but have
distinct loaded keys; loaded dynamic assignments between those brands fail.
Nested scopes, list elements and symbolic inherited parameters retain their
bindings. The identity of the containing struct and its fields remains branded
in both reflection APIs.

`union_fields()` and `non_union_fields()` return ordinal-ordered subsets with
the original parent and brand. `field_by_discriminant()` returns an optional
union arm, and `find_field()` provides optional named lookup. Named unions and
groups have their own schemas; lookup does not flatten their children.

Unknown wire tags, including `0xffff`, select no union arm. Loaded and compiled
dynamic readers and mutable getters reject inactive-arm reads. Loaded
`get_struct()`, `get_list()`, `get_text()` and `get_data()` check selection before
pointer access, default materialization or layout upgrades, leaving bytes and
capability owners unchanged on inactive access. Active getters still materialize pointer defaults and return
writable views with their generic bindings. Non-union fields remain accessible
when the containing union has an unknown tag. Use `group()`, initialization or
assignment when explicitly selecting a different arm is intended.
This check also precedes capability extraction. Selecting
another arm can leave an old capability pointer physically present, but that
pointer is no longer readable through the inactive arm. Previously extracted
capabilities retain their ordinary ownership. Internal orphan construction reads
defaults separately, without pretending that a synthetic message selected an arm.

`ListBuilder::get_list(index)` returns a writable view of an existing nested
list. Null children yield empty typed views; `init_list(index, count)` explicitly
replaces a child. Bounds and nested-list type checks happen before pointer access.
Compatible storage, generic bindings and capability tables are retained. Struct
children use the schema-aware layout getter, upgrading primitive or smaller
struct lists when necessary while preserving existing values and unknown fields.
The compiled dynamic list getter uses the same layout rules. Both APIs are checked
against pinned C++ in [the nested-list suite](../../tests/dynamic_nested_lists.rs).

Loaded struct fields and list elements expose `get_text()`/`get_data()` for
borrowed in-place edits, and `init_text()`/`init_data()` for sized replacement.
Field getters materialize nonempty schema defaults into message-owned storage;
empty defaults and null list elements stay null. Generic fields bound to Text or
Data retain their bound type. Existing storage is validated for byte-list encoding
and text termination; UTF-8 remains a separate `text::Builder::to_str()` check.

Initializers return zeroed bytes and select the field's union arm. Text sizes
exclude the trailing NUL byte. Text requires `size < (1 << 29) - 1`; data requires
`size < 1 << 29`. Type, bounds and wire-size failures happen before replacement
or union selection. Successful replacement releases any old capability owner.
The [blob suite](../../tests/dynamic_blobs.rs) checks C++ parity, default ownership,
byte/pointer preservation and borrow rejection. Compiled reflection also leaves
empty blob defaults null, matching C++.

Loaded untyped pointer fields have constraint-specific mutable access:

| Declared field type | Existing view | Replacement |
| --- | --- | --- |
| AnyPointer | `get_any_pointer()` → raw pointer builder | `init_any_pointer()` clears and selects |
| AnyStruct | `get_any_struct()` → struct sections | `init_any_struct()` allocates explicit word/pointer sections |
| AnyList | `get_any_list()` → actual list encoding/layout | `init_any_list()` or `init_any_struct_list()` |

Each method requires the matching resolved field constraint. Capability/interface
fields and generic fields bound to a concrete type do not become raw AnyPointer
slots. Constrained views can edit their contents but cannot replace the outer
pointer with a different kind. An unconstrained field exposes the existing raw
pointer builder and defers wire interpretation to its subsequent operations.

Getters check union selection before pointer access. Existing sections, list
encodings, unknown fields, addresses and capability tables are preserved. A null
AnyStruct getter materializes an empty struct; null AnyPointer/AnyList getters
remain null. Initializers select the union arm and release old descendants.
Invalid constraints, list encodings, element counts and total struct-list word
counts fail before replacement or selection. New storage is zeroed.

[Pointer-access verification](../../tests/dynamic_pointers.rs) compares C++ dynamic
lookup followed by its corresponding raw/AnyStruct/AnyList conversion. Rust's
constraint-specific API and initializer prevalidation are deliberately stronger
than C++ raw-pointer access, and have separate local rejection/ownership tests.

Schema-free `any_struct` and `any_list` readers/builders also accept loaded
metadata through `get_as_loaded(schema)` and `get_as_loaded(element_type)`.
They borrow the existing storage and loader without native registration. Readers
apply defaults to missing struct sections, including compatible primitive or
pointer lists viewed as structs. Builders require enough physical storage for
all declared struct sections; they never upgrade or resize through a borrowed
view. Use the owning pointer's typed getter when an upgrade is needed.

List casts check element encoding and struct nesting depth. They reject packed
bit reinterpretation, unknown or unresolved element types and mismatched schema
kinds, including types nested inside lists. Casts preserve brands, unknown fields,
capability tables and remaining reader budgets without traversing children or
extracting capabilities. A successful cast validates the view, not every child;
malformed pointers still fail when accessed. The returned view borrows the loader
and message, and a mutable view keeps exclusive access to its storage.
[Loaded-cast verification](../../tests/loaded_view_casts.rs) compares values and
canonical bytes with compiled Rust and supported pinned C++ casts.

Loaded `dynamic::Builder::into_any_struct()` and
`dynamic::ListBuilder::into_any_list()` consume a mutable view and erase its
metadata without copying or requiring native registration. Use
`loaded.reborrow().into_any_struct()` (or `into_any_list()`) for a temporary raw
view. The original storage borrow remains exclusive; erasure does not extend
its lifetime. Raw edits affect the original message, and unknown fields and
capability tables remain attached. Malformed children are left untouched until
accessed, and empty list views do not materialize null pointers.

Erasure exposes physical storage: an inline-list projection retains every data
and pointer section, and a group becomes a view of its entire containing struct,
including siblings outside that group. It is not a fieldwise group copy. The
[erasure tests](../../tests/loaded_view_casts/erasure.rs) cover these boundaries,
in-place edits and null/default storage; opaque hooks check capability ownership.

Compiled and loaded dynamic readers, builders and detached group views support
`has_with_mode(field, HasMode)` and `has_named_with_mode(name, HasMode)`.
`NonNull` is the existing `has()` behavior. `NonDefault` tests primitive wire
bits, so default-valued scalars are absent; signed zero and NaN payloads compare
by bits. Pointer fields use wire nullness in both modes, even with non-null
schema defaults. Active groups are present; inactive union arms and unknown
loaded field types are absent. These queries do not extract capabilities or
traverse pointer contents. [Presence verification](Cpp-Parity.md#dynamic-reflection-and-result-construction)
includes a pinned C++ comparison and fresh TLC/Rust trace replay.

`dynamic::Value::try_convert(&target_type)` performs explicit checked numeric
conversions, enum name/integer-ordinal conversion and borrowed Text-to-Data.
Compiled `dynamic_value::Reader::try_convert(target_type)` follows the same rules.
Convert first, then pass the result to the existing struct/list/group setter;
conversion errors cannot change its union selection or stored value. Numeric
overflow, fractions and non-finite float-to-integer values report
`NumericConversionOutOfRange`. Float results may round or overflow to infinity.
Enum ordinals may be unknown UInt16 values; enum names must match exactly.
Aggregate brands remain exact and capabilities may be upcast without polling.
Unknown loaded types and symbolic parameters cannot be conversion targets.

## Reflection coverage and limits

The [C++ parity checklist](Cpp-Parity.md#dynamic-reflection-and-result-construction)
owns the broader comparison, including the implemented checked conversions and
result-construction helpers. For loaded schema metadata specifically:

- [x] Loaded constant types/values, enum-member lookup and applied annotation
  metadata, including generic bindings.
- [x] Public loaded-schema generic-brand introspection corresponding to
  `isBranded`, `getGeneric`, `getBrandArgumentsAtScope` and `getGenericScopeIds`.
  Explicit defaults, empty scopes and symbolic inheritance retain their identity.
- [x] Loaded union/non-union field subsets and discriminant-to-field lookup,
  optional field lookup and inactive-arm read checks.
- [x] Optional `find_enumerant()`, `find_method()` and `find_superclass()` plus
  borrowed `short_display_name()` / `unqualified_name()`. Method and superclass
  searches use declaration-order DFS with 64 total visits and preserve applied
  brands. Errors from visited branches propagate; unused branches are not bound.
  Cyclic inheritance remains rejected at load time. Display names use the exact
  byte prefix in the schema; bad offsets return an error and UTF-8 validation is
  deferred to the returned Text reader. [Contract and verification](Cpp-Parity.md).
- [x] Member identity/hash snapshot keys across compiled and loaded reflection,
  with loader lifetimes, exact brands, declaring owners and fallible construction.
- [x] Native/compiled enum brand erasure and loaded enum wire-brand resolution,
  including nested generics, annotation callbacks, lists and symbolic scopes.
- [x] Loaded list native reader/builder casts, shared type compatibility checks
  and compiled capability-list dynamic downcasts. Storage aliasing, capability
  ownership and lifetimes are checked alongside bounded TLC/Rust/C++ replay.
- [x] Loaded native capability sharing and compiled/loaded capability and struct
  pipeline ownership transfer, with recoverable conversion errors and retained
  pending-call paths, membrane policy and schema-independent native lifetimes.
- [x] Individual native enum casts with nominal ID checking, open-enum
  introspection/dynamic conversion, and source-version ordinal lookup. All
  UInt16 values round-trip safely; loaded members retain the loader lifetime.
- [ ] Audit compiled metadata and native conversion coverage against every
  relevant public C++ entry point, with differential cases for remaining gaps.

Unchecked raw-message/schema-offset APIs and deprecated nongeneric dependency
lookup are not required as compatibility shims. Rust readers expose schema data
with lifetimes and checked pointer access. Runtime text compilation and callbacks
that mutate a loader through live borrowed readers remain outside this API's
contract, as described in the roadmap.

## Detached ownership

`capnp::schema_loader::dynamic::orphan` supplies loaded-schema equivalents of
the compiled [orphan API](Dynamic-Orphans.md). Struct and list builders expose
`with_orphanage`, `disown` and `adopt`, including named struct fields, scalars,
capabilities, groups and inline struct elements. Orphans borrow both their message
and schema registry. Dropping or updating either while an orphan exists is rejected
by Rust. Adoption checks arena and capability-table identity, schema authority,
generic arguments and interface inheritance before replacing the destination;
failure returns the orphan. Unknown physical struct fields and their capabilities
survive movement, and undersized inline targets return `WouldTruncate`.

`orphan::Root` extends the same checks to whole struct roots. Construct it from
an `AnyPointer` builder and the expected loaded schema, then use
`with_orphanage()` and `orphanage.in_root(&mut root)` to allocate detached values.
RPC contexts provide this pair through `get_results_orphanage(size_hint)`.
`root.adopt()` replaces the result root without copying its payload; failed
adoption retains the owner and destination. Result contexts also support
allocation hints and `init_results()`, and reject result construction for
streaming methods. [Result construction verification](Cpp-Parity.md) covers both
compiled and loaded roots.

```rust
let (mut root, orphanage) = root.with_orphanage();
let mut value = root.disown_named("source", &orphanage)?;
orphanage.in_struct(&mut root)?.edit(&mut value, |value| {
    let orphan::Editor::Struct(mut object) = value else { unreachable!() };
    object.set_named("text", Value::Text("updated".into()))
})?;
root.adopt_named("target", value).map_err(|failure| failure.error)?;
```

Access through a live same-context editor provides allocation, copying, list
concatenation, shallow resizing and immutable external data. `read`/`edit` use
callbacks that reborrow both payload and schema; borrowed views cannot escape.
Return owned data or a cloned native/untyped capability hook when retaining a
result. Completed edits survive callback errors and unwinding, retaining the
private capability table's ownership. Scalars are passed by value.

`new_group`, `copy_group`, `read_group` and `edit_group` support fieldwise groups,
including defaults, nested groups, union selection and checked field transfers.
These views avoid a synthetic parent allocation. A field orphan returned from a
group callback retains its original message and schema lifetimes. Whole-group
`read`/`edit` and `materialize_group` move fields into contiguous storage without
copying descendants; subsequent fieldwise access moves them back.

`release_as::<T>()` and typed callbacks require native registration and **exact
generic arguments**, unlike the ordinary native casts above. Generic conversion
also requires enclosing schema metadata, available in compiler requests; missing
scope metadata is rejected. A fieldwise group must be materialized before typed
conversion. Opaque `AnyPointer` values cannot be narrowed through adoption or
typed conversion. External data remains immutable and owner-backed. Allocation,
destruction and reclamation have the same limits as the compiled orphan API.

## Verification

`RpcSchemaLoader.tla` checks atomic loads, typed stubs, version selection,
load-once and rejected batches in 78 states. `RpcSchemaUpgrade.tla` checks
before/after dependency arrival and layout constraints in 94 states. Every edge
is replayed through production Rust; the second model runs for both group and
list upgrades. Together these supply 700 component replays and six rejected
mutations. Bounds are two schema IDs/two versions, three transaction operations
or four evolution operations; the state counts are separate graphs.

The existing `RpcDynamicCapability.tla` also runs against the loaded API:
72 RPC traces and 72 local traces cover explicit cancellation policy, early
results release, failures, owned extraction and use after response disposal.
Compiler-generated fixtures test native interoperability, defaults, unions,
generic groups, implicit method parameters and unknown type tags. A real RPC
test retrieves a schema catalog, loads it, calls a reflected server, pipelines
through a returned capability and calls that capability after dropping its
response. Seven differential cases compare validation/version selection with
the pinned C++ loader. Two compile-fail cases enforce arena and update lifetimes.
Loaded orphan ownership also replays every edge prefix of `RpcDynamicOrphans`,
`RpcOrphanAccess` and `RpcOrphanGroups`: respectively **156, 505 and 1,153 states**
and **2,560, 5,012 and 11,652 loaded Rust replays**. These are separate bounded
graphs, shared with compiled reflection. Their 24 fault mutations must violate
the expected invariants. Replays check callable retained capabilities, destruction,
private contexts, group fields, address preservation and rejected transfers under
ordinary and one-word segment allocation; access/group replays exercise both
returned errors and unwinding. Registry identity, native brands, defaults,
unknown layouts and copying have separate runtime regressions. The compiler
acceptance suite adds two positive and 13 negative loaded-orphan cases. Memcheck
exercises all five ordinary loaded-orphan regressions. These are bounded checks,
not composed-model refinement.

`RpcSchemaMetadata.tla` explores **152 states and 308 edge-prefix traces**, each
replayed through the production loader, annotation lookup and dynamic setters.
Bounds are one annotation application, two bound/value kinds (Text and Struct),
two loader authorities and three load/read/copy operations after setup. It checks
missing/wrong declaration kinds, mismatched bound value types, lazy declaration
arrival and rejected foreign aggregate copies. Three model mutations must fail
their expected invariants. Separate runtime cases cover null capability values,
opaque future types, malformed pointers, unbound and implicit parameters, and
unchanged destinations after rejected copies. A pinned C++ differential compares
**29 constant and member-annotation records** from a compiler-generated fixture.
These checks validate this reflection component; they do not prove composed RPC
refinement. Run them with `cargo test --locked --test schema_metadata`.

`RpcSchemaIntrospection.tla` explores **84 states and 336 edge-prefix traces**
through production Rust brand binding/erasure/inheritance and union reads. Bounds
are one generic scope, six brand forms, three wire-tag cases and three operations.
The union transitions run against both loaded and compiled reflection, including
compiled mutable getters. Four faulty model variants must violate their expected
invariants. Runtime cases also cover nested/unused generic scopes, missing and
out-of-range arguments, duplicate scopes, named groups, wrong-loader identity
and a retained live capability pointer behind an inactive arm. A C++ differential
matches **181 introspection records**. Run these checks with
`cargo test --locked --test schema_introspection`. These are component checks,
not a complete runtime equivalence proof.

Run `cargo test --test protocol_models schema_loader_model -- --exact` and
`cargo test --test protocol_models dynamic_capability_model -- --exact`; orphan checks use
`cargo test --test protocol_models dynamic_orphans_model -- --exact`, `cargo test --test protocol_models orphan_access_model -- --exact`
and `cargo test --test protocol_models orphan_groups_model -- --exact`, or the canonical
`cargo test --locked --workspace --all-targets`.

`cargo test --locked --test schema_identity` explores `SchemaMemberIdentity` in
201 states / 649 edge-prefix traces, replaying every prefix through compiled and
loaded Rust and pinned C++. It compares 3,608 cache observations plus 211 identity
comparisons, including hash collisions, loader separation, unused brands and
inherited aliases. Five model mutations must fail. Bounds are nine handles and
three cache operations; callbacks, lifetime rejection and exact construction
limits have separate Cargo tests. This is a reflection component check.

`cargo test --locked --test enum_brand` explores `EnumScopeIdentity` in 298 states /
770 edge-prefix traces. All traces replay through Rust and pinned C++: 2,914 state
observations and 50 identity comparisons across 820 scenarios. Six model mutations
must fail. The model bounds two reflection modes, five enum handles, three actions
and one non-union destination, checking cache identity, assignment rejection and
unknown ordinal preservation. Nested/symbolic scopes, malformed brands and
generated annotation callbacks have separate native tests.

`cargo test --locked --test native_enum` explores `NativeEnumCast` in 494 states /
917 edge-prefix traces, replayed through Rust and pinned C++ with 3,402 matching
observations. Nine fault mutations must fail. Seven source cases, four ordinals
and three actions check ID-only open enum casts, source-version member lookup
and writes to one union destination. Closed Rust enum rejection and exhaustive
UInt16 round trips have separate native tests; a compile-fail doctest prevents
loaded members from outliving their loader. This is a bounded conversion check.

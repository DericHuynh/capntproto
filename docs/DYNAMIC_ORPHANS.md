# Reflected value ownership

The alloc-enabled runtime supports `disown` and `adopt` on dynamic struct
builders and list builders. A disowned value retains its type, message identity
and capability ownership. Pointer payloads move without copying their bytes.
The compiled-schema public types live in `capnp::dynamic_orphan`.
Runtime-loaded schemas expose the equivalent operations through
`capnp::schema_loader::dynamic::orphan`, with separate message and schema
lifetimes and registered native conversion. See [SCHEMA_LOADER.md](SCHEMA_LOADER.md)
for that API and its additional 19,224 ownership/access/group trace replays.
The examples and per-API replay counts below describe compiled reflection.

```rust
use capnp::{dynamic_struct, dynamic_value};
use crate::dynamic_test_capnp::orphan_case; // Generated example schema.

let mut message = capnp::message::Builder::new_default();
let mut typed = message.init_root::<orphan_case::Builder>();
typed.reborrow().init_source().set_text("move this allocation");
let dynamic = dynamic_value::Builder::from(typed)
    .downcast::<dynamic_struct::Builder>();
let (mut root, orphanage) = dynamic.with_orphanage();
let value = root.disown_named("source", &orphanage)?;
root.adopt_named("target", value).map_err(|failure| failure.error)?;
# Ok::<(), capnp::Error>(())
```

`with_orphanage()` consumes a builder and returns that editor plus a lifetime
token. This keeps the message and its capability table borrowed while orphans
exist, while allowing sequential reborrows of fields and list elements. A list
builder can also mint a token. The token grants no independent memory access.
Orphans are move-only; `get_type()` exposes their metadata.

Whole pointer slots use `dynamic_orphan::Root::new(pointer, expected_type)`.
Its `with_orphanage()` returns a root editor and token, and
`orphanage.in_root(&mut root)` supplies allocation and scoped access before the
root has been initialized. `root.adopt(owner)` replaces the complete value;
`root.disown(&orphanage)` leaves a null pointer. Both preserve capability identity.
Inline group types are rejected as independent roots.

RPC `Results::get_orphanage(size_hint)` supplies this pair directly, already
imbued with the result capability table. Dynamic call contexts use
`get_results_orphanage(size_hint)`; loaded contexts return the corresponding
`schema_loader::dynamic::orphan::Root`. Finish the editor borrow before calling
`set_pipeline()`. The [C++ parity checklist](CPP_PARITY.md) records result
allocation, ownership, cancellation and differential verification details.

`disown(field, &orphanage)` and `disown_named(name, &orphanage)` validate the
containing schema, message/capability context and active union arm. Scalars are
captured as values and reset to their declared defaults. Pointer access follows
the C++ dynamic builder behavior: a declared default is materialized before
detachment, then the source pointer becomes null. Group movement preserves its
active union and known non-union fields, resets the source group's union to its
default and leaves parent siblings intact. Group nesting is bounded to 64.

List `disown(index, &orphanage)` resets scalar elements to zero, detaches pointer
elements, or allocates a struct header for an inline struct element. Moving an
inline struct includes its actual data and pointer sections, including unknown
fields; descendant allocations retain their addresses. `List(AnyPointer)` is
unsupported, as in the existing dynamic list editing API.

`adopt` and `adopt_named` consume the orphan on success. An `AdoptError` contains
both the error and the still-owned orphan on failure. Message identity includes
the capability context. Schema comparison includes generic brands; capability
adoption allows a declared derived interface to move into a compatible base
interface. Typed pointer orphans may move into an `AnyPointer` field. An opaque
`AnyPointer` orphan cannot be treated as a more specific type merely by adoption.
Scalars require matching types and the same message scope.

Failed adoption preserves the destination, including its union selection. Inline
list adoption checks the actual orphan dimensions and returns `WouldTruncate`
if the fixed element cannot hold them. It preserves the orphan for a later
compatible adoption. This is stricter than the C++ fixed-list transfer behavior.

Capability hooks move out of their original table entries during detachment and
return to those exact entries on adoption. Discarding the orphan immediately
releases its hooks, including nested capabilities in unknown pointer fields.
Independent clients remain usable, and revocation continues to apply after
movement. Replaced destination capabilities are released. Detached allocation
bytes remain message-owned until destruction; drop does not promise erasure or
reclamation.

## Allocation, access and resizing

`orphanage.in_struct(&mut root)` or `in_list(&mut list)` borrows a live editor
and checks that it belongs to the token's arena and capability context. The
returned `Access` provides `new_struct`, `new_list`, `new_text`, `new_data`,
`null` and `copy`, with `new_group` and `copy_group` for groups. Copying ordinary
struct/list values from another message preserves unknown fields and
copies capability references; subsequent payload edits are independent.
Struct allocation uses compiled schemas, including their generic brands.

```rust
let mut text = orphanage.in_struct(&mut root)?.new_text(3)?;
orphanage.in_struct(&mut root)?.edit(&mut text, |value| {
    value.downcast::<capnp::text::Builder>()
        .as_bytes_mut().copy_from_slice(b"abc");
    Ok(())
})?;
let mut text = text.release_as::<capnp::text::Owned>()
    .map_err(|failure| failure.error)?;
let owned = orphanage.in_struct(&mut root)?
    .read_typed(&mut text, |reader| Ok(reader.to_str()?.to_owned()))?;
root.adopt_named("description", text.into_dynamic())
    .map_err(|failure| failure.error)?;
```

`read` / `edit` and `read_typed` / `edit_typed` use scoped callbacks. Borrowed
readers, editors and nested orphans cannot escape; owned strings and capability
clients can. The original editor stays exclusively borrowed throughout access.
Null struct reads expose defaults without allocation; editing materializes the
struct. `release_as::<T>()` consumes the orphan only when its complete type and
brand match, returning ownership on failure. It does not reinterpret an opaque
`AnyPointer` or perform interface subtype widening.

Views use a private capability table. Hooks remain owned by the orphan, including
on callback errors or panic unwinding. New capability indices are reserved in the
original table before the view ends, so subsequent orphans cannot collide with
them. Replacing a capability releases the old hook immediately. Callback errors
do **not** roll back edits. Existing message-scoped orphans cannot be adopted
into a view with a different capability context; copying remains available.
The private table has the same index span as the original table, so access costs
scale with that span. As with existing builders, imbue the original builder with
a capability table before writing non-null capabilities.

`resize` / `resize_typed` support lists, text and data. Growth is zero-initialized,
text retains its terminator and shrinking releases removed capabilities.
Retained descendants keep their addresses; inline struct lists retain their
actual physical dimensions and unknown fields. Shrinking keeps the outer
list/blob address and clears removed bytes, unused bits and padding without
allocating more arena storage. Tail shrinking reclaims whole words from the segment allocator. Growth uses
available capacity in place when the allocation is at the segment tail, or
when growth fits existing padding; otherwise it relocates shallowly. Other
allocations in the segment prevent tail reclamation or extension. Length limits
are checked before allocation. These guarantees assume ordinary capability
destructors; arbitrary panicking destructors and allocator failure are not covered.

`concat(element_type, &[list_reader, ...])` deep-copies lists into a new orphan.
The declared element types must match exactly, including generic brands.
The allocation uses the maximum actual data and pointer sections across every
input, including fields unknown to the compiled schema. Primitive encodings
are upgraded to inline structs when necessary; bit lists cannot undergo that
upgrade. Pointer descendants are copied independently and capabilities retain
their client identities. Returned copy errors release acquired capability
references and leave the inputs and destination fields unchanged; unused arena
bytes may remain allocated. Length and total inline-word limits are checked
before allocating the payload.

Inputs are borrowed readers, normally from incoming or other immutable messages;
Rust's exclusive destination borrow still applies. Scoped detached views cannot
be collected into an escaping array of readers. An empty input slice creates an
empty list with the explicit element type, whereas the C++ API requires at least
one input. A null list contributes no physical layout. This avoids treating a
null bit-list reader's default Void encoding as a forbidden struct upgrade.

## Detached group views

`read_group` and `edit_group` lend a `GroupReader` or `GroupEditor` to a scoped
callback. They operate on owned fields, without allocating a synthetic parent
struct or copying the payload just to view it. `new_group(schema)` creates a
virtual default group. `copy_group(reader)` (also dispatched by `copy`) copies
known fields and the active union arm, excluding parent siblings and unknown
parent fields. Pointed-to structs and lists still preserve their full physical
layouts. Copy errors release any capability references already acquired.

```rust
let mut group = root.disown_named("body", &orphanage)?;
let field = orphanage.in_struct(&mut root)?.edit_group(&mut group, |mut group| {
    group.set_named("label", capnp::dynamic_value::Reader::Text("updated".into()))?;
    group.disown_named("label")
})?;
orphanage.in_struct(&mut root)?.edit_group(&mut group, |mut group| {
    group.adopt_named("label", field).map_err(|failure| failure.error)
})?;
root.adopt_named("body", group).map_err(|failure| failure.error)?;
```

Both views provide `get_schema`, `which`, `has`, field reads and nested group
reads. Missing fields read their compiled defaults without allocation. Editors
add `set`, `clear`, `adopt`, `disown`, pointer edits and nested group edits; each
field operation also has a `_named` form. Editing or disowning a null pointer
materializes its logical default. Active scalar/group fields are present;
null pointers and inactive union arms are absent. Reading, editing or disowning
an inactive arm fails. Setting, clearing or adopting selects the specified arm.
Disowning a selected arm keeps it selected with its default value.

Field descriptors must match the complete group schema and generic brand.
Adoption also checks the arena, capability context and value type before changing
any field or union selection. Failure returns the supplied owner. Successful
replacement publishes the new owner before releasing replaced capabilities,
including those inside a replaced nested group. Edits persist through callback
errors and panic unwinding.

A group field returned by `disown` retains the original message lifetime and can
be moved outside the callback or into another compatible group. Borrowed group
views and payload readers/editors cannot escape. Pointer payload callbacks still
use the private capability context described above; owners created inside those
callbacks cannot escape. The original message editor stays borrowed during all
views. No unsafe code is required for this group representation.

`materialize_group(&mut orphan)` moves a group's fields into independent,
parent-sized storage. Descendant payload addresses stay unchanged and parent
siblings start empty. Ordinary `read`/`edit` materialize automatically;
`release_as::<generated_group::Owned>()` works after explicit materialization.
`new_struct(group_schema)` allocates contiguous storage directly, while
`new_group` stays virtual until needed. `null(group_type)` also supports lazy
materialization. Groups still cannot be adopted into an `AnyPointer` slot.

Typed and dynamic callbacks retain the same scoped lifetimes and private
capability context as other pointer orphans. Fieldwise group APIs and parent
adoption can convert contiguous storage back into field owners. Conversion
releases hidden pointers left in inactive union arms by generated setters.
Completed edits survive callback errors and unwinding. Repeated conversion
can allocate additional group headers; it never copies descendant payloads.
To discard an attached group's active owned fields, disown it and drop its
owner. Legacy dynamic `clear(group)` resets the default union arm and non-union
fields, as in pinned C++; it does not destroy all union-arm storage.

Scalars can be copied/read and replaced through group `set`. Ordinary `edit`
passes scalar builders by value, matching C++ `DynamicValue::Builder`; changing
that local value does not change the orphan. Pointer/aggregate builders mutate
storage. Runtime-loaded reflection is described separately in
[SCHEMA_LOADER.md](SCHEMA_LOADER.md). These ownership APIs use compiled metadata
and the existing infallible allocator contract.

## External data

`Access::reference_external_data(ExternalData)` creates a zero-copy Data orphan.
`ExternalData::new(Arc<[Word]>, length)` retains shared immutable words;
`from_static` references static words. The unsafe `from_owner` constructor
supports stable immutable owners such as read-only mmap mappings. Its safety
contract requires readable, immutable bytes at a stable address for the entire
arena lifetime, including protection against changes to the backing file.

The buffer becomes a read-only message segment and remains retained until arena
destruction, even after all pointers and orphans are dropped. Readers and
serialization use the original payload address. Mutable Data/list/struct views
and all resize requests fail; replacement, clearing and disown/adopt work without
writing the external bytes. A deep copy creates independent writable storage.
The allocator never allocates into, clears, resizes or frees an external segment.

Storage must be eight-byte aligned, contain the complete final word, and fit
the wire length limit. Unlike the C++ API, constructors reject nonzero exposed
padding, preventing unintended padding bytes from being serialized. Empty
buffers remain non-null Data references. External data owns no capabilities;
unrelated message capabilities retain their normal lifetimes.

## Verification

Run `cargo test --test protocol_models dynamic_orphans_model -- --exact` and
`cargo test --test protocol_models orphan_access_model -- --exact`, or the canonical
`cargo test --locked --workspace --all-targets`.
The concatenation/shrink model can also be run independently with
`cargo test --test protocol_models orphan_concat_model -- --exact`.
Group views use `cargo test --test protocol_models orphan_groups_model -- --exact`. External
data and arena resizing use `cargo test --test protocol_models external_data_model -- --exact` and
`cargo test --test protocol_models arena_resize_model -- --exact` in the same scripts directory.

`RpcDynamicOrphans.tla` has 156 states and 320 edges. Every shortest prefix ending
in a graph edge replays through four production API paths (pointer fields,
capability lists, inline structs and groups), each with default allocation and
fixed one-word segments: 2,560 capability trace replays. Observations check
source/destination identity, orphan ownership, unchanged union selection,
destructor counts and calls through retained clients. Seven injected model
faults are rejected: arena/context/type bypass, lost ownership on error,
duplicate ownership, lost retained clients and leaked replaced capabilities.

`RpcOrphanAccess.tla` has 505 states and 1,253 edges. Every edge prefix replays
with normal and one-word segments and both returned editor errors and panic
unwinding: 5,012 capability traces. Observations check independent allocation,
private view ownership, typed conversion, arena/type rejection, resize, child
addresses, adoption/drop, retained clients and unrelated capabilities. Seven
model faults are rejected: leaked replacements, lost retained/edited clients,
ambient capability removal, retained truncated capabilities, type bypass and
deep copying children during resize.

`RpcOrphanConcat.tla` has 1,760 states and 5,340 edges, yielding 10,680 Rust
trace replays across ordinary and one-word segments. Each prefix exercises
foreign list concatenation through older metadata, failed partial capability
copy, type rejection, independent source/result mutation, in-place shrinking,
adoption/drop and retained clients. Eight injected faults are rejected: lost
unknown fields, missing copied/held ownership, leaked failed/truncated copies,
type bypass, shallow copying and movement during shrink.

`RpcOrphanGroups.tla` has 1,153 states and 2,913 edges. Every edge prefix is
replayed with ordinary/one-word segments and error/panic callback exits: 11,652
Rust traces. They check fieldwise/contiguous conversion, typed reads, group edits, nested union replacement, rejected type/arena
adoption, field movement, stable addresses and siblings, retained callable clients,
and capability destruction. Ten injected model faults fail the ownership,
selection and isolation invariants. These traces use disown/drop when destroying
an adopted group; they do not model legacy dynamic `clear(group)` as destruction.

Separate Rust regressions cover defaults, inactive union rejection, generic
brands, capability subtyping, revocation, nested groups, unknown fields,
undersized inline storage, payload addresses, interleaved detached capability
edits, foreign copies, null materialization, bits, blobs and empty anchors.
Compile-fail cases reject cloning, reuse, escaping the message, reacquiring the
message while an orphan exists, escaped view readers/editors/nested owners and
reborrowing an active view's anchor. Additional group cases reject escaped
group/payload views, edits through a read-only view and reuse of moved fields. The pinned C++ reference runner includes
twelve dynamic orphan tests from `orphan-test.c++:341-627`, 24 additional
allocation/upgrade/null/resize tests, five concatenation cases, three external-data cases and the
`dynamic-test.c++:714-731` group-copy/inactive-arm case. Separate Rust
regressions cover physical layout upgrades, unknown data/pointer sections,
nested capability lists, bit packing, null inputs, brand and size rejection.
Core wire tests check address stability, reclaimed segment extents, erased tails
and padding. `RpcExternalData.tla` checks 451 states and 1,219 edges, replayed
through Rust in normal and tiny segments (2,438 traces), with eight rejected
faults. `RpcArenaResize.tla` checks 16 states and 15 edges, replayed against
payload addresses, initialized bytes and physical segment lengths, with five
rejected faults. These are bounded component checks, not a proof of complete
RPC refinement.

`cargo test --test tooling orphan_memory_safety -- --exact` runs the external-data, group-conversion and arena
reuse regressions under Valgrind Memcheck. The canonical suite includes this
check and requires Valgrind, in addition to its existing Rust, C++ and TLC tools.

These checks cover bounded ownership histories. They do not establish arbitrary
alias-graph, allocator-unwind, concurrent-executor or whole-protocol refinement.

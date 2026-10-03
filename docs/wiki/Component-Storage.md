# Component storage and typed ORM facets

`Store::open_components()` opts into **RPROTO05**. A revision contains a small
immutable manifest and new bytes only for changed components. Unchanged
components reference earlier committed data. This suits objects with small
frequently changed metadata and large infrequently changed payloads.

`Store::open()` still creates and opens **RPROTO04**. Both open methods reject
the other format without rewriting it. There is no automatic migration. V5
also supports ordinary whole-entry objects under separate object keys; an
existing object cannot switch layouts. Existing `Object(T)`, bulk uploads and
persistence realms retain their whole-entry APIs.

## Local typed use

Each component is an independent, ordinary Cap'n Proto message preceded by
its 8-byte schema type ID. The application selects stable `ComponentId` values.
Use different generated struct types for metadata, content and other parts.
Cap'n Proto pointers stay inside their component; use explicit application IDs
for relationships between components.

```rust
use reproto::{
    authority::ObjectId,
    orm::components::{ComponentState, Edit},
    storage::{ComponentId, Revision, Store},
    store_capnp::document,
};
use std::{cell::RefCell, rc::Rc};

fn example() -> Result<(), Box<dyn std::error::Error>> {
let directory = tempfile::tempdir()?;
let store = Rc::new(RefCell::new(Store::open_components(directory.path().join("objects"))?));
let state = ComponentState::new(store, ObjectId::new(1).unwrap())?;
let metadata = ComponentId::new(1);
let mut message = capnp::message::Builder::new_default();
message.init_root::<document::Builder<'_>>().set_text("ready");
let edit = Edit::set::<document::Owned>(metadata, message.get_root_as_reader()?)?;
let revision = state.edit(Revision::INITIAL, Some(Revision::INITIAL), &[edit])?;
let snapshot = state.get::<document::Owned>(metadata)?;
snapshot.with_reader(|reader| {
    assert_eq!(reader.get_text()?, "ready");
    Ok(())
})?;
assert_eq!(snapshot.revision(), revision);
Ok(())
}
```

`ComponentState::edit` accepts heterogeneous `Edit::set::<T>()` replacements in
one atomic object revision. It serializes changed components only, rejects live
capabilities, and checks schema IDs before changing storage. Share one state per
object so newly issued facets reserve their schema even before their first write.
Persisted types are checked again on each write and after reopening. Raw Store
access is trusted administration and can bypass this policy; deleting and
recreating a raw component does not leave a permanent schema tombstone.

`TypedComponent::with_reader` parses the selected message directly from a held
mapping, with normal traversal and nesting limits. It neither copies the entire
object nor assembles delta chains. Snapshot acquisition uses an index lookup
and shares an immutable manifest; cloning shares that manifest and the mapping.
Remote `get` copies only the requested component into its RPC response.

The lower-level `Store::edit_components` accepts raw `ComponentUpdate` values:
`Some(bytes)` replaces or adds a component; `None` removes an existing component.
Duplicate IDs and removal of missing IDs fail before I/O. An explicitly supplied
replacement identical to the previous bytes reuses its stored data.

## Revisions, authority and publication

Both head and publication counters belong to the **entire object**. All edits
inherit unmentioned components from the current head, which may include drafts.
Disjoint component edits still conflict when their expected root heads differ.
Passing `None` as `expected_published` stages a private revision. Passing
`Some(expected)` checks both counters and publishes the new root in the same
checksummed record and file sync. `Store::publish` can separately publish a
retained draft. A failed or lost reply can have an uncertain commit outcome;
there is no automatic retry or exactly-once execution guarantee.

`ComponentServer::<T>::client(state, grant, id)` creates a capability bound to
one component and schema. `Component(T)` provides:

| Method | Rights | Result |
| --- | --- | --- |
| `get()` | GET | Selected component from the published root, plus root revision |
| `put(expectedHead, value)` | PUT | Durably staged root revision |
| `commit(expectedHead, expectedPublished, value)` | PUT and PUBLISH | Atomically staged and published root revision |

Rights and revocation are checked on every call. `commit` publishes **all**
components inherited from the head, including other components' staged changes;
PUBLISH remains object-wide authority. A component facet cannot change its bound
ID or schema. Host issuance must still bind grants to authenticated holders.
The capability works with the existing RPC transports; the regression test
exercises it over a real TCP connection.

Local `component_publication_after()` uses the same validated `PublicationCursor`
and retention rules as whole-entry history. It returns a component snapshot and
the matching cursor. Component RPC push subscriptions, pull history, remote
multi-component transactions and durable bulk component uploads are not yet
implemented. The whole-entry interfaces continue to provide their existing
features.

## Durability, bounds and compaction

V5 keeps the existing record framing, independent header validation, checksums,
exclusive writer locks, recovery barriers and fail-closed handling of complete
corruption. A torn final append is discarded as one unit. Once append I/O has
an uncertain outcome, the writer requires reopening. Successful recovery syncs
the file and directory before serving.

The isolated recovery gate also crashes a child at seven append/compaction
boundaries, then crashes again after recovery's barriers. It checks that a
two-component edit is atomic and that unchanged referenced data survives
compaction. Process exits do not emulate device power loss or real writeback
errors; see [resilience research](Storage-Resilience.md).

There are at most 256 components per object revision and 256 edits per call.
`max_entry_bytes` bounds the complete materialized manifest (24-byte prefix,
24-byte descriptors and every component padded to eight bytes), even if a write
only adds a reference. `max_file_bytes` bounds the file generation as before.
Quotas reject before file mutation. Encoding typed values can allocate before
the store checks its quotas; these are not process-wide memory admission limits.

All three retention policies work. Compaction inlines a shared component at its
first retained occurrence, rebases later references, and stores those bytes only
once among retained revisions of that object. It does not depend on keeping the
component's original revision. Previously acquired snapshots retain their old
mapping, data and writer locks. See [compaction contracts](Storage-Compaction.md).

The engine still uses one append-only mapped file, an in-memory index rebuilt
on open, and synchronous I/O. Every new revision stores all component
descriptors, so metadata costs grow with component count. Segmented files,
persistent indexes, group commit, automatic
component selection and page deltas within a large component remain future work.
Splitting small values into many components can increase storage overhead.

The opt-in [storage worker](Storage-Worker.md) runs the same V5 operations on
a dedicated thread with bounded admission and explicit outcomes. The typed
ComponentState and RPC facets above still use their synchronous Store binding.

## V5 encoding

The 64-byte header uses `RPROTO05` and version 5, with the same checkpoint boundary
and checksum fields as V4. Whole-entry kinds 1–7 keep their previous meanings.
Kind 8 appends a component revision; kind 9 checkpoints one. Their payload is:

| Field | Encoding |
| --- | --- |
| Manifest encoding version | u64, currently 1 |
| Component count | u64, 0–256 |
| Publish flag | u64, 0 or 1; must be 0 in checkpoints |
| Sorted component descriptors | ID u64, origin revision u64, byte length u64 |
| Inline data after each origin-zero descriptor | Component bytes, zero-padded to eight bytes |

All integers are little endian. Origin zero means inline data. Other origins
must precede the containing revision, refer to the same object's same component
ID, and identify the revision that actually holds its bytes. Lengths must match;
forward references, indirect chains, unordered IDs, nonzero padding, unknown
flags and trailing bytes are rejected. In-memory manifests resolve references
to offsets during recovery or commit. Checkpoint publication/floor records use
the existing encoding.

## Reproduce the comparison and tests

```sh
cargo run --locked --release --no-default-features --features storage --example component_store
cargo test --locked -p reproto --no-default-features --features storage --lib storage::components
cargo test --locked -p reproto --no-default-features --features storage --test component_orm
```

The example performs 200 updates to an 8-byte text field beside 64 KiB of
unchanged text, using typed Cap'n Proto messages. The whole-entry reference
encodes both fields into one Document; the component ORM splits them into two
Documents. Both paths stage and publish with one record and one file sync.
On the 2026-10-01 run:

| Logical bytes | Whole entry | Components |
| --- | ---: | ---: |
| Seed, including file header | 65,800 | 65,872 |
| 200 updates, excluding seed | 13,147,200 | 43,200 |
| Per update | 65,736 | 216 |
| Final history checkpoint | 13,224,256 | 128,368 |

This workload writes **304.33× fewer logical update bytes**. These are file byte
counts, not device write amplification or measured speedups. Seed and checkpoint
bytes are reported separately. The earlier [EAE comparison](EAE-Comparison.md)
uses different workloads and checkpoint accounting and is not directly comparable.
The follow-up [storage/RPC research](Storage-Research.md) measures actual disk
latency, component counts, snapshot mapping retention, batching and a dedicated
writer prototype, and prioritizes the next features.

Tests cover root CAS, quotas, mixed layouts, typed facets, live-capability
rejection, revocation, every truncation of a sample commit, malformed manifests,
injected append/sync/compaction failures, old snapshots and randomized operation
sequences checked against fully materialized values across reopen and retention.
They do not establish arbitrary hardware power-loss behavior or formal
verification of the new manifest format.

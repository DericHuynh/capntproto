# Store and persistence ledger compaction

The Store API addresses entries with `storage::ObjectKey`, distinct from
`storage::Revision` counters and `PublicationCursor` handles. Construct a key with `ObjectKey::new(number)` or
explicitly convert an authority ID with `ObjectKey::from(id)`. Zero and all 64
bits are valid storage keys. `Update::object` and `Store::objects()` use this
same type; recovery indexes retain it until explicit file encoding. Keys are
local addresses and are not authority or Store-instance brands.

`Snapshot::object()` and `revision()` expose immutable coordinates alongside
`bytes()`. Clone preserves those coordinates and retains the mapping and locks.
The former mutable revision field is removed. These API changes preserve the
binary file format.

Heads, publications, snapshot revisions, batch expectations/results, cursor
positions and history floors use `Revision`. `Revision::INITIAL` is the zero
counter before any entry; it is never a stored revision. Explicitly import wire
or disk numbers with `Revision::new(number)` and project them with `get()`.
`checked_next()` returns `None` after `Revision::MAX`. There is no implicit
numeric conversion, default counter or unchecked arithmetic. A revision is
object-local metadata, not evidence that an entry exists or has been published.
`publication_cursor(object, after)` remains the explicit numeric checkpoint
import, validating membership and retention before issuing a bound cursor.

`Store::compact(retention)` atomically replaces obsolete file history with a
checkpoint. `Realm::compact()` applies this operation to the latest committed
ledger head, preserving the realm ID, owners, references, deadlines, revocations,
and monotonic epoch/token counters. Both are explicit local administration;
compaction is not an RPC method or an automatic background task.

Current framing independently validates the record header before using its
payload length. Successful recovery syncs both the file and directory before
serving, including complete-but-previously-unsynced records and selected renames.
See [release acceptance](RELEASE_ACCEPTANCE.md) for regression evidence and limits.

## Retention and reads

`Retention::Publishable` keeps the current publication and every staged revision
newer than it. Any draft which was eligible for publication remains eligible.
`Retention::Latest` keeps only the current publication and latest head per object;
it deliberately retires intermediate drafts. The persistence realm uses `Latest`
because each head contains its complete authority state and has no separate
publication cursor. Both modes retire older publication events and move the
history floor to the latest publication. `Retention::History` retains every
available publication and all newer drafts; choose it for consumers that must
resume an event history. Previously discarded history remains unavailable.
Choose `Publishable` when callers may still publish intermediate staged revisions
but need only the latest published snapshot.

All modes preserve object IDs and revision numbers. New writes use `head + 1`
and retain the existing compare-and-set checks. Looking up a retired revision
returns `NotFound`; a `Snapshot` acquired before compaction remains readable,
including after further compactions and after the Store is dropped. A held
snapshot can therefore retain a removed file generation and its disk space.
Compaction does not revoke live capabilities, expire references, or erase every
historical copy of a secret. Realm expiration and collection remain explicit
administrative operations described in [PERSISTENCE.md](PERSISTENCE.md).

The returned `Compaction` reports `before_bytes`, `after_bytes`, and
`removed_revisions`. Byte counts describe the current pathname's generation,
not old mappings, filesystem allocation, or temporary disk usage. Creating a
replacement requires space for both the old and new generation until references
to the old one are released.

## Quotas

`storage::Limits` selects `max_entry_bytes` and `max_file_bytes`. Defaults remain
16 MiB and 256 MiB. `Store::open_with_limits()` accepts larger or smaller host
quotas, subject to checked arithmetic and address-space bounds. `set_limits()`
changes policy on an open healthy store and refuses a limit smaller than already
retained data. Quota rejections happen before a write and do not poison a Store.
Use `file_bytes()` to decide when to compact or grow the quota.

History checkpoints can grow when converting an already-trimmed store: each
object with a nonzero history floor needs an additional 96-byte metadata record.
This space is included in quota checks before replacement. A quota rejection
leaves publication indexes and the old generation intact.

Limits are host policy, not persisted metadata: reopen a larger file with suitable
limits. They do not change ORM schema validation, Cap'n Proto traversal bounds,
or the realm's independent owner/reference quotas. `Realm::from_store(store,
limits, executor)` accepts a configured, dedicated Store; use
`Realm::set_storage_limits()` before exhausting capacity. As with other failed
ledger commits, a failed realm write requires close/recovery before service
resumes. Storage quotas alone do not make transfers or memory usage unbounded.

## Replacement, locking and recovery

A Store holds both an exclusive data-file lock and a persistent adjacent
`<filename>.lock`. It resolves symlink aliases before acquiring the path lock,
and opens the data inode only after acquiring that lock. Every snapshot retains
both its generation's file and the stable path lock. Consequently compaction
cannot open a window for a second cooperating writer, even if only an old
snapshot survives. **Do not unlink or replace the lock file.** As before, all
writers must honor these advisory locks and must not externally truncate mapped
files. Unix compaction refuses hard-linked data files because replacing one
pathname would leave aliases referring to a different history.

The replacement is created with a unique name in the same directory. It contains
all selected entry records, publication records, and a checksummed header with
the exact checkpoint boundary. The implementation writes and synchronizes the
complete checkpoint, maps and locks it, atomically renames it over the data path,
and synchronizes the containing directory before returning success. Permission
bits are copied from the original file; other filesystem metadata such as ACLs
and extended attributes are not copied.

An error before replacement leaves the original Store usable. From replacement
through directory synchronization, an error quarantines the writer until
reopen/recovery. A Realm conservatively quarantines on any compaction error.
Recovery selects a complete old or new generation if replacement was not yet
acknowledged; an acknowledged replacement requires the new generation under the
filesystem's rename/fsync contract. The implementation never truncates a live
mapped generation. A complete checkpoint is mandatory; it cannot be mistaken
for an ordinary torn append. Incomplete trailing appends *after* a complete
checkpoint still recover using the existing validated-prefix rule.

An abrupt process death before rename may leave a `.reproto-compact-*` temporary
file. It is never selected for recovery; removing such abandoned files is host
maintenance. Only the current `RPROTO04` header is accepted. Version-1, version-2 and version-3
files are rejected without rewrite; development snapshots have no migration contract. These
contracts have been exercised on Linux; arbitrary hardware power loss, malicious
writers, multiwriter operation, cross-file transactions and automatic compaction
scheduling are outside this implementation.

## Current file format (version 4)

Every new store and checkpoint uses a 64-byte `RPROTO04` header: version 4 at
byte 8, checkpoint end at byte 16, reserved zero u64 at byte 24, and SHA-256 of
bytes 0..32 at bytes 32..64. Integers are little endian. Checkpoint end includes
the header, is 8-byte aligned, and lies within the file. A new append-only store
has an empty checkpoint ending at byte 64. Old headers are not accepted.

Records use the framing in [IMPLEMENTATION.md](IMPLEMENTATION.md#storage-format-version-4).
Kind 3 retains entries with strictly ordered `(object, revision)` keys. These
may have gaps in their positive revision numbers. Publication records follow:

* Kind 4 combines the latest publication and history floor, used by Latest and
  Publishable retention. It has an empty payload and strictly ordered object IDs.
* Kind 5 retains a publication event, ordered by `(object, revision)`, used by
  History retention. Every publication refers to a retained entry.
* Kind 6 records a history floor, ordered by object after all publications. It
  must name the object's first retained publication. An omitted floor means
  complete history from cursor zero.

All checkpoint records are confined to the declared prefix. Normal kind-1 entry,
kind-2 publication and atomic kind-7 batch records follow that boundary. Header
and record checksums, reserved bytes, padding, framing and ordering are checked.
An incomplete checkpoint fails; incomplete trailing appends recover the complete
prefix. The [durable bulk guide](DURABLE_BULK.md) specifies atomic batches.

`history_bounds` exposes the retained floor and latest publication. Import a
numeric wire/checkpoint position with `publication_cursor(object, after)`, then
read with `publication_after(&cursor)`. The immutable cursor binds this open
Store, object and position; another Store or a reopen returns `ForeignCursor`.
Import validates publication membership; every read rechecks writer health and
retention. A position below the floor returns `HistoryExpired` even if its cursor
was valid before compaction. Draft/future positions return `InvalidCursor`.

Reads leave the input cursor unchanged for retry and return a `Publication`
pairing the snapshot with its next cursor. For example:

```rust
let cursor = store.publication_cursor(object, 0)?;
if let Some(event) = store.publication_after(&cursor)? {
    let (snapshot, next) = event.into_parts();
    // Process snapshot.bytes(), then persist object and next.after().get() as a checkpoint.
    assert_eq!(snapshot.revision(), next.after());
}
```

Moving the Store and history-preserving compaction retain cursor ownership.
Cursors alone hold no file locks, so after reopening the host can explicitly
import the saved numeric position again. They neither reserve history nor confer
capability authority. Retention behavior is independent of header version;
compact and full-history records are current encodings, not migrations.

## Verification

`StorageCompaction.tla` explores **1,024 states / 2,006 edges** for publishable
retention and **1,140 states / 2,184 edges** for latest retention. All **4,190**
edge prefixes replay actual Store commits, file replacement, mmap snapshots,
locks, quota changes, and reopen in Rust. The finite model has one object, three
revisions, one compaction, one snapshot acquisition, one reopen and one quota
increase. Seven mutations detect lost heads, publications, drafts and cursors,
changed mapped bytes, lost writer exclusion, and writes past quota.

The persistence expiry model additionally includes ledger compaction among
pending restores, deadline changes, collection, owner deletion/recreation, and
restart: **3,112 states / 8,674 Rust trace replays**, with 15 rejected mutations.
Each edge endpoint checks the recovered ledger metadata, including counters.

Rust regressions cover multiple objects and compactions, every nonempty truncated
checkpoint, every subsequent torn append, corrupt checkpoint fields, injected IO
errors at four replacement stages, symlink/hard-link behavior, and quota growth
using 17 MiB entries in a file larger than 256 MiB. The model treats compaction
as atomic under the documented filesystem contract; these checks are not a
hardware power-loss proof or whole-runtime refinement.

`StorageHistory.tla` additionally checks 130 states / 241 native trace replays
with all five bounded cursor outcomes checked after each transition. History
checkpoint regressions cover every truncation and torn later append, invalid
publication/floor ordering, schema validation, quota rejection and the same
four injected replacement-stage failures.

Run `cargo test --test protocol_models storage_compaction_model -- --exact` and
`cargo test --test protocol_models persistence_expiry_model -- --exact` and
`cargo test --test protocol_models orm_history_model -- --exact`, or
`cargo test --locked --workspace --all-targets` for canonical source-hashed validation.

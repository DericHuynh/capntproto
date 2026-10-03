# Storage and ORM

Choose V4 whole-entry storage when replacing complete values is appropriate.
Choose explicit V5 [component storage](Component-Storage.md) when updates and
reads naturally address independently encoded parts of an object. Both use
root revision checks, immutable snapshots and the same durability assumptions.
A component split changes schema/layout choices; it is not an automatic diff engine.

The [storage worker](Storage-Worker.md) moves privileged local Store operations
off the async executor. Existing typed ORM servers still execute synchronously.
[Persistence](Persistence.md) adds owner-sealed durable references above storage;
[durable bulk](Durable-Bulk.md) atomically publishes a value and upload receipt.

## ORM contract

The whole-entry contract below remains available. Opt-in
[component storage](Component-Storage.md) adds typed per-component facets,
mapped local reads and atomic updates to multiple components of one root.
It uses an explicit V5 file; the existing APIs keep V4 compatibility.

The schema is `schemas/store.capnp`:

```capnp
interface Object(T) {
  get @0 () -> (value :T, revision :UInt64);
  put @1 (expectedHead :UInt64, value :T) -> (revision :UInt64);
  publish @2 (revision :UInt64, expectedPublished :UInt64) -> (revision :UInt64);
  subscribe @3 (after :UInt64, observer :Observer(T))
      -> (subscription :Subscription);
  history @4 () -> (history :History(T), floor :UInt64, published :UInt64);
}
interface History(T) {
  next @0 (after :UInt64) -> (result :HistoryResult(T));
  cancel @1 () -> ();
}
```

`put` durably stages an entire new entry using compare-and-swap on the head.
`publish` durably changes the visible revision using a separate compare-and-swap;
publication must advance and reference an existing staged revision. `get` returns
only the published entry. A successful result follows fsync; a failed or lost
response does not prove the operation did not persist. There is no automatic
retry of mutations or promise of exactly-once external effects.

Subscriptions send published snapshots, initially the latest revision newer than
`after`. Slow observers receive coalesced updates with revision gaps, not a
complete durable event log. There is one callback in flight per subscription,
and at most 64 subscriptions per ObjectState. Cancellation/drop stops future
callback scheduling; a callback already delivered may already have run.

`history` exposes every retained publication through a pull capability. The
consumer persists its own cursor after processing each returned event and
supplies it to `next`; retries and reconnects do not advance it. Reads at the
current publication wait, canceled or revoked reads terminate, and expired
history returns an explicit gap. `Retention::History` preserves these events
across compaction and restart. Pull and push capabilities share the per-state
subscription limit. See [Publication History](Publication-History.md) for the schema, cursor
scope, at-least-once processing contract and TLC/Rust trace coverage.

The schema is parameterized. This backend accepts generated struct types with a
stable schema ID, persists that ID, rejects type mismatches, and rejects live
capabilities embedded in stored values. The [persistence realm](Persistence.md) supplies explicit owner-sealed durable
references and typed object reconstruction; live client hooks are never stored. Method rights are checked on every call;
subscription authorization is rechecked before each callback.

## Storage format version 4

All integers are little-endian u64 unless stated otherwise. All record starts
and payloads are aligned to 8 bytes. One process holds an exclusive advisory
data-file lock and stable adjacent path lock for the lifetime of Store and every
mapped snapshot. Other programs must honor both locks and must not truncate or
rewrite the mapped file. The lock sidecar must not be unlinked or replaced.

File header (64 bytes): `RPROTO04`, version 4, checkpoint boundary, reserved
zero u64, then a SHA-256 checksum of the preceding 32 bytes. A new store has
an empty checkpoint ending at byte 64; older headers are rejected.
Each record is:

| Offset | Size | Meaning |
| --- | ---: | --- |
| 0 | 8 | `RPENTRY2` |
| 8 | 8 | Kind: 1 = staged entry, 2 = publication |
| 16 | 8 | Object ID |
| 24 | 8 | Revision |
| 32 | 8 | Payload length |
| 40 | 32 | SHA-256 of bytes 0..40 followed by unpadded payload |
| 72 | 8 | First 8 bytes of SHA-256 of header bytes 0..72; checked before trusting length |
| 80 | variable | Payload, followed by zero padding to 8-byte alignment |
| following payload/padding | 8 | `RPCOMMIT` |
| following magic | 8 | Total record length, including footer |

ORM payloads are schema ID followed by an unpacked Cap’n Proto framed message.
Publication records have empty payloads. Checksums detect corruption, not a
malicious writer. Storage is not encrypted at rest. Default limits are
16 MiB per payload and 256 MiB per file; host-selected `storage::Limits` permit
growth beyond these defaults. Reaching the configured capacity rejects writes.

Writes append records and fsync before advancing the in-memory index. Existing
mapped bytes are never modified. Readers retain an Arc-backed mapping and file
lock, while later revisions use a larger mapping. Opening a file scans the
validated prefix and removes an incomplete trailing record. The independent
header digest is checked before any length-based tail repair. Corrupt complete
headers fail without rewriting. Every successful reopen stabilizes the selected
file and containing directory before returning; either sync failure returns no
serving handle. An I/O failure quarantines the writer and new snapshot reads
until reopen/recovery. Existing snapshots remain
readable when Store is dropped and prevent a second writer from opening the file.

On Windows, directory handles use `FILE_FLAG_BACKUP_SEMANTICS` and request write
access so `sync_all` can call `FlushFileBuffers`. Directory open/flush failures
remain errors; the durability barrier is never silently skipped. See Microsoft's
[directory-handle requirements](https://learn.microsoft.com/en-us/windows/win32/fileio/obtaining-a-handle-to-a-directory)
and [flush access requirements](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).

This is a mutable store through whole-entry replacement, not arbitrary in-place
mutation of serialized pointers. Explicit compaction atomically replaces a file
with a current-format compact or history-preserving checkpoint while preserving held mappings; see
[Storage Compaction](Storage-Compaction.md) for retention, locks, configurable
quotas, the checkpoint format and verification. Multiple writers, cross-file
transactions, indexing beyond startup scanning, and recovery from arbitrary
filesystem/hardware corruption are not implemented.


## Verification

Use [Testing](Testing.md) for current commands and [Storage compaction](Storage-Compaction.md)
for detailed recovery checks. Process-crash and application I/O fault tests run
against the storage engine; they do not certify arbitrary device power loss,
writeback failures or hostile local writers. Research measurements and their
limits are in [Storage resilience](Storage-Resilience.md).

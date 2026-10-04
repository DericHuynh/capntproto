# Resumable bulk transfers into ORM storage

`durable_bulk::Receiver<T>` implements `bulk.capnp`'s additive `DurableTransfer`
capability. It receives one unpacked Cap'n Proto message for the generated struct
type `T`, validates its declared SHA-256 digest and excludes live capability
pointers. Completion publishes an ordinary typed `Object(T)` revision.

The trusted host calls `Receiver::create(state, journal_id, grant, config, digest)`.
The journal must be an unused, separately reserved object ID in the target's
Store. The host supplies the target `ObjectState` and a grant with both `PUT` and
`PUBLISH` rights. Each operation checks the grant and its revocation lineage.
Grants, ORM state and journal metadata share the checked nonzero
`authority::ObjectId` and `ObjectGeneration` types. ObjectState's binding is
immutable; `object()` and `store()` expose inspection for trusted hosts. Journal
decoding rejects zero IDs/generations, and resume compares typed bindings before
accepting a transfer. `Receiver::create` and `resume` take a distinct
`durable_bulk::JournalId`. Use `JournalId::new(number)` at the host's numeric
boundary and `journal.key()` to inspect its `storage::ObjectKey` slot. Zero and
`u64::MAX` are valid journal IDs. Selecting the target's own slot is rejected
before writing; the type does not allocate or reserve the slot. Store revisions,
journal expectations and `Checkpoint::revision` use `storage::Revision`.
`Revision::INITIAL` denotes no completed ORM revision; `revision.get()` explicitly
projects the wire/checkpoint number. Journal JSON and RPC fields retain their
numeric encoding. The former raw journal and Store revision arguments have been removed.

The transfer pins the object generation, schema ID, expected head and expected
publication. Clients cannot choose a target, journal, grant, or new expectations
through the transfer capability.

After authenticating a reconnecting holder, the host calls `Receiver::resume`
with that holder's authorized grant and the same journal. Journal IDs are not
credentials. Reissuing authority, persisting host authorization policy, and
discovering a server are application responsibilities. Generation and schema
mismatches are rejected. Dropping a receiver or an RPC connection preserves its
durable journal.

## Interface and retry rules

| Method | Contract |
| --- | --- |
| `describe` | Returns limits, acknowledged byte/chunk counts, phase, expected digest, and the completed ORM revision if any. |
| `write(sequence, data)` | Syncs one consecutive, nonempty chunk before acknowledging its sequence. An identical retained retry returns the same sequence without another commit; conflicting retries are rejected without changing the prefix. |
| `done` | Requires the declared length, verifies the digest and typed message, then atomically commits the target publication and completion receipt. Repeated completion returns the original receipt. |
| `cancel` | Durably closes a receiving transfer; repeated cancellation is idempotent. If completion already won, returns `Complete`. |

An incomplete `done` leaves the transfer resumable. Invalid final content records
a durable `Failed` phase and cannot publish. Target edits fail the pinned compare
and swap; the receiver cannot overwrite a concurrent editor or retarget itself.
Previously issued capabilities share the same journal and observe its current
state on every call. Revocation denies subsequent operations, including durable
cancellation; a trusted host can use separately authorized cleanup authority.

`upload_file(capability, &mut File).await` validates a seekable source file's
length and digest, queries the checkpoint, seeks to the acknowledged byte offset,
and sends the remaining chunks. It validates acknowledgments and the final
receipt. It keeps one chunk in flight, within the configured byte window. After
a lost reply, reacquire the capability and call it again. A server may have
committed an operation whose reply was lost; neither dropping a wait nor a
transport failure rolls back an acknowledged prefix or final publication.
The source file should remain immutable during upload; the receiver independently
checks the digest before publishing.

## Journal and transaction boundary

The journal uses published Store revisions: revision 1 contains the initial
metadata, each later chunk revision contains that metadata's updated checkpoint
and only that chunk's bytes, and one terminal revision records completion,
cancellation or failure. Each payload starts with `RPBULK01`, a little-endian
u64 metadata length, bounded JSON metadata, and the optional raw chunk.
Metadata binds the schema, object generation, expected versions and full digest.
Chunk records are validated for sequence, length and immutable metadata when
resuming or completing. The journal is a host-reserved internal object, not an
ORM object to export independently.

`Store::commit` writes up to 16 distinct object updates in one checksummed,
framed kind-7 record and syncs before updating its indexes or returning. Every
member compares its expected head; optional publication also compares the
expected published revision. A bulk completion's target and receipt are members
of the same record. Recovery therefore exposes both or neither under the existing
validated-prefix/fsync contract. IO failures quarantine the Store until recovery.
Quota and version failures leave it unchanged and can be retried after suitable
host action. Completion also wakes ORM history and the supplied state's push
subscribers. A later write to the target does not change the historical receipt.

Batch framing is an 8-byte count followed by member headers `(object, revision,
publish_flag, byte_length)` as four little-endian u64s and 8-byte-padded data.
The outer record's object/revision fields are zero. All members are validated
before any recovered index changes. Empty/oversized batches, duplicate object
IDs, nonconsecutive revisions, invalid publication flags and nonzero padding
are rejected. Kind 7 is allowed only after a checkpoint boundary. Compaction
flattens committed members into the existing retained-entry/publication format.
Older runtimes reject kind-7 records; this is a storage format extension.

## Retention and bounds

Use `Retention::History` while receiving: it retains the chunk journal through
compaction. Explicitly trimming required staging makes resumption/completion fail
instead of publishing a partial value. Terminal journals need only their latest
receipt and can be compacted with `Latest`. This implementation has no automatic
retention scheduler or journal deletion/ID reuse. Cancellation logically retires
staging; historical bytes and held mappings require ordinary compaction/release
to reclaim space and are not securely erased.

Transfers remain bounded by `bulk::Config` (64 MiB payload, 1 MiB chunk, 65,536
chunks) and the configured Store file/entry quotas. Construct these immutable
limits with `Config::new(length, max_chunk_bytes, window_bytes, max_chunks)?`.
Journal decoding validates the same limits and rejects unknown configuration
fields before exposing a checkpoint or resuming a receiver. Numeric JSON fields
retain their existing representation. The complete value plus
receipt must fit one atomic record; creation reserves a conservative 4,224-byte
metadata/framing allowance within the entry bound. The default entry limit is
16 MiB; hosts may raise it. Receiving writes each chunk once to disk and checks
the retention floor before acknowledging more data. Resume and completion scan
the immutable mapped prefix for validation. Final typed validation/publication
materializes a bounded whole value and the atomic record in memory. This is
resumable file transfer, not unbounded streaming or a cross-file transaction.

## Verification

Run `cargo nextest run --test protocol_models durable_bulk_model -- --exact` or the canonical
`cargo nextest run --locked --workspace --all-targets`. `RpcDurableBulk.tla` models two chunks,
explicit retries, incomplete/repeated completion, cancellation, restart,
history compaction, revocation, concurrent target edits and digest failure.
The seven graphs contain 1,625 states and 2,971 edges.
Every graph edge replays via a shortest prefix against both native receivers
and real two-system RPC with actual disk journals. Nine injected faults must
violate the expected invariants. Fair progress permits completion/cancellation
or an explicit authorization/version conflict; it does not promise network
availability or infinite retries.

Separate Rust regressions cover full restart after lost chunk/completion replies,
all byte truncations of an atomic batch, semantic corruption with recomputed
checksums, quota failure/growth, capability payload rejection, generation/schema
authorization, and real Native UDP delivery to ORM history/push consumers.
These are bounded checks under the documented filesystem assumptions, not proof
of arbitrary power loss or end-to-end runtime refinement.

# Owner-sealed persistence realm

`src/persistence.rs` implements a local durable realm for the standard
`Persistent(SturdyRef, Owner).save` interface. The standard schema and its wire
IDs are unchanged. `schemas/persistence.capnp` defines this realm's Owner,
SturdyRef and authenticated Restorer bootstrap. This is an application realm,
not a new RPC message or a universal persistent-object format.

An Owner is a stable, nonzero 16-byte ID. Its current authenticated Noise public
key and monotonically increasing epoch live in the realm ledger. A SturdyRef
contains the realm's random 16-byte ID and a 32-byte opaque reference token.
New tokens contain an eight-byte durable issuance serial and 24 random bytes;
the ledger retains the serial and token's SHA-256 digest. Issuance serials are
never reused after collection. Null owners are rejected.
This follows the pinned `persistent.capnp` design's separation of owner IDs
from replaceable authentication keys.

## Hosting and restoration

Open a dedicated ledger with `Realm::open(path, Limits::default())`, register
owners through trusted local administration, and register a `Factory` for each
nonzero descriptor kind. Factories are application code: they reconstruct an
object from its kind, object ID, generation and exact rights. Registrations are
immutable for an open realm and must be installed again after restart. The host
must preserve the meaning of kind IDs and generations across deployments.

`Descriptor::new(kind, object, generation, rights)` takes distinct `ObjectKind`,
`ObjectId`, `ObjectGeneration` and `Rights` values and is infallible. Each identifier
has an explicit checked `new(u64) -> Option<Self>` or `TryFrom<u64>` boundary;
zero is rejected, and `get()` projects the stored number. For example:

```rust
use reproto::authority::{ObjectGeneration, ObjectId, Rights};
use reproto::persistence::{Descriptor, ObjectKind};

let descriptor = Descriptor::new(
    ObjectKind::new(1).unwrap(),
    ObjectId::new(42).unwrap(),
    ObjectGeneration::new(3).unwrap(),
    Rights::ALL,
);
assert_eq!(descriptor.object().get(), 42);
```

Descriptor fields are private and immutable; `kind()`, `object()`, `generation()`
and `rights()` expose typed values. Cloning preserves these bindings. Checked
deserialization retains numeric ledger fields and rejects invalid identifiers,
rights and field sets. A descriptor is metadata, so constructing or decoding one
does not grant authority. IDs are not branded to a particular realm, and object
generations remain a trusted host policy, distinct from route generations.

`Realm::persistent(client, descriptor, authorize_save)` binds the standard
Persistent interface to one application capability. The host must ensure the
descriptor represents exactly that capability's authority. The explicit guard
runs on every save, outside internal borrows, and enforces live revocation and
allowed owner destinations. Descriptors must include `Rights::DELEGATE` to save.
Other interface calls retain their original hooks, hints, streaming and pipeline
behavior. Save waits for already queued local application streams before checking
its authorization guard. Persistent remains a distinct facet: its guard defines
save authority, including how application errors affect persistence. The wrapper is a settled composite facet: shortening it to its base
would lose Persistent. It does not automatically make embedded capabilities or
an application's bare self-reference persistent.

For direct local save calls use
`Realm::open_with_executor(path, limits, executor)`. Obtain the executor and its
driver from `capnp_rpc::new_call_executor()` and keep the driver running. The
standard save method is protected from caller cancellation after dispatch;
incoming calls automatically obtain a task owner from RpcSystem. A failed or
lost save response does not imply that a durable reference was not created.
There is no automatic retry or exactly-once token issuance.

Install `realm.bootstrap_factory()` with
`RpcSystem::new_with_bootstrap_factory(Box::new(network), local_noise_key, factory)`.
The Restorer captures the peer key supplied by the authenticated Noise network;
its RPC parameters contain only the reference. `Realm::restore(reference, peer)`
is also available to trusted local hosts; its peer argument is authenticated
transport context, never an untrusted request field. No transport or Snow
profile changes are needed: the existing `Noise_IK_25519_ChaChaPoly_BLAKE3` and
introduced-session IKpsk2 profiles remain pinned.

Restoration checks the reference, owner key and epoch before invoking a factory.
No ledger or registry borrow crosses application code or an await. After the
factory returns, the realm checks them again, including whether this realm
instance is still open. Revocation, key rotation, rotation back to the original
key, close and dropping the last Realm prevent a pending result from escaping.
Canceling restoration drops the factory future; application side effects already
performed by a factory are not rolled back.

`rotate_owner(id, expected_epoch, new_key)` durably changes the owner's key using
an epoch compare-and-swap. Epochs are allocated from a realm-wide durable counter;
use the returned epoch rather than assuming that it increased by one. Existing
SturdyRefs remain usable by the new key.
`revoke(reference)` writes a tombstone and rejects future and pending
restores. Both are trusted local administrative operations. Already returned
live capabilities retain their authority; revoke them separately through their
application policy if needed. A restored capability can save a fresh reference
only for the same owner while its source reference and captured authentication
epoch remain valid. Each successful save creates an independent durable grant;
revoking its parent token does not revoke previously issued child tokens.

RPC facets hold weak realm references. Keep a Realm handle alive while serving;
closing or dropping it does not keep persistence operational through surviving
clients. Factories that capture a strong Realm can create a reference cycle;
applications should use a separately managed administrative handle for reentry.

## Owner deletion, expiration and collection

`delete_owner(id, expected_epoch)` atomically removes the owner and all references
sealed to it, returning the number of removed references. It releases their
bounded ledger slots and blocks pending restores and renewal through previously
restored facets. Re-registering the same owner ID, even with the same key, assigns
a fresh epoch; stale rotation/deletion requests cannot target that incarnation.
Already returned live capabilities remain usable under their application policy.

Expiration uses **host-driven logical time**, persisted by
`advance_clock(now)`. Hosts choose the unit and advance it from their scheduler;
there is no background timer or implicit system-clock read. Time starts at zero,
cannot decrease, and survives restart. Each advance commits the new time and
removes expired and revoked references together. `advance_clock(realm.clock())`
collects revoked slots without advancing time. Its return value is the number of
removed references. File bytes and historical snapshots are not compacted or
securely erased by collection.

`persistent_until(client, descriptor, deadline, authorize_save)` binds the standard
Persistent facet with an absolute realm-clock deadline. Its saved references
expire when `clock >= deadline`. The deadline is checked when the protected save
actually issues the reference, after earlier local streams. The ordinary
`persistent()` binding continues to issue references without an expiration.

`expire_at(reference, deadline)` adds or shortens an existing reference's deadline;
it cannot extend or remove one. A deadline at or before the current clock removes
the reference immediately. Restoration checks expiration before factory invocation
and again before delivering its result. Saves through restored capabilities inherit
the source reference's current deadline, so renewal does not create an unlimited
grant from an expiring grant. Already issued child references remain independent:
later revocation or shortening of the parent does not retroactively change them.
Owner deletion removes all of that owner's references, including those children.

Deletion, deadline changes and clock advancement are trusted local administrative
operations. None is exposed by the authenticated Restorer RPC service. Missing or
collected references remain unavailable; `revoke()` is idempotent only while its
tombstone remains in the ledger.

## Typed ORM objects

`ObjectFactory<T>` supplies reconstruction for the existing whole-entry
`Object(T)` interface. Construct it with a shared `Store` and a fixed
`ObjectGeneration`; construction is infallible once this identifier is checked.
Register it under a stable `ObjectKind`. Use `factory.state(object)` with an
`ObjectId` for original and restored facets so clients share schema binding and
publication notifications.
`realm.persistent_object::<T>(state, grant, kind)` binds the object and checks the
grant's live lineage and delegation right for every save. Restored grants retain
the descriptor's exact rights and generation. Revoking the original ephemeral
grant blocks further saves; already saved durable grants are independent.
`Grant::root`, grant getters, descriptors and ORM state share `authority::ObjectId`
and `authority::ObjectGeneration`. Both require checked nonzero construction;
`persistent_object` carries them directly into the descriptor. ObjectState's
private binding exposes `object()` and a borrowed `store()` handle for trusted
administration. The Store API uses `storage::ObjectKey::from(object_id)`;
object IDs remain typed throughout this boundary. Revisions remain numeric.
`ObjectKind` remains in `persistence`; the former persistence-only object type
exports and raw grant/ORM constructors have been removed from this unreleased API.

The ORM still rejects embedded live connection capabilities. Applications may
store the explicit SturdyRef schema as data and restore it through the realm.
This does not serialize a live Rust client or recursively persist an object
capability graph. Arbitrary application factories remain responsible for their
own generation, schema and authority checks.

## Durability and bounds

The ledger uses the existing locked, append-only, checksummed Store format.
Each mutation appends one complete JSON ledger snapshot and acknowledges only
after the record is synchronized. Recovery selects the latest committed **head**,
not Store's separate publication cursor. Torn trailing records are discarded;
corrupt complete records, invalid metadata and files containing other objects
are rejected. Any failed Store write quarantines the open realm until it is
closed and recovered; an ambiguous write result cannot leave service running
against stale in-memory authorization.

Default limits are 64 owners and 1,024 references; maximum configurable limits
are 256 and 4,096. At most 64 factories can be registered. Revoked references
retain slots until collection; expired references are removed on clock advancement
or an already-passed deadline update. The default file limit is 256 MiB; a
host-configured Store can use larger bounds through `Realm::from_store()`.
`set_storage_limits()` changes the open realm's storage quota before exhaustion.
`Realm::compact()` retires older ledger heads through atomic file replacement,
retaining the complete current authority state, clock, and monotonic counters.
It does not invalidate pending factories or live capabilities. See
[STORAGE_COMPACTION.md](STORAGE_COMPACTION.md) for replacement/recovery semantics,
retained snapshots, lock sidecars and current file format version 4. Distributed
replication remains unimplemented.

Ledger format `reproto-realm/2` persists time, owner-epoch and issuance counters,
and per-reference deadlines. Only this current format is accepted. The unreleased
version-1 migration and defaulted lifecycle fields have been removed. Obsolete,
incomplete and invalid ledgers fail without being rewritten. Every reference has
a nonzero issuance serial. Counter exhaustion fails without writing or wrapping.
Storage is trusted local state: checksums detect corruption, not malicious
rewrites or rollback to an old valid ledger. Secure owner-key replacement,
backups and filesystem access are host responsibilities. File commits rely on
the existing Store/fsync contract; hardware power-loss behavior is not simulated.

## Verification

`PersistentFactoryBinding.tla` checks **293 states / 564 edge-prefix replays**
through real Realm registration, binding, Persistent.save, restoration and ORM
get/put calls. Bounds are two factory kinds, two objects, two generations, two
rights sets (VIEW plus DELEGATE, and ALL), one saved descriptor and one restore.
The factories accept different generations; tests observe which object is read
and whether writing is allowed. Five controls must detect missing registration,
wrong dispatch, ignored generation, object substitution and rights expansion.
This is bounded safety evidence with a fixed authorized owner; lifecycle,
revocation and restart remain covered by the models below. Run
`cargo test --locked --test persistence_types -- --nocapture` and
`cargo test --locked --test api_contracts persistent_descriptor_compile_contracts -- --exact --nocapture`.

`RpcPersistence.tla` checks one committed reference, three owner epochs including
key ABA, a wrong authenticated peer, asynchronous factory completion/cancellation,
revocation, close/reopen and renewal. TLC explores **588 states and 1,236 edges**.
Every edge is replayed via a shortest prefix against real Realm disk commits,
standard Persistent.save and gated Rust factory futures. Replays check returned
capability lifetime, failed-result destruction, canceled factory cleanup and
renewal outcomes. Five mutations bypass sealing, revocation, epoch comparison,
close handling or the renewal guard and violate the model invariants.

`RpcPersistentSave.tla` adds 18/16 states and 26 edges per allow/deny policy
scenario. Its 52 Rust trace replays cover a save behind an earlier local stream,
policy changes at stream completion, caller drop and realm close. Two mutations
reject an early save and cancellation of an already dispatched protected save.
Together these models supply 1,288 capability trace replays. The streaming
barrier check is local; arbitrary remote/proxy scheduling is outside this model.

`RpcPersistenceExpiry.tla` adds **3,112 states and 8,674 Rust edge-prefix replays**
for two clock ticks, shortened and inherited deadlines, owner deletion/recreation,
expired/revoked slot collection, one renewal/replacement, asynchronous factory completion or
cancellation, one restart and one ledger compaction. Every edge endpoint also
checks the recovered ledger metadata. Fifteen mutations violate delivery,
deadline, retirement, epoch, time or issuance invariants, including four
compaction rollback faults. All three persistence models total **9,962 trace replays**.

Ordinary tests cover real encrypted Noise RPC, streaming and capability pipelines
through bound facets, distinct reference/owner types, restart, exact ORM rights
and generation, shared subscriptions, malformed metadata/references, quotas and
reentrant factories. Save, rotation and revocation recovery are exercised at
every byte truncation of their final transaction. Lifecycle regressions likewise
cover deadline updates, clock advancement/collection, deletion, recreation and
rejection of obsolete formats without mutation, plus quota reuse, stale administrative
epochs, inherited lease limits, counter exhaustion and `u64::MAX` time. Actual disk-full/fsync errors
are not fault-injected. These are bounded component checks, not complete RPC
refinement, distributed persistence verification or a cryptographic proof.

Run `cargo test --test protocol_models persistence_model -- --exact` and
`cargo test --test protocol_models persistence_expiry_model -- --exact`, or the canonical
`cargo test --locked --workspace --all-targets` to include the full workspace checks and
source-hashed report.

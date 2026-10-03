# Durable ORM publication history

`Object(T).history()` creates a typed pull capability and returns the current
history floor and published revision. It requires `SUBSCRIBE` authority. The
consumer supplies its last processed publication to `History(T).next(after)`;
each call returns the earliest retained publication strictly after that cursor,
or waits for a publication. Staged drafts are never events. A nonzero cursor
must name a published revision; future and unpublished cursors fail.

The consumer persists its cursor after processing the event. Reading an event
does not acknowledge it or advance any server cursor. Retrying a cursor returns
the same retained event, including after reconnecting or reopening the store.
This provides at-least-once processing; applications must handle duplicates and
coordinate their own effects with their cursor checkpoint. A revision cursor is
scoped to the same object and store lineage, not a global identifier or a proof
against storage rollback. There is no durable server-side consumer registry.

If the cursor is below the history floor, `next` returns `gap` and that floor.
It does not silently skip lost publications. A consumer can fetch the current
snapshot with `get`, checkpoint its returned revision, and resume from there if
its application permits resynchronization. Bounds returned when opening a
history capability are observations, not retention reservations.

Existing `subscribe` push notifications still coalesce to the latest snapshot.
The new method is additive at ordinal 4; the existing method ordinals and
standard Cap'n Proto RPC schemas are unchanged.

## Durability and retention

Successful publication records are synced before becoming visible. Recovery
reconstructs their ordered per-object event index. `Retention::History` keeps
every available publication payload and all still-publishable drafts. Its current-format
checkpoint also preserves the history floor. It cannot restore history already
discarded by earlier compaction. `Latest` and `Publishable` explicitly discard
old events and advance each object's floor to its current published revision.
A compact latest-publication checkpoint starts with that revision as its floor;
ordinary append records after the checkpoint remain available.

History consumes the configured store quota. Rewriting a trimmed checkpoint as
a full-history checkpoint needs one additional floor record per affected object
and can fail if the quota has no space. A rejected compaction leaves the prior generation intact.
See [storage compaction](Storage-Compaction.md) for filesystem assumptions.

## Lifetime and resource bounds

Each history capability allows one outstanding `next`. Concurrent reads fail;
dropping a pending call releases its slot. Explicit `cancel` is idempotent and
ends pending reads. Revoking any ancestor grant also ends a blocked read without
waiting for another publication. A reply already delivered is not recalled.
History and push subscriptions share the 64-capability limit per `ObjectState`.
Cancel or capability destruction releases that reservation. One event per call
provides backpressure; no consumer queue or unbounded background delivery task
is created. Typed decoding checks each historical payload independently.

Store-wide notifications wake consumers even when a publisher uses another
`ObjectState` or the native store API. Reads subscribe before checking the index
to avoid losing a publication between the lookup and wait.

## Verification

`cargo test --test protocol_models orm_history_model -- --exact` checks `StorageHistory.tla` and
`RpcHistory.tla` with TLC, then replays every graph edge through a shortest
prefix against the native mmap store and both local and real two-system RPC
capabilities. The storage model has three staged revisions, selective
publication, one checkpoint of each retention kind and one restart. The RPC
model has two publications and two requests, retry, explicit acknowledgement,
pending-call drop, cancel, revocation and trimming, observed after each executor
drain. Fair publication/cancel/revoke is checked to terminate pending reads.
The graphs contain 130 storage states / 241 edges and 405 RPC states / 931
edges. All 241 native, 931 local capability and 931 wire trace prefixes replay.
Seven injected model faults violate the expected invariants.

Rust regressions additionally cover a full server restart with a persisted
consumer cursor, checkpoint corruption/truncation and torn later appends,
compaction failure stages, schema mismatches, mixed subscription capacity,
quota failure, independent publishers, and authenticated Native transport.
These are bounded checks, not proof of every executor schedule or power-loss
history. The machine-readable evidence is in
`reports/capntproto/orm-history/verification.json` and the canonical runtime report.

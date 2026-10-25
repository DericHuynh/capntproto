# Movable input views from the existing message pool

Messages::read_owned retains its lease and immutable buffer view together in a movable Reading owner. into_lease ends the view before returning the affine reservation. No second pool, reference count, heap owner or self reference is introduced. Dropping Reading alone retains the existing explicit-release policy.

The actual legacy incoming::PooledBytes adapter owns Reading and a concrete release closure. Its destructor ends the view before invoking pool release. Existing NoAllocBufferSegments validates and reads the actual serialized frame; malformed frame construction also returns the reservation. The generated legacy dispatch task can own this entire input chain while suspended, then release the original buffer on completion or cancellation.

Local checks pass: 228 engine tests; six actual no-default incoming tests; eight generated dispatch tests in both fixed and alloc profiles, each with zero new blocks/bytes; nine actual alloc-only legacy conversations; engine/RPC/fixture Clippy and host execution. M4 link has no allocator or undefined symbols. It is link evidence, not board execution. Hosted work/allocation comparison and Miri were pending at source commit; completed evidence follows.

The isolated fixture instantiates actual legacy concrete incoming/dispatch owners, not a complete no-alloc RpcSystem. Outgoing publication, capability/pipeline ownership and caller-backed connection integration remain open. No native latency claim is made.


## Completed qualification

[Immutable run 40205699992](https://github.com/DericHuynh/capntproto/actions/runs/40205699992) passed hosted checks and all 240 profile trials. Both revisions were built at the same source path; five paired repetitions retain means, sample SD, allocations/bytes and every regression in comparison.md/json. Branch misses are simulated, not native latency measurements.
All 715 common normalized function bodies match; scoped question/scheduler Ir/Bc/Bi deltas are zero in every sample. TLS blocks stay 7/57/202. Small whole-program Ir (-0.029%..+0.041%) and miss variations are retained. 70 Miri executions passed across both widths. This removes the movable-input ownership blocker, not the remaining complete connection/capability/pipeline/outgoing owners.

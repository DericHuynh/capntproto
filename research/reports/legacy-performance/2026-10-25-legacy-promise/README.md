# Concrete deferred owners in the legacy Promise

Promise<T,E,F> now keeps its actual future owner F. from_owner accepts concrete
Unpin futures, caller-pinned references and the shared engine's owning task-place
handles, with no allocation and no static lifetime bound. The same poll algorithm
is used. The hosted default remains Pin<Box<dyn Future<Output=Result<T,E>>>> and
retains its existing immediate/error/deferred representation. from_future still
allocates that default owner; the alloc profile still boxes synchronous errors.
Concrete future errors are returned directly without allocating, even with alloc.

The no-alloc profile also supports immediate success/error/result conversion,
using an inline error and a Ready default future type. Only the deferred owner is
projected for polling and it must be Unpin; results/errors are moved, never pinned.
No new unsafe code or pin projection is added. A !Unpin operation is pinned by its
caller or by the existing audited task-place owner.

Ownership is explicit: dropping a borrowed Pin reference does not destroy the
original future. A task-place owner does destroy it in place on drop. This is
necessary for cancellation to release service storage before slot reuse. Hosted
deferred destruction behavior is unchanged; a raw Promise retains its deferred
owner until the promise is dropped. The scheduler/typed response adapter has its
own completion-retirement policy.

Five core tests cover pending storage ownership/cancellation, stable addresses of
borrowing !Unpin futures while the Promise moves, concrete errors without boxed
conversion, immediate !Unpin outputs and fixed-profile immediate errors. All
measured paths allocate zero blocks/bytes. Two integration tests pass a suspended
service operation through the actual shared task-place owner and Promise, and
exercise real completion wake delivery plus a borrowed response that survives
future retirement. Waker Arc construction occurs outside the measured region;
cloning it within the operation allocates nothing. These are ownership/primitive
integration tests, not a complete network RPC conversation.

Local results: core 49 no-default unit tests, all targeted wire ownership tests,
13 no-default documentation tests (4 ignored), nine actual alloc-only legacy
conversation tests, and core-library/wire-all-target Clippy pass. The host probe
executes Request, Call envelope, borrowed completion, owning task-place future
and Promise together. Its M4 ELF links without allocator or undefined symbols;
embedded-link.json records scope/hash. It has not run on a board and does not
instantiate the complete legacy driver.

Hosted tests, strict-provenance Miri on both widths/profiles, and five repeated
compiled-work/allocation profiles require the immutable qualification. Native
latency must use dedicated droplets. Remaining ClientHook/PipelineHook and
connection ownership are still allocating/static-erased; this extraction does
not complete no-allocator legacy RPC or select the alternate bounded driver.

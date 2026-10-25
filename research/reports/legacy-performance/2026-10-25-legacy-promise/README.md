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
No new unsafe code or pin projection is added. Redundant async forwarding is
removed from the completion fixture/probe; the existing receiver is the future. A !Unpin operation is pinned by its
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

Local results: core 49 no-default unit tests, 117 no-default wire tests,
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


Source `522030dcf471796c0394b4df1b04a8571a2f7d05`, qualification
`1691d0f60ca6a46256adbbbcaee33643c9d69636`,
[run 40180710173](https://github.com/DericHuynh/capntproto/actions/runs/40180710173).
Strict-provenance Miri passes 18 core and 32 wire ownership executions across
64-bit/32-bit no-default/explicit-alloc profiles. Five core tests run without
alloc, four with alloc; the fifth specifically requires the inline fixed error
representation. Hosted compiled-work/allocation qualification remains running.


## Hosted qualification

Both jobs pass in run 40180710173, including the full hosted RPC suite, 30
encrypted behavior tests and 50 targeted Miri executions. All 240 repeated
work/allocation trials complete. Capability-owner plus scheduler self Ir/Bc/Bi
slopes are exact; TLS block slopes remain 7/57/202 and every byte/QUIC sample
is retained. Whole Ir ranges -0.006% to +0.038%, including a deep +63.160
paired mean. The whole deep Bc delta is +68.080; helper-only equality does not
prove whole-program equivalence.

247/304 normalized common captured assembly bodies match; 25 symbols appear
and 25 disappear. Making the default boxed future an explicit third generic
argument also renames Promise/PromiseInner and functions containing those types.
Raw function attribution preserves both names, so equal-work add/remove pairs
must be grouped before assigning their large deltas to a runtime change. The
summed capability-owner analysis includes both sides. It is not byte equivalence.

Simulated Bim improves in five cases, including -15.167% unary and -8.061%
concurrent, but regresses +1.555% pipeline. Bcm regresses +0.622% pipeline,
while improving in the other five cases. These results do not repair the eight
caller instructions introduced by the earlier Request change, and do not
establish native latency or whole no-allocator RPC completion.

Verified artifact 12594059851: 81,839,819 bytes, SHA-256
`c660c5a1f83dccbbc412ef227f8a58125324319a590dbed0a5cb730a6522a3d0`.
Raw profiles and captured assembly/symbols remain in that archive. Selected
function attribution reconciles every whole-program delta.


function-name-analysis.json performs that narrow default-type spelling grouping
on every raw function sample, conserving each whole delta. It identifies deep
Ir +63.080 and Bc +63.600 in memcpy, plus +5.120 in memset for both metrics;
these dominate the residual +63.160 Ir / +68.080 Bc after rename pairs cancel.
This is attribution of work, not proof about native latency or copy volume.
Reproduce by running canonicalize-promises.py on the unpacked verified archive,
which contains the complete functions.json; the smaller selected-functions
file is insufficient when large rename pairs displace other top changes.

# Actual legacy RPC without std (still allocating)

The alloc feature now compiles legacy RpcSystem, its connection/message/client hooks,
generated owned services, and the optional portable runtime without std. OS adapters,
Unix descriptors, implicit TLS local-client identity, membranes, segment caches and the
default clock remain std integrations. No selection of the bounded driver is involved.

The explicit ClientRegistry holds weak per-interface registrations and is retained by
its client states. All local views of a server must share that registry. This keeps
identity reuse and revocation conflict checks without requiring a global or thread local.
The hosted registry, collection and mutex types remain canonical. The allocator-only
profile uses hashbrown and futures' existing spin support; scoped wake mailboxes use
the same spin backend and release it before invoking a Waker. Platforms require
pointer-width atomics. Segment caching is absent, including its portable public options.

Local checks: five isolated no_std service/conversation tests pass, covering unary,
retained responses, depth-32 dependent access and pipelining, 32 simultaneous requests,
reverse callbacks, early cancellation and continued connection use, identity deduplication
and revocable alias rejection. The same fixture checks on thumbv7em-none-eabi. An earlier hosted
all-target Clippy pass and final alloc-only library/fixture Clippy pass with warnings denied;
core-only M4 checks and code-generator unit tests pass. This is not board execution,
an allocation-free link, encrypted transport testing, or native latency measurement.
The final hosted all-target Clippy retry was killed by workspace memory exhaustion
(cgroup OOM kill rose 14 to 15), so CI must recheck the final revision.
Hosted repeated profiles and two-width Miri will qualify an immutable source revision.

The remaining no-alloc blockers are real ownership interfaces: Box<dyn ClientHook> and
consuming boxed request/results hooks, Rc-backed connection/question/pipeline owners,
Vec capability/message/export storage and heap-owned erased futures. The read/output,
admission, task and pending providers are available, but those providers must be wired
into these actual protocol owners before the no-alloc target is complete. A core-only
facade check or bounded-driver conversation is insufficient evidence of completion.

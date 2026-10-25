# Owned legacy invocations enter generated service dispatch without boxing

CallContext and PreparedCall now transfer their original storage through
into_owner/into_owners. dispatch_call and dispatch_call_with_context retain those
owners inside the returned future and invoke the existing generated wire service
bindings. The result field is explicitly reborrowed for each child method's
lifetime; attempting to lend a local parameter owner for the original output
field's full lifetime correctly failed to compile. No unsafe lifetime extension,
self reference in a connection record, allocation fallback or new erased vtable
is introduced. Input owners remain alive across every method suspension.

The future consumes a concrete service adapter. Borrowing one shared mutable
dispatcher for an entire suspended call would serialize access through that
adapter. Per-call generated dispatchers can instead own a borrow of common
service state. A statically dispatched Service/ContextService implementation for
&mut S retains the exclusive-borrow option without requiring it for every call.
The default hosted Server/ClientHook path remains unchanged.

The isolated verification/legacy-fixed fixture generates inherited Base/Node
bindings and instantiates no bounded or hosted protocol driver. Five tests add
zero blocks and bytes in both fixed and alloc profiles: inherited-method input
retention with a real static Waker; cancellation destroying the generated method
before its parameter owner; independent application context; invalid interface,
method and parameters rejecting before application entry/result edits; and 32
concurrent suspended invocations through one shared service. The concurrency
case verifies peak/live count 32 before completing any method and validates all 32
real Return payloads afterward. Adapters remain owned by caller-pinned task
places, so no frame/context stores a future borrowing itself.

The host binary executes all five cases. Its M4 ELF links without allocator or
undefined symbols (embedded-link.json); it has not executed on a board. These
are dispatch/ownership proofs, not the complete legacy connection conversation
matrix. Connections still own admission, cancellation/streaming protection,
Return publication and all capability/pipeline/protocol storage. Native latency
and complete hosted compiled-work qualification remain pending.


Local verification also passes all 49 no-default core tests, 125 wire tests,
14 documentation tests (4 ignored), the nine actual alloc-only legacy conversation
fixtures, core/RPC library Clippy, isolated fixture all-target Clippy and formatting.
The local compiler reached the container memory limit while rebuilding a wire
test; removing 219 MB of obsolete generated compiler cache let it finish without
an OOM kill. All reports, archived evidence, source and link probes were retained.
This environment supplies no native performance measurements.


Qualification 5b485980227be7edc40f61c91bc441f188365a86,
run 40193516046, passes 38 strict-provenance Miri executions (20 new fixed/alloc
cases across both widths, 18 actual legacy conversations). Its verify job fails
at the M4 Cargo binary link: cargo-auditable injects the host flag
-Wl,-u,AUDITABLE_VERSION_INFO into direct rust-lld. Source and local M4 evidence
are intact. Workflow-only qualification 69a0b8d97733cb6993e6c957b4389b6e0fb8d5fa,
run 40194606842, keeps auditable hosted/library builds and uses the established
direct-rustc bare-metal probe link. Miri is not repeated for that identical source;
its earlier 38 executions remain the memory-safety evidence. Hosted/work checks
continue in the new run. No failed hosted/profile qualification is called a pass.

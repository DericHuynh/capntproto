# Concrete legacy clients create caller-backed requests

The public legacy Client<H> retains a concrete capability owner in every profile.
ClientStorage<P,R> selects request storage with a GAT tied to that owner's borrow;
try_new_call returns the existing Request<P,R,H> and publication still consumes
its original owner through RequestStorage. Internal records retain validated keys,
not clients borrowing the records' own context. No box, static lifetime or heap
fallback is required. Clone follows H's actual retention policy; the hosted
Box<dyn ClientHook> Clone already delegates to add_ref.

Hosted generated clients retain the default boxed hook and their original direct
new_call/new_call_with_hints paths. The optional fallible construction adapter
also works for that default. Concrete clients do not yet replace the hosted
connection's capability table, generated owned server dispatch or pipeline graph.

Three allocation-counted real-wire tests add zero blocks and bytes. They cover
cloned borrowed clients contending for one fixed request guard; parameter growth
failure preserving Call headers, hints and prior payload; publication rejection
returning the original buffer for retry without assigning an ID; and generation
revocation both before call construction and before sending an already prepared
request. Drop releases unsent guards without publication. The caller provider,
not Client itself, owns authentication, generations, output admission and IDs.

Local checks pass 49 core and 125 wire no-default tests,14 documentation tests
(4 ignored), nine actual alloc-only legacy conversations, core no-default library
Clippy, wire no-default all-target Clippy and RPC alloc-only library Clippy.
Core no-default all-target Clippy was an invalid broader check: an existing
alloc-dependent deep_copy_bounds integration target cannot use new_default in
that profile. The required no-default library check passes; default all-target
Clippy remains part of CI. Four report-parser tests reject false owner aliases.

The host probe executes borrowed Client -> concrete Request -> CallContext ->
caller-pinned suspended invocation -> real Return, validating both wire headers
and payload. Its M4 ELF has no allocator/undefined symbols (embedded-link.json).
This is not the complete connection/conversation matrix and has not run on a
board. Full hosted correctness, strict-provenance and repeated compiled-work/
allocation qualification are pending. No native timing is collected locally.


Qualification `3cd54685c5313669afca020986c6c7c48a864ca9`,
[run 40192177129](https://github.com/DericHuynh/capntproto/actions/runs/40192177129),
passes 30 strict-provenance Miri executions: three client cases across both fixed/
alloc profiles and pointer widths (12), plus nine actual legacy conversations on
both widths (18). Hosted validation passes; repeated profiles remain running.

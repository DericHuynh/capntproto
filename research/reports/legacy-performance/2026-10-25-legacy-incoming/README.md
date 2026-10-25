# Legacy incoming frames with concrete parameter and response ownership

IncomingMessage is now available in the actual RPC crate without std or alloc.
Only its owned network conversion and descriptor-transfer operations require
alloc/std. Reader<S,B> can serve as a concrete incoming frame with its existing
segment owner and traversal allowance. No bounded protocol driver is selected.

incoming::Params<M,C> and Response<M,C> retain the original whole frame and
capability bindings without prescribing Box, Vec, Rc or static lifetime. The
hosted private Params type aliases the same owner with its original Box<M> and
Vec of capability hooks; its frame/capability fields remain flat with the same
destruction order. Hosted ResponseState keeps its existing flat storage and
borrows the common result-selection operation. There is no nested owning record
or new erased-dispatch layer. Concrete owners and bind operations are statically
dispatched; optional borrowed trait objects remain an application choice.

Capabilities=() leaves wire indices unbound and grants no callable authority.
The driver must still admit/resolve descriptors in the original message context.
Hosted Vec bindings perform the existing capability imbue. Borrowed and boxed
IncomingMessage forwarding includes checked/cached get_params/get_results and
cache methods, so an optimized transport does not fall back to envelope parsing.
Hosted malformed-envelope error strings are retained; fixed errors carry the
same error kind without allocating a string.

Four incoming tests pass without alloc. Three measure zero allocated blocks and
bytes while retaining frame/capability destruction order, forwarding cached
borrowed transport views with zero get_body calls, and preserving one external
read budget through repeated owner extraction/reassembly. The fourth rejects
wrong envelope/Return variants. An additional hosted boxed-cache test allocates
its box before measurement and requires zero allocation/redecoding on access;
that hosted test requires CI in this constrained local environment.

The isolated legacy-fixed fixture now depends on the actual no-default RPC crate
and replaces its custom ParamsHook with IncomingMessage + incoming::Params. All
five generated-service cases pass with zero blocks/bytes in both fixed and alloc
profiles, including suspended input retention, real wake, cancellation ordering,
independent context, invalid calls and32 concurrent methods. Nine actual alloc-only
legacy conversations also pass. No-default RPC all-target Clippy, alloc library
Clippy, fixed fixture all-target Clippy, formatting and five owner-name parser
tests pass. The parser only equates the exact hosted Params type alias; borrowed
or differently bound frame owners remain distinct.

The host binary executes all fixed cases; its M4 link has no allocator/undefined
symbols (embedded-link.json). This is frame/dispatch integration, not a complete
allocation-free connection conversation matrix or board execution. Full hosted
compiled work, allocations, strict-provenance Miri and hardware latency remain
unqualified until the immutable checks. Mandatory outgoing-message, capability/
pipeline hook and connection resource ownership still need integration.

# Legacy question records with concrete storage owners

The actual legacy QuestionRef now stores the engine's Reference<C,K,F,T,P>.
Its default context/producer/reply/pipeline types remain the existing Rc,
fulfiller, Promise and weak pipeline owner. The common record instead permits
connection/pipeline keys, borrowed producers and futures retaining independently
owned frame storage. It requires no allocator or static lifetime and adds no
new dynamic dispatch. Hosted drop still owns last-interest protocol cleanup;
dropping the data record alone cannot authorize Finish or wire-ID reuse.

The record is deliberately not a public lease borrowing its containing context.
Caller-backed drivers put keys inside records and retain borrowing finalizers
outside them. Pipeline replacement returns the old owner so it can be destroyed
after releasing the mutable record borrow. Detaching the connection does not
cancel pending completion or destroy retained frames; producers are taken once.

Two zero-allocation tests cover context detachment while reply/pipeline/producer
interest remains, independent destruction and retained response validity, and
cancellation/reuse of a borrowed external producer slot on record destruction.
All 226 no-default engine tests, engine all-target Clippy, alloc-only RPC library
Clippy and nine actual allocating legacy conversation fixtures pass locally.

The host probe executes keyed question storage, borrowed response waiting,
retained external bytes and deferred fixed Finish publication. Its M4 ELF links
without allocator or undefined symbols (embedded-link.json). This is not the
complete connection/conversation matrix or board execution. Full hosted work,
allocation and strict-provenance qualification remain pending. The hosted record
still uses Rc/Weak; this factors its ownership contract without claiming the
whole legacy driver is allocation-free. Generated dispatch, capability hooks,
message storage and protocol tables still require caller-backed integration.


Qualification `e828d3ec633bff323b599b551c97526cb8472096`,
[run 40191032409](https://github.com/DericHuynh/capntproto/actions/runs/40191032409),
passes 26 strict-provenance Miri executions: two record cases across both profiles
and pointer widths (8), plus nine actual legacy conversations on both widths (18).
Hosted validation passes; repeated work/allocation profiles remain running.

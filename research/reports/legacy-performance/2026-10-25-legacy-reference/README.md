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
Full hosted validation and all 240 profile trials pass. The report retains every
five-sample mean, sample SD and paired delta. TLS blocks remain 7/57/202; full byte
and QUIC allocation samples remain in comparison.json.

This extraction regresses actual compiled work. The new nested owning Reference
has a separately generated destructor: concurrent self work adds 864 Ir there,
while QuestionCell::drop_slow saves 320 Ir. The combined question/scheduler group
therefore adds 544 Ir and 32 conditional branches per 32-call batch (paired SD zero),
or 17 Ir/one branch per question. Indirect instruction counts remain equal.
The narrow parent-only group would have hidden this cost. analyze.py includes
both the parent and core reference record. Normalized assembly matches 662 of 714
common bodies; two new selected symbols are the nested reference destructor.

Whole Ir rises 0.016–0.121% across all six cases. Simulated Bim regresses in unary
(+4.676%), concurrent (+37.468%), callback (+6.982%) and deep (+11.275%); dependent
and pipelined reads improve slightly. Conditional misses also regress in four
cases. These remain performance gaps, not a parity pass. A follow-up keeps the
hosted fields flat and shares their operations through borrowed projections;
its cost recovery needs its own qualification.

Raw artifact 12598389987 is 82,921,938 bytes, SHA-256
1055cce8704ce7b3b46b5ef240cc8d8b7295b238862b5e82426a47c6309f0040.
Native latency has not been measured.

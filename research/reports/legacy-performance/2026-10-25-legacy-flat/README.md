# Flat hosted question fields with shared borrowed projections

The owning Reference extraction adds a generated destructor boundary to the
hosted QuestionRef. Run 40191032409 traces +17 Ir and one conditional branch per
question, or +544 Ir / +32 Bc per 32-call batch (paired SD zero), despite unchanged
allocation counts. The raw regression remains in the reference report.

Hosted QuestionRef now keeps its original five owning fields flat. View/Parts
project borrowed fields into the same core operations used by caller-backed
Reference storage. Projections own no field, protocol interest, destructor or
vtable. Returning detached context/producer/pipeline owners lets callers destroy
them after ending table borrows. Last-question cleanup still delegates to the
qualified release policy; its context/key and destruction order are unchanged.
The generic owned record remains available to fixed providers. This avoids
imposing its physical destructor boundary on an existing hosted record.

All 226 no-default engine tests and the nine actual alloc-only legacy conversation
fixtures pass locally, including cancellation, fragmentation, callbacks and deep/
concurrent pipelines. Engine all-target Clippy, RPC alloc-only library Clippy and
formatting pass. The borrowed-record host probe executes, and its M4 ELF has no
allocator/undefined symbols. That component ELF is byte-identical to the earlier
record probe (same SHA-256); it does not prove hosted destructor code generation
or whole-driver performance. Recovery of the 17 Ir/question cost must be measured
in the immutable hosted qualification. No native timing is taken locally.

This is a field-layout correction, not completion of the full allocation-free
legacy connection. Protocol/hook/capability/pipeline/message integration and the
full fixed conversation matrix remain pending.


Qualification 3a96b622243d040edb96366c4e9949f4971add26,
run 40195152868, passes 26 strict-provenance Miri executions, complete hosted
verification and all 240 profile trials. It recovers exactly 17 Ir/one Bc per
question in all five samples: -17/-1 unary, -34/-2 dependent/pipelined/callback,
and -544/-32 concurrent/deep, all with paired SD zero and unchanged indirect
instruction counts. The nested-reference destructor symbols disappear and the
original fields remain flat; 663 of 715 common normalized bodies match.

TLS allocation counts remain 7/57/202 with full byte/QUIC spreads retained.
Whole Ir improves five cases, while dependent adds 0.235 with paired SD 83.137.
Concurrent whole Ir falls 556.480 (SD 25.250), deep falls 485.475 (SD 39.147).
Conditional misses improve 7.580% concurrent and 29.152% deep, but regress in the
other four cases. Indirect misses regress in unary/dependent/pipelined/concurrent
and improve callback/deep. Deep whole conditional branches still add 28.450
(SD 6.653), despite the scoped 32-branch saving; libc/memory/system work and linked
predictor effects remain part of the report. No complete native parity is claimed.

Artifact 12600348459 is 83,057,643 bytes, SHA-256
24e31192a90abb0b1d47d237f26b7e42e709e47cc37cdb0f88cf3bc13f79481e.

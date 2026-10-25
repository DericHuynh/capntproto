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

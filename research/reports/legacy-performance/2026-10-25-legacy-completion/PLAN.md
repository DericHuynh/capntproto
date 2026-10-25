# Completion target: legacy without std or an allocator

The target is the legacy driver using the same extracted algorithms under hosted and caller-backed ownership. A no-default build that merely selects the preexisting portable driver does not complete this work. API compatibility is not required. Default hosted behavior must retain measured instruction/branch work and allocations, with deviations traced and preserved.

Completion requires a no-std/no-alloc legacy RPC path exercising unary, dependent, pipelined, concurrent, callback and deep fixtures, including cancellation, partial IO, retained results and capacity rejection. Caller-provided IO/services choose their own allocation policy; tests and link evidence must distinguish their behavior from the driver.

## Work in progress

- Qualified base: `4888f89215b92e88b163bb0be6ceb0985d7f1de3`, readiness notifications extracted with exact component work parity.
- Admission: shared core transitions with sealed fixed-ring and hosted VecDeque backing; borrowed endpoints preserve ownership on full/closed rejection. Hosted adapter now delegates directly. Qualified by run 40158026841: 853 test executions, exact combined component Ir/Bi parity; branch prediction/layout deltas retained in the admission report.
- Task storage: caller-pinned owned future slots (including !Unpin), explicit admission failure, immediate in-place destruction and the same ready polling/retirement algorithm as hosted legacy. Local engine tests and hosted checks pass; Qualification run 40161615453: strict-provenance Miri passed for both pointer widths; hosted work profiles pending.
- Wake ownership: implemented static notification identities with standard Wakers, containing no task pointer; old escaping wakes can only poll a replacement. Small audited core-only capntproto-task crate owns raw-waker construction and pin projection. Engine remains unsafe-free. Qualification run 40161615453: strict-provenance Miri passed for both pointer widths; hosted work profiles pending.
- Scheduler: shared ordinary/deferred/direct-connection policy now drives both hosted fields and caller-backed providers. Bounded admission retains unpolled commands until capacity is available and reschedules on capacity release; separate stop completion bypasses saturation. Fixed empty notifications and sentinel-free restart policy are implemented. Local zero-allocation tests cover saturation, immediate cancellation destruction, external consumer turns and starvation bounds. Owned task places support safe two-word heterogeneous future erasure through normal Rust trait objects, without manual vtables. Qualification pending.
- Protocol/hook/message storage: eliminate mandatory Box/Rc/Vec ownership from the legacy core, preserving retained frames, pipelines, cleanup publication and ID reuse. Pending.
- Profiles/API: make legacy's core available in the no-default profile, separate allocating/OS adapters, and remove any need to select another protocol driver. Pending.
- End-to-end proof and repeated hosted regressions: pending.

Each qualification pins an immutable commit. Later implementation work can proceed while that prior commit is tested. Reports must preserve the exact source/fixture identity, all five samples and spreads, allocations and bytes, and remaining native-latency limitations. No main merge is implied.

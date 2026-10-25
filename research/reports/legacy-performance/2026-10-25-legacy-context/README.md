# Per-call application context is owned inside its dispatch task

The context-aware dispatch adapter now accepts an owner implementing the standard
BorrowMut<C> contract. It can own a per-call context value or pool guard, while
existing borrowed-context handlers receive the same mutable view. This removes
the requirement to retain a mutable context guard beside a suspended future.
No box, custom vtable, static lifetime or allocation fallback is introduced.

A single private flat resource capture retains input, results and context in that
declaration order. A whole-aggregate borrow method prevents disjoint async capture
from splitting that ownership. The aggregate is assembled before creating the
future, so even unpolled drop releases input first, results second and context
last. Metadata lookup and application entry remain lazy until polling. Context
and task/frame storage remain independently retained; records cannot contain a
future borrowing their own containing context.

The seventh isolated legacy-fixed test owns all three pool loans inside a pinned
task. Completion, suspended cancellation, unpolled drop, output-capacity failure
and invalid-method rejection release them in order, once. Context cleanup asserts
that input and result cleanup already ran, while result cleanup checks that the
actual input buffer is mutably borrowable. Completion releases the context before
the task handle is dropped; an interrupted method leaves its value unchanged.
All seven fixture tests add zero blocks and bytes in fixed and alloc profiles;
the existing borrowed-context case still passes, along with the prior32 concurrent
calls and real wake/cancellation cases.

Local fixture all-target/core library Clippy, nine actual alloc-only legacy
conversations, formatting and host execution pass. The M4 ELF has no allocator or
undefined symbols (embedded-link.json), but is not board execution or the full
connection conversation matrix. Full hosted work/allocation comparison and Miri
remain pending. Outgoing publication and complete caller-backed connection/
capability/pipeline integration still prevent claiming an allocation-free RpcSystem.

# Pooled invocation guards stay inside the pinned dispatch future

The previous dispatch adapter required a result field whose pool guard lived
outside its task. Such a guard cannot be moved alongside a field borrowing it
without making a self reference. DispatchResults instead lends a fallible view
from the concrete result owner after the task is pinned/polled. The future now
owns that guard and the original parameter owner; no allocation or unsafe
lifetime extension is introduced. Fixed fields and borrowed/RefMut providers
retain the same static dispatch option.

PreparedCall::borrow_owners lends disjoint parameter/result views while retaining
both original owners in their original aggregate. The adapter no longer splits
them into unrelated async locals: PreparedCall's declared input-before-output
destruction order now applies on completion, error, cancellation and unpolled
drop. Generated method futures are destroyed before either storage owner.
ReaderSegments also accepts immutable/exclusive RefCell loans so a reader can
retain its input pool guard across suspension without copying bytes.

The isolated legacy-fixed fixture adds one test covering four exits: successful
completion, pending cancellation, one-word output rejection, and dropping a task
before its first poll. Both input and output guards live inside caller-pinned
task storage. Result cleanup asserts that the input owner has dropped and its
actual pool slot is already mutably borrowable. Output storage becomes reusable
at completion even before the task handle is dropped; both slots are free after
all exits. Unpolled/rejected calls do not enter application code. Successful real
Return headers/payloads are validated. All six fixture cases add zero blocks and
bytes in fixed and alloc profiles, including the prior32 concurrent calls.

Local checks pass49 no-default core tests,14 documentation tests (4 ignored),
four no-default incoming tests, nine actual alloc-only legacy conversations,
core library and fixture all-target Clippy. Host execution passes all six cases.
The M4 link has no allocator/undefined symbols (embedded-link.json), but has not
executed on a board. Full hosted work/allocation qualification and strict-
provenance Miri remain pending. This removes a pooling ownership blocker; it
still does not instantiate the complete allocation-free connection driver or
prove native latency/CPP parity.

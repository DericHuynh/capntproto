# Shared legacy last-question release and resumable Finish publication

The actual legacy QuestionRef::drop now delegates its protocol ordering to the
core question::release policy. The statically dispatched connection provider
releases local interest, publishes Finish outside table borrows, removes the
record only after publication, and schedules idle detection. A single remove
replaces find followed by erase; the removed record leaves the table borrow
before destruction. This extraction adds no dynamic dispatch layer or new
unsafe code and does not change hosted message/connection ownership.

The same policy accepts a borrowed caller-owned context. Last retains its owner
borrow and original key without a box or reference count. A key need not Copy;
providers validate current generation and once-only last interest. Disarm
transfers teardown authority back to the owner. Internal records retain keys
and effects, never their own public borrowing finalizer.

A fixed provider can return Deferred after retaining Finish and the eventual
reclamation obligation in its existing control/question storage. Resume retries
that effect after output capacity becomes available; no transport polling loop
or allocation fallback is introduced. The ID stays occupied while publication
is deferred. A Return during deferred cancellation must update that retained
obligation instead of recycling the unpublished ID. The caller supplies this
storage and wake/output-driver policy; this helper does not provide a complete
fixed protocol driver or make arbitrary provider code allocation-free.

Six allocation-counted tests cover publication before reclamation, cancellation
before Return, repeated output-capacity rejection, Return during deferred
cancellation, generation replacement during transport entry, duplicate/stale
release and disarming, and noFinishNeeded. They use fixed cells/arrays and check
reentrant borrow availability and wire-ID occupancy; all add zero blocks/bytes.
All 220 no-default engine tests, engine all-target Clippy, RPC alloc-only library
Clippy and the nine actual alloc-only RPC conversation fixtures pass locally.

The host probe executes deferred Finish creation, retains its ID until output
acceptance, and validates the resulting real wire envelope. The M4 ELF links
without allocator/undefined symbols; embedded-link.json records hashes/scope.
It has not executed on a board or completed the full conversation matrix. One
local M4 build was killed at the container memory limit; clearing generated
compiler/test cache allowed the single-compiler retry to pass. No native timing
is taken in this environment.

Full hosted work/allocation checks and two-width Miri require the immutable
qualification against the preceding request-codegen source. This factors the
legacy protocol cleanup itself, but its hosted QuestionRef still uses Rc and its
transport still uses boxed messages. Whole no-global-allocator connection,
capability/pipeline storage and generated dispatch integration remain pending.

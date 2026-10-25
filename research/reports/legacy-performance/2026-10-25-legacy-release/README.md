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


Qualification `ac011c440c0ad700c65cbe059a5f73671145697a`,
[run 40186576383](https://github.com/DericHuynh/capntproto/actions/runs/40186576383),
passes 42 strict-provenance Miri executions: 24 cleanup cases across both widths
and fixed/alloc profiles, plus 18 actual legacy conversation cases across both
widths. Full hosted verification and all 240 profile trials pass. The five-repetition
comparison retains means, sample SD and paired deltas. All six whole-program Ir
means improve by 0.037–0.203%; simulated Bim improves by 7.185–24.219%. TLS
allocation counts remain 7/57/202 for unary/pipelined/concurrent cases; byte and
QUIC allocation samples remain in comparison.json.

The concurrent trace attributes exactly -1,344 Ir and -192 conditional branches
per batch (paired SD zero) to halving LocalTable<Question>::get_mut work. The
QuestionCell drop path adds 64 conditional branches (SD zero), so extracting only
the question-lifetime helper group would miss the lookup saving. Whole-program
concurrent Ir falls 1,363.280 ± 23.854 (paired sample SD), and conditional branches
fall 126.755 ± 4.706. The normalized disassembly comparison finds 656 of 708 common
bodies unchanged, with no added/removed selected symbols. It is not byte equality.

Conditional branch misses regress in unary (+0.666%), concurrent (+18.168%) and
deep-concurrent (+7.549%). These are real simulated regressions; removing a lookup
does not establish native speed or branch-prediction parity. Native latency has
not been measured here. The complete raw archive remains GitHub artifact
12597591936: 82,991,732 bytes, SHA-256
7383fd179b0c92ff3f97a75604cba2a9d6daa941d1c45dcfe838e1fe1a5969e9.
Selected attribution rows reconcile to whole-program deltas; assembly normalization
and symbol scope are recorded separately.

# One legacy response receiver for owned or borrowed questions

The actual legacy QuestionReply is now an alias to completion::direct::Receiver.
Its default owner is the same Rc<QuestionCell>, retaining Option-owner and done
fields plus a zero-sized marker. The duplicate private poll/cancel wrappers are
removed. The same receiver accepts a caller-backed lease that dereferences to
existing reply storage, without a required box, reference count or static lifetime.
It uses the same direct poll/release operations; no new unsafe code or hand-written
vtable is added. Borrowed trait-object storage is also supported without boxing.

Pending cancellation first removes and destroys the response future/waker outside
reply borrows, then drops the question lease and invokes its protocol cleanup.
Completion releases question interest immediately, so response values must retain
their own frame/capability leases. into_owner transfers single observation into
another adapter without cancellation. Cloning an owner for pipeline/protocol
interest does not authorize another receiver. The provider enforces that contract.
A Last::owner accessor lets a borrowed lease use its retained context without
storing a duplicate pointer. Last still validates through its context provider.

Four zero-allocation tests cover real static Waker notification and retained
external output after question release; in-place cancellation destruction before
last-interest callbacks and parameter-guard release; adapter transfer for success,
error and rejected late delivery; and borrowed erased storage without allocating
a wrapper. All 224 no-default engine tests, 12 documentation tests including
borrow-escape rejection, engine all-target Clippy, RPC alloc-only library Clippy,
and the nine actual alloc-only RPC conversations pass locally.

verification/portable/legacy-receiver-link.rs executes borrowed response waiting,
retained external bytes, deferred Finish encoding and ID reclamation on the host.
Its M4 ELF links without allocator/undefined symbols; embedded-link.json records
the exact scope and hashes. This is a response/control ownership probe, not a
complete RPC connection or the required full conversation matrix, and has not
executed on a board. The existing bounded protocol driver is not selected as a
replacement for legacy.

Full hosted instruction/allocation profiles and two-width strict-provenance Miri
require the immutable qualification. Native latency remains restricted to dedicated
droplets. Hosted QuestionCell, pipeline/capability/message storage and generated
dispatch still retain allocating ownership. Factoring their receiver removes a
contractual Rc requirement; it does not yet make the complete legacy driver
allocation-free or establish compiled-work/native performance parity.


Qualification `9950969c0c140032667e5f1822c0c354adbd9aa8`,
[run 40189088815](https://github.com/DericHuynh/capntproto/actions/runs/40189088815),
passes all 34 strict-provenance Miri executions: four receiver cases in each
fixed/alloc profile and pointer width (16), plus nine actual legacy conversations
on both widths (18). Full hosted validation and all 240 profile trials pass. Question-lifetime plus
scheduler Ir/Bc/Bi slopes are identical in every one of five samples after
including both the old QuestionReply and concrete Receiver names. The apparent
+480 Ir/-480 Ir function pair in the concurrent trace is exact drop-glue renaming,
not additional work. owner-name-analysis.json reconciles only the exact hosted
alias; borrowed and nondefault receiver types remain distinct. The reproducible
canonicalizer is verification/legacy-alloc/canonicalize-owner-types.py.

All 708 common normalized assembly bodies match; six removed and six added
selected symbols name the concrete reply alias instead. Reconciling their exact
alias definitions also matches all714 normalized bodies; no unmatched selected
symbols remain (owner-assembly-analysis.json). This normalization
compares offsets/instruction forms, not complete binary bytes or native timing.
TLS allocation counts remain 7/57/202. Full byte/QUIC samples are retained.

Whole-program means vary by -0.038% to +0.015% Ir. Deep Ir rises 46.410 with paired
sample SD18.962; libc memcpy accounts for +62.880 ±0.084 Ir and +64.570 ±0.224 Bc,
while memset falls 7.650 ±2.795 Ir. Common helper equality does not erase these
end-to-end differences or prove their cause. Simulated conditional misses rise
0.595% in pipelined reads and0.006% in deep; tiny Bim increases in three cases also
remain in the report. Native latency has not been qualified.

The raw archive is artifact12598266131, 82,866,301 bytes, SHA-256
1af8dd63d9f2e5ed7213498afa989a5bf9a50f7720a642fa51b84df16d589cac.

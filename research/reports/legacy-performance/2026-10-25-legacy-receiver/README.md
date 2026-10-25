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

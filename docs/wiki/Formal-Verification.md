# Formal verification

Use Kani for local Rust state, integer bounds, memory operations and failure
atomicity. Keep TLA+/TLC for distributed ordering, fairness, progress and protocol
composition. Keep compile-fail contracts for borrowing and API roles, Miri for
executed aliasing/ownership paths, allocation counters for the no-alloc contract,
and native/C++ differential tests for implementation compatibility. These checks
cover different properties; none substitutes for all the others.

## Portable engine proofs

The [proof inventory](../../verification/kani.json) pins **Kani 0.68.0**. Its
release bundles **CBMC 6.11.0** and Rust **nightly-2026-08-21**. This verifier
toolchain is separate from the project's native Rust 1.97 toolchain. Kani is a
development tool, not a dependency or feature of the deployed RPC engine.

```sh
cargo install kani-verifier --version 0.68.0 --locked
cargo kani setup
cargo fetch --locked
cargo run --locked -p capntproto-dev -- kani
```

The [CI workflow](../../.github/workflows/verification-models.yml) verifies the
Linux x86-64 release bundle checksum before setup. The runner verifies the exact
tool version, every inventoried harness and the named assertions in its mutation
counterexamples. Cargo/Kani runs offline, and hashes include the lockfile,
engine/core sources, proofs, runner and proof plan; changed inputs invalidate success.
Kani does not expose Cargo's `--locked` flag, so the surrounding command checks
the lockfile as part of these inputs. Do not disable unwinding, reachability,
overflow or pointer checks to make a proof pass.

| Property | Scope of the proof |
| --- | --- |
| Caller-owned reply completion | Arbitrary `u8` value, producer cancellation, receiver closure before/after delivery, once-only result recovery and reuse. A closed receiver cannot accept a new value; an accepted but undelivered value blocks reuse until recovery. Waker registration/reentry and generic destructors are covered by native tests and Miri, not this synchronous bounded proof. |
| Tracked reply ownership | Arbitrary `u8` value, receiver closure, duplicate delivery, normal consumption and producer shutdown. Cleanup cannot take a live receiver's value, shutdown returns an unconsumed owner exactly once, and an already-consumed owner cannot be reclaimed again. Waker notification and wire receipt cleanup are separate native/Miri checks. |
| Shared reply-slot claims | Arbitrary `u8` value and consumption/abandonment. A shared storage reference cannot start a new reply while either endpoint or its unique delivery observer remains accessible, including after consuming/recovering the prior value. After both endpoints drop, the next reply sees only its own value. Panicking wake hooks are covered separately by native unwind tests. |
| Terminal owner transfer | One slot per ledger with retained imports, export leases and answer pipelines; each owner transfers once after consuming live ledger authority. Two graph nodes and one waiting task; endpoints remain attached until tasks drain. One partially written send; only failed output can discard it, and an independent reader keeps its bytes. These bounded proofs do not cover the whole wire-driver cleanup sequence, which is exercised by native tests and Miri. |
| Handoff admission, draining, revocation and direct calls | Initial state plus one-step induction over every state satisfying the stated invariant, with arbitrary full-width `u64` counters. Preserves drain conservation, no counter wrap, embargo, rejection atomicity and phase/authority guards. |
| Local free-ID heap | Initial state and inductive transitions for three slots, with arbitrary values and full-width input IDs; lowest-vacancy allocation, duplicate removal, occupancy/heap consistency and domain isolation. Not a proof for every capacity. |
| High question IDs | Two sparse slots; arbitrary starting cursor including wraparound, live-ID collision, capacity failure and rollback. |
| Adopted questions | Arbitrary `u32` ID, independent storage ranges, invalid/duplicate/full rejection and original owner preservation. |
| Peer answer storage | Arbitrary `u32` keys, two inline slots and one sparse slot; bounded rejection, replacement and removal. |
| Question lifecycle | All eight-operation histories from ordinary/no-return construction; at most one Finish, correct result-cap release and no early reuse while Return is outstanding. |
| Fixed streaming credit | Arbitrary `u128` send sizes and `usize` windows, overflow rejection, both acknowledgement orders and credit conservation. |
| Adaptive clock rejection | Arbitrary sizes and full-width seconds; a backwards clock cannot release credit or change the window. This is not a full adaptive-controller or timing proof. |
| Fixed arena reservations | One to four words of aligned byte storage, two arbitrary full-width `u32` segment IDs and reservation sizes, then reset/drop. Checks exact admission, disjoint advancement, preservation of live bytes, zeroing and unchanged unused storage. Not arbitrary capacity or a proof of the wire encoder. |
| Fixed segment publication | One to four reserved words, arbitrary byte content, then consuming publication. The initialized segment remains intact and unused capacity is unchanged. A deliberate destructor-clearing fault must produce a counterexample. |
| Fixed arena rollback | One to four words of aligned byte storage; arbitrary valid published-prefix size and full-width reservation attempt. Rollback restores the cursor, preserves the prefix, zeroes discarded bytes, leaves unused storage untouched and permits exact-fit reuse. Callers must prove the discarded suffix is unpublished and unborrowed. |
| Export reference accounting | One-step induction over every `u32` count and retain/release amount. Rejected overflow and over-release preserve the count; releasing the last reference retires the export, and terminal exports cannot be revived. Local owner detachment and wire-ID generation are outside this proof. |
| Import cleanup acceptance | Every `u32` count and received amount; overflow rejection, exact pending-release counts, unchanged state when a ticket is dropped, and consumption only on explicit commit. Does not prove local-handle ownership or actual queue admission, delivery or connection cleanup. |
| Bounded import lifecycle | One slot and arbitrary full-width initial `u64` generation and wire ID. Local aliases keep ownership live; final release invalidates keys, rejected cleanup retains the slot, revival/reuse rejects stale keys, exhausted generations do not wrap, and accepted cleanup returns the owner. Connection-scope isolation is checked separately by compiler contracts. |
| Bounded import count admission | Inductive transition over valid arbitrary `u32` local/remote counts and `u64` generation, including pending cleanup. Overflow/generation rejection preserves both counts, generation and owner; success admits exactly one local and remote reference. Not a proof of an entire connection or arbitrary operation histories. |
| Atomic import batch | One slot, two repeated descriptors and arbitrary valid full-width reference counts/generation. The whole batch is admitted exactly when both grants fit; rejection preserves all counts, the generation and the owner. Native tests additionally cover new owners, multiple IDs, tombstones, capacity failures and forgotten preflight tickets. |
| Bounded export lifecycle | One slot, arbitrary full-width starting generation and message acceptance. Commit publishes all reserved grants; abort leaves the owner inaccessible to wire lookup. Final peer release invalidates keys while a local call lease retains ownership; lease completion detaches the owner. Reuse rejects stale keys and never wraps exhausted generations. |
| Bounded export reservation | Inductive transition over all valid full-width published/pending `u32` counts for a live or newly staged export. Reservation overflow preserves counts; commit publishes the reserved total, and abort preserves the old published count or retires the unpublished owner. Descriptor serialization, output acceptance, scope isolation and arbitrary-capacity histories are outside this proof. |
| Descriptor multiplicity | Two export slots, zero to two reservations per slot, and up to three arbitrary full-width wire IDs. Validation succeeds exactly when the decoded multiset matches all reservations, including when record errors are ignored. Reserved counts remain unchanged. This proof supplies IDs directly; it does not prove the RPC decoder or the enclosing publication transaction. |
| Descriptor count admission | Arbitrary full-width `u32` reservation and observed counts, with a symbolic prior rejection. Recording one ID cannot overflow; excess counts preserve scratch and poison the check, and finish requires the exact reserved count. |
| Message ownership and sends | One two-byte buffer and one send slot, arbitrary full-width initial `u64` buffer/send generations. Offered and accepted sends retain bytes after reader release; rejection, cancellation, partial/final completion, stale keys, zeroing on reuse and generation exhaustion preserve ownership. Scope isolation is checked separately by compiler contracts. |
| Partial send completion | One four-byte buffer at any valid length/progress, arbitrary `u32` reader count and full-width `usize` write count. Over-reported and zero-progress writes leave state unchanged; partial writes retain the send, and final completion releases only send ownership. Not a proof of async IO, queue delivery or exact descriptor binding. |
| Borrowed request during output reuse | Two one-byte buffers, arbitrary request/output contents and a live request view. Sending, completing and recycling the other buffer cannot overwrite the request. The production borrow cells execute in the proof; suspension and generated parameter readers are tested separately in the portable fixture. |
| Mutable result during output reuse | Two one-byte buffers with arbitrary values. A live writer-bound view survives completion and reuse of another message. Rust compiler contracts prohibit sealing/discarding that writer while its view is live; native tests cover forgotten-view recovery after terminal teardown. |
| Owning writable reservation | Two one-byte buffers with arbitrary values. Moving a writer into an exclusive view preserves its bytes while the other buffer changes; recovering and sealing it preserves its initialized prefix. Async IO and generated service execution are checked by Miri separately. |
| Dispatch identity across borrowed scopes | One endpoint and one task slot, with any `u64` starting sequence that permits two admissions. Rebuilding a dispatcher around the same graph cannot reuse an accepted task identity; a stale cancellation cannot consume the new owner. Capacity/exhaustion and teardown are tested natively. |
| Scoped question cleanup | One slot with cancellation, both Return/Finish acceptance orders and either no-finish hint. Rejected Finish retains the ID; the late Return and accepted cleanup are both required for reuse, and the old generation no longer resolves. |
| Reflected pipeline lifetime | One question and two local references, before/after Return with either noFinishNeeded value. Pipeline reflection requires the peer to retain the answer; the final local release emits Finish exactly when required. |
| Local tail adoption | One redirected answer, arbitrary full-width `u32` dependency count and wire ID. Overflow/rejection preserves the source; acceptance consumes adoption once and retains the producer through Finish/Return. Native tests cover cancellation and safe peer-ID reuse. |
| Incoming pipeline ownership | One answer, any wire ID, a retained pipeline dependency, either cancellation policy and either Finish/Return order. Finish cannot cancel execution needed by that dependency; its owner remains until the pipeline lease is released. |
| Answer-ID reuse | Two answer slots and an arbitrary peer wire ID. An admitted pipeline still dispatches after early Finish; Return plus Finish detaches the old wire identity while retaining its owner. Reusing that ID cannot redirect the old lease, and releasing that lease cannot remove the new answer. |
| Retained pipeline generations | Two answer slots, every positive `u32` dependency count and arbitrary peer wire ID. Retaining a lease after Finish and ID reuse increments the original generation only; overflow preserves both answers. Releasing the copied lease cannot reclaim the reused answer. |
| Incoming call preflight | Two answer slots and every full-width parent pipeline count and ledger generation. Rejected/dropped admission preserves both; acceptance inserts the call and retains exactly one parent dependency. Capacity and wire decoding are tested separately. |
| Implicit export releases | One export and every positive published count, with two arbitrary full-width release amounts. The whole batch must fit; ignoring a failed record cannot authorize a partial batch. Rejection preserves published ownership; acceptance applies the exact sum and retains retired owners for later cleanup. |
| FIFO partial output | Two two-byte frames and every prefix completion from zero through two bytes. A rejected/partial completion cannot expose the next frame; complete output advances exactly once. Poll wakeups and physical transport delivery remain separate tests. |
| Promise input resolution | Two import slots and an arbitrary positive wire count. One Resolve and a replacement grant commit together; rejected preflight preserves the replacement owner, accepted resolution cannot repeat and the original promise count never increases. |
| Promise output resolution | One export with any positive wire count. Dropped/forgotten output remains retryable; accepted Resolve consumes its obligation exactly once, without granting another original reference. Wire binding and one-hop forwarding are outside this proof. |
| Capability graph routing | Three slots, arbitrary peer identity and prior call/reflection choices. Frozen endpoints preserve the advertised route; used reflected resolutions remain blocked until their affine barrier opens. A separate proof covers every positive `u32` reference count and rejects barrier overflow without consuming the replacement. Not a distributed ordering or arbitrary-capacity proof. |
| Growing slot bounds | Every `usize` current length, requested increase and maximum. Successful growth preserves existing indices, supplies the complete requested capacity and stays within the configured maximum; rejection requires an impossible reservation. Allocation failure and moving live graph owners are checked by native tests and Miri, not this arithmetic proof. |
| Stable message segment indices | Every `usize` index. The segment and offset identify exactly the original slot, the offset stays inside its segment, and the unrepresentable final index is rejected before overflow. An aliasing-offset mutation must fail this identity assertion. Allocation, retained-byte accounting and live buffer loans are tested natively and with 32-bit Miri, outside this arithmetic proof. |
| Imported promise graph admission | Two empty slots at arbitrary `u64` generations and an arbitrary owner byte. Rejection preserves the owner and both slots; success gives the promise a distinct immutable endpoint. No proof of the wire-to-graph adapter or arbitrary capacities. |
| Bounded task dispatch | One endpoint and two task slots in a reachable reused-slot state, two distinct owners and optional output rejection. The oldest ready call is retained and stale cancellation cannot affect its successor. The full admission/reuse history is covered by native tests; promise scheduling, fairness and wire admission are outside this proof. |
| Loopback embargo reuse | One sparse slot and any starting `u32` ID. Rejection/cancellation preserve owners, IDs never wrap and old echoes cannot detach a new waiter after storage reuse. The dispatcher must separately establish when the old route has drained. |
| Reflected local capabilities | One export and two repeated receiverHosted references at every `u32` local count. Overflow/rejection preserves the whole batch; acceptance acquires both references. |
| Reflected promised answers | One parent and one free answer slot, two repeated dependencies, every `u32` pipeline count and valid `u64` generation. New-answer insertion and all dependencies commit together or leave both records unchanged. |

Loop bounds are checked by CBMC. These are safety claims at the documented
bounds and on the recorded target; there is no distributed liveness claim.
The handoff proof concerns application policy. Cryptographic identity and
authenticated introductions remain the hosted adapter's responsibility.
The fixed-arena harness starts from aligned byte storage. An initial Word-array
harness produced zeroing counterexamples with the pinned verifier that did not
reproduce in native execution. Word-to-byte conversion and Word-backed builders
remain covered by native regression tests and 32/64-bit Miri, outside that proof.

## Replacement decisions

- **Removed `RpcIdTables.tla` and its TLC replay.** It represented two occupancy
  bits and a single high allocation, with a separate allocator model. Kani now
  exercises the actual no-alloc free heap and sparse code, including wraparound.
  Native differential traces at seven capacities, high-ID regressions, zero-alloc
  checks, compile-fail contracts and hosted destructor-reentry tests remain.
- **Removed `HandoffCounterBoundary.tla` and its offset replay.** Its five-step
  histories represented counters near `u64::MAX - 2`. The portable production
  guards now have full-width inductive proofs. Native boundary tests remain and
  the existing cargo-mutants/LLVM guard campaign follows the extracted module.
- **Retained distributed and IO models**, including authenticated handoff,
  cancellation, network ordering, stream wakeups and adaptive-controller traces.
  Local safety proofs do not cover these models' scheduler and fairness claims.

The runner makes disposable copies and injects each fault from the proof
inventory. Examples include removing handoff admission, reusing a buffer with a
pending send, completing a partial write, reviving a stale capability generation,
omitting a descriptor, exceeding a growth limit and aliasing stable buffer slots.
All copies must compile and
fail the specified assertion in the matching positive proof. A timeout, missing
report or unrelated failure cannot qualify a negative control. The working
sources are never mutated by these checks.

## CI and evidence

The single PR/main CI entrypoint calls **Verification / Formal checks**.
Its Kani job runs first, then the retained TLC/Rust-replay job. Release benchmarks
depend on successful completion of this workflow. Standalone manual and weekly
runs use the same jobs; no new overlapping push/PR trigger is added.

`kani-report` contains `README.md`, `FAILED-PROOFS.md`, the per-harness duration
graph, raw JSON checks/counterexamples, logs and source/tool identities. Timing
bars are not coverage percentages. TLC retains its separate state-count and
outcome graphs in `models-report`; Kani proofs are not added to Cargo test counts.

The hosted ownership migration remains incomplete. Shared credit, question/table
and handoff transitions, wire-reference accounting, scoped import/export ownership,
message leases, resumable sends and stream framing are portable. Generated
borrowed dispatch, owning descriptor publication, pipelining, third-party protocol
jobs and caller-polled sessions are exercised by the portable fixtures. The default
hosted capability graph and network orchestration still require integration.
The wire crate binds hosted/promise grants and Resolve obligations to their sealed frame;
its decoder and transaction are covered by native fault tests and Miri, not the
descriptor-count proofs. Framing and full fixed-builder
publication are covered by fault injection, allocation counts and Miri, outside
the listed Kani harnesses. Recursive publication and ownership transfers
are tested natively and with Miri, not proved by the arena rollback harness.
A direct Kani harness for staged list promotion exceeded the 180-second limit
even after restricting it to one scalar element and two to six buffer words.
It was not added to the qualified inventory; no checks were disabled to accept it.
List promotion, generic writes and child-pointer transfers retain native capacity
sweeps, zero-allocation checks, compiler contracts and 32/64-bit Miri coverage.

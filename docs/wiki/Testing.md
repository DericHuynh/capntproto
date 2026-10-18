# Testing

Run commands from the repository root. Run the full suite with:

```sh
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
```

This includes runtime tests, generated API compiler contracts, bounded model
checks and Rust replay, C++ comparisons and specialized verification subprocesses.
It is much heavier than a compile/smoke check. Use the relevant focused target
during development and record exactly what ran.

## Test runner

Install the pinned runner with `cargo install cargo-nextest --version 0.9.146 --locked`,
or use nextest's [prebuilt binaries](https://nexte.st/docs/installation/pre-built-binaries/).
CI installs the same version through the shared auditable setup action.
`cargo nt` is the short form of `cargo nextest run --locked --workspace`.
Nextest runs each test in a separate process, so the repository no longer sets
`RUST_TEST_THREADS=1`. The [runner configuration](../../.config/nextest.toml)
limits nested compiler and verification campaigns to two simultaneous tests;
ordinary tests run concurrently. Retries are disabled and failures do not stop
remaining tests. Use `-j 1` when diagnosing resource contention.

Nextest does not run doctests: keep the explicit `cargo test --doc` command.
Miri, cargo-careful, LLVM coverage, mutation testing, Valgrind and nested Rust
fixtures use nextest too. Fuzz engines, TLC and the C++ reference retain their own
runners; nextest executes their Rust orchestration tests. Child-process crash
injection still directly executes its crash fixture to preserve signal semantics.

CI uses JUnit for aggregate counts and failed-test links, with a distinct report
per invocation. Tests skipped by a filter are excluded from pass counts. The
experimental nextest JSON stream is used only for individual nested-test
identities, never to sum suite counters. See [README reports](README-Reports.md).

## Prerequisites

Use the pinned toolchains in [rust-toolchain.toml](../../rust-toolchain.toml) and
[quality setup](../../.github/actions/quality-setup/action.yml). The Linux full suite
needs Rust 1.97.0, the pinned nightly tools, C/C++/CMake/Ninja, Cap'n Proto headers
and compiler, libclang, Java 17, the TLC release jar, Valgrind and archive tools.
The setup action specifies exact versions and checksums. Its runner disk-cleanup
step is for disposable CI hosts; do not execute that step on a workstation.

```sh
git submodule update --init --depth 1 -- vendor/capnproto
bash scripts/setup-auditable.sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
```

Keep this PATH for subsequent Cargo commands. Set `JAVA` to the Java executable
and `TLA2TOOLS_JAR` to the pinned jar, or install it at `target/tools/tla2tools.jar`.
[Quality and benchmarks](Quality-and-Benchmarks.md) describes the remaining tool installation
and CI lanes. Platform smoke jobs do not qualify storage durability on every OS.

## Focused selections

| Change | Useful command |
| --- | --- |
| Structured replies and call ergonomics | `cargo nextest run --locked --test rpc_reply --test field_api_rpc` |
| Compile-time reply rejection contracts | `cargo nextest run --locked --test api_contracts rpc_reply::structured_rpc_reply_compile_contracts -- --exact` |
| Driver ownership and native vats | `cargo nextest run --locked --test rpc_ownership --test native_vat` |
| Admission, pipelining and forwarding | `cargo nextest run --locked --test rpc_admission --test result_pipeline --test tail_transfer --test answer_adoption` |
| Independent C++ and published Rust peers, both drivers and directions | `cargo nextest run --locked --test upstream_interop` ([scope and upstream limits](Interoperability.md)) |
| TCP, TLS/mTLS and QUIC v1 | `cargo nextest run --locked --test tcp_rpc --test secure_rpc` |
| Native transport backends | `cargo nextest run --locked --lib transport::backend_tests` |
| Upstream QUIC adapter and registry pin | `cargo nextest run --locked --lib transport:: -- --skip tlc` and `cargo nextest run --locked --test tooling pinned_native_profile -- --exact` |
| Native routes and migration | `cargo nextest run --locked --test native_multiparty --test native_pipeline_migration --test native_deployment` |
| Storage worker and component ORM | `cargo nextest run --locked --test storage_worker --test component_orm` |
| Storage recovery in child processes | `cargo nextest run --locked --test tooling storage_crashes_in_isolated_process -- --exact` |
| Rust compiler and reference comparisons | `cargo nextest run --locked -p capntproto-compiler` and `cargo nextest run --locked --test schema_compiler` |
| Optional codecs and adapters | `cargo nextest run --locked -p capntproto-compat` |
| Generated field API contracts | `cargo nextest run --locked --test tooling generated_api_compile_contracts -- --exact` |
| Allocation-free generated data views and bounded RPC tables | `cargo nextest run --locked --manifest-path verification/portable/Cargo.toml --lib` |
| RPC table capacity/ownership faults and allocation counts | `cargo nextest run --locked -p capntproto-rpc-engine` |
| Source archive and downstream use | `cargo nextest run --locked --test release source_bundle_roundtrip -- --exact` and `cargo nextest run --locked --test tooling external_consumer_default_features -- --exact` |

The structured reply compiler gate includes positive controls and exact expected
Rust diagnostics for invalid ownership transitions. Runtime tests separately
exercise late errors, published child calls, failed parameter construction and
three-party adoption. A compiler test is not evidence of network reliability.

For smaller feature builds:

```sh
cargo check --locked --no-default-features --lib
cargo nextest run --locked --no-default-features --features tls --test secure_rpc
cargo nextest run --locked --no-default-features --features quic --test secure_rpc
cargo nextest run --locked --no-default-features --features storage --test storage_worker --test component_orm
```

Root integration tests are not all feature-gated. Use these selected targets;
`--no-default-features --all-targets` is not a supported substitute.

The isolated portable fixture also tests `--features alloc`, `--features std`
and `--features unaligned,sync_reader`, preserving host/target feature separation.
CI interprets the twenty-one portable fixture tests under Miri and links its generated
views, resumable framing, bounded peer/local-ID tables and RPC transitions for `thumbv7em-none-eabi`
without a global allocator; this is a link check, not execution on a physical
board. See [portable API scope](Rust-Generator.md#allocation-free-data-views).

Framing tests cancel the async adapter at every byte boundary of a multi-segment
frame, then resume and read adjacent frames without prefetching. Fault injection
covers EOF, interrupted reads, connection failures and overreported completions
at every byte. Capacity/traversal rejection, even/odd and maximum segment tables,
empty segments and terminal error retention are checked separately. The portable
fixture exercises generated views after one-byte reads, storage reuse and all
three frame-size overflow cases on 32-bit Miri. Host allocation counters cover
successful async framing and errors with `alloc` enabled; compile-fail docs keep
live message views from overlapping buffer reuse. These are executable tests and
Miri checks, not Kani proofs of framing.

Fixed-builder tests sweep every capacity from one to eighteen words with blob
lengths 0–128, checking exact-fit reservations and unchanged messages/cursors on
failure. Generated-schema tests cover defaults, typed scalar descriptors, nested
structs, successful union transitions and rejected publication at exhaustion.
They compare encoded bytes with the conventional generated builder and exercise
reset/teardown over initially dirty storage. Allocation counters include failure
and schema-upgrade rejection with `alloc` enabled. External compiler contracts
reject mismatched schema descriptors, access to the legacy editor, ignored growth
results and reuse while a reader or payload borrow is live. Miri runs the bounded
construction fixture on both 64- and 32-bit targets.

List tests sweep capacities 1–12 and lengths 0–65 for each wire element size.
Generated tests compare nested-list/group bytes against the hosted encoder and
preserve unknown enum ordinals. Recursive copying is retried at every capacity
boundary, including malformed later children, cycles and unsupported capabilities.
Failures restore both the published object and reservation cursor; an injected
unwind exercises rollback too. Struct upgrades preserve unknown fields and child
addresses. These portable scenarios run on 32/64-bit Miri. Allocation
counters additionally copy multi-segment sources through far pointers with
`alloc` enabled. Compiler contracts prevent legacy editors escaping through
list elements or groups.

List upgrades sweep capacity boundaries for inline and pointer-list encodings;
they preserve unknown fields and original child addresses, reject bit-list
promotion and oversized lengths, and handle empty/void lists. Generic tests cover
structs, open enums, text/data, nested lists, pointer elements and AnyPointer,
including exhausted copies into an inactive union arm. Compiler contracts prevent
known generic types from arbitrary casts, legacy-editor escape and mutation while
an AnyPointer reader is live. Allocation counters exercise these operations with
`alloc` enabled; CI includes the new scenarios in its 32-bit Miri filter.

The engine's table tests compare deterministic operation traces against a map
at seven inline/sparse capacity combinations, including zero capacity and
`u32::MAX` peer IDs. Separate checks retain owners across failed admission,
replacement and removal, verify teardown/reuse, and count allocations during
success, errors and cleanup. Hosted table tests retain destructor reentry checks.

Wire-reference tests cover full-count overflow, over-release, terminal exports,
and cleanup tickets surviving output rejection. The isolated fixture runs these
transitions under 32/64-bit Miri and includes them in the allocator-free ARM link;
allocation counters and compiler contracts separately enforce zero allocations
and exclusive, single-use cleanup acceptance. Six hosted fault tests inject count
exhaustion into ordinary, streaming, pipeline-only and tail sends, bootstrap,
promise-wrapped capabilities and result serialization. They check rollback of
new/repeated exports, unchanged import aliases and actual framed Release/exception
messages. Kani checks the production count transitions and a release-commit
mutation; it does not prove the hosted serializer or network delivery.

Bounded import ownership tests distinguish local clones from received wire grants,
inject both count limits and generation exhaustion, and retain pending cleanup
under output backpressure. Re-admission/reuse cannot revive old work keys; other
live slots retain their owners. Capacity/duplicate failures return the rejected
owner unchanged; teardown and injected driver unwind release retained owners.
A zero-allocation loop repeatedly reuses storage. The portable fixture exercises
the same ownership transitions under native feature profiles, 32/64-bit Miri and
the allocator-free ARM link. One positive and thirteen negative compiler cases
check scope isolation, token forgery/escape, duplicate release/commit and borrowed
views. Kani covers one-slot lifecycle and full-width count admission, with a
mutation that deliberately omits the revival generation change.

Bounded export tests inject failed publication at every position of a batch with
new and repeated grants. They check local leases across final peer release,
checked reference/generation limits, stale keys on reuse, forgotten batches,
rejected storage and driver/serialization unwind. Destructors run only after an
owner is detached or on terminal scope teardown. Native allocation counters cover
repeated multi-slot publication/reuse. The portable fixture exchanges descriptor
and Release metadata between bounded import/export tables, retaining an active
server lease after the client's final release; it runs in all feature profiles,
32/64-bit Miri and the ARM link. One positive and fifteen negative compiler cases
check lease/key scope, import/export key separation and exclusive single-use
publication. Kani checks a one-slot lifecycle and arbitrary full-width valid
staged/published counts. Mutations prematurely reclaim a leased owner and publish
an aborted export. Serialization of the exact reserved descriptors and actual IO
acceptance/delivery are outside these ownership tests.

Message-pool tests retain immutable bytes independently for readers and sends,
exercise every partial-write boundary, reject stale completion keys, and check
capacity/count/generation exhaustion, zero-length messages and terminal unwind.
Repeated publication/reuse is allocation-counted. Futures adapter tests cancel
at every byte, resume the exact suffix and inject interruptions, reset, zero
writes and invalid counts. They check explicit polling budgets, self-wakes,
retained readers, zero allocations and that byte completion does not flush or
close the stream. These tests do not simulate remote receipt or RPC completion.

The optional growing message provider is checked with live buffer views across
segment boundaries, independent count/size/retained-byte limits, free-buffer
growth, canceled async tasks, forgotten views and zero-allocation warm reuse.
FIFO tests grow a wrapped queue while its oldest frame is partially written;
an isolated missing-rotation fault must violate the expected bytes and order.
The growing portable profile replays driver conversations with the same limits
using growing buffers, sends and queues. Miri runs these ownership paths on
32-bit targets; fixed profiles retain their zero-allocation assertion.

Adopted-question replay also starts with zero sparse entries. The same limits
reject duplicate, invalid and excess IDs, preserve canceled questions until
their late Return, and keep ordinary questions independent. Owned completion
replay covers typed calls, streaming acknowledgments, queued pipelines, returned
capabilities, shutdown and abandonment after delivery. Conversion from shared
endpoints allocates nothing and preserves registered wakes and non-static owners;
native tests check once-only destruction and release of the slot allocation.
The Kani completion proofs cover the common state transitions in fixed storage;
shared allocation ownership is checked separately by native tests and Miri.

The hosted `scoped::ClientHooks` bridge has native and 32-bit Miri regressions
for generated calls through the engine dispatcher, queue limits and FIFO retry,
capability parameters/results, early pipelines followed by parent failure,
unpolled cancellation, connection shutdown, reentrant wakes and final-reference
reclamation. Tests reject foreign/opaque hook identity, retain pending promise
waiters, distinguish imported routability from settlement, and wait for reflected
barriers. Wire bridge regressions exercise dynamic payloads and null capability
slots, early/late pipelines, remote exceptions, rejected admission, tracked
cancellation and synchronous pipeline-drop wakes. The existing Kani frozen-route
proof also asserts readiness before and after the reflected barrier.
Server-adapter tests retain parameters after their original frame is released,
recover from hook-capacity exhaustion, and preserve the caller's result context.
Additional regressions cover invalid late projections and required completion
for redirected calls. Zero-inline-capacity growing-driver tests exercise large
capability tables, outgoing transforms, received Returns, result drafts, frozen
results, Join/Provide/loopback paths and recovery after capacity rejection. The
owned hook bridge also copies larger capability tables in both directions.
Allocator counters verify fixed capacities remain allocation-free with feature
unification. Oversized allocation requests preserve the original buffer; an
identity-map Kani harness checks the full integer domain's bound condition. A deliberate readiness fault (treating a routable import
as settled) is detected by the pending-promise regression.
A streaming regression first reproduced ordinary calls overtaking an
unpolled streaming send; hosted requests now admit both at `send()` time. These
checks cover scoped clients and wire admission; default `RpcSystem` migration
requires separate qualification.

The portable fixture's message roundtrip writes a real fixed-buffer generated
record, streams the frame
one byte at a time through `FrameReceiver` and reads the retained original after
send completion. This roundtrip also runs under 32/64-bit Miri and links into the
allocator-free ARM executable; the futures adapter is tested natively. One
positive and eighteen negative compiler cases enforce pool scope, exclusive
writers/windows, view lifetimes and consuming release/accept/completion APIs.
Two Kani proofs cover bounded buffer/send lifecycles and full-width write-count
validation. Mutations reclaim a buffer with a pending send and finish a send on
a partial write. The exact proof bounds are in [Formal verification](Formal-Verification.md).

RPC publication tests decode real framed Call/Return capability tables and check
exact new/repeated grant counts, missing/extra/unknown IDs, none tombstones,
unsupported descriptor kinds, attached FDs and invalid no-Finish returns. They
reject every truncated frame, trailing bytes, invalid roots, exhausted traversal,
insufficient scratch and full send storage. Rejection/unwind preserves existing
counts and unrelated readers; forgotten prepared values remain unpublished, and
failure after acceptance does not roll back a possibly observed grant. Repeated
acceptance and rejection is allocation-counted even when core `alloc` is enabled.

One positive and nine negative compiler cases enforce immutable validated bytes,
single-use acceptance, exclusive pool/table access and private send/batch state.
The twenty-first portable test sends a generated Return table one byte at a time
and imports its decoded grants; matching release counts preserve a live server
lease. It runs in the feature matrix, 32/64-bit Miri and the ARM link check. Kani
proves the engine's descriptor counter at documented bounds, with a mutation
that ignores a missing descriptor. Wire decoding and publication composition are
tested natively and under Miri; they are outside those Kani proofs. The hosted
runtime uses the same schema types; its existing protocol tests remain required.

Local tables replay 8,192 operations at each of seven capacity combinations,
compare lowest-free-ID allocation with a reference map, and inject growth failure.
Separate cases exercise arbitrary free-heap orders, duplicate removals, high-ID
wraparound with live entries, adopted-range rejection and cursor rollback on
exhaustion. Compile contracts keep peer IDs out of local allocators and restrict
high/adopted operations to question tables.

Application handoff admission, drain reservation, embargo and revocation guards
also live in the allocation-free engine; the hosted API reexports that exact
implementation. The portable fixture executes it under Miri and links it on ARM.

Run the pinned [Kani proofs](Formal-Verification.md) with
`cargo run --locked -p capntproto-dev -- kani`. The command checks the proof
inventory and uses deliberate production-code mutations as negative controls.
It writes source-bound JSON, counterexamples, failure links and a per-harness
time graph. These bounded harnesses are optional local checks. CI runs the expanded Verus
source-contract and mutation inventory before the retained TLA+ model job.

## Models, simulation and evidence

[models.json](../../test-support/verification/models.json) owns the active bounded
model catalog, corpus hashes, negative controls and scope. Discover focused
model checks with `cargo nextest list --locked --test protocol_models`.
Tests may run TLC and replay real implementation operations; read each model's
bounds and fairness assumptions. One prefix per graph edge does not cover all
histories or executor schedules.

Native packet simulations retain real TLS handshake entropy. Logs and selected
schedules support diagnostics; retired handshake simulators are not TLS evidence.
The [conformance guide](Model-Conformance.md) separates composed-model limits from
implemented RPC features. Historical, per-feature verification notes are retained
in [Testing history](../archive/Testing-History.md), not maintained as setup instructions.

[ignored-tests.json](../../quality/ignored-tests.json) is the current allowlist.
Some ignored negative controls and crash tests are invoked by ordinary parent
gates. Full composed exploration and clean release qualification are explicit:

```sh
cargo nextest run --locked --test protocol_models tlc_protocol_reference -- --ignored --exact
cargo nextest run --locked --test release isolated_release_qualification -- --ignored --exact
```

Do not run every ignored test indiscriminately: intentional negative controls
have different success criteria. Reports belong under `target/`; archived
research data records its original source and environment. Old pass counts do
not qualify changed source. See [release acceptance](Release-Acceptance.md).

## Documentation checks

```sh
cargo run --locked -p capntproto-dev -- wiki check
cargo nextest run --locked -p capntproto-dev --test wiki
cargo run --locked -p capntproto-dev -- wiki build --output target/wiki
```

The check validates local links, anchors, navigation and the documentation
inventory. The export rewrites wiki/source links without changing maintained
pages. CI uploads the export for review. See [Wiki maintenance](Wiki-Maintenance.md).

## AFL++ and IJON

The dedicated [fuzz workflow](../../.github/workflows/verification-fuzz.yml) runs
both engines. Existing libFuzzer targets retain ASan and their full seed matrices.
Pinned `afl` / `cargo-afl` **0.18.2** bundles AFL++ **4.40c**. Four targets share
those same framing, pointer, schema and RPC lifecycle oracles. The two Native
crypto/clock targets remain in libFuzzer because they do not provide deterministic
persistent inputs. Neither engine enables Quiche's crypto bypass.

```sh
cargo install cargo-afl --version 0.18.2 --locked
AFL_NO_CFG_FUZZING=1 cargo afl build --locked --manifest-path fuzz/Cargo.toml \
  --no-default-features --features afl-targets --bins --target-dir target/afl-build
cargo run --locked --manifest-path fuzz/Cargo.toml --no-default-features \
  --example afl_seeds -- target/afl-corpus
cargo run --locked -p capntproto-dev -- afl-fuzz --binaries target/afl-build/debug \
  --corpus target/afl-corpus --output target/afl-results --seconds 120
```

Use a fresh output directory for each campaign. The CI job runs `cargo afl
system-config` only on its disposable hosted Linux runner; this root-level system
tuning is an explicit local administrator decision. See the [Rust Fuzz Book](https://rust-fuzz.github.io/book/afl.html)
and [AFL++ IJON documentation](https://github.com/AFLplusplus/AFLplusplus/blob/stable/docs/IJON.md).

Stable Rust uses cargo-afl's sanitizer-coverage instrumentation and comparison
tracing; this job does not build the optional nightly LLVM CMPLOG plugins.
`-c -` records that choice explicitly. AFL targets have debug assertions and
overflow checks; ASan qualification comes from the separate libFuzzer campaigns.

The RPC IJON observer reports bounded combinations of live capabilities, running
calls, outstanding promises and pending questions. Two max-value channels reward
actual completions and oracle effects. It never rewards input length or raw IDs.
All per-input RPC state is constructed and dropped inside the existing oracle;
immutable fixture caches are warmed before the forkserver. Persistent processes
restart after 1,000 inputs. Shared-oracle tests compare repeated feedback and
results; instability percentages are retained, not hidden.

The AFL++ runtime's IJON flag must be supplied explicitly for the stable sancov
build. CI requires its **Using IJON feature** handshake; merely linking annotation
calls is insufficient. Findings, seed corpus, queue, raw `fuzzer_stats`, logs,
IJON max-input corpus and graph data are retained in `fuzz-report`. Any saved
crash or hang fails CI even if AFL exits successfully. Replay a retained input
with the existing shared-oracle entry point:

```sh
CAPNTPROTO_NATIVE_FUZZ_TARGET=rpc_lifecycle \
CAPNTPROTO_NATIVE_FUZZ_REPLAY=target/afl-results/rpc_lifecycle/default/queue/INPUT \
cargo nextest run --locked --manifest-path fuzz/Cargo.toml --no-default-features \
  --lib tests::replay_saved_fuzz_input -- --exact
```

## Unsafe-code review gates

The application crate denies undocumented unsafe blocks, missing public safety
contracts and implicit unsafe operations inside unsafe functions. Its libc and
mmap calls state descriptor ownership, buffer bounds and mapped-file lifetime
requirements at each boundary. Compiler, compatibility, RPC, codegen and futures
implementations forbid unsafe code (generated source strings are not executed
by the generator). Test assertions may panic; production Unix `FdReader::new`
uses an explicit assertion that its constant buffer size exceeds the minimum.

CI also runs `cargo run --locked -p capntproto-dev -- check-unsafe` on Linux with the pinned Clippy.
It records all three unsafe lints, including default workspace test targets, in
`target/quality/unsafe/`. Pre-existing debt in the owned core runtime is recorded
in `quality/unsafe-baseline.json`, with per-lint counts and whole-file hashes.
This includes inherited wire-layout code and is **not a safety audit or approval**.
New diagnostics, removed diagnostics, changes to those files, and attempts to
baseline application code fail the gate. Document new/changed unsafe code and
review any reduction to this baseline; do not regenerate it to dismiss a failure.
Feature-specific, non-Linux and generated code still require their respective
review/testing; this default-feature Linux inventory does not claim to cover them.

The unsafe [function-body lint](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html)
and Clippy's [unsafe-block documentation lint](https://rust-lang.github.io/rust-clippy/master/index.html#undocumented_unsafe_blocks)
check obligations and explanation presence, not their correctness. Miri, fuzzing,
bounds/aliasing tests and code review remain necessary for the pointer-heavy
serialization runtime. Its layout implementation stays together during this
maintenance change; RPC dispatch/capability handling and schema node generation
are split into focused modules without altering message layout.

The October 14 cached-reader review refreshes the hashes for `message.rs`,
`private/arena.rs` and `private/layout.rs` without increasing their existing
unsafe-lint counts. New cache borrows retain the original segment owner, retain
only offsets across moves, check all segment identities when transferring a
selection, and recheck the selected word's extent before borrowing it. Relative
offsets are widened before arithmetic on 32-bit targets; nonzero segments use
the original arena checks. The data-slice helpers now state their readable or
exclusive writable lifetime contracts and use explicit unsafe blocks, removing
the two `unsafe_op_in_unsafe_fn` diagnostics in `data.rs`. The 32-bit Miri lane
includes the selected-reader and immediate-promise ownership regressions. These
changes do not reclassify inherited unsafe documentation debt as verified code.

The memory qualification inventory now includes 33 ownership tests and 14 wire,
orphan, generated-API and schema tests. The workflow runs all 47 with aligned and unaligned
storage under both Stacked Borrows and Tree Borrows: 188 interpreted executions,
plus a deliberate freed-pointer control that must fail. The pinned inventory
was updated for the selected-reader regressions, including the two owning-cache tests; no test was filtered
out to accommodate the count change.

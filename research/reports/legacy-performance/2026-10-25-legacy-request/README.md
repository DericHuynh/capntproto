# Concrete request ownership without a box

Request<P,R,H> can now retain caller-backed parameters and publication in the
actual legacy capability API. H implements PayloadStorage<P> for fallible editing
and RequestStorage<R> to consume the original owner. The provider chooses its
completion/pipeline type, so no boxed future or allocating sender is imposed.
PreparedRequest<H,F> composes concrete storage and a one-shot publisher without
raw pointer erasure or new unsafe code. Parameters and results share
PayloadStorage<T>; the previous ResultStorage name is removed without an alias.

The default hosted Request remains the same two fields with a Box<dyn RequestHook>.
Its inline send body is transferred unchanged to that owner's RequestStorage
implementation. Editing and publication are separate bounds: send does not add a
new Params:Owned constraint. This is a storage boundary refactor, not a claim that
the remaining boxed ClientHook/PipelineHook or allocating connection records have
been replaced. Whole legacy no-allocator completion remains pending.

Six tests in capntproto-rpc-wire exercise original-buffer transfer through the
engine's borrowed completion slot, closed receiver rejection retaining bytes and
capacity, a RefMut guard released before the unsent publisher's captured drop,
an intact Call header after failed parameter growth, exhausted storage without
publication, and a real cancellation Waker observing a released MutexGuard.
All request/slot/edit/cancellation paths are measured at zero blocks/bytes. The
standard Arc/Waker setup in the last test occurs outside the measured region.
No always-poll executor or replacement bounded protocol is used as evidence.

Local checks: 44 core no-default and 115 wire no-default tests pass, nine actual
alloc-only legacy conversations pass, and core library / wire all-target Clippy
passes. The Request+Call ownership probe executes on the host and links for M4
without allocator or undefined symbols. The M4 link is not executed on a board,
and does not instantiate a complete RPC connection. embedded-link.json records
its exact scope and hashes. Hosted compiled work and two-width Miri require the
separate immutable qualification; native latency must use dedicated droplets.

Reproduce the meaningful local subset:

    cargo test -p capntproto-core --no-default-features --lib
    cargo test -p capntproto-rpc-wire --no-default-features --lib
    cargo test --manifest-path verification/legacy-alloc/Cargo.toml
    cargo clippy -p capntproto-core --no-default-features --lib -- -D warnings
    cargo clippy -p capntproto-rpc-wire --no-default-features --all-targets -- -D warnings

verification/portable/legacy-request-link.rs is the standalone ownership probe.
It builds a real Call envelope and transfers its fixed content owner through the
same shared completion primitive. It is not the portable protocol driver.


The immutable request qualification is `60f95b1937de55a45d87736e5d9e1f74b3f42e49`,
[run 40180030960](https://github.com/DericHuynh/capntproto/actions/runs/40180030960).
Its strict-provenance Miri job passes all six ownership tests in no-default and
explicit alloc profiles on 64-bit and 32-bit targets (24 executions). Hosted
profiles remain running; their result is not inferred from Miri success.

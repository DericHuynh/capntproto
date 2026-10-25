# Legacy without std, with an allocator

This isolated no_std fixture instantiates the actual legacy `capnp_rpc::RpcSystem`
and generated owned service API with `default-features = false, features = ["alloc"]`.
Its dependencies do not acquire std through the application workspace or a test executor.
Application-owned message queues implement `VatNetwork`/`Connection`; these queues and
the legacy runtime allocate. This does not prove allocation-free RPC or encrypted IO.

Run `cargo test --locked --manifest-path verification/legacy-alloc/Cargo.toml`.
The five tests cover retained responses, depth-32 dependent reads and pipelines,
32 concurrent calls, reverse callbacks, cancellation that destroys a suspended
service future and leaves the connection live, and local identity/revocation in
an explicit shared `ClientRegistry`. The test executor only polls and supplies a
standard noop Waker; it does not depend on futures' std executor.

Run `cargo check --locked --manifest-path verification/legacy-alloc/Cargo.toml --target thumbv7em-none-eabi`
to check the same fixture without an OS. This is compilation evidence, not board execution
or an allocator-free ELF link. M0 remains supported by the separate core-only profile;
the allocating runtimes require pointer-width atomics. The schema build uses capnp
and accepts `CAPNP_INCLUDE_DIR` for the standard annotation schema.

# Browser WASI Preview 3 frontend

This is a Rust component with native async WASI Preview 3 bindings, not a
wasm-bindgen RPC client. `wit/frontend.wit` describes its async export and its
small browser WebTransport host interface. WASI monotonic clocks are imported at
version 0.3.0. The Rust guest performs capability RPC against the shared greeting
schema and pipelines a session call.

The browser owns WebTransport, QUIC, TLS validation and certificate pins. The
JavaScript host only adapts byte IO and the monotonic clock; RPC stays in Rust.
The component uses bounded portable RPC tables and message storage. After both
replies are checked, it cancels its connection scope and releases the browser
session. This is scoped cancellation rather than a promise of graceful draining.

Run the tested backend/frontend pair with:

```sh
cargo run --locked -p capntproto-dev -- examples --browser-only
```

Install the prerequisites in [the integration guide](../integration/README.md).
The runner compiles a Rust core module for `wasm32-unknown-unknown`, wraps its
embedded component metadata using jco, and executes the resulting component in
Chromium with JSPI. This avoids requiring a tier-3 `wasm32-wasip3` sysroot.
The suite checks the emitted component's native async export and WASI imports.
Use this local-development certificate pinning setup only with the generated
short-lived credentials; production browsers should use a trusted certificate.

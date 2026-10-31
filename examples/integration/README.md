# Example integration tests

The same entry point runs locally and in the required CI examples job:

```sh
rustup target add wasm32-unknown-unknown thumbv7em-none-eabi
npm ci --prefix examples/integration
npm exec --prefix examples/integration -- playwright install --with-deps chromium
cargo run --locked -p capntproto-dev -- examples
```

Requires Rust 1.97, Node 24 and Chromium with JavaScript Promise Integration
(Chrome 137 or later). Native builds need the repository's usual dependencies.
Use `--native-only` or `--browser-only` after `examples` to select a subset.
`PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` selects an installed Chromium executable.
`CARGO_TARGET_DIR` selects the shared build directory.

The suite runs the distributed checkout crash/race tests against legacy, portable
and mixed drivers; the TCP, TLS, mutual TLS and WebTransport runtime/driver matrix;
the embedded TLS host tests and ARM no-std/Embassy checks; and a real browser
running the WASI Preview 3 component against separate backend processes. Browser
checks cover pipelining, concurrent sessions, Unicode payloads, certificate pins,
origin/path denial and recovery. All processes and temporary credentials are
cleaned up on success, failure and signals. A failed check exits nonzero.

`node --test examples/integration/process.test.mjs` tests the process harness.
No hardware board is required: the device TLS function runs against a host TLS
server, and the board-facing implementation is cross-compiled.

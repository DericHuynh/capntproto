# capntproto-futures

Asynchronous framing, buffered and scratch reads, and ordered write queues. This is a maintained first-party workspace crate derived from
[`capnp-futures`](https://github.com/capnproto/capnproto-rust), with the original MIT
[license](LICENSE) and [upstream changelog](CHANGELOG.md) preserved.
The upstream starting point and source hashes are recorded in
[provenance](../../vendor/provenance/capnp-futures-revision.json).

The Cargo package is `capntproto-futures`. Its Rust library name remains `capnp_futures`
to preserve compatibility with generated bindings. Use a package alias when
selecting it directly; the root `capntproto` crate already selects this coordinated set.
All maintained crates share the root lockfile and are tested with
`cargo nextest run --workspace`. See the [workspace policy](../../docs/wiki/Fork-Policy.md).

`BufferedRead::try_read_message_with_scratch` supports retained buffered messages.
Here scratch includes the segment table as well as the payload. Short-lived
messages still share the receive buffer; direct reads and insufficient scratch
use owned storage, matching the pinned C++ buffered reader. The returned
`BufferedScratchSegments` reports the selected storage and can be converted to
owned segments. Partial reads survive cancellation inside the stream and leave
caller scratch untouched. See [buffered input](src/buffered_read.rs).

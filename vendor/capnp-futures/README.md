[![crates.io](https://img.shields.io/crates/v/capnp-futures.svg)](https://crates.io/crates/capnp-futures)

[documentation](https://docs.rs/capnp-futures/)

Asynchronous reading and writing of Cap'n Proto messages in Rust.


This ReProto fork also provides `serialize::read_message_with_scratch` and
`serialize::try_read_message_with_scratch`. They reuse caller-owned word buffers
when the payload fits and fall back to owned storage otherwise. The returned
reader borrows the scratch buffer; segment metadata still allocates. See the
[API documentation in source](src/serialize.rs) for cancellation and lifetime rules.

`BufferedRead::try_read_message_with_scratch` supports retained buffered messages.
Here scratch includes the segment table as well as the payload. Short-lived
messages still share the receive buffer; direct reads and insufficient scratch
use owned storage, matching the pinned C++ buffered reader. The returned
`BufferedScratchSegments` reports the selected storage and can be converted to
owned segments. Partial reads survive cancellation inside the stream and leave
caller scratch untouched. See [buffered input](src/buffered_read.rs).
